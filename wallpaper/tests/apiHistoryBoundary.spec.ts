import { afterEach, describe, expect, it, vi } from 'vitest'
import { NativeChatAdapter } from '../src/chat/nativeAdapter.ts'
import { nativeRuntime } from '../src/native/runtime.ts'

/**
 * Minimal Tauri bridge stub. `nativeRuntime.apiHistory` resolves
 * `window.__TAURI_INTERNALS__.invoke`, so this captures the exact IPC payload
 * the renderer sends and lets a test script the reply.
 */
function fakeTauri(reply: (command: string, args: Record<string, unknown>) => unknown) {
  const invokes: Array<{ command: string; args: Record<string, unknown> }> = []
  vi.stubGlobal('window', {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        invokes.push({ command, args })
        return reply(command, args)
      },
    },
  })
  return invokes
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('api history IPC boundary', () => {
  it('sends the requested window and maps the bounded page without losing identity', async () => {
    const invokes = fakeTauri((command) => {
      expect(command).toBe('api_history')
      return {
        messages: [
          { id: 'm-9', role: 'assistant', content: '最近一轮', createdAt: 1_500, usage: { input: 1, output: 2 } },
        ],
        totalMessages: 320,
        hasMore: true,
        bytes: 4_096,
        limit: 50,
      }
    })

    const page = await nativeRuntime.apiHistory('conversation-a', 50)
    expect(invokes).toHaveLength(1)
    expect(invokes[0]?.args).toEqual({ conversationId: 'conversation-a', limit: 50 })
    // The durable identity survives the windowed read, so React keys and the
    // session cost remain stable instead of being regenerated.
    expect(page.messages).toEqual([
      { id: 'm-9', role: 'assistant', content: '最近一轮', createdAt: 1_500, usage: { input: 1, output: 2 } },
    ])
    expect(page.totalMessages).toBe(320)
    expect(page.hasMore).toBe(true)
    expect(page.bytes).toBe(4_096)
    expect(page.limit).toBe(50)
  })

  it('treats a legacy reply without pagination fields as a complete small transcript', async () => {
    fakeTauri(() => ({ messages: [{ id: 'm-1', role: 'user', content: 'hi', createdAt: 7 }] }))
    const page = await nativeRuntime.apiHistory('conversation-a')
    expect(page.messages).toHaveLength(1)
    expect(page.totalMessages).toBe(1)
    expect(page.hasMore).toBe(false)
    expect(page.limit).toBe(1)
  })

  it('asks for the bounded window and reports whether earlier turns exist', async () => {
    const calls: Array<{ conversationId: string; limit?: number }> = []
    const original = nativeRuntime.apiHistory
    nativeRuntime.apiHistory = (async (conversationId: string, limit?: number) => {
      calls.push({ conversationId, limit })
      return {
        messages: [
          { id: 'm-9', role: 'assistant' as const, content: '最近一轮', createdAt: 1_500 },
        ],
        totalMessages: 320,
        hasMore: true,
        bytes: 4_096,
        limit: limit ?? 0,
      }
    }) as typeof nativeRuntime.apiHistory
    try {
      const adapter = new NativeChatAdapter('deepseek-api', {}, 'conversation-a')
      // `ChatAdapter.history()` keeps its shared signature and asks for the
      // native default window rather than an unbounded clone.
      expect(await adapter.history()).toHaveLength(1)
      expect(calls[0]).toEqual({ conversationId: 'conversation-a', limit: undefined })
      expect(await adapter.boundedHistory(500)).toHaveLength(1)
      expect(calls[1]).toEqual({ conversationId: 'conversation-a', limit: 500 })
      // The "load earlier" affordance is a cheap one-message probe.
      expect(await adapter.hasMoreApiHistory()).toBe(true)
      expect(calls[2]).toEqual({ conversationId: 'conversation-a', limit: 1 })
    } finally {
      nativeRuntime.apiHistory = original
    }
  })

  it('reports no earlier API turns for a single-message transcript', async () => {
    const original = nativeRuntime.apiHistory
    nativeRuntime.apiHistory = (async () => ({
      messages: [],
      totalMessages: 1,
      hasMore: false,
      bytes: 0,
      limit: 1,
    })) as typeof nativeRuntime.apiHistory
    try {
      const adapter = new NativeChatAdapter('deepseek-api', {}, 'conversation-a')
      expect(await adapter.hasMoreApiHistory()).toBe(false)
    } finally {
      nativeRuntime.apiHistory = original
    }
  })

  it('never probes API history on the Harness or web paths', async () => {
    const original = nativeRuntime.apiHistory
    const calls: unknown[] = []
    nativeRuntime.apiHistory = (async (...args: unknown[]) => {
      calls.push(args)
      return { messages: [], totalMessages: 0, hasMore: false, bytes: 0, limit: 0 }
    }) as typeof nativeRuntime.apiHistory
    try {
      expect(await new NativeChatAdapter('harness').hasMoreApiHistory()).toBe(false)
      expect(await new NativeChatAdapter('deepseek-web').hasMoreApiHistory()).toBe(false)
      expect(calls).toEqual([])
    } finally {
      nativeRuntime.apiHistory = original
    }
  })

  it('exposes delete and clear against the background-owned commands', async () => {
    const invokes = fakeTauri((command) => {
      if (command === 'delete_api_conversation') return true
      if (command === 'clear_api_history') return 3
      return undefined
    })

    expect(await nativeRuntime.deleteApiConversation('conversation-a')).toBe(true)
    expect(await nativeRuntime.clearApiHistory()).toBe(3)
    expect(invokes).toEqual([
      { command: 'delete_api_conversation', args: { conversationId: 'conversation-a' } },
      { command: 'clear_api_history', args: {} },
    ])
  })
})
