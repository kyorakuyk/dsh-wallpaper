import type { BackendMode, ChatMessage } from '../domain/types.ts'
import { EventChatAdapter, type SendOptions } from './adapter.ts'

export class PreviewAdapter extends EventChatAdapter {
  readonly mode: BackendMode
  private messages: ChatMessage[] = []
  private timer: ReturnType<typeof setInterval> | undefined
  constructor(mode: BackendMode) { super(); this.mode = mode }
  async connect(): Promise<void> {
    this.emit({ type: 'model', provider: this.mode === 'harness' ? 'dsh' : 'deepseek', model: this.mode === 'harness' ? 'deepseek-pro' : 'deepseek-flash', tier: this.mode === 'harness' ? 'pro' : 'flash' })
  }
  disconnect(): void { void this.stop() }
  async send(text: string, _options?: SendOptions): Promise<void> {
    this.messages.push({ id: crypto.randomUUID(), role: 'user', content: text, createdAt: Date.now() })
    this.emit({ type: 'message', role: 'user', content: text })
    this.emit({ type: 'status', activity: 'thinking' })
    const answer = this.mode === 'harness' ? 'Harness 已接通。正式桌面应用会把这条消息交给标准 DSH 会话。' : '这是浏览器预览回复。正式应用会连接 DeepSeek 网页桥接或用户启用的 API。'
    let cursor = 0
    await new Promise<void>((resolve) => {
      this.timer = setInterval(() => {
        const delta = answer.slice(cursor, cursor + 2)
        cursor += 2
        if (delta) this.emit({ type: 'delta', text: delta })
        if (cursor >= answer.length) {
          if (this.timer) clearInterval(this.timer)
          this.timer = undefined
          this.messages.push({ id: crypto.randomUUID(), role: 'assistant', content: answer, createdAt: Date.now() })
          this.emit({ type: 'message', role: 'assistant', content: answer })
          this.emit({ type: 'status', activity: 'done' })
          resolve()
        }
      }, 35)
    })
  }
  async stop(): Promise<void> {
    if (this.timer) clearInterval(this.timer)
    this.timer = undefined
    this.emit({ type: 'status', activity: 'idle' })
  }
  async history(): Promise<ChatMessage[]> { return [...this.messages] }
}

