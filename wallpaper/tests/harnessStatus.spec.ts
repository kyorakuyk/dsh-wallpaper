import { afterEach, describe, expect, it, vi } from 'vitest'
import { compatibleHarnessBridgeStatus, fetchHarnessStatus } from '../src/connect/harness.ts'

function validBridgeStatus(): Record<string, unknown> {
  return {
    bridgeVersion: '1.0.0',
    protocolVersion: 1,
    dsh: 'online',
    authentication: 'ready',
    capabilities: [
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
  it('accepts only a ready v1 bridge with every required capability', () => {
    expect(compatibleHarnessBridgeStatus(validBridgeStatus())).toEqual({
      availability: 'bridge-ready',
      bridgeVersion: '1.0.0',
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

  it.each([
    {},
    { ...validBridgeStatus(), protocolVersion: 2 },
    { ...validBridgeStatus(), protocolVersion: '1' },
    { ...validBridgeStatus(), dsh: 'offline' },
    { ...validBridgeStatus(), authentication: 'unavailable' },
    { ...validBridgeStatus(), capabilities: ['sessions', 'resume', 'history', 'sse', 'cancel'] },
    { ...validBridgeStatus(), capabilities: [...validBridgeStatus().capabilities as string[], 7] },
  ])('rejects incompatible status documents', (document) => {
    expect(compatibleHarnessBridgeStatus(document)).toBeUndefined()
  })

  it('does not promote an arbitrary successful status endpoint to bridge-ready', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ status: 'unrelated-service' }) })
      .mockResolvedValueOnce({ ok: true, type: 'opaque' })
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchHarnessStatus('http://127.0.0.1:3080')).resolves.toEqual({ availability: 'web-only' })
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('returns bridge-ready only after the full status document validates', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => validBridgeStatus() }))

    await expect(fetchHarnessStatus('http://127.0.0.1:3080')).resolves.toMatchObject({
      availability: 'bridge-ready',
      model: 'deepseek-chat',
    })
  })
})
