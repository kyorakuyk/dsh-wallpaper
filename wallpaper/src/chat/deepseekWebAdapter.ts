import type { ChatMessage } from '../domain/types.ts'
import { nativeRuntime } from '../native/runtime.ts'
import { EventChatAdapter, type SendOptions } from './adapter.ts'

/**
 * Experimental placeholder only: it opens the official DeepSeek page in the
 * user's default browser. There is currently no in-app WebView2 profile, DOM
 * bridge, or Cookie access. It deliberately fails closed until a tested DOM
 * signature is installed; it never falls back to API or calls an unofficial
 * HTTP endpoint.
 */
export class DeepSeekWebAdapter extends EventChatAdapter {
  readonly mode = 'deepseek-web' as const
  private connected = false

  async connect(): Promise<void> {
    this.connected = true
    this.emit({ type: 'model', provider: 'deepseek-web', model: 'deepseek-chat', tier: 'flash' })
  }

  disconnect(): void { this.connected = false }

  async send(_text: string, _options?: SendOptions): Promise<void> {
    if (!this.connected) throw new Error('DeepSeek 网页实验入口尚未连接')
    await nativeRuntime.requestDeepSeekLogin()
    this.emit({ type: 'auth-required' })
    this.emit({ type: 'error', code: 'DEEPSEEK_WEB_UNSUPPORTED', recoverable: true, message: 'DeepSeek 网页模式目前只能在默认浏览器打开官方页面，尚不能在壁纸内发送或同步消息；本应用不读取 Cookie，也不会自动切换付费 API。' })
  }

  async stop(): Promise<void> { this.emit({ type: 'status', activity: 'idle' }) }
  async history(): Promise<ChatMessage[]> { return [] }
}
