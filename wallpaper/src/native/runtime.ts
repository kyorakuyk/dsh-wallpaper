import type { BackendMode, ChatMessage, ScopedChatEvent } from '../domain/types.ts'
import type { HarnessStatus } from '../connect/harness.ts'

export interface NativeSendOptions {
  conversationId?: string
  requestId?: string
  newConversation?: boolean
  baseUrl?: string
  model?: string
  /** CNY per million input tokens. Omitted means pricing is not configured. */
  priceInputPerMillion?: number
  /** CNY per million output tokens. Omitted means pricing is not configured. */
  priceOutputPerMillion?: number
}

export interface TranslucentTbStatus { installed: boolean; running: boolean; source?: string }
export interface LockScreenDiagnostics { supported: boolean; packageIdentity: boolean; takeoverAvailable: boolean; originalImageUri?: string; backupExists: boolean; backupValid: boolean; staleBackup: boolean; managedImageReady: boolean; managedImageActive: boolean; developmentBuild: boolean; warnings: string[] }
export interface ManagedDshStatus { managed: boolean; running: boolean; pid?: number; rootPath?: string; profile?: string }
/**
 * 凭据管理器愿意交代的全部内容：有没有 Key，以及脱敏形态（`sk-••••••••abcd`）。
 *
 * 没有明文——原生侧**不存在**把 Key 交出来的命令；脱敏也是那边算好的（`mask_api_key`），
 * 渲染端连"取中间几位自己拼"的机会都没有。
 */
export interface ApiKeyStatus {
  present: boolean
  masked?: string
}
/**
 * Outcome of the one automatic DSH start this process is allowed to attempt.
 * `outcome` is a closed, non-sensitive code; `external` means port 3080 was
 * already owned by someone else's DSH and was deliberately left alone.
 */
export interface ManagedDshAutostart {
  outcome: 'started' | 'started-unconfirmed' | 'already-attempted' | 'already-running' | 'root-path-missing'
    | 'root-path-invalid' | 'launcher-missing' | 'profile-invalid' | 'port-occupied-external'
    | 'command-not-confirmed' | 'unknown-target' | 'spawn-failed'
  pid?: number
  external: boolean
}
export interface AutostartStatus { enabled: boolean; source: 'startup-task' | 'run' | 'none' | 'disabled-by-user' | 'disabled-by-policy' | 'unsupported' }
/**
 * One endpoint from the native scan. `kind` is the expected client shape for a
 * known port; `bridgeFound` is true only when a wallpaper Bridge answered, so a
 * port hosting some other HTTP service is not offered as an endpoint.
 */
export interface HarnessEndpointScan {
  port: number
  kind: 'official-desktop' | 'community-desktop' | 'official-web'
  source: 'default' | 'user'
  bridgeFound: boolean
  status: HarnessStatus
}

/**
 * What the settings say the wallpaper may talk to.
 *
 * `subjectId` is the user's real choice (`shell:<aumid>` for a client that carries
 * its own checkout, else a source tree's path); `port` is their explicit endpoint
 * override; `extraPorts` are the ports they added for a checkout that does not
 * listen on the default. Native derives the admissible ports from the subject alone
 * — see `subject_endpoint_ports` — so this is a statement about the subject, never
 * about which client happens to answer.
 */
export interface HarnessEndpointScope {
  port?: number | null
  subjectId?: string
  extraPorts?: readonly number[]
}

/** What native made of that scope, for logging and for the settings card. */
export interface HarnessEndpointScopeResult {
  port?: number | null
  subjectId?: string
  /** The ports the subject may be reached on; empty when nothing is configured. */
  subjectPorts: number[]
}

/**
 * What the shim did when asked to start a chosen execution subject.
 *
 * `started-unconfirmed` is not a failure: the Windows shell accepted the request
 * but nothing answered before the launch timeout, which a slow first start of an
 * Electron client can produce. `already-running` means the subject's port was
 * already answering, so its live instance was reported and left untouched.
 */
export interface HarnessLaunchOutcome {
  outcome: 'started' | 'started-unconfirmed' | 'already-running' | 'unknown-target'
    | 'root-path-invalid' | 'launcher-missing' | 'profile-invalid'
    | 'port-occupied-external' | 'command-not-confirmed' | 'spawn-failed'
  kind: 'embedded-shell' | 'checkout'
  /** Present only when this application started and owns a child (a checkout). */
  pid?: number
  /** True when the subject's window was put out of sight after starting. */
  hidden: boolean
}

/**
 * What 「拉起 UI」 found, and what it had to do about it.
 *
 * Idempotent over the three states a subject can be in, so the caller does not
 * have to know which one it is looking at: it starts the subject when nothing
 * answers, shows a window the wallpaper hid at startup, and otherwise just brings
 * the window forward.
 */
export interface HarnessUiOutcome {
  outcome: 'raised' | 'raise-refused' | 'no-window' | 'not-running' | 'unknown-target'
  kind: 'embedded-shell' | 'checkout'
  /** True when this call had to start the subject first. */
  started: boolean
  /** The start's own code (word it with `launchOutcomeNotice`) when it had to. */
  startOutcome?: string
}

/**
 * What the last scan confirmed, and when.
 *
 * A scan is manual and expensive, so the settings surface shows this instead of
 * walking the disk on every open — with `verifiedAtMs` visible, because a cached
 * list must never look current: a client can be uninstalled between two scans.
 */
export interface HarnessTargetCatalog {
  schemaVersion: number
  /** Milliseconds since the epoch, from the scan that produced this list. */
  verifiedAtMs: number
  targets: HarnessTarget[]
  requiresSubjectChoice: boolean
}

/**
 * One harness execution subject, in the two classes the design froze
 * (`docs/design/harness-subject-and-ui-design.md`, §3).
 *
 * An `embedded-shell` carries its own checkout, so it fixes the service and the
 * window together and is identified by AUMID — never by a path, which is what
 * lets a client update leave a stored choice valid. A `checkout` is a source
 * tree: its path *is* its identity, it has no window of its own, and it is the
 * only class that uses the `profile` setting.
 */
export interface HarnessTarget {
  /** Stable key to store and later resolve back to a subject. */
  id: string
  kind: 'embedded-shell' | 'checkout'
  /** Which client shape this subject answers as, reusing the endpoint scan's vocabulary. */
  client: 'official-desktop' | 'community-desktop' | 'official-web'
  label: string
  /** Where the scan found it: a checkout's scan origin, or a shell's shortcut directory. */
  source: string
  identity: {
    /** The shell's AppUserModelID; set for shells only. */
    aumid?: string
    /** The checkout's root; set for checkouts only. */
    rootPath?: string
    /** Probe hints only. A user may move any port. */
    defaultPorts: number[]
  }
  launch: {
    /** `apps-folder` hands the shell a location-independent alias; `managed-command` spawns the tree. */
    kind: 'apps-folder' | 'managed-command'
    alias?: string
  }
  capabilities: {
    /** Re-launching reuses the running shell instead of opening a second window. */
    singleInstance: boolean
    /** It owns a Windows window, so 「拉起 UI」 can raise it. */
    ownsWindow: boolean
    /** It can be started with its window kept out of sight. */
    canStartHidden: boolean
    /** It needs the `profile` setting (checkouts only). */
    needsProfile: boolean
  }
}

/**
 * One subject scan. `requiresSubjectChoice` is set when more than one source
 * tree exists: the wallpaper must not choose for the user, so the settings
 * surface asks which tree is the default subject (§4.3).
 */
export interface HarnessTargetScan {
  targets: HarnessTarget[]
  requiresSubjectChoice: boolean
}

/**
 * Result of asking for a client's own Windows window.
 *
 * `raised` is the only success. `no-window` means something is listening on that
 * endpoint but owns no visible window — the CLI/webui shape, whose interface is a
 * browser URL instead — and `raise-refused` means Windows declined the foreground
 * change, which is normal for a desktop wallpaper and not a failure to report as
 * one. The codes are a contract with the wording table below.
 */
export interface RaiseClientOutcome {
  outcome: 'raised' | 'no-window' | 'not-running' | 'raise-refused'
  raised: boolean
}
export interface DeepSeekWebStatus { state: 'loading' | 'logged-out' | 'ready' | 'generating' | 'unsupported'; conversationId?: string; model?: string; signature: string }
export interface DeepSeekWebHistory { messages: ChatMessage[]; conversationId?: string; model?: string; state: DeepSeekWebStatus['state'] | 'loading' }
export interface DeepSeekWebAdapterConfigStatus { schemaVersion: number; adapterVersion: string; source: 'builtin' | 'local'; path: string; warning?: string }
/**
 * A bounded window of one durable API transcript. `hasMore` means older
 * messages exist on disk but were not cloned across IPC; the caller asks for a
 * larger `limit` when the user wants to read further back.
 */
export interface ApiHistoryPage {
  messages: ChatMessage[]
  totalMessages: number
  hasMore: boolean
  bytes: number
  limit: number
}
/**
 * One durable API transcript, as listed for the settings history page. Message
 * bodies are intentionally absent: this is a management view, not a reader.
 */
export interface ApiConversationSummary {
  id: string
  messageCount: number
  /** Serialized size of this transcript — what the persistence budget acts on. */
  bytes: number
  updatedAt: number
  firstMessageAt: number
  lastMessageAt: number
  /** True for the transcript this process is currently reading. */
  active: boolean
}

export interface ApiConversationListing {
  conversations: ApiConversationSummary[]
  totalBytes: number
  totalMessages: number
  /** Size the application trims to before writing. */
  budgetBytes: number
  /** Hard ceiling above which a save is refused. */
  maxBytes: number
}

export interface DesktopRect { x: number; y: number; width: number; height: number }
export interface DesktopDisplayInfo { id: string; name: string; bounds: DesktopRect; workArea: DesktopRect; scaleFactor: number; primary: boolean }

export interface NativeRuntime {
  isNative: boolean
  setLockScreen(enabled: boolean): Promise<string>
  clearStaleLockScreenBackup(confirmed: boolean): Promise<string>
  lockScreenDiagnostics(): Promise<LockScreenDiagnostics>
  setAutostart(enabled: boolean): Promise<AutostartStatus>
  autostartStatus(): Promise<AutostartStatus>
  translucentTbStatus(): Promise<TranslucentTbStatus>
  launchTranslucentTb(): Promise<void>
  openTranslucentTbInstall(): Promise<void>
  openWindowsLockScreenSettings(): Promise<void>
  /**
   * Store the DeepSeek API key the user typed in Settings.
   *
   * This is the one place a plaintext key crosses the Tauri IPC boundary, and only on the
   * way **in** — it replaced the Windows credential dialog, which the user found impossible
   * to read (it is built for username+password credentials). Nothing hands a key back:
   * `apiKeyStatus` answers with `maskApiKey`'s shape only, and the API client reads the
   * credential natively in Rust.
   */
  saveApiKey(key: string): Promise<void>
  /**
   * Whether a key is stored, and the masked form to show. `masked` is produced natively
   * (`mask_api_key`) and never contains the middle of the key.
   */
  apiKeyStatus(): Promise<ApiKeyStatus>
  requestDeepSeekLogin(): Promise<void>
  nativeBootstrapGeneration(): Promise<number>
  releaseNativeBootstrap(generation: number): Promise<boolean>
  ensureDeepSeekWeb(conversationId?: string, newConversation?: boolean): Promise<void>
  deepseekWebStatus(): Promise<DeepSeekWebStatus>
  deepseekWebHistory(conversationId?: string, newConversation?: boolean): Promise<DeepSeekWebHistory>
  deepseekWebAdapterConfig(): Promise<DeepSeekWebAdapterConfigStatus>
  openDeepSeekWebAdapterConfig(): Promise<DeepSeekWebAdapterConfigStatus>
  resetDeepSeekWebAdapterConfig(): Promise<DeepSeekWebAdapterConfigStatus>
  openSettingsWindow(): Promise<void>
  listenSystem(listener: (event: 'locked' | 'unlocked' | 'suspend' | 'resume') => void): Promise<() => void>
  listenChat(listener: (event: ScopedChatEvent) => void): Promise<() => void>
  sendChat(mode: BackendMode, text: string, options?: NativeSendOptions): Promise<string | undefined>
  cancelChat(mode: BackendMode): Promise<void>
  connectHarness(resumeSessionId: string | undefined, connectionId: string, model?: string): Promise<string>
  harnessHistory(): Promise<ChatMessage[]>
  harnessPresets(): Promise<Array<{ id: string; name?: string; description?: string; trust: 'system' | 'user'; broken?: string; isDefault: boolean }>>
  setHarnessPreset(preset: string): Promise<void>
  harnessControls(): Promise<{ permission: { current: string; options: string[] }; commands: Array<{ name: string; description: string; input?: { hint: string } }> }>
  /**
   * 当前 Harness 主体可用的模型目录（宿主提供、经桥接转发）。
   * `supported: false` = 宿主不具备枚举能力，不是"没有模型"。
   */
  harnessModels(): Promise<{ supported: boolean; provider?: string; current?: { provider: string; model: string }; models?: Array<{ id: string; name: string }> }>
  /**
   * 把选定的模型推给 Harness 宿主（宿主存进自己的设置，跨宿主重启生效）。
   * 宿主不具备该能力时拒绝——壁纸自己那次会话仍然用选定模型。
   */
  harnessSetModel(model: string): Promise<{ provider: string; model: string }>
  /** DeepSeek API 兼容端点自报的模型目录（`/models`）。 */
  apiModels(baseUrl: string): Promise<{ supported: boolean; models?: Array<{ id: string; name: string }> }>
  setHarnessPermission(permission: string): Promise<void>
  apiHistory(conversationId: string, limit?: number): Promise<ApiHistoryPage>
  listApiConversations(): Promise<ApiConversationListing>
  deleteApiConversation(conversationId: string): Promise<boolean>
  clearApiHistory(): Promise<number>
  listenTray(listener: (event: { type: 'backend'; backend: BackendMode }) => void): Promise<() => void>
  probeHarness(): Promise<HarnessStatus>
  /**
   * Publish which endpoints the wallpaper may talk to.
   *
   * The settings are the authority, and the native monitor owns the probe loop, so
   * the whole scope has to reach native state: the subject the user chose, the
   * endpoint they pinned, and the ports they added for a checkout. It travels as one
   * call because it is one decision — a partial update would leave the monitor
   * probing a port belonging to the subject the user has just left, which is the
   * silent substitution the connection path forbids.
   */
  setHarnessEndpointScope(scope: HarnessEndpointScope): Promise<HarnessEndpointScopeResult>
  /**
   * Leave the inner desktop — the island's 「X」.
   *
   * Native owns the fact "are we in the inner desktop": it hides and restores Explorer's
   * icon layer, and the floating ball's own pop-out rule keys off it. So the island asks
   * native instead of moving the interface by itself; the transition then arrives as the
   * same `desktop-workspace-toggle: leave` event the desktop double-click produces.
   */
  leaveInnerWorkspace(): Promise<void>
  /** Which local ports host a wallpaper Bridge, in the native layer's order. */
  scanHarnessEndpoints(extraPorts?: readonly number[]): Promise<HarnessEndpointScan[]>
  /**
   * Bring a desktop client's own window forward. Never launches anything, so a
   * client that is not running reports `not-running` rather than starting up.
   */
  /**
   * Verify that a renderer island `pointerdown` really is a user click.
   *
   * The native click route is closed, so the island event has to be reported from
   * here - and the native side deliberately does not trust that report: it re-checks
   * the physical left button and the window under the cursor, and rejects the call
   * otherwise. Returns the verdict for logging; it performs no activation.
   */
  verifyIslandClick(): Promise<string>
  raiseClientWindow(port: number): Promise<RaiseClientOutcome>
  /**
   * Open a windowless client's web UI in the default browser. Only a loopback
   * endpoint is accepted, so this cannot open an arbitrary destination.
   */
  openClientInBrowser(port: number, path?: string): Promise<void>
  /** Whether anything is listening, without raising it. */
  harnessEndpointListening(port: number): Promise<boolean>
  desktopDisplays(): Promise<DesktopDisplayInfo[]>
  desktopLayoutMetrics(displayId?: string): Promise<{ expandedBottomInset: number; taskbarVisible: boolean }>
  scanDshPaths(hintPath?: string, deepScan?: boolean): Promise<Array<{ rootPath: string; source: string }>>
  /**
   * List the harness execution subjects this machine offers: shells that carry
   * their own checkout, and source trees. Discovery only — it starts nothing and
   * takes over nothing.
   */
  scanHarnessTargets(hintPath?: string, deepScan?: boolean): Promise<HarnessTargetScan>
  /** The last confirmed subject list, or null when nothing has been scanned yet. */
  harnessTargetCatalog(): Promise<HarnessTargetCatalog | null>
  /**
   * Start the chosen execution subject. The class decides the mechanism — a shell
   * alias, or the managed checkout chain — so the caller passes an id and reads a
   * closed outcome code back.
   */
  launchHarnessTarget(options: {
    targetId: string
    profile?: string
    command?: string
  }): Promise<HarnessLaunchOutcome>
  /**
   * The unattended counterpart, at most once per wallpaper process (native state
   * is the single-flight authority, exactly as for `autostartManagedDsh`). The
   * differences are real: only this path may keep a window out of sight, and only
   * this path needs explicit consent for a custom launcher.
   */
  autostartHarnessTarget(options: {
    targetId?: string
    profile: string
    command?: string
    trustedCommand?: boolean
  }): Promise<ManagedDshAutostart>
  /**
   * Make the chosen subject's interface available and foreground, whatever state it
   * is in: not running, running with a window the wallpaper hid, or behind another
   * window. Starts it when nothing answers, which is why this is native policy
   * rather than a caller's branch.
   */
  ensureHarnessUi(options: {
    targetId?: string
    port: number
    profile?: string
    command?: string
  }): Promise<HarnessUiOutcome>
  launchDsh(rootPath: string, profile: string, command?: string): Promise<number>
  /**
   * One automatic start attempt per process, with the outcome remembered even
   * when it fails so a bad configuration cannot become a retry loop.
   */
  autostartManagedDsh(options: {
    rootPath?: string
    profile: string
    command?: string
    trustedCommand?: boolean
  }): Promise<ManagedDshAutostart>
  managedDshAutostartStatus(): Promise<ManagedDshAutostart | null>
  managedDshStatus(): Promise<ManagedDshStatus>
  stopManagedDsh(): Promise<void>
}

async function tauriAvailable(): Promise<boolean> {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

const nativeWindowAvailable = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const nativeRuntime: NativeRuntime = {
  isNative: nativeWindowAvailable,
  async setLockScreen(enabled) {
    if (!await tauriAvailable()) return '浏览器预览不支持设置系统锁屏。'
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('set_lock_screen_enabled', { enabled })
  },
  async clearStaleLockScreenBackup(confirmed) {
    if (!await tauriAvailable()) return '浏览器预览不支持清理系统锁屏恢复点。'
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('clear_stale_lock_screen_backup', { confirmed })
  },
  async lockScreenDiagnostics() {
    if (!await tauriAvailable()) return { supported: false, packageIdentity: false, takeoverAvailable: false, backupExists: false, backupValid: false, staleBackup: false, managedImageReady: false, managedImageActive: false, developmentBuild: false, warnings: ['浏览器预览不支持系统锁屏诊断。'] }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<LockScreenDiagnostics>('get_lock_screen_diagnostics')
  },
  async setAutostart(enabled) {
    if (!await tauriAvailable()) return { enabled: false, source: 'unsupported' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AutostartStatus>('set_autostart', { enabled })
  },
  async autostartStatus() {
    if (!await tauriAvailable()) return { enabled: false, source: 'unsupported' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AutostartStatus>('autostart_status')
  },
  async translucentTbStatus() {
    if (!await tauriAvailable()) return { installed: false, running: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<TranslucentTbStatus>('translucent_tb_status')
  },
  async launchTranslucentTb() {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('launch_translucent_tb')
  },
  async openTranslucentTbInstall() {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_translucent_tb_install')
  },
  async openWindowsLockScreenSettings() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_windows_lock_screen_settings')
  },
  async saveApiKey(key) {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持 Windows 凭据管理器')
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('save_api_key', { key })
  },
  async apiKeyStatus() {
    // 浏览器预览里没有凭据管理器：如实回答"没有 Key"，而不是抛错让设置页显示故障。
    if (!await tauriAvailable()) return { present: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ApiKeyStatus>('api_key_status')
  },
  async requestDeepSeekLogin() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('show_deepseek_login')
  },
  async nativeBootstrapGeneration() {
    if (!await tauriAvailable()) return 0
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('native_bootstrap_generation')
  },
  async releaseNativeBootstrap(generation) {
    if (!await tauriAvailable()) return true
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('release_native_bootstrap', { generation })
  },
  async ensureDeepSeekWeb(conversationId, newConversation = false) {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('deepseek_web_ensure', { conversationId, newConversation })
  },
  async deepseekWebStatus() {
    if (!await tauriAvailable()) return { state: 'loading', signature: 'deepseek-chat-dom-v2' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebStatus>('deepseek_web_status')
  },
  async deepseekWebHistory(conversationId, newConversation = false) {
    if (!await tauriAvailable()) return { messages: [], state: 'loading' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebHistory>('deepseek_web_history', { conversationId, newConversation })
  },
  async deepseekWebAdapterConfig() {
    if (!await tauriAvailable()) return { schemaVersion: 1, adapterVersion: 'preview', source: 'builtin', path: '浏览器预览不支持本地网页适配器配置' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebAdapterConfigStatus>('deepseek_web_adapter_config_status')
  },
  async openDeepSeekWebAdapterConfig() {
    if (!await tauriAvailable()) throw new Error('浏览器预览不支持打开网页适配器配置')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebAdapterConfigStatus>('open_deepseek_web_adapter_config')
  },
  async resetDeepSeekWebAdapterConfig() {
    if (!await tauriAvailable()) throw new Error('浏览器预览不支持恢复网页适配器配置')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebAdapterConfigStatus>('reset_deepseek_web_adapter_config')
  },
  async openSettingsWindow() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_settings_window')
  },
  async listenSystem(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    return listen<'locked' | 'unlocked' | 'suspend' | 'resume'>('system-session', (event) => listener(event.payload))
  },
  async listenChat(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    return listen<ScopedChatEvent>('chat-event', (event) => listener(event.payload))
  },
  async sendChat(mode, text, options) {
    const { invoke } = await import('@tauri-apps/api/core')
    return (await invoke<string | null>('send_chat', {
      mode,
      text,
      conversationId: options?.conversationId,
      requestId: options?.requestId,
      newConversation: options?.newConversation,
      baseUrl: options?.baseUrl,
      model: options?.model,
      priceInputPerMillion: options?.priceInputPerMillion,
      priceOutputPerMillion: options?.priceOutputPerMillion,
    })) ?? undefined
  },
  async cancelChat(mode) {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('cancel_chat', { mode })
  },
  async connectHarness(resumeSessionId, connectionId, model) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('connect_harness', { resumeSessionId, connectionId, model })
  },
  async harnessHistory() {
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ messages: Array<{ id: string; role: 'user' | 'assistant'; content: string }> }>('harness_history')
    return result.messages.map((message) => ({ ...message, createdAt: Date.now() }))
  },
  async harnessPresets() {
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ presets: Array<{ id: string; name?: string; description?: string; trust: 'system' | 'user'; broken?: string; isDefault: boolean }> }>('harness_presets')
    return result.presets
  },
  async setHarnessPreset(preset) {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('harness_set_preset', { preset })
  },
  async harnessControls() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ permission: { current: string; options: string[] }; commands: Array<{ name: string; description: string; input?: { hint: string } }> }>('harness_controls')
  },
  async harnessModels() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ supported: boolean; provider?: string; current?: { provider: string; model: string }; models?: Array<{ id: string; name: string }> }>('harness_models')
  },
  async apiModels(baseUrl) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ supported: boolean; models?: Array<{ id: string; name: string }> }>('api_models', { baseUrl })
  },
  async harnessSetModel(model) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ provider: string; model: string }>('harness_set_model', { model })
  },
  async setHarnessPermission(permission) {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('harness_set_permission', { permission })
  },
  async apiHistory(conversationId, limit) {
    if (!await tauriAvailable()) return { messages: [], totalMessages: 0, hasMore: false, bytes: 0, limit: limit ?? 0 }
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{
      messages: Array<{
        id: string
        role: 'user' | 'assistant'
        content: string
        createdAt: number
        usage?: ChatMessage['usage']
      }>
      totalMessages?: number
      hasMore?: boolean
      bytes?: number
      limit?: number
    }>('api_history', { conversationId, limit })
    // Message ids, timestamps and final usage are persisted by the native
    // DPAPI archive. Preserve them on resume so React keys and session cost
    // remain stable instead of fabricating a fresh zero-cost transcript.
    const messages = result.messages.map((message) => ({
      id: message.id,
      role: message.role,
      content: message.content,
      createdAt: message.createdAt,
      usage: message.usage,
    }))
    return {
      messages,
      totalMessages: result.totalMessages ?? messages.length,
      hasMore: result.hasMore === true,
      bytes: result.bytes ?? 0,
      limit: result.limit ?? messages.length,
    }
  },
  async listApiConversations() {
    if (!await tauriAvailable()) return { conversations: [], totalBytes: 0, totalMessages: 0, budgetBytes: 0, maxBytes: 0 }
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<Partial<ApiConversationListing>>('list_api_conversations')
    return {
      conversations: result.conversations ?? [],
      totalBytes: result.totalBytes ?? 0,
      totalMessages: result.totalMessages ?? 0,
      budgetBytes: result.budgetBytes ?? 0,
      maxBytes: result.maxBytes ?? 0,
    }
  },
  async deleteApiConversation(conversationId) {
    if (!await tauriAvailable()) return false
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('delete_api_conversation', { conversationId })
  },
  async clearApiHistory() {
    if (!await tauriAvailable()) return 0
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('clear_api_history')
  },
  async listenTray(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    const backendDispose = await listen<BackendMode>('tray-backend', (event) => listener({ type: 'backend', backend: event.payload }))
    return () => backendDispose()
  },
  async probeHarness() {
    if (!await tauriAvailable()) return { availability: 'offline' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessStatus>('probe_harness')
  },
  /**
   * Publish the endpoint scope the native monitor probes from.
   *
   * The monitor owns the probe loop, so the choice must reach native state: a
   * per-request port would leave the rendered status coming from the shipped
   * priority order while the settings named a subject. `subjectId` is the important
   * half — without it native only knew ports, and connected to whichever client
   * answered.
   */
  async setHarnessEndpointScope(scope: HarnessEndpointScope) {
    if (!await tauriAvailable()) {
      return { port: scope.port ?? null, subjectId: scope.subjectId, subjectPorts: [] }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessEndpointScopeResult>('set_harness_endpoint', {
      // Explicit nulls rather than omitted keys: the command's own boundary is
      // strict about what it received, and `None` is well defined on that side.
      port: scope.port ?? null,
      subjectId: scope.subjectId ?? null,
      extraPorts: scope.extraPorts ? [...scope.extraPorts] : [],
    })
  },
  /**
   * Ask native to leave the inner desktop, restoring Explorer's icon layer.
   *
   * A failure here is worth showing: without it the desktop stays in "inner" state and
   * the floating ball will not pop out again.
   */
  async leaveInnerWorkspace() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('leave_inner_workspace')
  },
  /**
   * Ask the native side to bring a desktop client's window forward.
   *
   * Native resolution matters here: the window is found from the port the
   * wallpaper is connected to, so it is the same client whose session the user is
   * talking to, and no stored executable path can go stale.
   */
  /** Returns the native verification verdict, or an empty string off Tauri. */
  async verifyIslandClick() {
      if (!await tauriAvailable()) return ''
      const { invoke } = await import('@tauri-apps/api/core')
      return invoke<string>('verify_island_click')
  },
  async raiseClientWindow(port: number) {
    if (!await tauriAvailable()) return { outcome: 'not-running' as const, raised: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<RaiseClientOutcome>('raise_client_window', { port })
  },
  /**
   * Open a windowless client's web UI in the default browser. Used only for the
   * CLI/webui shape, which has no Windows window to raise.
   */
  async openClientInBrowser(port: number, path = '/') {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_client_in_browser', { port, path })
  },
  async harnessEndpointListening(port: number) {
    if (!await tauriAvailable()) return false
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('harness_endpoint_listening', { port })
  },
  /**
   * Ask the native side which local ports host a wallpaper Bridge.
   *
   * The renderer can scan too, but the status the desktop actually renders comes
   * from this process, so the native scan is the authoritative one. `extraPorts`
   * carries the user's own additions on top of the three known client shapes.
   */
  async scanHarnessEndpoints(extraPorts: readonly number[] = []) {
    if (!await tauriAvailable()) return []
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessEndpointScan[]>('scan_harness_endpoints_command', { extraPorts: [...extraPorts] })
  },
  async desktopDisplays() {
    if (!await tauriAvailable()) return [{ id: 'preview', name: '预览屏幕', bounds: { x: 0, y: 0, width: 1920, height: 1080 }, workArea: { x: 0, y: 0, width: 1920, height: 1080 }, scaleFactor: 1, primary: true }]
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DesktopDisplayInfo[]>('desktop_displays')
  },
  async desktopLayoutMetrics(displayId) {
    if (!await tauriAvailable()) return { expandedBottomInset: 48, taskbarVisible: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ expandedBottomInset: number; taskbarVisible: boolean }>('desktop_layout_metrics', { displayId })
  },
  async scanDshPaths(hintPath, deepScan = false) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<Array<{ rootPath: string; source: string }>>('scan_dsh_paths', { hintPath, deepScan })
  },
  async scanHarnessTargets(hintPath, deepScan = false) {
    if (!await tauriAvailable()) return { targets: [], requiresSubjectChoice: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessTargetScan>('scan_harness_targets', { hintPath, deepScan })
  },
  async harnessTargetCatalog() {
    if (!await tauriAvailable()) return null
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessTargetCatalog | null>('harness_target_catalog')
  },
  async launchHarnessTarget(options) {
    if (!await tauriAvailable()) {
      return { outcome: 'spawn-failed' as const, kind: 'checkout' as const, hidden: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessLaunchOutcome>('launch_harness_target', {
      targetId: options.targetId,
      profile: options.profile,
      command: options.command,
    })
  },
  async autostartHarnessTarget(options) {
    if (!await tauriAvailable()) {
      // A browser preview has no native shell to own the process, so this reports
      // a terminal outcome instead of throwing into a boot path.
      return { outcome: 'spawn-failed' as const, external: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshAutostart>('autostart_harness_target', {
      targetId: options.targetId,
      profile: options.profile,
      command: options.command,
      trustedCommand: options.trustedCommand,
    })
  },
  async ensureHarnessUi(options) {
    if (!await tauriAvailable()) {
      return { outcome: 'not-running' as const, kind: 'checkout' as const, started: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessUiOutcome>('ensure_harness_ui', {
      targetId: options.targetId,
      port: options.port,
      profile: options.profile,
      command: options.command,
    })
  },
  async launchDsh(rootPath, profile, command) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('launch_dsh', { rootPath, profile, command })
  },
  /**
   * Ask the native side to start the configured DSH once per process.
   *
   * Native state is the single-flight authority: the caller may be a remounted
   * React tree, a second WebView, or an unlock broadcast, and all of them must
   * observe the same first-and-only attempt. A `false` gate here is a shortcut,
   * not the guarantee.
   */
  async autostartManagedDsh(options) {
    if (!await tauriAvailable()) {
      // A browser preview has no native shell to own the process. Report a
      // terminal outcome instead of throwing into a boot path.
      return { outcome: 'spawn-failed' as const, external: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshAutostart>('autostart_managed_dsh', {
      rootPath: options.rootPath,
      profile: options.profile,
      command: options.command,
      trustedCommand: options.trustedCommand,
    })
  },
  async managedDshAutostartStatus() {
    if (!await tauriAvailable()) return null
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshAutostart | null>('managed_dsh_autostart_status')
  },
  async managedDshStatus() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshStatus>('managed_dsh_status')
  },
  async stopManagedDsh() {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('stop_managed_dsh')
  },
}
