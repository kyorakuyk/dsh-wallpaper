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
import type { DeepSeekWebAdapterConfigStatus, DesktopDisplayInfo, DesktopWorkspaceStatus, ManagedDshStatus, ApiConversationListing, ApiKeyStatus } from '../native/runtime.ts'
import { preferredDisplayId } from '../runtime/displayLayout.ts'
import { harnessStateLabel } from '../connect/harnessLabels.ts'
import type { AutostartStatus, HarnessEndpointScan, HarnessTarget } from '../native/runtime.ts'
import { autostartDetail, autostartKnown } from './autostartCopy.ts'
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

function Card({ title, description, action, children }: { title: string; description?: string; action?: ReactNode; children: ReactNode }) {
  return <section className="settings-card"><header><div className="settings-card__heading"><h2>{title}</h2>{description && <p>{description}</p>}</div>{action && <div className="settings-card__action">{action}</div>}</header><div className="settings-card__body">{children}</div></section>
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
export function chatModeOptions(): Array<{ value: BackendMode; label: string }> {
  // 这个下拉只回答一件事：**滑槽在左边时，聊天连接哪个通道**。滑槽在右端时一定是 Harness，
  // 而那是滑槽自己的含义 —— 与这里无关，所以候选**恒为两项**，不为运行中的后端追加任何项。
  return [...CHAT_MODE_OPTIONS]
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
  // FREEZE(1B)：解构里去掉已冻结的 translucentTb。
  // const { settings, harnessStatus, onChange, onClose, translucentTb, page } = props
  const { settings, harnessStatus, onChange, onClose, page } = props
  const set = (patch: Partial<WallpaperSettings>) => onChange({ ...settings, ...patch })
  /** 「清除全部用户数据」的结果（成功后把"删了什么、还剩什么要你手动删"写在这一行里）。 */
  const [clearDetail, setClearDetail] = useState<string>()
  const [clearing, setClearing] = useState(false)
  const clearUserData = async () => {
    // 逐项写清会删什么、要你手动删什么：这类操作不可逆，值得在读清之前先拦一下。
    const confirmed = window.confirm([
      '将删除：',
      '· 桌面会话工作区（' + (props.desktopWorkspace?.workspaceDirectory ?? '数据目录下的「桌面会话」') + '）',
      '· 凭据管理器里保存的 DeepSeek API Key',
      '',
      '需要你自己删（本应用正在运行，删不干净）：',
      '· 设置与网页登录态：' + (props.desktopWorkspace?.dataDirectory ?? '%LOCALAPPDATA%\\com.dsh.wallpaper（退出后删除）'),
      '· 桥接凭据：~/.dsh/wallpaper',
      '',
      '继续？',
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
        ...result.removed.map((path) => `已删除 ${path}`),
        result.credentialRemoved ? '已删除凭据管理器里的 API Key' : '凭据管理器里没有这条 Key（无需删除）',
      ]
      const manual = result.manual.map((entry) => `退出应用后手动删除：${entry.what} → ${entry.path}`)
      setClearDetail([...done, ...manual].join('；'))
    } catch (error) {
      setClearDetail(`清除失败：${String(error)}`)
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
  const openRoutes: Array<{ value: 'browser' | 'tui'; label: string }> = !settings.dshLaunch.subjectId
    ? []
    : shellSelected
      ? [{ value: 'browser', label: '官方客户端窗口' }]
      : selectedSubject?.kind === 'installed-cli'
        ? [{ value: 'browser', label: '浏览器' }, { value: 'tui', label: '终端里的 TUI' }]
        : [{ value: 'browser', label: '浏览器' }]
  // 存着的路线只有在**真的有两条路**时才作数：官壳/源码树即便档案里写着 tui，也仍然走它们唯一的路。
  const openRoute: 'browser' | 'tui' =
    openRoutes.length > 1 && settings.dshLaunch.window === 'tui' ? 'tui' : 'browser'
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
      <div className="settings-brand"><img className="settings-brand__mark" src="/brand/persona-mark.png" alt="" draggable={false} /><div><strong>Wallpaper</strong><small>个性化控制中心</small></div></div>
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
          <Field title="中央会话窗" detail="关闭后仅可通过托盘右键或此处重新打开"><Toggle label="显示中央会话窗" checked={props.interactionEnabled} onChange={props.onSetInteractionEnabled} /></Field>
          <Field title="气泡布局" detail="中央悬浮始终展开；任务栏停靠以胶囊按钮唤起。"><Choice label="气泡布局" value={settings.interactionLayout} onChange={(value) => set({ interactionLayout: value as WallpaperSettings['interactionLayout'] })} options={[{ value: 'floating', label: '中央玻璃悬浮' }, { value: 'taskbar-docked', label: '任务栏停靠胶囊' }]} /></Field>
          <Field title="历史抽屉默认展开" detail="启动或解锁后直接显示最近的对话。"><Toggle label="历史抽屉默认展开" checked={settings.historyStartsExpanded} onChange={(value) => set({ historyStartsExpanded: value })} /></Field>
          <Field title="发送消息快捷键" detail="想防止误触发送的开发者可切换为 Ctrl+Enter 发送。"><Choice label="发送消息快捷键" value={settings.sendShortcut} onChange={(value) => set({ sendShortcut: value as WallpaperSettings['sendShortcut'] })} options={[{ value: 'Enter', label: 'Enter 发送，Ctrl+Enter 换行' }, { value: 'Ctrl+Enter', label: 'Ctrl+Enter 发送，Enter 换行' }]} /></Field>
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
        <Card title="高级外观（测试中）" description="环境渐变只作用于立绘；会话窗使用独立的亚克力透明度。">
          <Field title="环境渐变长度" detail={`从暗侧向亮侧延伸至 ${settings.portraitAmbientLength}%`}><input type="range" min="35" max="100" step="1" value={settings.portraitAmbientLength} onChange={(event) => set({ portraitAmbientLength: Number(event.target.value) })} /></Field>
          <Field title="环境渐变强度" detail={`${Math.round(settings.portraitAmbientStrength * 100)}%`}><input type="range" min="0" max="1" step="0.01" value={settings.portraitAmbientStrength} onChange={(event) => set({ portraitAmbientStrength: Number(event.target.value) })} /></Field>
          <Field title="中央会话窗透明度" detail={`${Math.round(settings.conversationOpacity * 100)}% · 仅影响亚克力底色，不影响文字可读性`}><input type="range" min="0.2" max="0.96" step="0.01" value={settings.conversationOpacity} onChange={(event) => set({ conversationOpacity: Number(event.target.value) })} /></Field>
          <Field title="中央会话窗磨砂" detail={`${settings.conversationBlur}px · 0 为纯透明玻璃，数值越高背景越柔和`}><input type="range" min="0" max="40" step="1" value={settings.conversationBlur} onChange={(event) => set({ conversationBlur: Number(event.target.value) })} /></Field>
        </Card>
      </>}

      {page === 'connections' && <>
        <Card title="聊天模式" description="二选一。更改会话的通道，会记作下次启动的默认值；网页桥接不会在失败时自动切到付费 API。">
          <Field title="当前使用" detail="滑槽在左边时，聊天走这里选的通道；改动会记作下次启动的默认值。">
            {/* 值是**启动默认值**：这个控件给"滑槽在左边"这个位置赋予含义。滑槽在右端时一定是
                Harness，那由滑槽自己表达，这里不描述、也不需要一个不可选但可见的 Harness 项。 */}
            <Choice label="当前使用" value={settings.defaultBackend} onChange={(value) => props.onSelectBackend(value as BackendMode)} options={chatModeOptions()} />
          </Field>
          <Field title="DSH 就绪时自动切换" detail="当 harness 就绪时自动切换到 harness 模式。"><Toggle label="DSH 自动切换" checked={settings.autoSwitchHarness} onChange={(value) => set({ autoSwitchHarness: value })} /></Field>
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
          title="DeepSeek Harness 连接"
          description="当第一次使用与本机含有多个不同dsh时使用"
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
            title="运行方式"
            detail={props.dshScanBusy
              ? '正在后台搜索可识别的运行方式，请稍候。'
              : props.harnessTargets.length === 0
                ? '点「扫描」找出本机可以运行的 DSH。'
                : [props.subjectChoice, `已发现 ${props.harnessTargets.length} 个可选项${props.subjectCatalogVerifiedAt ? `（${catalogAgeLabel(props.subjectCatalogVerifiedAt)}）` : ''}。`].filter(Boolean).join(' ')}
          >
            <span className="integration-actions">
              {props.harnessTargets.length > 0 && (
                <Choice
                  label="运行方式"
                  value={settings.dshLaunch.subjectId ?? ''}
                  onChange={props.onSelectSubject}
                  options={[
                    /*
                      A subject stored before this list was rescanned stays visible:
                      dropping the user's choice because a scan has not run yet is the
                      "looks empty" failure the catalogue exists to prevent.
                    */
                    ...(settings.dshLaunch.subjectId && !props.harnessTargets.some((target) => sameSubject(target.id, settings.dshLaunch.subjectId))
                      ? [{ value: settings.dshLaunch.subjectId, label: `当前：${displaySubjectPath(settings.dshLaunch.subjectId)}` }]
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
                {props.dshScanBusy ? '扫描中…' : props.harnessTargets.length > 0 ? '重新扫描' : '扫描'}
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
            {!cliSelected && <Field title="源码目录" detail="这份源码的位置；扫描会用它作为下一次查找的提示路径。"><span className="settings-static">{displaySubjectPath(selectedSubject?.identity.rootPath ?? settings.dshLaunch.rootPath)}</span></Field>}
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
            title="打开界面"
            detail={!selectedSubject
              ? '先在上面选定运行方式。'
              : shellSelected
                ? '会把它自己的窗口调到前台；如果它没在运行，会先把它启动起来。'
                : openRoutes.length > 1
                  ? '浏览器用它的网页界面；TUI 会在一个新的终端窗口里打开。没在运行时都会先把它启动起来。'
                  : '会用它的网页界面（默认浏览器）；如果它没在运行，会先把它启动起来。'}
          >
            {/* 两条路才给下拉；只有一条路就写一行字，而不是做一个改不动、点了也没反应的控件。 */}
            {openRoutes.length > 1
              ? (
                  <Choice
                    label="拉起的窗口"
                    value={openRoute}
                    onChange={(value) => props.onSelectWindow(value === 'tui' ? 'tui' : 'browser')}
                    options={openRoutes}
                  />
                )
              : openRoutes.length === 1
                ? <span className="settings-static">{openRoutes[0]!.label}</span>
                : null}
            <button className="settings-action" disabled={!settings.dshLaunch.subjectId || props.openBusy} onClick={openRoute === 'tui' ? props.onOpenTui : props.onOpenClient}>
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
          {/* 未读到不等于没开：状态由「系统」页的 probe 读取，占位值的 enabled 也是 false，
              所以这条警告只在确实读到是关的时候出现，否则它会为开着的自启报错。 */}
          {settings.dshLaunch.autoStartWithWallpaper && autostartKnown(props.autostart) && !props.autostart.enabled && (
            <Field title="壁纸开机自启未生效" detail="开机后自动启动 DSH 依赖壁纸自身的开机自启。">{
              props.autostart.source === 'disabled-by-user'
                ? 'Windows 任务管理器已禁用本应用的自启项，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。'
                : props.autostart.source === 'disabled-by-policy'
                  ? '系统策略禁用了本应用的自启项，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。'
                  : '壁纸自身尚未设置开机自启，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。请在「常规」中开启壁纸自启。'
            }</Field>
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
          {!shellSelected && <Field title="本应用启动的 DSH" detail={props.managedDsh.managed
            ? '该 DSH 由本应用启动，可以在这里停止它。'
            : '本应用没有启动 DSH；其他人启动的实例不会被停止。'}><span className="integration-actions"><button className="settings-action secondary" disabled={props.managedDshBusy} onClick={props.onRefreshManagedDsh}>刷新</button>{/* 启用条件跟**是不是本应用启动的**走，不跟"有没有在跑"走：只要 3080 上有别的东西在跑，
              旧写法就会点亮一个点了没反应的按钮（实测：装机重启后壁纸丢了"这是我的孩子"的记录）。 */}
            <button className="settings-action secondary" disabled={!props.managedDsh.managed || props.managedDshBusy} onClick={props.onStopAllManagedDsh}>停止本应用启动的 DSH</button></span></Field>}
        </Card>
        <Card title="DeepSeek 网页入口（实验）" description="在壁纸里用你的网页版账号对话；登录后直连。"><Field title="页面" detail="页面和登录状态由独立 WebView2 配置目录保存；本应用不读取、复制或记录 Cookie。"><button className="settings-action" onClick={props.onRequestDeepSeekLogin}>打开应用内页面</button></Field>{/* 适配器那行的说明里**不再显示本地 override 的文件路径**（用户要求）：那是一串
            `%APPDATA%\com.dsh.wallpaper\deepseek-web-adapter.override.json`，对"网页结构变了才需要动它"
            这件事没有任何帮助，只会把一行说明压成三行。路径仍然在原生侧读得到
            （`deepseekWebAdapterConfig.path`），「打开配置」按钮就是照着它打开文件的 —— 需要它的人
            按那个按钮，不需要它的人不必看见。 */}
        <Field title="网页适配（高级）" detail={props.deepseekWebAdapterConfig ? `网页结构变化时才需要动它，平常不用管。${props.deepseekWebAdapterConfig.source === 'local' ? '本地 override' : '内置默认'} · ${props.deepseekWebAdapterConfig.adapterVersion}${props.deepseekWebAdapterConfig.warning ? ` · ${props.deepseekWebAdapterConfig.warning}` : ''}` : '正在读取配置状态…'}><span className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshDeepSeekWebAdapterConfig}>刷新</button><button className="settings-action secondary" onClick={props.onOpenDeepSeekWebAdapterConfig}>打开配置</button><button className="settings-action secondary" onClick={props.onResetDeepSeekWebAdapterConfig}>恢复默认</button></span></Field></Card>
        <Card title="DeepSeek API" description="API 模式会产生实际费用，密钥只保存在 Windows 凭据管理器。">
          {/* 只深耕 DeepSeek：地址不再暴露成设置项（值仍是默认的官方地址），少一个能填错的地方。
              用户的原话是"API 网址可以省略"。 */}
          <Field
            title="访问密钥"
            detail="在这里填入 DeepSeek API Key，按测试确认连通性，自动拉取可用模型。"
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
        <Card title="素材库（导入功能测试中）" description="导入的单张素材先选择用途，再出现在对应组件的枚举菜单中。主题包和插件将在后续版本单独处理。">
          <div className="asset-library-toolbar"><button className="settings-action" onClick={props.onImportAppearance} disabled={props.appearanceBusy}>导入图片素材</button><span>{props.appearanceAssets.filter((asset) => asset.status === 'inbox').length} 项待分类 · {props.appearanceAssets.filter((asset) => asset.status === 'classified').length} 项可用</span></div>
          {props.appearanceAssets.filter((asset) => asset.status === 'inbox').length > 0 && <div className="asset-inbox">{props.appearanceAssets.filter((asset) => asset.status === 'inbox').map((asset) => <div className="asset-inbox-row" key={asset.id}><span><strong>{asset.originalName}</strong><small>{asset.width && asset.height ? `${asset.width} × ${asset.height}` : '图片'}{asset.hasAlpha ? ' · 透明背景' : ''}</small></span><Choice label={`${asset.originalName} 的用途`} value="" onChange={(slot) => props.onClassifyAppearance(asset.id, slot as AppearanceSlot)} disabled={props.appearanceBusy} options={[{ value: '', label: '选择用途…' }, ...componentSlots.map(({ slot, label }) => ({ value: slot, label }))]} /></div>)}</div>}
          <div className="asset-component-list">{componentSlots.map(({ slot, label, detail }) => {
            const candidates = props.appearanceAssets.filter((asset) => asset.status === 'classified' && asset.slots.includes(slot))
            const selected = props.appearanceOverrides[slot] ?? ''
            return <div className="asset-component-row" key={slot}><span><strong>{label}</strong><small>{detail} · {candidates.length} 项可选</small></span><Choice label={label} value={selected} onChange={(id) => { if (id) props.onSelectAppearance(slot, id); else props.onClearAppearance(slot) }} disabled={props.appearanceBusy} emptyMessage={candidates.length === 0 ? '暂无此类素材，请先导入并指定用途' : undefined} options={[{ value: '', label: '使用默认' }, ...candidates.map((asset) => ({ value: asset.id, label: `${asset.originalName}${asset.width && asset.height ? ` (${asset.width} × ${asset.height})` : ''}` }))]} /></div>
          })}</div>
        </Card>
        <Card title="苏醒动画（开发中）">
          <Field title="启用动画"><Toggle label="启用苏醒动画" checked={settings.animationsEnabled} onChange={(value) => set({ animationsEnabled: value })} /></Field>
          <Field title="每次解锁播放"><Toggle label="每次解锁播放" checked={settings.playWakeOnEveryUnlock} onChange={(value) => set({ playWakeOnEveryUnlock: value })} /></Field>
          <Field title="跳过苏醒过程"><Toggle label="跳过苏醒过程" checked={settings.skipWakeAnimation} onChange={(value) => set({ skipWakeAnimation: value })} /></Field>
          {/* 动画速度与氛围强度先冻结前端（用户要求）：这两项还在开发中，暴露出来只会让设置显得
              比实际能用的多。设置字段与后端行为都保留着（见 store.ts 的 animationSpeed /
              animationIntensity），将来解冻时把这两行还原即可，不需要重新接线。 */}
          {/* <Field title="动画速度" detail={`${settings.animationSpeed.toFixed(1)}×`}><input type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(e) => set({ animationSpeed: Number(e.target.value) })} /></Field> */}
          {/* <Field title="氛围强度"><Choice label="氛围强度" value={settings.animationIntensity} onChange={(value) => set({ animationIntensity: value as WallpaperSettings['animationIntensity'] })} options={[{ value: 'low', label: '克制' }, { value: 'normal', label: '标准' }, { value: 'high', label: '鲜明' }]} /></Field> */}
        </Card>
      </>}

      {page === 'personas' && <>
        <Card title="人物列表" description="四张正式立绘是固定的后端／模型层级映射。此处只用于审阅；如需替换某张图，请到“外观 → 素材库”为对应槽位指定素材。">
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
        <Card title="数据与目录" description="「桌面会话」的工作区落在壁纸自己的数据目录里：升级安装会保留，卸载之后也留得下。若想彻底删除数据，在卸载前请先点击下面的“清除全部用户数据”。">
          <Field title="工作区" detail={props.desktopWorkspace ? (props.desktopWorkspace.workspaceExists ? props.desktopWorkspace.workspaceDirectory : props.desktopWorkspace.workspaceDirectory + '（还没创建；桥第一次用到时会在这里建出来）') : '正在读取…'}><span /></Field>
          {/* 卸载时问不了（MSIX 没有自定义卸载界面），所以"想清干净的时候能清干净"这个入口放在这里。
              原生只做它能证明做完的两件：工作区目录 + 凭据管理器里那条 Key；WebView2 配置目录正被
              运行中的进程占用，删不干净，所以如实回报路径让用户退出后自己删。 */}
          <Field title="项目记忆" detail={props.desktopWorkspace ? props.desktopWorkspace.memoryFile + (props.desktopWorkspace.memoryExists ? '（助手维护；说话人格等长期要求就写在这里）' : '（还没有：你或助手第一次「记下来」时会出现）') : '正在读取…'}>
            <button
              className="settings-action secondary"
              disabled={props.openingMemory}
              onClick={() => { void props.onOpenProjectMemory?.() }}
            >{props.openingMemory ? '正在打开…' : '打开项目记忆'}</button>
          </Field>
          <Field title="清除全部用户数据" detail={clearDetail ?? '删除本应用的桌面会话工作区，以及凭据管理器里保存的 API Key。设置与网页登录态在 WebView2 配置目录里，需要退出应用后手动删除（下面会给出路径）。'}>
            <button
              className="settings-action secondary"
              disabled={clearing}
              onClick={() => { void clearUserData() }}
            >{clearing ? '正在清除…' : '清除全部用户数据'}</button>
          </Field>
        </Card>
        <Card title="Windows 集成">
          <Field title="登录后自动启动" detail={props.autostartBusy ? '正在更新 Windows 启动任务，请稍候；设置中心仍可继续使用。' : autostartDetail(props.autostart)}><Toggle label="登录后自动启动" checked={settings.autostart} onChange={(value) => set({ autostart: value })} disabled={props.autostartBusy} /></Field>
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

    <footer className="settings-statusbar"><span>dsh-wallpaper · v0.3.0</span><span><i />设置会自动保存</span></footer>
  </div>
}
