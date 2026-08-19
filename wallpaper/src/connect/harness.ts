export type HarnessAvailability = 'offline' | 'web-only' | 'bridge-ready'
export interface HarnessStatus { availability: HarnessAvailability; bridgeVersion?: string; model?: string; provider?: string; reasoningEffort?: string }

/**
 * A process listening on 3080 is not sufficient to create a wallpaper
 * session.  Keep this small predicate as the single browser-side definition
 * of Harness usability so prompts, automatic selection, and probe settling
 * cannot accidentally disagree.
 */
export function isHarnessReady(availability: HarnessAvailability): boolean {
  return availability === 'bridge-ready'
}

// Keep this browser-preview fallback aligned with the native monitor. A local
// HTTP 200 is not enough to enable Harness: port 3080 may be DSH's regular
// web UI, an older bridge, or an unrelated local service.
const HARNESS_BRIDGE_PROTOCOL_VERSION = 1
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

/**
 * Accept only the versioned bridge contract required to create a fresh native
 * Harness session. `resume` is deliberately optional: DSH only exposes it
 * when its optional session-persistence service is installed, and absence of
 * that service must not make otherwise usable Harness mode disappear.
 */
export function compatibleHarnessBridgeStatus(data: unknown): HarnessStatus | undefined {
  const document = asRecord(data)
  const capabilities = document?.capabilities
  if (
    document?.protocolVersion !== HARNESS_BRIDGE_PROTOCOL_VERSION
    || document.dsh !== 'online'
    || document.authentication !== 'ready'
    || !Array.isArray(capabilities)
    || !capabilities.every((capability) => typeof capability === 'string')
    || !REQUIRED_HARNESS_BRIDGE_CAPABILITIES.every((required) => capabilities.includes(required))
  ) return undefined

  const status: HarnessStatus = { availability: 'bridge-ready' }
  for (const field of ['bridgeVersion', 'model', 'provider', 'reasoningEffort'] as const) {
    const value = document[field]
    if (typeof value === 'string' && value.trim()) status[field] = value
  }
  return status
}

export async function fetchHarnessStatus(baseUrl = 'http://127.0.0.1:3080'): Promise<HarnessStatus> {
  try {
    const response = await fetch(`${baseUrl}/api/wallpaper/v1/status`, { signal: AbortSignal.timeout(1200), headers: { Accept: 'application/json' } })
    if (response.ok) {
      const status = compatibleHarnessBridgeStatus(await response.json())
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
    return response.ok ? { availability: 'web-only' } : { availability: 'offline' }
  } catch { return { availability: 'offline' } }
}

export function monitorHarness(onChange: (status: HarnessStatus) => void, probe: () => Promise<HarnessStatus> = fetchHarnessStatus): { stop(): void; pollNow(): Promise<HarnessStatus> } {
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let inFlight = false
  let successes = 0
  let failures = 0
  let current: HarnessAvailability = 'offline'
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
    // `web-only` is useful diagnostic state, but it is a failed Harness
    // probe and therefore needs three consecutive observations before it can
    // replace a previously ready Bridge.
    if (isHarnessReady(status.availability)) {
      successes += 1; failures = 0
      if (successes >= 2 && current !== status.availability) { current = status.availability; onChange(status) }
      else if (current === status.availability) onChange(status)
    } else {
      failures += 1; successes = 0
      if (failures >= 3 && current !== status.availability) { current = status.availability; onChange(status) }
    }
    return status
  }
  const schedule = () => {
    if (stopped) return
    timer = setTimeout(async () => { await pollNow(); schedule() }, current === 'offline' ? 2000 : 5000)
  }
  void pollNow().finally(schedule)
  return { stop() { stopped = true; if (timer) clearTimeout(timer) }, pollNow }
}
