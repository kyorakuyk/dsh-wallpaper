import { describe, expect, it } from 'vitest'
import { acceptsScopedChatEvent, isCurrentAdapter } from '../src/chat/nativeAdapter.ts'
import type { ScopedChatEvent } from '../src/domain/types.ts'

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
})
