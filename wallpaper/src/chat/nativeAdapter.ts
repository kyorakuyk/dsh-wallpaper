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
    this.sessionId = await nativeRuntime.connectHarness(this.sessionId, connectionId, this.nativeOptions.model)
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
    // `assistant/message` is the durable terminal record. Some hosts omit the
    // later `turn/end` event from a replaced SSE connection; make the desktop
    // idle as soon as the final message itself arrives.
    if (this.mode === 'harness' && chatEvent.type === 'message' && chatEvent.role === 'assistant') {
      this.emit({ type: 'status', activity: 'done' })
    }
  }

  async send(text: string, options?: SendOptions): Promise<void> {
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
      this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: String(error) })
      throw error
    }
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
    const messages = this.mode === 'harness'
      ? await nativeRuntime.harnessHistory()
      : this.mode === 'deepseek-api' && this.sessionId
        ? await nativeRuntime.apiHistory(this.sessionId)
        : []
    this.replaceKnownMessages(messages)
    return messages
  }

  conversationId(): string | undefined { return this.sessionId }
}
