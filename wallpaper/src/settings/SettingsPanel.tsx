import { formatMessage, msg, sentenceOf, setLanguage, t, useLanguage, type Language, type Message, type MessageKey } from '../i18n/index.ts'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
// `ModelTierRule` 随「模型与形态映射」卡片一起被冻结，解冻时连同上面那行 `updateRule` 一起加回来。
import type { BackendMode } from '../domain/types.ts'
import type { WallpaperSettings } from './store.ts'
import { type SettingsPage } from './settingsProbes.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
import type { DeepSeekWebAdapterConfigStatus, DesktopDisplayInfo, DesktopWorkspaceStatus, ManagedDshStatus, ApiConversationListing, ApiKeyStatus, UpdateCheckReport } from '../native/runtime.ts'
import { downloadPercent, offeredUpdate, updatePhase, type UpdateDownloadState } from '../features/update/updateState.ts'
import { downloadFailedMessage, downloadProgressMessage, updateOutcomeMessage } from '../features/update/updateCopy.ts'

import type { AutostartStatus, HarnessEndpointScan, HarnessTarget } from '../native/runtime.ts'
import { autostartDetail } from './autostartCopy.ts'
import { SettingsIcon, type SettingsIconName } from './SettingsIcon.tsx'
import { SettingsPersonaBadge } from './SettingsPersonaBadge.tsx'
import { GeneralTab } from './tabs/GeneralTab.tsx'
import { AppearanceTab } from './tabs/AppearanceTab.tsx'
import { PersonasTab } from './tabs/PersonasTab.tsx'
import { ConnectionsTab } from './tabs/ConnectionsTab.tsx'
import { HistoryTab, formatBytes, formatHistoryTime, historyPressure } from './tabs/HistoryTab.tsx'

import { Card, Field, Toggle } from './components/SettingsControls.tsx'
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
const pages: Array<{ id: Page; icon: SettingsIconName; labelKey: MessageKey; hintKey: MessageKey }> = [
  { id: 'general', icon: 'general', labelKey: 'nav.general.label', hintKey: 'nav.general.hint' },
  { id: 'connections', icon: 'connections', labelKey: 'nav.connections.label', hintKey: 'nav.connections.hint' },
  { id: 'appearance', icon: 'appearance', labelKey: 'nav.appearance.label', hintKey: 'nav.appearance.hint' },
  { id: 'personas', icon: 'personas', labelKey: 'nav.personas.label', hintKey: 'nav.personas.hint' },
  { id: 'history', icon: 'history', labelKey: 'nav.history.label', hintKey: 'nav.history.hint' },
  { id: 'system', icon: 'system', labelKey: 'nav.system.label', hintKey: 'nav.system.hint' },
]

export { formatBytes, formatHistoryTime, historyPressure }

export {
  BACKEND_MODE_LABELS,
  CHAT_MODE_OPTIONS,
  PriceInput,
  apiModelOptions,
  backendModeLabel,
  backendModeMessage,
  catalogAgeSuffix,
  chatModeOptions,
  harnessEndpointKindLabel,
} from './tabs/ConnectionsTab.tsx'

export function SettingsPanel(props: SettingsPanelProps) {
  const { settings, harnessStatus, onChange, onClose, page } = props
  const [savedAt, setSavedAt] = useState<number>()
  const savedTimer = useRef<ReturnType<typeof setTimeout>>()
  const [selectedDisplayId, setSelectedDisplayId] = useState<string>()
  const [showDisplayNumbers, setShowDisplayNumbers] = useState(false)
  const settingsContentRef = useRef<HTMLElement>(null)
  const set = (patch: Partial<WallpaperSettings>) => {
    onChange({ ...settings, ...patch })
    setSavedAt(Date.now())
    if (savedTimer.current !== undefined) clearTimeout(savedTimer.current)
    savedTimer.current = setTimeout(() => setSavedAt(undefined), 1600)
  }
  useEffect(() => () => {
    if (savedTimer.current !== undefined) clearTimeout(savedTimer.current)
  }, [])
  useLayoutEffect(() => {
    if (settingsContentRef.current) settingsContentRef.current.scrollTop = 0
  }, [page])
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
  // 与被冻结的「模型与形态映射」卡片同进退：它唯一的用途就是在那张卡片里改规则。
  // const updateRule = (index: number, patch: Partial<ModelTierRule>) => set({ modelTierRules: settings.modelTierRules.map((rule, i) => i === index ? { ...rule, ...patch } : rule) })
  const pageMeta = pages.find((item) => item.id === page)!
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

    <main className="settings-content" ref={settingsContentRef}>
      <div className="settings-page-heading"><span>{t('settings.page.heading', { page: t(pageMeta.labelKey) })}</span><h1>{t(pageMeta.labelKey)}</h1><p>{t(pageMeta.hintKey)}</p></div>

      {page === 'general' && <GeneralTab
          {...props}
          set={set}
          selectedDisplayId={selectedDisplayId}
          setSelectedDisplayId={setSelectedDisplayId}
          showDisplayNumbers={showDisplayNumbers}
          setShowDisplayNumbers={setShowDisplayNumbers}
          language={language}
          onLanguageChange={(value) => { const next = value as Language; set({ language: next }); setLanguage(next) }}
        />}

      {page === 'connections' && <ConnectionsTab
          {...props}
          set={set}
        />}

      {page === 'appearance' && <AppearanceTab
          {...props}
          set={set}
        />}

      {page === 'personas' && <>
        <PersonasTab appearanceAssets={props.appearanceAssets} appearanceOverrides={props.appearanceOverrides} />
        {/* 模型与形态映射：前端先冻结（用户要求）。规则的**存储与应用都保留**（settings.modelTierRules
            仍会被人物形态使用），冻结的只是这张编辑卡片 —— 它在形状确定前暴露出来，用户会先学会一个
            将来会变的界面。解冻时把下面这段还原即可。 */}
        {/* <Card title="模型与形态映射" description="模型层级决定年龄，思考强度只改变氛围。">
          <div className="rule-list">{settings.modelTierRules.length === 0 && <div className="settings-empty"><strong>尚未创建自定义规则</strong><span>未识别模型会保持当前形态，冷启动默认为 Flash。</span></div>}{settings.modelTierRules.map((rule, index) => <div className="rule-row" key={index}><select aria-label="后端" value={rule.backend} onChange={(e) => updateRule(index, { backend: e.target.value as ModelTierRule['backend'] })}><option value="*">全部后端</option><option value="deepseek-web">DeepSeek Web</option><option value="deepseek-api">DeepSeek API</option><option value="harness">Harness</option></select><select aria-label="匹配方式" value={rule.match} onChange={(e) => updateRule(index, { match: e.target.value as ModelTierRule['match'] })}><option value="exact">精确</option><option value="contains">包含</option><option value="regex">正则</option></select><input aria-label="模型名称" value={rule.pattern} placeholder="模型名称或表达式" onChange={(e) => updateRule(index, { pattern: e.target.value })} /><select aria-label="形态" value={rule.tier} onChange={(e) => updateRule(index, { tier: e.target.value as ModelTierRule['tier'] })}><option value="flash">Flash · 幼年</option><option value="pro">Pro · 成年</option></select><button aria-label="删除规则" onClick={() => set({ modelTierRules: settings.modelTierRules.filter((_, i) => i !== index) })}>×</button></div>)}</div>
          <button className="settings-action secondary add-rule" onClick={() => set({ modelTierRules: [...settings.modelTierRules, { backend: '*', pattern: '', match: 'contains', tier: 'flash' }] })}>＋ 新增映射规则</button>
        </Card> */}
      </>}

      {page === 'history' && <HistoryTab {...props} />}

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
        </Card>
      </>}
    </main>

    <footer className="settings-statusbar"><span>dsh-wallpaper · v0.4.10</span><span aria-live="polite">{savedAt ? <><SettingsIcon name="check" size={14} />{t('statusbar.saved')}</> : <><i />{t('statusbar.autosave')}</>}</span></footer>
  </div>
}
