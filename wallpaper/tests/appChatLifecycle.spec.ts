import { beforeAll, describe, expect, it } from 'vitest'
import type { ChatAdapter } from '../src/chat/adapter.ts'
import { NativeChatAdapter } from '../src/chat/nativeAdapter.ts'
import type { BackendMode } from '../src/domain/types.ts'
import { nativeRuntime } from '../src/native/runtime.ts'

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
    const { HARNESS_DISCONNECTED_ERROR_PREFIX, canAutoSelectHarness, harnessAvailabilityPatch } = await import('../src/App.tsx')
    const disconnected = harnessAvailabilityPatch('harness', 'offline')

    expect(disconnected).toMatchObject({ activity: 'idle' })
    expect(disconnected?.error).toContain(HARNESS_DISCONNECTED_ERROR_PREFIX)
    expect(disconnected?.error).toContain('已保留当前 Harness 会话和对话记录')
    expect(harnessAvailabilityPatch('deepseek-web', 'offline')).toBeUndefined()
    expect(harnessAvailabilityPatch('harness', 'bridge-ready', disconnected?.error)).toEqual({ activity: 'idle', error: undefined })
    expect(canAutoSelectHarness('bridge-ready', 'deepseek-web', true)).toBe(true)
    expect(canAutoSelectHarness('offline', 'harness', true)).toBe(false)
    expect(canAutoSelectHarness('offline', 'harness', false)).toBe(false)
  })

  it('keeps an API adapter lifecycle stable while committing later request settings', async () => {
    const { apiAdapterOptionsFromSettings, chatAdapterLifecycleKey, updateApiAdapterOptions } = await import('../src/App.tsx')
    const first = {
      deepseekApi: {
        baseUrl: 'https://api.deepseek.com',
        model: 'deepseek-chat',
        priceInputPerMillion: 1,
        priceOutputPerMillion: 2,
      },
    }
    const edited = {
      deepseekApi: {
        baseUrl: 'https://proxy.example.test/v1',
        model: 'deepseek-reasoner',
        priceInputPerMillion: 3,
        priceOutputPerMillion: 4,
      },
    }
    const stableOptions = apiAdapterOptionsFromSettings(first)
    const mountedLifecycle = chatAdapterLifecycleKey('deepseek-api', 7)

    // The mounted adapter holds this exact object. Updating it for the next
    // request must not manufacture a new lifecycle/effect identity.
    expect(updateApiAdapterOptions(stableOptions, edited)).toBe(stableOptions)
    expect(stableOptions).toEqual({
      baseUrl: 'https://proxy.example.test/v1',
      model: 'deepseek-reasoner',
      priceInputPerMillion: 3,
      priceOutputPerMillion: 4,
    })
    expect(chatAdapterLifecycleKey('deepseek-api', 7)).toBe(mountedLifecycle)
    expect(chatAdapterLifecycleKey('deepseek-api', 8)).not.toBe(mountedLifecycle)
  })

  it('applies daily policy only when unlocking after a local calendar rollover', async () => {
    const { chatAdapterLifecycleKey, isInitialSystemSessionSignal, shouldStartNewConversationOnUnlock } = await import('../src/App.tsx')
    const beforeMidnight = new Date(2026, 7, 18, 23, 59, 0)
    const afterMidnight = new Date(2026, 7, 19, 0, 1, 0)

    // Changing a policy in Settings changes neither adapter identity nor an
    // in-flight stream. The daily decision belongs to the later unlock event.
    expect(chatAdapterLifecycleKey('deepseek-api', 7)).toBe(chatAdapterLifecycleKey('deepseek-api', 7))
    expect(shouldStartNewConversationOnUnlock('daily', '2026-08-18', beforeMidnight)).toBe(false)
    expect(shouldStartNewConversationOnUnlock('daily', '2026-08-18', afterMidnight)).toBe(true)
    expect(shouldStartNewConversationOnUnlock('new-on-unlock', '2026-08-19', afterMidnight)).toBe(true)
    expect(shouldStartNewConversationOnUnlock('resume-last', '2026-08-18', afterMidnight)).toBe(false)
    expect(isInitialSystemSessionSignal(false, 'resume')).toBe(true)
    expect(isInitialSystemSessionSignal(false, 'unlocked')).toBe(false)
    expect(isInitialSystemSessionSignal(true, 'resume')).toBe(false)
  })

  it('persists an API session as soon as send has allocated its conversation ID', async () => {
    const { disposeChatAdapter, persistConversationPointerWhenAvailable } = await import('../src/App.tsx')
    const writes: Array<{ backend: BackendMode; id: string }> = []
    const apiWithAllocatedId = {
      ...adapter('deepseek-api'),
      conversationId: () => 'api-created-before-native-await',
    }

    // This is deliberately invoked without waiting for a send promise. It
    // models the narrow interval where NativeChatAdapter has synchronously
    // allocated its UUID but the native request is still pending.
    expect(persistConversationPointerWhenAvailable(
      apiWithAllocatedId,
      'deepseek-api',
      (backend, id) => writes.push({ backend, id }),
    )).toBe('api-created-before-native-await')
    expect(writes).toEqual([{ backend: 'deepseek-api', id: 'api-created-before-native-await' }])

    // Effect teardown uses the same helper; switching to another backend must
    // not write the pending API ID under that new backend's pointer.
    expect(persistConversationPointerWhenAvailable(
      apiWithAllocatedId,
      'harness',
      (backend, id) => writes.push({ backend, id }),
    )).toBeUndefined()
    expect(writes).toHaveLength(1)

    let unsubscribed = false
    let disconnected = false
    const teardownAdapter = {
      ...apiWithAllocatedId,
      disconnect: () => { disconnected = true },
    }
    disposeChatAdapter(
      teardownAdapter,
      'deepseek-api',
      () => { unsubscribed = true },
      (backend, id) => writes.push({ backend, id }),
    )
    expect(writes.at(-1)).toEqual({ backend: 'deepseek-api', id: 'api-created-before-native-await' })
    expect(unsubscribed).toBe(true)
    expect(disconnected).toBe(true)
  })

  it('makes the real native API session ID available before its native request settles', async () => {
    const originalSendChat = nativeRuntime.sendChat
    let settleRequest: ((id: string | undefined) => void) | undefined
    nativeRuntime.sendChat = async () => new Promise<string | undefined>((resolve) => { settleRequest = resolve })
    try {
      const adapter = new NativeChatAdapter('deepseek-api')
      const pending = adapter.send('persist this before a backend switch')
      const id = adapter.conversationId()
      const writes: Array<{ backend: BackendMode; id: string }> = []
      const { persistConversationPointerWhenAvailable } = await import('../src/App.tsx')

      expect(id).toMatch(/.+/)
      expect(persistConversationPointerWhenAvailable(
        adapter,
        'deepseek-api',
        (backend, conversationId) => writes.push({ backend, id: conversationId }),
      )).toBe(id)
      expect(writes).toEqual([{ backend: 'deepseek-api', id }])

      settleRequest?.(id)
      await pending
    } finally {
      nativeRuntime.sendChat = originalSendChat
    }
  })
})
