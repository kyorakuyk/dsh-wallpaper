import type { BackendMode, ChatMessage } from '../domain/types.ts'
import { nativeRuntime, type NativeSendOptions } from '../native/runtime.ts'
import { EventChatAdapter, type SendOptions } from './adapter.ts'

export class NativeChatAdapter extends EventChatAdapter {
  readonly mode: BackendMode
  private nativeUnsubscribe: (() => void) | undefined
  private sessionId: string | undefined
  private readonly nativeOptions: NativeSendOptions

  constructor(mode: BackendMode, nativeOptions: NativeSendOptions = {}, resumeSessionId?: string) {
    super()
    this.mode = mode
    this.nativeOptions = nativeOptions
    this.sessionId = resumeSessionId
  }

  async connect(): Promise<void> {
    this.nativeUnsubscribe = await nativeRuntime.listenChat((event) => this.emit(event))
    if (this.mode === 'harness') this.sessionId = await nativeRuntime.connectHarness(this.sessionId)
  }

  disconnect(): void {
    this.nativeUnsubscribe?.()
    this.nativeUnsubscribe = undefined
  }

  async send(text: string, options?: SendOptions): Promise<void> {
    if (this.mode !== 'harness') this.emit({ type: 'message', role: 'user', content: text })
    this.emit({ type: 'status', activity: 'sending' })
    try {
      await nativeRuntime.sendChat(this.mode, text, {
        ...this.nativeOptions,
        conversationId: options?.conversationId ?? this.sessionId,
        model: options?.model ?? this.nativeOptions.model,
      })
    } catch (error) {
      this.emit({ type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: String(error) })
      throw error
    }
  }

  async stop(): Promise<void> { await nativeRuntime.cancelChat(this.mode) }

  async history(): Promise<ChatMessage[]> {
    return this.mode === 'harness' ? nativeRuntime.harnessHistory() : []
  }

  conversationId(): string | undefined { return this.sessionId }
}
