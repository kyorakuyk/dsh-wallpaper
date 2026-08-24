import { useEffect, useRef, useState } from 'react'
import type { Activity, BackendMode, ChatMessage, RuntimeState, TokenUsage } from '../../domain/types.ts'
import { Button, Glass, Icon } from '../../ui/primitives/index.ts'
import { BACKEND_PRESENTATION, composerPlaceholder, formatCost, isBusyActivity, sessionCostSummary, turnUsageSummary } from './conversationViewModel.ts'
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
  expandedBottomInset?: number
  /** Both API rates are explicitly configured; zero is still configured. */
  apiPricingConfigured?: boolean
  /** Bridge availability drives the DSH indicator and mode switch. */
  harnessAvailability?: RuntimeState['harness']
  onSelectBackend?: (backend: BackendMode) => void
  onStartHarness?: () => void
  onConfigureHarness?: () => void
  harnessStarting?: boolean
  modelOptions?: readonly string[]
  selectedModel?: string
  onSelectModel?: (model: string) => void
  presetOptions?: readonly { id: string; name?: string; broken?: string }[]
  selectedPreset?: string
  onSelectPreset?: (preset: string) => void
  permission?: { current: string; options: string[] }
  commands?: readonly { name: string; description: string; input?: { hint: string } }[]
  onSelectPermission?: (permission: string) => void
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
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [commandMenuOpen, setCommandMenuOpen] = useState(false)
  const historyRef = useRef<HTMLDivElement>(null)
  const busy = isBusyActivity(props.activity)
  const backend = BACKEND_PRESENTATION[props.backend]
  const totalCost = sessionCostSummary(props.messages)
  const turnUsage = turnUsageSummary(props.usage, props.backend, Boolean(props.apiPricingConfigured))
  const harnessAvailability = props.harnessAvailability ?? 'offline'
  const harnessReady = harnessAvailability === 'bridge-ready'
  const harnessLabel = props.harnessStarting ? 'DSH 正在启动'
    : harnessAvailability === 'bridge-ready'
    ? 'DSH Bridge 已连接'
    : harnessAvailability === 'web-only'
      ? 'DSH 在线，缺少壁纸 Bridge'
      : 'DSH Bridge 离线'
  const showHistory = props.historyExpanded
  const today = new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', weekday: 'short' }).format(new Date())

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
    className={`conversation-shell dsh-chat dsh-theme-${props.backend === 'harness' ? 'harness' : 'deepseek'} ${showHistory ? 'expanded' : ''} ${busy ? 'dsh-chat--busy' : ''} ${hovered ? 'dsh-chat--hovered' : ''} ${focused ? 'dsh-chat--focused' : ''}`}
    data-persistent={props.persistent ? 'true' : undefined}
    style={{ ['--dsh-chat-acrylic-opacity' as string]: (props.acrylicOpacity ?? .74).toFixed(2), ['--dsh-chat-acrylic-blur' as string]: `${props.acrylicBlur ?? 19}px`, ['--dsh-chat-expanded-bottom' as string]: `${props.expandedBottomInset ?? 48}px` }}
    data-dsh-theme={props.backend === 'harness' ? 'harness' : 'deepseek'}
    data-interaction-region="chat"
    aria-label="AI 对话"
  >
    {showHistory && <div className="dsh-chat__history-wrap">
      <div className="dsh-chat__history" ref={historyRef} data-interaction-region="chat-history" aria-label="当前会话记录" aria-live="polite" onWheel={(event) => event.stopPropagation()}>
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
    </div>}

    <header className="dsh-chat__topbar">
        <div className="dsh-chat__identity">
          <span className="dsh-chat__sigil"><Icon name="spark" size={14} /></span>
          <span className="dsh-chat__backend">桌面会话</span>
          <span className="dsh-chat__backend-detail">{today}</span>
        </div>
        <span className="dsh-chat__topbar-spacer" />
        <div className="dsh-chat__usage-rail">
          <span className="dsh-chat__turn-usage" data-usage-available={turnUsage.available ? 'true' : 'false'} aria-label={turnUsage.available ? '本轮用量' : '本轮用量未提供'}>
            <span>本轮 入 {turnUsage.input}</span>
            <span>出 {turnUsage.output}</span>
            <span>缓存 {turnUsage.cacheRead}</span>
            <span className={turnUsage.cost === '价格未配置' ? 'dsh-chat__billing' : undefined}>费用 {turnUsage.cost}</span>
          </span>
          <span className="dsh-chat__session-cost">
            <span>{totalCost ? `会话 ${formatCost(totalCost.cost, totalCost.estimated)}` : '会话费用未提供'}</span>
          </span>
        </div>
        <span className="dsh-chat__topbar-spacer" />
        <div className="dsh-chat__bottom-status">
        <span className={`dsh-chat__status dsh-chat__status--harness dsh-chat__status--${harnessAvailability}`} title={harnessLabel}>
          <span className="dsh-chat__status-dot" />{harnessLabel}
        </span>
        <button
          type="button"
          className={`dsh-chat__mode-switch ${props.backend === 'harness' ? 'is-harness' : ''}`}
          role="switch"
          aria-checked={props.backend === 'harness'}
          aria-label={harnessReady ? `切换至${props.backend === 'harness' ? ' DeepSeek' : ' Harness'} 模式` : props.harnessStarting ? harnessLabel : props.onStartHarness ? '启动 DSH' : '配置 DSH'}
          title={harnessReady ? `当前：${props.backend === 'harness' ? 'Harness，点击切回 DeepSeek' : 'DeepSeek，点击切换 Harness'}` : props.harnessStarting ? harnessLabel : props.onStartHarness ? '启动已配置的 DSH 后端' : '先配置 DSH 根目录与 profile'}
          disabled={props.harnessStarting}
          onClick={() => harnessReady
            ? props.onSelectBackend?.(props.backend === 'harness' ? 'deepseek-web' : 'harness')
            : props.onStartHarness?.() ?? props.onConfigureHarness?.()}
        >
          <span className="dsh-chat__mode-switch-track"><span className="dsh-chat__mode-switch-knob" /></span>
          <span className="dsh-chat__mode-switch-label" aria-hidden="true">DSH</span>
        </button>
        </div>
        {!props.persistent && <Button variant="ghost" iconOnly onClick={props.onClose} aria-label="收起对话"><Icon name="close" /></Button>}
      </header>

    <Glass
      className="dsh-chat__card"
      strength="strong"
      elevation="floating"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocusCapture={() => setFocused(true)}
      onBlurCapture={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false) }}
    >
      <div className="dsh-chat__island-toolbar">
        <div className="dsh-chat__island-toolbar-left">
          <label className="dsh-chat__preset">
            <select aria-label="选择 DSH 模式" value={props.selectedPreset ?? ''} disabled={!props.onSelectPreset || props.messages.length > 0} onChange={(event) => props.onSelectPreset?.(event.target.value)}>
              {!props.presetOptions?.length && <option value="">标准模式</option>}
              {props.presetOptions?.map((preset) => <option key={preset.id} value={preset.id} disabled={Boolean(preset.broken)}>{preset.name ?? preset.id}{preset.broken ? '（不可用）' : ''}</option>)}
            </select>
          </label>
        </div>
        <Button className="dsh-chat__history-button" variant="ghost" onClick={props.onToggleHistory} aria-expanded={showHistory}>
          <Icon name="history" size={14} />{showHistory ? '收起记录' : '会话记录'}<Icon name="chevron-up" size={13} />
        </Button>
      </div>
      <form className="dsh-chat__composer" onSubmit={(event) => { event.preventDefault(); submit() }}>
        <textarea
          className="dsh-chat__textarea"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey && !event.nativeEvent.isComposing) {
              event.preventDefault()
              submit()
            }
          }}
          placeholder={composerPlaceholder(Boolean(props.disabled), props.activity)}
          rows={3}
          aria-label="输入消息"
        />
        {busy
          ? <Button className="dsh-chat__stop" variant="secondary" iconOnly onClick={props.onStop} aria-label="停止生成"><Icon name="stop" /></Button>
          : <Button className="dsh-chat__send" variant="primary" iconOnly type="submit" disabled={!draft.trim() || props.disabled} aria-label="发送消息"><Icon name="arrow-up" /></Button>}
      </form>

      <footer className="dsh-chat__footer">
        {props.commands?.length ? <div className="dsh-chat__command-menu">
          <button type="button" className="dsh-chat__command-menu-button" aria-haspopup="menu" aria-expanded={commandMenuOpen} onClick={() => setCommandMenuOpen((value) => !value)}>
            <span aria-hidden="true">⌘</span> 命令 <span aria-hidden="true">⌄</span>
          </button>
          {commandMenuOpen && <div className="dsh-chat__command-menu-list" role="menu" aria-label="选择命令">
            {props.commands.map((command) => <button key={command.name} type="button" role="menuitem" className="dsh-chat__command-menu-item" onClick={() => { setDraft(`/${command.name} `); setCommandMenuOpen(false) }}>
              <strong>/{command.name}</strong><span>{command.description}</span>
            </button>)}
          </div>}
        </div> : null}
        {props.permission && <label className="dsh-chat__permission-picker">◈<select aria-label="选择权限" value={props.permission.current} onChange={(event) => props.onSelectPermission?.(event.target.value)}>{props.permission.options.map((permission) => <option key={permission} value={permission}>{permission}</option>)}</select></label>}
        {(!props.onSelectModel || !props.modelOptions?.length) && <span className="dsh-chat__meta"><Icon name="model" size={13} /><span className="dsh-chat__model">{props.modelLabel}</span></span>}
        <label className="dsh-chat__model-picker" title={props.onSelectModel ? '切换模型' : '当前后端不支持在壁纸中切换模型'}>
          <Icon name="model" size={13} />
          <select
            aria-label="切换模型"
            value={props.selectedModel ?? ''}
            disabled={!props.onSelectModel || !props.modelOptions?.length}
            onChange={(event) => props.onSelectModel?.(event.target.value)}
          >
            {!props.modelOptions?.length && <option value="">模型不可切换</option>}
            {props.modelOptions?.map((model) => <option key={model} value={model}>{model}</option>)}
          </select>
        </label>
      </footer>
    </Glass>
  </section>
}
