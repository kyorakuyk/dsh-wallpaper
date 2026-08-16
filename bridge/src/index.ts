import type { Context } from '@deepseek-ai/cordis'
import Schema from '@deepseek-ai/schemastery'
import type { AgentHandle } from '@deepseek-ai/dsh-agent'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import { SessionId, type Session, type SessionEvent } from '@deepseek-ai/dsh-session'
import type {} from '@deepseek-ai/dsh-host-webserver'
import type {} from '@deepseek-ai/dsh-user-approval'
import { createHash, randomBytes, randomUUID } from 'node:crypto'
import { mkdir, open, readFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { API_PREFIX, BRIDGE_VERSION, bearerAuthorized, contentText, mapSessionEvent, parseSessionRoute, type BridgeEvent } from './protocol.ts'

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

function defaultTokenFile(): string {
  const root = process.env.DSH_HOME?.trim() || join(homedir(), '.dsh')
  return join(root, 'wallpaper', 'bridge-token')
}

async function ensureToken(file: string): Promise<string> {
  try {
    const token = (await readFile(file, 'utf8')).trim()
    if (token.length >= 32) return token
    throw new Error('bridge token is too short')
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
  }
  await mkdir(dirname(file), { recursive: true })
  const generated = randomBytes(32).toString('base64url')
  try {
    const handle = await open(file, 'wx', 0o600)
    try { await handle.writeFile(`${generated}\n`, 'utf8') } finally { await handle.close() }
    return generated
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
    return (await readFile(file, 'utf8')).trim()
  }
}

function json(res: ServerResponse, status: number, value: unknown): void {
  res.statusCode = status
  res.setHeader('content-type', 'application/json; charset=utf-8')
  res.setHeader('cache-control', 'no-store')
  res.end(JSON.stringify(value))
}

async function readJson(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = []
  let length = 0
  for await (const chunk of req) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
    length += buffer.length
    if (length > 1_048_576) throw new Error('request body exceeds 1 MiB')
    chunks.push(buffer)
  }
  if (length === 0) return {}
  const value: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'))
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('JSON object required')
  return value as Record<string, unknown>
}

function sse(res: ServerResponse, event: BridgeEvent): void {
  if (!res.writableEnded) res.write(`data: ${JSON.stringify(event)}\n\n`)
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

function historyOf(session: Session): Array<Record<string, unknown>> {
  return session.deriveMessages()
    .filter((message) => message.role === 'user' || message.role === 'assistant')
    .map((message) => ({ id: message.id, role: message.role, content: contentText(message) }))
}

export function apply(ctx: Context, config: Config = {}): void {
  const live = new Map<string, LiveSession>()
  let token = ''
  const tokenReady = ensureToken(config.tokenFile?.trim() || defaultTokenFile()).then((value) => { token = value })

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
    if (!live.has(id)) return
    for (const mapped of mapSessionEvent(event)) publish(id, mapped)
  })

  ctx.on('approval/request', (request, next) => {
    const sessionId = String(request.agent.session.id)
    if (live.has(sessionId)) {
      publish(sessionId, {
        type: 'approval-required',
        sessionId,
        summary: request.reason?.trim() || `工具 ${request.toolName} 需要用户批准`,
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
        const active = [...live.values()].at(-1)
        json(res, 200, {
          bridgeVersion: BRIDGE_VERSION,
          protocolVersion: 1,
          dsh: 'online',
          capabilities: ['sessions', 'resume', 'history', 'sse', 'cancel', 'approval-handoff'],
          ...(active ? sessionSummary(active) : {}),
        })
      },
    })

    const sessionsDispose = wctx.webServer.register({
      kind: 'prefix',
      path: `${API_PREFIX}/sessions`,
      handler: async (req, res) => {
        await tokenReady
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
            const id = resume || requested || `wallpaper-${randomUUID()}`
            const existing = live.get(id)
            if (existing) return json(res, 200, sessionSummary(existing))
            const provider = typeof body.provider === 'string' && body.provider.trim() ? body.provider.trim() : undefined
            const model = typeof body.model === 'string' && body.model.trim() ? body.model.trim() : undefined
            const agentOptions = provider || model ? { provider, model } : undefined
            const handle = resume
              ? await wctx.agents.resume({ resumeSessionId: SessionId(id), agentOptions })
              : await wctx.agents.create({
                  sessionId: SessionId(id),
                  meta: { cwd: typeof body.cwd === 'string' ? body.cwd : config.cwd },
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
            res.flushHeaders()
            entry.clients.add(res)
            sse(res, { type: 'status', activity: entry.handle.agent.status === 'running' ? 'thinking' : 'idle' })
            const heartbeat = setInterval(() => { if (!res.writableEnded) res.write(': heartbeat\n\n') }, 15_000)
            req.on('close', () => { clearInterval(heartbeat); entry.clients.delete(res) })
            return
          }
          if (route.kind === 'messages') {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            const body = await readJson(req)
            const text = typeof body.text === 'string' ? body.text.trim() : ''
            if (!text) return json(res, 400, { error: 'text-required' })
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
          const reference = createHash('sha256').update(String(error)).digest('hex').slice(0, 12)
          wctx.logger.warn(`wallpaper bridge request failed (${reference})`)
          return json(res, 500, { error: 'bridge-error', reference })
        }
      },
    })
    wctx.effect(() => () => { statusDispose(); sessionsDispose() })
  })
}

export default apply
