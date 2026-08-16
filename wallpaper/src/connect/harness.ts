export type HarnessAvailability = 'offline' | 'web-only' | 'bridge-ready'
export interface HarnessStatus { availability: HarnessAvailability; bridgeVersion?: string; model?: string; provider?: string; reasoningEffort?: string }

export async function fetchHarnessStatus(baseUrl = 'http://127.0.0.1:3080'): Promise<HarnessStatus> {
  try {
    const response = await fetch(`${baseUrl}/api/wallpaper/v1/status`, { signal: AbortSignal.timeout(1200), headers: { Accept: 'application/json' } })
    if (response.ok) {
      const data = await response.json() as Record<string, unknown>
      return { availability: 'bridge-ready', bridgeVersion: typeof data.bridgeVersion === 'string' ? data.bridgeVersion : undefined, model: typeof data.model === 'string' ? data.model : undefined, provider: typeof data.provider === 'string' ? data.provider : undefined, reasoningEffort: typeof data.reasoningEffort === 'string' ? data.reasoningEffort : undefined }
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
