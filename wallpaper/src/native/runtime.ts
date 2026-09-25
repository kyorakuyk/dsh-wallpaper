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
 * Outcome of the one automatic DSH start this process is allowed to attempt.
 * `outcome` is a closed, non-sensitive code; `external` means port 3080 was
 * already owned by someone else's DSH and was deliberately left alone.
 */
export interface ManagedDshAutostart {
  outcome: 'started' | 'already-attempted' | 'root-path-missing' | 'root-path-invalid'
    | 'launcher-missing' | 'profile-invalid' | 'port-occupied-external' | 'command-not-confirmed' | 'spawn-failed'
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
   * Opens Windows' native credential dialog. The API key never crosses the
   * WebView IPC boundary: Windows persists it directly in Credential Manager.
   * `false` means that the user dismissed the dialog without making a change.
   */
  promptForApiKeyCredential(): Promise<boolean>
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
  setHarnessPermission(permission: string): Promise<void>
  apiHistory(conversationId: string, limit?: number): Promise<ApiHistoryPage>
  listApiConversations(): Promise<ApiConversationListing>
  deleteApiConversation(conversationId: string): Promise<boolean>
  clearApiHistory(): Promise<number>
  listenTray(listener: (event: { type: 'backend'; backend: BackendMode }) => void): Promise<() => void>
  probeHarness(): Promise<HarnessStatus>
  /** Pin the native monitor's endpoint (`null` clears it back to auto). */
  setHarnessEndpoint(port: number | null): Promise<number | null>
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
  async promptForApiKeyCredential() {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持 Windows 凭据管理器')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('prompt_for_api_key')
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
   * Pin the endpoint the native monitor probes, or clear the pin with `null`.
   *
   * The monitor owns the probe loop, so the choice must reach native state: a
   * per-request port would leave the rendered status coming from the default port
   * while the settings card implied otherwise.
   */
  async setHarnessEndpoint(port: number | null) {
    if (!await tauriAvailable()) return null
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number | null>('set_harness_endpoint', { port })
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
