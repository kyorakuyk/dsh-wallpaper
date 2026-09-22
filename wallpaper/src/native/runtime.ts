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
export interface AutostartStatus { enabled: boolean; source: 'startup-task' | 'run' | 'none' | 'disabled-by-user' | 'disabled-by-policy' | 'unsupported' }
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
  releaseNativeBootstrap(): Promise<void>
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
  deleteApiConversation(conversationId: string): Promise<boolean>
  clearApiHistory(): Promise<number>
  listenTray(listener: (event: { type: 'backend'; backend: BackendMode }) => void): Promise<() => void>
  probeHarness(): Promise<HarnessStatus>
  desktopDisplays(): Promise<DesktopDisplayInfo[]>
  desktopLayoutMetrics(displayId?: string): Promise<{ expandedBottomInset: number; taskbarVisible: boolean }>
  scanDshPaths(hintPath?: string, deepScan?: boolean): Promise<Array<{ rootPath: string; source: string }>>
  launchDsh(rootPath: string, profile: string, command?: string): Promise<number>
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
  async releaseNativeBootstrap() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('release_native_bootstrap')
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
  async managedDshStatus() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshStatus>('managed_dsh_status')
  },
  async stopManagedDsh() {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('stop_managed_dsh')
  },
}
