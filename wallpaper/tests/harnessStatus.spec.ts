import { afterEach, describe, expect, it, vi } from 'vitest'
import { compatibleHarnessBridgeStatus, fetchHarnessStatus, interpretHarnessBridgeStatus, isHarnessReady } from '../src/connect/harness.ts'

function validBridgeStatus(): Record<string, unknown> {
  return {
    bridgeVersion: '0.1.1',
    bridgeBuild: 'dev',
    protocolVersion: 1,
    dsh: 'online',
    state: 'bridge-ready',
    reasonCode: 'ready',
    authentication: 'ready',
    capabilities: [
      'status',
      'control',
      'sessions',
      'resume',
      'history',
      'sse',
      'cancel',
      'approval-handoff',
      'future-capability',
    ],
    provider: 'deepseek',
    model: 'deepseek-chat',
    reasoningEffort: 'high',
  }
}

afterEach(() => vi.unstubAllGlobals())

describe('Harness bridge status contract', () => {
  it('accepts a ready v1 bridge with every required capability', () => {
    expect(compatibleHarnessBridgeStatus(validBridgeStatus())).toEqual({
      availability: 'bridge-ready',
      bridgeVersion: '0.1.1',
      bridgeBuild: 'dev',
      protocolVersion: 1,
      provider: 'deepseek',
      model: 'deepseek-chat',
      reasoningEffort: 'high',
    })
  })

  it('keeps fresh-session Harness usable when optional resume is unavailable', () => {
    const status = validBridgeStatus()
    status.capabilities = (status.capabilities as string[]).filter((capability) => capability !== 'resume')
    expect(compatibleHarnessBridgeStatus(status)).toMatchObject({ availability: 'bridge-ready' })
  })

  it('treats a document without a Bridge identity as not a Bridge at all', () => {
    // These are not Bridge statuses, so the caller must fall back to the
    // root-page probe instead of rendering a wrong diagnosis.
    for (const document of [
      {},
      { status: 'unrelated-service' },
      { protocolVersion: 1 },
      { dsh: 'online' },
      { ...validBridgeStatus(), protocolVersion: '1' },
      { ...validBridgeStatus(), dsh: 'offline' },
    ]) {
      expect(interpretHarnessBridgeStatus(document), JSON.stringify(document)).toBeUndefined()
      expect(compatibleHarnessBridgeStatus(document)).toBeUndefined()
    }
  })

  it('names the layer that is unusable instead of collapsing every cause into web-only', () => {
    // A protocol the wallpaper cannot speak.
    expect(interpretHarnessBridgeStatus({ ...validBridgeStatus(), protocolVersion: 9 })).toMatchObject({
      availability: 'bridge-incompatible',
      reasonCode: 'protocol-version-mismatch',
    })
    // The Bridge is mounted but its token is unavailable.
    expect(interpretHarnessBridgeStatus({ ...validBridgeStatus(), authentication: 'unavailable' })).toMatchObject({
      availability: 'bridge-auth-unavailable',
      reasonCode: 'token-unavailable',
    })
    // The Bridge is still composing its service set: waiting fixes it.
    expect(interpretHarnessBridgeStatus({
      ...validBridgeStatus(),
      state: 'bridge-loading',
      capabilities: ['status'],
    })).toMatchObject({ availability: 'bridge-loading', reasonCode: 'services-pending' })
    // A capability set that will never satisfy the wallpaper.
    expect(interpretHarnessBridgeStatus({
      ...validBridgeStatus(),
      state: 'bridge-ready',
      capabilities: ['sessions', 'history', 'sse', 'cancel'],
    })).toMatchObject({ availability: 'bridge-incompatible', reasonCode: 'capabilities-missing' })
  })

  it('reads a legacy Bridge that predates the state field', () => {
    // An older installed copy announces neither `state` nor `reasonCode`.
    // A complete capability set is still ready...
    const legacy: Record<string, unknown> = { ...validBridgeStatus() }
    delete legacy.state
    delete legacy.reasonCode
    delete legacy.bridgeBuild
    expect(interpretHarnessBridgeStatus(legacy)).toMatchObject({ availability: 'bridge-ready' })
    // ...and an incomplete one is incompatible rather than "loading", because
    // nothing in the document says it intends to finish.
    const legacyPartial: Record<string, unknown> = { ...legacy, capabilities: ['sessions'] }
    expect(interpretHarnessBridgeStatus(legacyPartial)).toMatchObject({
      availability: 'bridge-incompatible',
      reasonCode: 'capabilities-missing',
    })
  })

  it('forwards the specific host member instead of discarding it', () => {
    // The Bridge names the member that did not match, and the native layer
    // forwards it. The renderer is the layer the desktop displays, so it must
    // not be the one that throws the detail away — that would make the Rust-side
    // forwarding pointless.
    const named = { ...validBridgeStatus(), state: 'bridge-incompatible', reasonCode: 'host-shape-mismatch:agentPresets.recompose', capabilities: ['status'] }
    expect(interpretHarnessBridgeStatus(named)).toMatchObject({
      availability: 'bridge-incompatible',
      reasonCode: 'host-shape-mismatch:agentPresets.recompose',
    })

    // A `waiting:` reason describes a wait, so it must not be rendered as the
    // explanation for an incompatibility...
    const misleading = { ...named, reasonCode: 'waiting:workspaceRegistry' }
    expect(interpretHarnessBridgeStatus(misleading)).toMatchObject({
      availability: 'bridge-incompatible',
      reasonCode: 'capabilities-missing',
    })
    // ...and neither may a stale `ready` left over from an earlier status.
    expect(interpretHarnessBridgeStatus({ ...named, reasonCode: 'ready' })).toMatchObject({
      reasonCode: 'capabilities-missing',
    })

    // A shape mismatch arriving while the Bridge says it is loading is still a
    // load, so the loading vocabulary applies rather than the incompatible one.
    const duringLoad = { ...named, state: 'bridge-loading' }
    expect(interpretHarnessBridgeStatus(duringLoad)).toMatchObject({
      availability: 'bridge-loading',
      reasonCode: 'services-pending',
    })
  })

  it('treats a malformed capability list as not a Bridge', () => {
    expect(interpretHarnessBridgeStatus({ ...validBridgeStatus(), capabilities: [...validBridgeStatus().capabilities as string[], 7] }))
      .toMatchObject({ availability: 'bridge-incompatible', reasonCode: 'capabilities-missing' })
    expect(interpretHarnessBridgeStatus({ ...validBridgeStatus(), capabilities: 'sessions' }))
      .toMatchObject({ availability: 'bridge-incompatible', reasonCode: 'capabilities-missing' })
  })

  it('only reports readiness for bridge-ready', () => {
    expect(isHarnessReady('bridge-ready')).toBe(true)
    for (const availability of ['offline', 'web-only', 'bridge-loading', 'bridge-auth-unavailable', 'bridge-incompatible'] as const) {
      expect(isHarnessReady(availability)).toBe(false)
    }
  })

  it('uses an inspectable successful root response only as web-only, never as bridge-ready', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ status: 'unrelated-service' }) })
      .mockResolvedValueOnce({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchHarnessStatus('http://127.0.0.1:3080')).resolves.toEqual({
      availability: 'web-only',
      reasonCode: 'bridge-status-missing',
    })
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('does not downgrade a Bridge diagnosis to a root-page guess', async () => {
    // A Bridge that answers with a diagnostic must be reported as such, and the
    // root page must not be probed at all.
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ...validBridgeStatus(), state: 'bridge-loading', capabilities: ['status'] }),
    })
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchHarnessStatus('http://127.0.0.1:3080')).resolves.toMatchObject({
      availability: 'bridge-loading',
      reasonCode: 'services-pending',
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('fails closed when a browser fallback receives an opaque no-cors-style response', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce({ ok: false, status: 404 })
      .mockResolvedValueOnce({ ok: false, type: 'opaque', status: 0 })
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchHarnessStatus('http://127.0.0.1:3080')).resolves.toEqual({ availability: 'offline' })
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock.mock.calls[1]?.[1]).not.toMatchObject({ mode: 'no-cors' })
  })

  it('returns bridge-ready only after the full status document validates', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => validBridgeStatus() }))

    await expect(fetchHarnessStatus('http://127.0.0.1:3080')).resolves.toMatchObject({
      availability: 'bridge-ready',
      model: 'deepseek-chat',
    })
  })
})
