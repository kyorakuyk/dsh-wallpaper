import { describe, expect, it, vi } from 'vitest'
import { acceptsScopedChatEvent, isCurrentAdapter, NativeChatAdapter } from '../src/chat/nativeAdapter.ts'
import type { ScopedChatEvent } from '../src/domain/types.ts'
import { nativeRuntime } from '../src/native/runtime.ts'

const apiEvent: ScopedChatEvent = {
  type: 'delta',
  text: 'current',
  backend: 'deepseek-api',
  conversationId: 'api-current',
  requestId: 'request-current',
}

describe('native chat event isolation', () => {
  it('accepts only the current API backend, conversation, and request', () => {
    expect(acceptsScopedChatEvent('deepseek-api', 'api-current', 'request-current', apiEvent)).toBe(true)
    expect(acceptsScopedChatEvent('harness', 'api-current', 'request-current', apiEvent)).toBe(false)
    expect(acceptsScopedChatEvent('deepseek-api', 'api-old', 'request-current', apiEvent)).toBe(false)
    expect(acceptsScopedChatEvent('deepseek-api', 'api-current', 'request-old', apiEvent)).toBe(false)
    expect(acceptsScopedChatEvent('deepseek-api', 'api-current', 'request-current', { ...apiEvent, backend: undefined })).toBe(false)
  })

  it('still isolates Harness by the scoped session', () => {
    const harnessEvent: ScopedChatEvent = {
      type: 'message',
      role: 'assistant',
      content: 'current Harness reply',
      backend: 'harness',
      conversationId: 'harness-current',
      requestId: 'connection-current',
    }
    expect(acceptsScopedChatEvent('harness', 'harness-current', 'connection-current', harnessEvent)).toBe(true)
    expect(acceptsScopedChatEvent('harness', 'harness-old', 'connection-current', harnessEvent)).toBe(false)
    // A different bridge connection is rejected by NativeChatAdapter before
    // this helper, because an initial SSE snapshot may bind the session ID.
  })

  it('rejects late async work after the adapter was replaced or disposed', () => {
    const current = {}
    const previous = {}
    expect(isCurrentAdapter(current, current, false)).toBe(true)
    expect(isCurrentAdapter(current, previous, false)).toBe(false)
    expect(isCurrentAdapter(current, current, true)).toBe(false)
  })

  it('reconciles a durable second turn when its live SSE events were missed', async () => {
    vi.useFakeTimers()
    const originalSendChat = nativeRuntime.sendChat
    const originalHistory = nativeRuntime.harnessHistory
    const initial = [
      { id: 'u1', role: 'user' as const, content: '第一轮', createdAt: 1 },
      { id: 'a1', role: 'assistant' as const, content: '第一轮答复', createdAt: 2 },
    ]
    const completed = [
      ...initial,
      { id: 'u2', role: 'user' as const, content: '第二轮', createdAt: 3 },
      { id: 'a2', role: 'assistant' as const, content: '第二轮答复', createdAt: 4 },
    ]
    const history = vi.fn()
      .mockResolvedValueOnce(initial)
      .mockResolvedValue(completed)
    nativeRuntime.sendChat = vi.fn(async () => 'harness-session')
    nativeRuntime.harnessHistory = history
    try {
      const adapter = new NativeChatAdapter('harness')
      const events: Array<{ type: string; role?: string; content?: string; activity?: string }> = []
      adapter.subscribe((event) => events.push(event as typeof events[number]))
      await adapter.history()
      await adapter.send('第二轮')
      await vi.advanceTimersByTimeAsync(750)
      expect(history).toHaveBeenCalledTimes(2)
      expect(events).toEqual(expect.arrayContaining([
        { type: 'message', role: 'user', content: '第二轮' },
        { type: 'message', role: 'assistant', content: '第二轮答复' },
        { type: 'status', activity: 'done' },
      ]))
      adapter.disconnect()
    } finally {
      nativeRuntime.sendChat = originalSendChat
      nativeRuntime.harnessHistory = originalHistory
      vi.useRealTimers()
    }
  })
})
