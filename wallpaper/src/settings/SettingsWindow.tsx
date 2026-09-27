import { useEffect, useMemo, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import type { BackendMode } from '../domain/types.ts'
import { appCoreClient } from '../runtime/appCoreClient.ts'
import { nativeRuntime, type AutostartStatus, type ApiConversationListing, type ApiKeyStatus, type DeepSeekWebAdapterConfigStatus, type DesktopDisplayInfo, type DesktopWorkspaceStatus, type HarnessEndpointScan, type HarnessTarget, type LockScreenDiagnostics, type ManagedDshStatus, type TranslucentTbStatus } from '../native/runtime.ts'
import { loadSettings, saveSettings, type WallpaperSettings } from './store.ts'
import {
  clientRaiseAction,
  endpointKindLabel,
  endpointScopeOf,
  raiseOutcomeNotice,
  subjectClientKind,
  subjectEndpointPorts,
  type HarnessClientKind,
} from '../connect/endpoints.ts'
import { launchOutcomeNotice, reachNeedsBrowser, subjectChoicePrompt } from '../connect/harnessSubjects.ts'
import { SettingsPanel, backendModeLabel, type SettingsPanelHarnessStatus } from './SettingsPanel.tsx'
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
import './SettingsWindow.css'


/**
 * 缩放方向。`@tauri-apps/api` 把 `ResizeDirection` 声明成模块内局部类型、并不导出，
 * 所以从 API 自身推导，避免抄一份联合类型再跟它漂移。
 */
type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>['startResizeDragging']>[0]

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
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [page, setPage] = useState<SettingsPage>('general')
  const [harness, setHarness] = useState<SettingsPanelHarnessStatus>('offline')
  const [interactionEnabled, setInteractionEnabled] = useState(true)
  const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })
  const [harnessTargets, setHarnessTargets] = useState<HarnessTarget[]>([])
  /**
   * When the shown subject list was last confirmed by a real scan. Kept so the card
   * can say how old it is: a cached list must not look current.
   */
  const [catalogVerifiedAt, setCatalogVerifiedAt] = useState<number>()
  const [managedDsh, setManagedDsh] = useState<ManagedDshStatus>({ managed: false, running: false })
  const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()
  const [desktopDisplays, setDesktopDisplays] = useState<DesktopDisplayInfo[]>([])
  const [lockScreenBusy, setLockScreenBusy] = useState(false)
  const [autostartBusy, setAutostartBusy] = useState(false)
  /**
   * The real Windows autostart state, kept separately from the boolean the user
   * toggles. "Start DSH with the wallpaper" is only a wallpaper-start trigger,
   * so the card needs the actual source (`none`, disabled by the user, disabled
   * by policy) to say whether a login-time start will really happen.
   */
  const [autostartState, setAutostartState] = useState<AutostartStatus>({ enabled: false, source: 'none' })
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
  const [notice, setNotice] = useState<string>()
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
  // lock-screen request never overwrites unrelated settings changed while it
  // was in flight.
  const settingsRef = useRef(settings)
  const lockScreenOperationRef = useRef(false)
  const autostartOperationRef = useRef(false)
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
  const lockScreenDiagnosticsRequestRef = useRef(0)
  const apiHistoryOperationRef = useRef(false)
  // Probes must never outlive this window: `hide_settings_window` keeps the
  // WebView alive, but a reload or a real teardown would otherwise let a late
  // result update an unmounted component.
  const mountedRef = useRef(true)

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
      .catch((error) => setNotice(`设置同步失败：${String(error)}`))
  }

  useEffect(() => {
    if (!notice) return
    // Notices are transient feedback, not a second persistent UI layer.
    // Failures stay long enough to read; every notice still has a keyboard-
    // and pointer-accessible dismissal control below.
    const delay = /失败|错误|拒绝|无法|不允许/.test(notice) ? 8200 : 4200
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
    .catch((error) => setNotice(`素材库读取失败：${String(error)}`))

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
      // The scan that just finished is the verification this list now carries.
      setCatalogVerifiedAt(Date.now())
      if (announce) {
        setNotice(subjectChoicePrompt(scan.targets) ?? (scan.targets.length > 0
          ? `扫描完成，发现 ${scan.targets.length} 个可选执行主体。`
          : '未发现 DSH 项目或已安装的客户端；可手动填写 DSH 项目根目录后再扫描。'))
      }
    } catch (error) {
      setNotice(`扫描 DSH 失败：${String(error)}`)
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
      setCatalogVerifiedAt(catalog.verifiedAtMs)
    })()
  }, [])

  const refreshLockScreenDiagnostics = async () => {
    const request = ++lockScreenDiagnosticsRequestRef.current
    try {
      const diagnostics = await nativeRuntime.lockScreenDiagnostics()
      if (!mountedRef.current) return
      // An older initial/refresh request may finish after a successful
      // takeover or restore. Never let it replace the newer system result.
      if (request === lockScreenDiagnosticsRequestRef.current) setLockScreenDiagnostics(diagnostics)
    } catch (error) {
      if (request === lockScreenDiagnosticsRequestRef.current) setNotice(`锁屏检查失败：${String(error)}`)
    }
  }

  const refreshDesktopDisplays = async () => {
    try {
      const displays = await nativeRuntime.desktopDisplays()
      if (mountedRef.current) setDesktopDisplays(displays)
    } catch (error) {
      setNotice(`显示器列表读取失败：${String(error)}`)
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
        setNotice('未发现可接入的 Harness。请先启动任一个客户端官官方桌面 / 第三方桌面 / 官方 Web）后重新扫描。')
      } else if (bridges.every((item) => item.status.availability !== 'bridge-ready')) {
        setNotice(`发现 ${bridges.length} 个 Harness，但当前都不可对话；详情见端点下拉。`)
      } else {
        setNotice(`发现 ${bridges.length} 个可接入的 Harness。`)
      }
    } catch (error) {
      setNotice(`扫描接入端点失败：${String(error)}`)
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
    // answering", which would reach a client the user did not choose.
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
        profile: current.profile,
        command: current.command,
      })
      // A start that failed is the actionable half: it names what to fix.
      if (ensured.started && ensured.outcome === 'not-running') {
        setNotice(launchOutcomeNotice({ outcome: ensured.startOutcome ?? 'spawn-failed', kind: ensured.kind })
          ?? '启动失败，请查看日志中的启动记录。')
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
          setNotice('没有可打开的界面：主体没有在本机监听任何端口。')
          return
        }
        await nativeRuntime.openClientInBrowser(live)
        setNotice(`已在默认浏览器中打开 127.0.0.1:${live}。`)
        return
      }
      const notice = raiseOutcomeNotice(ensured.outcome, kind)
      if (notice) setNotice(notice)
      else if (ensured.outcome === 'raised') setNotice(`已把 ${endpointKindLabel(kind)} 的窗口拉到前台。`)
      else setNotice(`${endpointKindLabel(kind)} 的窗口已恢复；Windows 拒绝了前台切换，点一下它即可。`)
    } catch (error) {
      setNotice(`打开客户端界面失败：${String(error)}`)
    } finally {
      if (mountedRef.current) setReachBusy(false)
    }
  }

  const refreshAutostartStatus = async () => {    if (!nativeRuntime.isNative) return
    try {
      const status: AutostartStatus = await nativeRuntime.autostartStatus()
      const current = settingsRef.current
      if (!mountedRef.current) return
      setAutostartState(status)
      if (current.autostart !== status.enabled) {
        const next = { ...current, autostart: status.enabled }
        settingsRef.current = next
        setSettings(next)
        saveSettings(next)
        void invoke('publish_settings', { settings: next }).catch((error) => setNotice(`设置同步失败：${String(error)}`))
      }
      if (status.source === 'disabled-by-user') setNotice('Windows 已禁用 DSH Wallpaper 开机启动，请在系统设置中允许。')
      if (status.source === 'disabled-by-policy') setNotice('Windows 策略禁止 DSH Wallpaper 开机启动。')
    } catch (error) {
      setNotice(`读取开机自启状态失败：${String(error)}`)
    }
  }

  /**
   * One runner per probe. The controller decides *when* a runner may start;
   * it never lets two runs of the same probe overlap, and it reports a failed
   * probe as a notice instead of letting the window lose its interactivity.
   */
  const probeControllerHolder = useRef<{ refresh: (probe: SettingsProbe) => Promise<void> }>()
  const probeRunners = useMemo<Record<SettingsProbe, () => Promise<unknown>>>(() => ({
    translucentTb: async () => {
      const status = await nativeRuntime.translucentTbStatus()
      if (mountedRef.current) setTranslucentTb(status)
    },
    managedDsh: async () => {
      const status = await nativeRuntime.managedDshStatus()
      if (mountedRef.current) setManagedDsh(status)
    },
    deepseekWebAdapterConfig: async () => {
      const status = await nativeRuntime.deepseekWebAdapterConfig()
      if (mountedRef.current) setDeepseekWebAdapterConfig(status)
    },
    lockScreenDiagnostics: refreshLockScreenDiagnostics,
    autostartStatus: refreshAutostartStatus,
    desktopDisplays: refreshDesktopDisplays,
    // The history listing is a management view, not a live surface: it is read
    // when the page opens and on explicit refresh, never polled.
    apiHistory: async () => {
      const listing = await nativeRuntime.listApiConversations()
      if (mountedRef.current) setApiHistory(listing)
    },
  }), [])

  const probeController = useMemo(() => createSettingsProbeController({
    runProbe: (probe) => probeRunners[probe](),
    schedule: createProbeScheduler(window),
    onError: (probe, error) => setNotice(settingsProbeErrorMessage(probe, error)),
  }), [probeRunners])
  probeControllerHolder.current = probeController

  /**
   * Manual refresh buttons run a fresh probe. They must not be silently
   * swallowed by the "already probed" bookkeeping, and they must not start a
   * second run while the first one is still in flight.
   */
  const refreshProbe = (probe: SettingsProbe) => void probeControllerHolder.current?.refresh(probe)
  const refreshTranslucentTb = () => refreshProbe('translucentTb')
  const refreshManagedDsh = () => refreshProbe('managedDsh')
  const refreshDeepSeekWebAdapterConfig = () => refreshProbe('deepseekWebAdapterConfig')
  const openDeepSeekWebAdapterConfig = async () => {
    try {
      setDeepseekWebAdapterConfig(await nativeRuntime.openDeepSeekWebAdapterConfig())
      setNotice('已打开网页适配器配置；保存后下一次网页状态、历史或发送操作会读取新配置。')
    } catch (error) {
      setNotice(`网页适配器配置打开失败：${String(error)}`)
    }
  }
  const resetDeepSeekWebAdapterConfig = async () => {
    if (!window.confirm('恢复默认网页适配器配置会覆盖当前本地 override 文件。确定继续吗？')) return
    try {
      setDeepseekWebAdapterConfig(await nativeRuntime.resetDeepSeekWebAdapterConfig())
      setNotice('网页适配器配置已恢复默认。')
    } catch (error) {
      setNotice(`网页适配器配置恢复失败：${String(error)}`)
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
      if (mountedRef.current) setNotice(`读取访问密钥状态失败：${String(error)}`)
    }
  }

  const refreshApiModelCatalog = async (): Promise<boolean> => {
    try {
      const baseUrl = settingsRef.current.deepseekApi.baseUrl
      const catalog = await nativeRuntime.apiModels(baseUrl)
      if (!mountedRef.current) return true
      if (!catalog.supported) {
        setNotice('该 API 地址不提供模型列表（HTTP 404/405）。')
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
      if (mountedRef.current) setNotice(`读取模型列表失败：${String(error)}`)
      return false
    }
  }

  const testApiKey = async () => {
    if (apiKeyBusy) return
    const draft = apiKeyDraft.trim()
    if (!draft && !apiKeyStatus?.present) {
      setNotice('请先填入 DeepSeek API Key。')
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
      setNotice(draft ? 'API Key 已保存到 Windows 凭据管理器，模型列表已更新。' : '已保存的 API Key 可用，模型列表已更新。')
    } catch (error) {
      setNotice(`API Key 保存失败：${String(error)}`)
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
        setNotice('模型列表已刷新。')
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
      setNotice(`已切换为${backendModeLabel(backend)}，正在运行的壁纸立即生效。`)
    } catch (error) {
      setNotice(`切换后端失败：${String(error)}`)
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
      if (mountedRef.current) setNotice(settingsProbeErrorMessage('apiHistory', error))
    } finally {
      apiHistoryOperationRef.current = false
      if (mountedRef.current) setApiHistoryBusy(false)
    }
  }

  const deleteApiConversation = async (conversationId: string) => {
    if (apiHistoryOperationRef.current) return
    // Deleting a transcript is irreversible and cannot be undone from here.
    if (!window.confirm(`删除 API 会话 ${conversationId} 的本地记录？此操作无法撤销。`)) return
    apiHistoryOperationRef.current = true
    setApiHistoryBusy(true)
    try {
      const removed = await nativeRuntime.deleteApiConversation(conversationId)
      setNotice(removed ? `已删除 API 会话 ${conversationId} 的本地记录。` : '该会话已不存在，列表已刷新。')
    } catch (error) {
      setNotice(`删除 API 会话失败：${String(error)}`)
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
    if (!window.confirm(`清空全部 ${count} 个 API 会话的本地记录？此操作无法撤销，但不会影响 DeepSeek 网页入口或 Harness 会话。`)) return
    apiHistoryOperationRef.current = true
    setApiHistoryBusy(true)
    try {
      const cleared = await nativeRuntime.clearApiHistory()
      setNotice(cleared > 0 ? `已清空 ${cleared} 个 API 会话的本地记录。` : '没有可清空的 API 会话记录。')
    } catch (error) {
      setNotice(`清空 API 历史失败：${String(error)}`)
    } finally {
      apiHistoryOperationRef.current = false
      if (mountedRef.current) setApiHistoryBusy(false)
      await refreshApiHistory()
    }
  }

  const setLockScreenEnabled = async (enabled: boolean, force = false) => {
    // `lockScreenBusy` only changes after a render. The ref closes the small
    // double-click / keyboard activation window before that render occurs.
    if (lockScreenOperationRef.current || (!force && settingsRef.current.lockScreenEnabled === enabled)) return
    lockScreenOperationRef.current = true
    setLockScreenBusy(true)
    try {
      const confirmation = await nativeRuntime.setLockScreen(enabled)
      // Do not optimistically persist or broadcast the setting: Windows is
      // authoritative here. Only record the requested state after its native
      // setter succeeds.
      commitSettings({ ...settingsRef.current, lockScreenEnabled: enabled })
      setNotice(confirmation)
      await refreshLockScreenDiagnostics()
    } catch (error) {
      // Keep the previously committed setting visible and persisted. This is
      // especially important when the MSIX identity gate rejects takeover.
      setNotice(`${enabled ? '接管锁屏图片' : '恢复原锁屏图片'}失败：${String(error)}`)
    } finally {
      lockScreenOperationRef.current = false
      setLockScreenBusy(false)
    }
  }
  const openWindowsLockScreenSettings = async () => {
    try {
      await nativeRuntime.openWindowsLockScreenSettings()
      setNotice('已打开 Windows 锁屏设置；请在系统设置中选择要恢复的图片。')
    } catch (error) {
      setNotice(`无法打开 Windows 锁屏设置：${String(error)}`)
    }
  }
  const clearStaleLockScreenBackup = async () => {
    if (lockScreenOperationRef.current) return
    if (!window.confirm('清理旧锁屏恢复点会永久删除已保存的原锁屏图片副本。Windows 当前锁屏图片不会被修改。确定继续吗？')) return
    lockScreenOperationRef.current = true
    setLockScreenBusy(true)
    try {
      setNotice(await nativeRuntime.clearStaleLockScreenBackup(true))
      await refreshLockScreenDiagnostics()
    } catch (error) {
      setNotice(`清理旧恢复点失败：${String(error)}`)
    } finally {
      lockScreenOperationRef.current = false
      setLockScreenBusy(false)
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
    }).catch((error) => setNotice(String(error)))
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
    }).catch((error) => setNotice(String(error)))
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

  const change = (next: WallpaperSettings) => {
    const previous = settingsRef.current
    const autostartChanged = next.autostart !== previous.autostart
    // System lock-screen ownership is deliberately excluded from the normal
    // immediate-save path. The dedicated async operation above is the only
    // place allowed to persist or broadcast a change to this field.
    const normalNext = next.lockScreenEnabled === previous.lockScreenEnabled
      ? next
      : { ...next, lockScreenEnabled: previous.lockScreenEnabled }
    if (autostartChanged) {
      if (autostartOperationRef.current) return
      autostartOperationRef.current = true
      setAutostartBusy(true)
      void nativeRuntime.setAutostart(next.autostart)
        .then((status) => {
          const current = settingsRef.current
          if (mountedRef.current) setAutostartState(status)
          commitSettings({ ...current, autostart: status.enabled })
          if (status.enabled !== next.autostart) setNotice('Windows 没有接受这次开机自启变更，请检查系统启动应用权限。')
        })
        .catch((error) => setNotice(`开机自启更新失败：${String(error)}`))
        .finally(() => {
          autostartOperationRef.current = false
          setAutostartBusy(false)
        })
      return
    }
    commitSettings(normalNext)
  }

  const close = () => void invoke('hide_settings_window')
  const importAppearance = async () => {
    setAppearanceBusy(true)
    try {
      const paths = await chooseAppearanceImportPaths()
      if (paths.length === 0) return
      await nativeAppearance.importPaths(paths)
      refreshAppearance()
    } catch (error) { setNotice(`导入失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const classifyAppearance = async (assetId: string, slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { await nativeAppearance.classifyAsset(assetId, [slot]); refreshAppearance() } catch (error) { setNotice(`素材分类失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const selectAppearance = async (slot: AppearanceSlot, assetId: string) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.setOverride(slot, assetId)).overrides); void invoke('notify_appearance_changed').catch((error) => setNotice(`外观同步失败：${String(error)}`)) } catch (error) { setNotice(`应用素材失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const clearAppearance = async (slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.clearOverride(slot)).overrides); void invoke('notify_appearance_changed').catch((error) => setNotice(`外观同步失败：${String(error)}`)) } catch (error) { setNotice(`恢复默认失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }

  return <main className="settings-window">
    <ResizeHandles />
    {notice && <div className="settings-window__notice" role="status"><span>{notice}</span><button type="button" aria-label="关闭通知" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); setNotice(undefined) }}>×</button></div>}
    <SettingsPanel
      settings={settings}
      page={page}
      onPageChange={(next) => { setPage(next); if (next === 'appearance') refreshAppearance() }}
      harnessStatus={harness}
      translucentTb={translucentTb}
      harnessTargets={harnessTargets}
      subjectCatalogVerifiedAt={catalogVerifiedAt}
      subjectChoice={subjectChoicePrompt(harnessTargets) ?? undefined}
      onSelectSubject={(targetId) => change({
        ...settingsRef.current,
        dshLaunch: {
          ...settingsRef.current.dshLaunch,
          subjectId: targetId,
          // A checkout's id *is* its path, so keeping the root-path field in step
          // means the profile/launcher fields below still describe the same tree.
          // A shell keeps whatever path is there, so switching back is lossless.
          ...(targetId.startsWith('shell:') ? {} : { rootPath: targetId }),
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
      onRefreshManagedDsh={refreshManagedDsh}
      onStopManagedDsh={() => void nativeRuntime.stopManagedDsh().then(() => { setNotice('已停止本应用启动的 DSH。'); refreshManagedDsh() }).catch((error) => setNotice(String(error)))}
      onChange={change}
      onRefreshTranslucentTb={refreshTranslucentTb}
      onLaunchTranslucentTb={() => void nativeRuntime.launchTranslucentTb().then(refreshTranslucentTb).catch((error) => setNotice(String(error)))}
      onInstallTranslucentTb={() => void nativeRuntime.openTranslucentTbInstall().catch((error) => setNotice(String(error)))}
      appearanceAssets={appearanceAssets}
      appearanceOverrides={appearanceOverrides}
      appearanceBusy={appearanceBusy}
      onImportAppearance={() => { void importAppearance() }}
      onClassifyAppearance={(assetId, slot) => { void classifyAppearance(assetId, slot) }}
      onSelectAppearance={(slot, assetId) => { void selectAppearance(slot, assetId) }}
      onClearAppearance={(slot) => { void clearAppearance(slot) }}
      lockScreenDiagnostics={lockScreenDiagnostics}
      onRefreshLockScreenDiagnostics={refreshLockScreenDiagnostics}
      onRestoreLockScreen={() => { void openWindowsLockScreenSettings() }}
      onClearStaleLockScreenBackup={() => { void clearStaleLockScreenBackup() }}
      onSetLockScreenEnabled={(enabled) => { void setLockScreenEnabled(enabled) }}
      lockScreenBusy={lockScreenBusy}
      autostartBusy={autostartBusy}
      desktopDisplays={desktopDisplays}
      onRefreshDesktopDisplays={refreshDesktopDisplays}
      apiHistory={apiHistory}
      apiHistoryBusy={apiHistoryBusy}
      onRefreshApiHistory={() => { void refreshApiHistory() }}
      onDeleteApiConversation={(conversationId) => { void deleteApiConversation(conversationId) }}
      onClearApiHistory={() => { void clearApiHistory() }}
      onRequestDeepSeekLogin={() => void nativeRuntime.requestDeepSeekLogin().catch((error) => setNotice(`无法打开 DeepSeek 应用内页面：${String(error)}`))}
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
      onSetInteractionEnabled={(enabled) => void appCoreClient.setInteractionEnabled(enabled).then((snapshot) => setInteractionEnabled(snapshot.interaction.enabled)).catch((error) => setNotice(String(error)))}
      desktopWorkspace={desktopWorkspace}
      onOpenProjectMemory={async () => {
        // 打开的是**文件所在的位置**：文件在就选中它，不在就把工作区目录打开（原生实现）。
        // 结果用那句 notice 如实回报，而不是让用户自己去猜窗口为什么没动。
        setOpeningMemory(true)
        try {
          const opened = await nativeRuntime.openProjectMemory()
          setNotice(opened.memoryExists ? '已在资源管理器中选中「项目记忆.md」。' : '还没有「项目记忆.md」：已打开桌面会话目录，你或助手第一次“记下来”时它会出现在这里。')
        } catch (error) {
          setNotice(String(error))
        } finally {
          setOpeningMemory(false)
        }
      }}
      openingMemory={openingMemory}
      onClose={close}
    />
  </main>
}
