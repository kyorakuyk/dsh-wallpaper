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
import { join, resolve } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { API_PREFIX, BRIDGE_VERSION, bearerAuthorized, contentText, errorReference, isSafeSessionId, isVisibleWallpaperMessage, mapSessionEvent, parseSessionRoute, type BridgeEvent } from './protocol.ts'

export const name = 'wallpaper-bridge'
export const inject = ['agentDefaultModel', 'agentPresets', 'agents', 'webServer', 'workspaceRegistry', 'permissionPresets', 'commands']

export interface Config {
  /**
   * Host-owned DSH data root. The bridge always creates its bearer token at
   * `<tokenRoot>/wallpaper/bridge-token`; it never treats a configured path as
   * a token file or rewrites ACLs on the configured root itself.
   */
  tokenRoot?: string
  /**
   * Optional host-owned working directory for newly created sessions.
   * This is bridge configuration, never a value accepted over the wallpaper
   * HTTP protocol: a bearer token authorizes chat, not arbitrary filesystem
   * context selection.
   */
  cwd?: string
  /** Host-owned directory registered as the wallpaper's DSH workspace. */
  workspacePath?: string
  /** Durable DSH workspace display title for wallpaper sessions. */
  workspaceTitle?: string
}

export const Config: Schema<Config> = Schema.object({
  tokenRoot: Schema.string(),
  cwd: Schema.string(),
  workspacePath: Schema.string(),
  workspaceTitle: Schema.string(),
}) as Schema<Config>

interface LiveSession {
  handle: AgentHandle
  clients: Set<ServerResponse>
}

/** The narrow host-owned default model API consumed by the bridge. */
interface DefaultModelSelection {
  currentSelection(): { provider: string; model: string }
}

interface DesktopWorkspace {
  readonly title: string
  readonly path: string
  readonly sessionIds: readonly SessionId[]
  attachSession(sessionId: SessionId): Promise<void>
}

interface WorkspaceRegistry {
  list(): readonly DesktopWorkspace[]
  create(path: string, title?: string): Promise<DesktopWorkspace>
}

interface AgentPresetDirectory {
  readonly id: string
  readonly name?: string
  readonly description?: string
  readonly trust: 'system' | 'user'
  readonly broken?: string
}

interface AgentPresets {
  readonly defaultId: string
  list(): Promise<readonly AgentPresetDirectory[]>
  mount(agentContext: Context, preset?: string): Promise<unknown>
  recompose(agentContext: Context, preset: string): Promise<AgentPresetDirectory>
}

interface PermissionPresets { readonly names: readonly string[]; current(session: Session): string; set(session: Session, name: string): void }
interface Commands {
  list(agent: { id: unknown }): readonly { name: string; description: string; input?: { hint: string } }[]
  execute(agent: { id: unknown }, text: string, signal: AbortSignal): Promise<unknown>
}

// This is an HTTP boundary, so measure the actual UTF-8 payload rather than
// JavaScript UTF-16 code units. Keep it in lockstep with the native client.
const MAX_MESSAGE_BYTES = 100_000
const MAX_REQUEST_BODY_BYTES = 1_048_576
// The native wallpaper client rejects individual Harness SSE records above
// this size. Keep the producer at the same boundary so an unexpected DSH
// payload cannot first accumulate in this process's HTTP write queue.
const MAX_SSE_EVENT_BYTES = 4 * 1024 * 1024
const MAX_SSE_IDENTIFIER_BYTES = 200
const MAX_SSE_SUMMARY_BYTES = 500
const MAX_SSE_TOKEN_COUNT = 1_000_000_000_000
const MAX_SSE_COST = 1_000_000
const MIN_TOKEN_LENGTH = 32
const execFileAsync = promisify(execFile)
const TOKEN_DIRECTORY_NAME = 'wallpaper'
const TOKEN_FILE_NAME = 'bridge-token'

class RequestBodyError extends Error {
  constructor(readonly status: 400 | 413, readonly code: 'invalid-request' | 'request-too-large') {
    super(code)
  }
}

function defaultTokenRoot(): string {
  return process.env.DSH_HOME?.trim() || join(homedir(), '.dsh')
}

/**
 * Resolve the one supported on-disk location for the local bearer token.
 *
 * `root` may be host-configured, but only the dedicated `wallpaper` child is
 * ever ACL-rewritten. This deliberately replaces the old `tokenFile` option:
 * accepting an arbitrary token filename would make its parent directory an
 * unsafe target for a private-ACL reset.
 */
export function tokenFileForRoot(root: string): string {
  return join(resolve(root), TOKEN_DIRECTORY_NAME, TOKEN_FILE_NAME)
}

function configuredTokenRoot(config: Config): string {
  return config.tokenRoot?.trim() || defaultTokenRoot()
}

function desktopWorkspaceTitle(config: Config): string {
  return config.workspaceTitle?.trim() || '桌面会话'
}

function desktopWorkspacePath(config: Config): string {
  return resolve(config.workspacePath?.trim() || join(configuredTokenRoot(config), 'workspace', 'dsh-wallpaper-desktop'))
}

function localDailyWallpaperSessionId(now: Date = new Date()): string {
  const date = [now.getFullYear(), String(now.getMonth() + 1).padStart(2, '0'), String(now.getDate()).padStart(2, '0')].join('-')
  return `wallpaper-${date}`
}

async function ensureDesktopWorkspace(registry: WorkspaceRegistry, config: Config): Promise<DesktopWorkspace> {
  const title = desktopWorkspaceTitle(config)
  const existing = registry.list().find((workspace) => workspace.title === title)
  if (existing) return existing
  const path = desktopWorkspacePath(config)
  await mkdir(path, { recursive: true })
  return registry.create(path, title)
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
 * Build the deliberately narrow ACL rewrite used for the local bearer token.
 *
 * `icacls /grant:r` replaces only the named SID's existing explicit ACEs;
 * it does not clear ACEs belonging to Everyone, Users, or another account.
 * Start with `/reset` so stale explicit entries are removed, then establish
 * the current user's explicit allow ACE *before* removing inherited defaults.
 * This ordering matters: a filesystem watcher can otherwise observe the
 * directory in the small `/inheritance:r` -> `/grant:r` gap with no usable
 * ACE and fail its whole watch with EPERM. See the Microsoft `icacls`
 * reference for `/reset`, `/inheritancelevel:r`, and `/grant:r` semantics.
 *
 * Exported only for the source-level regression test; it is not part of the
 * wallpaper HTTP protocol.
 */
export function windowsTokenAclCommands(file: string, sid: string): ReadonlyArray<readonly string[]> {
  return [
    [file, '/setowner', `*${sid}`],
    [file, '/reset'],
    [file, '/grant:r', `*${sid}:(F)`],
    [file, '/inheritance:r'],
  ]
}

/**
 * The containing directory is private before a token is ever written. This
 * avoids a newly-created token briefly inheriting a broad ACL and is also the
 * barrier that prevents another local account from replacing the token
 * between the bridge's ACL repair and write steps. This is always the
 * application-owned `<tokenRoot>/wallpaper` child, never an arbitrary parent
 * supplied by configuration.
 */
export function windowsTokenDirectoryAclCommands(directory: string, sid: string): ReadonlyArray<readonly string[]> {
  return [
    [directory, '/setowner', `*${sid}`],
    [directory, '/reset'],
    [directory, '/grant:r', `*${sid}:(OI)(CI)(F)`],
    [directory, '/inheritance:r'],
  ]
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
  // A bearer credential must not retain an explicit ACE belonging to another
  // user.  Run every step directly (never through a shell), and fail closed on
  // the first error: restoring inherited access as a "recovery" fallback
  // would make an unsafe token usable again.
  for (const args of windowsTokenAclCommands(absoluteFile, sid)) {
    await execFileAsync('icacls.exe', [...args], options)
  }
}

async function restrictTokenDirectory(directory: string): Promise<void> {
  if (process.platform !== 'win32') {
    await chmod(directory, 0o700)
    return
  }
  const sid = await currentWindowsSid()
  const absoluteDirectory = resolve(directory)
  const options = { windowsHide: true, timeout: 5_000, maxBuffer: 8_192 }
  for (const args of windowsTokenDirectoryAclCommands(absoluteDirectory, sid)) {
    await execFileAsync('icacls.exe', [...args], options)
  }
}

async function assertRegularTokenFile(file: string): Promise<void> {
  const stat = await lstat(file)
  // A pre-existing hard link could make token rotation overwrite an unrelated
  // file. The token path has no legitimate link count other than one.
  if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1) {
    throw new Error('bridge token path is not a regular file')
  }
}

async function assertTokenDirectory(directory: string): Promise<void> {
  const stat = await lstat(directory)
  // Do this before ACL changes: a junction/symlink must never redirect the
  // ACL rewrite onto a directory outside of the bridge-owned namespace.
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new Error('bridge token directory is not a regular directory')
  }
}

async function ensureToken(root: string): Promise<string> {
  const file = tokenFileForRoot(root)
  const directory = join(resolve(root), TOKEN_DIRECTORY_NAME)
  // Creating the directory is harmless even when it initially inherits a
  // broad DACL: no credential exists yet. Tighten it before reading or
  // creating the bearer value.
  await mkdir(directory, { recursive: true })
  await assertTokenDirectory(directory)
  await restrictTokenDirectory(directory)

  // Reuse an existing private token. Rotating it on every startup breaks an
  // already-running bridge when a second DSH launch races for port 3080: the
  // old server keeps the previous in-memory token while the new process
  // overwrites the shared file before its listen attempt fails.
  try {
    await assertRegularTokenFile(file)
    await restrictTokenFile(file)
    return validateToken((await readFile(file, 'utf8')).trim())
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
  }

  const generated = randomBytes(32).toString('base64url')
  try {
    const handle = await open(file, 'wx', 0o600)
    await handle.close()
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
    await assertRegularTokenFile(file)
    await restrictTokenFile(file)
    return validateToken((await readFile(file, 'utf8')).trim())
  }
  await assertRegularTokenFile(file)
  await restrictTokenFile(file)
  // The directory and file now reject other Windows accounts. Open after the
  // postcondition rather than writing the secret during the creation window.
  const handle = await open(file, 'r+')
  try {
    await handle.truncate(0)
    await handle.writeFile(`${generated}\n`, 'utf8')
  } finally {
    await handle.close()
  }
  // Read back only to validate the actual persisted value; do not return a
  // caller-provided or pre-rotation token.
  return validateToken((await readFile(file, 'utf8')).trim())
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

function boundedSseText(value: unknown, maximum: number): string | undefined {
  return typeof value === 'string' && Buffer.byteLength(value, 'utf8') <= maximum ? value : undefined
}

function boundedSseInteger(value: unknown): number | undefined {
  return typeof value === 'number'
    && Number.isSafeInteger(value)
    && value >= 0
    && value <= MAX_SSE_TOKEN_COUNT
    ? value
    : undefined
}

function safeSseSessionId(value: unknown): string | undefined {
  const sessionId = boundedSseText(value, MAX_SSE_IDENTIFIER_BYTES)
  return sessionId !== undefined && isSafeSessionId(sessionId) ? sessionId : undefined
}

/**
 * Rebuild every outbound event before serializing it. Session events come
 * from DSH rather than this HTTP server, so their runtime values must still
 * be treated as untrusted at the SSE boundary. In particular, check large
 * strings before JSON.stringify can duplicate them into Node's response
 * buffer.
 */
function serializeSseEvent(event: BridgeEvent): string | undefined {
  let safe: BridgeEvent
  switch (event.type) {
    case 'status': {
      if (!['idle', 'sending', 'thinking', 'streaming', 'tool', 'done'].includes(event.activity)) return undefined
      safe = { type: 'status', activity: event.activity }
      break
    }
    case 'delta': {
      const text = boundedSseText(event.text, MAX_MESSAGE_BYTES)
      if (text === undefined) return undefined
      safe = { type: 'delta', text }
      break
    }
    case 'message': {
      const content = boundedSseText(event.content, MAX_MESSAGE_BYTES)
      if (content === undefined || (event.role !== 'user' && event.role !== 'assistant')) return undefined
      safe = { type: 'message', role: event.role, content }
      break
    }
    case 'usage': {
      const input = boundedSseInteger(event.input)
      const output = boundedSseInteger(event.output)
      const cacheRead = event.cacheRead === undefined ? undefined : boundedSseInteger(event.cacheRead)
      const cost = event.cost === undefined
        ? undefined
        : typeof event.cost === 'number' && Number.isFinite(event.cost) && event.cost >= 0 && event.cost <= MAX_SSE_COST
          ? event.cost
          : undefined
      if (input === undefined || output === undefined
        || (event.cacheRead !== undefined && (cacheRead === undefined || cacheRead > input))
        || (event.cost !== undefined && cost === undefined)) return undefined
      safe = {
        type: 'usage',
        input,
        output,
        ...(cacheRead === undefined ? {} : { cacheRead }),
        ...(cost === undefined ? {} : { cost }),
      }
      break
    }
    case 'model': {
      const model = boundedSseText(event.model, MAX_SSE_IDENTIFIER_BYTES)
      const provider = event.provider === undefined ? undefined : boundedSseText(event.provider, MAX_SSE_IDENTIFIER_BYTES)
      const effort = event.effort === undefined ? undefined : boundedSseText(event.effort, MAX_SSE_IDENTIFIER_BYTES)
      if (model === undefined || (event.provider !== undefined && provider === undefined)
        || (event.effort !== undefined && effort === undefined)) return undefined
      safe = {
        type: 'model',
        model,
        ...(provider === undefined ? {} : { provider }),
        ...(effort === undefined ? {} : { effort }),
      }
      break
    }
    case 'approval-required': {
      const sessionId = safeSseSessionId(event.sessionId)
      const summary = boundedSseText(event.summary, MAX_SSE_SUMMARY_BYTES)
      if (sessionId === undefined || summary === undefined) return undefined
      safe = { type: 'approval-required', sessionId, summary }
      break
    }
    case 'error': {
      const code = boundedSseText(event.code, MAX_SSE_IDENTIFIER_BYTES)
      const message = boundedSseText(event.message, MAX_MESSAGE_BYTES)
      if (code === undefined || message === undefined || typeof event.recoverable !== 'boolean') return undefined
      safe = { type: 'error', code, recoverable: event.recoverable, message }
      break
    }
    case 'disconnected': {
      if (event.recoverable !== true) return undefined
      safe = { type: 'disconnected', recoverable: true }
      break
    }
    default:
      return undefined
  }
  const record = `data: ${JSON.stringify(safe)}\n\n`
  return Buffer.byteLength(record, 'utf8') <= MAX_SSE_EVENT_BYTES ? record : undefined
}

function closeSseClient(res: ServerResponse): void {
  if (!res.writableEnded && !res.destroyed) res.destroy?.()
}

function writeSseRecord(res: ServerResponse, record: string): boolean {
  if (res.writableEnded || res.destroyed) return false
  try {
    // A false return means Node has crossed its high-water mark. Do not keep
    // publishing while it waits for drain: this is a one-way live stream, so
    // disconnecting a slow client bounds memory and lets it reconnect.
    if (res.write(record)) return true
  } catch {
    // A peer can close between the state check and write(). Treat it exactly
    // like a slow or dead client and keep it out of the subscriber set.
  }
  closeSseClient(res)
  return false
}

function sse(res: ServerResponse, event: BridgeEvent): boolean {
  const record = serializeSseEvent(event)
  if (record !== undefined) return writeSseRecord(res, record)

  // Do not serialize, truncate, or log an oversize DSH-derived value. A
  // bounded protocol error tells the native client why the stream ended while
  // ensuring no further data is queued for this subscriber.
  const failure = serializeSseEvent({
    type: 'error',
    code: 'HARNESS_SSE_EVENT_LIMIT',
    recoverable: true,
    message: 'DSH bridge 事件超过安全大小限制，连接已关闭。',
  })
  if (failure !== undefined) void writeSseRecord(res, failure)
  closeSseClient(res)
  return false
}

function heartbeatSse(res: ServerResponse): boolean {
  return writeSseRecord(res, ': heartbeat\n\n')
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
    .filter(isVisibleWallpaperMessage)
    .map((message) => ({ id: message.id, role: message.role, content: contentText(message) }))
}

export function apply(ctx: Context, config: Config = {}): void {
  const live = new Map<string, LiveSession>()
  // A collection POST awaits agent creation. Without this per-session
  // single-flight map, two requests that arrive in that await gap can each
  // create an AgentHandle for the same logical session and leak one of them.
  // Keep pending work separate from `live`: only a fully created handle is
  // allowed to receive messages or SSE subscribers.
  const creating = new Map<string, Promise<LiveSession>>()
  let stopping = false
  const canResume = (): boolean => {
    // Keep the lightweight unit-test harness compatible while production
    // Cordis contexts use `get()` for optional service discovery.
    const get = (ctx as Context & { get?: (name: string) => unknown }).get
    return typeof get === 'function' && get.call(ctx, 'sessionPersistence') !== undefined
  }
  let token = ''
  let tokenFailure: string | undefined
  const tokenReady = ensureToken(configuredTokenRoot(config))
    .then((value) => { token = value })
    .catch((error: unknown) => { tokenFailure = errorReference(error) })

  const publish = (sessionId: string, event: BridgeEvent): void => {
    const entry = live.get(sessionId)
    if (!entry) return
    for (const client of [...entry.clients]) {
      if (client.writableEnded || client.destroyed) entry.clients.delete(client)
      else if (!sse(client, event)) entry.clients.delete(client)
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
    stopping = true
    const disposals: Promise<void>[] = []
    for (const entry of live.values()) {
      for (const client of entry.clients) client.end()
      disposals.push(entry.handle.dispose())
    }
    live.clear()
    // A create already in flight cannot be cancelled through the public DSH
    // API. Await it so its post-await shutdown check can dispose the handle
    // instead of installing it after this bridge has been torn down.
    await Promise.allSettled([...creating.values()])
    creating.clear()
    await Promise.allSettled(disposals)
  })

  // Status must not depend on optional session-control services.  A Web
  // profile may take longer to compose commands, presets, or workspaces than
  // its HTTP listener; reporting the bridge as missing during that interval
  // makes the wallpaper's route toggle misleading.  The mutating routes
  // below still wait for their complete service set.
  ctx.inject(['webServer'], (statusContext) => {
    if (statusContext.webServer.host !== '127.0.0.1') {
      throw new Error('dsh-wallpaper-bridge refuses to run on a non-loopback WebServer')
    }
    const dispose = statusContext.webServer.register({
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
    statusContext.effect(() => () => { dispose() })
  })

  // The HTTP handlers create and resume standard DSH agents.  Keep both
  // services in this child scope explicitly: a test mock that happens to
  // expose `agents` next to `webServer` must not hide a real Cordis scope
  // where only the declared dependencies are available.
  ctx.inject(['agentDefaultModel', 'agentPresets', 'agents', 'webServer', 'workspaceRegistry', 'permissionPresets', 'commands'], (wctx) => {
    const controlDispose = wctx.webServer.register({
      kind: 'prefix',
      path: `${API_PREFIX}/control`,
      handler: async (req, res) => {
        await tokenReady
        if (tokenFailure !== undefined) return json(res, 503, { error: 'bridge-token-unavailable', reference: tokenFailure })
        if (!bearerAuthorized(req.headers.authorization, token)) return json(res, 401, { error: 'unauthorized' })
        const pathname = new URL(req.url ?? '/', 'http://127.0.0.1').pathname
        try {
          const host = wctx as unknown as { agentPresets: AgentPresets; permissionPresets: PermissionPresets; commands: Commands }
          if (pathname === `${API_PREFIX}/control/presets` && req.method === 'GET') {
          const presets = await host.agentPresets.list()
          // Preset composition paths are host-private. Expose only the
          // metadata needed to render a picker and explain unavailable rows.
          return json(res, 200, {
            presets: presets.map((preset) => ({
              id: preset.id,
              ...(preset.name ? { name: preset.name } : {}),
              ...(preset.description ? { description: preset.description } : {}),
              trust: preset.trust,
              ...(preset.broken ? { broken: preset.broken } : {}),
              isDefault: preset.id === host.agentPresets.defaultId,
            })),
          })
          }
          const presetSwitch = pathname.match(new RegExp(`^${API_PREFIX}/control/sessions/([^/]+)/preset$`))
          const sessionControls = pathname.match(new RegExp(`^${API_PREFIX}/control/sessions/([^/]+)$`))
          if (sessionControls && req.method === 'GET') {
            const sessionId = decodeURIComponent(sessionControls[1] ?? '')
            if (!isSafeSessionId(sessionId)) return json(res, 400, { error: 'invalid-session-id' })
            const entry = live.get(sessionId)
            if (!entry) return json(res, 404, { error: 'session-not-live' })
            return json(res, 200, {
              // DSH's permission service derives its display value from the
              // durable event stream.  Keep this narrow structural view here:
              // a few pre-release package builds exposed an older declaration
              // accepting Session while the runtime already expects events.
              permission: {
                current: (host.permissionPresets as unknown as { current(events: readonly SessionEvent[]): string })
                  .current(entry.handle.agent.session.events),
                options: host.permissionPresets.names,
              },
              commands: host.commands.list(entry.handle.agent).map((command) => ({ name: command.name, description: command.description, ...(command.input ? { input: command.input } : {}) })),
            })
          }
          const permissionSwitch = pathname.match(new RegExp(`^${API_PREFIX}/control/sessions/([^/]+)/permission$`))
          if (permissionSwitch && req.method === 'POST') {
            const sessionId = decodeURIComponent(permissionSwitch[1] ?? '')
            if (!isSafeSessionId(sessionId)) return json(res, 400, { error: 'invalid-session-id' })
            const entry = live.get(sessionId)
            if (!entry) return json(res, 404, { error: 'session-not-live' })
            const body = await readJson(req)
            const permission = typeof body.permission === 'string' ? body.permission.trim() : ''
            if (!host.permissionPresets.names.includes(permission)) return json(res, 400, { error: 'unknown-permission-preset' })
            host.permissionPresets.set(entry.handle.agent.session, permission)
            return json(res, 200, { sessionId, permission })
          }
          if (!presetSwitch || req.method !== 'POST') return json(res, 404, { error: 'not-found' })
          const sessionId = decodeURIComponent(presetSwitch[1] ?? '')
          if (!isSafeSessionId(sessionId)) return json(res, 400, { error: 'invalid-session-id' })
          const entry = live.get(sessionId)
          if (!entry) return json(res, 404, { error: 'session-not-live' })
          if (entry.handle.agent.session.events.some((event) => event.type === 'turn/start')) {
            return json(res, 409, { error: 'agent-preset-locked' })
          }
          const body = await readJson(req)
          const preset = typeof body.agentPreset === 'string' ? body.agentPreset.trim() : ''
          if (!preset) return json(res, 400, { error: 'agent-preset-required' })
          const selected = (await host.agentPresets.list()).find((candidate) => candidate.id === preset)
          if (!selected) return json(res, 400, { error: 'unknown-agent-preset' })
          if (selected.broken) return json(res, 409, { error: 'agent-preset-unavailable' })
          await host.agentPresets.recompose(entry.handle.agent.ctx, preset)
          ;(entry.handle.agent.session.append as (...args: unknown[]) => unknown)(
            'agent-preset/selected',
            { agentPreset: preset },
          )
          return json(res, 200, { sessionId, agentPreset: preset })
        } catch (error) {
          const reference = errorReference(error)
          wctx.logger.warn(`wallpaper bridge control query failed (${reference})`)
          return json(res, 500, { error: 'bridge-error', reference })
        }
      },
    })

    const sessionsDispose = wctx.webServer.register({
      kind: 'prefix',
      path: `${API_PREFIX}/sessions`,
      handler: async (req, res) => {
        const host = wctx as unknown as { commands: Commands }
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
            const requestedResume = typeof body.resumeSessionId === 'string' ? body.resumeSessionId.trim() : ''
            const automaticDailySession = !requested && !requestedResume
            const workspace = automaticDailySession
              ? await ensureDesktopWorkspace(
                (wctx as unknown as { workspaceRegistry: WorkspaceRegistry }).workspaceRegistry,
                config,
              )
              : undefined
            const id = automaticDailySession ? localDailyWallpaperSessionId() : requestedResume || requested || `wallpaper-${randomUUID()}`
            const resume = requestedResume || (workspace?.sessionIds.some((sessionId) => String(sessionId) === id) ? id : '')
            if (resume && !canResume()) return json(res, 409, { error: 'resume-unavailable' })
            if (!isSafeSessionId(id)) return json(res, 400, { error: 'invalid-session-id' })
            const existing = live.get(id)
            if (existing) return json(res, 200, sessionSummary(existing))
            const provider = typeof body.provider === 'string' && body.provider.trim() ? body.provider.trim() : undefined
            const model = typeof body.model === 'string' && body.model.trim() ? body.model.trim() : undefined
            // `ctx.agents.create()` is intentionally low-level: unlike the
            // Web API gateway it does not apply the host's default model on
            // behalf of a caller. A blank provider/model would therefore let
            // a session start and only fail later when `{{model}}` is rendered
            // in the deployment persona. Read the host-owned selection here;
            // request values remain an explicit override for future clients.
            const host = wctx as unknown as { agentDefaultModel: DefaultModelSelection; agentPresets: AgentPresets }
            const defaults = host
              .agentDefaultModel.currentSelection()
            const preset = typeof body.agentPreset === 'string' && body.agentPreset.trim()
              ? body.agentPreset.trim()
              : host.agentPresets.defaultId
            const availablePresets = await host.agentPresets.list()
            const selectedPreset = availablePresets.find((candidate) => candidate.id === preset)
            if (!selectedPreset) return json(res, 400, { error: 'unknown-agent-preset' })
            if (selectedPreset.broken) return json(res, 409, { error: 'agent-preset-unavailable' })
            const agentOptions = {
              provider: provider ?? defaults.provider,
              model: model ?? defaults.model,
            }
            // A session's workspace is a host policy choice.  Do not permit a
            // local HTTP caller to override it, even if it holds the Bridge
            // token; the wallpaper never needs this control to create a
            // standard DSH session.
            // The deployment persona contains a strict `{{cwd}}` variable.
            // A fresh DSH session therefore needs a host-owned workspace even
            // when the operator did not configure a narrower bridge cwd.
            // `process.cwd()` is the DSH launch directory, never HTTP input.
            const cwd = workspace?.path || config.cwd?.trim() || process.cwd()
            let pending = creating.get(id)
            const createdByThisRequest = pending === undefined
            if (!pending) {
              pending = (async (): Promise<LiveSession> => {
                if (stopping) throw new Error('wallpaper bridge is shutting down')
                const handle = resume
                  ? await wctx.agents.resume({
                    resumeSessionId: SessionId(id),
                    agentOptions,
                    setup: (agentContext) => host.agentPresets.mount(agentContext, preset).then(() => undefined),
                  })
                  : await wctx.agents.create({
                      sessionId: SessionId(id),
                      meta: { cwd, agentPreset: preset },
                      agentOptions,
                      setup: (agentContext) => host.agentPresets.mount(agentContext, preset).then(() => undefined),
                    })
                // Plugin teardown may have started while DSH created the
                // agent. Dispose it rather than leaving a live handle that no
                // route can own or clean up.
                if (stopping) {
                  await handle.dispose().catch(() => undefined)
                  throw new Error('wallpaper bridge is shutting down')
                }
                const entry: LiveSession = { handle, clients: new Set<ServerResponse>() }
                live.set(id, entry)
                if (workspace && !resume) await workspace.attachSession(SessionId(id))
                return entry
              })()
              creating.set(id, pending)
              const clearPending = () => {
                if (creating.get(id) === pending) creating.delete(id)
              }
              // Handle both resolution and rejection so this bookkeeping
              // promise never becomes an unhandled rejection of its own.
              void pending.then(clearPending, clearPending)
            }
            const entry = await pending
            return json(res, createdByThisRequest ? 201 : 200, sessionSummary(entry))
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
            let subscribed = true
            for (const event of initialEvents(entry)) {
              if (!sse(res, event)) {
                subscribed = false
                break
              }
            }
            if (!subscribed) {
              entry.clients.delete(res)
              return
            }
            res.flushHeaders()
            const heartbeat = setInterval(() => {
              if (!heartbeatSse(res)) {
                clearInterval(heartbeat)
                entry.clients.delete(res)
              }
            }, 15_000)
            req.on('close', () => { clearInterval(heartbeat); entry.clients.delete(res) })
            return
          }
          if (route.kind === 'messages') {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            const body = await readJson(req)
            const text = typeof body.text === 'string' ? body.text.trim() : ''
            if (!text) return json(res, 400, { error: 'text-required' })
            if (Buffer.byteLength(text, 'utf8') > MAX_MESSAGE_BYTES) return json(res, 413, { error: 'text-too-large' })
            // Slash commands belong to DSH's scoped command registry. Running
            // them here preserves their normal session events/side effects;
            // ordinary prose remains a model followup. The wallpaper never
            // interprets command names itself.
            if (text.startsWith('/')) {
              await host.commands.execute(entry.handle.agent, text, new AbortController().signal)
            } else {
              entry.handle.agent.followup(createUserMessage({ content: [{ type: 'text', text }], source: { kind: 'user' } }))
            }
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
    wctx.effect(() => () => { controlDispose(); sessionsDispose() })
  })
}

export default apply
