import { beforeAll, describe, expect, it } from 'vitest'
import type { ChatAdapter } from '../src/chat/adapter.ts'
import type { BackendMode } from '../src/domain/types.ts'

// AppCore's browser preview fallback is evaluated when App.tsx is imported.
// These are pure lifecycle tests, so a minimal window is enough and avoids
// mounting a full desktop surface just to verify stale-operation guards.
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

function adapter(mode: BackendMode): ChatAdapter {
  return {
    mode,
    async connect() {},
    disconnect() {},
    async send() {},
    async stop() {},
    async history() { return [] },
    subscribe() { return () => undefined },
  }
}

describe('App chat lifecycle isolation', () => {
  it('accepts only the mounted adapter for the backend it was created for', async () => {
    const { isCurrentChatOperation } = await import('../src/App.tsx')
    const api = adapter('deepseek-api')
    const harness = adapter('harness')

    expect(isCurrentChatOperation(api, 'deepseek-api', api, 'deepseek-api', false)).toBe(true)
    expect(isCurrentChatOperation(harness, 'harness', api, 'deepseek-api', false)).toBe(false)
    expect(isCurrentChatOperation(api, 'harness', api, 'deepseek-api', false)).toBe(false)
    expect(isCurrentChatOperation(api, 'deepseek-api', api, 'deepseek-api', true)).toBe(false)
  })

  it('keeps Harness selected and supplies a controlled disconnected state', async () => {
    const { HARNESS_DISCONNECTED_ERROR_PREFIX, harnessAvailabilityPatch } = await import('../src/App.tsx')
    const disconnected = harnessAvailabilityPatch('harness', 'offline')

    expect(disconnected).toMatchObject({ activity: 'idle' })
    expect(disconnected?.error).toContain(HARNESS_DISCONNECTED_ERROR_PREFIX)
    expect(disconnected?.error).toContain('已保留当前 Harness 会话和对话记录')
    expect(harnessAvailabilityPatch('deepseek-web', 'offline')).toBeUndefined()
    expect(harnessAvailabilityPatch('harness', 'bridge-ready', disconnected?.error)).toEqual({ activity: 'idle', error: undefined })
  })
})
