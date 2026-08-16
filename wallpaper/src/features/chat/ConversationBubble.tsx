import { useEffect, useRef, useState } from 'react'
import type { Activity, BackendMode, ChatMessage, TokenUsage } from '../../domain/types.ts'
import { Button, Glass, Icon } from '../../ui/primitives/index.ts'
import { ACTIVITY_LABEL, BACKEND_PRESENTATION, composerPlaceholder, formatCost, isBusyActivity, sessionCost, usageTokenCount } from './conversationViewModel.ts'
import './ConversationBubble.css'

export interface ConversationBubbleProps {
  backend: BackendMode
  activity: Activity
  modelLabel: string
  messages: ChatMessage[]
  streamingText: string
  historyExpanded: boolean
  usage?: TokenUsage
  disabled?: boolean
  collapsed?: boolean
  layout?: 'floating' | 'taskbar-docked'
  expandDirection?: 'up' | 'down' | 'left' | 'right' | 'center'
  onExpand?: () => void
  onToggleHistory: () => void
  onSend: (text: string) => void
  onStop: () => void
  onClose: () => void
}

function UsageLine({ usage }: { usage?: TokenUsage }) {
  if (!usage) return null
  return <small className="dsh-chat__message-usage">
    输入 {usage.input} · 输出 {usage.output}
    {usage.cacheRead !== undefined ? ` · 缓存 ${usage.cacheRead}` : ''}
    {usage.cost !== undefined ? ` · ${formatCost(usage.cost, usage.estimated)}` : ''}
  </small>
}

export function ConversationBubble(props: ConversationBubbleProps) {
  const [draft, setDraft] = useState('')
  const historyRef = useRef<HTMLDivElement>(null)
  const busy = isBusyActivity(props.activity)
  const backend = BACKEND_PRESENTATION[props.backend]
  const totalCost = sessionCost(props.messages)
  const tokenCount = usageTokenCount(props.usage)

  if (props.collapsed) return <button
    type="button"
    className={`dsh-chat-collapsed dsh-chat-collapsed--${props.layout ?? 'floating'} dsh-chat-collapsed--${props.expandDirection ?? 'center'} dsh-theme-${props.backend === 'harness' ? 'harness' : 'deepseek'}`}
    data-interaction-region="chat-collapsed"
    onClick={props.onExpand}
    aria-label="展开 AI 对话"
  >
    <span className="dsh-chat-collapsed__sigil"><Icon name="spark" size={15} /></span>
    {(props.layout ?? 'floating') === 'floating' && <span className="dsh-chat-collapsed__label">{backend.shortName}</span>}
    <Icon name="chevron-up" size={16} />
  </button>

  const submit = () => {
    const text = draft.trim()
    if (!text || busy || props.disabled) return
    props.onSend(text)
    setDraft('')
  }

  useEffect(() => {
    if (!props.historyExpanded) return
    historyRef.current?.scrollTo({ top: historyRef.current.scrollHeight, behavior: 'smooth' })
  }, [props.historyExpanded, props.messages.length, props.streamingText])

  return <section
    className={`conversation-shell dsh-chat dsh-theme-${props.backend === 'harness' ? 'harness' : 'deepseek'} ${props.historyExpanded ? 'expanded' : ''} ${busy ? 'dsh-chat--busy' : ''}`}
    data-dsh-theme={props.backend === 'harness' ? 'harness' : 'deepseek'}
    data-interaction-region="chat"
    aria-label="AI 对话"
  >
    {props.historyExpanded && <Glass className="dsh-chat__history-wrap" strength="strong" elevation="floating">
      <div className="dsh-chat__history" ref={historyRef} aria-label="当前会话记录" aria-live="polite">
        {props.messages.length === 0 && !props.streamingText && <div className="dsh-chat__empty">
          <Icon name="spark" size={22} />
          <strong>会话还很安静</strong>
          <p>写下一个想法，大肥鱼会在这里陪你整理。</p>
        </div>}
        {props.messages.map((message) => <article key={message.id} className={`dsh-chat__message dsh-chat__message--${message.role}`}>
          <span className="dsh-chat__message-label">{message.role === 'user' ? '你' : '大肥鱼'}</span>
          <p className="dsh-chat__message-body">{message.content}</p>
          <UsageLine usage={message.usage} />
        </article>)}
        {props.streamingText && <article className="dsh-chat__message dsh-chat__message--assistant">
          <span className="dsh-chat__message-label">大肥鱼</span>
          <p className="dsh-chat__message-body">{props.streamingText}<span className="dsh-chat__caret" aria-hidden="true" /></p>
        </article>}
      </div>
    </Glass>}

    <Glass className="dsh-chat__card" strength="strong" elevation="floating">
      <header className="dsh-chat__topbar">
        <div className="dsh-chat__identity">
          <span className="dsh-chat__sigil"><Icon name="spark" size={14} /></span>
          <span className="dsh-chat__backend">{backend.shortName}</span>
          <span className="dsh-chat__backend-detail">{backend.description}</span>
        </div>
        <span className="dsh-chat__topbar-spacer" />
        <span className="dsh-chat__status" title={backend.name}>
          <span className="dsh-chat__status-dot" />{ACTIVITY_LABEL[props.activity]}
        </span>
        <Button variant="ghost" iconOnly onClick={props.onClose} aria-label="收起对话"><Icon name="close" /></Button>
      </header>

      <form className="dsh-chat__composer" onSubmit={(event) => { event.preventDefault(); submit() }}>
        <textarea
          className="dsh-chat__textarea"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault()
              submit()
            }
          }}
          placeholder={composerPlaceholder(Boolean(props.disabled), props.activity)}
          disabled={props.disabled}
          rows={2}
          aria-label="输入消息"
        />
        {busy
          ? <Button className="dsh-chat__stop" variant="secondary" iconOnly onClick={props.onStop} aria-label="停止生成"><Icon name="stop" /></Button>
          : <Button className="dsh-chat__send" variant="primary" iconOnly type="submit" disabled={!draft.trim() || props.disabled} aria-label="发送消息"><Icon name="arrow-up" /></Button>}
      </form>

      <footer className="dsh-chat__footer">
        <span className="dsh-chat__meta"><Icon name="model" size={13} /><span className="dsh-chat__model">{props.modelLabel}</span></span>
        {props.backend === 'deepseek-api' && <><span className="dsh-chat__separator" /><span className="dsh-chat__billing">API 计费</span></>}
        {tokenCount !== undefined && <><span className="dsh-chat__separator" /><span>{tokenCount} tokens</span></>}
        {totalCost > 0 && <><span className="dsh-chat__separator" /><span>会话 {formatCost(totalCost)}</span></>}
        <Button className="dsh-chat__history-button" variant="ghost" onClick={props.onToggleHistory} aria-expanded={props.historyExpanded}>
          <Icon name="history" size={14} />{props.historyExpanded ? '收起记录' : '会话记录'}<Icon name="chevron-up" size={13} />
        </Button>
      </footer>
    </Glass>
  </section>
}
