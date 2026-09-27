import type { Context } from '@deepseek-ai/cordis'
import Schema from '@deepseek-ai/schemastery'
import type { AgentHandle } from '@deepseek-ai/dsh-agent'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import { SessionId, type Session, type SessionEvent } from '@deepseek-ai/dsh-session'
import type {} from '@deepseek-ai/dsh-host-webserver'
import type {} from '@deepseek-ai/dsh-user-approval'
import { randomBytes, randomUUID } from 'node:crypto'
import { execFile } from 'node:child_process'
import { chmod, lstat, mkdir, open, readFile, writeFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { API_PREFIX, BRIDGE_AUTHORED_AGAINST, BRIDGE_BUILD, BRIDGE_PROTOCOL_VERSION, BRIDGE_VERSION, bearerAuthorized, contentText, errorReference, isSafeSessionId, isVisibleWallpaperMessage, mapSessionEvent, parseSessionRoute, type BridgeEvent, type BridgeQuestion, type BridgeQuestionOption } from './protocol.ts'
import {
  createHostAdapter,
  HOST_ADAPTER_SERVICES,
  HostIncompatibleError,
  type AgentPresetDirectory,
  type CommandDescriptor,
  type DesktopWorkspace,
  type HostAdapter,
} from './host.ts'

export const name = 'wallpaper-bridge'
/**
 * Declared dependencies, taken from the same list the host adapter validates so
 * the two cannot drift apart.
 *
 * Note for host authors: because this module also has a default export (the
 * `apply` function), the Cordis loader reads `plugin.inject` off the *unwrapped*
 * value, so this export documents the dependency set for tooling rather than
 * gating `apply`. The gating is done explicitly by the `ctx.inject(...)` calls
 * inside `apply`, which is also what lets the public `/status` route mount
 * before the session services are composed. See `bridge/README.md`.
 */
export const inject = HOST_ADAPTER_SERVICES

/**
 * Owns asynchronous work that outlives the call which started it, so plugin
 * teardown can wait for it.
 *
 * `apply()` starts token provisioning — `mkdir`, `whoami.exe`, four `icacls`
 * passes, a file create/write — as a detached promise. Nothing awaited it at
 * teardown, so a caller that removed the token root right after disposing the
 * plugin (a test fixture, a DSH shutdown that cleans its data directory, a
 * launched-then-stopped DSH) deleted a directory while `icacls` still held a
 * handle on it. On Windows that surfaces as EBUSY from the caller's file
 * operation, and the outcome looked like a flaky unrelated failure.
 *
 * Every detached task must therefore be registered here. `settle()` is
 * idempotent, stops new work from being accepted, and resolves only once every
 * registered task has finished.
 */
export class AsyncWorkTracker {
  private readonly pending = new Set<Promise<unknown>>()
  private drained: Promise<void> | undefined
  private closed = false

  /** True once `settle()` has been requested; no new work is accepted after. */
  get isDraining(): boolean {
    return this.closed
  }

  /**
   * Track an already-started promise. Returns `false` when the tracker is
   * draining, in which case the caller must not rely on its side effects.
   */
  track(work: Promise<unknown>): boolean {
    if (this.closed) return false
    // Remove on settlement, and absorb rejection here so a tracked task that
    // fails never becomes an unhandled rejection of its own. The promise added
    // to `pending` must be the one that removes itself, otherwise the set would
    // never drain.
    let tracked: Promise<void>
    tracked = work.then(
      () => undefined,
      (error: unknown) => {
        this.onError?.(error)
        return undefined
      },
    ).finally(() => { this.pending.delete(tracked) })
    this.pending.add(tracked)
    return true
  }

  /** Run a task under the tracker, refusing to start once draining. */
  run(work: () => Promise<void>): boolean {
    if (this.closed) return false
    return this.track(Promise.resolve().then(work))
  }

  /** How many registered tasks are still running. */
  get size(): number {
    return this.pending.size
  }

  /**
   * Stop accepting work and resolve once everything registered has finished.
   * Calling it twice returns the same promise and never re-opens the tracker.
   */
  settle(): Promise<void> {
    this.closed = true
    if (this.drained) return this.drained
    // Re-check `pending` until it is empty: a task may register a follow-up
    // before it resolves, and that follow-up must also be awaited.
    this.drained = (async () => {
      while (this.pending.size > 0) {
        await Promise.allSettled([...this.pending])
      }
    })()
    return this.drained
  }

  /** Optional sink for a tracked task's failure. Set before tracking. */
  onError: ((error: unknown) => void) | undefined
}

export interface Config {
  /**
   * 「助手日」的边界小时（0–23，默认 4）：跨日不在零点而在凌晨四点 —— 深夜还在做的事，
   * 在用户心里"今天"还没过去。日会话命名（`wallpaper-<助手日>`）以它为准。
   * **必须与壁纸设置里的 `dayBoundaryHour` 同值**，否则"同一段对话"会在两侧变成两天。
   */
  dayBoundaryHour?: number
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
  /** Permission preset for newly created desktop sessions. */
  desktopPermission?: string
}

export const Config: Schema<Config> = Schema.object({
  tokenRoot: Schema.string(),
  cwd: Schema.string(),
  workspacePath: Schema.string(),
  workspaceTitle: Schema.string(),
  desktopPermission: Schema.string(),
  dayBoundaryHour: Schema.number(),
}) as Schema<Config>

interface LiveSession {
  handle: AgentHandle
  clients: Set<ServerResponse>
  /** Last time this handle did anything (subscriber attach, event, message).
   * The idle sweep uses it to release handles nobody is watching. */
  lastActivityAt: number
  /** When this process created the handle. Only the archive-visibility check
   * reads it: a session we just created cannot have been archived yet, while
   * `lastActivityAt` moves with every request and would never expire. */
  createdAt: number
}

/** The narrow host-owned default model API consumed by the bridge. */
interface DefaultModelSelection {
  currentSelection(): { provider: string; model: string }
}

// The host surface is declared and validated once in `./host.ts`. The scoped
// views below are what each route group receives, so a handler cannot reach a
// service the adapter did not verify. `DesktopWorkspace`,
// `AgentPresetDirectory` and `CommandDescriptor` come from the adapter itself.
type PermissionPresets = Pick<HostAdapter, 'permissionNames' | 'currentPermission' | 'setPermission'>
type Commands = Pick<HostAdapter, 'commandsFor' | 'executeCommand'>
type AgentPresets = Pick<HostAdapter, 'defaultPresetId' | 'presetDirectory' | 'mountPreset' | 'recomposePreset'>

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
const MAX_SSE_QUESTION_COUNT = 8
const MAX_SSE_QUESTION_OPTIONS = 12
const MAX_SSE_TOKEN_COUNT = 1_000_000_000_000
const MAX_SSE_COST = 1_000_000
// Resource ceilings. The bridge is a resident process that any local
// token-holder can drive, so every allocation it owns needs an explicit bound:
// live agent handles, SSE subscribers per handle, in-flight creations, and the
// history payload it puts on the wire.
//
// A desktop has one wallpaper; more than a handful of live sessions means a
// client is leaking handles rather than doing work, so refuse instead of
// growing. Over-limit requests get a stable 429/409 the caller can act on
// rather than an unbounded allocation.
export const MAX_LIVE_SESSIONS = 8
export const MAX_SSE_CLIENTS_PER_SESSION = 4
/** 刚建出来的会话在这么长时间内不参与"归档可见性"判断：宿主的可见列表可能还没包含它，
 * 而按"不可见"判死会丢掉一条健康的会话。（用户实测：桌面会话目录搬家之后正是这种情况。） */
export const ARCHIVED_VISIBILITY_GRACE_MS = 30_000
export const MAX_PENDING_CREATIONS = 8
export const MAX_HISTORY_MESSAGES = 256
export const MAX_HISTORY_BYTES = 4 * 1024 * 1024
/** An idle handle with no subscriber is released after this long. Timestamps
 * and message ids are preserved, so a reconnect resumes the same DSH session
 * instead of losing the transcript. */
export const LIVE_SESSION_IDLE_TTL_MS = 10 * 60 * 1000
export const LIVE_SESSION_SWEEP_INTERVAL_MS = 30 * 1000
const MIN_TOKEN_LENGTH = 32
const execFileAsync = promisify(execFile)
const TOKEN_DIRECTORY_NAME = 'wallpaper'
const TOKEN_FILE_NAME = 'bridge-token'
const DEFAULT_DESKTOP_PERMISSION_PRESET = 'workspace-write'
const DESKTOP_ENTRY_CONTEXT_NAME = 'wallpaper:desktop-entry'

class RequestBodyError extends Error {
  constructor(readonly status: 400 | 413, readonly code: 'invalid-request' | 'request-too-large') {
    super(code)
  }
}

/**
 * A resource ceiling was reached. `status`/`code` are stable contract values:
 * 429 means "retry later, this bridge is full", 409 means "this specific
 * handle is already fully subscribed".
 */
class BridgeLimitError extends Error {
  constructor(readonly status: 429 | 409, readonly code: string) {
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
  return resolve(config.workspacePath?.trim() || config.cwd?.trim() || defaultDesktopWorkspacePath(config))
}

/** 壁纸的打包标识，也就是它数据目录的名字。 */
export const WALLPAPER_DATA_DIRECTORY_NAME = 'com.dsh.wallpaper'
/** 桌面会话目录名：工作区目录与它在 DSH 里的标题同名。 */
export const DESKTOP_WORKSPACE_DIRECTORY_NAME = '桌面会话'

/**
 * 工作区默认落在**壁纸自己的数据目录**下，而不是 DSH 那边、也不是安装目录。
 *
 * * 安装目录不行：MSIX 的安装目录运行时可写但**每次升级整个被替换** —— 写在那里的东西必丢；
 * * 数据目录刚好满足全部要求：运行时可写、升级保留、卸载也留得下（用户明确要求"卸载之后
 *   也要保留桌面会话的数据"）。固定位置 + 固定名字，缺失时重建同名目录即可。
 */
export function defaultDesktopWorkspacePath(config: Config, localAppData = process.env.LOCALAPPDATA): string {
  const root = localAppData?.trim()
  if (root) return join(root, WALLPAPER_DATA_DIRECTORY_NAME, DESKTOP_WORKSPACE_DIRECTORY_NAME)
  return join(configuredTokenRoot(config), 'workspace', DESKTOP_WORKSPACE_DIRECTORY_NAME)
}

/** The wallpaper bridge is intentionally local-only, including every
 * mutating/control route. Keep this as one exact predicate so a future route
 * cannot accidentally copy only the status-route guard. */
export function isLoopbackWebServerHost(host: unknown): host is '127.0.0.1' {
  return host === '127.0.0.1'
}

/**
 * 桌面入口的**简报**：这是"你是谁、能做什么、边界在哪、怎么改人格、记忆在哪"的唯一注入点。
 *
 * 只在创建/恢复桌面会话时注入（`desktopAgentSetup`），所以它只影响桌面工作区这一处的对话 ——
 * 别的目录的会话看不到它，也不会被它污染（用户定的范围）。
 *
 * 措辞原则：**只说真话**。读是全局放行的、写只在桌面工作区内（越界会被沙箱直接拒绝），
 * 这两件事都必须写清楚，否则助手会承诺做不到的事、或者在沙箱拒绝之后反复重试。
 */
export function desktopEntryPrompt(
  cwd: string,
  workspaceTitle: string,
  permission: string,
  options: { day?: string; memoryPath?: string; memory?: string } = {},
): string {
  const sections: string[] = [
    [
      '【你是谁】你是这台电脑桌面上的 DSH 助手：用户通过桌面上的输入岛跟你说话，',
      '他看不到 Harness 的完整界面（工具面板、审批面板都在别处）。',
      '所以每一轮都要用一两句话把"你做了什么、结果是什么、下一步要他做什么"说清楚，',
      '不要假设他能看到你的工具调用或日志。',
    ].join(''),
    [
      '【能力与边界】',
      `- 读：可以搜索并读取这台电脑上的文件来回答他的问题（读是全局放行的）。`,
      `- 写：只在桌面会话目录内可写：${cwd}（工作区标题：${workspaceTitle}）。`,
      `- 越界写会被沙箱直接拒绝。遇到拒绝时**不要反复重试**：直说"这块需要你把目标目录放进桌面会话，`,
      `  或者把权限提上去"，并等他决定。`,
      `- 当前权限预设：${permission}。原生权限服务说了算，不要暗示自己有超出它的权限。`,
      '- 破坏性、外发、花钱的动作先问；与他当前要求无关的改动不要顺手做。',
    ].join('\n'),
    [
      '【说话人格与外观是两件事，别混】',
      '- 说话人格（语气、称呼、要不要先问、有没有角色扮演）＝ **跟他说一声，你把它写进项目记忆**，下次会话照样生效。',
      '- 外观（立绘、形态、气泡）＝ 设置中心 →「形态」/「外观」，那里改，与你无关。',
      '- 他说"改人格"时，默认指说话人格。',
    ].join('\n'),
    [
      '【项目记忆】',
      options.memoryPath ? `- 文件（**只给你自己看**）：${options.memoryPath}` : '- 项目记忆文件在桌面会话目录里，名字是 项目记忆.md。',
      '- **不要在桌面回复里贴绝对路径**（那是聊天面，不是文件管理器）。他要改记忆，就让他去「设置 → 高级 → 数据与目录 → 打开项目记忆」，那里会选中这个文件。',
      '- 他说"记下来 / 以后都这样 / 别再问"⇒ 写它，并回一句"记下了：…"让他核对。',
      '- 每次会话开始先读它；与此刻的说法冲突时以**此刻**为准，并把旧条目标注为"已被 X 取代"（不要静默删除）。',
      '- 只放长期生效的要求与偏好，不放密码、令牌等敏感内容，也不放一次性任务。',
    ].join('\n'),
  ]
  if (options.day) sections.push(`【今天】助手日：${options.day}（跨日边界是本地 04:00，深夜的事仍算前一天）。`)
  if (options.memory) {
    sections.push(`【项目记忆内容】（用户长期要求，按上面的规则使用）\n${options.memory}`)
  }
  sections.push(
    '若某件事需要工具审批或更完整的 Harness 控制面，直接请用户打开 Harness；不要假装壁纸能替他批准。',
  )
  return sections.join('\n\n')
}

function desktopAgentSetup(
  host: HostAdapter,
  preset: string,
  cwd: string,
  workspaceTitle: string,
  permission: string,
  promptOptions: { day?: string; memoryPath?: string; memory?: string } = {},
): (agentContext: Context) => Promise<void> {
  return async (agentContext) => {
    await host.mountPreset(agentContext, preset)
    agentContext.systemPrompt.context({
      name: DESKTOP_ENTRY_CONTEXT_NAME,
      order: -90,
      text: desktopEntryPrompt(cwd, workspaceTitle, permission, promptOptions),
    })
  }
}

function desktopPermission(config: Config): string {
  return config.desktopPermission?.trim() || DEFAULT_DESKTOP_PERMISSION_PRESET
}

/** 「助手日」的默认边界小时（与壁纸设置里的 `dayBoundaryHour` 同一默认值）。 */
export const DEFAULT_DAY_BOUNDARY_HOUR = 4

/** 桌面会话的**项目记忆**文件名：住在桌面工作区里，用户可读可改，卸载后保留。 */
export const PROJECT_MEMORY_FILE_NAME = '项目记忆.md'

/** 注入记忆的上限：它是"长期要求"清单，不是剪贴板。超长就截断并**说明**截断了。 */
export const MAX_PROJECT_MEMORY_CHARS = 8_000

/** 边界小时：配置值夹在 0–23，坏值退回默认（0 是合法的：零点跨日）。 */
export function configuredDayBoundaryHour(config: Config): number {
  const raw = config.dayBoundaryHour
  return typeof raw === 'number' && Number.isFinite(raw)
    ? Math.min(23, Math.max(0, Math.floor(raw)))
    : DEFAULT_DAY_BOUNDARY_HOUR
}

/**
 * 「助手日」：把本地时间往前挪 `boundaryHour` 小时，再取那个日历日。
 *
 * 用户定的规则（2026-09-27）：跨日不在零点而在**凌晨四点** —— 深夜还在做的事，在他心里"今天"
 * 还没过去；00:30 开始的事到 04:00 之前都算前一天。这与壁纸设置里的 `assistantDay` 是**同一条
 * 规则的两侧实现**：改一处就要改另一处（P1 会加一条启动自检，不一致就报警）。
 */
export function assistantDay(now: Date = new Date(), boundaryHour: number = DEFAULT_DAY_BOUNDARY_HOUR): string {
  const boundary = Number.isFinite(boundaryHour)
    ? Math.min(23, Math.max(0, Math.floor(boundaryHour)))
    : DEFAULT_DAY_BOUNDARY_HOUR
  const shifted = new Date(now.getTime() - boundary * 3_600_000)
  return [shifted.getFullYear(), String(shifted.getMonth() + 1).padStart(2, '0'), String(shifted.getDate()).padStart(2, '0')].join('-')
}

export function localDailyWallpaperSessionId(
  now: Date = new Date(),
  boundaryHour: number = DEFAULT_DAY_BOUNDARY_HOUR,
): string {
  return `wallpaper-${assistantDay(now, boundaryHour)}`
}

/**
 * 读桌面会话的**项目记忆**（`<工作区>/项目记忆.md`）。
 *
 * 用户定的机制：说话人格（语气、称呼、边界）这类"长期有效"的要求落在这里，**每条新会话都注入
 * 一次**；范围就是桌面工作区这一处 —— 别的目录的对话不受影响（不污染它们）。读不到、读失败、
 * 内容是空的，一律当作"没有记忆"：它从来不是创建会话的前提。
 */
export async function readProjectMemory(
  workspacePath: string,
  maxChars: number = MAX_PROJECT_MEMORY_CHARS,
): Promise<string | undefined> {
  try {
    const text = (await readFile(join(workspacePath, PROJECT_MEMORY_FILE_NAME), 'utf8')).trim()
    if (!text) return undefined
    return text.length > maxChars
      ? `${text.slice(0, maxChars)}\n（记忆过长，已截断到 ${maxChars} 字：请把已失效的条目删掉或精简。）`
      : text
  } catch {
    return undefined
  }
}

function recoveredDailyWallpaperSessionId(sessionId: string): string {
  return `${sessionId}-recovered`
}

async function ensureDesktopWorkspace(registry: HostAdapter, config: Config): Promise<DesktopWorkspace> {
  const title = desktopWorkspaceTitle(config)
  const path = desktopWorkspacePath(config)
  // 按**路径**认领，而不是按标题：标题认领会让工作区停在"第一次创建时那个目录"上，
  // 用户改了位置也没用；而位置一旦丢失（比如工作区被删），标题认领会静默在一个别的地方
  // 重建同名工作区 —— 用户看到的还是「桌面会话」，目录却不是他以为的那个。
  const samePath = (candidate: string) => resolve(candidate).toLowerCase() === path.toLowerCase()
  const existing = registry.workspaces().find((workspace) => samePath(workspace.path))
  if (existing) return existing
  // 目录不存在就重建一个**同名**目录，标题仍是「桌面会话」。
  await mkdir(path, { recursive: true })
  return registry.createWorkspace(path, title)
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

/**
 * 从 `sessionPersistence.list()` 的快照里取出"可见会话 id 集合"。
 *
 * 抽成纯函数是为了能直接测：它决定"这条会话还在不在"，判错一次就会把用户的活会话判死，
 * 或者把消息继续发进已归档的会话。任何读不懂的形状一律跳过；不是数组则回答"不可知"。
 *
 * 真机实测的形状（2026-09-27，81 份快照）是
 * `{ header: { version, id, createdAt, cwd, origin, … }, revision, sizeBytes }` —— id 在
 * **`header.id`** 里。以前这里只读顶层 `id`/`sessionId`，于是 81 份快照读出来是**空集合**，
 * 而空集合的语义是"每一条会话都不可见"：归档判断因此对**所有**会话成立。这条错误比它看上去
 * 严重——它会把健康的会话判死（岛的开关切不回去、桥报错，都是这一条）。
 */
export function visibleSessionIdSet(all: unknown): Set<string> | undefined {
  if (!Array.isArray(all)) return undefined
  const ids = new Set<string>()
  for (const snapshot of all) {
    if (snapshot === null || typeof snapshot !== 'object') continue
    const record = snapshot as Record<string, unknown>
    const header = record.header !== null && typeof record.header === 'object'
      ? record.header as Record<string, unknown>
      : undefined
    const candidate = header?.id ?? record.id ?? record.sessionId
    if (typeof candidate === 'string' && candidate.trim()) ids.add(candidate.trim())
  }
  // **非空却一个 id 都读不出来**，说明快照的形状变了：那是"不可知"，不是"全都不可见"。
  // 后者会把每一条活会话都判死，所以宁可退回"不做判断"。
  if (ids.size === 0 && all.length > 0) return undefined
  return ids
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
    case 'question-required': {
      const sessionId = safeSseSessionId(event.sessionId)
      if (sessionId === undefined || event.questions.length === 0 || event.questions.length > MAX_SSE_QUESTION_COUNT) return undefined
      const questions: BridgeQuestion[] = []
      for (const question of event.questions) {
        const id = boundedSseText(question.id, MAX_SSE_IDENTIFIER_BYTES)
        const text = boundedSseText(question.question, MAX_SSE_SUMMARY_BYTES)
        if (id === undefined || text === undefined) return undefined
        const options = question.options === undefined ? undefined : question.options.slice(0, MAX_SSE_QUESTION_OPTIONS).flatMap((option): BridgeQuestionOption[] => {
          const label = boundedSseText(option.label, MAX_SSE_SUMMARY_BYTES)
          if (label === undefined) return []
          const description = option.description === undefined ? undefined : boundedSseText(option.description, MAX_SSE_SUMMARY_BYTES)
          return [{ label, ...(description === undefined ? {} : { description }) }]
        })
        const detail = question.detail === undefined ? undefined : boundedSseText(question.detail, MAX_SSE_SUMMARY_BYTES)
        const header = question.header === undefined ? undefined : boundedSseText(question.header, MAX_SSE_IDENTIFIER_BYTES)
        questions.push({
          id,
          question: text,
          ...(detail === undefined ? {} : { detail }),
          ...(header === undefined ? {} : { header }),
          ...(options?.length ? { options } : {}),
          ...(question.multiSelect === undefined ? {} : { multiSelect: question.multiSelect }),
        })
      }
      safe = { type: 'question-required', sessionId, questions }
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

/**
 * Chunks held for one subscriber while its socket is backed up.
 *
 * `res.write()` returning false means the socket's buffer crossed the high-water
 * mark — Node's documented signal that the caller should wait for `'drain'`, not
 * that the peer is gone. Treating it as fatal was a real defect: the assistant
 * message and its usage event arrive in the same tick, so the second write can
 * cross the mark and the subscriber was then destroyed *at the exact moment the
 * reply was being delivered*, leaving the user with a reply that exists in DSH
 * history but never reaches the wallpaper.
 */
const SSE_QUEUE_MAX_CHUNKS = 1024
const SSE_QUEUE_MAX_BYTES = 4 * 1024 * 1024

interface SseQueue {
  chunks: string[]
  bytes: number
  /** A `'drain'` listener is attached; further writes must be queued. */
  waitingForDrain: boolean
  onDrain?: () => void
}

const sseQueues = new WeakMap<ServerResponse, SseQueue>()

function queueFor(res: ServerResponse): SseQueue {
  let queue = sseQueues.get(res)
  if (!queue) {
    queue = { chunks: [], bytes: 0, waitingForDrain: false }
    sseQueues.set(res, queue)
  }
  return queue
}

function flushSseQueue(res: ServerResponse): void {
  const queue = queueFor(res)
  while (queue.chunks.length > 0) {
    if (res.writableEnded || res.destroyed) {
      queue.chunks.length = 0
      queue.bytes = 0
      return
    }
    const chunk = queue.chunks.shift() as string
    queue.bytes -= Buffer.byteLength(chunk, 'utf8')
    let accepted = false
    try {
      accepted = res.write(chunk)
    } catch {
      queue.chunks.length = 0
      queue.bytes = 0
      closeSseClient(res)
      return
    }
    if (!accepted) {
      // Still backed up: wait for the next drain before continuing.
      attachDrainListener(res, queue)
      return
    }
  }
  if (queue.waitingForDrain && queue.onDrain) {
    res.off?.('drain', queue.onDrain)
    queue.onDrain = undefined
    queue.waitingForDrain = false
  }
}

function attachDrainListener(res: ServerResponse, queue: SseQueue): void {
  if (queue.waitingForDrain) return
  if (typeof res.once !== 'function') return
  queue.waitingForDrain = true
  queue.onDrain = () => {
    queue.waitingForDrain = false
    queue.onDrain = undefined
    flushSseQueue(res)
  }
  res.once('drain', queue.onDrain)
}

/**
 * Write one SSE record, queueing it while the socket is backed up.
 *
 * Returns false only when the peer is genuinely gone. A full socket buffer is
 * never a reason to drop a live subscriber.
 */
function writeSseRecord(res: ServerResponse, record: string): boolean {
  if (res.writableEnded || res.destroyed) return false
  const queue = queueFor(res)
  // Anything queued means an earlier write is still waiting for drain, so order
  // must be preserved by queueing behind it rather than writing now.
  if (queue.chunks.length > 0 || queue.waitingForDrain) {
    return enqueueSseRecord(res, queue, record)
  }
  try {
    if (res.write(record)) return true
  } catch {
    // A peer can close between the state check and write().
    closeSseClient(res)
    return false
  }
  // Backpressure: hold the record and resume on drain.
  return enqueueSseRecord(res, queue, record)
}

function enqueueSseRecord(res: ServerResponse, queue: SseQueue, record: string): boolean {
  const size = Buffer.byteLength(record, 'utf8')
  if (queue.chunks.length >= SSE_QUEUE_MAX_CHUNKS || queue.bytes + size > SSE_QUEUE_MAX_BYTES) {
    // Bound memory rather than the connection's lifetime: drop what is queued and
    // resynchronise from the next event. The client reconciles from `history`
    // after a reconnect, whereas losing the subscriber loses every later turn.
    queue.chunks.length = 0
    queue.bytes = 0
  }
  queue.chunks.push(record)
  queue.bytes += size
  attachDrainListener(res, queue)
  return true
}

/**
 * Drop a subscriber and everything queued for it.
 *
 * Must run on every removal path: a lingering `'drain'` listener would keep a
 * closed response alive and could flush queued records into a dead socket.
 */
function releaseSseClient(res: ServerResponse): void {
  const queue = sseQueues.get(res)
  if (queue) {
    if (queue.onDrain) res.off?.('drain', queue.onDrain)
    queue.chunks.length = 0
    queue.bytes = 0
    queue.waitingForDrain = false
    queue.onDrain = undefined
    sseQueues.delete(res)
  }
  closeSseClient(res)
}

function sse(res: ServerResponse, event: BridgeEvent): boolean {
  const record = serializeSseEvent(event)
  if (record !== undefined) return writeSseRecord(res, record)

  // `undefined` means the event cannot be represented. That is an omission, not a
  // failure: a missing token counter, or a value outside its bound, must cost one
  // event and never the subscriber. Closing the stream here was a real defect —
  // the usage event follows the assistant message in the same tick, so an
  // unrepresentable usage payload destroyed the connection that had just received
  // the reply, and the wallpaper saw no reply at all while DSH history showed one.
  return true
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

/**
 * History is trimmed *here*, before it becomes a JSON response body. The
 * native client already rejects an oversized history payload, so letting the
 * bridge serialize megabytes only for the peer to discard them wastes the
 * wallpaper's memory and the loopback bandwidth. Newest messages win, and an
 * individual message body is never truncated.
 */
export function historyOf(session: Session, limit = MAX_HISTORY_MESSAGES, maxBytes = MAX_HISTORY_BYTES): {
  messages: Array<Record<string, unknown>>
  truncated: boolean
} {
  const all = session.deriveMessages()
    .filter(isVisibleWallpaperMessage)
    .map((message) => ({ id: message.id, role: message.role, content: contentText(message) }))
  const requested = Number.isSafeInteger(limit) && limit > 0
    ? Math.min(limit, MAX_HISTORY_MESSAGES)
    : MAX_HISTORY_MESSAGES
  const budget = Number.isSafeInteger(maxBytes) && maxBytes > 0
    ? Math.min(maxBytes, MAX_HISTORY_BYTES)
    : MAX_HISTORY_BYTES

  const kept: Array<Record<string, unknown>> = []
  let bytes = 0
  for (let index = all.length - 1; index >= 0; index -= 1) {
    if (kept.length >= requested) break
    const message = all[index]
    if (!message) break
    const size = Buffer.byteLength(JSON.stringify(message), 'utf8')
    // Always keep the newest message, even if that single body is over budget:
    // dropping it would silently lose the answer the user is reading.
    if (kept.length > 0 && bytes + size > budget) break
    bytes += size
    kept.push(message)
  }
  kept.reverse()
  return { messages: kept, truncated: kept.length < all.length }
}

/** An over-limit request keeps the connection usable but allocates nothing. */
function requireCapacity(condition: boolean, status: 429 | 409, code: string): void {
  if (!condition) throw new BridgeLimitError(status, code)
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
  /** The idle-sweep timer, owned here so teardown can clear it explicitly. */
  let sweepTimer: NodeJS.Timeout | undefined
  /** A subscriber attach or event counts as activity for the idle sweep. */
  const touch = (entry: LiveSession): void => { entry.lastActivityAt = Date.now() }
  /**
   * Release handles that no subscriber is watching. The DSH session itself is
   * durable: only this process's live agent handle goes away, and a reconnect
   * resumes the same session ID.
   */
  const sweepIdleSessions = (now = Date.now()): void => {
    if (stopping) return
    for (const [sessionId, entry] of [...live]) {
      if (entry.clients.size > 0) continue
      if (now - entry.lastActivityAt < LIVE_SESSION_IDLE_TTL_MS) continue
      live.delete(sessionId)
      void entry.handle.dispose().catch(() => undefined)
    }
  }
  const canResume = (): boolean => {
    // Keep the lightweight unit-test harness compatible while production
    // Cordis contexts use `get()` for optional service discovery.
    const get = (ctx as Context & { get?: (name: string) => unknown }).get
    return typeof get === 'function' && get.call(ctx, 'sessionPersistence') !== undefined
  }

  /**
   * 临时诊断（拿到答案就删）：把 `list()` 到底给了什么，写进桥自己的目录。
   *
   * 起因：真机上归档一条会话之后，409 从来没有发生过 —— 所以必须先分清是**我们读不到
   * `sessionPersistence`**（返回 `undefined`，调用方按"不可知"放行），还是 `list()` **本来就不
   * 排除归档会话**（那说明 DSH 的注释与实现不一致）。DSH 不落盘文本日志、`ctx.logger` 我读不到，
   * 所以证据必须落到 `~/.dsh/wallpaper/visible-sessions.json`（和 bridge-token 同一个目录）。
   */
  const writeVisibilityDiagnostic = async (payload: Record<string, unknown>): Promise<void> => {
    try {
      const directory = configuredTokenRoot(config)
      await mkdir(directory, { recursive: true })
      await writeFile(
        join(directory, 'visible-sessions.json'),
        JSON.stringify({ at: new Date().toISOString(), ...payload }, null, 2),
      )
    } catch {
      // 诊断失败绝不影响主流程：它只是取证。
    }
  }

  /**
   * DSH 侧"此刻可见的已存会话"。
   *
   * 归档一个会话之后，它就从可见集合里消失了（DSH 自己的注释写得很直白：`list()` 返回
   * "one snapshot **per visible** stored session"）—— 而**我们的活句柄还在**：这不是错误、
   * 不会掉线，灯照旧是绿的，于是消息发进一个再也看不见的会话里。用户实测报的"桌面端归档后
   * 吞输入"就是这条。
   *
   * 读不到（服务缺席或读失败）时返回 `undefined`，语义是**不可知**：调用方据此不做任何判断。
   * 宁可像以前一样工作，也不要因为一次读失败就把一个好好的会话判死。
   */
  const visibleStoredSessionIds = async (): Promise<Set<string> | undefined> => {
    const get = (ctx as Context & { get?: (name: string) => unknown }).get
    if (typeof get !== 'function') {
      void writeVisibilityDiagnostic({ outcome: 'no-context-get' })
      return undefined
    }
    const service = get.call(ctx, 'sessionPersistence') as { list?: unknown } | undefined
    if (!service || typeof service.list !== 'function') {
      void writeVisibilityDiagnostic({ outcome: 'no-session-persistence', service: service ? typeof service : 'absent' })
      return undefined
    }
    try {
      const all = await (service.list as () => Promise<unknown>).call(service)
      const ids = visibleSessionIdSet(all)
      // 临时诊断：`list()` 到底给了什么，必须落到我能读的地方（见上面那段说明）。
      void writeVisibilityDiagnostic({
        outcome: 'listed',
        isArray: Array.isArray(all),
        count: Array.isArray(all) ? all.length : undefined,
        sample: Array.isArray(all) && all.length > 0 ? JSON.stringify(all[0]).slice(0, 600) : undefined,
        visibleIds: ids ? [...ids].slice(0, 40) : undefined,
      })
      return ids
    } catch (error) {
      void writeVisibilityDiagnostic({ outcome: 'list-threw', message: String(error).slice(0, 300) })
      return undefined
    }
  }

  /** 工作区登记 + 可见性：**两个都满足**才算"这条会话还在"。 */
  const storedAndVisible = (workspace: DesktopWorkspace, sessionId: string, visible: Set<string> | undefined): boolean =>
    workspace.sessionIds.some((id) => String(id) === sessionId) && (!visible || visible.has(sessionId))

  let token = ''
  let tokenFailure: string | undefined
  // Token provisioning is the first detached asynchronous work this plugin
  // starts, and the only piece that touches the filesystem before any request.
  // Registering it is what lets teardown wait instead of racing it.
  const bootstrap = new AsyncWorkTracker()
  bootstrap.onError = () => undefined
  const tokenReady: Promise<void> = (async () => {
    try {
      token = await ensureToken(configuredTokenRoot(config))
    } catch (error) {
      tokenFailure = errorReference(error)
    }
  })()
  bootstrap.track(tokenReady)

  const publish = (sessionId: string, event: BridgeEvent): void => {
    const entry = live.get(sessionId)
    if (!entry) return
    touch(entry)
    for (const client of [...entry.clients]) {
      if (client.writableEnded || client.destroyed) {
        entry.clients.delete(client); releaseSseClient(client)
      } else if (!sse(client, event)) {
        entry.clients.delete(client); releaseSseClient(client)
      }
    }
  }

  ctx.on('session/event', (session: Session, event: SessionEvent) => {
    const id = String(session.id)
    const entry = live.get(id)
    // Session IDs are not a unique live-agent identity: another owner can
    // create a session with the same ID. Never relay its events to this
    // bridge's subscriber merely because the string happens to match.
    if (!entry || entry.handle.agent.session !== session) return
    for (const mapped of mapSessionEvent(event, id)) publish(id, mapped)
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

  // Teardown order matters and every step is idempotent, because a caller can
  // dispose the plugin more than once and because a shutdown races whatever
  // asynchronous bootstrap work was still running.
  let teardown: Promise<void> | undefined
  const teardownOnce = (): Promise<void> => {
    if (teardown) return teardown
    teardown = (async () => {
      stopping = true
      // 1. Stop accepting work. The idle sweep is the other timer that can
      //    dispose a handle, so it goes first.
      clearInterval(sweepTimer)
      // 2. Let the detached bootstrap finish. `ensureToken` spawns `whoami` and
      //    four `icacls` passes against the token directory; a caller that
      //    removes the token root as soon as this plugin is gone would
      //    otherwise delete a directory those processes still hold, which
      //    Windows reports as EBUSY against the caller's own file operation.
      await bootstrap.settle()
      // 3. Release every live handle and its subscribers.
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
    })()
    return teardown
  }

  ctx.effect(() => teardownOnce)

  /**
   * What the route table actually contains, as opposed to what this Bridge is
   * capable of in principle.
   *
   * `/status` is mounted by the `['webServer']` scope, while every session and
   * control route lives in the seven-service scope below. A host whose services
   * arrive late, or that is missing one of them, therefore has a perfectly
   * reachable status endpoint and no usable session routes. The old status
   * response announced `sessions`/`history`/`sse`/`cancel`/`approval-handoff`
   * unconditionally, so a wallpaper could see `bridge-ready` and then get a 404
   * on its first `POST /sessions`.
   *
   * Capabilities are now derived from this record after registration succeeds.
   */
  const registered = { control: false, sessions: false }
  const statusDispose = { current: undefined as (() => void) | undefined }
  const controlDispose = { current: undefined as (() => void) | undefined }
  const sessionRoutesDispose = { current: undefined as (() => void) | undefined }
  /**
   * Set when the composed host does not expose the shape the adapter validates.
   * Reported instead of `bridge-loading`, because waiting cannot fix it and the
   * user's action is to update DSH or the Bridge.
   */
  let hostIncompatibility: { code: string; detail: string } | undefined

  /**
   * What the mounted route table can *structurally* honour.
   *
   * Two deliberate distinctions, both load-bearing for the consumer:
   *
   *  - `capabilities` describes what is mounted, not what is safe to use. An
   *    unusable token still leaves the session routes registered (they refuse
   *    per request with 503), so `sessions` may legitimately appear alongside
   *    `authentication: "unavailable"`. Both interpreters check `authentication`
   *    before capability matching, so such a document can never be read as
   *    `bridge-ready`; suppressing the capability instead would erase the
   *    difference between "mounted but unauthenticated" and "not mounted".
   *  - An incompatible *host* is different: no route was registered, so nothing
   *    beyond the diagnostic surface is announced.
   */
  const liveCapabilities = (): string[] => {
    const capabilities = ['status']
    if (hostIncompatibility) return capabilities
    if (registered.control) capabilities.push('control')
    if (registered.sessions) {
      capabilities.push('sessions', 'history', 'sse', 'cancel', 'approval-handoff')
      // DSH exposes session persistence as an optional service. Announcing
      // `resume` without it would make the wallpaper offer a resume that can
      // only fail with `resume-unavailable`.
      if (canResume()) capabilities.push('resume')
    }
    return capabilities
  }

  /**
   * State the wallpaper renders. `bridge-loading` is a first-class answer, not
   * a failure: it means "this Bridge exists, wait for it". An incompatible host
   * is a different answer, because no amount of waiting changes it.
   */
  const bridgeState = (): { state: string; reasonCode: string } => {
    if (tokenFailure !== undefined) return { state: 'bridge-auth-unavailable', reasonCode: 'token-unavailable' }
    // The specific member is part of the reason code on purpose: it is the only
    // actionable part, and it is a compile-time identifier, never host data.
    // Collapsing it to a bare `host-shape-mismatch` would leave the wallpaper
    // able to say "incompatible" but not which service to report.
    if (hostIncompatibility) {
      return {
        state: 'bridge-incompatible',
        reasonCode: `${hostIncompatibility.code}:${hostIncompatibility.detail}`,
      }
    }
    if (registered.control && registered.sessions) return { state: 'bridge-ready', reasonCode: 'ready' }
    return { state: 'bridge-loading', reasonCode: 'services-pending' }
  }

  // Status must not depend on optional session-control services.  A Web
  // profile may take longer to compose commands, presets, or workspaces than
  // its HTTP listener; reporting the bridge as missing during that interval
  // makes the wallpaper's route toggle misleading.  The mutating routes
  // below still wait for their complete service set.
  ctx.inject(['webServer'], (statusContext) => {
    if (!isLoopbackWebServerHost(statusContext.webServer.host)) return
    const dispose = statusContext.webServer.register({
      kind: 'exact',
      path: `${API_PREFIX}/status`,
      handler: async (_req, res) => {
        await tokenReady
        const { state, reasonCode } = bridgeState()
        json(res, 200, {
          // The Bridge's own release version. This used to report a separate
          // hardcoded contract number, so the status of an installed copy could
          // not be matched against its package version or build output.
          bridgeVersion: BRIDGE_VERSION,
          bridgeBuild: BRIDGE_BUILD,
          protocolVersion: BRIDGE_PROTOCOL_VERSION,
          dsh: 'online',
          // The DSH API surface this build was compiled against. DSH exposes no
          // runtime version, so this is the only version claim the Bridge can
          // actually prove; see `BRIDGE_AUTHORED_AGAINST` and the compatibility
          // matrix in bridge/README.md.
          authoredAgainst: BRIDGE_AUTHORED_AGAINST,
          state,
          reasonCode,
          // Only what the mounted route table can honour right now.
          capabilities: liveCapabilities(),
          authentication: tokenFailure === undefined ? 'ready' : 'unavailable',
          ...(tokenFailure === undefined ? {} : { tokenReference: tokenFailure }),
        })
      },
    })
    statusDispose.current = dispose
    statusContext.effect(() => () => {
      statusDispose.current = undefined
      dispose()
    })
  })

  // The HTTP handlers create and resume standard DSH agents.  Keep both
  // services in this child scope explicitly: a test mock that happens to
  // expose `agents` next to `webServer` must not hide a real Cordis scope
  // where only the declared dependencies are available.
  ctx.inject([...HOST_ADAPTER_SERVICES], (wctx) => {
    if (!isLoopbackWebServerHost(wctx.webServer.host)) return
    /**
     * Every piece of published state in this scope is revoked here, and this
     * dispatcher is registered *before* anything can go wrong. An early `return`
     * on an incompatible host would otherwise skip the only cleanup hook, and a
     * stale `bridge-incompatible` would outlive the scope that recorded it —
     * which is exactly what a regression test caught.
     */
    const rollbackRegistrations = (): void => {
      registered.control = false
      registered.sessions = false
      hostIncompatibility = undefined
      controlDispose.current?.()
      controlDispose.current = undefined
      sessionRoutesDispose.current?.()
      sessionRoutesDispose.current = undefined
    }
    wctx.effect(() => rollbackRegistrations)
    /**
     * Build the validated host adapter once for this scope.
     *
     * A host that does not expose the shape the Bridge drives throws
     * `HostIncompatibleError` here, before any route is registered. That is
     * deliberate: the status route stays mounted and reports
     * `bridge-incompatible`, instead of the wallpaper sending a session request
     * into a handler that will fail on an unexpected host shape mid-creation.
     */
    let host: HostAdapter
    try {
      host = createHostAdapter(wctx as unknown as Context)
    } catch (error) {
      const detail = error instanceof HostIncompatibleError ? error.detail : 'unknown'
      hostIncompatibility = { code: 'host-shape-mismatch', detail }
      // Surfaced for diagnosis (non-sensitive: the member name only), so a
      // support report names the service that did not match.
      wctx.logger.error?.(`wallpaper bridge host is incompatible (${detail})`)
      return
    }
    hostIncompatibility = undefined
    /**
     * A route-registration failure is not fatal to the Bridge: the status route
     * stays mounted so the wallpaper can still diagnose "the Bridge is here but
     * unusable" instead of seeing an unresponsive port. But a *partial*
     * registration must not be announced, so a failure rolls back whatever
     * already registered before the error leaves this scope.
     */
    try {
    const controlDisposeFn = wctx.webServer.register({
      kind: 'prefix',
      path: `${API_PREFIX}/control`,
      handler: async (req, res) => {
        await tokenReady
        if (tokenFailure !== undefined) return json(res, 503, { error: 'bridge-token-unavailable', reference: tokenFailure })
        if (!bearerAuthorized(req.headers.authorization, token)) return json(res, 401, { error: 'unauthorized' })
        const pathname = new URL(req.url ?? '/', 'http://127.0.0.1').pathname
        try {
          if (pathname === `${API_PREFIX}/control/models`) {
          if (req.method === 'POST') {
            // Changing the host's default model is what keeps the wallpaper and
            // the host in step: a session started from the DSH UI afterwards uses
            // the same model the wallpaper shows. The host persists it itself.
            const body = await readJson(req)
            const model = typeof body.model === 'string' ? body.model.trim() : ''
            if (!model) return json(res, 400, { error: 'model-required' })
            const directory = await host.modelDirectory()
            const provider = typeof body.provider === 'string' && body.provider.trim()
              ? body.provider.trim()
              : (directory.current?.provider ?? '')
            if (!provider) return json(res, 400, { error: 'model-provider-unknown' })
            const applied = await host.setDefaultModel(
              { provider, model },
              directory.supported ? directory.models.map((entry) => entry.id) : [],
            )
            if (!applied) return json(res, 501, { error: 'model-selection-unsupported' })
            return json(res, 200, { provider, model })
          }
          if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
          // The host's own catalog, never a Bridge-side guess: a picker listing
          // ids the host would reject is worse than a picker with one row.
          const directory = await host.modelDirectory()
          return json(res, 200, directory)
          }
          if (pathname === `${API_PREFIX}/control/presets`) {
          if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
          const presets = await host.presetDirectory()
          const defaultPresetId = host.defaultPresetId()
          // Preset composition paths are host-private. Expose only the
          // metadata needed to render a picker and explain unavailable rows.
          return json(res, 200, {
            presets: presets.map((preset) => ({
              id: preset.id,
              ...(preset.name ? { name: preset.name } : {}),
              ...(preset.description ? { description: preset.description } : {}),
              trust: preset.trust,
              ...(preset.broken ? { broken: preset.broken } : {}),
              isDefault: preset.id === defaultPresetId,
            })),
          })
          }
          const presetSwitch = pathname.match(new RegExp(`^${API_PREFIX}/control/sessions/([^/]+)/preset$`))
          const sessionControls = pathname.match(new RegExp(`^${API_PREFIX}/control/sessions/([^/]+)$`))
          if (sessionControls) {
            // A known path with the wrong verb is a client error (405), not a
            // missing route (404). The session scope already answered this way,
            // so the two halves of the same interface must agree.
            if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
            const sessionId = decodeURIComponent(sessionControls[1] ?? '')
            if (!isSafeSessionId(sessionId)) return json(res, 400, { error: 'invalid-session-id' })
            const entry = live.get(sessionId)
            if (!entry) return json(res, 404, { error: 'session-not-live' })
            return json(res, 200, {
              // DSH derives this display value from the durable event stream, so
              // the adapter takes events rather than the Session object. Hosts
              // that expect a Session are handled inside the adapter.
              permission: {
                current: host.currentPermission(entry.handle.agent.session.events),
                options: host.permissionNames(),
              },
              commands: host.commandsFor(entry.handle.agent).map((command) => ({ name: command.name, description: command.description, ...(command.input ? { input: command.input } : {}) })),
            })
          }
          const permissionSwitch = pathname.match(new RegExp(`^${API_PREFIX}/control/sessions/([^/]+)/permission$`))
          if (permissionSwitch) {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            const sessionId = decodeURIComponent(permissionSwitch[1] ?? '')
            if (!isSafeSessionId(sessionId)) return json(res, 400, { error: 'invalid-session-id' })
            const entry = live.get(sessionId)
            if (!entry) return json(res, 404, { error: 'session-not-live' })
            const body = await readJson(req)
            const permission = typeof body.permission === 'string' ? body.permission.trim() : ''
            if (!host.permissionNames().includes(permission)) return json(res, 400, { error: 'unknown-permission-preset' })
            host.setPermission(entry.handle.agent.session, permission)
            return json(res, 200, { sessionId, permission })
          }
          if (!presetSwitch) return json(res, 404, { error: 'not-found' })
          if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
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
          const selected = (await host.presetDirectory()).find((candidate) => candidate.id === preset)
          if (!selected) return json(res, 400, { error: 'unknown-agent-preset' })
          if (selected.broken) return json(res, 409, { error: 'agent-preset-unavailable' })
          await host.recomposePreset(entry.handle.agent.ctx, preset)
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

    controlDispose.current = controlDisposeFn
    // `registered.control` is deliberately *not* set here. The two route groups
    // are announced together at the end of the scope, because a Bridge that
    // advertises `control` while `sessions` is still unregistered would let the
    // wallpaper drive a half-composed route table.

    const sessionsDisposeFn = wctx.webServer.register({
      kind: 'prefix',
      path: `${API_PREFIX}/sessions`,
      handler: async (req, res) => {
        await tokenReady
        if (tokenFailure !== undefined) return json(res, 503, { error: 'bridge-token-unavailable', reference: tokenFailure })
        if (!bearerAuthorized(req.headers.authorization, token)) return json(res, 401, { error: 'unauthorized' })
        // Tokens stay valid while the bridge is alive, so a request that lands
        // after teardown must be refused rather than creating a handle this
        // process will never dispose. Checked after authentication so a
        // shutdown never turns into an information leak about the bridge state.
        if (stopping) return json(res, 503, { error: 'bridge-shutting-down' })
        const url = new URL(req.url ?? '/', 'http://127.0.0.1')
        const route = parseSessionRoute(url.pathname)
        if (!route) return json(res, 404, { error: 'not-found' })
        let stage = 'route'
        try {
          if (route.kind === 'collection') {
            if (req.method !== 'POST') return json(res, 405, { error: 'method-not-allowed' })
            stage = 'request'
            const body = await readJson(req)
            const requested = typeof body.sessionId === 'string' ? body.sessionId.trim() : ''
            const requestedResume = typeof body.resumeSessionId === 'string' ? body.resumeSessionId.trim() : ''
            const automaticDailySession = !requested && !requestedResume
            // Every session created by this bridge belongs to the dedicated
            // desktop workspace. Apart from keeping the cwd boundary
            // consistent, this gives resumeSessionId a durable ownership
            // check instead of accepting any well-shaped DSH session ID.
            const workspace = await ensureDesktopWorkspace(host, config)
            stage = 'session-identity'
            // 可见性只在**每个请求开头读一次**：归档一个会话之后，用户期望的是"下次说话时
            // 自动换一条新的"，而不是继续往看不见的那条里发（用户实测报的"吞输入"）。
            const visible = await visibleStoredSessionIds()
            const dailyId = localDailyWallpaperSessionId(new Date(), configuredDayBoundaryHour(config))
            const recoveredDailyId = recoveredDailyWallpaperSessionId(dailyId)
            const recoveredDailyExists = automaticDailySession
              && storedAndVisible(workspace, recoveredDailyId, visible)
            const id = automaticDailySession
              ? (recoveredDailyExists ? recoveredDailyId : dailyId)
              : requestedResume || requested || `wallpaper-${randomUUID()}`
            if (!isSafeSessionId(id)) return json(res, 400, { error: 'invalid-session-id' })
            const ownedResume = storedAndVisible(workspace, id, visible)
            // An unmatched resume ID is rewritten to a fresh session before the
            // live lookup, so a client that simply reconnects must resolve to
            // the live handle it already owns rather than being told the
            // (nonexistent) resume is unavailable. Without this order, a
            // reconnect while persistence is unavailable answered 409 even
            // though the very same session was live in this process.
            const effectiveIdRequest = (requestedResume || automaticDailySession) && !ownedResume
              ? `wallpaper-${randomUUID()}`
              : id
            const existing = live.get(effectiveIdRequest)
            if (existing) {
              touch(existing)
              return json(res, 200, sessionSummary(existing))
            }
            const resume = requestedResume || automaticDailySession
              ? (ownedResume ? id : '')
              : ''
            if (requestedResume && !resume) return json(res, 409, { error: 'resume-unavailable' })
            if (resume && !canResume()) return json(res, 409, { error: 'resume-unavailable' })
            // Refuse rather than allocate once this bridge already owns its
            // full set of live handles, or once the in-flight creations could
            // exceed that set. 429 is the actionable stable answer.
            requireCapacity(live.size < MAX_LIVE_SESSIONS, 429, 'too-many-live-sessions')
            requireCapacity(creating.size < MAX_PENDING_CREATIONS, 429, 'too-many-pending-sessions')
            const provider = typeof body.provider === 'string' && body.provider.trim() ? body.provider.trim() : undefined
            const model = typeof body.model === 'string' && body.model.trim() ? body.model.trim() : undefined
            // `agents.create()` is intentionally low-level: unlike the
            // Web API gateway it does not apply the host's default model on
            // behalf of a caller. A blank provider/model would therefore let
            // a session start and only fail later when `{{model}}` is rendered
            // in the deployment persona. Read the host-owned selection here;
            // request values remain an explicit override for future clients.
            const defaults = host.defaultModel()
            const preset = typeof body.agentPreset === 'string' && body.agentPreset.trim()
              ? body.agentPreset.trim()
              : host.defaultPresetId()
            const availablePresets = await host.presetDirectory()
            const selectedPreset = availablePresets.find((candidate) => candidate.id === preset)
            if (!selectedPreset) return json(res, 400, { error: 'unknown-agent-preset' })
            if (selectedPreset.broken) return json(res, 409, { error: 'agent-preset-unavailable' })
            const permission = desktopPermission(config)
            if (!host.permissionNames().includes(permission)) {
              return json(res, 409, { error: 'desktop-permission-unavailable', permission })
            }
            // 项目记忆（`<工作区>/项目记忆.md`）：每条新会话注入一次；读不到就是"没有记忆"。
            const memory = await readProjectMemory(workspace.path)
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
            const cwd = workspace.path
            const workspaceTitle = workspace.title
            let pending = creating.get(id)
            const createdByThisRequest = pending === undefined
            if (!pending) {
              pending = (async (): Promise<LiveSession> => {
                if (stopping) throw new Error('wallpaper bridge is shutting down')
                let effectiveId = id
                let handle
                // 项目记忆 + 今天的助手日：**每条新会话都注入一次**（用户定的机制，范围只有桌面
                // 工作区这一处）。读记忆失败也要照常建会话 —— 它不是前提。三次创建路径共用这一份
                // setup，避免"三条路径三份措辞"。
                const setup = desktopAgentSetup(host, preset, cwd, workspaceTitle, permission, {
                  day: assistantDay(new Date(), configuredDayBoundaryHour(config)),
                  memoryPath: join(workspace.path, PROJECT_MEMORY_FILE_NAME),
                  ...(memory ? { memory } : {}),
                })
                if (resume) {
                  try {
                    stage = 'resume'
                    handle = await host.resumeAgent({
                      resumeSessionId: SessionId(id),
                      provider: agentOptions.provider,
                      model: agentOptions.model,
                      setup,
                    })
                  } catch (error) {
                    if (!automaticDailySession) throw error
                    // The daily wallpaper session is a disposable recovery
                    // boundary. DSH can wrap persistence, projection, or
                    // composition failures so the corruption marker is only
                    // present in a nested cause (and older builds may omit it
                    // altogether). Preserve the old log for diagnosis, remove
                    // only its workspace pointer, and create a deterministic
                    // replacement so the desktop entry remains usable.
                    void error
                    await workspace.detachSession(SessionId(id))
                    effectiveId = recoveredDailyWallpaperSessionId(dailyId)
                    handle = await host.createAgent({
                      sessionId: SessionId(effectiveId),
                      cwd,
                      agentPreset: preset,
                      provider: agentOptions.provider,
                      model: agentOptions.model,
                      setup,
                    })
                  }
                } else {
                  // 我们选的那个 id 在会话存储里**已经存在**时，不能直接 create：DSH 会拒绝
                  // 同 id 重复的会话。真机上这条路径正是这样 500 的 —— 桌面会话目录搬进数据目录
                  // 之后，工作区注册表里不再有旧的日会话，于是 `resume` 为空，我们又原样拿它的 id
                  // 去 create（存储里那条还在，只是不再归我们）。挑一个**确定空闲**的 id：先试既有的
                  // 确定性替代 `-recovered`，它也被占了才退到随机 id。
                  if (!resume && visible?.has(effectiveId)) {
                    const recoveredId = recoveredDailyWallpaperSessionId(dailyId)
                    effectiveId = visible.has(recoveredId) ? `wallpaper-${randomUUID()}` : recoveredId
                    wctx.logger.info?.(
                      `wallpaper bridge: session id ${id} already exists in storage; creating ${effectiveId} instead`,
                    )
                  }
                  stage = 'create'
                  handle = await host.createAgent({
                    sessionId: SessionId(effectiveId),
                    cwd,
                    agentPreset: preset,
                    provider: agentOptions.provider,
                    model: agentOptions.model,
                    setup: desktopAgentSetup(host, preset, cwd, workspaceTitle, permission),
                  })
                }
                // New desktop sessions start with the narrow workspace-write
                // boundary. Resumed sessions keep the user's explicit DSH
                // permission choice instead of silently overriding it.
                if (!resume || effectiveId !== id) {
                  stage = 'permission'
                  try {
                    host.setPermission(handle.agent.session, permission)
                  } catch (error) {
                    await handle.dispose().catch(() => undefined)
                    throw error
                  }
                }
                // Plugin teardown may have started while DSH created the
                // agent. Dispose it rather than leaving a live handle that no
                // route can own or clean up.
                if (stopping) {
                  await handle.dispose().catch(() => undefined)
                  throw new Error('wallpaper bridge is shutting down')
                }
                const entry: LiveSession = { handle, clients: new Set<ServerResponse>(), lastActivityAt: Date.now(), createdAt: Date.now() }
                live.set(effectiveId, entry)
                if (!resume || effectiveId !== id) await workspace.attachSession(SessionId(effectiveId))
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

          // 归档的会话在我们这边**看不出任何异常**：句柄还在、灯还是绿的。所以在服务任何一条
          // 会话路由之前先核对一次可见性；确认不可见就丢掉句柄，并用一个可区分的错误回答，
          // 让壁纸知道该换一条新会话了 —— 而不是继续往黑洞里发消息（用户实测报的"吞输入"）。
          //
          // 宽限期：**我们自己刚建出来的**会话不可能已经被归档，而宿主那边的可见列表可能还没来得及
          // 包含它（smoke 里就是这样）。那时按"不可见"判死，等于把一条健康的会话丢掉。
          const recentlyCreated = Date.now() - entry.createdAt < ARCHIVED_VISIBILITY_GRACE_MS
          const stillVisible = recentlyCreated ? undefined : await visibleStoredSessionIds()
          if (stillVisible && !stillVisible.has(route.sessionId)) {
            live.delete(route.sessionId)
            await entry.handle.dispose().catch(() => undefined)
            wctx.logger.info?.(`wallpaper bridge: session ${route.sessionId} is no longer visible (archived); released (age ${Date.now() - entry.createdAt} ms, grace ${ARCHIVED_VISIBILITY_GRACE_MS} ms, ${stillVisible.size} visible)`)
            return json(res, 409, { error: 'session-archived', sessionId: route.sessionId })
          }

          if (route.kind === 'history') {
            if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
            const query = new URL(req.url ?? '', 'http://127.0.0.1').searchParams
            const limit = Number.parseInt(query.get('limit') ?? '', 10)
            const maxBytes = Number.parseInt(query.get('maxBytes') ?? '', 10)
            const history = historyOf(entry.handle.agent.session, limit, maxBytes)
            touch(entry)
            return json(res, 200, {
              sessionId: route.sessionId,
              messages: history.messages,
              truncated: history.truncated,
              limits: { maxMessages: MAX_HISTORY_MESSAGES, maxBytes: MAX_HISTORY_BYTES },
            })
          }
          if (route.kind === 'events') {
            if (req.method !== 'GET') return json(res, 405, { error: 'method-not-allowed' })
            // One wallpaper reader per handle is the normal case. A second
            // subscriber is only ever a reconnect overlap, so a small cap
            // catches a leaking client before it can multiply the write fan-out.
            requireCapacity(
              entry.clients.size < MAX_SSE_CLIENTS_PER_SESSION,
              409,
              'too-many-subscribers',
            )
            res.statusCode = 200
            res.setHeader('content-type', 'text/event-stream; charset=utf-8')
            res.setHeader('cache-control', 'no-store')
            res.setHeader('connection', 'keep-alive')
            // Subscribe before returning any data so a turn submitted during
            // connection setup cannot race past the client. The snapshot then
            // brings a newly attached reader up to the current state.
            entry.clients.add(res)
            touch(entry)
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
              entry.clients.delete(res); releaseSseClient(res)
              return
            }
            res.flushHeaders()
            const heartbeat = setInterval(() => {
              if (!heartbeatSse(res)) {
                clearInterval(heartbeat)
                entry.clients.delete(res); releaseSseClient(res)
              }
            }, 15_000)
            req.on('close', () => {
              clearInterval(heartbeat)
              entry.clients.delete(res); releaseSseClient(res)
            })
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
              await host.executeCommand(entry.handle.agent, text, new AbortController().signal)
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
          if (error instanceof BridgeLimitError) return json(res, error.status, { error: error.code })
          if (error instanceof SyntaxError || error instanceof RangeError) return malformed(res, error, wctx.logger)
          const reference = errorReference(error)
          wctx.logger.warn(`wallpaper bridge request failed (${reference})`)
          return json(res, 500, { error: 'bridge-error', reference, stage })
        }
      },
    })
    sessionRoutesDispose.current = sessionsDisposeFn
    // Both groups are registered, so the route table is now whole and can be
    // announced. This is the only place either flag becomes true.
    registered.control = true
    registered.sessions = true
    } catch (error) {
      // Registration failed part-way. Revoke everything and rethrow: the scope
      // is then consistently "not mounted", and Cordis reports the failure
      // while the status route keeps answering `bridge-loading`.
      rollbackRegistrations()
      throw error
    }

    wctx.effect(() => {
      // Release live agent handles nobody is watching. This is a background
      // timer with a bounded per-tick cost (it only walks `live`, never a
      // session's message list) and it is cleared on teardown with the routes.
      sweepTimer = setInterval(() => sweepIdleSessions(), LIVE_SESSION_SWEEP_INTERVAL_MS)
      if (typeof sweepTimer.unref === 'function') sweepTimer.unref()
      return () => {
        clearInterval(sweepTimer)
        sweepTimer = undefined
        // Revoke the announcement before the routes disappear, so a status
        // request racing this teardown cannot be told about capabilities that
        // are already gone. (`rollbackRegistrations` is also registered by its
        // own effect at the top of this scope, so the incompatible-host path
        // still revokes; calling it twice is safe.)
        rollbackRegistrations()
      }
    })
  })
}
export default apply
