/**
 * Discovery of the DSH instances a wallpaper may connect to.
 *
 * The wallpaper used to probe exactly one hardcoded address
 * (`http://127.0.0.1:3080`). That works only for a Host started from the CLI,
 * because the three client shapes ship different ports and only some of them are
 * configurable:
 *
 * | client                | default port | configurable |
 * | --------------------- | ------------ | ------------ |
 * | official desktop shell| 19387        | no — compiled into its asar |
 * | DSH CLI / core        | 3080         | yes (`--port`) |
 *
 * （第三方桌面客户端曾经也在这张表里，2026-09-27 按用户要求移除：实测它把整台本地
 * HTTP 服务放在自己的授权之后，壁纸用 bridge token 打过去一律 403，连 `/` 都进不去 ——
 * 留着只会让"可选择的主体"里有一个永远点不亮的东西。）
 *
 * So "connect to 3080" silently means "connect only to the CLI shape", and a user
 * running the official shell sees `offline` while a perfectly ready wallpaper
 * Bridge is listening one port away. Discovery replaces that assumption with a
 * scanned candidate list plus an explicit user choice.
 *
 * Nothing here reads a credential: identification uses the Bridge's public
 * `/status`, and a candidate that answers with something else is reported as
 * `web-only` rather than assumed to be ours.
 *
 * Discovery and *the right to use* a candidate are separate questions, and only
 * the second one is binding: scanning may look at every port (a client can be
 * moved), while `selectEndpoint` may only ever return a port the settings
 * configure. A ready Bridge found outside that set belongs to a different subject,
 * and connecting to it is how the wallpaper used to answer a session the user
 * never chose.
 */
import { interpretHarnessBridgeStatus, type HarnessStatus } from './harness.ts'

/** The client shapes, in the priority order the wallpaper defaults to. */
export type HarnessClientKind = 'official-desktop' | 'official-web'

export interface HarnessEndpointCandidate {
  /** TCP port on 127.0.0.1. */
  port: number
  /** Which client shape this port belongs to, when it can be determined. */
  kind: HarnessClientKind
  /** Whether this candidate came from a default, a config file, or the user. */
  source: 'default' | 'configured' | 'user'
  /** Probe result. `bridge-ready` (etc.) means a Bridge answered here. */
  status: HarnessStatus
  /** True when a wallpaper Bridge answered, whatever its state. */
  bridgeFound: boolean
}

/** A stable, non-sensitive label for the settings dropdown. */
export function endpointKindLabel(kind: HarnessClientKind): string {
  switch (kind) {
    case 'official-desktop': return '桌面客户端'
    default: return 'Web / CLI'
  }
}

/**
 * Priority used whenever several endpoints are usable. The user's stated order:
 * the official desktop client first, then plain official web/CLI. Lower sorts first.
 */
export function endpointPriority(kind: HarnessClientKind): number {
  switch (kind) {
    case 'official-desktop': return 0
    default: return 1
  }
}

/**
 * Built-in candidates, and the reason each port is here rather than discovered.
 * The `kind` is an expectation to be confirmed by the probe: if the official
 * shell is not the thing listening on 19387, the candidate is still reported
 * honestly as whatever actually answered.
 */
export const DEFAULT_ENDPOINT_PORTS: ReadonlyArray<{ port: number; kind: HarnessClientKind }> = [
  { port: 19387, kind: 'official-desktop' },
  { port: 3080, kind: 'official-web' },
]

/**
 * The shell subjects this build knows, with the port each one carries.
 *
 * A shell compiles its port into its own package, so for a shell the port *is*
 * part of the client: it is the one endpoint fact that belongs to a subject rather
 * than to a scan. The two AUMIDs and their ports are the same pairs
 * `harness_targets.rs` holds in `SHELL_APPS`, and the settings surface can never
 * let a user type one — a scan produces them. A third copy is the price of the
 * browser preview having no native side at all, so `endpoints.spec.ts` pins these
 * pairs against `DEFAULT_ENDPOINT_PORTS` above: a change to either list fails
 * loudly instead of silently scoping a subject to a port nobody listens on.
 */
const SHELL_SUBJECTS: Readonly<Record<string, { kind: HarnessClientKind; ports: readonly number[] }>> = {
  'com.deepseek.dsh': { kind: 'official-desktop', ports: [19387] },
}

/**
 * The subject a stored choice must fall back to when this build no longer supports it.
 *
 * 2026-09-27：第三方桌面客户端被移除（它把本地接口锁在自己的授权后面，壁纸一律 403 ✓）。
 * 可是**用户设置里可能还存着它的 AUMID** —— 如果就这么放着，主体下拉里没有它、端点卡片
 * 只显示"已配置但无处可看"，用户看到的是一个永远点不亮的灯，而且没有任何解释 ✗。
 *
 * 所以：一个**壳形态、但本 build 认不出**的存储值，落回官方桌面客户端 ✓，并把原因说出来 ✓。
 * 返回 `null` 表示"不用动"：没有存值、存的是源码目录（路径永远是合法的身份 ✓）、
 * 或者存的是本 build 认识的主体 ✓。
 */
export function unsupportedShellSubjectFallback(
  subjectId: string | undefined,
): { subjectId: string; notice: string } | null {
  const subject = (subjectId ?? '').trim()
  if (subject === '' || !subject.startsWith(SHELL_SUBJECT_PREFIX)) return null
  if (subjectClientKind(subject) !== undefined) return null
  return {
    subjectId: OFFICIAL_SHELL_SUBJECT_ID,
    notice:
      '原先选定的桌面客户端已不再受支持：它把本地接口锁在自己的授权后面，壁纸请求一律被拒绝；'
      + '已切回官方桌面客户端。',
  }
}

/**
 * The subject-id namespace for a shell that carries its own checkout.
 *
 * The same prefix as `harnessSubjects.isEmbeddedShellSubject`; the two are pinned
 * against each other in the tests, because a drift here would classify a shell as
 * a source tree and hand it a checkout's port.
 */
export const SHELL_SUBJECT_PREFIX = 'shell:'

/**
 * The official desktop client's subject id — the one subject this build always knows.
 *
 * Spelled here rather than at each call site because the fallback above *stores* it:
 * a typo would persist a subject no scan ever produces. `endpoints.spec.ts` pins it
 * against `SHELL_SUBJECTS`, so the two cannot drift apart.
 */
export const OFFICIAL_SHELL_SUBJECT_ID = `${SHELL_SUBJECT_PREFIX}com.deepseek.dsh`

/**
 * 一条**不属于当前主体**的显式端点（存量的自相矛盾）。
 *
 * "用户的 pin 优先"是刻意的规则，不动它。但那条 pin 是**用户当年为那个主体选的端口**：换主体后
 * 它继续生效，就变成"按旧主体的端口去开新主体的界面"——实测过一次：主体是只该用 3080 的已安装
 * CLI，pin 还停在官方客户端的 19387，于是点「打开」把官方客户端的窗口拉到了前台。
 *
 * 这里只做判定，不修改任何东西：改正该由调用方**说出来**再做（与"已不受支持的主体"同一套做法）。
 * 返回 `undefined` 表示不算矛盾——没有存值、没有主体（这条 pin 无所属）、或 pin 就在本主体自己的
 * 端口里。
 */
export function staleEndpointPort(launch: {
  subjectId?: string
  rootPath?: string
  endpointPort?: number
}): number | undefined {
  const pinned = launch.endpointPort
  if (pinned === undefined) return undefined
  const allowed = subjectEndpointPorts({ ...endpointScopeOf(launch), endpointPort: undefined })
  if (!allowed) return undefined
  return allowed.includes(pinned) ? undefined : pinned
}

/**
 * The subject-id namespace for a **globally installed** DSH CLI (`npm i -g @deepseek-ai/dsh`).
 *
 * A third class, not a second spelling of a checkout: it has no source tree to name, so
 * its id names the launcher on `PATH` instead. The native side spells this prefix in
 * `harness_targets.rs::CLI_ID_PREFIX`, and `endpoints.spec.ts` pins the two together.
 */
export const CLI_SUBJECT_PREFIX = 'cli:'

/**
 * DSH's own web default: the port a source checkout listens on unless the user
 * moved it with `--port`.
 */
export const CHECKOUT_ENDPOINT_PORT =
  DEFAULT_ENDPOINT_PORTS.find((entry) => entry.kind === 'official-web')?.port ?? 3080

/**
 * What the settings say the wallpaper may talk to.
 *
 * The user chooses a *subject* (a shell, or a source tree) — never a port. The
 * port is where that subject happens to answer, which is why it may only ever be
 * read from this scope and never inferred from what a scan found listening. The
 * three fields travel together because they are one decision: which subject, the
 * user's explicit override, and the ports they added by hand for a checkout.
 */
export interface EndpointScope {
  /** `shell:<aumid>` for a client that carries its own checkout, else a tree's path. */
  subjectId?: string
  /** The user's explicit endpoint choice, which outranks the subject. */
  endpointPort?: number
  /** Ports the user added for a checkout that does not listen on the default. */
  extraPorts?: readonly number[]
}

function usablePorts(ports: readonly number[]): number[] {
  const unique: number[] = []
  for (const port of ports) {
    if (!Number.isInteger(port) || port < 1 || port > 65535 || unique.includes(port)) continue
    unique.push(port)
  }
  return unique
}

/**
 * The ports a configured subject may be reached on, or `undefined` when nothing
 * is configured and the shipped priority order applies.
 *
 * This is the whole of the "no substitution" rule. A subject's set is decided by
 * the subject alone:
 *
 * * an explicit pin is one port, the user's own statement about where their DSH is;
 * * a shell owns the port compiled into it, and nothing else — a shell cannot be
 *   moved to another port, so a second port for it would mean another client;
 * * a source tree listens on DSH's default, plus any port the user added for it by
 *   hand. An added port is admissible because adding it *is* the user telling the
 *   wallpaper which subject answers there; anything a scan merely found answering
 *   is not, which is why discovery can never widen this set.
 *
 * `undefined` (nothing chosen yet) is not the same as `[]` (chosen, but this build
 * cannot say where it answers): the first keeps the shipped priority order, the
 * second means the configured subject is unreachable.
 */
export function subjectEndpointPorts(scope: EndpointScope): number[] | undefined {
  if (scope.endpointPort !== undefined) {
    return usablePorts([scope.endpointPort])
  }
  const subject = (scope.subjectId ?? '').trim()
  if (!subject) return undefined
  if (subject.toLowerCase().startsWith(SHELL_SUBJECT_PREFIX)) {
    const aumid = subject.slice(SHELL_SUBJECT_PREFIX.length).trim().toLowerCase()
    // An AUMID this build does not know has no port to offer. Reporting that as
    // "nothing configured" would put the wallpaper back on the priority order —
    // i.e. on a client the user did not choose.
    return usablePorts(SHELL_SUBJECTS[aumid]?.ports ?? [])
  }
  // 已安装的 CLI 与源码检出同形状（都 boot 一个 profile、都在 DSH 自己的默认端口上服务 ✓）：
  // 它只是没有树可指 ✗。所以端口规则与检出一致。
  if (subject.toLowerCase().startsWith(CLI_SUBJECT_PREFIX)) {
    return usablePorts([CHECKOUT_ENDPOINT_PORT, ...(scope.extraPorts ?? [])])
  }
  return usablePorts([CHECKOUT_ENDPOINT_PORT, ...(scope.extraPorts ?? [])])
}

/** True when the settings name the subject whose endpoints may be used. */
export function endpointScopeConfigured(scope: EndpointScope): boolean {
  return subjectEndpointPorts(scope) !== undefined
}

/**
 * The scope a stored launch configuration describes.
 *
 * The one place the four stored fields become a scope, so the background surface and
 * the settings window cannot disagree about what the settings mean. Structural
 * rather than typed against the settings module: this asks for the fields it reads,
 * not for a whole `WallpaperSettings`. `rootPath` is the pre-`subjectId` spelling of
 * a checkout choice and is still honoured, so a profile written by an older version
 * keeps working (§4.4).
 */
export function endpointScopeOf(launch: {
  subjectId?: string
  rootPath?: string
  endpointPort?: number
  extraEndpointPorts?: readonly number[]
}): EndpointScope {
  return {
    subjectId: launch.subjectId ?? launch.rootPath,
    endpointPort: launch.endpointPort,
    extraPorts: launch.extraEndpointPorts,
  }
}

/**
 * The client shape a subject belongs to, or `undefined` when this build cannot
 * say (an unknown shell AUMID).
 *
 * Needed because the shape decides *how* a subject's interface is reached — a
 * desktop client owns a window, a source tree's interface is the browser — and the
 * scan cannot answer that when nothing is currently answering on its port. A
 * configured checkout is `official-web` by construction (§3 of the subject
 * design: a tree has no window of its own).
 */
export function subjectClientKind(subjectId: string | undefined): HarnessClientKind | undefined {
  const subject = (subjectId ?? '').trim()
  if (!subject) return undefined
  // 已安装的 CLI 明确按 `official-web` 处理（它没有自己的窗口 ✓，界面在浏览器 ✓）—— 与检出差
  // 的只是"没有源码树" ✓，所以这里分开写，让"这是有意为之"看得见。
  if (subject.toLowerCase().startsWith(CLI_SUBJECT_PREFIX)) return 'official-web'
  if (!subject.toLowerCase().startsWith(SHELL_SUBJECT_PREFIX)) return 'official-web'
  const aumid = subject.slice(SHELL_SUBJECT_PREFIX.length).trim().toLowerCase()
  return SHELL_SUBJECTS[aumid]?.kind
}

/** Sort candidates by the user's priority, then by port for stability. */
export function orderCandidates(
  candidates: readonly HarnessEndpointCandidate[],
): HarnessEndpointCandidate[] {
  return [...candidates].sort((a, b) => {
    const byKind = endpointPriority(a.kind) - endpointPriority(b.kind)
    return byKind !== 0 ? byKind : a.port - b.port
  })
}

/**
 * Candidates worth offering, in priority order.
 *
 * Only ports that actually host something are meaningful to show, but a port
 * with no listener is *not* an error: it simply means that client is not running,
 * which the settings UI reports as "found N usable".
 */
export function scanSummary(candidates: readonly HarnessEndpointCandidate[]): {
  scanned: number
  bridges: number
  ready: number
  summary: string
} {
  const ordered = orderCandidates(candidates)
  const bridges = ordered.filter((candidate) => candidate.bridgeFound)
  const ready = bridges.filter((candidate) => candidate.status.availability === 'bridge-ready')
  return {
    scanned: ordered.length,
    bridges: bridges.length,
    ready: ready.length,
    summary: bridges.length === 0
      ? `已扫描 ${ordered.length} 个端口，未发现可接入的 Harness`
      : `已扫描 ${ordered.length} 个端口，发现 ${bridges.length} 个可接入的 Harness（其中 ${ready.length} 个可用）`,
  }
}

/**
 * The candidate a session should use.
 *
 * Two frozen rules decide the answer, and they are both about *not* moving:
 *
 * 1. **Only the configured subject's own ports are admissible.** When the settings
 *    name a subject, a ready Bridge on any other port is a different subject, and
 *    using it would answer a session the user did not choose — even though it is
 *    "better" by every measure a scan can see.
 * 2. **Whatever answered stays in use.** `stickyPort` is the port already in use;
 *    while it still answers a Bridge it wins over a higher-priority candidate, so
 *    a second client coming up cannot pull the wallpaper off the first one.
 *
 * With nothing configured (no subject, no pin) both reduce to the shipped priority
 * order, which is the one case where "any of them will do" is what the user said.
 */
export function selectEndpoint(
  candidates: readonly HarnessEndpointCandidate[],
  scope: EndpointScope = {},
  stickyPort?: number,
): HarnessEndpointCandidate | undefined {
  const permitted = subjectEndpointPorts(scope)
  if (permitted === undefined) {
    return preferred(orderCandidates(candidates), stickyPort)
  }
  // A configured subject: its own ports only, in the order the subject declares
  // them. `orderCandidates` is deliberately not applied — client priority answers
  // "which subject should be used", a question the settings already answered, and
  // re-ranking here is how a scan would sneak another client back in.
  const inScope = permitted
    .map((port) => candidates.find((candidate) => candidate.port === port))
    .filter((candidate): candidate is HarnessEndpointCandidate => candidate !== undefined)
  // The last resort is the subject's own first port even when it answers nothing:
  // its diagnostic (offline, web-only, or a Bridge that is not ready) is the
  // actionable truth about *that* subject, which is more useful than "no endpoint".
  return preferred(inScope, stickyPort) ?? inScope[0]
}

/** One candidate that answered something, or the best of what is left. */
function preferred(
  ordered: readonly HarnessEndpointCandidate[],
  stickyPort: number | undefined,
): HarnessEndpointCandidate | undefined {
  return (stickyPort === undefined
    ? undefined
    : ordered.find((candidate) => candidate.port === stickyPort && candidate.bridgeFound))
    ?? ordered.find((candidate) => candidate.status.availability === 'bridge-ready')
    ?? ordered.find((candidate) => candidate.bridgeFound)
}

/** Per-endpoint probe timeout. Short: these are loopback requests. */
const PROBE_TIMEOUT_MS = 1200

/** The client shape a known port belongs to, for labelling and priority. */
function kindOfPort(port: number): HarnessClientKind {
  return DEFAULT_ENDPOINT_PORTS.find((entry) => entry.port === port)?.kind ?? 'official-web'
}

/**
 * Probe exactly these ports, adding nothing.
 *
 * The scoped probe a configured subject uses: a port that was not named is a port
 * that must not be looked at, because the only thing a foreign answer can do here
 * is tempt the selection. `scanEndpoints` stays the discovery scan behind the
 * settings card's 「扫描」 action and the browser preview's auto mode, where finding
 * something new is the whole point.
 */
export async function scanScopedPorts(ports: readonly number[]): Promise<HarnessEndpointCandidate[]> {
  return Promise.all(usablePorts(ports).map((port) => probeEndpoint(
    port,
    kindOfPort(port),
    DEFAULT_ENDPOINT_PORTS.some((entry) => entry.port === port) ? 'default' : 'user',
  )))
}

/**
 * How the wallpaper reaches a client's interface.
 *
 * The rule is by client shape, because they genuinely differ: the CLI / webui
 * shape has no Windows window (its interface *is* a browser URL on the port it
 * listens on), while the two desktop clients own windows that can be raised.
 * Getting this backwards would open a browser for a client that has a window, or
 * try to raise a window that does not exist.
 */
export type ClientRaiseAction = 'browser' | 'window'

export function clientRaiseAction(kind: HarnessClientKind): ClientRaiseAction {
  return kind === 'official-web' ? 'browser' : 'window'
}

/**
 * Wording for a raise outcome, or `null` when nothing needs saying.
 *
 * `raise-refused` is deliberately silent: Windows refuses foreground changes from
 * a process that is not itself foreground, which is the normal situation for a
 * desktop wallpaper. The window was still restored, so the user's next click
 * reaches it — reporting that as an error would be noise on a working path.
 */
export function raiseOutcomeNotice(outcome: string, kind: HarnessClientKind): string | null {
  switch (outcome) {
    case 'raised':
    case 'raise-refused':
      return null
    case 'no-window':
      return `${endpointKindLabel(kind)} 没有可拉起的窗口；它的界面可能在浏览器里，请改用「在浏览器中打开」。`
    case 'not-running':
      return `${endpointKindLabel(kind)} 未在运行。请先启动它，然后重新扫描。`
    default:
      return null
  }
}

/**
 * Probe one port for a wallpaper Bridge.
 *
 * Identification is deliberately strict: anything that answers without the
 * Bridge's own `/status` identity is `web-only`, never promoted to a Bridge. A
 * port that answers nothing at all is `offline`.
 */
export async function probeEndpoint(
  port: number,
  kind: HarnessClientKind,
  source: HarnessEndpointCandidate['source'] = 'default',
): Promise<HarnessEndpointCandidate> {
  const base = `http://127.0.0.1:${port}`
  let status: HarnessStatus = { availability: 'offline' }
  let bridgeFound = false
  try {
    const response = await fetch(`${base}/api/wallpaper/v1/status`, {
      signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
      headers: { Accept: 'application/json' },
    })
    if (response.ok) {
      const interpreted = interpretHarnessBridgeStatus(await response.json().catch(() => undefined))
      if (interpreted) {
        status = interpreted
        bridgeFound = true
      } else {
        // Something answered on the Bridge path but is not a Bridge status.
        status = { availability: 'web-only', reasonCode: 'bridge-status-invalid' }
      }
    } else {
      // A 401/404 here still proves an HTTP service; the root probe decides.
      status = { availability: 'web-only', reasonCode: 'bridge-status-missing' }
    }
  } catch {
    try {
      const root = await fetch(base, { signal: AbortSignal.timeout(PROBE_TIMEOUT_MS) })
      status = root.ok
        ? { availability: 'web-only', reasonCode: 'bridge-status-missing' }
        : { availability: 'offline' }
    } catch {
      status = { availability: 'offline' }
    }
  }
  return { port, kind, source, status, bridgeFound }
}

/**
 * Scan every candidate concurrently. Ports are independent, so probing them in
 * series would make the settings scan as slow as the sum of the timeouts.
 */
export async function scanEndpoints(
  extraPorts: readonly number[] = [],
): Promise<HarnessEndpointCandidate[]> {
  const seen = new Set<number>()
  const plan: Array<{ port: number; kind: HarnessClientKind; source: HarnessEndpointCandidate['source'] }> = []
  for (const { port, kind } of DEFAULT_ENDPOINT_PORTS) {
    if (seen.has(port)) continue
    seen.add(port)
    plan.push({ port, kind, source: 'default' })
  }
  for (const port of extraPorts) {
    if (!Number.isInteger(port) || port < 1 || port > 65535 || seen.has(port)) continue
    seen.add(port)
    // A user-supplied port has no known client shape; treat it as plain DSH.
    plan.push({ port, kind: 'official-web', source: 'user' })
  }
  return Promise.all(plan.map((item) => probeEndpoint(item.port, item.kind, item.source)))
}
