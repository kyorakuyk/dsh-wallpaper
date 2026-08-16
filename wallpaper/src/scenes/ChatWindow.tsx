/** 会话窗：默认 DeepSeek 网页版；3080 在线时可切 DSH 会话框（新窗口加载） */

import type { PersonaManifest } from '../persona/types.ts'

export interface ChatWindowProps {
  persona: PersonaManifest
  /** true = 会话窗指向 DSH(3080)；false = chat.deepseek.com */
  harnessMode: boolean
  onClose: () => void
}

/** 打开外部聊天目标（独立浏览器/新标签；网页壁纸模式下无法内嵌 iframe 登录态） */
export function openChatTarget(harnessMode: boolean): void {
  const url = harnessMode ? 'http://127.0.0.1:3080' : 'https://chat.deepseek.com'
  window.open(url, '_blank')
}

export function ChatWindow({ persona, harnessMode, onClose }: ChatWindowProps) {
  return (
    <div
      className="scene scene-chat"
      style={{ ['--persona-primary' as string]: persona.theme.primary }}
    >
      <div className="chat-bar">
        <span className="chat-title">
          {harnessMode ? '🖥️ DeepSeek Harness 会话' : '🌐 DeepSeek 网页版'}
        </span>
        <button className="chat-close" onClick={onClose}>
          ✕
        </button>
      </div>
      <div className="chat-body">
        <p>会话窗将在新窗口打开：</p>
        <code>{harnessMode ? 'http://127.0.0.1:3080' : 'https://chat.deepseek.com'}</code>
        <div className="chat-actions">
          <button onClick={() => openChatTarget(harnessMode)}>打开会话</button>
          <button onClick={onClose}>关闭</button>
        </div>
      </div>
    </div>
  )
}
