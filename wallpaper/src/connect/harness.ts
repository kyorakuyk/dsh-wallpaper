/**
 * The single definition of "is this Bridge response usable?".
 *
 * Historically this file answered a boolean question, so every imperfect answer
 * collapsed into the same two states (`web-only` / `offline`). When the Bridge
 * reported itself ready before its session routes were mounted, a user saw
 * "DSH online, missing Bridge" for what was actually a loading race, and a
 * broken revision looked identical to a Bridge that was never installed.
 *
 * The interpreter below returns a *diagnostic* state instead. `bridge-ready`
 * keeps its exact old meaning (the only state that may send a message), and
 * every other outcome now names the layer that is not usable yet.
 */

export type HarnessAvailability =
  | 'offline'
  | 'web-only'
  | 'bridge-loading'
  | 'bridge-auth-unavailable'
  | 'bridge-incompatible'
  | 'bridge-ready'

export interface HarnessStatus {
  availability: HarnessAvailability
  /** Stable, non-sensitive reason for a non-ready state. */
  reasonCode?: string
  /**
   * 一个**就绪过的**桥接正在失联，但还没到"确认掉线"的窗口。
   *
   * 用户要的是这个中间态可见（灯变黄、先挂起），而 `availability` 保持不变：这样滑槽、模型
   * 列表、会话都还按"它还在"处理 ✓，不会因为一次瞬发就复位 ✗。
   */
  probing?: boolean
  bridgeVersion?: string
  /** Non-sensitive build identifier, so an old installed Bridge is identifiable. */
  bridgeBuild?: string
  protocolVersion?: number
  model?: string
  provider?: string
  reasoningEffort?: string
}

/**
 * A process listening on 3080 is not sufficient to create a wallpaper session.
 * Keep this small predicate as the single browser-side definition of Harness
 * usability so prompts, automatic selection, and probe settling cannot
 * accidentally disagree.
 */
export function isHarnessReady(availability: HarnessAvailability): boolean {
  return availability === 'bridge-ready'
}

/** True for any state where a Bridge process answered but is not usable yet. */
export function isHarnessBridgePresent(availability: HarnessAvailability): boolean {
  return availability !== 'offline' && availability !== 'web-only'
}

const HARNESS_BRIDGE_PROTOCOL_VERSION = 1

/**
 * Capabilities required before the desktop may create or drive a session.
 * `resume` is deliberately optional: DSH only exposes it when its optional
 * session-persistence service is installed, and its absence must not make an
 * otherwise usable Harness mode disappear.
 */
const REQUIRED_HARNESS_BRIDGE_CAPABILITIES = [
  'sessions',
  'history',
  'sse',
  'cancel',
  'approval-handoff',
] as const

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}

function optionalString(document: Record<string, unknown>, field: string): string | undefined {
  const value = document[field]
  return typeof value === 'string' && value.trim() ? value : undefined
}

function withIdentity(status: HarnessStatus, document: Record<string, unknown>): HarnessStatus {
  const identity: HarnessStatus = { ...status }
  for (const field of ['bridgeVersion', 'bridgeBuild', 'model', 'provider', 'reasoningEffort'] as const) {
    const value = optionalString(document, field)
    if (value) identity[field] = value
  }
  if (typeof document.protocolVersion === 'number') identity.protocolVersion = document.protocolVersion
  return identity
}

/**
 * Interpret one `/api/wallpaper/v1/status` body.
 *
 * Returns `undefined` when the document is not a Bridge status at all (wrong
 * shape), which is what lets the caller fall back to the root-page probe.
 * A Bridge that answers but is unusable returns a *state*, never `undefined`:
 * "the Bridge is loading" and "there is no Bridge" need different user action.
 */
export function interpretHarnessBridgeStatus(data: unknown): HarnessStatus | undefined {
  const document = asRecord(data)
  if (!document) return undefined
  // `protocolVersion` and `dsh` are the minimum identity of this endpoint. A
  // document without them is some other service answering on the same port.
  if (typeof document.protocolVersion !== 'number' || document.dsh !== 'online') return undefined

  // The Bridge names its own state when it can. The declared value selects which
  // closed reason vocabulary is acceptable below; a specific reason is forwarded
  // only when it belongs to that vocabulary, so a stale field can never describe
  // the wrong condition.
  const declared = optionalString(document, 'state')
  const capabilities = document.capabilities
  const capabilityList = Array.isArray(capabilities)
    && capabilities.every((capability) => typeof capability === 'string')
    ? capabilities as string[]
    : undefined
  const authentication = optionalString(document, 'authentication')

  if (document.protocolVersion !== HARNESS_BRIDGE_PROTOCOL_VERSION) {
    return withIdentity({
      availability: 'bridge-incompatible',
      reasonCode: 'protocol-version-mismatch',
    }, document)
  }
  if (authentication !== 'ready') {
    // The reason vocabulary belongs to the consumer, not to the payload: a
    // Bridge that reports `state: bridge-ready` while its token is missing must
    // not have that stale `reasonCode` describe the failure.
    return withIdentity({
      availability: 'bridge-auth-unavailable',
      reasonCode: 'token-unavailable',
    }, document)
  }
  if (capabilityList === undefined) {
    // The document identifies itself as a Bridge but its capability list is
    // unusable. Treating that as "no capabilities" is the safe reading: the
    // wallpaper must not send into a route table it cannot verify.
    return withIdentity({
      availability: 'bridge-incompatible',
      reasonCode: 'capabilities-missing',
    }, document)
  }

  const hasEveryCapability = REQUIRED_HARNESS_BRIDGE_CAPABILITIES
    .every((required) => capabilityList.includes(required))
  if (hasEveryCapability) {
    return withIdentity({ availability: 'bridge-ready' }, document)
  }

  // A Bridge that explicitly says it is still composing its service set is
  // loading, and it may name the specific service it waits for. Anything else
  // that lacks a required capability cannot be fixed by waiting, so it is
  // reported as incompatible.
  if (declared === 'bridge-loading') {
    return withIdentity({
      availability: 'bridge-loading',
      reasonCode: waitingReasonCode(optionalString(document, 'reasonCode')),
    }, document)
  }
  // A host whose *shape* did not match names the member after the colon. That is
  // the actionable part, and the native layer deliberately forwards it, so this
  // layer must not be the one that throws it away: the renderer is what the
  // desktop actually displays. Anything else keeps the consumer's own code,
  // because a `waiting:` reason would misdescribe a condition waiting cannot fix.
  return withIdentity({
    availability: 'bridge-incompatible',
    reasonCode: hostShapeReasonCode(optionalString(document, 'reasonCode')),
  }, document)
}

/**
 * The loading reason vocabulary is closed and owned by the consumer. A payload's
 * `reasonCode` is only accepted when it is already in that vocabulary, so a
 * stale field (for example `ready` left over from an earlier status) can never
 * be rendered as the explanation for a wait.
 */
function waitingReasonCode(candidate: string | undefined): string {
  if (candidate === 'services-pending' || candidate?.startsWith('waiting:')) return candidate
  return 'services-pending'
}

/** The shape-mismatch vocabulary: a fixed prefix plus a compile-time member name. */
function hostShapeReasonCode(candidate: string | undefined): string {
  if (candidate?.startsWith('host-shape-mismatch:')) return candidate
  return 'capabilities-missing'
}

/**
 * Backwards-compatible wrapper. Existing callers that only ask "is this
 * response a usable Bridge?" keep working, and callers that need the reason
 * should use `interpretHarnessBridgeStatus` directly.
 */
export function compatibleHarnessBridgeStatus(data: unknown): HarnessStatus | undefined {
  const status = interpretHarnessBridgeStatus(data)
  return status && isHarnessReady(status.availability) ? status : undefined
}

/**
 * Loopback base URL for a DSH endpoint.
 *
 * The port is a parameter rather than a constant because the client shapes
 * listen on different ports (official desktop shell 19387, CLI/core 3080) and only
 * some of them are configurable. See
 * `connect/endpoints.ts` for discovery and the priority order.
 */
export function harnessBaseUrl(port = 3080): string {
  return `http://127.0.0.1:${port}`
}

export async function fetchHarnessStatus(baseUrl = harnessBaseUrl()): Promise<HarnessStatus> {
  try {
    const response = await fetch(`${baseUrl}/api/wallpaper/v1/status`, { signal: AbortSignal.timeout(1200), headers: { Accept: 'application/json' } })
    if (response.ok) {
      const status = interpretHarnessBridgeStatus(await response.json())
      // A Bridge that answered with a diagnostic state is the most specific
      // information available; do not downgrade it to a root-page guess.
      if (status) return status
    }
  } catch { /* fall through */ }
  try {
    // Do not use `no-cors` here. An opaque response proves neither the HTTP
    // status nor the local process identity, so it must never turn a browser
    // preview into a misleading "DSH online" state. Native builds use their
    // direct Rust loopback probe instead; this fallback is intentionally
    // conservative when the browser cannot inspect a cross-origin response.
    const response = await fetch(baseUrl, { signal: AbortSignal.timeout(900) })
    return response.ok ? { availability: 'web-only', reasonCode: 'bridge-status-missing' } : { availability: 'offline' }
  } catch { return { availability: 'offline' } }
}

/**
 * 探针节奏与判定窗口集中在这里——用户要的"三个数"一处就能调。
 *
 * 设计意图（用户指定）：壁纸刚打开时**高频握手**，尽快匹配到刚上线的后端；几分钟没呼应就
 * 进**待机**低频（没配自启、用户也没手动拉起时的常态）；连上后**低频保活**；一旦失联立刻
 * **提速**确认，好把"瞬发抖动"和"真死"分开。
 */
export const HARNESS_PROBE_MS = {
  /** 启动后的高频握手。 */
  startup: 2_000,
  /** 待机：只为"上线即被匹配"，不烧资源。 */
  standby: 30_000,
  /** 已连接：低频保活。 */
  connected: 15_000,
  /** 疑似挂起：立刻提速。原生那条路用 1.2 s，两边保持一致。 */
  suspect: 1_200,
} as const

/** 高频握手的窗口长度；超过它仍未连上就转待机。 */
export const HARNESS_STARTUP_WINDOW_MS = 3 * 60_000

/**
 * 失联后多久才判定"确认掉线"。
 *
 * 这段时间里灯是黄的、滑槽不复位。用户实测这个缓冲**太长**，所以从 60 s 收到 20 s：原生那条路
 * 现在先看**端口属主的进程还在不在**（属主一消失就是确证，1.2 s 档位下约两秒内见结果），这个
 * 窗口只剩"连进程都认不出来"时的兜底——兜底不该比取证还慢一个数量级。
 */
export const HARNESS_SUSPECT_GRACE_MS = 20_000

export function monitorHarness(onChange: (status: HarnessStatus) => void, probe: () => Promise<HarnessStatus> = fetchHarnessStatus): { stop(): void; pollNow(): Promise<HarnessStatus> } {
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let inFlight = false
  let successes = 0
  let failures = 0
  let current: HarnessAvailability = 'offline'
  // 相位时间戳：决定"多久探一次"，以及"还没连上是不是已经过了高频握手的窗口"。
  const startedAt = Date.now()
  let readyAt: number | undefined
  let suspectSince: number | undefined
  const pollNow = async (): Promise<HarnessStatus> => {
    if (inFlight) return { availability: current }
    inFlight = true
    let status: HarnessStatus
    try {
      status = await probe()
    } catch {
      // A custom preview probe is allowed to reject; preserve the same
      // fail-closed behaviour as the built-in status fetcher.
      status = { availability: 'offline' }
    } finally {
      inFlight = false
    }
    // Mirror the native monitor: only a compatible Bridge is a success.
    // Every other state is a failed Harness probe and therefore needs three
    // consecutive observations before it can replace a previously ready
    // Bridge, so a single slow probe cannot flicker the mode switch.
    if (isHarnessReady(status.availability)) {
      successes += 1; failures = 0
      suspectSince = undefined
      readyAt = Date.now()
      if (successes >= 2 && current !== status.availability) { current = status.availability; onChange(status) }
      else if (current === status.availability) onChange(status)
    } else {
      failures += 1; successes = 0
      // ④ 疑似挂起：就绪过的桥接失联了，但还在宽限窗口内——**先挂起，不判定死亡**。
      // 这一路刻意不改 `availability`（滑槽、模型列表、会话都还按"它还在"处理），
      // 只把 `probing` 抖出来让灯变黄；超过宽限窗口才落到下面的"确认掉线"。
      if (readyAt !== undefined && suspectSince === undefined) suspectSince = Date.now()
      if (suspectSince !== undefined && Date.now() - suspectSince < HARNESS_SUSPECT_GRACE_MS) {
        const probing = { ...status, probing: true }
        onChange(probing)
        return probing
      }
      if (failures >= 3 && current !== status.availability) { current = status.availability; onChange(status) }
    }
    return status
  }
  // 四档节奏：启动高频握手 → 几分钟没呼应转待机 → 连上后低频保活 → 疑似挂起立刻提速。
  const nextInterval = () => {
    if (isHarnessReady(current)) return suspectSince !== undefined ? HARNESS_PROBE_MS.suspect : HARNESS_PROBE_MS.connected
    return Date.now() - startedAt < HARNESS_STARTUP_WINDOW_MS ? HARNESS_PROBE_MS.startup : HARNESS_PROBE_MS.standby
  }
  const schedule = () => {
    if (stopped) return
    timer = setTimeout(async () => { await pollNow(); schedule() }, nextInterval())
  }
  void pollNow().finally(schedule)
  return { stop() { stopped = true; if (timer) clearTimeout(timer) }, pollNow }
}
