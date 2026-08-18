import type { Context } from '@deepseek-ai/cordis'
import Schema from '@deepseek-ai/schemastery'
import type { AgentHandle } from '@deepseek-ai/dsh-agent'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import { SessionId, type Session, type SessionEvent } from '@deepseek-ai/dsh-session'
import type {} from '@deepseek-ai/dsh-host-webserver'
import type {} from '@deepseek-ai/dsh-user-approval'
import { randomBytes, randomUUID } from 'node:crypto'
import { execFile } from 'node:child_process'
import { chmod, lstat, mkdir, open, readFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { API_PREFIX, BRIDGE_VERSION, bearerAuthorized, contentText, errorReference, isSafeSessionId, mapSessionEvent, parseSessionRoute, type BridgeEvent } from './protocol.ts'

export const name = 'wallpaper-bridge'
export const inject = ['agents', 'webServer']

export interface Config {
  tokenFile?: string
  cwd?: string
}

export const Config: Schema<Config> = Schema.object({
  tokenFile: Schema.string(),
  cwd: Schema.string(),
}) as Schema<Config>

interface LiveSession {
  handle: AgentHandle
  clients: Set<ServerResponse>
}

// This is an HTTP boundary, so measure the actual UTF-8 payload rather than
// JavaScript UTF-16 code units. Keep it in lockstep with the native client.
const MAX_MESSAGE_BYTES = 100_000
const MAX_CWD_LENGTH = 4_096
const MAX_REQUEST_BODY_BYTES = 1_048_576
const MIN_TOKEN_LENGTH = 32
const execFileAsync = promisify(execFile)

class RequestBodyError extends Error {
  constructor(readonly status: 400 | 413, readonly code: 'invalid-request' | 'request-too-large') {
    super(code)
  }
}

function defaultTokenFile(): string {
  const root = process.env.DSH_HOME?.trim() || join(homedir(), '.dsh')
  return join(root, 'wallpaper', 'bridge-token')
}

function validateToken(token: string): string {
  if (token.length < MIN_TOKEN_LENGTH) throw new Error('bridge token is too short')
  return token
}

async function currentWindowsSid(): Promise<string> {
  const { stdout } = await execFileAsync('whoami.exe', ['/user', '/fo', 'csv', '/nh'], {
    windowsHide: true,
    timeout: 5_000,
    maxBuffer: 8_192,
  })
  // `/fo csv` quotes fields, but a SID begins at a quote boundary rather than
  // a `\b` word boundary; search the actual SID grammar directly.
  // `\d` has historically been vulnerable to host RegExp Unicode-mode
  // differences in this bundled runtime. SID syntax is ASCII by definition.
  const sid = stdout.match(/S-[0-9]+(?:-[0-9]+)+/)?.[0]
  if (!sid) throw new Error('unable to determine current Windows SID')
  return sid
}

/**
 * The bridge token is a local bearer credential. POSIX modes are sufficient
 * there; on Windows explicitly replace inherited ACLs with the current SID.
 * Failing to establish that boundary is an authentication setup failure, not
 * a reason to leave a readable token behind and continue serving requests.
 */
async function restrictTokenFile(file: string): Promise<void> {
  if (process.platform !== 'win32') {
    await chmod(file, 0o600)
    return
  }
  const sid = await currentWindowsSid()
  const absoluteFile = resolve(file)
  const options = { windowsHide: true, timeout: 5_000, maxBuffer: 8_192 }
  // Remove inherited grants and add exactly the current account's full-control
  // ACE. Do not use `/reset`: it fails for a newly-created file when its ACL
  // owner cannot be resolved in constrained environments. Arguments are passed
  // directly (never through a shell).
  await execFileAsync('icacls.exe', [absoluteFile, '/inheritance:r'], options)
  try {
    await execFileAsync('icacls.exe', [absoluteFile, '/grant:r', `*${sid}:(F)`], options)
  } catch (error) {
    // A narrow pre-existing DACL can reject `/grant:r` after inheritance is
    // removed. Re-enable inherited ACLs before failing closed so a user can
    // repair or delete the file rather than being locked out of their profile.
    await execFileAsync('icacls.exe', [absoluteFile, '/inheritance:e'], options).catch(() => undefined)
    throw error
  }
}

async function readAndRestrictToken(file: string): Promise<string> {
  // `icacls` reports a generic process failure for a missing file rather than
  // Node's ENOENT. Check first so initial bridge startup creates the token
  // instead of treating the normal first-run state as an ACL failure.
  const stat = await lstat(file)
  if (!stat.isFile() || stat.isSymbolicLink()) {
    throw new Error('bridge token path is not a regular file')
  }
  await restrictTokenFile(file)
  // Only read a pre-existing credential after its permissions are repaired.
  // Re-reading after ACL mutation also prevents an EEXIST winner from leaving
  // us using a token swapped while permissions were being established.
  return validateToken((await readFile(file, 'utf8')).trim())
}

async function ensureToken(file: string): Promise<string> {
  try {
    return await readAndRestrictToken(file)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
  }
  await mkdir(dirname(file), { recursive: true })
  const generated = randomBytes(32).toString('base64url')
  try {
    const handle = await open(file, 'wx', 0o600)
    try { await handle.writeFile(`${generated}\n`, 'utf8') } finally { await handle.close() }
    return await readAndRestrictToken(file)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
    // Another bridge instance won creation. Apply exactly the same validation
    // and platform permission rules as the ordinary existing-token path.
    return await readAndRestrictToken(file)
  }
}

function json(res: ServerResponse, status: number, value: unknown): void {
  res.statusCode = status
  res.setHeader('content-type', 'application/json; charset=utf-8')
  res.setHeader('cache-control', 'no-store')
  res.end(JSON.stringify(value))
}

function malformed(res: ServerResponse, error: unknown, logger: { warn(message: string): void }): void {
  const reference = errorReference(error)
  logger.warn(`wallpaper bridge request rejected (${reference})`)
  json(res, 400, { error: 'invalid-request', reference })
}

async function readJson(req: IncomingMessage): Promise<Record<string, unknown>> {
  const contentLength = req.headers['content-length']
  const declaredLength = typeof contentLength === 'string' ? Number(contentLength) : NaN
  if (Number.isFinite(declaredLength) && declaredLength > MAX_REQUEST_BODY_BYTES) {
    req.destroy?.()
    throw new RequestBodyError(413, 'request-too-large')
  }
  const chunks: Buffer[] = []
  let length = 0
  try {
    for await (const chunk of req) {
      const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
      length += buffer.length
      if (length > MAX_REQUEST_BODY_BYTES) {
        // Stop accepting bytes immediately; a 413 is still attempted by the
        // route handler for clients whose connection remains writable.
        req.destroy?.()
        throw new RequestBodyError(413, 'request-too-large')
      }
      chunks.push(buffer)
    }
  } catch (error) {
    if (error instanceof RequestBodyError) throw error
    throw new RequestBodyError(400, 'invalid-request')
  }
  if (length === 0) return {}
  let value: unknown
  try {
    value = JSON.parse(Buffer.concat(chunks).toString('utf8'))
  } catch {
    throw new RequestBodyError(400, 'invalid-request')
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new RequestBodyError(400, 'invalid-request')
  }
  return value as Record<string, unknown>
}

function sse(res: ServerResponse, event: BridgeEvent): void {
  if (!res.writableEnded) res.write(`data: ${JSON.stringify(event)}\n\n`)
}

function initialEvents(entry: LiveSession): BridgeEvent[] {
  const { agent } = entry.handle
  const model: BridgeEvent[] = agent.options.model
    ? [{ type: 'model', model: agent.options.model, ...(agent.options.provider ? { provider: agent.options.provider } : {}) }]
    : []
  return [
    ...model,
    { type: 'status', activity: agent.status === 'running' ? 'thinking' : 'idle' },
  ]
}

function sessionSummary(live: LiveSession): Record<string, unknown> {
  const { agent } = live.handle
  return {
    sessionId: agent.session.id,
    status: agent.status,
    provider: agent.options.provider,
    model: agent.options.model,
  }
}

function messageForClient(error: unknown): string {
  // DSH errors can include provider response bodies, paths, or credentials.
  // The wallpaper only needs to know that the turn stopped; the reference is
  // enough to correlate a local host log without putting the raw failure into
  // the WebView or SSE transcript.
  const reference = errorReference(error)
  return `Harness 会话执行失败（参考 ${reference}）`
}

function approvalSummary(toolName: unknown): string {
  // `reason` is request-specific and may contain a command, path, or user
  // data. A tool name is useful enough for the hand-off UI, but constrain it
  // before it crosses the SSE boundary as well.
  const safeName = typeof toolName === 'string'
    ? toolName.trim().replace(/[^A-Za-z0-9_.:-]/g, '').slice(0, 80)
    : ''
  return safeName
    ? `工具“${safeName}”需要在 Harness 中批准`
    : '一个工具调用需要在 Harness 中批准'
}

function historyOf(session: Session): Array<Record<string, unknown>> {
  return session.deriveMessages()
    .filter((message) => message.role === 'user' || message.role === 'assistant')
    .map((message) => ({ id: message.id, role: message.role, content: contentText(message) }))
}

export function apply(ctx: Context, config: Config = {}): void {
  const live = new Map<string, LiveSession>()
  const canResume = (): boolean => {
    // Keep the lightweight unit-test harness compatible while production
    // Cordis contexts use `get()` for optional service discovery.
    const get = (ctx as Context & { get?: (name: string) => unknown }).get
    return typeof get === 'function' && get.call(ctx, 'sessionPersistence') !== undefined
  }
  let token = ''
  let tokenFailure: string | undefined
  const tokenReady = ensureToken(config.tokenFile?.trim() || defaultTokenFile())
    .then((value) => { token = value })
    .catch((error: unknown) => { tokenFailure = errorReference(error) })

  const publish = (sessionId: string, event: BridgeEvent): void => {
    const entry = live.get(sessionId)
    if (!entry) return
    for (const client of [...entry.clients]) {
      if (client.writableEnded || client.destroyed) entry.clients.delete(client)
      else sse(client, event)
    }
  }

  ctx.on('session/event', (session: Session, event: SessionEvent) => {
    const id = String(session.id)
    const entry = live.get(id)
    // Session IDs are not a unique live-agent identity: another owner can
    // create a session with the same ID. Never relay its events to this
    // bridge's subscriber merely because the string happens to match.
    if (!entry || entry.handle.agent.session !== session) return
    for (const mapped of mapSessionEvent(event)) publish(id, mapped)
  })

  ctx.on('agent/error', ({ agent, error }) => {
    const sessionId = String(agent.session.id)
    const entry = live.get(sessionId)
    // A session ID can be reused by another agent owner. Error events carry
    // the agent object, so apply the same identity check used for disposal and
    // approval events before relaying anything to the wallpaper.
    if (entry?.handle.agent !== agent) return
    publish(sessionId, {
      type: 'error',
      code: 'HARNESS_AGENT_ERROR',
      recoverable: true,
      message: messageForClient(error),
    })
  })

  ctx.on('agent/disposed', ({ agent }) => {
    const sessionId = String(agent.session.id)
    const entry = live.get(sessionId)
    if (!entry || entry.handle.agent !== agent) return
    publish(sessionId, { type: 'disconnected', recoverable: true })
    for (const client of entry.clients) client.end()
    live.delete(sessionId)
  })

  ctx.on('approval/request', (request, next) => {
    const sessionId = String(request.agent.session.id)
    const entry = live.get(sessionId)
    if (entry?.handle.agent === request.agent) {
      publish(sessionId, {
        type: 'approval-required',
        sessionId,
        summary: approvalSummary(request.toolName),
      })
    }
    return next()
  })

  ctx.effect(() => async () => {
    const disposals: Promise<void>[] = []
    for (const entry of live.values()) {
      for (const client of entry.clients) client.end()
      disposals.push(entry.handle.dispose())
    }
    live.clear()
    await Promise.allSettled(disposals)
  })

  ctx.inject(['webServer'], (wctx) => {
    if (wctx.webServer.host !== '127.0.0.1') {
      throw new Error('dsh-wallpaper-bridge refuses to run on a non-loopback WebServer')
    }

    const statusDispose = wctx.webServer.register({
      kind: 'exact',
      path: `${API_PREFIX}/status`,
      handler: async (_req, res) => {
        await tokenReady
        json(res, 200, {
          bridgeVersion: BRIDGE_VERSION,
          protocolVersion: 1,
          dsh: 'online',
          capabilities: [
            'sessions',
            ...(canResume() ? ['resume'] : []),
            'history',
            'sse',
            'cancel',
            'approval-handoff',
          ],
          authentication: tokenFailure === undefined ? 'ready' : 'unavailable',
          ...(tokenFailure === undefined ? {} : { tokenReference: tokenFailure }),
        })
      },
    })

    const sessionsDispose = wctx.webServer.register({
      kind: 'prefix',
      path: `${API_PREFIX}/sessions`,
      handler: async (req, res) => {
        await tokenReady
        if (tokenFailure !== undefined) return json(res, 503, { error: 'bridge-token-unavailable', reference: tokenFailure })
        if (!bearerAuthorized(req.headers.authorization, token)) return json(res, 401, { error: 'unauthorized' })
        const url = new URL(req.url ?? '/', 'http://127.0.0.1')
        const route = parseSessionRoute(url.pathname)
        if (!route) return json(res, 404, { error: 'not-found' })
        try {
          if (route.kind === 'collection') {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            const body = await readJson(req)
            const requested = typeof body.sessionId === 'string' ? body.sessionId.trim() : ''
            const resume = typeof body.resumeSessionId === 'string' ? body.resumeSessionId.trim() : ''
            if (resume && !canResume()) return json(res, 409, { error: 'resume-unavailable' })
            const id = resume || requested || `wallpaper-${randomUUID()}`
            if (!isSafeSessionId(id)) return json(res, 400, { error: 'invalid-session-id' })
            const existing = live.get(id)
            if (existing) return json(res, 200, sessionSummary(existing))
            const provider = typeof body.provider === 'string' && body.provider.trim() ? body.provider.trim() : undefined
            const model = typeof body.model === 'string' && body.model.trim() ? body.model.trim() : undefined
            const agentOptions = provider || model ? { provider, model } : undefined
            const cwd = typeof body.cwd === 'string' && body.cwd.trim() ? body.cwd.trim() : config.cwd
            if (cwd && cwd.length > MAX_CWD_LENGTH) return json(res, 400, { error: 'invalid-cwd' })
            const handle = resume
              ? await wctx.agents.resume({ resumeSessionId: SessionId(id), agentOptions })
              : await wctx.agents.create({
                  sessionId: SessionId(id),
                  meta: { cwd },
                  agentOptions,
                })
            const entry = { handle, clients: new Set<ServerResponse>() }
            live.set(id, entry)
            return json(res, 201, sessionSummary(entry))
          }

          const entry = live.get(route.sessionId)
          if (!entry) return json(res, 404, { error: 'session-not-live', sessionId: route.sessionId })

          if (route.kind === 'history') {
            if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
            return json(res, 200, { sessionId: route.sessionId, messages: historyOf(entry.handle.agent.session) })
          }
          if (route.kind === 'events') {
            if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
            res.statusCode = 200
            res.setHeader('content-type', 'text/event-stream; charset=utf-8')
            res.setHeader('cache-control', 'no-store')
            res.setHeader('connection', 'keep-alive')
            // Subscribe before returning any data so a turn submitted during
            // connection setup cannot race past the client. The snapshot then
            // brings a newly attached reader up to the current state.
            entry.clients.add(res)
            // The native client treats this header as a connection-ready
            // acknowledgement. It is set only after `clients.add`, so a
            // successful response proves the bridge will observe the first
            // followup submitted after `connect_harness` returns.
            res.setHeader('x-dsh-wallpaper-sse-ready', '1')
            for (const event of initialEvents(entry)) sse(res, event)
            res.flushHeaders()
            const heartbeat = setInterval(() => { if (!res.writableEnded) res.write(': heartbeat\n\n') }, 15_000)
            req.on('close', () => { clearInterval(heartbeat); entry.clients.delete(res) })
            return
          }
          if (route.kind === 'messages') {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            const body = await readJson(req)
            const text = typeof body.text === 'string' ? body.text.trim() : ''
            if (!text) return json(res, 400, { error: 'text-required' })
            if (Buffer.byteLength(text, 'utf8') > MAX_MESSAGE_BYTES) return json(res, 413, { error: 'text-too-large' })
            entry.handle.agent.followup(createUserMessage({ content: [{ type: 'text', text }], source: { kind: 'user' } }))
            return json(res, 202, { accepted: true, sessionId: route.sessionId })
          }
          if (route.kind === 'cancel') {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            entry.handle.agent.cancel({ kind: 'user' })
            publish(route.sessionId, { type: 'status', activity: 'idle' })
            return json(res, 202, { cancelled: true, sessionId: route.sessionId })
          }
        } catch (error) {
          if (error instanceof RequestBodyError) {
            if (error.status === 400) malformed(res, error, wctx.logger)
            else json(res, error.status, { error: error.code })
            return
          }
          if (error instanceof SyntaxError || error instanceof RangeError) return malformed(res, error, wctx.logger)
          const reference = errorReference(error)
          wctx.logger.warn(`wallpaper bridge request failed (${reference})`)
          return json(res, 500, { error: 'bridge-error', reference })
        }
      },
    })
    wctx.effect(() => () => { statusDispose(); sessionsDispose() })
  })
}

export default apply
