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

  constructor(mode: BackendMode, nativeOptions: NativeSendOptions = {}, resumeSessionId?: string) {
    super()
    this.mode = mode
    this.nativeOptions = nativeOptions
    this.sessionId = resumeSessionId
  }

  async connect(): Promise<void> {
    const harnessConnectionId = this.mode === 'harness' ? crypto.randomUUID() : undefined
    if (harnessConnectionId) this.requestId = harnessConnectionId
    this.nativeUnsubscribe = await nativeRuntime.listenChat((event) => this.receiveNativeEvent(event))
    if (this.mode === 'harness') this.sessionId = await nativeRuntime.connectHarness(this.sessionId, harnessConnectionId!)
  }

  disconnect(): void {
    this.nativeUnsubscribe?.()
    this.nativeUnsubscribe = undefined
    this.requestId = undefined
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
    this.emit(chatEvent)
  }

  async send(text: string, options?: SendOptions): Promise<void> {
    if (this.mode === 'deepseek-api' && !this.sessionId) this.sessionId = crypto.randomUUID()
    if (this.mode === 'deepseek-api') this.requestId = crypto.randomUUID()
    const requestedConversationId = options?.conversationId ?? this.sessionId
    if (requestedConversationId) this.sessionId = requestedConversationId
    if (this.mode !== 'harness') this.emit({ type: 'message', role: 'user', content: text })
    this.emit({ type: 'status', activity: 'sending' })
    try {
      const returnedConversationId = await nativeRuntime.sendChat(this.mode, text, {
        ...this.nativeOptions,
        conversationId: requestedConversationId,
        requestId: this.requestId,
        model: options?.model ?? this.nativeOptions.model,
      })
      if (returnedConversationId) this.sessionId = returnedConversationId
    } catch (error) {
      this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: String(error) })
      throw error
    }
  }

  async stop(): Promise<void> { await nativeRuntime.cancelChat(this.mode) }

  async history(): Promise<ChatMessage[]> {
    if (this.mode === 'harness') return nativeRuntime.harnessHistory()
    if (this.mode === 'deepseek-api' && this.sessionId) return nativeRuntime.apiHistory(this.sessionId)
    return []
  }

  conversationId(): string | undefined { return this.sessionId }
}
