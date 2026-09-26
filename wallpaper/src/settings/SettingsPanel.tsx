import { useEffect, useRef, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { BackendMode, ModelTierRule } from '../domain/types.ts'
import { BACKGROUND_OPTIONS, MAX_PRICE_PER_MILLION, normalizedPrice, type WallpaperSettings } from './store.ts'
import { type SettingsPage } from './settingsProbes.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
import type { DeepSeekWebAdapterConfigStatus, DesktopDisplayInfo, LockScreenDiagnostics, ManagedDshStatus, ApiConversationListing, ApiKeyStatus } from '../native/runtime.ts'
import { preferredDisplayId } from '../runtime/displayLayout.ts'
import { harnessStateLabel } from '../connect/harnessLabels.ts'
import type { AutostartStatus, HarnessEndpointScan, HarnessTarget } from '../native/runtime.ts'
import { catalogAgeLabel, displaySubjectPath, sameSubject, subjectOptionLabel } from '../connect/harnessSubjects.ts'
import { OfficialPersonaCards } from '../persona/OfficialPersonaCards.tsx'
import './SettingsPanel.css'

type Page = SettingsPage

/**
 * The settings sidebar renders the same diagnostic states the wallpaper probe
 * produces. Re-exported from the probe so a new state cannot be added on one
 * side only.
 */
export type SettingsPanelHarnessStatus = import('../connect/harness.ts').HarnessAvailability

export interface SettingsPanelProps {
  settings: WallpaperSettings
  /**
   * The active page is owned by `SettingsWindow`, because it also drives which
   * system probes may start. `SettingsPanel` only renders the matching page.
   */
  page: Page
  onPageChange: (page: Page) => void
  harnessStatus: SettingsPanelHarnessStatus
  onChange: (settings: WallpaperSettings) => void
  onRequestDeepSeekLogin: () => void
  /**
   * 访问密钥：待保存的草稿、已保存那一条的脱敏形态、可用模型列表，以及三个动作。
   *
   * 草稿与脱敏串分开传：输入框里是"要保存的"，下面那行是"已经存好的"。
   */
  apiKeyDraft: string
  onApiKeyDraftChange: (value: string) => void
  apiKeyStatus?: ApiKeyStatus
  apiKeyBusy: boolean
  onTestApiKey: () => void
  onRefreshApiModels: () => void
  apiModelCatalog?: Array<{ id: string; name: string }>
  /** 上面那份列表的拉取时间（ISO 串）；来自持久化缓存，用来说明它的新鲜度。 */
  apiModelCatalogFetchedAt?: string
  /**
   * 壁纸此刻在用的 chat 模式（`undefined` = 还没读到快照）。
   *
   * 与 `settings.defaultBackend` 分开传：托盘、自动切换、主体退出后的复位都会改运行值而不改设置。
   */
  liveBackend?: BackendMode
  /** 切到某个模式：设置中心会调原生 `select_backend`，正在运行的壁纸立即生效。 */
  onSelectBackend: (backend: BackendMode) => void
  onClose: () => void
  interactionEnabled: boolean
  onSetInteractionEnabled: (enabled: boolean) => void
  translucentTb: { installed: boolean; running: boolean; source?: string }
  onRefreshTranslucentTb: () => void
  onLaunchTranslucentTb: () => void
  onInstallTranslucentTb: () => void
  /**
   * The execution subjects the shim found: shells that carry their own checkout,
   * and source trees (§3). Choosing one fixes which service the wallpaper starts;
   * a source tree still needs its window (and its profile) chosen separately.
   */
  harnessTargets: HarnessTarget[]
  /**
   * When those subjects were last confirmed by a scan. Shown as an age rather than
   * hidden, because a cached list that looks current would send the launcher after a
   * client the user has since uninstalled.
   */
  subjectCatalogVerifiedAt?: number
  /** The §4.3 prompt when several source trees exist, so the user chooses one. */
  subjectChoice?: string
  onSelectSubject: (targetId: string) => void
  /**
   * Actual Windows autostart state. The "start DSH with the wallpaper" setting
   * is only a wallpaper-start trigger, so the card must say so rather than
   * implying a login-time guarantee.
   */
  autostart: AutostartStatus
  onScanDsh: () => void
  dshScanBusy: boolean
  /**
   * Endpoint discovery. The wallpaper used to probe one hardcoded port (3080),
   * which silently meant "only ever connect to the CLI shape" — the official
   * desktop shell listens on 19387 and the community desktop on 43120. The card
   * reports how many Bridges the scan found and lets the user pin one.
   */
  endpointScan: HarnessEndpointScan[]
  endpointScanBusy: boolean
  endpointScanDone: boolean
  onScanEndpoints: () => void
  /**
   * Reach the selected client's own interface. The action differs by client
   * shape: a desktop client's window is raised, while the windowless CLI/webui
   * shape is opened in the default browser at the endpoint it listens on.
   */
  /** Idempotent 「打开界面」: start it if needed, then show it. */
  onOpenClient: () => void
  /** Which action the current selection takes, for the button label. */
  reachAction: 'browser' | 'window'
  /** Opening waits for the client to answer, so the button reports that wait. */
  openBusy: boolean
  managedDsh: ManagedDshStatus
  onRefreshManagedDsh: () => void
  onStopManagedDsh: () => void
  deepseekWebAdapterConfig?: DeepSeekWebAdapterConfigStatus
  onRefreshDeepSeekWebAdapterConfig: () => void
  onOpenDeepSeekWebAdapterConfig: () => void
  onResetDeepSeekWebAdapterConfig: () => void
  appearanceAssets: AppearanceAssetSummary[]
  appearanceOverrides: Partial<Record<AppearanceSlot, string>>
  appearanceBusy: boolean
  onImportAppearance: () => void
  onClassifyAppearance: (assetId: string, slot: AppearanceSlot) => void
  onSelectAppearance: (slot: AppearanceSlot, assetId: string) => void
  onClearAppearance: (slot: AppearanceSlot) => void
  lockScreenDiagnostics?: LockScreenDiagnostics
  onRefreshLockScreenDiagnostics: () => void
  onRestoreLockScreen: () => void
  onClearStaleLockScreenBackup: () => void
  onSetLockScreenEnabled: (enabled: boolean) => void
  lockScreenBusy: boolean
  autostartBusy: boolean
  desktopDisplays: DesktopDisplayInfo[]
  onRefreshDesktopDisplays: () => void | Promise<void>
  /** Durable DeepSeek API transcripts, for the history management page. */
  apiHistory?: ApiConversationListing
  apiHistoryBusy: boolean
  onRefreshApiHistory: () => void
  onDeleteApiConversation: (conversationId: string) => void
  onClearApiHistory: () => void
}

const componentSlots: Array<{ slot: AppearanceSlot; label: string; detail: string }> = [
  { slot: 'desktop.background', label: '桌面背景', detail: '工作室场景的底图' },
  { slot: 'persona.deepseek.flash', label: 'DeepSeek Flash 立绘', detail: '蓝色幼年形态' },
  { slot: 'persona.deepseek.pro', label: 'DeepSeek Pro 立绘', detail: '蓝色成年形态' },
  { slot: 'persona.harness.flash', label: 'Harness Flash 立绘', detail: '黑红幼年形态' },
  { slot: 'persona.harness.pro', label: 'Harness Pro 立绘', detail: '黑红成年形态' },
]

const pages: Array<{ id: Page; icon: string; label: string; hint: string }> = [
  { id: 'general', icon: '⌂', label: '常规', hint: '启动与使用方式' },
  { id: 'connections', icon: '⌁', label: '连接', hint: 'DeepSeek 与 DSH' },
  { id: 'appearance', icon: '◐', label: '外观', hint: '背景与动画' },
  { id: 'personas', icon: '◇', label: '形态', hint: '模型映射规则' },
  { id: 'history', icon: '☰', label: '历史', hint: 'API 会话记录' },
  { id: 'system', icon: '⚙', label: '系统', hint: 'Windows 集成' },
]

/** `18.4 MB`-style size for the history rows. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${Math.round(bytes)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** Local timestamp for one history row; `—` when the value is unknown. */
export function formatHistoryTime(timestamp: number, now: Date = new Date()): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '—'
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return '—'
  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate()
  const time = `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
  return sameDay
    ? `今天 ${time}`
    : `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')} ${time}`
}

/**
 * How close the archive is to the ceiling that stops persistence, expressed as
 * a percentage of the budget the application trims to. The UI must never imply
 * "full" at exactly the budget: trimming happens silently at that point, and
 * only the hard ceiling causes a refusal.
 */
export function historyPressure(totalBytes: number, budgetBytes: number): number {
  if (!Number.isFinite(totalBytes) || !Number.isFinite(budgetBytes) || budgetBytes <= 0) return 0
  return Math.max(0, Math.min(100, Math.round(totalBytes / budgetBytes * 100)))
}

function Card({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return <section className="settings-card"><header><h2>{title}</h2>{description && <p>{description}</p>}</header><div className="settings-card__body">{children}</div></section>
}

function Field({ title, detail, children }: { title: string; detail?: string; children: ReactNode }) {
  return <div className="settings-field"><div className="settings-field__copy"><strong>{title}</strong>{detail && <span>{detail}</span>}</div><div className="settings-field__control">{children}</div></div>
}

function Toggle({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={`settings-toggle ${checked ? 'is-on' : ''}`} onClick={() => onChange(!checked)}><span /></button>
}

function Choice({ value, options, onChange, label, disabled = false, emptyMessage }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string; disabled?: boolean; emptyMessage?: string }) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const current = options.find((option) => option.value === value)?.label ?? options[0]?.label ?? '请选择'
  useEffect(() => {
    const close = (event: MouseEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false) }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [])
  return <div className={`settings-choice ${open ? 'is-open' : ''}`} ref={root}>
    <button type="button" className="settings-choice__trigger" aria-label={label} aria-expanded={open} disabled={disabled} onClick={() => setOpen((shown) => !shown)}><span>{current}</span><i>⌄</i></button>
    {open && <div className="settings-choice__menu" role="listbox" aria-label={label}>{options.map((option) => <button type="button" key={option.value} className={option.value === value ? 'is-selected' : ''} role="option" aria-selected={option.value === value} onClick={() => { onChange(option.value); setOpen(false) }}>{option.label}</button>)}{emptyMessage && <span className="settings-choice__empty">{emptyMessage}</span>}</div>}
  </div>
}



/** Local copy of the kind label so the panel does not import the connect layer. */
export function harnessEndpointKindLabel(kind: HarnessEndpointScan['kind']): string {
  switch (kind) {
    case 'official-desktop': return '桌面客户端'
    case 'community-desktop': return '第三方桌面客户端'
    default: return 'Web / CLI'
  }
}

function displayLabel(display: DesktopDisplayInfo, index: number): string {  const number = /DISPLAY(\d+)/i.exec(display.id)?.[1]
  return number ? `显示器 ${number}` : display.name.trim() || `显示器 ${index + 1}`
}

/**
 * 后端 → 显示名。三种都在：提示语与"此刻在用哪一格"仍然会遇到 Harness（托盘、自动切换、
 * 主体退出后的复位都可能把壁纸切过去），缺一个就会退化成内部 id。
 */
export const BACKEND_MODE_LABELS: Record<BackendMode, string> = {
  'deepseek-web': 'DeepSeek 网页入口（实验）',
  'deepseek-api': 'DeepSeek API（付费）',
  harness: 'DeepSeek Harness',
}

/**
 * 「聊天模式」下拉里的候选：**只有两种聊天后端**。
 *
 * Harness 刻意不在这里（用户要求从这个下拉里删掉它）：它不是"换一个聊天后端"，而是
 * "换一个主体来对话"——连不连、什么时候连，由桌面上的那个开关以及「DSH 就绪时自动切换」管，
 * 在这里再给一个入口只会让两条路互相打架。
 */
export const CHAT_MODE_OPTIONS: Array<{ value: BackendMode; label: string }> = [
  { value: 'deepseek-web', label: BACKEND_MODE_LABELS['deepseek-web'] },
  { value: 'deepseek-api', label: BACKEND_MODE_LABELS['deepseek-api'] },
]

/**
 * 候选 + **当前值**。
 *
 * 当前值可能是 Harness：那时它必须留在候选里并标出来源，否则 `Choice` 会退回第一个候选，
 * 控件显示成"网页入口"而壁纸其实在 Harness 上——一个会骗人的开关。
 */
export function chatModeOptions(current: BackendMode): Array<{ value: BackendMode; label: string }> {
  if (CHAT_MODE_OPTIONS.some((option) => option.value === current)) return [...CHAT_MODE_OPTIONS]
  return [...CHAT_MODE_OPTIONS, { value: current, label: `${BACKEND_MODE_LABELS[current]}（由桌面开关切换）` }]
}

export function backendModeLabel(backend: BackendMode): string {
  return BACKEND_MODE_LABELS[backend] ?? backend
}

/**
 * 模型列表那句说明里的"什么时候拉的"。
 *
 * 列表是**持久化缓存**（用户实测要求"刷新结果要持久化"），所以它可能是几天前拉的；不把时间写出来，
 * 用户会以为打开设置时刚拉过。解析不了就什么都不说，而不是编一个时间。
 */
export function catalogAgeSuffix(fetchedAt: string | undefined, now = Date.now()): string {
  if (!fetchedAt) return ''
  const at = Date.parse(fetchedAt)
  if (!Number.isFinite(at)) return ''
  const minutes = Math.max(0, Math.round((now - at) / 60_000))
  if (minutes < 1) return '（刚刚拉取）'
  if (minutes < 60) return `（${minutes} 分钟前拉取）`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `（${hours} 小时前拉取）`
  return `（${Math.round(hours / 24)} 天前拉取）`
}

/**
 * 「模型」那一栏的候选：拉取到的目录 + **当前值**。
 *
 * 三件事都在这里说清：
 * 1. 显示名用「id · 官方名」（`deepseek-flash · DeepSeek-V4.1-Flash`），因为 API 认的是 id；
 * 2. **当前值必须留在候选里**，哪怕它已经不在目录中——否则控件会退回到"第一个候选"，看起来
 *    像设置被别人改掉了。官方改名时这条尤其重要：用户机器上多半存着一个旧名字；
 * 3. 旧名字要**标出来**（"不在当前目录里"），而不是静默地混在新名字中间。
 */
export function apiModelOptions(
  catalog: Array<{ id: string; name: string }>,
  current: string,
): Array<{ value: string; label: string }> {
  const options = catalog.map((model) => ({
    value: model.id,
    label: model.name && model.name !== model.id ? `${model.id} · ${model.name}` : model.id,
  }))
  const trimmed = current.trim()
  if (trimmed && !options.some((option) => option.value === trimmed)) {
    options.push({ value: trimmed, label: `${trimmed}（不在当前目录里）` })
  }
  return options
}

export function PriceInput({
  label,
  value,
  onChange,
}: {
  label: string
  value: number | undefined
  onChange: (value: number | undefined) => void
}) {
  return <input
    aria-label={label}
    className="price-input"
    type="number"
    min="0"
    max={MAX_PRICE_PER_MILLION}
    step="0.0001"
    inputMode="decimal"
    placeholder="未配置"
    value={value ?? ''}
    onChange={(event) => {
      const raw = event.target.value.trim()
      const parsed = Number(raw)
      onChange(raw === '' ? undefined : normalizedPrice(parsed))
    }}
  />
}

export function SettingsPanel(props: SettingsPanelProps) {
  const { settings, harnessStatus, onChange, onClose, translucentTb, page } = props
  const set = (patch: Partial<WallpaperSettings>) => onChange({ ...settings, ...patch })
  /**
   * The chosen subject, and whether it is the class that carries its own checkout.
   *
   * An unknown or not-yet-chosen id is treated as a checkout: that is what the
   * root-path/profile/launcher fields describe, and it is also the class a
   * profile written by an older version falls back to.
   */
  const selectedSubject = props.harnessTargets.find((target) => target.id === settings.dshLaunch.subjectId)
  const shellSelected = selectedSubject?.kind === 'embedded-shell'
  /**
   * Clients that are actually running. The endpoint picker is only a *choice* when
   * there is more than one: with zero or one, ports and Bridge states are internal
   * bookkeeping the user cannot act on, so showing them would be disclosure without
   * a decision attached to it.
   */
  const updateRule = (index: number, patch: Partial<ModelTierRule>) => set({ modelTierRules: settings.modelTierRules.map((rule, i) => i === index ? { ...rule, ...patch } : rule) })
  const pageMeta = pages.find((item) => item.id === page)!
  const displayOptions = props.desktopDisplays.map((display, index) => ({ value: display.id, label: displayLabel(display, index) }))
  const setDisplayBackground = (displayId: string, value: string) => {
    const backgrounds = { ...settings.multiScreen.backgrounds }
    if (value) backgrounds[displayId] = value as WallpaperSettings['background']
    else delete backgrounds[displayId]
    set({ multiScreen: { ...settings.multiScreen, backgrounds } })
  }

  return <div className="settings-app">
    <header className="settings-titlebar">
      <div className="settings-titlebar__drag" aria-hidden="true" onMouseDown={(event) => {
        if (event.button === 0) void invoke('start_settings_drag')
      }} />
      <div className="settings-brand"><span className="settings-brand__mark">DSH</span><div><strong>Wallpaper</strong><small>个性化控制中心</small></div></div>
      <button className="settings-window-close" aria-label="关闭设置" onClick={onClose}>×</button>
    </header>

    <aside className="settings-sidebar">
      <nav>{pages.map((item) => <button key={item.id} className={page === item.id ? 'is-active' : ''} onClick={() => props.onPageChange(item.id)}><span className="settings-nav__icon">{item.icon}</span><span><strong>{item.label}</strong><small>{item.hint}</small></span></button>)}</nav>
      <div className="settings-sidebar__status">
        <i className={harnessStatus === 'bridge-ready' ? 'is-online' : harnessStatus === 'offline' ? '' : 'is-pending'} />
        <span>{harnessStateLabel(harnessStatus)}</span>
      </div>
    </aside>

    <main className="settings-content">
      <div className="settings-page-heading"><div><span>设置 / {pageMeta.label}</span><h1>{pageMeta.label}</h1></div><p>{pageMeta.hint}</p></div>

      {page === 'general' && <>
        <Card title="交互方式" description="决定会话气泡如何出现在桌面上。">
          <Field title="中央会话窗" detail="关闭后仅可通过托盘右键或此处重新显示；不会因失焦、切换应用或按 Esc 自动消失。"><Toggle label="显示中央会话窗" checked={props.interactionEnabled} onChange={props.onSetInteractionEnabled} /></Field>
          <Field title="气泡布局" detail="中央悬浮始终展开；任务栏停靠以胶囊按钮唤起。"><Choice label="气泡布局" value={settings.interactionLayout} onChange={(value) => set({ interactionLayout: value as WallpaperSettings['interactionLayout'] })} options={[{ value: 'floating', label: '中央玻璃悬浮' }, { value: 'taskbar-docked', label: '任务栏停靠胶囊' }]} /></Field>
          <Field title="历史抽屉默认展开" detail="启动或解锁后直接显示最近的对话。"><Toggle label="历史抽屉默认展开" checked={settings.historyStartsExpanded} onChange={(value) => set({ historyStartsExpanded: value })} /></Field>
          <Field title="发送消息快捷键" detail="习惯回车换行的开发者可切换为 Ctrl+Enter 发送。"><Choice label="发送消息快捷键" value={settings.sendShortcut} onChange={(value) => set({ sendShortcut: value as WallpaperSettings['sendShortcut'] })} options={[{ value: 'Enter', label: 'Enter 发送，Ctrl+Enter 换行' }, { value: 'Ctrl+Enter', label: 'Ctrl+Enter 发送，Enter 换行' }]} /></Field>
        </Card>
        {props.desktopDisplays.length > 1 && <Card title={`多屏桌面 · 已检测 ${props.desktopDisplays.length} 个屏幕`} description="每块屏幕独立铺满自己的背景；对话窗和立绘可以分别指定目标屏幕。未单独指定的屏幕跟随全局背景。">
          <Field title="启用独立多屏背景" detail={settings.multiScreen.enabled ? '已按屏幕分别渲染；修改某一屏不会改变其他屏幕的背景选择。' : '关闭时保持现有跨虚拟桌面的单一场景；开启后才显示逐屏选择。'}><Toggle label="启用独立多屏背景" checked={settings.multiScreen.enabled} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, enabled: value } })} /></Field>
          {settings.multiScreen.enabled && <>
            {props.desktopDisplays.map((display, index) => <Field key={display.id} title={displayOptions[index]?.label ?? `显示器 ${index + 1}`} detail={`${display.bounds.width} × ${display.bounds.height} 像素 · 缩放 ${Math.round(display.scaleFactor * 100)}%${display.primary ? ' · 主显示器' : ''}`}><Choice label={`${displayOptions[index]?.label ?? display.id} 背景`} value={settings.multiScreen.backgrounds[display.id] ?? ''} onChange={(value) => setDisplayBackground(display.id, value)} options={[{ value: '', label: '跟随全局背景' }, ...BACKGROUND_OPTIONS.map((background) => ({ value: background.id, label: background.name }))]} /></Field>)}
            <Field title="对话窗所在屏幕" detail="只移动会话层，不重新加载其他屏幕的背景。"><Choice label="对话窗所在屏幕" value={preferredDisplayId(props.desktopDisplays, settings.multiScreen.conversationDisplayId) ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, conversationDisplayId: value || undefined } })} options={displayOptions} /></Field>
            <Field title="立绘所在屏幕" detail="立绘和头顶气泡只挂载到选中的屏幕。"><Choice label="立绘所在屏幕" value={preferredDisplayId(props.desktopDisplays, settings.multiScreen.portraitDisplayId) ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, portraitDisplayId: value || undefined } })} options={displayOptions} /></Field>
          </>}
          <div className="integration-actions"><button className="settings-action secondary" onClick={() => void props.onRefreshDesktopDisplays()}>刷新显示器检测</button></div>
        </Card>}
        <Card title="会话生命周期"><Field title="新会话策略" detail="网页模式会固定到同一个 DeepSeek 会话地址；其他后端分别保留自己的最近会话。"><Choice label="新会话策略" value={settings.conversationPolicy} onChange={(value) => set({ conversationPolicy: value as WallpaperSettings['conversationPolicy'] })} options={[{ value: 'resume-last', label: '恢复最近会话' }, { value: 'new-on-unlock', label: '每次解锁新建' }, { value: 'daily', label: '每日新建' }]} /></Field></Card>
        <Card title="高级外观" description="环境渐变只作用于立绘；会话窗使用独立的亚克力透明度。">
          <Field title="环境渐变长度" detail={`从暗侧向亮侧延伸至 ${settings.portraitAmbientLength}%`}><input type="range" min="35" max="100" step="1" value={settings.portraitAmbientLength} onChange={(event) => set({ portraitAmbientLength: Number(event.target.value) })} /></Field>
          <Field title="环境渐变强度" detail={`${Math.round(settings.portraitAmbientStrength * 100)}%`}><input type="range" min="0" max="1" step="0.01" value={settings.portraitAmbientStrength} onChange={(event) => set({ portraitAmbientStrength: Number(event.target.value) })} /></Field>
          <Field title="中央会话窗透明度" detail={`${Math.round(settings.conversationOpacity * 100)}% · 仅影响亚克力底色，不影响文字可读性`}><input type="range" min="0.2" max="0.96" step="0.01" value={settings.conversationOpacity} onChange={(event) => set({ conversationOpacity: Number(event.target.value) })} /></Field>
          <Field title="中央会话窗磨砂" detail={`${settings.conversationBlur}px · 0 为纯透明玻璃，数值越高背景越柔和`}><input type="range" min="0" max="40" step="1" value={settings.conversationBlur} onChange={(event) => set({ conversationBlur: Number(event.target.value) })} /></Field>
        </Card>
      </>}

      {page === 'connections' && <>
        <Card title="聊天模式" description="三选一。改的是**正在运行**的那个壁纸，同时也记作下次启动的默认值；网页桥接不会在失败时自动切到付费 API。">
          <Field title="当前使用" detail={props.liveBackend && props.liveBackend !== settings.defaultBackend ? `壁纸此刻在用：${backendModeLabel(props.liveBackend)}（与默认值不同，可能刚被托盘或自动切换改过；此窗口打开时读取）。Harness 不在这里切换。` : '立即切换正在运行的壁纸；此窗口打开时读取它现在用哪一种。Harness 由桌面上的那个开关切换。'}>
            {/* 值是**运行中**的那个后端，不是启动默认值：两者会分叉（托盘换后端、
                `autoSwitchHarness` 自动切到 Harness、主体退出后壁纸自己复位），显示事实而不是意图。
                当前值是 Harness 时它也留在候选里（标注"由桌面开关切换"），否则这个控件会显示成
                网页入口而壁纸其实在 Harness 上。 */}
            <Choice label="当前使用" value={props.liveBackend ?? settings.defaultBackend} onChange={(value) => props.onSelectBackend(value as BackendMode)} options={chatModeOptions(props.liveBackend ?? settings.defaultBackend)} />
          </Field>
          <Field title="DSH 就绪时自动切换" detail="仅检测到兼容的壁纸 Bridge 才会切换。"><Toggle label="DSH 自动切换" checked={settings.autoSwitchHarness} onChange={(value) => set({ autoSwitchHarness: value })} /></Field>
        </Card>
                {/*
          Wording rules for this card, applied to every sentence in it:
          * name what the user is choosing between, never how it is built — "客户端 /
            源码目录", not the design's "自带检出 / 执行主体", which stay internal;
          * every control says what it will *do*, including that it may start
            something. The text below used to promise "不会替你启动它", which stopped
            being true when 「拉起 UI」 gained the right to start a subject: a promise the
            code no longer keeps must not survive in a sentence;
          * no port, no Bridge state, no filesystem path, and no step the user cannot
            take themselves;
          * one name per thing. The autostart toggle used to call the same setting both
            "壁纸自身开机自启" and "你的系统自启设置"; the page has one name for it, so
            the toggle points at that page instead of introducing a second.
        */}
        <Card title="DeepSeek Harness 启动" description="选择由谁来跑 DeepSeek Harness：客户端自带运行环境，源码目录由本应用启动。已经在运行的实例不会被接管或关闭。">
          {/*
            One control for "who runs it", because that is one question. The scanned
            subjects used to be a row each with its own 采用 button, plus a separate row
            prompting for a default when several source trees exist; a select answers all
            of it in one line, and that prompt becomes this line's description. There is
            deliberately no manual path entry: fewer items was the request, and typing a
            directory is discovery work the scan already does.
          */}
          <Field
            title="运行方式"
            detail={props.dshScanBusy
              ? '正在后台搜索可识别的运行方式，请稍候。'
              : props.harnessTargets.length === 0
                ? '点「扫描」找出本机可以运行的 DeepSeek Harness；扫描不会阻塞设置中心。'
                : [props.subjectChoice, `已发现 ${props.harnessTargets.length} 个可选项${props.subjectCatalogVerifiedAt ? `（${catalogAgeLabel(props.subjectCatalogVerifiedAt)}）` : ''}。`].filter(Boolean).join(' ')}
          >
            <span className="integration-actions">
              {props.harnessTargets.length > 0 && (
                <select
                  className="settings-select"
                  aria-label="运行方式"
                  value={settings.dshLaunch.subjectId ?? ''}
                  onChange={(event) => props.onSelectSubject(event.target.value)}
                >
                  {/*
                    A subject stored before this list was rescanned stays visible:
                    dropping the user's choice because a scan has not run yet is the
                    "looks empty" failure the catalogue exists to prevent.
                  */}
                  {settings.dshLaunch.subjectId && !props.harnessTargets.some((target) => sameSubject(target.id, settings.dshLaunch.subjectId)) && (
                    <option value={settings.dshLaunch.subjectId}>{`当前：${displaySubjectPath(settings.dshLaunch.subjectId)}`}</option>
                  )}
                  {props.harnessTargets.map((target) => (
                    <option key={target.id} value={target.id}>{subjectOptionLabel(target, props.harnessTargets)}</option>
                  ))}
                </select>
              )}
              <button className="settings-action secondary" disabled={props.dshScanBusy} onClick={props.onScanDsh}>
                {props.dshScanBusy ? '扫描中…' : props.harnessTargets.length > 0 ? '重新扫描' : '扫描'}
              </button>
            </span>
          </Field>
          {!shellSelected && <>
            {/*
              These three belong to a source directory only, which is why the block
              disappears for a client. The directory itself is read-only: with no manual
              entry the scan is what fills the list, so an editable field would be an
              input with no effect.
            */}
            <Field title="源码目录" detail="这份源码的位置；扫描会用它作为下一次查找的提示路径。"><span>{displaySubjectPath(selectedSubject?.identity.rootPath ?? settings.dshLaunch.rootPath)}</span></Field>
            <Field title="数据档案（Profile）" detail="这份源码使用的档案名；不同档案的会话互不相通。"><input value={settings.dshLaunch.profile} placeholder="desktop" onChange={(e) => set({ dshLaunch: { ...settings.dshLaunch, profile: e.target.value || 'desktop' } })} /></Field>
            <Field title="启动命令" detail="一般留空即可。只有在需要用别的程序启动它时，才填写那个程序的完整路径（不能带参数）。"><input value={settings.dshLaunch.command ?? ''} placeholder="留空时使用内置的启动方式" onChange={(e) => set({ dshLaunch: { ...settings.dshLaunch, command: e.target.value || undefined, trustedCommandForAutoStart: e.target.value ? settings.dshLaunch.trustedCommandForAutoStart : false } })} /></Field>
          </>}
          {/*
            One action, named for what the user wants (see it), not for the two things it
            does (start it if needed, then show it). 「启动执行主体」 used to sit beside this
            as a peer row: two similar options for one intent, and the ambiguity was in
            the pair rather than in either label.
          */}
          <Field
            title="打开界面"
            detail={!selectedSubject
              ? '先在上面选定运行方式。'
              : shellSelected
                ? '会把它自己的窗口调到前台；如果它没在运行，会先把它启动起来。'
                : '会用它自己的界面（默认浏览器）；如果它没在运行，会先把它启动起来。'}
          >
            <button className="settings-action" disabled={!settings.dshLaunch.subjectId || props.openBusy} onClick={props.onOpenClient}>
              {props.openBusy ? '处理中…' : '打开'}
            </button>
          </Field>
          <Field
            title="随壁纸启动 DSH"
            detail={shellSelected
              ? '壁纸启动时自动把该客户端跑起来（它不支持静默启动时会直接出现窗口）。要让它随登录生效，还需要在「常规」里开启壁纸开机自启。已经在运行的实例不会被接管或重启。'
              : '壁纸启动时自动把该源码目录跑起来。要让它随登录生效，还需要在「常规」里开启壁纸开机自启。已经在运行的实例不会被接管或停止。'}
          >
            <Toggle
              label="随壁纸启动 DSH"
              checked={settings.dshLaunch.autoStartWithWallpaper}
              onChange={(value) => set({ dshLaunch: { ...settings.dshLaunch, autoStartWithWallpaper: value } })}
            />
          </Field>
          {settings.dshLaunch.autoStartWithWallpaper && !props.autostart.enabled && (
            <Field title="壁纸开机自启未生效" detail="开机后自动启动 DSH 依赖壁纸自身的开机自启。">{
              props.autostart.source === 'disabled-by-user'
                ? 'Windows 任务管理器已禁用本应用的自启项，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。'
                : props.autostart.source === 'disabled-by-policy'
                  ? '系统策略禁用了本应用的自启项，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。'
                  : '壁纸自身尚未设置开机自启，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。请在「常规」中开启壁纸自启。'
            }</Field>
          )}
          {settings.dshLaunch.command && settings.dshLaunch.autoStartWithWallpaper && (
            <Field
              title="自动启动不使用自定义启动命令"
              detail="node.exe / pnpm 以外的启动器在无人值守时自动执行需要你明确同意。手动「启动」始终使用该命令。"
            >
              <Toggle
                label="允许自动启动使用该命令"
                checked={settings.dshLaunch.trustedCommandForAutoStart}
                onChange={(value) => set({ dshLaunch: { ...settings.dshLaunch, trustedCommandForAutoStart: value } })}
              />
            </Field>
          )}
                    <Field title="受管进程" detail={props.managedDsh.managed ? '该 DSH 由本应用启动，可以在这里停止它。' : '本应用没有启动 DSH；其他人启动的实例不会被停止。'}><span className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshManagedDsh}>刷新</button><button className="settings-action secondary" disabled={!props.managedDsh.running} onClick={props.onStopManagedDsh}>停止本应用启动的 DSH</button></span></Field>
        </Card>
        <Card title="DeepSeek 网页入口（实验）" description="在应用内持久 WebView2 中打开 DeepSeek 页面，登录后可从桌面会话窗发送消息。"><Field title="页面" detail="页面和登录状态由独立 WebView2 配置目录保存；本应用不读取、复制或记录 Cookie。"><button className="settings-action" onClick={props.onRequestDeepSeekLogin}>打开应用内页面</button></Field><Field title="网页适配器配置" detail={props.deepseekWebAdapterConfig ? `${props.deepseekWebAdapterConfig.source === 'local' ? '本地 override' : '内置默认'} · ${props.deepseekWebAdapterConfig.adapterVersion} · ${props.deepseekWebAdapterConfig.path}${props.deepseekWebAdapterConfig.warning ? ` · ${props.deepseekWebAdapterConfig.warning}` : ''}` : '正在读取配置状态…'}><span className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshDeepSeekWebAdapterConfig}>刷新</button><button className="settings-action secondary" onClick={props.onOpenDeepSeekWebAdapterConfig}>打开配置</button><button className="settings-action secondary" onClick={props.onResetDeepSeekWebAdapterConfig}>恢复默认</button></span></Field></Card>
        <Card title="DeepSeek API" description="API 模式会产生实际费用，密钥只保存在 Windows 凭据管理器。">
          {/* 只深耕 DeepSeek：地址不再暴露成设置项（值仍是默认的官方地址），少一个能填错的地方。
              用户的原话是"API 网址可以省略"。 */}
          <Field
            title="访问密钥"
            detail="在这里填入 DeepSeek API Key，按「测试」确认可用并保存到 Windows 凭据管理器；测试会同时拉取可用模型列表。密钥不支持读回，下面显示的是脱敏后的形态。"
          >
            <span className="api-key-actions">
              <input
                type="password"
                aria-label="DeepSeek API Key"
                placeholder={props.apiKeyStatus?.present ? '填入新的 Key 以替换' : 'sk-…'}
                value={props.apiKeyDraft}
                onChange={(event) => props.onApiKeyDraftChange(event.target.value)}
                onKeyDown={(event) => { if (event.key === 'Enter') props.onTestApiKey() }}
                autoComplete="off"
                spellCheck={false}
              />
              <button className="settings-action secondary" disabled={props.apiKeyBusy} onClick={props.onTestApiKey}>测试</button>
            </span>
          </Field>
          <Field title="已保存" detail={props.apiKeyStatus?.present ? '这是凭据管理器里那一条的脱敏形态。' : '还没有保存过 API Key。'}>
            <span className="api-key-actions">
              <code className="api-key-masked" aria-label="已保存的 API Key（脱敏）">{props.apiKeyStatus?.present ? props.apiKeyStatus.masked ?? '••••' : '未配置'}</code>
              <button className="settings-action secondary" disabled={props.apiKeyBusy} onClick={props.onRefreshApiModels}>刷新</button>
            </span>
          </Field>
          <Field title="模型" detail={props.apiModelCatalog && props.apiModelCatalog.length > 0 ? `可用模型 ${props.apiModelCatalog.length} 项${catalogAgeSuffix(props.apiModelCatalogFetchedAt)}。换了一批名字就按「刷新」。` : '按「刷新」拉取可用模型；拉到的列表会记下来，下次打开设置直接显示。'}>
            {/* 用面板自己的 `Choice`，**不用** `input list` + `datalist` 那套原生下拉：用户实测
                它在设置窗里根本展不开（只有箭头在那儿摆着，点不动）。`Choice` 是本窗口里已经在用
                的下拉（显示器背景、素材用途），展开由自己控制。
                （这里刻意不写出标签原文，测试用"源码里不许出现该标签"来钉住这条。） */}
            <Choice
              label="DeepSeek API 模型"
              value={settings.deepseekApi.model}
              onChange={(model) => set({ deepseekApi: { ...settings.deepseekApi, model } })}
              options={apiModelOptions(props.apiModelCatalog ?? [], settings.deepseekApi.model)}
              emptyMessage={props.apiModelCatalog && props.apiModelCatalog.length > 0 ? undefined : '还没有拉取到模型列表，先按「刷新」。'}
            />
          </Field>
          <Field title="输入价格" detail="人民币／每百万 input tokens。输入、输出价格都配置后，才会显示本轮和会话估算费用。"><PriceInput label="输入价格（人民币每百万 tokens）" value={settings.deepseekApi.priceInputPerMillion} onChange={(priceInputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceInputPerMillion } })} /></Field>
          <Field title="输出价格" detail="人民币／每百万 output tokens。留空不会伪造零费用；缓存 token 没有单独价格时会标为估算。"><PriceInput label="输出价格（人民币每百万 tokens）" value={settings.deepseekApi.priceOutputPerMillion} onChange={(priceOutputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceOutputPerMillion } })} /></Field>
        </Card>
      </>}

      {page === 'appearance' && <>
        <Card title="桌面背景" description="内置背景与你的素材将保持独立。"><div className="background-grid">{BACKGROUND_OPTIONS.map((background) => <button key={background.id} data-background={background.id} className={settings.background === background.id ? 'is-active' : ''} onClick={() => set({ background: background.id })}><span style={background.path ? { backgroundImage: `url(${background.path})` } : undefined} /><strong>{background.name}</strong>{settings.background === background.id && <i>当前</i>}</button>)}</div></Card>
        <Card title="素材库" description="导入的单张素材先选择用途，再出现在对应组件的枚举菜单中。主题包和插件将在后续版本单独处理。">
          <div className="asset-library-toolbar"><button className="settings-action" onClick={props.onImportAppearance} disabled={props.appearanceBusy}>导入图片素材</button><span>{props.appearanceAssets.filter((asset) => asset.status === 'inbox').length} 项待分类 · {props.appearanceAssets.filter((asset) => asset.status === 'classified').length} 项可用</span></div>
          {props.appearanceAssets.filter((asset) => asset.status === 'inbox').length > 0 && <div className="asset-inbox">{props.appearanceAssets.filter((asset) => asset.status === 'inbox').map((asset) => <div className="asset-inbox-row" key={asset.id}><span><strong>{asset.originalName}</strong><small>{asset.width && asset.height ? `${asset.width} × ${asset.height}` : '图片'}{asset.hasAlpha ? ' · 透明背景' : ''}</small></span><Choice label={`${asset.originalName} 的用途`} value="" onChange={(slot) => props.onClassifyAppearance(asset.id, slot as AppearanceSlot)} disabled={props.appearanceBusy} options={[{ value: '', label: '选择用途…' }, ...componentSlots.map(({ slot, label }) => ({ value: slot, label }))]} /></div>)}</div>}
          <div className="asset-component-list">{componentSlots.map(({ slot, label, detail }) => {
            const candidates = props.appearanceAssets.filter((asset) => asset.status === 'classified' && asset.slots.includes(slot))
            const selected = props.appearanceOverrides[slot] ?? ''
            return <div className="asset-component-row" key={slot}><span><strong>{label}</strong><small>{detail} · {candidates.length} 项可选</small></span><Choice label={label} value={selected} onChange={(id) => { if (id) props.onSelectAppearance(slot, id); else props.onClearAppearance(slot) }} disabled={props.appearanceBusy} emptyMessage={candidates.length === 0 ? '暂无此类素材，请先导入并指定用途' : undefined} options={[{ value: '', label: '使用官方默认' }, ...candidates.map((asset) => ({ value: asset.id, label: `${asset.originalName}${asset.width && asset.height ? ` (${asset.width} × ${asset.height})` : ''}` }))]} /></div>
          })}</div>
        </Card>
        <Card title="苏醒动画">
          <Field title="启用动画"><Toggle label="启用苏醒动画" checked={settings.animationsEnabled} onChange={(value) => set({ animationsEnabled: value })} /></Field>
          <Field title="每次解锁播放"><Toggle label="每次解锁播放" checked={settings.playWakeOnEveryUnlock} onChange={(value) => set({ playWakeOnEveryUnlock: value })} /></Field>
          <Field title="跳过苏醒过程"><Toggle label="跳过苏醒过程" checked={settings.skipWakeAnimation} onChange={(value) => set({ skipWakeAnimation: value })} /></Field>
          <Field title="动画速度" detail={`${settings.animationSpeed.toFixed(1)}×`}><input type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(e) => set({ animationSpeed: Number(e.target.value) })} /></Field>
          <Field title="氛围强度"><Choice label="氛围强度" value={settings.animationIntensity} onChange={(value) => set({ animationIntensity: value as WallpaperSettings['animationIntensity'] })} options={[{ value: 'low', label: '克制' }, { value: 'normal', label: '标准' }, { value: 'high', label: '鲜明' }]} /></Field>
        </Card>
      </>}

      {page === 'personas' && <>
        <Card title="人物列表" description="四张正式立绘是固定的后端／模型层级映射。此处只用于审阅；如需替换某张图，请到“外观 → 素材库”为对应槽位指定素材。">
          <OfficialPersonaCards assets={props.appearanceAssets} overrides={props.appearanceOverrides} />
        </Card>
        <Card title="模型与形态映射" description="模型层级决定年龄，思考强度只改变氛围。">
          <div className="rule-list">{settings.modelTierRules.length === 0 && <div className="settings-empty"><strong>尚未创建自定义规则</strong><span>未识别模型会保持当前形态，冷启动默认为 Flash。</span></div>}{settings.modelTierRules.map((rule, index) => <div className="rule-row" key={index}><select aria-label="后端" value={rule.backend} onChange={(e) => updateRule(index, { backend: e.target.value as ModelTierRule['backend'] })}><option value="*">全部后端</option><option value="deepseek-web">DeepSeek Web</option><option value="deepseek-api">DeepSeek API</option><option value="harness">Harness</option></select><select aria-label="匹配方式" value={rule.match} onChange={(e) => updateRule(index, { match: e.target.value as ModelTierRule['match'] })}><option value="exact">精确</option><option value="contains">包含</option><option value="regex">正则</option></select><input aria-label="模型名称" value={rule.pattern} placeholder="模型名称或表达式" onChange={(e) => updateRule(index, { pattern: e.target.value })} /><select aria-label="形态" value={rule.tier} onChange={(e) => updateRule(index, { tier: e.target.value as ModelTierRule['tier'] })}><option value="flash">Flash · 幼年</option><option value="pro">Pro · 成年</option></select><button aria-label="删除规则" onClick={() => set({ modelTierRules: settings.modelTierRules.filter((_, i) => i !== index) })}>×</button></div>)}</div>
          <button className="settings-action secondary add-rule" onClick={() => set({ modelTierRules: [...settings.modelTierRules, { backend: '*', pattern: '', match: 'contains', tier: 'flash' }] })}>＋ 新增映射规则</button>
        </Card>
      </>}

      {page === 'history' && <>
        <Card
          title="API 会话记录"
          description="DeepSeek API 模式的历史记录以当前 Windows 用户的加密档案保存在本机。删除只影响这份档案，不影响 DeepSeek 网页入口或 Harness 会话。"
        >
          <div className="asset-library-toolbar">
            <span>
              共 {props.apiHistory?.conversations.length ?? 0} 个会话 ·{' '}
              {props.apiHistory?.totalMessages ?? 0} 条消息 ·{' '}
              {formatBytes(props.apiHistory?.totalBytes ?? 0)}
              {props.apiHistory && props.apiHistory.budgetBytes > 0
                ? `（预算 ${formatBytes(props.apiHistory.budgetBytes)}，超过后自动淘汰最旧记录）`
                : ''}
            </span>
            <span className="integration-actions">
              <button className="settings-action secondary" disabled={props.apiHistoryBusy} onClick={props.onRefreshApiHistory}>
                {props.apiHistoryBusy ? '读取中…' : '刷新'}
              </button>
              <button
                className="settings-action secondary"
                disabled={props.apiHistoryBusy || (props.apiHistory?.conversations.length ?? 0) === 0}
                onClick={props.onClearApiHistory}
              >
                清空全部 API 历史
              </button>
            </span>
          </div>

          {props.apiHistory && props.apiHistory.budgetBytes > 0 && <div className="history-usage" role="presentation">
            <div className="history-usage__bar"><i style={{ width: `${historyPressure(props.apiHistory.totalBytes, props.apiHistory.budgetBytes)}%` }} /></div>
            <small>
              已用 {historyPressure(props.apiHistory.totalBytes, props.apiHistory.budgetBytes)}% 的应用预算；
              硬上限 {formatBytes(props.apiHistory.maxBytes)}，达到上限时本次运行会停止写入磁盘而不是覆盖已有记录。
            </small>
          </div>}

          {props.apiHistory === undefined && <div className="settings-empty"><strong>正在读取 API 会话记录…</strong><span>首次读取需要解密本机档案。</span></div>}

          {props.apiHistory?.conversations.length === 0 && <div className="settings-empty">
            <strong>没有可删除的 API 会话记录</strong>
            <span>使用 DeepSeek API 模式发送过消息后，这里会出现可管理的会话。</span>
          </div>}

          {(props.apiHistory?.conversations.length ?? 0) > 0 && <div className="history-list">
            {props.apiHistory?.conversations.map((conversation) => <div className="history-row" key={conversation.id}>
              <span className="history-row__main">
                <strong title={conversation.id}>
                  {conversation.id}
                  {conversation.active && <i className="history-row__badge">当前会话</i>}
                </strong>
                <small>
                  {conversation.messageCount} 条消息 · {formatBytes(conversation.bytes)} · 最后活动 {formatHistoryTime(conversation.lastMessageAt)}
                </small>
              </span>
              <button
                className="settings-action secondary history-row__delete"
                disabled={props.apiHistoryBusy}
                aria-label={`删除 API 会话 ${conversation.id}`}
                onClick={() => props.onDeleteApiConversation(conversation.id)}
              >
                删除
              </button>
            </div>)}
          </div>}
        </Card>
      </>}

      {page === 'system' && <>
        <Card title="Windows 集成">
          <Field title="登录后自动启动" detail={props.autostartBusy ? '正在更新 Windows 启动任务，请稍候；设置中心仍可继续使用。' : 'MSIX 优先使用 Windows StartupTask，旧版/开发版回退到当前用户启动项；版本更新会保留此状态。'}><Toggle label="登录后自动启动" checked={settings.autostart} onChange={(value) => set({ autostart: value })} disabled={props.autostartBusy} /></Field>
          <Field title="接管锁屏图片" detail={props.lockScreenBusy ? '正在应用系统锁屏设置，请稍候。' : '使用内置且已审计的熟睡画面；密码界面仍由 Windows 原生安全桌面处理。正式版需要 MSIX 包身份。'}><Toggle label="接管锁屏图片" checked={settings.lockScreenEnabled} onChange={props.onSetLockScreenEnabled} disabled={props.lockScreenBusy} /></Field>
          <div className="lockscreen-diagnostics">
            <div className="lockscreen-diagnostics__row"><div><strong>接管状态</strong><small>{props.lockScreenDiagnostics?.managedImageActive ? '正在使用大肥鱼的熟睡画面' : '未检测到本应用的锁屏图片'}</small></div>{props.lockScreenDiagnostics?.managedImageActive ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRestoreLockScreen}>打开 Windows 锁屏设置</button> : props.lockScreenDiagnostics?.staleBackup ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onClearStaleLockScreenBackup}>{props.lockScreenBusy ? '正在清理…' : '清理旧恢复点（删除原图副本）'}</button> : null}</div>
            <div className="lockscreen-diagnostics__row"><div><strong>接管前检查</strong><small>{props.lockScreenDiagnostics ? props.lockScreenDiagnostics.takeoverAvailable ? 'Windows 与当前应用身份允许尝试设置锁屏图片' : props.lockScreenDiagnostics.supported ? 'Windows 允许，但当前正式版需要 MSIX 包身份' : '当前系统不允许应用修改锁屏图片' : '正在读取系统状态…'}</small></div><button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRefreshLockScreenDiagnostics}>{props.lockScreenBusy ? '正在应用…' : '刷新检查'}</button></div>
            {props.lockScreenDiagnostics && <ul><li>备份：{props.lockScreenDiagnostics.staleBackup ? '已保留，但当前锁屏已被外部更改' : props.lockScreenDiagnostics.backupValid ? '原静态图片可恢复' : props.lockScreenDiagnostics.backupExists ? '备份失效' : '尚未创建（首次接管时保存）'}</li><li>托管睡眠图：{props.lockScreenDiagnostics.managedImageReady ? '已准备' : '首次接管时准备'}</li>{props.lockScreenDiagnostics.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>}
          </div>
          <Field title="睡眠快捷键"><input className="short-input" value={settings.sleepHotkey} onChange={(e) => set({ sleepHotkey: e.target.value })} /></Field>
        </Card>
        <Card title="透明任务栏" description="通过松耦合方式连接独立安装的 TranslucentTB，本应用不会修改其配置。">
          <div className="integration-status"><div><i className={translucentTb.running ? 'is-online' : ''} /><span><strong>{translucentTb.running ? 'TranslucentTB 正在运行' : translucentTb.installed ? 'TranslucentTB 已安装' : 'TranslucentTB 未安装'}</strong><small>{translucentTb.source ?? '由用户独立安装和管理'}</small></span></div><div className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshTranslucentTb}>刷新</button><button className="settings-action" onClick={translucentTb.installed ? props.onLaunchTranslucentTb : props.onInstallTranslucentTb}>{translucentTb.installed ? '启动' : '前往商店'}</button></div></div>
        </Card>
      </>}
    </main>

    <footer className="settings-statusbar"><span>dsh-wallpaper · v0.2.0</span><span><i />设置会自动保存</span></footer>
  </div>
}
