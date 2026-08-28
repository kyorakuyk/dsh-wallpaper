import type { ChatMessage } from '../domain/types.ts'
import { nativeRuntime } from '../native/runtime.ts'
import { EventChatAdapter, type SendOptions } from './adapter.ts'

/**
 * DeepSeek's free web route is hosted in a dedicated, persistent WebView2
 * window owned by the Tauri shell. The adapter only receives the closed event
 * protocol from Rust; it never sees cookies, page HTML, or login credentials.
 */
export class DeepSeekWebAdapter extends EventChatAdapter {
  readonly mode = 'deepseek-web' as const
  private connected = false
  private sessionId: string | undefined
  private requestId: string | undefined
  private nativeUnsubscribe: (() => void) | undefined

  async connect(): Promise<void> {
    this.nativeUnsubscribe = await nativeRuntime.listenChat((event) => {
      if (event.backend !== this.mode) return
      if (this.sessionId && event.conversationId && event.conversationId !== this.sessionId && !(this.requestId && event.requestId === this.requestId)) return
      // Every native event emitted for a send carries its request ID. Keep
      // matching strict even after completion has cleared `this.requestId`;
      // otherwise a delayed `sending` event from the previous turn could
      // resurrect the busy composer after `done`.
      if (event.requestId && event.requestId !== this.requestId) return
      if (event.conversationId) this.sessionId = event.conversationId
      const { backend: _backend, conversationId: _conversationId, requestId: _requestId, ...chatEvent } = event
      this.emit(chatEvent)
      if (chatEvent.type === 'status' && (chatEvent.activity === 'done' || chatEvent.activity === 'idle')) this.requestId = undefined
    })
    const status = await nativeRuntime.deepseekWebStatus()
    this.sessionId = status.conversationId
    this.connected = true
    this.emit({ type: 'model', provider: 'deepseek-web', model: status.model ?? 'deepseek-chat', tier: 'unknown' })
    if (status.state === 'unsupported') {
      this.emit({ type: 'error', code: 'DEEPSEEK_WEB_UNSUPPORTED', recoverable: true, message: 'DeepSeek 网页结构无法识别，网页桥接需要更新。' })
    }
  }

  disconnect(): void {
    this.connected = false
    const hadActiveRequest = this.requestId !== undefined
    this.requestId = undefined
    this.nativeUnsubscribe?.()
    this.nativeUnsubscribe = undefined
    // A backend switch tears down the adapter identity. Stop a native DOM
    // polling turn as well, otherwise the hidden WebView could retain the
    // active slot for up to its timeout and block the next web turn.
    if (hadActiveRequest) void nativeRuntime.cancelChat(this.mode)
  }

  async send(text: string, options?: SendOptions): Promise<void> {
    if (!this.connected) throw new Error('DeepSeek 网页实验入口尚未连接')
    const trimmed = text.trim()
    if (!trimmed) throw new Error('消息不能为空。')
    this.requestId = crypto.randomUUID()
    this.emit({ type: 'message', role: 'user', content: text })
    const returnedConversationId = await nativeRuntime.sendChat(this.mode, text, {
      conversationId: options?.conversationId ?? this.sessionId,
      requestId: this.requestId,
    })
    if (returnedConversationId) this.sessionId = returnedConversationId
  }

  async stop(): Promise<void> { await nativeRuntime.cancelChat(this.mode) }

  async history(): Promise<ChatMessage[]> {
    const result = await nativeRuntime.deepseekWebHistory()
    this.sessionId = result.conversationId ?? this.sessionId
    if (result.model) this.emit({ type: 'model', provider: 'deepseek-web', model: result.model, tier: 'unknown' })
    return result.messages
  }

  conversationId(): string | undefined { return this.sessionId }
}
