import { describe, expect, it, vi, afterEach } from 'vitest'
import {
  DEFAULT_ENDPOINT_PORTS,
  endpointPriority,
  orderCandidates,
  scanSummary,
  selectEndpoint,
  scanEndpoints,
  type HarnessEndpointCandidate,
} from '../src/connect/endpoints.ts'

function candidate(
  port: number,
  availability: HarnessEndpointCandidate['status']['availability'],
  kind: HarnessEndpointCandidate['kind'] = 'official-web',
): HarnessEndpointCandidate {
  return {
    port,
    kind,
    source: 'default',
    status: { availability },
    bridgeFound: availability !== 'offline' && availability !== 'web-only',
  }
}

afterEach(() => vi.unstubAllGlobals())

describe('endpoint priority', () => {
  it('ships the official desktop first, then community desktop, then web/CLI', () => {
    // The user's stated default order. This is also why the three known ports are
    // listed in this sequence rather than numerically.
    expect(endpointPriority('official-desktop')).toBeLessThan(endpointPriority('community-desktop'))
    expect(endpointPriority('community-desktop')).toBeLessThan(endpointPriority('official-web'))
    expect(DEFAULT_ENDPOINT_PORTS.map((entry) => entry.port)).toEqual([19387, 43120, 3080])
  })

  it('orders candidates by kind and then by port, without mutating the input', () => {
    const input = [
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(43120, 'bridge-ready', 'community-desktop'),
      candidate(19387, 'bridge-ready', 'official-desktop'),
    ]
    expect(orderCandidates(input).map((entry) => entry.port)).toEqual([19387, 43120, 3080])
    // The caller's array keeps its original order.
    expect(input.map((entry) => entry.port)).toEqual([3080, 43120, 19387])
  })

  it('breaks ties inside one kind by port so the list is stable', () => {
    const ordered = orderCandidates([
      candidate(5000, 'bridge-ready', 'official-web'),
      candidate(4000, 'bridge-ready', 'official-web'),
    ])
    expect(ordered.map((entry) => entry.port)).toEqual([4000, 5000])
  })
})

describe('endpoint selection', () => {
  it('prefers the highest-priority ready endpoint when nothing is pinned', () => {
    const chosen = selectEndpoint([
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(19387, 'bridge-ready', 'official-desktop'),
    ], undefined)
    expect(chosen?.port).toBe(19387)
  })

  it('falls back to a Bridge that is present but not ready, over nothing', () => {
    // A Bridge that answers `bridge-auth-unavailable` is a different problem from
    // no Bridge at all, and the user needs to see which one it is.
    const chosen = selectEndpoint([
      candidate(19387, 'bridge-auth-unavailable', 'official-desktop'),
    ], undefined)
    expect(chosen?.port).toBe(19387)
    expect(chosen?.status.availability).toBe('bridge-auth-unavailable')
  })

  it('never selects a port that only hosts a plain web service', () => {
    expect(selectEndpoint([candidate(19387, 'web-only', 'official-desktop')], undefined)).toBeUndefined()
    expect(selectEndpoint([candidate(3080, 'offline')], undefined)).toBeUndefined()
  })

  it('honours a pinned port even when it is not ready and another one is', () => {
    // Silently switching would answer a *different* client than the user picked,
    // which is worse than reporting the chosen one as unavailable.
    const chosen = selectEndpoint([
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(19387, 'bridge-loading', 'official-desktop'),
    ], 19387)
    expect(chosen?.port).toBe(19387)
    expect(chosen?.status.availability).toBe('bridge-loading')
  })

  it('falls back to auto when the pinned port is not among the candidates', () => {
    const chosen = selectEndpoint([candidate(3080, 'bridge-ready', 'official-web')], 19387)
    expect(chosen?.port).toBe(3080)
  })
})

describe('scan summary', () => {
  it('distinguishes "found none" from "found some but none usable"', () => {
    const none = scanSummary([candidate(19387, 'offline'), candidate(3080, 'offline')])
    expect(none.bridges).toBe(0)
    expect(none.summary).toContain('未发现可接入')
    // The count of scanned ports is always reported, so a scan that found nothing
    // still proves it looked somewhere.
    expect(none.summary).toContain('2')

    const present = scanSummary([
      candidate(19387, 'bridge-loading', 'official-desktop'),
      candidate(3080, 'offline'),
    ])
    expect(present.bridges).toBe(1)
    expect(present.ready).toBe(0)
    expect(present.summary).toContain('发现 1 个')

    const ready = scanSummary([
      candidate(19387, 'bridge-ready', 'official-desktop'),
      candidate(43120, 'bridge-ready', 'community-desktop'),
    ])
    expect(ready.ready).toBe(2)
    expect(ready.summary).toContain('2 个可用')
  })
})

describe('scanning', () => {
  it('identifies a Bridge and ignores a non-Bridge service on the same path', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes(':19387/api/')) {
        return {
          ok: true,
          json: async () => ({
            protocolVersion: 1,
            dsh: 'online',
            state: 'bridge-ready',
            reasonCode: 'ready',
            authentication: 'ready',
            capabilities: ['status', 'control', 'sessions', 'history', 'sse', 'cancel', 'approval-handoff'],
          }),
        }
      }
      if (url.includes(':43120/api/')) {
        // Answers the path, but with some other service's document.
        return { ok: true, json: async () => ({ status: 'unrelated-service' }) }
      }
      throw new Error('connection refused')
    })
    vi.stubGlobal('fetch', fetchMock)

    const found = await scanEndpoints()
    expect(found.map((entry) => entry.port)).toEqual([19387, 43120, 3080])
    expect(found[0]).toMatchObject({ port: 19387, bridgeFound: true })
    expect(found[0]?.status.availability).toBe('bridge-ready')
    // A non-Bridge document must not be promoted to an endpoint.
    expect(found[1]).toMatchObject({ port: 43120, bridgeFound: false })
    expect(found[1]?.status.availability).toBe('web-only')
    expect(found[2]).toMatchObject({ port: 3080, bridgeFound: false })
    expect(found[2]?.status.availability).toBe('offline')
  })

  it('accepts user-supplied ports without letting duplicates or junk in', async () => {
    const seen: string[] = []
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      seen.push(url)
      throw new Error('refused')
    }))
    const found = await scanEndpoints([19387, 6000, 0, 70000, 6000, 1.5])
    expect(found.map((entry) => entry.port)).toEqual([19387, 43120, 3080, 6000])
    // A duplicate of a default port is not probed twice.
    expect(seen.filter((url) => url.includes(':19387/'))).toHaveLength(1)
    // Invalid values never become candidates.
    for (const bad of ['0', '70000', '1.5']) {
      expect(seen.some((url) => url.includes(`:${bad}/`))).toBe(false)
    }
  })

  it('reports web-only when the Bridge path fails but the root page answers', async () => {
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url.includes('/api/')) return { ok: false, status: 401 }
      return { ok: true }
    }))
    const found = await scanEndpoints()
    for (const entry of found) {
      expect(entry.bridgeFound).toBe(false)
      expect(entry.status.availability).toBe('web-only')
      expect(entry.status.reasonCode).toBe('bridge-status-missing')
    }
  })
})
