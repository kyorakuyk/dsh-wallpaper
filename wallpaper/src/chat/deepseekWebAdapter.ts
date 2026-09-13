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

  /**
   * The native invoke resolves after the DOM polling turn has ended, but the
   * Tauri event can be missed while a WebView is being repainted or replaced.
   * Keep the release operation request-scoped so a late event cannot release
   * a newer turn.
   */
  private settleRequest(requestId: string, activity: 'idle' | 'done'): void {
    if (this.requestId !== requestId) return
    this.requestId = undefined
    this.emit({ type: 'status', activity })
  }

  async connect(): Promise<void> {
    this.nativeUnsubscribe = await nativeRuntime.listenChat((event) => {
      if (event.backend !== this.mode) return
      if (this.sessionId && event.conversationId && event.conversationId !== this.sessionId && !(this.requestId && event.requestId === this.requestId)) return
      // Every native event emitted for a send carries its request ID. Keep
      // matching strict even after completion has cleared `this.requestId`;
      // otherwise a delayed `sending` event from the previous turn could
      // resurrect the busy composer after `done`.
      const activeRequestId = this.requestId
      // Every native web event belongs to one send operation. Requiring the
      // ID in both directions rejects unscoped stale events after completion
      // and prevents an event from one turn from being accepted during the
      // short window before a later turn receives its first status event.
      if (!activeRequestId || event.requestId !== activeRequestId) return
      if (event.conversationId) this.sessionId = event.conversationId
      const { backend: _backend, conversationId: _conversationId, requestId: _requestId, ...chatEvent } = event
      this.emit(chatEvent)
      // The assistant message is the durable terminal record. Do not make
      // the composer depend on a second status event: a native event can be
      // delivered after a WebView replacement or after the renderer has
      // already painted the final reply. Clearing the request here also
      // makes a late `sending` event from the same turn fail the request-id
      // guard above.
      if (chatEvent.type === 'message' && chatEvent.role === 'assistant') {
        this.settleRequest(activeRequestId, 'done')
        return
      }
      if (chatEvent.type === 'status' && (chatEvent.activity === 'done' || chatEvent.activity === 'idle')) {
        this.settleRequest(activeRequestId, chatEvent.activity)
        return
      }
      // These events are terminal too. Clearing the request here prevents the
      // normal Promise-return fallback below from turning a lost idle event
      // into a false `done` state after login/error handling.
      if ((chatEvent.type === 'auth-required' || chatEvent.type === 'error') && this.requestId === activeRequestId) this.requestId = undefined
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
    if (this.requestId) throw new Error('DeepSeek 网页上一条消息仍在处理中，请等待完成或点击停止。')
    this.requestId = crypto.randomUUID()
    const requestId = this.requestId
    this.emit({ type: 'message', role: 'user', content: text })
    try {
      const returnedConversationId = await nativeRuntime.sendChat(this.mode, text, {
        conversationId: options?.conversationId ?? this.sessionId,
        requestId,
      })
      if (returnedConversationId) this.sessionId = returnedConversationId
      // `send_chat` resolves only after Rust has finished the DOM polling turn.
      // If the corresponding `done`/assistant event was lost at the WebView
      // boundary, release the renderer here instead of leaving the Stop button
      // mounted until the next request or the 180-second timeout.
      if (this.requestId === requestId) this.settleRequest(requestId, 'done')
    } catch (error) {
      // If native failed before it could emit its terminal error/idle pair,
      // do not leave the old request ID able to accept a delayed WebView
      // event from a failed turn.
      if (this.requestId === requestId) this.requestId = undefined
      throw error
    }
  }

  async stop(): Promise<void> {
    const requestId = this.requestId
    if (!requestId) return
    // Release the renderer before waiting for WebView2 to acknowledge the
    // stop click. Native cancellation remains request-scoped and can finish
    // later without affecting a subsequent send.
    this.requestId = undefined
    this.emit({ type: 'status', activity: 'idle' })
    try {
      await nativeRuntime.cancelChat(this.mode)
    } catch (error) {
      // The UI is already released, but surface a native cancellation failure
      // to the caller so it can show the actionable error.
      throw error
    }
  }

  async history(): Promise<ChatMessage[]> {
    const result = await nativeRuntime.deepseekWebHistory()
    this.sessionId = result.conversationId ?? this.sessionId
    if (result.model) this.emit({ type: 'model', provider: 'deepseek-web', model: result.model, tier: 'unknown' })
    return result.messages
  }

  conversationId(): string | undefined { return this.sessionId }
}
