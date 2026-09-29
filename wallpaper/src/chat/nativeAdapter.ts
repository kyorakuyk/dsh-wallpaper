import type { BackendMode, ChatMessage, ScopedChatEvent } from '../domain/types.ts'
import { nativeRuntime, type NativeSendOptions } from '../native/runtime.ts'
import { EventChatAdapter, type SendOptions } from './adapter.ts'

/**
 * The third argument is backend-scoped and deliberately not one shared field:
 * for `deepseek-api` it is the per-request ID, while for `harness` the native
 * side stamps every event of one SSE connection with the connection ID. Those
 * are different lifetimes (a turn vs. the adapter's event scope), so callers
 * must pass the matching one instead of reusing a single `requestId`.
 */
export function acceptsScopedChatEvent(
  mode: BackendMode,
  sessionId: string | undefined,
  scopeId: string | undefined,
  event: ScopedChatEvent,
): boolean {
  if (event.backend !== mode || !event.conversationId || event.conversationId !== sessionId) return false
  // The web adapter keeps its own stricter request-id gate.
  return mode !== 'deepseek-api' || event.requestId === scopeId
}

/** Prevent an async connect/history completion from an old adapter from
 * overwriting the currently mounted backend after a mode switch. */
export function isCurrentAdapter(active: unknown, candidate: unknown, disposed: boolean): boolean {
  return !disposed && active === candidate
}

/**
 * 原生侧"这条会话已被归档"的稳定标记，与 `wallpaper/src-tauri/src/chat.rs` 的
 * `HARNESS_SESSION_ARCHIVED` 一一对应（那边注释写明这是**跨进程契约**，改一处就要改另一处）。
 *
 * 用**包含**匹配而不是相等：Tauri 拒绝一条命令时抛的是那条 Rust 错误字符串本身（不是 Error），
 * 而它对用户话术的改动不该把这条机器可读的标识一起弄丢。
 */
export const HARNESS_SESSION_ARCHIVED = 'HARNESS_SESSION_ARCHIVED'

export function isArchivedSessionError(error: unknown): boolean {
  return String(error ?? '').includes(HARNESS_SESSION_ARCHIVED)
}

/**
 * 原生侧"这条端点还没有会话"的稳定标记，与 `chat.rs` 的 `HARNESS_NO_SESSION` 一一对应。
 *
 * 触发它的情形是"看着连上了、其实没有会话"：**切换主体不会重建适配器**（适配器持有的是"连着哪
 * 个端点、哪条会话"），于是探测范围换了、事件流与会话留在旧端点上。与"被归档"不同，这里**没有
 * 任何东西被替换掉** —— 用户只是第一次往这条端点上发话，所以恢复可以静默完成：连上（`POST
 * /sessions` 会建立会话），再把这句话原样发一次，不发任何"已换到新会话"的提示。
 */
export const HARNESS_NO_SESSION = 'HARNESS_NO_SESSION'

export function isMissingSessionError(error: unknown): boolean {
  return String(error ?? '').includes(HARNESS_NO_SESSION)
}

/**
 * 换会话时**必须**说清楚的一句。用户归档掉一条会话之后，桥拒绝它的消息，我们能做的只有
 * 在今天的新会话里重发——但"把用户刚说的话挪到另一条会话里"这件事不能静默发生（用户对主体
 * 的同一原则：不允许静默替换），所以这句话就是他看到的凭据。
 */
export const ARCHIVED_SESSION_NOTICE = '这条会话已被归档，已在今天的新会话里重新发送。'

/**
 * 宿主拒绝这一轮时说的那句。与归档不同：会话还在、只是不再接受我们的消息（实测：被归档的
 * 会话，`turn/end` 的 `reason.kind` 是 `blocked`）。给用户的动作是同一个 —— 换一条新会话。
 */
export const BLOCKED_TURN_NOTICE = 'DSH 拒绝了这一轮（这条会话已被归档）；已在今天的新会话里重新发送。'

export class NativeChatAdapter extends EventChatAdapter {
  readonly mode: BackendMode
  private nativeUnsubscribe: (() => void) | undefined
  private sessionId: string | undefined
  /**
   * Harness event scope. Stable for the lifetime of one SSE connection so
   * every event of every turn on that connection matches, and renewed only by
   * `stop()`. It must never be cleared while the adapter stays mounted,
   * otherwise the next Harness turn would have no scope to accept events on.
   */
  private harnessConnectionId: string | undefined
  /** DeepSeek API request ID. Created per `send()` and cleared by `stop()`. */
  private apiRequestId: string | undefined
  /**
   * Local isolation token for one user turn. Late deltas/messages/status from
   * a stopped turn fail this check before they can repaint the composer.
   */
  private turnToken: string | undefined
  private turnActive = false
  /** 这一轮的原话，供"被宿主拒绝之后重发一次"用。 */
  private lastTurnText: string | undefined
  /** 本轮已经为"被拒绝"换过一次会话：只换一次，不在换—重发之间打转。 */
  private blockedTurnRecovered = false
  private readonly nativeOptions: NativeSendOptions
  private readonly deliveredMessageCounts = new Map<string, number>()
  private reconcileTimer: ReturnType<typeof setTimeout> | undefined
  private reconcileInFlight = false
  private reconcileAttempts = 0
  private disposed = false

  constructor(mode: BackendMode, nativeOptions: NativeSendOptions = {}, resumeSessionId?: string) {
    super()
    this.mode = mode
    this.nativeOptions = nativeOptions
    this.sessionId = resumeSessionId
  }

  async connect(): Promise<void> {
    this.disposed = false
    await this.openEventScope()
  }

  /**
   * Create a fresh Harness event scope (new bridge connection ID) while
   * keeping the DSH session ID. Used by `connect()` and after `stop()`.
   */
  private async openEventScope(): Promise<void> {
    const connectionId = crypto.randomUUID()
    this.nativeUnsubscribe?.()
    this.nativeUnsubscribe = await nativeRuntime.listenChat((event) => this.receiveNativeEvent(event))
    if (this.disposed) {
      // `disconnect()` ran while `listenChat` was still pending: release the
      // listener we just received instead of leaking it for the process life.
      this.nativeUnsubscribe()
      this.nativeUnsubscribe = undefined
      return
    }
    this.harnessConnectionId = connectionId
    // 预设随新会话一起交出去。恢复一条已有会话时原生**不带**它：预设属于那条会话，不该被改写。
    this.sessionId = await nativeRuntime.connectHarness(
      this.sessionId,
      connectionId,
      this.nativeOptions.model,
      this.nativeOptions.preset,
    )
  }

  disconnect(): void {
    this.disposed = true
    this.stopHistoryReconciliation()
    this.nativeUnsubscribe?.()
    this.nativeUnsubscribe = undefined
    this.harnessConnectionId = undefined
    this.apiRequestId = undefined
    this.turnToken = undefined
    this.turnActive = false
  }

  /** Scope ID the scoped-event gate must compare against for this backend. */
  private eventScopeId(): string | undefined {
    return this.mode === 'harness' ? this.harnessConnectionId : this.apiRequestId
  }

  /**
   * A stopped turn must not publish anything else: not a buffered SSE delta
   * that was already in flight, not a reconcile timer, and not a reconcile
   * request that was already awaiting the native history call.
   */
  private canPublishDuringTurn(): boolean {
    return !this.disposed && this.turnActive
  }

  private messageKey(message: Pick<ChatMessage, 'role' | 'content'>): string {
    return `${message.role}\u0000${message.content}`
  }

  private rememberMessage(message: Pick<ChatMessage, 'role' | 'content'>): void {
    const key = this.messageKey(message)
    this.deliveredMessageCounts.set(key, (this.deliveredMessageCounts.get(key) ?? 0) + 1)
  }

  /**
   * A cancelled turn is the one case where the "already delivered" ledger must
   * be dropped: the durable history read after a stop is authoritative, and a
   * wrong counter would silently swallow the transcript instead of repeating
   * it. The UI appends messages, so an over-eager reset can at worst duplicate
   * one message; under-counting would lose the answer.
   */
  private resetDeliveredMessageCounts(): void {
    this.deliveredMessageCounts.clear()
  }

  private replaceKnownMessages(messages: ChatMessage[]): void {
    this.deliveredMessageCounts.clear()
    for (const message of messages) this.rememberMessage(message)
  }

  private stopHistoryReconciliation(): void {
    if (this.reconcileTimer !== undefined) clearTimeout(this.reconcileTimer)
    this.reconcileTimer = undefined
    this.reconcileAttempts = 0
  }

  private scheduleHistoryReconciliation(): void {
    if (this.disposed || this.mode !== 'harness' || this.reconcileTimer !== undefined) return
    this.reconcileAttempts = 0
    this.reconcileTimer = setTimeout(() => {
      this.reconcileTimer = undefined
      void this.reconcileHistory(this.turnToken)
    }, 700)
  }

  /**
   * A DSH turn is durable before it is necessarily delivered to every SSE
   * subscriber. Polling the bounded history endpoint for a short window gives
   * the desktop a lossless fallback when a host closes/replaces its SSE socket
   * between two turns. Normal live events are counted first, so this cannot
   * duplicate messages that already reached the composer.
   *
   * `turnToken` is the turn this reconciliation belongs to. Once the user
   * stops that turn the token is invalidated, and neither an in-flight history
   * call nor a queued retry may publish anything.
   */
  private async reconcileHistory(turnToken: string | undefined): Promise<void> {
    if (this.disposed || this.mode !== 'harness' || this.reconcileInFlight || this.reconcileAttempts >= 30) return
    this.reconcileInFlight = true
    this.reconcileAttempts += 1
    try {
      const history = await nativeRuntime.harnessHistory()
      if (!this.canPublishDuringTurn() || this.turnToken !== turnToken) return
      const available = new Map<string, number>()
      const missing: ChatMessage[] = []
      for (const message of history) {
        const key = this.messageKey(message)
        const seen = available.get(key) ?? 0
        available.set(key, seen + 1)
        if (seen >= (this.deliveredMessageCounts.get(key) ?? 0)) missing.push(message)
      }
      for (const message of missing) {
        this.rememberMessage(message)
        this.emit({ type: 'message', role: message.role, content: message.content })
      }
      if (missing.some((message) => message.role === 'assistant')) {
        this.emit({ type: 'status', activity: 'done' })
        this.stopHistoryReconciliation()
      } else if (this.reconcileAttempts < 30) {
        this.reconcileTimer = setTimeout(() => {
          this.reconcileTimer = undefined
          void this.reconcileHistory(turnToken)
        }, 1000)
      }
    } catch {
      // A cancelled turn has no reconciliation left to retry. Checking here
      // (rather than only at the top) also covers a native history call that
      // rejects after the user pressed Stop.
      if (!this.canPublishDuringTurn() || this.turnToken !== turnToken) return
      if (this.reconcileAttempts < 30) {
        this.reconcileTimer = setTimeout(() => {
          this.reconcileTimer = undefined
          void this.reconcileHistory(turnToken)
        }, 1000)
      }
    } finally {
      this.reconcileInFlight = false
    }
  }

  private receiveNativeEvent(event: ScopedChatEvent): void {
    // Legacy/native-unscoped events are deliberately ignored here. A scoped
    // adapter must never guess ownership on an application-global channel.
    if (event.backend !== this.mode || !event.conversationId) return
    // A stopped turn has already released the composer. Any event that was
    // still in flight for it must not resurrect `streaming`.
    if (this.mode === 'harness' && !this.turnActive) return
    if (this.mode === 'harness' && !this.harnessConnectionId) return
    // The bridge may send its initial model/status snapshot in the same turn
    // that fulfills connectHarness. Accept that one scoped snapshot and bind
    // its session ID; every later event must match it exactly.
    if (this.mode === 'harness' && !this.sessionId) this.sessionId = event.conversationId
    if (!acceptsScopedChatEvent(this.mode, this.sessionId, this.eventScopeId(), event)) return
    const { backend: _backend, conversationId: _conversationId, requestId: _requestId, ...chatEvent } = event
    if (chatEvent.type === 'message') {
      this.rememberMessage(chatEvent)
      if (chatEvent.role === 'assistant') this.stopHistoryReconciliation()
    }
    if (chatEvent.type === 'error') this.stopHistoryReconciliation()
    this.emit(chatEvent)
    // 宿主**拒绝了这一轮**（桥把 `turn/end { reason.kind: "blocked" }` 翻成 `turn-blocked`）：
    // 这条会话不再收我们的消息了 —— 实测就是被归档的会话，几毫秒内结束、没有回答。用户看到的
    // 是"发出去了、永远没回应、灯还是绿的"，所以除了那句错误，还要**自己换一条新会话并把这句
    // 重发一次**：他不必再手动重来一次。上面那句 `emit` 必须在前，先让他知道发生了什么。
    if (this.mode === 'harness' && chatEvent.type === 'error' && chatEvent.code === 'turn-blocked') {
      void this.recoverBlockedTurn()
      return
    }
    // `assistant/message` is the durable terminal record. Some hosts omit the
    // later `turn/end` event from a replaced SSE connection; make the desktop
    // idle as soon as the final message itself arrives.
    if (this.mode === 'harness' && chatEvent.type === 'message' && chatEvent.role === 'assistant') {
      this.emit({ type: 'status', activity: 'done' })
    }
  }

  /**
   * 一轮被宿主拒绝之后的收尾：离开这条会话、开一条新的、把这句重发一次。
   *
   * 只做一次（`blockedTurnRecovered`）：新会话要是又被拒，就报错，不在"换—重发"之间打转。
   * 走的是和归档恢复**同一套**机制，因为对用户来说这是同一件事：这条会话不能用了。
   */
  private async recoverBlockedTurn(): Promise<void> {
    if (this.blockedTurnRecovered || this.disposed) return
    this.blockedTurnRecovered = true
    const text = this.lastTurnText
    this.turnActive = false
    this.stopHistoryReconciliation()
    const failure = await this.recoverArchivedSession()
    if (failure || !text) {
      if (failure) {
        this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: `换一条新会话也没有成功：${failure}` })
      }
      return
    }
    this.emit({ type: 'conversation-reset', reason: 'turn-blocked', message: BLOCKED_TURN_NOTICE })
    await this.runTurn(text, this.nativeOptions.model ? { model: this.nativeOptions.model } : undefined, false)
      .catch(() => undefined)
  }

  async send(text: string, options?: SendOptions): Promise<void> {
    return this.runTurn(text, options, true)
  }

  /**
   * 用户的一轮话。`allowArchivedRecovery` 只对**重发**那一轮为 false：刚换到的新会话如果
   * 立刻又不可用，必须报错，而不是在"重连—重发"之间打转。
   */
  private async runTurn(text: string, options: SendOptions | undefined, allowArchivedRecovery: boolean): Promise<void> {
    // 一次新的用户发送重置两处本轮状态：原话（重发用）与"已经为被拒绝换过一次会话"。
    this.lastTurnText = text
    if (allowArchivedRecovery) this.blockedTurnRecovered = false
    if (this.mode === 'deepseek-api' && !this.sessionId) this.sessionId = crypto.randomUUID()
    if (this.mode === 'deepseek-api') this.apiRequestId = crypto.randomUUID()
    // Every send owns a new turn identity. The previous turn's token is gone
    // before any native call starts, so a late event from a stopped turn can
    // never be accepted into this one.
    const turnToken = crypto.randomUUID()
    this.turnToken = turnToken
    this.turnActive = true
    const requestedConversationId = options?.conversationId ?? this.sessionId
    if (requestedConversationId) this.sessionId = requestedConversationId
    if (this.mode !== 'harness') this.emit({ type: 'message', role: 'user', content: text })
    this.emit({ type: 'status', activity: 'sending' })
    try {
      if (this.mode === 'harness') this.scheduleHistoryReconciliation()
      const returnedConversationId = await nativeRuntime.sendChat(this.mode, text, {
        ...this.nativeOptions,
        conversationId: requestedConversationId,
        // The API path needs its per-request ID. The Harness path is scoped by
        // its connection ID on the native side, so the turn token must not be
        // sent as a request ID there.
        requestId: this.mode === 'deepseek-api' ? this.apiRequestId : undefined,
        model: options?.model ?? this.nativeOptions.model,
      })
      if (returnedConversationId) this.sessionId = returnedConversationId
    } catch (error) {
      if (this.turnToken === turnToken) this.turnActive = false
      this.stopHistoryReconciliation()
      if (allowArchivedRecovery && this.mode === 'harness' && !this.disposed && isMissingSessionError(error)) {
        // 没有会话 ⇒ 建立它，再把这句原话发一次。
        try {
          await this.openEventScope()
        } catch (connectError) {
          const message = `${String(error)} 重新连接这条端点也没有成功：${String(connectError)}`
          this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message })
          throw new Error(message)
        }
        await this.runTurn(text, options?.model ? { model: options.model } : undefined, false)
        return
      }
      if (allowArchivedRecovery && this.mode === 'harness' && !this.disposed && isArchivedSessionError(error)) {
        const failure = await this.recoverArchivedSession()
        if (!failure) {
          // 先告诉界面：它正在显示的那段转写属于一条**已被归档**的会话，而下面这些事件
          // 会落到另一条会话上。顺序不能反——重发一旦开始，转写就该已经在换了。
          this.emit({ type: 'conversation-reset', reason: 'session-archived', message: ARCHIVED_SESSION_NOTICE })
          // 重发**不带**上一轮的 `conversationId`：那个 id 正是刚被桥拒绝的那条会话，
          // 带着它就等于又往归档会话里发一次。
          await this.runTurn(text, options?.model ? { model: options.model } : undefined, false)
          return
        }
        const message = `${String(error)} 换一条新会话也没有成功：${failure}`
        this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message })
        throw new Error(message)
      }
      this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: String(error) })
      throw error
    }
  }

  /**
   * 离开那条被归档的会话，改绑一条新的。
   *
   * **必须在这里做**：SSE 事件流的生命周期归渲染端（见 `stop()` 的说明），原生只清掉它自己
   * 缓存的会话 id 就回去了——"有会话、没有事件流"正是用户遇到的那种"看着连上了、却永远安静"
   * 的状态。事件流建错的代价是静默，所以重连这件事由持有事件流的这一侧负责。
   *
   * 先清掉 `sessionId` 是关键：桥只在**没有** resume id 时才去建"今天这条桌面会话"，带着
   * 刚被它拒绝的 id 去 resume，只会再被拒一次。
   *
   * 返回 `undefined` 表示换成功；否则是给用户看的原因（这时调用方要报错，不能假装换成了）。
   */
  private async recoverArchivedSession(): Promise<string | undefined> {
    this.sessionId = undefined
    this.resetDeliveredMessageCounts()
    try {
      await this.openEventScope()
    } catch (error) {
      return String(error)
    }
    if (this.disposed) return '连接已经关闭'
    if (!this.sessionId) return '桥没有给出新的会话'
    return undefined
  }

  /**
   * Stop the current turn for either native backend. The renderer releases the
   * send key *before* awaiting the native cancellation, and the stopped turn
   * can no longer publish anything.
   *
   * DeepSeek API: clearing the request ID is enough, because every API event
   * carries that per-request ID.
   *
   * Harness: the native event scope is the SSE connection ID, which cannot be
   * cleared for the whole adapter without breaking the next turn. It is
   * renewed instead, against the *same* DSH session ID.
   */
  async stop(): Promise<void> {
    const hadActiveTurn = this.turnActive
    this.turnActive = false
    this.turnToken = undefined
    // Stop the history reconciler before anything else: it is the only other
    // publisher that could re-add an assistant message to a stopped turn.
    this.stopHistoryReconciliation()
    if (this.mode === 'deepseek-api') this.apiRequestId = undefined
    // Release the composer first, then let the native cancellation settle.
    if (hadActiveTurn) this.emit({ type: 'status', activity: 'idle' })
    if (!hadActiveTurn) {
      // Nothing this adapter owns is running; a duplicate Stop must stay a
      // no-op instead of tearing down a healthy event scope.
      return
    }
    try {
      await nativeRuntime.cancelChat(this.mode)
    } finally {
      if (this.mode === 'harness' && !this.disposed) {
        // Reconcile what the cancelled turn already committed, then give the
        // next turn a clean event scope. `sessionId` is preserved so the DSH
        // session itself is never replaced.
        this.resetDeliveredMessageCounts()
        try {
          await this.openEventScope()
        } catch {
          /* A failed reconnect is recoverable: the next connect() retries. */
        }
      }
    }
  }

  async history(): Promise<ChatMessage[]> {
    return this.boundedHistory()
  }

  /**
   * The API transcript is read as a bounded window (`limit` counts backwards
   * from the newest turn) instead of cloning an arbitrarily large archive
   * across IPC. The caller asks for a larger window when the user wants to
   * read earlier turns; the default matches the native command's own default.
   */
  async boundedHistory(limit?: number): Promise<ChatMessage[]> {
    const messages = this.mode === 'harness'
      ? await nativeRuntime.harnessHistory()
      : this.mode === 'deepseek-api' && this.sessionId
        ? (await nativeRuntime.apiHistory(this.sessionId, limit)).messages
        : []
    this.replaceKnownMessages(messages)
    return messages
  }

  /** True when older durable API turns exist beyond the returned window. */
  async hasMoreApiHistory(): Promise<boolean> {
    if (this.mode !== 'deepseek-api' || !this.sessionId) return false
    const page = await nativeRuntime.apiHistory(this.sessionId, 1)
    return page.totalMessages > 1
  }

  conversationId(): string | undefined { return this.sessionId }
}
