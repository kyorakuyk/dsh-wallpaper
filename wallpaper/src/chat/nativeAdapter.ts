import type { BackendMode, ChatMessage, ScopedChatEvent } from '../domain/types.ts'
import { nativeRuntime, type NativeSendOptions } from '../native/runtime.ts'
import { EventChatAdapter, type SendOptions } from './adapter.ts'

export function acceptsScopedChatEvent(
  mode: BackendMode,
  sessionId: string | undefined,
  requestId: string | undefined,
  event: ScopedChatEvent,
): boolean {
  if (event.backend !== mode || !event.conversationId || event.conversationId !== sessionId) return false
  return mode !== 'deepseek-api' || event.requestId === requestId
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
  private requestId: string | undefined
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
    const harnessConnectionId = this.mode === 'harness' ? crypto.randomUUID() : undefined
    if (harnessConnectionId) this.requestId = harnessConnectionId
    this.nativeUnsubscribe = await nativeRuntime.listenChat((event) => this.receiveNativeEvent(event))
    if (this.mode === 'harness') this.sessionId = await nativeRuntime.connectHarness(this.sessionId, harnessConnectionId!, this.nativeOptions.model)
  }

  disconnect(): void {
    this.disposed = true
    this.stopHistoryReconciliation()
    this.nativeUnsubscribe?.()
    this.nativeUnsubscribe = undefined
    this.requestId = undefined
  }

  private messageKey(message: Pick<ChatMessage, 'role' | 'content'>): string {
    return `${message.role}\u0000${message.content}`
  }

  private rememberMessage(message: Pick<ChatMessage, 'role' | 'content'>): void {
    const key = this.messageKey(message)
    this.deliveredMessageCounts.set(key, (this.deliveredMessageCounts.get(key) ?? 0) + 1)
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
      void this.reconcileHistory()
    }, 700)
  }

  /**
   * A DSH turn is durable before it is necessarily delivered to every SSE
   * subscriber. Polling the bounded history endpoint for a short window gives
   * the desktop a lossless fallback when a host closes/replaces its SSE socket
   * between two turns. Normal live events are counted first, so this cannot
   * duplicate messages that already reached the composer.
   */
  private async reconcileHistory(): Promise<void> {
    if (this.disposed || this.mode !== 'harness' || this.reconcileInFlight || this.reconcileAttempts >= 30) return
    this.reconcileInFlight = true
    this.reconcileAttempts += 1
    try {
      const history = await nativeRuntime.harnessHistory()
      if (this.disposed) return
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
          void this.reconcileHistory()
        }, 1000)
      }
    } catch {
      if (this.reconcileAttempts < 30) {
        this.reconcileTimer = setTimeout(() => {
          this.reconcileTimer = undefined
          void this.reconcileHistory()
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
    if (this.mode === 'harness' && event.requestId !== this.requestId) return
    // The bridge may send its initial model/status snapshot in the same turn
    // that fulfills connectHarness. Accept that one scoped snapshot and bind
    // its session ID; every later event must match it exactly.
    if (this.mode === 'harness' && !this.sessionId) this.sessionId = event.conversationId
    if (!acceptsScopedChatEvent(this.mode, this.sessionId, this.requestId, event)) return
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
    if (this.mode === 'deepseek-api') this.requestId = crypto.randomUUID()
    const requestedConversationId = options?.conversationId ?? this.sessionId
    if (requestedConversationId) this.sessionId = requestedConversationId
    if (this.mode !== 'harness') this.emit({ type: 'message', role: 'user', content: text })
    this.emit({ type: 'status', activity: 'sending' })
    try {
      if (this.mode === 'harness') this.scheduleHistoryReconciliation()
      const returnedConversationId = await nativeRuntime.sendChat(this.mode, text, {
        ...this.nativeOptions,
        conversationId: requestedConversationId,
        requestId: this.requestId,
        model: options?.model ?? this.nativeOptions.model,
      })
      if (returnedConversationId) this.sessionId = returnedConversationId
    } catch (error) {
      this.stopHistoryReconciliation()
      this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: String(error) })
      throw error
    }
  }

  async stop(): Promise<void> { await nativeRuntime.cancelChat(this.mode) }

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
