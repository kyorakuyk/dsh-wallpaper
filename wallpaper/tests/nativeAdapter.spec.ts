import { afterEach, describe, expect, it, vi } from 'vitest'
import { NativeChatAdapter, ARCHIVED_SESSION_NOTICE, BLOCKED_TURN_NOTICE, isArchivedSessionError, isMissingSessionError } from '../src/chat/nativeAdapter.ts'
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
  /**
   * Rejections to hand to the next `sendChat` calls, in order, so a test can
   * model the bridge refusing a session it has archived. Empty = success.
   */
  sendErrors: unknown[] = []
  /** Session IDs returned by `connectHarness`, in call order. */
  connectSessionIds: string[] = []

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
      const failure = this.sendErrors.shift()
      if (failure) throw failure
      // 忠实于原生：`send_chat` 的 Harness 分支只回报 `Ok(())`（invoke 得到 null），
      // 会话 id 只由 `connect_harness` 给出。谁换会话、换到哪条，全看那一次连接。
      return mode === 'harness' ? undefined : options?.conversationId
    }) as typeof nativeRuntime.sendChat)
    install('cancelChat', (async (mode: string) => {
      if (this.cancelBarrier) await this.cancelBarrier
      this.cancels.push(mode)
      if (this.cancelError) throw this.cancelError
    }) as typeof nativeRuntime.cancelChat)
    install('connectHarness', (async (resumeSessionId: string | undefined, connectionId: string) => {
      this.connectCalls.push({ resumeSessionId, connectionId })
      return this.connectSessionIds.shift() ?? 'harness-session'
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

/**
 * 用户把一条会话归档掉之后，桥会拒绝它的消息。用户实测到的症状是"输入被吞了、灯还是绿的"：
 * 壁纸还绑在旧会话上，而那条会话已经没有人在听了。这一组测试把"换会话"这件事的全部后果
 * 钉住——包括**它必须被说出来**（用户的原则：不允许静默替换）。
 */
/**
 * "灯说已连接、一发却说没有会话"：切换主体**不会**重建适配器，探测范围换了、事件流与会话留在旧端点
 * 上，于是发送被原生拒绝。与归档的关键区别是：这里**没有任何东西被替换掉**，所以恢复必须静默完成
 * —— 用户只是第一次往这条端点上发话，不该收到"已换到新会话"的提示。
 */
describe('NativeChatAdapter missing-session recovery', () => {
  const missing = () => new Error('HARNESS_NO_SESSION: Harness 会话尚未建立')

  it('recognises the marker, and does not confuse it with an archived session', () => {
    expect(isMissingSessionError('HARNESS_NO_SESSION: Harness 会话尚未建立')).toBe(true)
    expect(isMissingSessionError(new Error('HARNESS_NO_SESSION: x'))).toBe(true)
    expect(isMissingSessionError('HARNESS_SESSION_ARCHIVED: 这条会话已归档')).toBe(false)
    expect(isMissingSessionError(undefined)).toBe(false)
  })

  it('connects and resends the same sentence once, without announcing a switch', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      native.sendErrors = [missing()]
      native.connectSessionIds = ['session-on-this-endpoint']

      await adapter.send('这句话要发出去')

      // 连上（这一步才会 `POST /sessions` 建立会话），然后把原话再发一次。
      expect(native.connectCalls).toHaveLength(2)
      expect(native.chatSends.map((send) => send.text)).toEqual(['这句话要发出去', '这句话要发出去'])
      // 静默：没有会话被替换，就不该出现"已换到新会话"那类通知。
      expect(events.some((event) => event.type === 'conversation-reset')).toBe(false)
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })
})

describe('NativeChatAdapter archived-session recovery', () => {
  /** 原生侧抛出的就是这条字符串；这里用 Error 包一层，两种形状都要能认出来。 */
  const archived = () => new Error(`HARNESS_SESSION_ARCHIVED: 这条会话已在桌面端归档，桥不再接受它的消息。`)

  it('recognises the marker in the plain string Tauri rejects with', () => {
    expect(isArchivedSessionError('HARNESS_SESSION_ARCHIVED: 这条会话已在桌面端归档')).toBe(true)
    expect(isArchivedSessionError(new Error('HARNESS_SESSION_ARCHIVED: x'))).toBe(true)
    // 别的 409（"另一台 DSH 占用这条会话"）绝不能冒充归档。
    expect(isArchivedSessionError('发送失败：409 另一台 DSH 正在使用这条会话')).toBe(false)
    expect(isArchivedSessionError(undefined)).toBe(false)
  })

  it('reconnects without a resume ID and resends the same sentence once', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      native.sendErrors = [archived()]
      native.connectSessionIds = ['fresh-daily-session']

      await adapter.send('这句话不能丢')

      // 换会话要同时做到两件事：重新建事件流（否则正是"有会话、没有事件流"那种静默状态），
      // 并且 **不带** resume id —— 带着刚被桥拒绝的那个 id 去 resume，只会再被拒一次。
      expect(native.connectCalls).toHaveLength(2)
      expect(native.connectCalls[1]?.resumeSessionId).toBeUndefined()
      expect(native.connectCalls[1]?.connectionId).not.toBe(native.connectCalls[0]?.connectionId)
      // 那句话必须重发，且发到**新**会话上。
      expect(native.chatSends.map((send) => send.text)).toEqual(['这句话不能丢', '这句话不能丢'])
      expect(native.chatSends[1]?.options?.conversationId).toBe('fresh-daily-session')
      expect(adapter.conversationId()).toBe('fresh-daily-session')
      // 通知在重发之前发出：重发一开始，轨道上那段转写就该已经在换了。
      expect(events).toEqual([
        { type: 'status', activity: 'sending' },
        { type: 'conversation-reset', reason: 'session-archived', message: ARCHIVED_SESSION_NOTICE },
        { type: 'status', activity: 'sending' },
      ])
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('accepts events only on the connection opened for the replacement session', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const archivedConnection = native.connectCalls[0]?.connectionId
      const events = collector(adapter)
      native.sendErrors = [archived()]
      native.connectSessionIds = ['fresh-daily-session']
      await adapter.send('这句话不能丢')

      const freshConnection = native.connectCalls[1]?.connectionId
      events.length = 0
      // 旧的（已归档的）事件流上晚到的东西：桥已经不在听那条会话，壁纸也不能再听。
      native.emit(harnessEvent(archivedConnection!, 'harness-session', { type: 'delta', text: '归档会话的晚到增量' }))
      expect(events).toEqual([])
      native.emit(harnessEvent(freshConnection!, 'fresh-daily-session', { type: 'message', role: 'user', content: '这句话不能丢' }))
      expect(events).toEqual([{ type: 'message', role: 'user', content: '这句话不能丢' }])
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('reports the failure instead of looping when the replacement is archived too', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      native.sendErrors = [archived(), archived()]
      native.connectSessionIds = ['fresh-daily-session']

      await expect(adapter.send('这句话')).rejects.toThrow('HARNESS_SESSION_ARCHIVED')

      // 只换一次、只重发一次：不能变成"重连—重发"之间的死循环。
      expect(native.connectCalls).toHaveLength(2)
      expect(native.chatSends).toHaveLength(2)
      expect(events.filter((event) => event.type === 'conversation-reset')).toHaveLength(1)
      expect(events.filter((event) => event.type === 'error')).toHaveLength(1)
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('never swaps the session when the failure is not an archive', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      native.sendErrors = [new Error('发送失败：409 另一台 DSH 正在使用这条会话')]

      await expect(adapter.send('这句话')).rejects.toThrow('409')

      // 没有被归档：会话、事件流、那句话都不许动，只有一条报错。
      expect(native.connectCalls).toHaveLength(1)
      expect(native.chatSends).toHaveLength(1)
      expect(adapter.conversationId()).toBe('harness-session')
      expect(events.some((event) => event.type === 'conversation-reset')).toBe(false)
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  it('says so when the replacement session cannot be established either', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      const originalConnect = nativeRuntime.connectHarness
      nativeRuntime.connectHarness = (async () => { throw new Error('bridge 未就绪') }) as typeof nativeRuntime.connectHarness
      native.sendErrors = [archived()]

      // 换不成新会话时**不能**假装换成了：用户必须同时看到"会话被归档了"和"换也没成功"。
      await expect(adapter.send('这句话')).rejects.toThrow('bridge 未就绪')
      nativeRuntime.connectHarness = originalConnect

      expect(native.chatSends).toHaveLength(1)
      const errors = events.filter((event) => event.type === 'error')
      expect(errors).toHaveLength(1)
      expect(errors[0] && 'message' in errors[0] ? errors[0].message : '').toContain('HARNESS_SESSION_ARCHIVED')
      expect(errors[0] && 'message' in errors[0] ? errors[0].message : '').toContain('bridge 未就绪')
      expect(events.some((event) => event.type === 'conversation-reset')).toBe(false)
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })

  /**
   * 宿主拒绝这一轮（桥把 `turn/end { reason.kind: "blocked" }` 翻成 `turn-blocked`）时，
   * 用户看到的是"发出去了、永远没回应、灯还是绿的"。所以除了那句错误，壁纸要**自己**换一条
   * 新会话并把这句重发一次 —— 与归档恢复同一套机制，因为对用户来说是同一件事。
   */
  it('leaves the session and resends once when the host refuses the turn', async () => {
    const native = new FakeNative()
    try {
      const adapter = await connectedHarness(native)
      const events = collector(adapter)
      const connectionId = native.connectCalls[0]?.connectionId
      expect(connectionId).toBeTruthy()
      await adapter.send('这句话不能丢')
      events.length = 0
      native.connectSessionIds = ['fresh-daily-session']

      native.emit(harnessEvent(connectionId!, 'harness-session', {
        type: 'error',
        code: 'turn-blocked',
        recoverable: true,
        message: 'DSH 拒绝了这一轮对话（原因：blocked）',
      }))
      await new Promise((resolve) => setTimeout(resolve, 0))

      // 用户先知道发生了什么，然后才看到我们换了会话并重发。
      expect(events[0]).toMatchObject({ type: 'error', code: 'turn-blocked' })
      expect(events[1]).toEqual({ type: 'conversation-reset', reason: 'turn-blocked', message: BLOCKED_TURN_NOTICE })
      expect(native.connectCalls).toHaveLength(2)
      expect(native.connectCalls[1]?.resumeSessionId).toBeUndefined()
      expect(native.chatSends.map((send) => send.text)).toEqual(['这句话不能丢', '这句话不能丢'])
      expect(native.chatSends[1]?.options?.conversationId).toBe('fresh-daily-session')
      adapter.disconnect()
    } finally {
      native.restore()
    }
  })
})
