import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { Activity, BackendMode, ChatMessage, RuntimeState, TokenUsage } from '../../domain/types.ts'
import { Button, Glass, Icon } from '../../ui/primitives/index.ts'
import { composerPlaceholder, formatCost, isBusyActivity, sessionCostSummary, turnUsageSummary } from './conversationViewModel.ts'
import { growHistoryWindow, historyWindow, HISTORY_RENDER_WINDOW } from './streamRender.ts'
import { harnessStateLabel } from '../../connect/harnessLabels.ts'
import './ConversationBubble.css'

/** Optional speaker labels for themes that want explicit attribution. */
export type ConversationSpeakerLabels = Partial<Record<ChatMessage['role'], string>>

export interface ConversationBubbleProps {
  backend: BackendMode
  activity: Activity
  modelLabel: string
  messages: ChatMessage[]
  streamingText: string
  historyExpanded: boolean
  /**
   * Draft text owned by the parent.
   *
   * The composer is deliberately rebuilt when the desktop regains the foreground,
   * because only a rebuild restores the WebView keyboard channel. Keeping the draft
   * above this component means that rebuild does not discard what the user typed.
   */
  initialDraft?: string
  onDraftChange?: (draft: string) => void
  /** Hidden by default; callers may opt into custom role names. */
  speakerLabels?: ConversationSpeakerLabels
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
  sendShortcut?: 'Enter' | 'Ctrl+Enter'
  onToggleHistory: () => void
  onSend: (text: string) => void
  onStop: () => void
  onClose: () => void
}

/// What a drafting or sending action should do to the conversation history view.
///
/// Three branches, and each one is deliberate:
///
/// - typing while collapsed must not reveal the history, because expanding on the first
///   keystroke moves the composer while the user is still aiming at it;
/// - sending while collapsed must always reveal it, because the reply is about to arrive;
/// - typing or sending while already expanded changes nothing, so the layout stays put.
export function shouldRevealHistory(
  trigger: 'typing' | 'send',
  historyExpanded: boolean,
): boolean {
  if (historyExpanded) return false
  return trigger === 'send'
}

export function insertNewlineAtSelection(value: string, start: number, end: number): { value: string; caret: number } {
  const safeStart = Math.max(0, Math.min(start, value.length))
  const safeEnd = Math.max(safeStart, Math.min(end, value.length))
  return {
    value: `${value.slice(0, safeStart)}\n${value.slice(safeEnd)}`,
    caret: safeStart + 1,
  }
}

function UsageLine({ usage }: { usage?: TokenUsage }) {  if (!usage) return null
  return <small className="dsh-chat__message-usage">
    输入 {usage.input} · 输出 {usage.output}
    {usage.cacheRead !== undefined ? ` · 缓存 ${usage.cacheRead}` : ''}
    {usage.cost !== undefined ? ` · ${formatCost(usage.cost, usage.estimated)}` : ''}
  </small>
}

export function ConversationBubble(props: ConversationBubbleProps) {
  // Seeded from the parent so the deliberate rebuild that restores keyboard
  // focus does not discard a half-typed message.
  const [draft, setDraft] = useState(() => props.initialDraft ?? '')
  useEffect(() => { props.onDraftChange?.(draft) }, [draft])
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [commandMenuOpen, setCommandMenuOpen] = useState(false)
  const historyRef = useRef<HTMLDivElement>(null)
  const historyAutoScrollRef = useRef(true)
  // Streaming history is prepended to the DOM when the user asks for earlier
  // turns. Capture the scroll height before that commit and restore the same
  // distance from the bottom afterwards, so the viewport does not jump.
  const historyAnchorRef = useRef<number>()
  // Deltas are coalesced upstream (App's chat-event handler) so this surface
  // receives at most one streaming update per animation frame. It renders the
  // prop directly: the live answer must never depend on an effect having run.
  const streamText = props.streamingText
  const [historyLimit, setHistoryLimit] = useState(HISTORY_RENDER_WINDOW)
  const busy = isBusyActivity(props.activity)
  const totalCost = sessionCostSummary(props.messages)
  const turnUsage = turnUsageSummary(props.usage, props.backend, Boolean(props.apiPricingConfigured))
  const harnessAvailability = props.harnessAvailability ?? 'offline'
  const harnessReady = harnessAvailability === 'bridge-ready'
  const harnessLabel = props.harnessStarting ? 'DSH 正在启动' : harnessStateLabel(harnessAvailability)
  const showHistory = props.historyExpanded
  const today = new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', weekday: 'short' }).format(new Date())
  const historyView = useMemo(() => historyWindow(props.messages.length, historyLimit), [props.messages.length, historyLimit])
  // Only the newest slice is in the DOM. The full transcript stays in memory,
  // and the live streaming article is rendered separately, so an in-progress
  // answer is never truncated by this window.
  const renderedMessages = historyView.visible === props.messages.length
    ? props.messages
    : props.messages.slice(props.messages.length - historyView.visible)

  const loadEarlier = useCallback(() => {
    const element = historyRef.current
    if (element) historyAnchorRef.current = element.scrollHeight - element.scrollTop
    setHistoryLimit((current) => growHistoryWindow(current, props.messages.length))
  }, [props.messages.length])

  // A new backend/transcript restores the default window; the user's "load
  // earlier" choice is per-viewing, not a permanent setting.
  useEffect(() => {
    setHistoryLimit(HISTORY_RENDER_WINDOW)
  }, [props.backend])


  const submit = () => {
    const text = draft.trim()
    if (!text || busy || props.disabled) return
    // Sending always reveals the conversation, however the island looked while the
    // draft was being written. Typing deliberately does not: expanding on the first
    // keystroke moves the composer while the user is aiming at it, and it also meant a
    // collapsed island could never be typed into without changing its layout.
    if (shouldRevealHistory('send', props.historyExpanded)) props.onToggleHistory()
    props.onSend(text)
    setDraft('')
  }

  const insertNewline = (textarea: HTMLTextAreaElement) => {
    const { value: nextDraft, caret } = insertNewlineAtSelection(
      draft,
      textarea.selectionStart ?? draft.length,
      textarea.selectionEnd ?? textarea.selectionStart ?? draft.length,
    )
    setDraft(nextDraft)
    // React updates the controlled textarea on the next commit. Restore the
    // caret after that commit so Ctrl+Enter behaves like a normal newline even
    // on hosts that reserve the browser's default shortcut for submit.
    requestAnimationFrame(() => {
      textarea.selectionStart = caret
      textarea.selectionEnd = caret
    })
  }

  useEffect(() => {
    if (!props.historyExpanded) {
      historyAutoScrollRef.current = true
      return
    }
    if (historyAutoScrollRef.current) {
      // Streaming deltas arrive frequently. Smooth-scrolling on every delta
      // keeps the browser's scroll animation perpetually in flight and makes
      // wheel input appear ignored. Follow the bottom directly only while the
      // user has not deliberately moved away from it.
      historyRef.current?.scrollTo({ top: historyRef.current.scrollHeight, behavior: 'auto' })
    }
  }, [props.historyExpanded, props.messages.length, streamText])

  // Restore the reader's position after earlier turns were prepended.
  useLayoutEffect(() => {
    const anchor = historyAnchorRef.current
    if (anchor === undefined) return
    historyAnchorRef.current = undefined
    const element = historyRef.current
    if (!element) return
    element.scrollTop = element.scrollHeight - anchor
  }, [historyLimit])

  // 折叠态不在壁纸表面上渲染任何东西。
  //
  // 折叠态曾经是这里的一枚胶囊（`data-interaction-region="chat-collapsed"`）。但壁纸场景
  // 画在 Explorer 图标层之下，表桌面上它**收不到任何鼠标消息**（见
  // `docs/evidence/input-model-desktop-hit-testing.md`），那枚胶囊因此既点不到、
  // 也不能把点击放给桌面。现在折叠态由独立顶层窗口里的悬浮球承担
  // （`src/floating/BallWindow.tsx` + `src-tauri/src/floating_ball.rs`），
  // 壁纸表面只负责展开态。**不要在这里恢复折叠胶囊**：那等于把"点不到的控件"再画一遍。
  // `collapsed` 这个 prop 仍然保留，因为 `App.tsx` 还在传它。
  if (props.collapsed) return null

  return <section
    className={`conversation-shell dsh-chat dsh-theme-${props.backend === 'harness' ? 'harness' : 'deepseek'} ${showHistory ? 'expanded' : ''} ${busy ? 'dsh-chat--busy' : ''} ${hovered ? 'dsh-chat--hovered' : ''} ${focused ? 'dsh-chat--focused' : ''}`}
    data-persistent={props.persistent ? 'true' : undefined}
    style={{ ['--dsh-chat-acrylic-opacity' as string]: (props.acrylicOpacity ?? .74).toFixed(2), ['--dsh-chat-acrylic-blur' as string]: `${props.acrylicBlur ?? 19}px`, ['--dsh-chat-expanded-bottom' as string]: `${props.expandedBottomInset ?? 48}px` }}
    data-dsh-theme={props.backend === 'harness' ? 'harness' : 'deepseek'}
    data-interaction-region="chat"
    aria-label="AI 对话"
  >
    {showHistory && <div className="dsh-chat__history-wrap">
      <div
        className="dsh-chat__history"
        ref={historyRef}
        data-interaction-region="chat-history"
        aria-label="当前会话记录"
        aria-live="polite"
        onScroll={() => {
          const element = historyRef.current
          if (!element) return
          historyAutoScrollRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 28
        }}
        onWheel={(event) => event.stopPropagation()}
      >
        {historyView.hasEarlier && <button type="button" className="dsh-chat__history-earlier" onClick={loadEarlier}>
          加载更早的 {historyView.hidden} 条记录
        </button>}
        {renderedMessages.map((message, index) => <article key={message.id} className={`dsh-chat__message dsh-chat__message--${message.role}`} style={{ ['--message-index' as string]: String(Math.max(0, renderedMessages.length - index - 1)) }}>
          {props.speakerLabels?.[message.role]?.trim() && <span className="dsh-chat__message-label">{props.speakerLabels[message.role]}</span>}
          <p className="dsh-chat__message-body">{message.content}</p>
          <UsageLine usage={message.usage} />
        </article>)}
        {streamText && <article className="dsh-chat__message dsh-chat__message--assistant">
          {props.speakerLabels?.assistant?.trim() && <span className="dsh-chat__message-label">{props.speakerLabels.assistant}</span>}
          <p className="dsh-chat__message-body">{streamText}<span className="dsh-chat__caret" aria-hidden="true" /></p>
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
          onChange={(event) => {
            const nextDraft = event.target.value
            setDraft(nextDraft)
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing || event.altKey || event.metaKey) return

            const isSend = props.sendShortcut === 'Ctrl+Enter'
              ? event.key === 'Enter' && event.ctrlKey && !event.shiftKey
              : event.key === 'Enter' && !event.shiftKey && !event.ctrlKey

            const isNewline = props.sendShortcut === 'Ctrl+Enter'
              ? event.key === 'Enter' && !event.ctrlKey
              : event.key === 'Enter' && (event.ctrlKey || event.shiftKey)

            if (isSend) {
              event.preventDefault()
              submit()
            } else if (isNewline) {
              // Only need to manually insert if the browser doesn't do it naturally
              if (props.sendShortcut === 'Enter' && event.ctrlKey) {
                event.preventDefault()
                insertNewline(event.currentTarget)
              }
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
