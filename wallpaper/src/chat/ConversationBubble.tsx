import { useState } from 'react'
import type { Activity, BackendMode, ChatMessage, TokenUsage } from '../domain/types.ts'

export interface ConversationBubbleProps {
  backend: BackendMode
  activity: Activity
  modelLabel: string
  messages: ChatMessage[]
  streamingText: string
  historyExpanded: boolean
  usage?: TokenUsage
  disabled?: boolean
  onToggleHistory: () => void
  onSend: (text: string) => void
  onStop: () => void
  onClose: () => void
}

const backendName: Record<BackendMode, string> = {
  'deepseek-web': 'DeepSeek 免费网页桥接（实验）',
  'deepseek-api': 'DeepSeek API · 计费',
  harness: 'DeepSeek Harness',
}

export function ConversationBubble(props: ConversationBubbleProps) {
  const [draft, setDraft] = useState('')
  const busy = ['sending', 'thinking', 'streaming', 'tool'].includes(props.activity)
  const submit = () => {
    const text = draft.trim()
    if (!text || busy || props.disabled) return
    props.onSend(text)
    setDraft('')
  }
  const totalCost = props.messages.reduce((sum, message) => sum + (message.usage?.cost ?? 0), 0)
  return (
    <section className={`conversation-shell ${props.historyExpanded ? 'expanded' : ''}`} aria-label="AI 对话气泡">
      <button className="history-handle" onClick={props.onToggleHistory} aria-label="展开或收起会话记录">
        {props.historyExpanded ? '⌄ 收起记录' : '⌃ 会话记录'}
      </button>
      {props.historyExpanded && (
        <div className="conversation-history">
          {props.messages.length === 0 && <p className="empty-history">还没有消息，叫醒她聊两句吧。</p>}
          {props.messages.map((message) => (
            <article key={message.id} className={`chat-message ${message.role}`}>
              <strong>{message.role === 'user' ? '你' : '大肥鱼'}</strong>
              <p>{message.content}</p>
              {message.usage && <small>输入 {message.usage.input} · 输出 {message.usage.output}{message.usage.cost !== undefined ? ` · ¥${message.usage.cost.toFixed(4)}` : ''}</small>}
            </article>
          ))}
          {props.streamingText && <article className="chat-message assistant streaming"><strong>大肥鱼</strong><p>{props.streamingText}<span className="typing-caret" /></p></article>}
        </div>
      )}
      <div className="conversation-card">
        <header>
          <span>{backendName[props.backend]}</span>
          <span className={`activity-dot ${props.activity}`}>{props.activity === 'idle' ? '待命' : props.activity}</span>
          <button onClick={props.onClose} aria-label="关闭对话">×</button>
        </header>
        <textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); submit() }
          }}
          placeholder={props.disabled ? '当前模式暂不可用' : '今天要一起处理什么？'}
          disabled={props.disabled}
          rows={2}
        />
        <footer>
          <span>{props.modelLabel}</span>
          {props.backend === 'deepseek-api' && <span className="billing-badge">API 计费中</span>}
          {props.usage && <span>本轮 {props.usage.input + props.usage.output} tokens</span>}
          {totalCost > 0 && <span>会话 ¥{totalCost.toFixed(4)}</span>}
          {busy ? <button onClick={props.onStop}>停止</button> : <button onClick={submit} disabled={!draft.trim() || props.disabled}>发送</button>}
        </footer>
      </div>
    </section>
  )
}

