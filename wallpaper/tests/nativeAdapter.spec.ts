import { afterEach, describe, expect, it, vi } from 'vitest'
import { NativeChatAdapter } from '../src/chat/nativeAdapter.ts'
import type { ChatEvent, ChatMessage, ScopedChatEvent } from '../src/domain/types.ts'
import { nativeRuntime, type NativeSendOptions } from '../src/native/runtime.ts'

/**
 * A controllable stand-in for the Tauri bridge. It records every native call
 * and lets a test deliver a scoped `chat-event` exactly when the real WebView
 * would (including after a Stop click).
 */
class FakeNative {
  readonly chatSends: Array<{ mode: string; text: string; options?: NativeSendOptions }> = []
  cancels: string[] = []
  readonly connectCalls: Array<{ resumeSessionId?: string; connectionId: string }> = []
  historyCount = 0
  history: ChatMessage[] = []
  /** When set, `sendChat` waits for this instead of resolving immediately. */
  sendBarrier: Promise<void> | undefined
  /** When set, `cancelChat` waits for this before rejecting with `cancelError`. */
  cancelBarrier: Promise<void> | undefined
  cancelError: Error | undefined
  /** Set by a test to model a bridge whose `listenChat` still has to settle. */
  listenBarrier: Promise<void> | undefined

  private readonly listeners = new Set<(event: ScopedChatEvent) => void>()
  private readonly originals: Array<[keyof typeof nativeRuntime, unknown]> = []

  constructor() {
    const install = <K extends keyof typeof nativeRuntime>(key: K, value: (typeof nativeRuntime)[K]) => {
      this.originals.push([key, nativeRuntime[key]])
      nativeRuntime[key] = value as never
    }
    install('listenChat', (async (listener: (event: ScopedChatEvent) => void) => {
      if (this.listenBarrier) await this.listenBarrier
      this.listeners.add(listener)
      return () => { this.listeners.delete(listener) }
    }) as typeof nativeRuntime.listenChat)
    install('sendChat', (async (mode: string, text: string, options?: NativeSendOptions) => {
      this.chatSends.push({ mode, text, options })
      if (this.sendBarrier) await this.sendBarrier
      return mode === 'harness' ? 'harness-session' : options?.conversationId
    }) as typeof nativeRuntime.sendChat)
    install('cancelChat', (async (mode: string) => {
      if (this.cancelBarrier) await this.cancelBarrier
      this.cancels.push(mode)
      if (this.cancelError) throw this.cancelError
    }) as typeof nativeRuntime.cancelChat)
    install('connectHarness', (async (resumeSessionId: string | undefined, connectionId: string) => {
      this.connectCalls.push({ resumeSessionId, connectionId })
      return 'harness-session'
    }) as typeof nativeRuntime.connectHarness)
    install('harnessHistory', (async () => {
      this.historyCount += 1
      return this.history
    }) as typeof nativeRuntime.harnessHistory)
  }

  /** Number of live `chat-event` subscriptions; catches leaked listeners. */
  get listenerCount(): number { return this.listeners.size }

  emit(event: ScopedChatEvent): void {
    for (const listener of [...this.listeners]) listener(event)
  }

  restore(): void {
    for (const [key, value] of this.originals) nativeRuntime[key] = value as never
    this.originals.length = 0
    this.listeners.clear()
  }
}

function harnessEvent(connectionId: string, conversationId: string, event: ChatEvent): ScopedChatEvent {
  return { ...event, backend: 'harness', conversationId, requestId: connectionId }
}

function apiEvent(requestId: string, conversationId: string, event: ChatEvent): ScopedChatEvent {
  return { ...event, backend: 'deepseek-api', conversationId, requestId }
}

function collector(adapter: NativeChatAdapter): ChatEvent[] {
  const events: ChatEvent[] = []
  adapter.subscribe((event) => events.push(event))
  return events
}

async function connectedHarness(native: FakeNative): Promise<NativeChatAdapter> {
  const adapter = new NativeChatAdapter('harness')
  await adapter.connect()
  return adapter
}

const idle = { type: 'status', activity: 'idle' } as const

afterEach(() => {
  vi.useRealTimers()
})

describe('NativeChatAdapter stop lifecycle', () => {
  it('publishes idle before the native cancellation promise settles', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      await adapter.send('写一段很长的回答')
      events.length = 0

      let releaseCancel: (() => void) | undefined
      native.cancelBarrier = new Promise<void>((resolve) => { releaseCancel = resolve })
      const stopped = adapter.stop()
      // The user-visible requirement: the send key is usable before the native
      // IPC round-trip completes.
      expect(events).toEqual([idle])
      releaseCancel?.()
      await stopped
      expect(events).toEqual([idle])
    } finally {
      native.restore()
    }
  })

  it('clears the history reconciliation timer so a cancelled turn never polls again', async () => {
    vi.useFakeTimers()
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      collector(adapter)
      await adapter.send('生成一段很长的回答')

      await adapter.stop()
      // The stop path reconnects the bridge but must not leave a reconciler
      // polling `harness_history` for the cancelled turn.
      await vi.advanceTimersByTimeAsync(10_000)
      expect(native.historyCount).toBe(0)
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('does not publish a reconcile history read that resolves after the user stopped', async () => {
    vi.useFakeTimers()
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      await adapter.send('生成一段很长的回答')
      native.history = [
        { id: 'u1', role: 'user', content: '生成一段很长的回答', createdAt: 1 },
        { id: 'a1', role: 'assistant', content: '被取消的半截回答', createdAt: 2 },
      ]
      // Let the reconciler start its native history read, then stop this turn
      // before that read settles.
      let releaseHistory: (() => void) | undefined
      const barrier = new Promise<void>((resolve) => { releaseHistory = resolve })
      const originalHistory = nativeRuntime.harnessHistory
      nativeRuntime.harnessHistory = (async () => {
        native.historyCount += 1
        await barrier
        return native.history
      }) as typeof nativeRuntime.harnessHistory

      await vi.advanceTimersByTimeAsync(700)
      expect(native.historyCount).toBe(1)
      await adapter.stop()
      releaseHistory?.()
      await vi.advanceTimersByTimeAsync(5000)
      nativeRuntime.harnessHistory = originalHistory

      expect(events).toEqual([{ type: 'status', activity: 'sending' }, idle])
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('rejects late delta, message, and status from a stopped Harness turn', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const connectionId = native.connectCalls[0]?.connectionId
      expect(connectionId).toBeTruthy()
      const events = collector(adapter)
      await adapter.send('写一段很长的回答')
      await adapter.stop()
      events.length = 0

      native.emit(harnessEvent(connectionId!, 'harness-session', { type: 'delta', text: '晚到的增量' }))
      native.emit(harnessEvent(connectionId!, 'harness-session', { type: 'message', role: 'assistant', content: '晚到的完整回答' }))
      native.emit(harnessEvent(connectionId!, 'harness-session', { type: 'status', activity: 'streaming' }))

      expect(events).toEqual([])
    } finally {
      native.restore()
    }
  })

  it('renews the Harness event scope for the next turn while keeping the DSH session', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const firstConnection = native.connectCalls[0]?.connectionId
      expect(firstConnection).toBeTruthy()
      const events = collector(adapter)
      await adapter.send('第一轮')

      await adapter.stop()
      // A cancelled adapter must not replace the DSH session it was bound to.
      expect(native.connectCalls[1]?.resumeSessionId).toBe('harness-session')
      const secondConnection = native.connectCalls[1]?.connectionId
      expect(secondConnection).toBeTruthy()
      expect(secondConnection).not.toBe(firstConnection)
      events.length = 0

      // The previous event scope must not be usable to re-enter streaming.
      native.emit(harnessEvent(firstConnection!, 'harness-session', { type: 'delta', text: '旧连接增量' }))
      native.emit(harnessEvent(firstConnection!, 'harness-session', { type: 'status', activity: 'streaming' }))
      native.emit(harnessEvent(firstConnection!, 'harness-session', { type: 'message', role: 'assistant', content: '旧连接回答' }))
      expect(events).toEqual([])

      await adapter.send('第二轮')
      events.length = 0
      native.emit(harnessEvent(secondConnection!, 'harness-session', { type: 'delta', text: '新回答' }))
      native.emit(harnessEvent(secondConnection!, 'harness-session', { type: 'message', role: 'assistant', content: '新回答' }))
      expect(events).toEqual([
        { type: 'delta', text: '新回答' },
        { type: 'message', role: 'assistant', content: '新回答' },
        { type: 'status', activity: 'done' },
      ])
    } finally {
      native.restore()
    }
  })

  it('keeps a stopped Harness turn from polluting a turn sent immediately after', async () => {
    vi.useFakeTimers()
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      await adapter.send('第一轮')
      await adapter.stop()

      await adapter.send('第二轮')
      events.length = 0
      native.history = [
        { id: 'u1', role: 'user', content: '第一轮', createdAt: 1 },
        { id: 'a1', role: 'assistant', content: '第一轮被取消', createdAt: 2 },
        { id: 'u2', role: 'user', content: '第二轮', createdAt: 3 },
        { id: 'a2', role: 'assistant', content: '第二轮答复', createdAt: 4 },
      ]
      await vi.advanceTimersByTimeAsync(1000)
      // The cancelled turn is not replayed as live stream state, but the
      // durable transcript (including what DSH already committed for it) is
      // restored in order, followed by the new turn.
      expect(events).toEqual([
        { type: 'message', role: 'user', content: '第一轮' },
        { type: 'message', role: 'assistant', content: '第一轮被取消' },
        { type: 'message', role: 'user', content: '第二轮' },
        { type: 'message', role: 'assistant', content: '第二轮答复' },
        { type: 'status', activity: 'done' },
      ])
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('keeps an API turn stopped even when the native cancellation fails', async () => {
    const native = new FakeNative()
    try {
      const adapter = new NativeChatAdapter('deepseek-api')
      await adapter.connect()
      const events = collector(adapter)
      await adapter.send('写一段很长的回答')
      const requestId = native.chatSends[0]?.options?.requestId
      const conversationId = adapter.conversationId()
      expect(requestId).toBeTruthy()
      expect(conversationId).toBeTruthy()
      events.length = 0

      native.cancelError = new Error('native cancel failed')
      await expect(adapter.stop()).rejects.toThrow('native cancel failed')
      expect(events).toEqual([idle])

      // A late event for the cancelled request must stay rejected even though
      // the native cancellation itself failed.
      native.emit(apiEvent(requestId!, conversationId!, { type: 'delta', text: '晚到增量' }))
      native.emit(apiEvent(requestId!, conversationId!, { type: 'status', activity: 'streaming' }))
      expect(events).toEqual([idle])
    } finally {
      native.restore()
    }
  })

  it('isolates a late API event from the previous request after a new send', async () => {
    const native = new FakeNative()
    try {
      const adapter = new NativeChatAdapter('deepseek-api')
      await adapter.connect()
      const events = collector(adapter)
      await adapter.send('第一轮')
      const firstRequest = native.chatSends[0]?.options?.requestId
      const conversationId = adapter.conversationId()
      await adapter.stop()
      await adapter.send('第二轮')
      const secondRequest = native.chatSends[1]?.options?.requestId
      expect(secondRequest).not.toBe(firstRequest)
      events.length = 0

      native.emit(apiEvent(firstRequest!, conversationId!, { type: 'delta', text: '旧请求增量' }))
      native.emit(apiEvent(secondRequest!, conversationId!, { type: 'delta', text: '新请求增量' }))
      native.emit(apiEvent(secondRequest!, conversationId!, { type: 'message', role: 'assistant', content: '新请求回答' }))
      expect(events).toEqual([
        { type: 'delta', text: '新请求增量' },
        { type: 'message', role: 'assistant', content: '新请求回答' },
      ])
    } finally {
      native.restore()
    }
  })

  it('makes a duplicate stop a no-op that keeps one live event scope', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      collector(adapter)
      await adapter.send('写一段回答')
      await adapter.stop()
      const connectionsAfterStop = native.connectCalls.length
      const scopeAfterStop = native.connectCalls.at(-1)?.connectionId

      await adapter.stop()
      expect(native.cancels).toEqual(['harness'])
      expect(native.connectCalls).toHaveLength(connectionsAfterStop)
      expect(native.listenerCount).toBe(1)
      // The surviving scope is still the usable one for the next turn.
      native.emit(harnessEvent(scopeAfterStop!, 'harness-session', { type: 'delta', text: '幸存作用域' }))
    } finally {
      native.restore()
    }
  })

  it('does not open a Harness scope when it is disposed while listenChat is pending', async () => {
    const native = new FakeNative()
    try {
      let releaseListen: (() => void) | undefined
      native.listenBarrier = new Promise<void>((resolve) => { releaseListen = resolve })
      const adapter = new NativeChatAdapter('harness')
      adapter.subscribe(() => undefined)
      const connecting = adapter.connect()
      // The React effect tears the adapter down before its own connect
      // resolved, which is exactly the race the disposer guard covers.
      adapter.disconnect()
      releaseListen?.()
      await connecting

      expect(native.connectCalls).toHaveLength(0)
      expect(native.listenerCount).toBe(0)
    } finally {
      native.restore()
    }
  })

  it('does not leak an event listener across connect, stop, and disconnect', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      expect(native.listenerCount).toBe(1)
      await adapter.send('写一段回答')
      // The stop path replaces the bridge connection; exactly one listener
      // must be live so a replaced scope cannot double-publish.
      await adapter.stop()
      expect(native.listenerCount).toBe(1)
      adapter.disconnect()
      expect(native.listenerCount).toBe(0)
    } finally {
      native.restore()
    }
  })
})
