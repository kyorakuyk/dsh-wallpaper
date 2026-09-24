/**
 * Selection and monitoring of the DSH endpoint a wallpaper session uses.
 *
 * `connect/harness.ts` answers "is this one document a usable Bridge?".
 * `connect/endpoints.ts` answers "which local ports host a Bridge at all?".
 * This module joins them: it keeps the chosen endpoint current over time, so a
 * client that starts later (or restarts) is picked up without the user
 * re-selecting anything, while an explicit choice is never silently overridden.
 */
import { isHarnessReady, type HarnessStatus } from './harness.ts'
import {
  orderCandidates,
  probeEndpoint,
  scanEndpoints,
  selectEndpoint,
  type HarnessEndpointCandidate,
} from './endpoints.ts'

export interface EndpointMonitorOptions {
  /**
   * The port the user pinned, or undefined for the shipped priority order.
   * Read through a callback rather than captured, so changing the dropdown takes
   * effect on the next tick instead of requiring a restart.
   */
  chosenPort: () => number | undefined
  extraPorts?: () => readonly number[]
  onChange: (update: EndpointUpdate) => void
  /** Poll interval while an endpoint is usable. */
  readyIntervalMs?: number
  /** Poll interval while nothing is usable. */
  idleIntervalMs?: number
  /** Consecutive agreeing observations required before a state change is published. */
  settle?: number
}

export interface EndpointUpdate {
  status: HarnessStatus
  /** The endpoint the status came from, when one was selected. */
  port?: number
  /** Every candidate seen in the scan that produced this update. */
  candidates: HarnessEndpointCandidate[]
  /** True when the update is a change of endpoint, not just of state. */
  endpointChanged: boolean
}

export interface EndpointMonitor {
  stop(): void
  /** Re-scan immediately (the settings "扫描" action). */
  scanNow(): Promise<HarnessEndpointCandidate[]>
  /** The candidates from the most recent scan. */
  current(): HarnessEndpointCandidate[]
}

/**
 * Watch for a usable Bridge across all known endpoints.
 *
 * Deliberately stateful about the *endpoint* as well as the status: the desktop
 * needs to show which client it is talking to, and a change of endpoint is a
 * different fact from a change of readiness. Losing the selected endpoint falls
 * back to another ready one only when the user did not pin a port.
 */
export function monitorHarnessEndpoint(options: EndpointMonitorOptions): EndpointMonitor {
  const readyInterval = options.readyIntervalMs ?? 5000
  const idleInterval = options.idleIntervalMs ?? 3000
  const settle = options.settle ?? 2

  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let inFlight = false
  let candidates: HarnessEndpointCandidate[] = []
  let lastPort: number | undefined
  let lastAvailability = 'offline'
  let streak = 0
  let streakKey = ''

  const publish = (update: EndpointUpdate): void => {
    if (!stopped) options.onChange(update)
  }

  const tick = async (): Promise<void> => {
    if (stopped || inFlight) return
    inFlight = true
    let next: HarnessEndpointCandidate[] = []
    try {
      next = await scanEndpoints(options.extraPorts?.() ?? [])
    } catch {
      next = []
    } finally {
      inFlight = false
    }
    if (stopped) return
    candidates = next

    const chosen = selectEndpoint(next, options.chosenPort())
    // A pinned port that currently answers nothing is still reported as that
    // endpoint being offline, rather than quietly moving to another client.
    const status: HarnessStatus = chosen?.status
      ?? { availability: 'offline', reasonCode: 'no-endpoint' }
    const port = chosen?.port
    const key = `${port ?? 'none'}:${status.availability}`
    const changedEndpoint = port !== lastPort
    const changedState = status.availability !== lastAvailability

    if (changedEndpoint && port !== undefined && lastPort !== undefined) {
      // An endpoint switch is worth reporting at once: it changes which session
      // the user is talking to, so debouncing it would show the wrong client.
      streak = 0
      lastPort = port
      lastAvailability = status.availability
      streakKey = key
      publish({ status, port, candidates, endpointChanged: true })
    } else if (key === streakKey) {
      streak += 1
      if (streak === settle && (changedState || changedEndpoint)) {
        lastPort = port
        lastAvailability = status.availability
        publish({ status, port, candidates, endpointChanged: changedEndpoint })
      }
    } else {
      streakKey = key
      streak = 1
    }

    const delay = isHarnessReady(status.availability) ? readyInterval : idleInterval
    if (!stopped) timer = setTimeout(() => void tick(), delay)
  }

  void tick()

  return {
    stop() {
      stopped = true
      if (timer) clearTimeout(timer)
    },
    async scanNow() {
      const scanned = await scanEndpoints(options.extraPorts?.() ?? [])
      candidates = scanned
      return orderCandidates(scanned)
    },
    current: () => candidates,
  }
}

/**
 * Probe one endpoint on demand, for the explicit "测试这个端点" action.
 * Kept next to the monitor so a manual test and the background poll cannot use
 * different identification rules.
 */
export async function testEndpoint(port: number): Promise<HarnessEndpointCandidate> {
  return probeEndpoint(port, 'official-web', 'user')
}
