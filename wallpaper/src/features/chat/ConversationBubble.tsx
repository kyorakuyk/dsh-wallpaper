import { useEffect, useRef, useState } from 'react'
import type { Activity, BackendMode, ChatMessage, TokenUsage } from '../../domain/types.ts'
import { Button, Glass, Icon } from '../../ui/primitives/index.ts'
import { ACTIVITY_LABEL, BACKEND_PRESENTATION, composerPlaceholder, formatCost, isBusyActivity, sessionCostSummary, turnUsageSummary } from './conversationViewModel.ts'
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
  persistent?: boolean
  acrylicOpacity?: number
  acrylicBlur?: number
  /** Both API rates are explicitly configured; zero is still configured. */
  apiPricingConfigured?: boolean
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
  const totalCost = sessionCostSummary(props.messages)
  const turnUsage = turnUsageSummary(props.usage, props.backend, Boolean(props.apiPricingConfigured))
  // An empty drawer has no information value and turns the workspace into a
  // large blank panel. It appears only once there is transcript content.
  const showHistory = props.historyExpanded && (props.messages.length > 0 || Boolean(props.streamingText))

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

  return <section
    className={`conversation-shell dsh-chat dsh-theme-${props.backend === 'harness' ? 'harness' : 'deepseek'} ${showHistory ? 'expanded' : ''} ${busy ? 'dsh-chat--busy' : ''}`}
    data-persistent={props.persistent ? 'true' : undefined}
    style={{ ['--dsh-chat-acrylic-opacity' as string]: (props.acrylicOpacity ?? .74).toFixed(2), ['--dsh-chat-acrylic-blur' as string]: `${props.acrylicBlur ?? 19}px` }}
    data-dsh-theme={props.backend === 'harness' ? 'harness' : 'deepseek'}
    data-interaction-region="chat"
    aria-label="AI 对话"
  >
    {showHistory && <Glass className="dsh-chat__history-wrap" strength="strong" elevation="floating">
      <div className="dsh-chat__history" ref={historyRef} aria-label="当前会话记录" aria-live="polite">
        {props.messages.map((message, index) => <article key={message.id} className={`dsh-chat__message dsh-chat__message--${message.role}`} style={{ ['--message-index' as string]: String(Math.max(0, props.messages.length - index - 1)) }}>
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
        {!props.persistent && <Button variant="ghost" iconOnly onClick={props.onClose} aria-label="收起对话"><Icon name="close" /></Button>}
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
          rows={3}
          aria-label="输入消息"
        />
        {busy
          ? <Button className="dsh-chat__stop" variant="secondary" iconOnly onClick={props.onStop} aria-label="停止生成"><Icon name="stop" /></Button>
          : <Button className="dsh-chat__send" variant="primary" iconOnly type="submit" disabled={!draft.trim() || props.disabled} aria-label="发送消息"><Icon name="arrow-up" /></Button>}
      </form>

      <footer className="dsh-chat__footer">
        <span className="dsh-chat__meta"><Icon name="model" size={13} /><span className="dsh-chat__model">{props.modelLabel}</span></span>
        <span className="dsh-chat__turn-usage" data-usage-available={turnUsage.available ? 'true' : 'false'} aria-label={turnUsage.available ? '本轮用量' : '本轮用量未提供'}>
          <span className="dsh-chat__separator" />
          <span>本轮 入 {turnUsage.input}</span>
          <span>出 {turnUsage.output}</span>
          <span>缓存 {turnUsage.cacheRead}</span>
          <span className={turnUsage.cost === '价格未配置' ? 'dsh-chat__billing' : undefined}>费用 {turnUsage.cost}</span>
        </span>
        <span className="dsh-chat__session-cost">
          <span className="dsh-chat__separator" />
          <span>{totalCost ? `会话 ${formatCost(totalCost.cost, totalCost.estimated)}` : '会话费用未提供'}</span>
        </span>
        <Button className="dsh-chat__history-button" variant="ghost" onClick={props.onToggleHistory} aria-expanded={showHistory}>
          <Icon name="history" size={14} />{showHistory ? '收起记录' : '会话记录'}<Icon name="chevron-up" size={13} />
        </Button>
      </footer>
    </Glass>
  </section>
}
