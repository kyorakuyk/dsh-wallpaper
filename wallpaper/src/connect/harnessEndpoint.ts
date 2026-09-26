/**
 * Selection and monitoring of the DSH endpoint a wallpaper session uses.
 *
 * `connect/harness.ts` answers "is this one document a usable Bridge?".
 * `connect/endpoints.ts` answers "which local ports host a Bridge at all?".
 * This module joins them: it keeps a probe loop running and decides which endpoint
 * the wallpaper is talking to.
 *
 * The decision is the frozen one: the configured *subject* decides, and nothing a
 * scan finds may replace it. A client that starts later is picked up because the
 * wallpaper already probes its configured subject's ports — not because a scan
 * may hand the session to whatever answered most recently.
 */
import { isHarnessReady, type HarnessStatus } from './harness.ts'
import {
  endpointScopeConfigured,
  orderCandidates,
  probeEndpoint,
  scanEndpoints,
  scanScopedPorts,
  selectEndpoint,
  subjectEndpointPorts,
  type EndpointScope,
  type HarnessEndpointCandidate,
} from './endpoints.ts'

export interface EndpointMonitorOptions {
  /**
   * What the settings configure: the subject, the user's pin, and any ports they
   * added. Read through a callback rather than captured, so an edit takes effect on
   * the next tick instead of requiring a restart.
   */
  scope: () => EndpointScope
  /** Extra ports the user added for the scan, beyond what the scope admits. */
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
 * different fact from a change of readiness. The endpoint itself is decided by the
 * configured subject and is never substituted: when the subject is not answering,
 * that is reported as the subject being offline rather than as another subject
 * being ready (see `selectEndpoint`).
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
  /**
   * The endpoint already in use, and the scope it was chosen under.
   *
   * Sticky on purpose: once the wallpaper is talking to a subject, a second one
   * coming up must not pull the conversation away from it. The scope key resets the
   * stickiness when the user changes subject — that is a real change of subject,
   * not a substitution.
   */
  let stickyPort: number | undefined
  let stickyScope = ''

  const publish = (update: EndpointUpdate): void => {
    if (!stopped) options.onChange(update)
  }

  const tick = async (): Promise<void> => {
    if (stopped || inFlight) return
    inFlight = true
    const scope = options.scope()
    const scopeKey = JSON.stringify([
      scope.subjectId ?? '',
      scope.endpointPort ?? null,
      scope.extraPorts ?? [],
    ])
    if (scopeKey !== stickyScope) {
      stickyScope = scopeKey
      stickyPort = undefined
    }
    let next: HarnessEndpointCandidate[] = []
    try {
      // Probe the configured subject's own ports and nothing else. Scanning every
      // port would be harmless in itself, but it is how a foreign client's ready
      // state ends up one refactor away from being selected, and the settings card
      // has its own explicit scan for the "what is out there" question.
      const permitted = subjectEndpointPorts(scope)
      next = await (permitted === undefined
        ? scanEndpoints(options.extraPorts?.() ?? [])
        : scanScopedPorts(permitted))
    } catch {
      next = []
    } finally {
      inFlight = false
    }
    if (stopped) return
    candidates = next

    const chosen = selectEndpoint(next, scope, stickyPort)
    // A configured subject that answers nothing gets its own reason code: "the
    // client you chose is not up" is a different situation from "nothing was found
    // here", and it is the one the settings card has to explain without pretending
    // another client is the answer.
    const offlineReason = endpointScopeConfigured(scope) ? 'subject-offline' : 'no-endpoint'
    const status: HarnessStatus = chosen
      ? (chosen.status.availability === 'offline' && !chosen.status.reasonCode
        ? { ...chosen.status, reasonCode: offlineReason }
        : chosen.status)
      : { availability: 'offline', reasonCode: offlineReason }
    const port = chosen?.port
    const key = `${port ?? 'none'}:${status.availability}`
    const changedEndpoint = port !== lastPort
    const changedState = status.availability !== lastAvailability
    // Only a Bridge that is actually usable becomes the sticky choice; a port that
    // merely answered an error page must not freeze the selection.
    if (chosen && isHarnessReady(chosen.status.availability)) stickyPort = chosen.port

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
