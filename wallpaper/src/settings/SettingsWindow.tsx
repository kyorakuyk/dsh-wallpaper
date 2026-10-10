import { summarizeBridgeInstall } from '../connect/bridgeInstall.ts'
import { useEffect, useMemo, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import type { BackendMode } from '../domain/types.ts'
import { appCoreClient } from '../runtime/appCoreClient.ts'
import { nativeRuntime, type AutostartStatus, type ApiConversationListing, type ApiKeyStatus, type DeepSeekWebAdapterConfigStatus, type DesktopDisplayInfo, type DesktopWorkspaceStatus, type HarnessEndpointScan, type HarnessTarget, type ManagedDshStatus } from '../native/runtime.ts'
import { loadSettings, saveSettings, type WallpaperSettings } from './store.ts'
import {
  CLI_SUBJECT_PREFIX,
  clientRaiseAction,
  endpointKindMessage,
  endpointScopeOf,
  raiseOutcomeNotice,
  staleEndpointPort,
  subjectClientKind,
  subjectEndpointPorts,
  unsupportedShellSubjectFallback,
  type HarnessClientKind,
} from '../connect/endpoints.ts'
import { launchOutcomeNotice, reachNeedsBrowser, subjectChoicePrompt } from '../connect/harnessSubjects.ts'
import { profileForLaunch } from '../connect/harnessProfiles.ts'
import { SettingsPanel, backendModeMessage, type SettingsPanelHarnessStatus } from './SettingsPanel.tsx'
import { autostartRefusalNotice } from './autostartCopy.ts'
import { createAutostartQueue, type AutostartQueue } from './autostartQueue.ts'
import {
  createProbeScheduler,
  createSettingsProbeController,
  settingsProbeErrorMessage,
  DISPLAY_LIST_FALLBACK_INTERVAL_MS,
  PAGE_PROBES,
  type SettingsPage,
  type SettingsProbe,
} from './settingsProbes.ts'
import { chooseAppearanceImportPaths, nativeAppearance } from '../native/appearance.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
import { useUpdate } from '../features/update/useUpdate.ts'
import { formatSentence, msg, sentenceOf, t, useLanguage, useStoredLanguage, type Sentence } from '../i18n/index.ts'
import './SettingsWindow.css'


/**
 * 缩放方向。`@tauri-apps/api` 把 `ResizeDirection` 声明成模块内局部类型、并不导出，
 * 所以从 API 自身推导，避免抄一份联合类型再跟它漂移。
 */
type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>['startResizeDragging']>[0]

/**
 * 提示条上的那一条：**内容与语气一起**存。
 *
 * 这里原来靠一条正则去猜"这条提示是不是失败"（`/失败|错误|拒绝|无法|不允许/`）来决定它停留
 * 8200 还是 4200 毫秒。句子搬进词条之后，那个猜法在英文下立刻失效 —— 文字已经不是那几个字了。
 * 语气本来就是**调用点知道的事实**（它就是"这一步失败了"这件事本身），所以由调用点给出来，
 * 不再从渲染出来的文字里反推。
 */
interface SettingsNotice {
  /**
   * 要显示的那句话，**没求值**：我们自己写的句子是 `Message`（渲染期求值 → 切语言会重译），
   * 少数几句是原生返回的自由文本（`string`，本批不翻译）。
   */
  text: Sentence
  failure: boolean
}

/**
 * 缩放边条：窗口的非客户区已被去掉（原生 `WM_NCCALCSIZE` 归零），Windows 因此不再提供
 * "拖边改大小"的命中区；而 WebView2 渲染窗口属于另一个进程、铺满整个客户区，鼠标命中测试
 * 归它，父窗口的 `WM_NCHITTEST` 永远不会被问到。所以缩放必须由**渲染层**发起，
 * 与标题栏拖动（`start_settings_drag` → `startDragging`）走同一条路。
 *
 * 只在贴边 6 逻辑像素（角上 12）生效，与原生窗口的缩放带手感一致；
 * 中间区域完全不遮挡，面板里的控件照常点击。
 */
const RESIZE_HANDLES: Array<[ResizeDirection, string]> = [
  ['North', 'n'],
  ['South', 's'],
  ['West', 'w'],
  ['East', 'e'],
  ['NorthWest', 'nw'],
  ['NorthEast', 'ne'],
  ['SouthWest', 'sw'],
  ['SouthEast', 'se'],
]

function ResizeHandles() {
  return <>
    {RESIZE_HANDLES.map(([direction, suffix]) => (
      <div
        key={direction}
        className={`settings-resize settings-resize--${suffix}`}
        aria-hidden="true"
        onPointerDown={(event) => {
          if (event.button !== 0) return
          event.preventDefault()
          // 失败不必打扰用户：窗口没在缩放态是视觉问题，不是数据问题。
          void getCurrentWindow().startResizeDragging(direction).catch(() => undefined)
        }}
      />
    ))}
  </>
}


export function SettingsWindow() {
  // 这个窗口的 JSX 里有词条（提示条的关闭按钮），所以语言一变就要重渲染一次。
  useLanguage()
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [page, setPage] = useState<SettingsPage>('general')
  const [harness, setHarness] = useState<SettingsPanelHarnessStatus>('offline')
  const [interactionEnabled, setInteractionEnabled] = useState(true)
  const [harnessTargets, setHarnessTargets] = useState<HarnessTarget[]>([])
  /**
   * When the shown subject list was last confirmed by a real scan. Kept so the card
   * can say how old it is: a cached list must not look current.
   */
  const [catalogVerifiedAt, setCatalogVerifiedAt] = useState<number>()
  // 本机有没有 TUI：由扫描结果带回来，决定要不要给出「终端里的 TUI」这个选项。
  const [tuiAvailable, setTuiAvailable] = useState(false)
  // 与壁纸窗口同一条规矩：语言由设置文档推导，两个窗口都跟得上。
  useStoredLanguage(settings.language)
  const [managedDsh, setManagedDsh] = useState<ManagedDshStatus>({ instances: [], managed: false, running: false })
  /**
   * 停止/刷新是否在飞。
   *
   * 与 `managedDsh` 分开，因为按钮的可用性要跟**动作**走而不是跟状态走：停止一个实例要起
   * taskkill 并等它结束，那几百毫秒里再点一次不该发出第二条命令。
   *
   * 它喂给卡片底部的「停止本应用启动的 DSH」与它旁边的「刷新」（同一个 `stopManagedInstance`），
   * 用来挡住重复点击。
   */
  const [managedDshBusy, setManagedDshBusy] = useState(false)
  const [desktopDisplays, setDesktopDisplays] = useState<DesktopDisplayInfo[]>([])
  const [autostartBusy, setAutostartBusy] = useState(false)
  /**
   * The real Windows autostart state, kept separately from the boolean the user
   * toggles. "Start DSH with the wallpaper" is only a wallpaper-start trigger,
   * so the card needs the actual source (`none`, disabled by the user, disabled
   * by policy) to say whether a login-time start will really happen.
   */
  // `reason: null` with source `none` is the state Rust never produces: it is
  // this page's placeholder until the first read comes back, and the row says
  // so instead of showing an unchecked switch as a fact.
  const [autostartState, setAutostartState] = useState<AutostartStatus>({ enabled: false, source: 'none', reason: null })
  /**
   * Endpoint discovery result. `endpointScanDone` is deliberately separate from
   * an empty list: "not scanned yet" and "scanned and found nothing" need
   * different wording, and the user's rule is that the first scan is mandatory.
   */
  const [endpointScan, setEndpointScan] = useState<HarnessEndpointScan[]>([])
  const [endpointScanBusy, setEndpointScanBusy] = useState(false)
  const [endpointScanDone, setEndpointScanDone] = useState(false)
  const [reachBusy, setReachBusy] = useState(false)
  const [dshScanBusy, setDshScanBusy] = useState(false)
  const [notice, setNotice] = useState<SettingsNotice>()
  const [appearanceAssets, setAppearanceAssets] = useState<AppearanceAssetSummary[]>([])
  const [appearanceOverrides, setAppearanceOverrides] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [appearanceBusy, setAppearanceBusy] = useState(false)
  const [deepseekWebAdapterConfig, setDeepseekWebAdapterConfig] = useState<DeepSeekWebAdapterConfigStatus>()
  const [apiHistory, setApiHistory] = useState<ApiConversationListing>()
  const [apiHistoryBusy, setApiHistoryBusy] = useState(false)
  // 访问密钥：输入框里是**待保存**的草稿，脱敏串是**已保存**的那一条。
  // 两者刻意分开：草稿一旦保存就清空，免得"输入框里还留着 Key、看起来像没生效"。
  const [apiKeyDraft, setApiKeyDraft] = useState('')
  const [apiKeyStatus, setApiKeyStatus] = useState<ApiKeyStatus>()
  const [apiKeyBusy, setApiKeyBusy] = useState(false)
  // 可用模型列表：由「测试」/「刷新」拉取，也用于「模型」那一栏的下拉候选。
  const [apiModelCatalog, setApiModelCatalog] = useState<Array<{ id: string; name: string }>>()
  /** 上面那份列表是什么时候拉到的（来自持久化缓存）。 */
  const [apiModelCatalogFetchedAt, setApiModelCatalogFetchedAt] = useState<string>()
  /** 「桌面会话」工作区路径（原生只读自检；系统页显示，方便核对）。 */
  const [desktopWorkspace, setDesktopWorkspace] = useState<DesktopWorkspaceStatus>()
  /**
   * 更新检测（`features/update/`，计划书 §四）：系统页那张卡片的报告、忙碌标志与那行提示。
   *
   * 报告来自原生 `update_check`（只有码与数字），文案由 `features/update/updateCopy.ts` 说。
   * 探针的实现是 `useMemo(…, [])`，所以下面那个 runner 通过 ref 拿"当下这一个"控制器
   * （与 `settingsRef` 同一条理由）。
   */
  const update = useUpdate()
  const updateRef = useRef(update)
  updateRef.current = update
  /** 正在打开「项目记忆」：桌面会话里不贴路径，改它的入口只在这里。 */
  const [openingMemory, setOpeningMemory] = useState(false)
  /**
   * 壁纸**此刻**在用的 chat 模式。
   *
   * 与 `settings.defaultBackend`（启动时的默认值）分开：托盘菜单、`autoSwitchHarness`、以及壁纸
   * 自己在主体退出时的复位都会改这个值，而它们都不会回头改设置。开关显示的是这个，才等于显示事实。
   */
  const [liveBackend, setLiveBackend] = useState<BackendMode>()
  // A state update does not become visible to an async callback until React
  // renders again. Keep the last committed settings here so a successful
  // async request never overwrites unrelated settings changed while it
  // was in flight.
  const settingsRef = useRef(settings)
  const autostartOperationRef = useRef(false)
  /**
   * What the autostart switch currently stands for: the user's latest request
   * while one is on the wire, otherwise the state Windows last reported. A
   * toggle only travels to Rust when it differs from this, so a refused or
   * superseded change can always be asked for again.
   */
  const latestAutostartRef = useRef(settings.autostart)
  /**
   * The shape of the subject the reach action reaches.
   *
   * Derived from the configured subject first, never from "which client happens to be
   * ready": reaching a client the user did not choose is the same substitution the
   * connection path forbids. It comes from the subject because the shape decides *how*
   * the interface is reached and has to be known even while nothing answers.
   */
  const reachKind: HarnessClientKind = subjectClientKind(settings.dshLaunch.subjectId ?? settings.dshLaunch.rootPath)
    ?? endpointScan.find((item) => item.port === settings.dshLaunch.endpointPort)?.kind
    ?? 'official-web'
  const dshScanOperationRef = useRef(false)
  const apiHistoryOperationRef = useRef(false)
  // Probes must never outlive this window: `hide_settings_window` keeps the
  // WebView alive, but a reload or a real teardown would otherwise let a late
  // result update an unmounted component.
  const mountedRef = useRef(true)

  /** 一条"说事"的提示：走完了一步、看到了什么、接下来会怎样。 */
  const showNotice = (text: Sentence) => setNotice({ text, failure: false })
  /** 一条失败提示：停留时间长一些，因为它通常带着下一步（去哪看、改什么）。 */
  const showFailure = (text: Sentence) => setNotice({ text, failure: true })

  useEffect(() => {
    // Push the stored endpoint scope on mount, not only when the user changes it.
    // The native monitor holds its own endpoint state, so without this a saved
    // choice was only honoured after the user touched the control again — and
    // this surface has a separate storage partition from the background one, so
    // it cannot assume the other already pushed it. The subject travels with it:
    // that is what stops the monitor from probing another client's port.
    void nativeRuntime.setHarnessEndpointScope(endpointScopeOf(settingsRef.current.dshLaunch))
      .catch(() => null)
  }, [])

  const commitSettings = (next: WallpaperSettings) => {
    settingsRef.current = next
    setSettings(next)
    saveSettings(next)
    // The native monitor owns the probe loop, so an endpoint change has to be
    // pushed explicitly; publishing settings alone would leave the rendered
    // status coming from the previous port — or, worse, from another subject.
    void nativeRuntime.setHarnessEndpointScope(endpointScopeOf(next.dshLaunch)).catch(() => null)
    // Every Tauri WebView owns an isolated browser storage partition. Route
    // settings through Rust so this renderer cannot emit to arbitrary Tauri
    // event targets; Rust delivers the snapshot only to the background host.
    void invoke('publish_settings', { settings: next })
      .catch((error) => showFailure(msg('settings.window.notice.sync-failed', { error: sentenceOf(error) })))
  }

  useEffect(() => {
    if (!notice) return
    // Notices are transient feedback, not a second persistent UI layer.
    // Failures stay long enough to read; every notice still has a keyboard-
    // and pointer-accessible dismissal control below.
    const delay = notice.failure ? 8200 : 4200
    const timer = window.setTimeout(() => setNotice(undefined), delay)
    return () => window.clearTimeout(timer)
  }, [notice])

  useEffect(() => {
    // 打开设置就要看到"现在存的是哪一条"（脱敏），以及**上次拉到的模型列表**。
    // 后者是持久化的缓存，所以不必每次开窗都重新拉一次网络（用户实测报过"下次仍旧需要重新刷新"）。
    // 地址变过就不认这份缓存——它是从旧地址拉来的。
    const cached = settingsRef.current.deepseekApi.modelCatalog
    if (cached && cached.baseUrl === settingsRef.current.deepseekApi.baseUrl) {
      setApiModelCatalog(cached.models)
      setApiModelCatalogFetchedAt(cached.fetchedAt)
    }
    void readApiKeyStatus()
    // 只读自检：把「桌面会话」落在哪儿读出来（位置规则与桥一致）。失败不打扰用户——
    // 系统页会显示"正在读取…"，比弹一条不知所云的错误好。
    void nativeRuntime.desktopWorkspaceStatus()
      .then((status) => { if (mountedRef.current) setDesktopWorkspace(status) })
      .catch(() => undefined)
  }, [])

  const refreshAppearance = () => void Promise.all([nativeAppearance.getState(), nativeAppearance.listAssets()])
    .then(([snapshot, assets]) => {
      if (!mountedRef.current) return
      setAppearanceOverrides(snapshot.overrides)
      setAppearanceAssets(assets)
    })
    .catch((error) => showFailure(msg('settings.window.notice.appearance-read-failed', { error: sentenceOf(error) })))

  /**
   * The manual scan is the *only* place that walks the disk for DSH projects, and
   * the only place that reads the shell registrations. `deepScan` is requested
   * explicitly here and never on window open.
   *
   * One command fills both answers — the subjects to choose from, and (through the
   * subject model) the checkout the root-path field describes — so there is no
   * second walk of the same disk that could disagree with this one.
   */
  const scanDsh = async (announce = false) => {
    if (dshScanOperationRef.current) return
    dshScanOperationRef.current = true
    setDshScanBusy(true)
    try {
      const scan = await nativeRuntime.scanHarnessTargets(settingsRef.current.dshLaunch.rootPath, announce)
      if (!mountedRef.current) return
      setHarnessTargets(scan.targets)
      setTuiAvailable(scan.tuiAvailable)
      // The scan that just finished is the verification this list now carries.
      setCatalogVerifiedAt(Date.now())
      // 设置里存着的主体可能已经**不再受支持**（第三方客户端 2026-09-27 被移除）：那就落回官方
      // 桌面客户端，并把原因当场说出来 —— 既不静默改设置，也不让用户对着一个永远点不亮的灯猜。
      const fallback = unsupportedShellSubjectFallback(settingsRef.current.dshLaunch.subjectId)
      if (fallback) {
        change({ ...settingsRef.current, dshLaunch: { ...settingsRef.current.dshLaunch, subjectId: fallback.subjectId } })
        showNotice(fallback.notice)
      } else if (staleEndpointPort(settingsRef.current.dshLaunch) !== undefined) {
        // 存量的自相矛盾：显式端点是**上一个主体的**端口。留着它，"用户 pin 优先"这条正确的规则
        // 就会按旧主体的端口去开新主体的界面（实测：主体是只该用 3080 的已安装 CLI，pin 还是官方
        // 客户端的 19387，点「打开」把官方客户端的窗口拉到了前台）。清掉并说出来。
        const stale = staleEndpointPort(settingsRef.current.dshLaunch)!
        change({ ...settingsRef.current, dshLaunch: { ...settingsRef.current.dshLaunch, endpointPort: undefined } })
        showNotice(msg('settings.window.endpoints.stale-port', { port: stale }))
      } else if (announce) {
        showNotice(subjectChoicePrompt(scan.targets) ?? (scan.targets.length > 0
          ? msg('settings.window.scan.done', { count: scan.targets.length })
          : msg('settings.window.scan.none')))
      }
    } catch (error) {
      showFailure(msg('settings.window.scan.failed', { error: sentenceOf(error) }))
    } finally {
      dshScanOperationRef.current = false
      setDshScanBusy(false)
    }
  }


  /**
   * Show what the last scan confirmed, without walking the disk on open.
   *
   * The scan itself stays manual (§4.1). This only avoids the opposite mistake: a
   * settings window that shows nothing until the user scans again would hide subjects
   * the wallpaper already knows how to start.
   */
  useEffect(() => {
    void (async () => {
      const catalog = await nativeRuntime.harnessTargetCatalog().catch(() => null)
      if (!mountedRef.current || !catalog) return
      setHarnessTargets(catalog.targets)
      setTuiAvailable(catalog.tuiAvailable)
      setCatalogVerifiedAt(catalog.verifiedAtMs)
    })()
  }, [])

  const refreshDesktopDisplays = async () => {
    try {
      const displays = await nativeRuntime.desktopDisplays()
      if (mountedRef.current) setDesktopDisplays(displays)
    } catch (error) {
      showFailure(msg('settings.window.notice.displays-read-failed', { error: sentenceOf(error) }))
    }
  }

  /**
   * Scan every known endpoint. The user's own extra ports are passed through so
   * a non-standard client is not silently excluded from the list.
   */
  const scanEndpoints = async () => {
    if (endpointScanBusy) return
    setEndpointScanBusy(true)
    try {
      const found = await nativeRuntime.scanHarnessEndpoints(settingsRef.current.dshLaunch.extraEndpointPorts ?? [])
      if (!mountedRef.current) return
      setEndpointScan(found)
      setEndpointScanDone(true)
      // Pushing the current choice to the native monitor is what makes the subject
      // affect the status the desktop renders, rather than only the label in this card.
      await nativeRuntime.setHarnessEndpointScope(endpointScopeOf(settingsRef.current.dshLaunch)).catch(() => null)
      const bridges = found.filter((item) => item.bridgeFound)
      if (bridges.length === 0) {
        showNotice(msg('settings.window.endpoints.none'))
      } else if (bridges.every((item) => item.status.availability !== 'bridge-ready')) {
        showNotice(msg('settings.window.endpoints.unavailable', { count: bridges.length }))
      } else {
        showNotice(msg('settings.window.endpoints.found', { count: bridges.length }))
      }
    } catch (error) {
      showFailure(msg('settings.window.endpoints.failed', { error: sentenceOf(error) }))
    } finally {
      if (mountedRef.current) setEndpointScanBusy(false)
    }
  }

  /**
   * Reach the selected client's interface.
   *
   * One action, because the user's intent is one thing — "show me that client" —
   * and the states behind it (not running / hidden / backgrounded) are native's to
   * distinguish. Only the *last* step splits by client shape, because it genuinely
   * differs: a desktop client owns a window to bring forward, while the CLI/webui
   * shape has no window and its interface is the browser at the endpoint it
   * listens on. The design freezes the browser as that shape's only window choice.
   */
  const reachClient = async () => {
    const current = settingsRef.current.dshLaunch
    const subjectId = current.subjectId ?? current.rootPath
    // The subject's own ports, in its own order — never "whatever the scan found
    // answering", which would reach a client the user did not choose. 「启动参数」里的
    // `--port` **属于这个主体**（我们就是这样启动它的），所以它排在最前面：并行实例靠它让
    // 浏览器走到 3081，而不是默认的 3080。
    const ports = subjectEndpointPorts(endpointScopeOf(current)) ?? []
    // The user's pin wins; otherwise native decides from the subject itself, which
    // prefers a port that is actually listening. Passing the subject's first port
    // unconditionally would start a second instance of a checkout the user moved.
    const port = current.endpointPort ?? (ports.length === 1 ? ports[0]! : 0)
    const kind = subjectClientKind(subjectId) ?? reachKind
    setReachBusy(true)
    try {
      // §5.2: one idempotent action covers all three states — the subject may not be
      // running, may be running with the window this wallpaper hid at startup, or may
      // just be behind another window. Native decides which, and starts the subject
      // itself when nothing answers, so the caller never has to branch on that.
      const ensured = await nativeRuntime.ensureHarnessUi({
        targetId: subjectId,
        port,
        profile: profileForLaunch(),
      })
      // A start that failed is the actionable half: it names what to fix.
      if (ensured.started && ensured.outcome === 'not-running') {
        showFailure(launchOutcomeNotice({ outcome: ensured.startOutcome ?? 'spawn-failed', kind: ensured.kind })
          ?? t('settings.window.reach.start-failed'))
        return
      }
      if (reachNeedsBrowser(ensured.outcome)) {
        // No window exists for this shape; the browser is its interface, and it is
        // now confirmed to be answering (this action started it if it was not). The
        // address is resolved among the subject's own ports, because 0 above
        // deliberately left the choice of port to native.
        const live = port > 0
          ? port
          : (await nativeRuntime.scanHarnessEndpoints([...ports])).find((item) => item.bridgeFound)?.port
        if (!live) {
          showNotice(msg('settings.window.reach.no-port'))
          return
        }
        // 门票是**按端口**存的（`known_web_handoff(port)`）。
        await nativeRuntime.openClientInBrowser(live)
        showNotice(msg('settings.window.reach.browser-opened', { port: live }))
        return
      }
      const outcomeNotice = raiseOutcomeNotice(ensured.outcome, kind)
      if (outcomeNotice) showNotice(outcomeNotice)
      else if (ensured.outcome === 'raised') showNotice(msg('settings.window.reach.raised', { label: endpointKindMessage(kind) }))
      else showNotice(msg('settings.window.reach.restored', { label: endpointKindMessage(kind) }))
    } catch (error) {
      showFailure(msg('settings.window.reach.failed', { error: sentenceOf(error) }))
    } finally {
      if (mountedRef.current) setReachBusy(false)
    }
  }

  /**
   * 停掉本应用启动的实例。
   *
   * 一个动作、一个实现：`stop_managed_dsh` 只多了一个可选参数。这里刻意**不做**"先乐观地把
   * 那一行藏起来"：停止成不成功由原生回答，界面只做它说的那一件事 —— 否则一次失败会留下
   * "已经停掉了"的假象，而那个假象比一行红字更坏。
   *
   * `instanceKey` 是原生契约（`stop_managed_dsh` 按实例停止）留下的可选参数；现役界面只走不给
   * instanceKey 的那条路 —— 卡片底部的「停止本应用启动的 DSH」，语义是停全部。
   */
  const stopManagedInstance = async (instanceKey?: string) => {
    setManagedDshBusy(true)
    try {
      await nativeRuntime.stopManagedDsh(instanceKey)
      showNotice(instanceKey === undefined ? msg('settings.window.managed.stopped-all') : msg('settings.window.managed.stopped-one'))
    } catch (error) {
      showFailure(sentenceOf(error))
    } finally {
      if (mountedRef.current) setManagedDshBusy(false)
      // 成功失败都刷一次：成功了那一行该消失，失败了清单也该说出**现在**的真相（也许它本来就
      // 已经退了，或已经被别的程序接管）。
      refreshManagedDsh()
    }
  }

  const refreshAutostartStatus = async () => {
    if (!nativeRuntime.isNative) return
    try {
      const status: AutostartStatus = await nativeRuntime.autostartStatus()
      const current = settingsRef.current
      if (!mountedRef.current) return
      setAutostartState(status)
      // A toggle is on the wire: its read-back is newer than this probe's, so
      // this one must not write the older value into the switch.
      if (autostartOperationRef.current) return
      latestAutostartRef.current = status.enabled
      if (current.autostart !== status.enabled) {
        const next = { ...current, autostart: status.enabled }
        settingsRef.current = next
        setSettings(next)
        saveSettings(next)
        void invoke('publish_settings', { settings: next }).catch((error) => showFailure(msg('settings.window.notice.sync-failed', { error: sentenceOf(error) })))
      }
      if (status.source === 'disabled-by-user') showNotice(msg('settings.window.autostart.disabled-by-user'))
      if (status.source === 'disabled-by-policy') showNotice(msg('settings.window.autostart.disabled-by-policy'))
    } catch (error) {
      showFailure(msg('settings.window.autostart.read-failed', { error: sentenceOf(error) }))
    }
  }

  /**
   * 把某个主体的桥对齐到我们钉住的那一版。
   *
   * 版本相符时这条命令在 CLI 那边是**空操作**（同一个包同一个版本再 add 一次），所以它同时
   * 覆盖两个触发点：换主体时、以及设置窗口每次打开时（用户批准的就是"每次启动检查一次"）。
   * 结果只经 summarizeBridgeInstall 说一句：需要用户确认版本豁免的那种情况，决定权在他手里。
   */
  const ensureBridgeFor = async (subjectId: string) => {
    const profile = settingsRef.current.dshLaunch.profile ?? 'web'
    try {
      const outcomes = await nativeRuntime.ensureProfileBridge(subjectId, profile)
      const summary = summarizeBridgeInstall(outcomes)
      if (summary) showNotice(summary.text)
    } catch (error) {
      showFailure(msg('settings.window.bridge.failed', { error: sentenceOf(error) }))
    }
  }

  // 只在**主体真的换了**（或首次挂载）时动手，而不是每次渲染。
  const bridgedSubjectRef = useRef<string>()
  useEffect(() => {
    const subjectId = settings.dshLaunch.subjectId
    if (!subjectId || bridgedSubjectRef.current === subjectId) return
    bridgedSubjectRef.current = subjectId
    void ensureBridgeFor(subjectId)
  }, [settings.dshLaunch.subjectId])

  /**
   * One runner per probe. The controller decides *when* a runner may start;
   * it never lets two runs of the same probe overlap, and it reports a failed
   * probe as a notice instead of letting the window lose its interactivity.
   */
  const probeControllerHolder = useRef<{ refresh: (probe: SettingsProbe) => Promise<void> }>()
  const probeRunners = useMemo<Record<SettingsProbe, () => Promise<unknown>>>(() => ({
    managedDsh: async () => {
      const status = await nativeRuntime.managedDshStatus()
      if (mountedRef.current) setManagedDsh(status)
    },
    deepseekWebAdapterConfig: async () => {
      const status = await nativeRuntime.deepseekWebAdapterConfig()
      if (mountedRef.current) setDeepseekWebAdapterConfig(status)
    },
    autostartStatus: refreshAutostartStatus,
    desktopDisplays: refreshDesktopDisplays,
    // The history listing is a management view, not a live surface: it is read
    // when the page opens and on explicit refresh, never polled.
    apiHistory: async () => {
      const listing = await nativeRuntime.listApiConversations()
      if (mountedRef.current) setApiHistory(listing)
    },
    /**
     * 更新检测：用户主动打开系统页，所以这一次走**手动**检查（不受原生侧 6 小时节流限制，
     * §四"手动检查不受频率限制"）。于是卡片上一定有一个真结论 —— "已是最新" / "有新版本 x" /
     * 失败原因 —— 而不是一行"本次跳过"，也不需要为"读上次结果"再加一条命令。
     * 卡片上那枚「检查更新」按钮重复的就是这一次（`checkForUpdates`）。
     */
    updateStatus: async () => {
      await updateRef.current.check(true)
    },
  }), [])

  const probeController = useMemo(() => createSettingsProbeController({
    runProbe: (probe) => probeRunners[probe](),
    schedule: createProbeScheduler(window),
    onError: (probe, error) => showFailure(settingsProbeErrorMessage(probe, error)),
  }), [probeRunners])
  probeControllerHolder.current = probeController

  /**
   * Manual refresh buttons run a fresh probe. They must not be silently
   * swallowed by the "already probed" bookkeeping, and they must not start a
   * second run while the first one is still in flight.
   */
  const refreshProbe = (probe: SettingsProbe) => void probeControllerHolder.current?.refresh(probe)
  const refreshManagedDsh = () => refreshProbe('managedDsh')
  const refreshDeepSeekWebAdapterConfig = () => refreshProbe('deepseekWebAdapterConfig')
  const openDeepSeekWebAdapterConfig = async () => {
    try {
      setDeepseekWebAdapterConfig(await nativeRuntime.openDeepSeekWebAdapterConfig())
      showNotice(msg('settings.window.adapter.opened'))
    } catch (error) {
      showFailure(msg('settings.window.adapter.open-failed', { error: sentenceOf(error) }))
    }
  }
  const resetDeepSeekWebAdapterConfig = async () => {
    if (!window.confirm(t('settings.window.adapter.reset-confirm'))) return
    try {
      setDeepseekWebAdapterConfig(await nativeRuntime.resetDeepSeekWebAdapterConfig())
      showNotice(msg('settings.window.adapter.reset-done'))
    } catch (error) {
      showFailure(msg('settings.window.adapter.reset-failed', { error: sentenceOf(error) }))
    }
  }

  /**
   * 访问密钥的三件事，都在设置中心里完成：
   *
   * - 「测试」：拿输入框里的（没有就用已保存的）去调 `/models`。**这一步同时就是"拉取可用
   *   模型列表"**——DeepSeek 的 `/models` 需要密钥，200 就等于"密钥可用"，而它的返回体正是
   *   模型目录，不需要第二次请求（用户要求"可用模型列表也在此刻拉取更新"）。
   * - 测试成功时把输入框里的 Key 存进凭据管理器：能通过测试的才值得保存。
   * - 「刷新」：只重拉模型目录，不动密钥——官方改名时用它（用户原话："防止官方的模型命名又有变动"）。
   */
  const readApiKeyStatus = async () => {
    try {
      const status = await nativeRuntime.apiKeyStatus()
      if (mountedRef.current) setApiKeyStatus(status)
    } catch (error) {
      if (mountedRef.current) showFailure(msg('settings.window.api-key.read-failed', { error: sentenceOf(error) }))
    }
  }

  const refreshApiModelCatalog = async (): Promise<boolean> => {
    try {
      const baseUrl = settingsRef.current.deepseekApi.baseUrl
      const catalog = await nativeRuntime.apiModels(baseUrl)
      if (!mountedRef.current) return true
      if (!catalog.supported) {
        showNotice(msg('settings.window.api-key.models-unsupported'))
        return false
      }
      const models = catalog.models ?? []
      setApiModelCatalog(models)
      // **持久化**这次拉取的结果：用户实测报过"刷新结果也没有持久化，下次仍旧需要重新刷新"。
      // 缓存带上地址与时间：地址一换就作废，界面上也能说明它是什么时候拉来的。
      commitSettings({
        ...settingsRef.current,
        deepseekApi: {
          ...settingsRef.current.deepseekApi,
          modelCatalog: { baseUrl, models, fetchedAt: new Date().toISOString() },
        },
      })
      return true
    } catch (error) {
      if (mountedRef.current) showFailure(msg('settings.window.api-key.models-read-failed', { error: sentenceOf(error) }))
      return false
    }
  }

  const testApiKey = async () => {
    if (apiKeyBusy) return
    const draft = apiKeyDraft.trim()
    if (!draft && !apiKeyStatus?.present) {
      showNotice(msg('settings.window.api-key.missing'))
      return
    }
    setApiKeyBusy(true)
    try {
      if (draft) {
        await nativeRuntime.saveApiKey(draft)
        setApiKeyDraft('')
      }
      const ok = await refreshApiModelCatalog()
      if (!ok) return
      // 目录里那份 `fetchedAt` 是刚写进设置的，用它把"上次拉取"的说明一并刷新。
      const catalog = settingsRef.current.deepseekApi.modelCatalog
      if (catalog) setApiModelCatalogFetchedAt(catalog.fetchedAt)
      await readApiKeyStatus()
      showNotice(draft ? msg('settings.window.api-key.saved') : msg('settings.window.api-key.usable'))
    } catch (error) {
      showFailure(msg('settings.window.api-key.save-failed', { error: sentenceOf(error) }))
    } finally {
      if (mountedRef.current) setApiKeyBusy(false)
    }
  }

  /**
   * 只刷新目录：密钥不动，因此也不需要"有没有 Key"这一层判断——`/models` 自己会回答。
   */
  const refreshApiModels = async () => {
    if (apiKeyBusy) return
    setApiKeyBusy(true)
    try {
      const baseUrl = settingsRef.current.deepseekApi.baseUrl
      if (await refreshApiModelCatalog()) {
        const catalog = settingsRef.current.deepseekApi.modelCatalog
        if (catalog?.baseUrl === baseUrl) setApiModelCatalogFetchedAt(catalog.fetchedAt)
        showNotice(msg('settings.window.api-key.models-refreshed'))
      }
    } finally {
      if (mountedRef.current) setApiKeyBusy(false)
    }
  }

  /**
   * 切换 chat 模式，**并让正在运行的壁纸立刻生效**。
   *
   * 两件事一起做，因为它们本来就是一件事的两面：
   * 1. `select_backend`（原生 AppCore）——背景端就是靠 `app-snapshot` 改 `runtime.backend` 的，
   *    托盘菜单走的同一条路，所以这里不需要第二条通路；
   * 2. 写进 `settings.defaultBackend`——它同时是**下次启动**的默认值。
   *
   * 不做"先存后切"两步：用户点的是"现在用哪个"，而不是"以后用哪个"。
   */
  const selectBackend = async (backend: BackendMode) => {
    commitSettings({ ...settingsRef.current, defaultBackend: backend })
    try {
      const snapshot = await appCoreClient.selectBackend(backend)
      if (mountedRef.current) setLiveBackend(snapshot.backend)
      showNotice(msg('settings.window.backend.switched', { label: backendModeMessage(backend) }))
    } catch (error) {
      showFailure(msg('settings.window.backend.failed', { error: sentenceOf(error) }))
    }
  }

  /**
   * Deleting bumps the shared asset and the durable API transcript at the same
   * time; `bumpRefreshEpoch` is not involved, so nothing else resets. These
   * operations read and rewrite the encrypted archive, so they share one busy
   * flag and a ref that closes the double-click window before the next render.
   */
  const refreshApiHistory = async () => {
    if (apiHistoryOperationRef.current) return
    apiHistoryOperationRef.current = true
    setApiHistoryBusy(true)
    try {
      const listing = await nativeRuntime.listApiConversations()
      if (mountedRef.current) setApiHistory(listing)
    } catch (error) {
      if (mountedRef.current) showFailure(settingsProbeErrorMessage('apiHistory', error))
    } finally {
      apiHistoryOperationRef.current = false
      if (mountedRef.current) setApiHistoryBusy(false)
    }
  }

  const deleteApiConversation = async (conversationId: string) => {
    if (apiHistoryOperationRef.current) return
    // Deleting a transcript is irreversible and cannot be undone from here.
    if (!window.confirm(t('settings.window.history.delete-confirm', { id: conversationId }))) return
    apiHistoryOperationRef.current = true
    setApiHistoryBusy(true)
    try {
      const removed = await nativeRuntime.deleteApiConversation(conversationId)
      showNotice(removed
        ? msg('settings.window.history.deleted', { id: conversationId })
        : msg('settings.window.history.gone'))
    } catch (error) {
      showFailure(msg('settings.window.history.delete-failed', { error: sentenceOf(error) }))
    } finally {
      apiHistoryOperationRef.current = false
      if (mountedRef.current) setApiHistoryBusy(false)
      // Re-read either way: a failed delete must not leave a stale row that
      // looks like it succeeded.
      await refreshApiHistory()
    }
  }

  const clearApiHistory = async () => {
    if (apiHistoryOperationRef.current) return
    const count = apiHistory?.conversations.length ?? 0
    if (!window.confirm(t('settings.window.history.clear-confirm', { count }))) return
    apiHistoryOperationRef.current = true
    setApiHistoryBusy(true)
    try {
      const cleared = await nativeRuntime.clearApiHistory()
      showNotice(cleared > 0
        ? msg('settings.window.history.cleared', { count: cleared })
        : msg('settings.window.history.nothing-to-clear'))
    } catch (error) {
      showFailure(msg('settings.window.history.clear-failed', { error: sentenceOf(error) }))
    } finally {
      apiHistoryOperationRef.current = false
      if (mountedRef.current) setApiHistoryBusy(false)
      await refreshApiHistory()
    }
  }

  // First paint owns no probe at all: only the local settings and the AppCore
  // snapshot (which the "显示中央会话窗" switch needs) are read up front. The
  // visible page's own data starts on the next frame.
  //
  // 这份快照里的 `backend` 就是"壁纸此刻在用哪种 chat 模式"，「聊天模式」那一栏显示它。
  // **只在打开窗口时读一次**：`app-snapshot` 是 Rust 定向发给背景端的事件
  // （`emit_to(BACKGROUND_WINDOW_LABEL)`），"系统事件不进设置 WebView"这条边界由
  // `tests/nativeChatBoundary.spec.ts` 钉着，不该为了一个显示值放宽它；本窗口自己切换后端时
  // 会用它那次调用的返回值就地更新（见 `selectBackend`）。
  useEffect(() => {
    if (!appCoreClient.native) return
    void appCoreClient.snapshot().then((snapshot) => {
      if (!mountedRef.current) return
      setHarness(snapshot.harness)
      setInteractionEnabled(snapshot.interaction.enabled)
      setLiveBackend(snapshot.backend)
    }).catch((error) => showFailure(sentenceOf(error)))
  }, [])

  // The single native event subscription this window owns. It is registered
  // without awaiting the native call, so a window that opens and closes
  // quickly can never leave a stray listener behind.
  useEffect(() => {
    const current = getCurrentWindow()
    let disposed = false
    let unsubscribe: (() => void) | undefined
    void current.onCloseRequested((event) => {
      event.preventDefault()
      void invoke('hide_settings_window')
    }).then((dispose) => {
      if (disposed) dispose()
      else unsubscribe = dispose
    }).catch((error) => showFailure(sentenceOf(error)))
    return () => {
      disposed = true
      unsubscribe?.()
      unsubscribe = undefined
    }
  }, [])

  // Grouped, once-only probes. Nothing here runs because the window opened;
  // each group starts the first time its page is actually shown.
  useEffect(() => {
    mountedRef.current = true
    probeController.activate(page)
    return () => { mountedRef.current = false }
  }, [page, probeController])

  // While settings is open, the monitor list is refreshed on a slow fallback
  // cadence only. The settings WebView deliberately does not subscribe to the
  // background's display events, and the page has an explicit "刷新显示器检测"
  // control, so a fast resident poll bought nothing but IPC traffic.
  useEffect(() => {
    if (!PAGE_PROBES[page].includes('desktopDisplays')) return
    const displayTimer = window.setInterval(() => { void refreshDesktopDisplays() }, DISPLAY_LIST_FALLBACK_INTERVAL_MS)
    return () => window.clearInterval(displayTimer)
  }, [page])

  // A reload or a real teardown cancels every probe that has not started yet.
  useEffect(() => () => probeController.dispose(), [probeController])

  /**
   * The autostart switch, serialized (see `createAutostartQueue`). Built on
   * first use: it captures the commit path, the notice channel and the mounted
   * guard, all of which exist by the time a user can move the switch.
   */
  const autostartQueueRef = useRef<AutostartQueue>()
  const autostartQueue = () => {
    if (!autostartQueueRef.current) {
      autostartQueueRef.current = createAutostartQueue({
        send: (enabled) => nativeRuntime.setAutostart(enabled),
        onBusy: (busy) => {
          autostartOperationRef.current = busy
          if (mountedRef.current) setAutostartBusy(busy)
        },
        onSettled: (status, requested) => {
          if (!mountedRef.current) return
          setAutostartState(status)
          // A newer toggle is already queued: its own read-back decides the
          // switch, and committing this older one would put the switch back
          // where the user just moved it from.
          if (latestAutostartRef.current !== requested) return
          latestAutostartRef.current = status.enabled
          commitSettings({ ...settingsRef.current, autostart: status.enabled })
          const refusal = autostartRefusalNotice(status, requested)
          if (refusal) showNotice(refusal)
        },
        onError: (error) => {
          if (mountedRef.current) showFailure(msg('settings.window.autostart.update-failed', { error: sentenceOf(error) }))
        },
      })
    }
    return autostartQueueRef.current
  }

  const change = (next: WallpaperSettings) => {
    const autostartChanged = next.autostart !== latestAutostartRef.current
    const normalNext = next
    if (autostartChanged) {
      // The switch follows the user at once; Windows' answer decides whether it
      // stays there. A second toggle is queued instead of dropped, and the
      // response of the request it replaces cannot move the switch back.
      latestAutostartRef.current = next.autostart
      commitSettings(normalNext)
      autostartQueue().request(next.autostart)
      return
    }
    commitSettings(normalNext)
  }

  const close = () => void invoke('hide_settings_window')
  /**
   * 更新那几个动作（计划书 §四）。全部交给同一个控制器：两个窗口（壁纸气泡 / 这里）与同一份
   * 原生状态打交道，动作也只有一份实现。失败与"没落盘"由控制器写进 `update.notice`，
   * 卡片就在原地把那句话显示出来 —— 不需要再弹一条窗口通知说同一件事。
   *
   * `downloadUpdate` 现在是**真的下载**（第三片）：进度从全局的 `update-download` 事件回来，
   * 所以在这里按下「下载」，壁纸上的气泡也会跟着走进度条。「打开发布页」留着做回落（§六）。
   */
  const checkForUpdates = async () => {
    await updateRef.current.check(true)
  }
  const downloadUpdate = () => { void updateRef.current.startDownload() }
  const installUpdate = () => { void updateRef.current.install() }
  const openUpdatePage = () => { void updateRef.current.openReleasePage() }
  const dismissUpdate = (version: string) => { void updateRef.current.dismiss(version) }
  const importAppearance = async () => {
    setAppearanceBusy(true)
    try {
      const paths = await chooseAppearanceImportPaths()
      if (paths.length === 0) return
      await nativeAppearance.importPaths(paths)
      refreshAppearance()
    } catch (error) { showFailure(msg('settings.window.appearance.import-failed', { error: sentenceOf(error) })) } finally { setAppearanceBusy(false) }
  }
  const classifyAppearance = async (assetId: string, slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { await nativeAppearance.classifyAsset(assetId, [slot]); refreshAppearance() } catch (error) { showFailure(msg('settings.window.appearance.classify-failed', { error: sentenceOf(error) })) } finally { setAppearanceBusy(false) }
  }
  const selectAppearance = async (slot: AppearanceSlot, assetId: string) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.setOverride(slot, assetId)).overrides); void invoke('notify_appearance_changed').catch((error) => showFailure(msg('settings.window.appearance.sync-failed', { error: sentenceOf(error) }))) } catch (error) { showFailure(msg('settings.window.appearance.apply-failed', { error: sentenceOf(error) })) } finally { setAppearanceBusy(false) }
  }
  const clearAppearance = async (slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.clearOverride(slot)).overrides); void invoke('notify_appearance_changed').catch((error) => showFailure(msg('settings.window.appearance.sync-failed', { error: sentenceOf(error) }))) } catch (error) { showFailure(msg('settings.window.appearance.reset-failed', { error: sentenceOf(error) })) } finally { setAppearanceBusy(false) }
  }

  return <main className="settings-window">
    <ResizeHandles />
    {notice && <div className="settings-window__notice" role="status"><span>{formatSentence(notice.text)}</span><button type="button" aria-label={t('settings.window.notice.dismiss')} onPointerDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); setNotice(undefined) }}>×</button></div>}
    <SettingsPanel
      settings={settings}
      page={page}
      onPageChange={(next) => { setPage(next); if (next === 'appearance') refreshAppearance() }}
      harnessStatus={harness}
      harnessTargets={harnessTargets}
      tuiAvailable={tuiAvailable}
      subjectCatalogVerifiedAt={catalogVerifiedAt}
      subjectChoice={subjectChoicePrompt(harnessTargets) ?? undefined}
      onSelectSubject={(targetId) => change({
        ...settingsRef.current,
        dshLaunch: {
          ...settingsRef.current.dshLaunch,
          subjectId: targetId,
          // 显式端点属于**上一个主体**：它是"用户当年为那个主体选的那个端口"，换主体后就是一条
          // 自相矛盾的选择（实测：主体是 3080 的已安装 CLI，pin 还是官方客户端的 19387，于是点
          // 打开把官方客户端的窗口拉到了前台）。用户的 pin 优先这条规则不动，但换主体必须清掉它。
          endpointPort: undefined,
          // 已安装的 CLI 既不是源码树也不是壳：它没有根目录可填（id 里的 `cli:` 是身份命名空间，
          // 不是路径），而它的档案是 dsh 自己那一个 —— web。沿用一个属于源码目录/官壳的 desktop
          // 会让它启动后立刻退出（实测：日志里 `--profile desktop`，而同一命令换 web 一切正常）。
          // 只在档案还是那个默认值时才替换；用户自己填过的档案一律不动。
          ...(targetId.startsWith(CLI_SUBJECT_PREFIX)
                  ? {
                      rootPath: undefined,
                    }
            // A checkout's id *is* its path, so keeping the root-path field in step
            // means the profile/launcher fields below still describe the same tree.
            // A shell keeps whatever path is there, so switching back is lossless.
            : targetId.startsWith('shell:') ? {} : { rootPath: targetId }),
        },
      })}
      endpointScan={endpointScan}
      endpointScanBusy={endpointScanBusy}
      endpointScanDone={endpointScanDone}
      onScanEndpoints={() => { void scanEndpoints() }}
      onOpenClient={() => { void reachClient() }}
      openBusy={reachBusy}
      reachAction={clientRaiseAction(reachKind)}
      autostart={autostartState}
      onScanDsh={() => { void scanDsh(true) }}
      dshScanBusy={dshScanBusy}
      managedDsh={managedDsh}
      managedDshBusy={managedDshBusy}
      onRefreshManagedDsh={refreshManagedDsh}
      onStopAllManagedDsh={() => { void stopManagedInstance() }}
      onOpenTui={() => void nativeRuntime.openSubjectTui().then((result) => {
        // 契约：没装 TUI 时原生返回 `opened: false` 与一句"怎么办"。**把那句显示出来**，
        // 绝不静默改成打开浏览器 —— 那等于替用户换了一条他没选的路。
        if (!result.opened) {
          showNotice(result.message ?? msg('settings.window.tui.missing'))
          return
        }
        showNotice(msg('settings.window.tui.launched'))
      }).catch((error) => showFailure(msg('settings.window.tui.failed', { error: sentenceOf(error) })))}
      onSelectWindow={(value) => change({
        ...settingsRef.current,
        dshLaunch: { ...settingsRef.current.dshLaunch, window: value },
      })}
      onChange={change}
      appearanceAssets={appearanceAssets}
      appearanceOverrides={appearanceOverrides}
      appearanceBusy={appearanceBusy}
      onImportAppearance={() => { void importAppearance() }}
      onClassifyAppearance={(assetId, slot) => { void classifyAppearance(assetId, slot) }}
      onSelectAppearance={(slot, assetId) => { void selectAppearance(slot, assetId) }}
      onClearAppearance={(slot) => { void clearAppearance(slot) }}
      autostartBusy={autostartBusy}
      desktopDisplays={desktopDisplays}
      onRefreshDesktopDisplays={refreshDesktopDisplays}
      apiHistory={apiHistory}
      apiHistoryBusy={apiHistoryBusy}
      onRefreshApiHistory={() => { void refreshApiHistory() }}
      onDeleteApiConversation={(conversationId) => { void deleteApiConversation(conversationId) }}
      onClearApiHistory={() => { void clearApiHistory() }}
      updateReport={update.report}
      updateDownload={update.download}
      updateBusy={update.busy}
      updateNotice={update.notice}
      onCheckForUpdates={() => { void checkForUpdates() }}
      onDownloadUpdate={downloadUpdate}
      onInstallUpdate={installUpdate}
      onOpenUpdatePage={openUpdatePage}
      onDismissUpdate={dismissUpdate}
      onRequestDeepSeekLogin={() => void nativeRuntime.requestDeepSeekLogin().catch((error) => showFailure(msg('settings.window.deepseek-web.open-failed', { error: sentenceOf(error) })))}
      deepseekWebAdapterConfig={deepseekWebAdapterConfig}
      onRefreshDeepSeekWebAdapterConfig={refreshDeepSeekWebAdapterConfig}
      onOpenDeepSeekWebAdapterConfig={() => { void openDeepSeekWebAdapterConfig() }}
      onResetDeepSeekWebAdapterConfig={() => { void resetDeepSeekWebAdapterConfig() }}
      apiKeyDraft={apiKeyDraft}
      onApiKeyDraftChange={setApiKeyDraft}
      apiKeyStatus={apiKeyStatus}
      apiKeyBusy={apiKeyBusy}
      onTestApiKey={() => { void testApiKey() }}
      onRefreshApiModels={() => { void refreshApiModels() }}
      apiModelCatalog={apiModelCatalog}
      apiModelCatalogFetchedAt={apiModelCatalogFetchedAt}
      liveBackend={liveBackend}
      onSelectBackend={(backend) => { void selectBackend(backend) }}
      interactionEnabled={interactionEnabled}
      onSetInteractionEnabled={(enabled) => void appCoreClient.setInteractionEnabled(enabled).then((snapshot) => setInteractionEnabled(snapshot.interaction.enabled)).catch((error) => showFailure(sentenceOf(error)))}
      desktopWorkspace={desktopWorkspace}
      onOpenProjectMemory={async () => {
        // 打开的是**文件所在的位置**：文件在就选中它，不在就把工作区目录打开（原生实现）。
        // 结果用那句 notice 如实回报，而不是让用户自己去猜窗口为什么没动。
        setOpeningMemory(true)
        try {
          const opened = await nativeRuntime.openProjectMemory()
          showNotice(opened.memoryExists ? msg('settings.window.memory.selected') : msg('settings.window.memory.opened-folder'))
        } catch (error) {
          showFailure(sentenceOf(error))
        } finally {
          setOpeningMemory(false)
        }
      }}
      openingMemory={openingMemory}
      onClose={close}
    />
  </main>
}
