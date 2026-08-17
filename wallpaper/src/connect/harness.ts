export type HarnessAvailability = 'offline' | 'web-only' | 'bridge-ready'
export interface HarnessStatus { availability: HarnessAvailability; bridgeVersion?: string; model?: string; provider?: string; reasoningEffort?: string }

// Keep this browser-preview fallback aligned with the native monitor. A local
// HTTP 200 is not enough to enable Harness: port 3080 may be DSH's regular
// web UI, an older bridge, or an unrelated local service.
const HARNESS_BRIDGE_PROTOCOL_VERSION = 1
const REQUIRED_HARNESS_BRIDGE_CAPABILITIES = [
  'sessions',
  'resume',
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
 * Accept only the versioned bridge contract required by the native Harness
 * client. This deliberately fails closed so preview mode cannot offer a
 * Harness switch that will fail later while opening a session or SSE stream.
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
    const response = await fetch(baseUrl, { signal: AbortSignal.timeout(900), mode: 'no-cors' })
    return response.type === 'opaque' || response.ok ? { availability: 'web-only' } : { availability: 'offline' }
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
    const status = await probe()
    inFlight = false
    if (status.availability === 'offline') {
      failures += 1; successes = 0
      if (failures >= 3 && current !== 'offline') { current = 'offline'; onChange(status) }
    } else {
      successes += 1; failures = 0
      if (successes >= 2 && current !== status.availability) { current = status.availability; onChange(status) }
      else if (current === status.availability) onChange(status)
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
