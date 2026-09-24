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
 * | third-party desktop   | 43120        | yes (its own settings) |
 * | DSH CLI / core        | 3080         | yes (`--port`) |
 *
 * So "connect to 3080" silently means "connect only to the CLI shape", and a user
 * running the official shell sees `offline` while a perfectly ready wallpaper
 * Bridge is listening one port away. Discovery replaces that assumption with a
 * scanned candidate list plus an explicit user choice.
 *
 * Nothing here reads a credential: identification uses the Bridge's public
 * `/status`, and a candidate that answers with something else is reported as
 * `web-only` rather than assumed to be ours.
 */
import { interpretHarnessBridgeStatus, type HarnessStatus } from './harness.ts'

/** The three client shapes, in the priority order the wallpaper defaults to. */
export type HarnessClientKind = 'official-desktop' | 'community-desktop' | 'official-web'

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
    case 'official-desktop': return '官方桌面客户端'
    case 'community-desktop': return '第三方桌面客户端'
    default: return '官方 Web / CLI'
  }
}

/**
 * Priority used whenever several endpoints are usable. The user's stated order:
 * official desktop, then the community desktop, then plain official web/CLI.
 * Lower sorts first.
 */
export function endpointPriority(kind: HarnessClientKind): number {
  switch (kind) {
    case 'official-desktop': return 0
    case 'community-desktop': return 1
    default: return 2
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
  { port: 43120, kind: 'community-desktop' },
  { port: 3080, kind: 'official-web' },
]

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

/** The candidate a session should use, honouring an explicit choice first. */
export function selectEndpoint(
  candidates: readonly HarnessEndpointCandidate[],
  chosenPort: number | undefined,
): HarnessEndpointCandidate | undefined {
  const ordered = orderCandidates(candidates)
  if (chosenPort !== undefined) {
    // An explicit choice is honoured even when it is not currently ready: the
    // user may be starting that client, and silently switching to another one
    // would answer a different session than the one they picked.
    const chosen = ordered.find((candidate) => candidate.port === chosenPort)
    if (chosen) return chosen
  }
  return ordered.find((candidate) => candidate.status.availability === 'bridge-ready')
    ?? ordered.find((candidate) => candidate.bridgeFound)
}

/** Per-endpoint probe timeout. Short: these are loopback requests. */
const PROBE_TIMEOUT_MS = 1200

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
