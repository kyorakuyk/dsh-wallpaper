import { openRoutesFor, selectedOpenRoute } from '../connect/openRoutes.ts'
import { formatMessage, formatSentence, msg, sentenceOf, setLanguage, t, useLanguage, type Language, type Message, type MessageKey } from '../i18n/index.ts'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
// `ModelTierRule` 随「模型与形态映射」卡片一起被冻结，解冻时连同上面那行 `updateRule` 一起加回来。
import type { BackendMode } from '../domain/types.ts'
import { BACKGROUND_OPTIONS, MAX_PRICE_PER_MILLION, normalizedPrice, type WallpaperSettings } from './store.ts'
import { type SettingsPage } from './settingsProbes.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
// FREEZE(1A)：锁屏退出，LockScreenDiagnostics 一并冻结（单行 import 列表里不能用 // 注释单项，所以整行注释、旁边写出不含它的版本）。
// import type { DeepSeekWebAdapterConfigStatus, DesktopDisplayInfo, DesktopWorkspaceStatus, LockScreenDiagnostics, ManagedDshStatus, ApiConversationListing, ApiKeyStatus } from '../native/runtime.ts'
import type { DeepSeekWebAdapterConfigStatus, DesktopDisplayInfo, DesktopWorkspaceStatus, ManagedDshStatus, ApiConversationListing, ApiKeyStatus, UpdateCheckReport } from '../native/runtime.ts'
import { downloadPercent, offeredUpdate, updatePhase, type UpdateDownloadState } from '../features/update/updateState.ts'
import { downloadFailedMessage, downloadProgressMessage, updateOutcomeMessage } from '../features/update/updateCopy.ts'
import { preferredDisplayId } from '../runtime/displayLayout.ts'
import { assetUrl } from '../runtime/assets.ts'
import type { AutostartStatus, HarnessEndpointScan, HarnessTarget } from '../native/runtime.ts'
import { autostartDetail, autostartKnown } from './autostartCopy.ts'
import { SettingsIcon, type SettingsIconName } from './SettingsIcon.tsx'
import { SettingsPersonaBadge } from './SettingsPersonaBadge.tsx'
import { DisplayLayoutMap } from './DisplayLayoutMap.tsx'
// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：「起别名」与实例下拉被冻在这个 build 之外，所以它们要的两样东西
// 也一起冻住 —— `instanceLabel` 只给实例下拉的行文字用，`subjectAlias` 只给「起别名」输入框回显用，
// `launchArgsIssue` 只给「启动参数」那行的校验提示用。三个函数本身一行都没动（
// `connect/harnessSubjects.ts` / `connect/launchArgs.ts` 里的纯逻辑与它们的测试照常跑）。
// 怎么恢复：取消注释下面两个 import，再去掉本文件里对应的三处 FREEZE 注释。
// ---------------------------------------------------------------------------
// import { catalogAgeLabel, displaySubjectPath, instanceLabel, sameSubject, subjectAlias, subjectOptionLabel } from '../connect/harnessSubjects.ts'
import { catalogAgeLabel, displaySubjectPath, sameSubject, subjectOptionLabel } from '../connect/harnessSubjects.ts'
// import { launchArgsIssue } from '../connect/launchArgs.ts'
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
   * 「桌面会话」工作区落在哪儿（来自原生只读自检）。
   *
   * 位置规则与桥一致：**壁纸自己的数据目录**下的 `桌面会话`。装成 MSIX 之后这一点尤其要能看见——
   * 安装目录每次升级被整体替换（写那里的必丢），数据目录则升级保留、卸载也留得下。
   */
  desktopWorkspace?: DesktopWorkspaceStatus
  /** 在资源管理器里打开「项目记忆」；桌面会话里不贴路径，入口只在这里。 */
  onOpenProjectMemory?: () => Promise<void>
  openingMemory?: boolean
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
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30），透明任务栏这块随之冻结。恢复办法：取消注释。
  // translucentTb: { installed: boolean; running: boolean; source?: string }
  // onRefreshTranslucentTb: () => void
  // onLaunchTranslucentTb: () => void
  // onInstallTranslucentTb: () => void
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
  /**
   * The §4.3 prompt when several source trees exist, so the user chooses one.
   *
   * 收 `Message`（没求值）而不是字符串：这句话在渲染期求值，切语言时它跟着变。
   */
  subjectChoice?: Message
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
   * desktop shell listens on 19387 and the CLI/webui shape on 3080. The card
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
  /**
   * 「打开」的另一条路线：在一个**新的终端窗口**里拉起 TUI。
   *
   * 只有**没有自己窗口**的主体才会用到它（官方桌面客户端自带窗口 ⇒ 这个问题对它不成立）。
   */
  onOpenTui: () => void
  /** 用户选的路线。存进 `dshLaunch.window`；缺省按浏览器（与旧档案行为一致）。 */
  onSelectWindow: (value: 'browser' | 'tui') => void
  /** 本机有没有 TUI（扫描结果）。没有就不摆出那一项。 */
  tuiAvailable: boolean
  /**
   * FREEZE（临时冻结，不是删除）：「启动参数」的输入框不在这一版里，所以它的三个 prop 与
   * 「起别名」的那一个也一起冻住。
   *
   * 为什么关：本 build 有意回到 a8e2e91 之前的行为 —— 界面上没有「启动参数」行、没有「起别名」
   * 行，也没有标题右上角的实例下拉，启动链不接受任何参数（`App.tsx` / `SettingsWindow.tsx` 里
   * 三个入口都冻结了）。留着一个改了没用的输入框，比它不在更坏。
   *
   * 为什么标成可选而不是删掉：这样 `SettingsWindow` 给不给都不算类型错误，而恢复时两边一起取消
   * 注释就行 —— 这也是 `LayoutProbe` 那套"冻结就注释掉、复活就打开"的做法。
   *
   * 怎么恢复：取消注释这四个 prop，并在 `SettingsWindow.tsx` 里恢复对应的两个 handler 与两处传参。
   */
  // onSelectLaunchArgs: (args: string) => void
  // onSelectSubjectAlias: (alias: string) => void
  // onStopManagedInstance: (instanceKey: string) => void
  /**
   * Which action the current selection takes, for the button label.
   */
  reachAction: 'browser' | 'window'
  /** Opening waits for the client to answer, so the button reports that wait. */
  openBusy: boolean
  managedDsh: ManagedDshStatus
  /** 刷新/停止是否在飞（下拉冻结之后，它仍然钉着底部「停止本应用启动的 DSH」那个按钮）。 */
  managedDshBusy: boolean
  onRefreshManagedDsh: () => void
  /** 停掉本应用启动的**全部**实例（原「停止本应用启动的 DSH」那个动作）。 */
  onStopAllManagedDsh: () => void
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
  // FREEZE(1A)：锁屏退出，这 6 个 prop 随之冻结（见 docs/plans/release-scope-cleanup-plan.md 第一节）。
  // lockScreenDiagnostics?: LockScreenDiagnostics
  // onRefreshLockScreenDiagnostics: () => void
  // onRestoreLockScreen: () => void
  // onClearStaleLockScreenBackup: () => void
  // onSetLockScreenEnabled: (enabled: boolean) => void
  // lockScreenBusy: boolean
  autostartBusy: boolean
  desktopDisplays: DesktopDisplayInfo[]
  onRefreshDesktopDisplays: () => void | Promise<void>
  /** Durable DeepSeek API transcripts, for the history management page. */
  apiHistory?: ApiConversationListing
  apiHistoryBusy: boolean
  onRefreshApiHistory: () => void
  onDeleteApiConversation: (conversationId: string) => void
  onClearApiHistory: () => void
  /**
   * 更新检测（`features/update/`，计划书 §四）：系统页那张卡片。
   *
   * 报告来自原生 `update_check`（只有码与数字），**文案由 `features/update/updateCopy.ts` 说** ——
   * 面板这里只摆字段。`updateNotice` 与气泡下面那行是同一句（忽略没落盘、调用被拒）。
   * `updateDownload` 是那条全局 `update-download` 事件的当下值：下载中、已就绪、还是失败。
   */
  updateReport?: UpdateCheckReport
  updateDownload?: UpdateDownloadState
  updateBusy: boolean
  updateNotice?: Message
  /** 手动检查：不受原生侧 6 小时节流限制。 */
  onCheckForUpdates: () => void
  /** 「下载」/「重试」：交给原生流式下载（§六）。 */
  onDownloadUpdate: () => void
  /** 「点击安装」：把下载好的安装包交给 Windows（§六）。 */
  onInstallUpdate: () => void
  /** 「打开发布页」：资产缺席或下载失败时的回落（§3.1、§六）。 */
  onOpenUpdatePage: () => void
  /** 「忽略」：把这一个版本记进原生状态文件；更晚的版本仍会提示。 */
  onDismissUpdate: (version: string) => void
}

/**
 * 素材库里可以挂单项素材的那几个槽位，以及它们在这个页面上的名字与说明。
 *
 * 存的是**键**而不是文字：语言是启动之后才从设置文档回填的，模块加载时就求值的记录会永远停在
 * 中文（这一条在 `appearanceViewModel.ts` 的 `SLOT_PRESENTATION` 上踩过，那里用的是 getter）。
 * 所以文字在渲染处用 `t(...)` 取，这里只留"哪个槽位、用哪两条键"。
 */
const componentSlots: Array<{ slot: AppearanceSlot; labelKey: MessageKey; detailKey: MessageKey }> = [
  { slot: 'desktop.background', labelKey: 'settings.appearance.component.desktop-background.label', detailKey: 'settings.appearance.component.desktop-background.detail' },
  { slot: 'persona.deepseek.flash', labelKey: 'settings.appearance.component.persona-deepseek-flash.label', detailKey: 'settings.appearance.component.persona-deepseek-flash.detail' },
  { slot: 'persona.deepseek.pro', labelKey: 'settings.appearance.component.persona-deepseek-pro.label', detailKey: 'settings.appearance.component.persona-deepseek-pro.detail' },
  { slot: 'persona.harness.flash', labelKey: 'settings.appearance.component.persona-harness-flash.label', detailKey: 'settings.appearance.component.persona-harness-flash.detail' },
  { slot: 'persona.harness.pro', labelKey: 'settings.appearance.component.persona-harness-pro.label', detailKey: 'settings.appearance.component.persona-harness-pro.detail' },
]

const pages: Array<{ id: Page; icon: SettingsIconName; labelKey: MessageKey; hintKey: MessageKey }> = [
  { id: 'general', icon: 'general', labelKey: 'nav.general.label', hintKey: 'nav.general.hint' },
  { id: 'connections', icon: 'connections', labelKey: 'nav.connections.label', hintKey: 'nav.connections.hint' },
  { id: 'appearance', icon: 'appearance', labelKey: 'nav.appearance.label', hintKey: 'nav.appearance.hint' },
  { id: 'personas', icon: 'personas', labelKey: 'nav.personas.label', hintKey: 'nav.personas.hint' },
  { id: 'history', icon: 'history', labelKey: 'nav.history.label', hintKey: 'nav.history.hint' },
  { id: 'system', icon: 'system', labelKey: 'nav.system.label', hintKey: 'nav.system.hint' },
]

/** `18.4 MB`-style size for the history rows. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${Math.round(bytes)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/**
 * Local timestamp for one history row; `—` when the value is unknown.
 *
 * 同一天那半句是词条（`今天 09:05`），比较早的那些直接写完整日期 —— 那是一串数字格式，不是句子。
 */
export function formatHistoryTime(timestamp: number, now: Date = new Date()): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '—'
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return '—'
  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate()
  const time = `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
  return sameDay
    ? t('settings.history.today', { time })
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

function Card({ title, description, action, children }: { title: string; description?: string; action?: ReactNode; children: ReactNode }) {
  return <section className="settings-card"><header><div className="settings-card__heading"><h2>{title}</h2>{description && <p>{description}</p>}</div>{action && <div className="settings-card__action">{action}</div>}</header><div className="settings-card__body">{children}</div></section>
}

/**
 * 一行设置：左边是文案列（标题 + 说明 + 备注），右边是控件列。
 *
 * `note` 和 `detail` 一样住在**文案列**里；`children` 只放真的控件（开关、下拉、按钮）。
 * 这条界线是踩过一次才划出来的：一句话当 `children` 传进来会落进**控件列**，而控件列按
 * max-content 定宽、不收缩也不折行（`.settings-field__control`），于是那句话横穿卡片、被
 * `.settings-card` 的 `overflow: hidden` 裁掉；唯一能收缩的文案列被挤到只剩一个词宽，
 * 标题与说明都变成一词一行 —— 「壁纸自启未生效」那条警告就是这么坏的（它没有控件，
 * 唯一的内容就是那句话）。没有控件的那一行，请把话交给 `detail` / `note`，别塞进 `children`。
 */
function Field({ title, detail, note, children }: { title: string; detail?: string; note?: string; children?: ReactNode }) {
  return <div className="settings-field"><div className="settings-field__copy"><strong>{title}</strong>{detail && <span>{detail}</span>}{note && <span className="settings-field__note">{note}</span>}</div>{children !== undefined && children !== null && <div className="settings-field__control">{children}</div>}</div>
}

function Toggle({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={`settings-toggle ${checked ? 'is-on' : ''}`} onClick={() => onChange(!checked)}><span /></button>
}

function Segmented({ value, options, onChange, label }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string }) {
  return <div className="settings-segmented" role="radiogroup" aria-label={label}>{options.map((option) =>
    <button type="button" key={option.value} role="radio" aria-checked={option.value === value} className={option.value === value ? 'is-selected' : ''} onClick={() => onChange(option.value)}>{option.label}</button>)}</div>
}

function Choice({ value, options, onChange, label, disabled = false, emptyMessage, emptyLabel }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string; disabled?: boolean; emptyMessage?: string; emptyLabel: string }) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const current = options.find((option) => option.value === value)?.label ?? options[0]?.label ?? emptyLabel
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
    case 'official-desktop': return t('harness.subject.kind.embedded-shell')
    default: return 'Web / CLI'
  }
}

// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：「当前已启动实例」下拉（`RunningInstances`）。
//
// 为什么关：本 build 回到 `a8e2e91` 之前的行为 —— 停止入口只有一个，就是卡片底部那行
// 「本应用启动的 DSH」+「停止本应用启动的 DSH」（那个按钮已经恢复了，见连接卡片底部）。标题
// 右上角的下拉在这个世界里没有第二个实例可列，留着它只会与底部那个按钮做同一件事。
//
// 怎么恢复：把下面这个组件取消注释，恢复连接卡片上的 `action={<RunningInstances … />}`，并在
// `SettingsWindow.tsx` 里恢复 `onStopManagedInstance` 的传参（三处传参各有自己的 FREEZE 注释）；
// 再把底部那行改回注释（它就在那里留档）。原生侧一行都不用动：`managed_dsh_status` 仍然返回
// 列表、`stop_managed_dsh` 仍然接受 instanceKey、`managedDshBusy` 这个 prop 现在钉着底部那个
// 按钮的可用性。
//
// 卡片标题右上角的「当前已启动实例」。
//
// 每一行读作 `别名 · 端口`，行尾的 × 停掉**那一个**实例。同一个源码目录起了两个端口时，这是
// 唯一能分清"我要停的是哪一个"的地方 —— 所以行文字必须带端口，而不是只写一个名字。
//
// 行**不是**可选项：这里的下拉是一个清单，不是单选。点行不做任何事（没有"选中"这个状态），
// 要动就动行尾那个 ×。做成"可选"会让人以为选中它就会改掉「打开界面」的目标，而那条路由
// 「启动参数」里的端口决定（`endpoints.ts`），两个真相来源只会互相打架。
//
// 它复用了 `Choice` 的那套样式类，因为外观该与同一个窗口里的其他下拉一致。
//
// function RunningInstances({ instances, targets, aliases, busy, onStopInstance, onStopAll, onRefresh }: {
//   instances: readonly ManagedDshInstance[]
//   targets: readonly HarnessTarget[]
//   aliases: Readonly<Record<string, string>> | undefined
//   busy: boolean
//   onStopInstance: (instanceKey: string) => void
//   onStopAll: () => void
//   onRefresh: () => void
// }) {
//   const [open, setOpen] = useState(false)
//   const root = useRef<HTMLDivElement>(null)
//   useEffect(() => {
//     const close = (event: MouseEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false) }
//     window.addEventListener('mousedown', close)
//     return () => window.removeEventListener('mousedown', close)
//   }, [])
//   const labelFor = (instance: ManagedDshInstance) => instanceLabel(instance.subjectId, instance.port, targets, aliases)
//   // 只有一行时直接把那一行写在按钮上：用户不必为了读到一个名字而先点开一次。
//   const triggerText = instances.length === 0
//     ? '当前已启动实例（无）'
//     : instances.length === 1
//       ? labelFor(instances[0]!)
//       : `当前已启动实例（${instances.length} 个）`
//   return <div className={`settings-choice settings-instances ${open ? 'is-open' : ''}`} ref={root}>
//     <button
//       type="button"
//       className="settings-choice__trigger"
//       aria-label="当前已启动实例"
//       aria-expanded={open}
//       onClick={() => setOpen((shown) => !shown)}
//     >
//       <span>{triggerText}</span><i>⌄</i>
//     </button>
//     {open && <div className="settings-choice__menu settings-instances__menu" role="list" aria-label="当前已启动实例">
//       {instances.length === 0
//         ? <span className="settings-choice__empty">本应用没有启动 DSH；其他人启动的实例不会被列在这里，也不会被停止。</span>
//         : instances.map((instance) => {
//             const label = labelFor(instance)
//             return <div className="settings-instances__row" role="listitem" key={instance.instanceKey}>
//               <span className="settings-instances__name" title={instance.subjectId}>{label}</span>
//               <button
//                 type="button"
//                 className="settings-instances__stop"
//                 aria-label={`停止实例 ${label}`}
//                 disabled={busy}
//                 onClick={() => onStopInstance(instance.instanceKey)}
//               >
//                 ×
//               </button>
//             </div>
//           })}
//       <div className="settings-instances__footer">
//         <button type="button" className="settings-action secondary" disabled={busy} onClick={onRefresh}>刷新</button>
//       </div>
//     </div>}
//     // 「全部停止」紧挨着下拉，因为它们是同一个动作的两个范围：一个是"停这一个"，一个是"都停"。
//     // 原来卡片底部那个「停止本应用启动的 DSH」按钮已经被它取代 —— 两个控件做同一件事，
//     // 用户就得猜它们有什么区别（而答案曾经是"没有区别"）。
//     <button className="settings-action secondary" disabled={busy || instances.length === 0} onClick={onStopAll}>全部停止</button>
//   </div>
// }
// ---------------------------------------------------------------------------

function displayLabel(display: DesktopDisplayInfo, index: number): string {  const number = /DISPLAY(\d+)/i.exec(display.id)?.[1]
  return number
    ? t('settings.general.display.number', { number })
    : display.name.trim() || t('settings.general.display.fallback', { index: index + 1 })
}

/**
 * 后端 → 显示名。三种都在：提示语与"此刻在用哪一格"仍然会遇到 Harness（托盘、自动切换、
 * 主体退出后的复位都可能把壁纸切过去），缺一个就会退化成内部 id。
 *
 * 三个名字都是 getter：语言是启动后才回填的，写成普通字段就会永远停在 import 时那一种。
 */
export const BACKEND_MODE_LABELS: Record<BackendMode, string> = {
  get 'deepseek-web'() { return backendModeLabel('deepseek-web') },
  get 'deepseek-api'() { return backendModeLabel('deepseek-api') },
  // Harness 那一格是**产品名**：两种语言里都一样，所以它不是词条（见 `backendModeMessage`）。
  get harness() { return 'DeepSeek Harness' },
}

/**
 * 「聊天模式」下拉里的候选：**只有两种聊天后端**。
 *
 * Harness 刻意不在这里（用户要求从这个下拉里删掉它）：它不是"换一个聊天后端"，而是
 * "换一个主体来对话"——连不连、什么时候连，由桌面上的那个开关以及「DSH 就绪时自动切换」管，
 * 在这里再给一个入口只会让两条路互相打架。
 */
export const CHAT_MODE_OPTIONS: Array<{ value: BackendMode; label: string }> = [
  // `label` 也是 getter：这张表在 import 时建好，普通字段会把语言钉死在那一刻（实测：切到
  // 英文后这个下拉里仍是中文），而 `BACKEND_MODE_LABELS` 那三个名字本来就是 getter。
  { value: 'deepseek-web', get label() { return BACKEND_MODE_LABELS['deepseek-web'] } },
  { value: 'deepseek-api', get label() { return BACKEND_MODE_LABELS['deepseek-api'] } },
]

/**
 * 候选 + **当前值**。
 *
 * 当前值可能是 Harness：那时它必须留在候选里并标出来源，否则 `Choice` 会退回第一个候选，
 * 控件显示成"网页入口"而壁纸其实在 Harness 上——一个会骗人的开关。
 */
export function chatModeOptions(): Array<{ value: BackendMode; label: string }> {
  // 这个下拉只回答一件事：**滑槽在左边时，聊天连接哪个通道**。滑槽在右端时一定是 Harness，
  // 而那是滑槽自己的含义 —— 与这里无关，所以候选**恒为两项**，不为运行中的后端追加任何项。
  return [...CHAT_MODE_OPTIONS]
}

/**
 * 同一句的**词条**形态：当别的句子的参数、或要存进状态时用它 —— 渲染期才求值，切语言跟着变。
 *
 * Harness 那一格返回的是**字符串**而不是词条：`DeepSeek Harness` 是产品名，两种语言里一样，
 * 把它塞进字典只会多一条永远相同的词条。`MessageParam` 本来就收字符串，所以它照样能当参数。
 */
export function backendModeMessage(backend: BackendMode): Message | string {
  if (backend === 'deepseek-web') return msg('settings.connections.backend.deepseek-web')
  if (backend === 'deepseek-api') return msg('settings.connections.backend.deepseek-api')
  // 与改动前一样：认不出来的值原样透出，而不是编一个名字。
  return backend === 'harness' ? 'DeepSeek Harness' : backend
}

export function backendModeLabel(backend: BackendMode): string {
  const message = backendModeMessage(backend)
  return typeof message === 'string' ? message : formatMessage(message)
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
  if (minutes < 1) return t('settings.connections.api.model.age.just-now')
  if (minutes < 60) return t('settings.connections.api.model.age.minutes', { minutes })
  const hours = Math.round(minutes / 60)
  if (hours < 48) return t('settings.connections.api.model.age.hours', { hours })
  return t('settings.connections.api.model.age.days', { days: Math.round(hours / 24) })
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
    options.push({ value: trimmed, label: t('settings.connections.api.model.stale', { model: trimmed }) })
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
    placeholder={t('settings.connections.api.price.placeholder')}
    value={value ?? ''}
    onChange={(event) => {
      const raw = event.target.value.trim()
      const parsed = Number(raw)
      onChange(raw === '' ? undefined : normalizedPrice(parsed))
    }}
  />
}

export function SettingsPanel(props: SettingsPanelProps) {
  // FREEZE(1B)：解构里去掉已冻结的 translucentTb。
  // const { settings, harnessStatus, onChange, onClose, translucentTb, page } = props
  const { settings, harnessStatus, onChange, onClose, page } = props
  const [savedAt, setSavedAt] = useState<number>()
  const savedTimer = useRef<ReturnType<typeof setTimeout>>()
  const [selectedDisplayId, setSelectedDisplayId] = useState<string>()
  const [showDisplayNumbers, setShowDisplayNumbers] = useState(false)
  const set = (patch: Partial<WallpaperSettings>) => {
    onChange({ ...settings, ...patch })
    setSavedAt(Date.now())
    if (savedTimer.current !== undefined) clearTimeout(savedTimer.current)
    savedTimer.current = setTimeout(() => setSavedAt(undefined), 1600)
  }
  useEffect(() => () => {
    if (savedTimer.current !== undefined) clearTimeout(savedTimer.current)
  }, [])
  // 语言变了要重渲染：词条是在渲染时取的，所以订阅一下就够。
  const language = useLanguage()
  /** 「清除全部用户数据」的结果（成功后把"删了什么、还剩什么要你手动删"写在这一行里）。 */
  /**
   * 「清空用户数据」那一步的结果：**一组没求值的句子**，不是拼好的一段文字。
   *
   * 结果条数不定（删掉的文件 + 手动处理项），所以它是一组 `Message`，渲染期各自求值、再用同一个
   * 分隔符接起来 —— 存成一段拼好的字符串，语言一换它就留在旧语言里。
   */
  const [clearDetail, setClearDetail] = useState<Message[]>()
  const [clearing, setClearing] = useState(false)
  const clearUserData = async () => {
    // 逐项写清会删什么、要你手动删什么：这类操作不可逆，值得在读清之前先拦一下。
    const confirmed = window.confirm([
      t('settings.system.clear.confirm.intro'),
      t('settings.system.clear.confirm.workspace', { workspace: props.desktopWorkspace?.workspaceDirectory ?? t('settings.system.clear.confirm.workspace-fallback') }),
      t('settings.system.clear.confirm.credentials'),
      '',
      t('settings.system.clear.confirm.manual-heading'),
      t('settings.system.clear.confirm.settings', { data: props.desktopWorkspace?.dataDirectory ?? t('settings.system.clear.confirm.data-fallback') }),
      t('settings.system.clear.confirm.bridge'),
      '',
      t('settings.system.clear.confirm.continue'),
    ].join('\n'))
    if (!confirmed) return
    setClearing(true)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const result = await invoke<{
        removed: string[]
        credentialRemoved: boolean
        manual: Array<{ what: string; path: string }>
      }>('clear_user_data')
      const done = [
        ...result.removed.map((path) => msg('settings.system.clear.removed', { path })),
        result.credentialRemoved ? msg('settings.system.clear.credential-removed') : msg('settings.system.clear.credential-absent'),
      ]
      const manual = result.manual.map((entry) => msg('settings.system.clear.manual', { what: entry.what, path: entry.path }))
      setClearDetail([...done, ...manual])
    } catch (error) {
      setClearDetail([msg('settings.system.clear.failed', { error: sentenceOf(error) })])
    } finally {
      setClearing(false)
    }
  }
  /**
   * The chosen subject, and whether it is the class that carries its own checkout.
   *
   * An unknown or not-yet-chosen id is treated as a checkout: that is what the
   * root-path/profile/launcher fields describe, and it is also the class a
   * profile written by an older version falls back to.
   */
  const selectedSubject = props.harnessTargets.find((target) => target.id === settings.dshLaunch.subjectId)
  const shellSelected = selectedSubject?.kind === 'embedded-shell'
  // 已安装的 CLI 没有"源码目录"这回事：它拿的是别人装好的东西，路径只有它的启动器有意义，
  // 而启动器由扫描决定、不由用户填写。所以给这一类别单独收起那一行，而不是让它显示一个空框。
  const cliSelected = selectedSubject?.kind === 'installed-cli'
  /**
   * FREEZE（临时冻结，不是删除）：「启动参数」那一串现在能不能用。
   *
   * 判据是"启动器会不会收到一个它理解不了的词"，而不是"这串字好不好看"：条数、长度、控制字符。
   * 有意见时那句话**顶掉**用法说明 —— 一行同时说两件事，用户只会读到第一件。
   *
   * 为什么关：这一版没有「启动参数」输入框（见下面那条 Field 的 FREEZE 注释），所以没有东西
   * 可以把校验结果显示出来；一个算了没人看的变量在本项目里会被 `noUnusedLocals` 拦下。
   * 恢复办法：取消注释这一行，并把下面的 Field 一起打开（`launchArgsIssue` 的规则一行都没动）。
   */
  // const argsIssue = launchArgsIssue(settings.dshLaunch.args)
  // 「打开」能做什么，由主体决定：官壳只有自己的窗口，源码树只有浏览器，只有"已安装的 CLI"真的有
  // 两条路可选。**单项不做成下拉** —— 那是一个点了没反应、也无法改变的控件。
  // 这条规则住在 connect/openRoutes.ts：它有单测，包括「没装 TUI 就不给选项」与「卸掉 TUI 之后不回落到死选项」。
  const openRoutes = openRoutesFor({
    hasSubject: Boolean(settings.dshLaunch.subjectId),
    shellSelected,
    subjectKind: selectedSubject?.kind,
    tuiAvailable: props.tuiAvailable,
  })
  // 存着的路线只有在**真的有两条路**时才作数：官壳/源码树即便档案里写着 tui，也仍然走它们唯一的路。
  const openRoute = selectedOpenRoute(openRoutes, settings.dshLaunch.window)
  /**
   * Clients that are actually running. The endpoint picker is only a *choice* when
   * there is more than one: with zero or one, ports and Bridge states are internal
   * bookkeeping the user cannot act on, so showing them would be disclosure without
   * a decision attached to it.
   */
  // 与被冻结的「模型与形态映射」卡片同进退：它唯一的用途就是在那张卡片里改规则。
  // const updateRule = (index: number, patch: Partial<ModelTierRule>) => set({ modelTierRules: settings.modelTierRules.map((rule, i) => i === index ? { ...rule, ...patch } : rule) })
  const pageMeta = pages.find((item) => item.id === page)!
  const displayOptions = props.desktopDisplays.map((display, index) => ({ value: display.id, label: displayLabel(display, index) }))
  const selectedDisplayIdForView = preferredDisplayId(props.desktopDisplays, selectedDisplayId ?? settings.multiScreen.portraitDisplayId) ?? ''
  const selectedDisplayLabel = displayOptions.find((option) => option.value === selectedDisplayIdForView)?.label ?? ''
  const portraitDisplayId = preferredDisplayId(props.desktopDisplays, settings.multiScreen.portraitDisplayId)
  const conversationDisplayId = preferredDisplayId(props.desktopDisplays, settings.multiScreen.conversationDisplayId)
  const backgroundUrlForDisplay = (displayId: string) => {
    const backgroundId = settings.multiScreen.backgrounds[displayId] ?? settings.background
    const path = BACKGROUND_OPTIONS.find((background) => background.id === backgroundId)?.path
    return path ? assetUrl(path) : undefined
  }
  const setDisplayBackground = (displayId: string, value: string) => {
    const backgrounds = { ...settings.multiScreen.backgrounds }
    if (value) backgrounds[displayId] = value as WallpaperSettings['background']
    else delete backgrounds[displayId]
    set({ multiScreen: { ...settings.multiScreen, backgrounds } })
  }
  /**
   * 更新卡片上那几个派生值（计划书 §四）。
   *
   * 都在这儿算，是因为它们**要在渲染期求值**：`updateOutcomeMessage`/`downloadProgressMessage`
   * 返回的是没求值的 `Message`（切语言时整句跟着变），而时间戳那句沿用了历史页那个"当天只写时刻"
   * 的格式。
   *
   * `updateNow` 是**与气泡同一个状态机**（`updatePhase`）：报告给前三个状态、全局的下载事件给
   * 后三个，所以卡片与气泡不会各说各的。
   */
  const updateReport = props.updateReport
  const updateOffer = offeredUpdate(updateReport)
  const updateNow = updatePhase(updateReport, props.updateDownload)
  const updateCurrentVersion = updateReport
    ? updateReport.currentVersion ?? t('settings.system.update.current.unavailable')
    : t('settings.system.reading')
  const updateLastChecked = updateReport?.checkedAtMs
    ? formatHistoryTime(updateReport.checkedAtMs)
    : t('settings.system.update.last-check.never')

  const backend = props.liveBackend ?? settings.defaultBackend
  const themeClass = backend === 'harness' ? 'settings-app--harness' : 'settings-app--deepseek'

  return <div className={`settings-app ${themeClass}`}>
    <header className="settings-titlebar">
      <div className="settings-titlebar__drag" aria-hidden="true" onMouseDown={(event) => {
        if (event.button === 0) void invoke('start_settings_drag')
      }} />
      <div className="settings-brand"><div><strong>Wallpaper</strong><small>{t('settings.brand.subtitle')}</small></div></div>
      <button className="settings-window-close" aria-label={t('settings.window.close')} onClick={onClose}><SettingsIcon name="close" /></button>
    </header>

    <aside className="settings-sidebar">
      <SettingsPersonaBadge backend={backend} backgroundId={settings.background} harnessStatus={harnessStatus} />
      <nav>{pages.map((item) => <button key={item.id} className={page === item.id ? 'is-active' : ''} onClick={() => props.onPageChange(item.id)}><span className="settings-nav__icon"><SettingsIcon name={item.icon} /></span><span><strong>{t(item.labelKey)}</strong></span></button>)}</nav>
    </aside>

    <main className="settings-content">
      <div className="settings-page-heading"><span>{t('settings.page.heading', { page: t(pageMeta.labelKey) })}</span><h1>{t(pageMeta.labelKey)}</h1><p>{t(pageMeta.hintKey)}</p></div>

      {page === 'general' && <>
        <Card title={t('settings.general.interaction.title')} description={t('settings.general.interaction.description')}>
          <Field title={t('settings.general.conversation-window.title')} detail={t('settings.general.conversation-window.detail')}><Toggle label={t('settings.general.conversation-window.toggle')} checked={props.interactionEnabled} onChange={props.onSetInteractionEnabled} /></Field>
          <Field title={t('settings.general.bubble-layout.title')} detail={t('settings.general.bubble-layout.detail')}><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.bubble-layout.title')} value={settings.interactionLayout} onChange={(value) => set({ interactionLayout: value as WallpaperSettings['interactionLayout'] })} options={[{ value: 'floating', label: t('settings.general.bubble-layout.floating') }, { value: 'taskbar-docked', label: t('settings.general.bubble-layout.taskbar-docked') }]} /></Field>
          <Field title={t('settings.general.history-drawer.title')} detail={t('settings.general.history-drawer.detail')}><Toggle label={t('settings.general.history-drawer.toggle')} checked={settings.historyStartsExpanded} onChange={(value) => set({ historyStartsExpanded: value })} /></Field>
          <Field title={t('settings.general.shortcut.title')} detail={t('settings.general.shortcut.detail')}><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.shortcut.title')} value={settings.sendShortcut} onChange={(value) => set({ sendShortcut: value as WallpaperSettings['sendShortcut'] })} options={[{ value: 'Enter', label: t('settings.general.shortcut.enter') }, { value: 'Ctrl+Enter', label: t('settings.general.shortcut.ctrl-enter') }]} /></Field>
        </Card>
        <Card title={t('language.card')}>
          <Field title={t('language.label')} detail={t('language.hint')}>
            <Choice emptyLabel={t('settings.choice.empty')} label={t('language.label')} value={language} options={[{ value: 'zh', label: t('language.zh') }, { value: 'en', label: t('language.en') }]} onChange={(value) => { const next = value as Language; set({ language: next }); setLanguage(next) }} />
          </Field>
        </Card>
        {props.desktopDisplays.length > 1 && <Card title={t('settings.general.multi-screen.title', { count: props.desktopDisplays.length })} description={t('settings.general.multi-screen.description')}>
          <Field title={t('settings.general.multi-screen.toggle')} detail={settings.multiScreen.enabled ? t('settings.general.multi-screen.on') : t('settings.general.multi-screen.off')}><Toggle label={t('settings.general.multi-screen.toggle')} checked={settings.multiScreen.enabled} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, enabled: value } })} /></Field>
          {settings.multiScreen.enabled && <>
            <DisplayLayoutMap displays={props.desktopDisplays} labels={displayOptions.map((option) => option.label)} backgroundUrlFor={backgroundUrlForDisplay} portraitDisplayId={portraitDisplayId} conversationDisplayId={conversationDisplayId} selectedId={selectedDisplayIdForView} onSelect={setSelectedDisplayId} showNumbers={showDisplayNumbers} onToggleNumbers={() => setShowDisplayNumbers((shown) => !shown)} />
            <Field title={t('settings.general.display-map.background-of', { display: selectedDisplayLabel })} detail={t('settings.general.display-map.background-hint')}>
              <div className="settings-display-background-grid" role="group" aria-label={t('settings.general.display-map.background-of', { display: selectedDisplayLabel })}>
                <button type="button" className={`settings-display-background__follow ${!settings.multiScreen.backgrounds[selectedDisplayIdForView] ? 'is-active' : ''}`} aria-pressed={!settings.multiScreen.backgrounds[selectedDisplayIdForView]} onClick={() => setDisplayBackground(selectedDisplayIdForView, '')}>
                  <span><SettingsIcon name="check" size={16} /></span>
                  <strong>{t('settings.general.display-map.follow-global')}</strong>
                </button>
                {BACKGROUND_OPTIONS.map((background) => <button type="button" key={background.id} data-background={background.id} className={settings.multiScreen.backgrounds[selectedDisplayIdForView] === background.id ? 'is-active' : ''} aria-pressed={settings.multiScreen.backgrounds[selectedDisplayIdForView] === background.id} onClick={() => setDisplayBackground(selectedDisplayIdForView, background.id)}>
                  <span style={background.path ? { backgroundImage: `url(${assetUrl(background.path)})` } : undefined} />
                  <strong>{background.name}</strong>
                </button>)}
              </div>
            </Field>
            <Field title={t('settings.general.display.portrait.title')} detail={t('settings.general.display.portrait.detail')}>
              {props.desktopDisplays.length <= 4
                ? <Segmented label={t('settings.general.display.portrait.title')} value={portraitDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, portraitDisplayId: value || undefined } })} options={displayOptions} />
                : <Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.display.portrait.title')} value={portraitDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, portraitDisplayId: value || undefined } })} options={displayOptions} />}
            </Field>
            <Field title={t('settings.general.display.conversation.title')} detail={t('settings.general.display.conversation.detail')}>
              {props.desktopDisplays.length <= 4
                ? <Segmented label={t('settings.general.display.conversation.title')} value={conversationDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, conversationDisplayId: value || undefined } })} options={displayOptions} />
                : <Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.display.conversation.title')} value={conversationDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, conversationDisplayId: value || undefined } })} options={displayOptions} />}
            </Field>
          </>}
          <div className="integration-actions"><button className="settings-action secondary" onClick={() => void props.onRefreshDesktopDisplays()}>{t('settings.general.display.refresh')}</button></div>
        </Card>}
        <Card title={t('settings.general.conversation-lifecycle.title')}><Field title={t('settings.general.conversation-policy.title')} detail={t('settings.general.conversation-policy.detail')}><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.conversation-policy.title')} value={settings.conversationPolicy} onChange={(value) => set({ conversationPolicy: value as WallpaperSettings['conversationPolicy'] })} options={[{ value: 'resume-last', label: t('settings.general.conversation-policy.resume-last') }, { value: 'new-on-unlock', label: t('settings.general.conversation-policy.new-on-unlock') }, { value: 'daily', label: t('settings.general.conversation-policy.daily') }]} /></Field></Card>
        <Card title={t('settings.general.advanced.title')} description={t('settings.general.advanced.description')}>
          <Field title={t('settings.general.ambient-length.title')} detail={t('settings.general.ambient-length.detail', { percent: settings.portraitAmbientLength })}><input type="range" min="35" max="100" step="1" value={settings.portraitAmbientLength} onChange={(event) => set({ portraitAmbientLength: Number(event.target.value) })} /></Field>
          <Field title={t('settings.general.ambient-strength.title')} detail={`${Math.round(settings.portraitAmbientStrength * 100)}%`}><input type="range" min="0" max="1" step="0.01" value={settings.portraitAmbientStrength} onChange={(event) => set({ portraitAmbientStrength: Number(event.target.value) })} /></Field>
          <Field title={t('settings.general.conversation-opacity.title')} detail={t('settings.general.conversation-opacity.detail', { percent: Math.round(settings.conversationOpacity * 100) })}><input type="range" min="0.2" max="0.96" step="0.01" value={settings.conversationOpacity} onChange={(event) => set({ conversationOpacity: Number(event.target.value) })} /></Field>
          <Field title={t('settings.general.conversation-blur.title')} detail={t('settings.general.conversation-blur.detail', { pixels: settings.conversationBlur })}><input type="range" min="0" max="40" step="1" value={settings.conversationBlur} onChange={(event) => set({ conversationBlur: Number(event.target.value) })} /></Field>
        </Card>
      </>}

      {page === 'connections' && <>
        <Card title={t('settings.connections.chat-mode.title')} description={t('settings.connections.chat-mode.description')}>
          <Field title={t('settings.connections.chat-mode.field-title')} detail={t('settings.connections.chat-mode.field-detail')}>
            {/* 值是**启动默认值**：这个控件给"滑槽在左边"这个位置赋予含义。滑槽在右端时一定是
                Harness，那由滑槽自己表达，这里不描述、也不需要一个不可选但可见的 Harness 项。 */}
            <Choice emptyLabel={t('settings.choice.empty')} label={t('settings.connections.chat-mode.field-title')} value={settings.defaultBackend} onChange={(value) => props.onSelectBackend(value as BackendMode)} options={chatModeOptions()} />
          </Field>
          <Field title={t('settings.connections.chat-mode.auto-switch.title')} detail={t('settings.connections.chat-mode.auto-switch.detail')}><Toggle label={t('settings.connections.chat-mode.auto-switch.toggle')} checked={settings.autoSwitchHarness} onChange={(value) => set({ autoSwitchHarness: value })} /></Field>
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
        <Card
          title={t('settings.connections.harness.title')}
          description={t('settings.connections.harness.description')}
          // FREEZE（临时冻结，不是删除）：卡片标题右上角的「当前已启动实例」下拉。它随
          // 「启动参数」一起冻住（本 build 回到单实例行为，停止入口只有下面那行）。恢复办法：
          // 取消注释下面这个 `action`，并恢复 `RunningInstances` 组件（本文件内，注释里留着）
          // 与 `SettingsWindow.tsx` 里那两处传参。
          // action={<RunningInstances
          //   instances={props.managedDsh.instances}
          //   targets={props.harnessTargets}
          //   aliases={settings.dshLaunch.aliases}
          //   busy={props.managedDshBusy}
          //   onStopInstance={props.onStopManagedInstance}
          //   onStopAll={props.onStopAllManagedDsh}
          //   onRefresh={props.onRefreshManagedDsh}
          // />}
        >
          {/* 旧的卡片描述留档（不再显示）：选择由谁来接管复杂工作：客户端自带运行环境，源码目录由本应用启动。
              已经在运行的实例不会被接管或关闭。 */}
          {/*
            One control for "who runs it", because that is one question. The scanned
            subjects used to be a row each with its own 采用 button, plus a separate row
            prompting for a default when several source trees exist; a select answers all
            of it in one line, and that prompt becomes this line's description. There is
            deliberately no manual path entry: fewer items was the request, and typing a
            directory is discovery work the scan already does.
          */}
          <Field
            title={t('settings.connections.subject.title')}
            detail={props.dshScanBusy
              ? t('settings.connections.subject.scanning')
              : props.harnessTargets.length === 0
                ? t('settings.connections.subject.none')
                : [formatSentence(props.subjectChoice), t('settings.connections.subject.found', { count: props.harnessTargets.length, age: props.subjectCatalogVerifiedAt ? `（${catalogAgeLabel(props.subjectCatalogVerifiedAt)}）` : '' })].filter(Boolean).join(' ')}
          >
            <span className="integration-actions">
              {props.harnessTargets.length > 0 && (
                <Choice
                  emptyLabel={t('settings.choice.empty')}
                  label={t('settings.connections.subject.title')}
                  value={settings.dshLaunch.subjectId ?? ''}
                  onChange={props.onSelectSubject}
                  options={[
                    /*
                      A subject stored before this list was rescanned stays visible:
                      dropping the user's choice because a scan has not run yet is the
                      "looks empty" failure the catalogue exists to prevent.
                    */
                    ...(settings.dshLaunch.subjectId && !props.harnessTargets.some((target) => sameSubject(target.id, settings.dshLaunch.subjectId))
                      ? [{ value: settings.dshLaunch.subjectId, label: t('settings.connections.subject.current', { path: displaySubjectPath(settings.dshLaunch.subjectId) }) }]
                      : []),
                    // FREEZE（临时冻结，不是删除）：这一行原来把别名表传进去（`subjectOptionLabel(
                    // target, props.harnessTargets, settings.dshLaunch.aliases)`）。不传就是"用目录名
                    // 区分两个同名克隆"——也就是这个功能之前的行文字（`源码目录 · DeepSeekHarness.old
                    // · 0.1.0-rc.5`）。恢复办法：把第三个实参加回去（`subjectOptionLabel` 的别名规则
                    // 一行都没动，`subjectOptionVersion.spec.ts` 仍然钉着它）。
                    ...props.harnessTargets.map((target) => ({ value: target.id, label: subjectOptionLabel(target, props.harnessTargets) })),
                  ]}
                />
              )}
              <button className="settings-action secondary" disabled={props.dshScanBusy} onClick={props.onScanDsh}>
                {props.dshScanBusy ? t('settings.connections.subject.scanning-button') : props.harnessTargets.length > 0 ? t('settings.connections.subject.rescan') : t('settings.connections.subject.scan')}
              </button>
            </span>
          </Field>
          {!shellSelected && <>
            {/*
              这几项属于**源码目录**，所以选中客户端时整块消失；选中已安装的 CLI 时只有"源码目录"
              一行消失（其余几项对它仍然有意义：档案名、启动参数）。目录本身是只读的：没有手工
              填写的地方，列表由扫描填，可编辑的字段会是一个改了也没用的输入框。
              关于「启动参数」：它取代了原来的「启动命令」——不是换了措辞，而是**收掉了一项能力**
              （原来的框里可以填任意一个程序、由壁纸去执行它；「启动参数」只能往我们自己选定的那个
              启动器后面加词，于是"自动启动要不要用这个自定义命令"那一次授权也不再需要）。本 build
              把这一项整个冻住，见下面那个输入框上的 FREEZE 注释。
            */}
            {!cliSelected && <Field title={t('settings.connections.root-path.title')} detail={t('settings.connections.root-path.detail')}><span className="settings-static">{displaySubjectPath(selectedSubject?.identity.rootPath ?? settings.dshLaunch.rootPath)}</span></Field>}
            {/* FREEZE（临时冻结，不是删除）：「起别名」输入框。
                为什么关：别名唯一的作用就是顶替「运行方式」和实例下拉里的"上级目录名"那一段，而
                那个下拉现在按目录名走、实例下拉整个冻住了 —— 留着这个框就是多一个改了也不影响
                任何显示的输入框。别名表本身没删：`dshLaunch.aliases` 与它的归一化测试照常跑，
                写进设置里的值只是暂时不显示。
                怎么恢复：取消下面这个 Field 的注释，并恢复 `props.onSelectSubjectAlias` 那个 prop
                与它在本文件顶部的 `subjectAlias` import。 */}
            {/*
            {!cliSelected && <Field
              title="起别名"
              detail={`只在「运行方式」和实例下拉里显示，留空就用目录名（${selectedSubject?.label ?? '目录名'}）。两个同名目录原本靠上一级目录区分，别名会顶替那一段。`}
            >
              <input
                value={subjectAlias(settings.dshLaunch.subjectId, settings.dshLaunch.aliases)}
                placeholder="留空时使用目录名"
                aria-label="起别名"
                maxLength={64}
                onChange={(e) => props.onSelectSubjectAlias(e.target.value)}
              />
            </Field>}
            */}
            {/* 由用户决定隐藏（2026-09-30）：它不是偏好，而是我们走哪条启动链的结果 —— 官壳用自己独占的 desktop、TUI 用它自己的 dsh-tui、CLI 与检出只认能提供 HTTP 的 web。理由见 connect/harnessProfiles.ts。 */}
            {/* FREEZE（临时冻结，不是删除）：「启动参数」行。
                为什么关：本 build 有意回到这个功能之前的行为 —— 没有参数这一项，启动链也不接受
                参数（`App.tsx` 里三个入口、`SettingsWindow.tsx` 里两个入口都冻结了）。界面上留着
                一个改了没用的框，比它不在更坏；而它旁边的说明还在讲端口与并行实例，那些话在
                单实例的世界里只会让人以为改得动。
                怎么恢复：取消下面这个 Field 的注释，并恢复 `props.onSelectLaunchArgs` 那个 prop、
                本文件顶部的 `launchArgsIssue` import，以及下面那条 `argsIssue` 计算
                （`connect/launchArgs.ts` 本身一行都没动，分词与读端口的测试照常跑）。
                校验逻辑的落点也留档在这里：`launchArgsIssue(settings.dshLaunch.args)`。 */}
            {/*
            <Field
              title="启动参数"
              detail={launchArgsIssue(settings.dshLaunch.args) ?? `追加到启动器后面的参数，例如 --port 3081。留空就用默认端口；参数按你写的原样传递，不经过命令行解释器（引号只在这里解释一次）。TUI 没有端口概念。目前支持的组合是官方桌面客户端加一个实例；换端口不隔离会话与工作区（隔离单位是 DSH_HOME，不是端口），再起第二个实例会与它共用同一份会话与工作区记录。`}
            >
              <input
                value={settings.dshLaunch.args ?? ''}
                placeholder="留空时使用默认启动方式"
                aria-label="启动参数"
                maxLength={512}
                onChange={(e) => props.onSelectLaunchArgs(e.target.value)}
              />
            </Field>
            */}
          </>}
          {/*
            One action, named for what the user wants (see it), not for the two things it
            does (start it if needed, then show it). 「启动执行主体」 used to sit beside this
            as a peer row: two similar options for one intent, and the ambiguity was in
            the pair rather than in either label.
          */}
          <Field
            title={t('settings.connections.open.title')}
            detail={!selectedSubject
              ? t('settings.connections.open.detail.no-subject')
              : shellSelected
                ? t('settings.connections.open.detail.shell')
                : openRoutes.length > 1
                  ? t('settings.connections.open.detail.multi')
                  : t('settings.connections.open.detail.single')}
          >
            {/* 两条路才给下拉；只有一条路就写一行字，而不是做一个改不动、点了也没反应的控件。 */}
            {openRoutes.length > 1
              ? (
                  <Choice
                    emptyLabel={t('settings.choice.empty')}
                    label={t('settings.connections.open.window-label')}
                    value={openRoute}
                    onChange={(value) => props.onSelectWindow(value === 'tui' ? 'tui' : 'browser')}
                    options={openRoutes}
                  />
                )
              : openRoutes.length === 1
                ? <span className="settings-static">{openRoutes[0]!.label}</span>
                : null}
            <button className="settings-action" disabled={!settings.dshLaunch.subjectId || props.openBusy} onClick={openRoute === 'tui' ? props.onOpenTui : props.onOpenClient}>
              {props.openBusy ? t('settings.connections.open.busy') : t('settings.connections.open.action')}
            </button>
          </Field>
          <Field
            title={t('settings.connections.launch-with-wallpaper.title')}
            detail={shellSelected
              ? t('settings.connections.launch-with-wallpaper.shell')
              : t('settings.connections.launch-with-wallpaper.checkout')}
          >
            <Toggle
              label={t('settings.connections.launch-with-wallpaper.title')}
              checked={settings.dshLaunch.autoStartWithWallpaper}
              onChange={(value) => set({ dshLaunch: { ...settings.dshLaunch, autoStartWithWallpaper: value } })}
            />
          </Field>
          {/* 未读到不等于没开：状态由「系统」页的 probe 读取，占位值的 enabled 也是 false，
              所以这条警告只在确实读到是关的时候出现，否则它会为开着的自启报错。 */}
          {/* 这条警告**没有控件**：那句话是文案，不是控件 —— 走 `note` 落在文案列里。
              以前它是 `children`（见 `Field` 的注释），于是落进右边不收缩也不折行的控件列，
              把左边的标题与说明挤成一词一行，自己还横穿卡片被裁掉。 */}
          {settings.dshLaunch.autoStartWithWallpaper && autostartKnown(props.autostart) && !props.autostart.enabled && (
            <Field
              title={t('settings.connections.autostart-warning.title')}
              detail={t('settings.connections.autostart-warning.detail')}
              note={props.autostart.source === 'disabled-by-user'
                ? t('settings.connections.autostart-warning.disabled-by-user')
                : props.autostart.source === 'disabled-by-policy'
                  ? t('settings.connections.autostart-warning.disabled-by-policy')
                  : t('settings.connections.autostart-warning.not-configured')}
            />
          )}
          {/* 官壳不显示这一项（用户要求）：它的退出方式是托盘菜单，用户手里本来就有；
              而停它会当场关掉用户自己的客户端、并弹一条"宿主意外退出"的报错框 —— 与其预告这个
              后果，不如不给这个入口。另外两类（源码目录、已安装的 CLI）没有托盘也没有窗口，
              要停只能开终端敲命令，摩擦大得多，所以它们照常显示。
              这一行是**恢复**回来的：`a8e2e91` 把它换成了标题右上角的下拉 +「全部停止」，而那个
              下拉（以及并行实例本身）在这个 build 里冻住了 —— 不恢复它，就再没有任何地方能停掉
              本应用启动的实例。动作与从前逐字相同：`stop_managed_dsh` 不给 instanceKey 就是
              "停全部"，也就是 `App.tsx` / `SettingsWindow.tsx` 里 `onStopAllManagedDsh` 那一个。
              用 `managedDshBusy` 挡住重复点击（停止要起 taskkill 并等它结束）。 */}
          {!shellSelected && <Field title={t('settings.connections.managed.title')} detail={props.managedDsh.managed
            ? t('settings.connections.managed.yes')
            : t('settings.connections.managed.no')}><span className="integration-actions"><button className="settings-action secondary" disabled={props.managedDshBusy} onClick={props.onRefreshManagedDsh}>{t('settings.refresh')}</button>{/* 启用条件跟**是不是本应用启动的**走，不跟"有没有在跑"走：只要 3080 上有别的东西在跑，
              旧写法就会点亮一个点了没反应的按钮（实测：装机重启后壁纸丢了"这是我的孩子"的记录）。 */}
            <button className="settings-action secondary" disabled={!props.managedDsh.managed || props.managedDshBusy} onClick={props.onStopAllManagedDsh}>{t('settings.connections.managed.stop')}</button></span></Field>}
        </Card>
        <Card title={t('settings.connections.web.title')} description={t('settings.connections.web.description')}><Field title={t('settings.connections.web.page.title')} detail={t('settings.connections.web.page.detail')}><button className="settings-action" onClick={props.onRequestDeepSeekLogin}>{t('settings.connections.web.page.open')}</button></Field>{/* 适配器那行的说明里**不再显示本地 override 的文件路径**（用户要求）：那是一串
            `%APPDATA%\com.dsh.wallpaper\deepseek-web-adapter.override.json`，对"网页结构变了才需要动它"
            这件事没有任何帮助，只会把一行说明压成三行。路径仍然在原生侧读得到
            （`deepseekWebAdapterConfig.path`），「打开配置」按钮就是照着它打开文件的 —— 需要它的人
            按那个按钮，不需要它的人不必看见。 */}
        <Field title={t('settings.connections.web.adapter.title')} detail={props.deepseekWebAdapterConfig ? `${t('settings.connections.web.adapter.detail')}${props.deepseekWebAdapterConfig.source === 'local' ? t('settings.connections.web.adapter.source-local') : t('settings.connections.web.adapter.source-bundled')} · ${props.deepseekWebAdapterConfig.adapterVersion}${props.deepseekWebAdapterConfig.warning ? ` · ${props.deepseekWebAdapterConfig.warning}` : ''}` : t('settings.connections.web.adapter.reading')}><span className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshDeepSeekWebAdapterConfig}>{t('settings.refresh')}</button><button className="settings-action secondary" onClick={props.onOpenDeepSeekWebAdapterConfig}>{t('settings.connections.web.adapter.open')}</button><button className="settings-action secondary" onClick={props.onResetDeepSeekWebAdapterConfig}>{t('settings.connections.web.adapter.reset')}</button></span></Field></Card>
        <Card title={t('settings.connections.api.title')} description={t('settings.connections.api.description')}>
          {/* 只深耕 DeepSeek：地址不再暴露成设置项（值仍是默认的官方地址），少一个能填错的地方。
              用户的原话是"API 网址可以省略"。 */}
          <Field
            title={t('settings.connections.api.key.title')}
            detail={t('settings.connections.api.key.detail')}
          >
            <span className="api-key-actions">
              <input
                type="password"
                aria-label={t('settings.connections.api.key.aria')}
                placeholder={props.apiKeyStatus?.present ? t('settings.connections.api.key.replace') : t('settings.connections.api.key.placeholder')}
                value={props.apiKeyDraft}
                onChange={(event) => props.onApiKeyDraftChange(event.target.value)}
                onKeyDown={(event) => { if (event.key === 'Enter') props.onTestApiKey() }}
                autoComplete="off"
                spellCheck={false}
              />
              <button className="settings-action secondary" disabled={props.apiKeyBusy} onClick={props.onTestApiKey}>{t('settings.connections.api.key.test')}</button>
            </span>
          </Field>
          <Field title={t('settings.connections.api.saved.title')} detail={props.apiKeyStatus?.present ? t('settings.connections.api.saved.present') : t('settings.connections.api.saved.absent')}>
            <span className="api-key-actions">
              <code className="api-key-masked" aria-label={t('settings.connections.api.saved.aria')}>{props.apiKeyStatus?.present ? props.apiKeyStatus.masked ?? '••••' : t('settings.connections.api.saved.unconfigured')}</code>
              <button className="settings-action secondary" disabled={props.apiKeyBusy} onClick={props.onRefreshApiModels}>{t('settings.refresh')}</button>
            </span>
          </Field>
          <Field title={t('settings.connections.api.model.title')} detail={props.apiModelCatalog && props.apiModelCatalog.length > 0 ? t('settings.connections.api.model.available', { count: props.apiModelCatalog.length, age: catalogAgeSuffix(props.apiModelCatalogFetchedAt) }) : t('settings.connections.api.model.hint')}>
            {/* 用面板自己的 `Choice`，**不用** `input list` + `datalist` 那套原生下拉：用户实测
                它在设置窗里根本展不开（只有箭头在那儿摆着，点不动）。`Choice` 是本窗口里已经在用
                的下拉（显示器背景、素材用途），展开由自己控制。
                （这里刻意不写出标签原文，测试用"源码里不许出现该标签"来钉住这条。） */}
            <Choice
              emptyLabel={t('settings.choice.empty')}
              label={t('settings.connections.api.model.label')}
              value={settings.deepseekApi.model}
              onChange={(model) => set({ deepseekApi: { ...settings.deepseekApi, model } })}
              options={apiModelOptions(props.apiModelCatalog ?? [], settings.deepseekApi.model)}
              emptyMessage={props.apiModelCatalog && props.apiModelCatalog.length > 0 ? undefined : t('settings.connections.api.model.empty')}
            />
          </Field>
          {/* ---------------------------------------------------------------------------
          FREEZE（临时冻结，不是删除）：价格输入（「输入价格」/「输出价格」）与它们那两句说明。
          为什么关：用户要求界面上不再出现"计价"。这两项的**唯一**去处就是喂本轮／会话的估算读数，
          而那两个显示面上一批已经冻结（见 `ConversationBubble.tsx` 里 `UsageLine`、顶栏
          `dsh-chat__usage-rail` 与 `turnUsage`/`totalCost` 那几处 FREEZE）。于是「输入价格」那句
          说明 ——"输入、输出价格都配置后，才会显示本轮和会话估算费用"—— 说的显示面已经不在了，
          把它留在页面上就是留一句**假话**；而两个改了也看不见任何变化的输入框，比它们不在更坏
          （与「起别名」「启动参数」冻结时同一条理由）。
          关掉之后：连接页不再有这两个输入项，两条 `detail` 也一并停止渲染 —— 界面上不再出现
          `价格`／`费用`／估算数字／`price-input` 这些痕迹（渲染结果由 `settingsPricing.spec.ts`
          钉住）。
          数据与结构一行没动：`store.ts` 的 `priceInputPerMillion` / `priceOutputPerMillion` 字段、
          归一化与设置文档迁移全部照旧，`App.tsx` 照常读取并传给气泡（`apiPricingConfigured`）与
          原生侧；写进设置里的值只是暂时不显示。
          怎么恢复：取消下面这两行的注释即可，别处一个字都不用改 —— `PriceInput` 组件本身、
          `MAX_PRICE_PER_MILLION`、以及四条词条（标题／说明／标签／占位）都还在原位。
          --------------------------------------------------------------------------- */}
          {/* <Field title={t('settings.connections.api.price-input.title')} detail={t('settings.connections.api.price-input.detail')}><PriceInput label={t('settings.connections.api.price-input.label')} value={settings.deepseekApi.priceInputPerMillion} onChange={(priceInputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceInputPerMillion } })} /></Field> */}
          {/* <Field title={t('settings.connections.api.price-output.title')} detail={t('settings.connections.api.price-output.detail')}><PriceInput label={t('settings.connections.api.price-output.label')} value={settings.deepseekApi.priceOutputPerMillion} onChange={(priceOutputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceOutputPerMillion } })} /></Field> */}
        </Card>
      </>}

      {page === 'appearance' && <>
        <Card title={t('settings.appearance.background.title')} description={t('settings.appearance.background.description')}><div className="background-grid">{BACKGROUND_OPTIONS.map((background) => <button key={background.id} data-background={background.id} className={settings.background === background.id ? 'is-active' : ''} onClick={() => set({ background: background.id })}><span style={background.path ? { backgroundImage: `url(${background.path})` } : undefined} /><strong>{background.name}</strong>{settings.background === background.id && <i>{t('settings.appearance.background.current')}</i>}</button>)}</div></Card>
        <Card title={t('settings.appearance.library.title')} description={t('settings.appearance.library.description')}>
          <div className="asset-library-toolbar"><button className="settings-action" onClick={props.onImportAppearance} disabled={props.appearanceBusy}>{t('settings.appearance.library.import')}</button><span>{t('settings.appearance.library.counts', { inbox: props.appearanceAssets.filter((asset) => asset.status === 'inbox').length, usable: props.appearanceAssets.filter((asset) => asset.status === 'classified').length })}</span></div>
          {props.appearanceAssets.filter((asset) => asset.status === 'inbox').length > 0 && <div className="asset-inbox">{props.appearanceAssets.filter((asset) => asset.status === 'inbox').map((asset) => <div className="asset-inbox-row" key={asset.id}><span><strong>{asset.originalName}</strong><small>{asset.width && asset.height ? `${asset.width} × ${asset.height}` : t('settings.appearance.asset.image')}{asset.hasAlpha ? t('settings.appearance.asset.transparent') : ''}</small></span><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.appearance.asset.purpose', { name: asset.originalName })} value="" onChange={(slot) => props.onClassifyAppearance(asset.id, slot as AppearanceSlot)} disabled={props.appearanceBusy} options={[{ value: '', label: t('settings.appearance.asset.choose-purpose') }, ...componentSlots.map(({ slot, labelKey }) => ({ value: slot, label: t(labelKey) }))]} /></div>)}</div>}
          <div className="asset-component-list">{componentSlots.map(({ slot, labelKey, detailKey }) => {
            const label = t(labelKey)
            const detail = t(detailKey)
            const candidates = props.appearanceAssets.filter((asset) => asset.status === 'classified' && asset.slots.includes(slot))
            const selected = props.appearanceOverrides[slot] ?? ''
            return <div className="asset-component-row" key={slot}><span><strong>{label}</strong><small>{t('settings.appearance.component.detail', { detail, count: candidates.length })}</small></span><Choice emptyLabel={t('settings.choice.empty')} label={label} value={selected} onChange={(id) => { if (id) props.onSelectAppearance(slot, id); else props.onClearAppearance(slot) }} disabled={props.appearanceBusy} emptyMessage={candidates.length === 0 ? t('settings.appearance.component.none') : undefined} options={[{ value: '', label: t('settings.appearance.component.use-default') }, ...candidates.map((asset) => ({ value: asset.id, label: `${asset.originalName}${asset.width && asset.height ? ` (${asset.width} × ${asset.height})` : ''}` }))]} /></div>
          })}</div>
        </Card>
        <Card title={t('settings.appearance.wake.title')}>
          <Field title={t('settings.appearance.wake.enabled.title')}><Toggle label={t('settings.appearance.wake.enabled.toggle')} checked={settings.animationsEnabled} onChange={(value) => set({ animationsEnabled: value })} /></Field>
          <Field title={t('settings.appearance.wake.every-unlock.title')}><Toggle label={t('settings.appearance.wake.every-unlock.toggle')} checked={settings.playWakeOnEveryUnlock} onChange={(value) => set({ playWakeOnEveryUnlock: value })} /></Field>
          <Field title={t('settings.appearance.wake.skip.title')}><Toggle label={t('settings.appearance.wake.skip.toggle')} checked={settings.skipWakeAnimation} onChange={(value) => set({ skipWakeAnimation: value })} /></Field>
          {/* 动画速度与氛围强度先冻结前端（用户要求）：这两项还在开发中，暴露出来只会让设置显得
              比实际能用的多。设置字段与后端行为都保留着（见 store.ts 的 animationSpeed /
              animationIntensity），将来解冻时把这两行还原即可，不需要重新接线。 */}
          {/* <Field title="动画速度" detail={`${settings.animationSpeed.toFixed(1)}×`}><input type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(e) => set({ animationSpeed: Number(e.target.value) })} /></Field> */}
          {/* <Field title="氛围强度"><Choice label="氛围强度" value={settings.animationIntensity} onChange={(value) => set({ animationIntensity: value as WallpaperSettings['animationIntensity'] })} options={[{ value: 'low', label: '克制' }, { value: 'normal', label: '标准' }, { value: 'high', label: '鲜明' }]} /></Field> */}
        </Card>
      </>}

      {page === 'personas' && <>
        <Card title={t('settings.personas.list.title')} description={t('settings.personas.list.description')}>
          <OfficialPersonaCards assets={props.appearanceAssets} overrides={props.appearanceOverrides} />
        </Card>
        {/* 模型与形态映射：前端先冻结（用户要求）。规则的**存储与应用都保留**（settings.modelTierRules
            仍会被人物形态使用），冻结的只是这张编辑卡片 —— 它在形状确定前暴露出来，用户会先学会一个
            将来会变的界面。解冻时把下面这段还原即可。 */}
        {/* <Card title="模型与形态映射" description="模型层级决定年龄，思考强度只改变氛围。">
          <div className="rule-list">{settings.modelTierRules.length === 0 && <div className="settings-empty"><strong>尚未创建自定义规则</strong><span>未识别模型会保持当前形态，冷启动默认为 Flash。</span></div>}{settings.modelTierRules.map((rule, index) => <div className="rule-row" key={index}><select aria-label="后端" value={rule.backend} onChange={(e) => updateRule(index, { backend: e.target.value as ModelTierRule['backend'] })}><option value="*">全部后端</option><option value="deepseek-web">DeepSeek Web</option><option value="deepseek-api">DeepSeek API</option><option value="harness">Harness</option></select><select aria-label="匹配方式" value={rule.match} onChange={(e) => updateRule(index, { match: e.target.value as ModelTierRule['match'] })}><option value="exact">精确</option><option value="contains">包含</option><option value="regex">正则</option></select><input aria-label="模型名称" value={rule.pattern} placeholder="模型名称或表达式" onChange={(e) => updateRule(index, { pattern: e.target.value })} /><select aria-label="形态" value={rule.tier} onChange={(e) => updateRule(index, { tier: e.target.value as ModelTierRule['tier'] })}><option value="flash">Flash · 幼年</option><option value="pro">Pro · 成年</option></select><button aria-label="删除规则" onClick={() => set({ modelTierRules: settings.modelTierRules.filter((_, i) => i !== index) })}>×</button></div>)}</div>
          <button className="settings-action secondary add-rule" onClick={() => set({ modelTierRules: [...settings.modelTierRules, { backend: '*', pattern: '', match: 'contains', tier: 'flash' }] })}>＋ 新增映射规则</button>
        </Card> */}
      </>}

      {page === 'history' && <>
        <Card
          title={t('settings.history.title')}
          description={t('settings.history.description')}
        >
          <div className="asset-library-toolbar">
            <span>
              {t('settings.history.summary', {
                conversations: props.apiHistory?.conversations.length ?? 0,
                messages: props.apiHistory?.totalMessages ?? 0,
                size: formatBytes(props.apiHistory?.totalBytes ?? 0),
              })}
              {props.apiHistory && props.apiHistory.budgetBytes > 0
                ? t('settings.history.budget', { size: formatBytes(props.apiHistory.budgetBytes) })
                : ''}
            </span>
            <span className="integration-actions">
              <button className="settings-action secondary" disabled={props.apiHistoryBusy} onClick={props.onRefreshApiHistory}>
                {props.apiHistoryBusy ? t('settings.history.refreshing') : t('settings.refresh')}
              </button>
              <button
                className="settings-action secondary"
                disabled={props.apiHistoryBusy || (props.apiHistory?.conversations.length ?? 0) === 0}
                onClick={props.onClearApiHistory}
              >
                {t('settings.history.clear')}
              </button>
            </span>
          </div>

          {props.apiHistory && props.apiHistory.budgetBytes > 0 && <div className="history-usage" role="presentation">
            <div className="history-usage__bar"><i style={{ width: `${historyPressure(props.apiHistory.totalBytes, props.apiHistory.budgetBytes)}%` }} /></div>
            <small>
              {t('settings.history.usage', {
                percent: historyPressure(props.apiHistory.totalBytes, props.apiHistory.budgetBytes),
                limit: formatBytes(props.apiHistory.maxBytes),
              })}
            </small>
          </div>}

          {props.apiHistory === undefined && <div className="settings-empty"><strong>{t('settings.history.loading.title')}</strong><span>{t('settings.history.loading.detail')}</span></div>}

          {props.apiHistory?.conversations.length === 0 && <div className="settings-empty">
            <strong>{t('settings.history.empty.title')}</strong>
            <span>{t('settings.history.empty.detail')}</span>
          </div>}

          {(props.apiHistory?.conversations.length ?? 0) > 0 && <div className="history-list">
            {props.apiHistory?.conversations.map((conversation) => <div className="history-row" key={conversation.id}>
              <span className="history-row__main">
                <strong title={conversation.id}>
                  {conversation.id}
                  {conversation.active && <i className="history-row__badge">{t('settings.history.active')}</i>}
                </strong>
                <small>
                  {t('settings.history.row-meta', { messages: conversation.messageCount, size: formatBytes(conversation.bytes), time: formatHistoryTime(conversation.lastMessageAt) })}
                </small>
              </span>
              <button
                className="settings-action secondary history-row__delete"
                disabled={props.apiHistoryBusy}
                aria-label={t('settings.history.row-delete', { id: conversation.id })}
                onClick={() => props.onDeleteApiConversation(conversation.id)}
              >
                {t('settings.history.delete')}
              </button>
            </div>)}
          </div>}
        </Card>
      </>}

      {page === 'system' && <>
        {/* 更新（计划书 §四）：当前版本、上次检查（时间 + 结论）、手动「检查更新」；有可用更新时
            同一张卡片上给出「下载」与「忽略」—— 与立绘气泡同一状态机、同一份原生状态。 */}
        <Card title={t('settings.system.update.title')} description={t('settings.system.update.description')}>
          <Field title={t('settings.system.update.current.title')}>
            <span className="settings-static">{updateCurrentVersion}</span>
          </Field>
          <Field
            title={t('settings.system.update.last-check.title')}
            detail={updateReport
              ? `${updateLastChecked} · ${formatMessage(updateOutcomeMessage(updateReport))}`
              : t('settings.system.reading')}
          >
            <span className="integration-actions">
              <button className="settings-action" disabled={props.updateBusy} onClick={props.onCheckForUpdates}>
                {props.updateBusy ? t('settings.system.update.checking') : t('settings.system.update.check')}
              </button>
              {updateOffer && <>
                {/* 与立绘气泡同一个状态机（`updateNow`）与同一条下载事件：这里给的是同样的动作。
                    `downloading` 只有进度（§十：不提供取消），`ready` 是「点击安装」，
                    `failed` 是「重试」+「打开发布页」（§六 的回落）。 */}
                {updateNow === 'downloading' && props.updateDownload
                  ? <span className="settings-static" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={downloadPercent(props.updateDownload)} aria-valuetext={formatMessage(downloadProgressMessage(props.updateDownload))}>
                      {formatMessage(downloadProgressMessage(props.updateDownload))}
                    </span>
                  : <>
                      <button className="settings-action secondary" disabled={props.updateBusy} onClick={updateNow === 'ready' ? props.onInstallUpdate : props.onDownloadUpdate}>
                        {updateNow === 'ready'
                          ? t('update.action.install')
                          : updateNow === 'failed'
                            ? t('update.action.retry')
                            : updateOffer.asset ? t('update.action.download') : t('update.action.release-page')}
                      </button>
                      {(updateNow === 'failed' || !updateOffer.asset) && (
                        <button className="settings-action secondary" disabled={props.updateBusy} onClick={props.onOpenUpdatePage}>
                          {t('update.action.release-page')}
                        </button>
                      )}
                      {updateNow !== 'failed' && (
                        <button className="settings-action secondary" disabled={props.updateBusy} onClick={() => props.onDismissUpdate(updateOffer.version)}>
                          {t('update.action.dismiss')}
                        </button>
                      )}
                    </>}
              </>}
            </span>
          </Field>
          {/* 失败原因就显示在卡片上（§四 的 `failed`、§八 7：断网、资产缺失、校验不符都要看得见）。 */}
          {updateNow === 'failed' && <p className="settings-hint" role="status">{formatMessage(downloadFailedMessage(props.updateDownload))}</p>}
          {props.updateNotice && <p className="settings-hint" role="status">{formatMessage(props.updateNotice)}</p>}
        </Card>
        <Card title={t('settings.system.data.title')} description={t('settings.system.data.description')}>
          <Field title={t('settings.system.workspace.title')} detail={props.desktopWorkspace ? (props.desktopWorkspace.workspaceExists ? props.desktopWorkspace.workspaceDirectory : props.desktopWorkspace.workspaceDirectory + t('settings.system.workspace.missing')) : t('settings.system.reading')}><span /></Field>
          {/* 卸载时问不了（MSIX 没有自定义卸载界面），所以"想清干净的时候能清干净"这个入口放在这里。
              原生只做它能证明做完的两件：工作区目录 + 凭据管理器里那条 Key；WebView2 配置目录正被
              运行中的进程占用，删不干净，所以如实回报路径让用户退出后自己删。 */}
          <Field title={t('settings.system.memory.title')} detail={props.desktopWorkspace ? props.desktopWorkspace.memoryFile + (props.desktopWorkspace.memoryExists ? t('settings.system.memory.present') : t('settings.system.memory.absent')) : t('settings.system.reading')}>
            <button
              className="settings-action secondary"
              disabled={props.openingMemory}
              onClick={() => { void props.onOpenProjectMemory?.() }}
            >{props.openingMemory ? t('settings.system.memory.opening') : t('settings.system.memory.open')}</button>
          </Field>
          <Field title={t('settings.system.clear.title')} detail={clearDetail ? clearDetail.map(formatMessage).join(t('settings.system.clear.join')) : t('settings.system.clear.detail')}>
            <button
              className="settings-action secondary"
              disabled={clearing}
              onClick={() => { void clearUserData() }}
            >{clearing ? t('settings.system.clear.clearing') : t('settings.system.clear.action')}</button>
          </Field>
        </Card>
        <Card title={t('settings.system.windows.title')}>
          <Field title={t('settings.system.autostart.title')} detail={props.autostartBusy ? t('settings.system.autostart.busy') : autostartDetail(props.autostart)}><Toggle label={t('settings.system.autostart.toggle')} checked={settings.autostart} onChange={(value) => set({ autostart: value })} disabled={props.autostartBusy} /></Field>
          {/* FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30）。恢复办法：还原这段，并恢复接口里的 6 个 prop 与 SettingsWindow 的传参。
          <Field title="接管锁屏图片" detail={props.lockScreenBusy ? '正在应用系统锁屏设置，请稍候。' : '使用内置且已审计的熟睡画面；密码界面仍由 Windows 原生安全桌面处理。正式版需要 MSIX 包身份。'}><Toggle label="接管锁屏图片" checked={settings.lockScreenEnabled} onChange={props.onSetLockScreenEnabled} disabled={props.lockScreenBusy} /></Field>
          <div className="lockscreen-diagnostics">
            <div className="lockscreen-diagnostics__row"><div><strong>接管状态</strong><small>{props.lockScreenDiagnostics?.managedImageActive ? '正在使用大肥鱼的熟睡画面' : '未检测到本应用的锁屏图片'}</small></div>{props.lockScreenDiagnostics?.managedImageActive ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRestoreLockScreen}>打开 Windows 锁屏设置</button> : props.lockScreenDiagnostics?.staleBackup ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onClearStaleLockScreenBackup}>{props.lockScreenBusy ? '正在清理…' : '清理旧恢复点（删除原图副本）'}</button> : null}</div>
            <div className="lockscreen-diagnostics__row"><div><strong>接管前检查</strong><small>{props.lockScreenDiagnostics ? props.lockScreenDiagnostics.takeoverAvailable ? 'Windows 与当前应用身份允许尝试设置锁屏图片' : props.lockScreenDiagnostics.supported ? 'Windows 允许，但当前正式版需要 MSIX 包身份' : '当前系统不允许应用修改锁屏图片' : '正在读取系统状态…'}</small></div><button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRefreshLockScreenDiagnostics}>{props.lockScreenBusy ? '正在应用…' : '刷新检查'}</button></div>
            {props.lockScreenDiagnostics && <ul><li>备份：{props.lockScreenDiagnostics.staleBackup ? '已保留，但当前锁屏已被外部更改' : props.lockScreenDiagnostics.backupValid ? '原静态图片可恢复' : props.lockScreenDiagnostics.backupExists ? '备份失效' : '尚未创建（首次接管时保存）'}</li><li>托管睡眠图：{props.lockScreenDiagnostics.managedImageReady ? '已准备' : '首次接管时准备'}</li>{props.lockScreenDiagnostics.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>}
          </div>
          */}
        </Card>
        {/* FREEZE(1B)：透明任务栏卡片随系统集成一起冻结。恢复办法：去掉这对注释，并恢复接口与传参。
        <Card title="透明任务栏" description="通过松耦合方式连接独立安装的 TranslucentTB，本应用不会修改其配置。">
          <div className="integration-status"><div><i className={translucentTb.running ? 'is-online' : ''} /><span><strong>{translucentTb.running ? 'TranslucentTB 正在运行' : translucentTb.installed ? 'TranslucentTB 已安装' : 'TranslucentTB 未安装'}</strong><small>{translucentTb.source ?? '由用户独立安装和管理'}</small></span></div><div className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshTranslucentTb}>刷新</button><button className="settings-action" onClick={translucentTb.installed ? props.onLaunchTranslucentTb : props.onInstallTranslucentTb}>{translucentTb.installed ? '启动' : '前往商店'}</button></div></div>
        </Card>
        */}
      </>}
    </main>

    <footer className="settings-statusbar"><span>dsh-wallpaper · v0.4.9</span><span aria-live="polite">{savedAt ? <><SettingsIcon name="check" size={14} />{t('statusbar.saved')}</> : <><i />{t('statusbar.autosave')}</>}</span></footer>
  </div>
}
