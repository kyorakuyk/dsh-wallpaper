import type { BackendMode, ChatMessage, ScopedChatEvent } from '../domain/types.ts'
import type { HarnessStatus } from '../connect/harness.ts'

export interface NativeSendOptions {
  conversationId?: string
  requestId?: string
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

export interface NativeRuntime {
  isNative: boolean
  setLockScreen(enabled: boolean): Promise<string>
  clearStaleLockScreenBackup(): Promise<string>
  lockScreenDiagnostics(): Promise<LockScreenDiagnostics>
  setAutostart(enabled: boolean): Promise<void>
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
  apiHistory(conversationId: string): Promise<ChatMessage[]>
  listenTray(listener: (event: { type: 'backend'; backend: BackendMode }) => void): Promise<() => void>
  probeHarness(): Promise<HarnessStatus>
  desktopLayoutMetrics(): Promise<{ expandedBottomInset: number; taskbarVisible: boolean }>
  scanDshPaths(): Promise<Array<{ rootPath: string; source: string }>>
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
  async clearStaleLockScreenBackup() {
    if (!await tauriAvailable()) return '浏览器预览不支持清理系统锁屏恢复点。'
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('clear_stale_lock_screen_backup')
  },
  async lockScreenDiagnostics() {
    if (!await tauriAvailable()) return { supported: false, packageIdentity: false, takeoverAvailable: false, backupExists: false, backupValid: false, staleBackup: false, managedImageReady: false, managedImageActive: false, developmentBuild: false, warnings: ['浏览器预览不支持系统锁屏诊断。'] }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<LockScreenDiagnostics>('get_lock_screen_diagnostics')
  },
  async setAutostart(enabled) {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('set_autostart', { enabled })
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
  async apiHistory(conversationId) {
    if (!await tauriAvailable()) return []
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ messages: Array<{
      id: string
      role: 'user' | 'assistant'
      content: string
      createdAt: number
      usage?: ChatMessage['usage']
    }> }>('api_history', { conversationId })
    // Message ids, timestamps and final usage are persisted by the native
    // DPAPI archive. Preserve them on resume so React keys and session cost
    // remain stable instead of fabricating a fresh zero-cost transcript.
    return result.messages.map((message) => ({
      id: message.id,
      role: message.role,
      content: message.content,
      createdAt: message.createdAt,
      usage: message.usage,
    }))
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
  async desktopLayoutMetrics() {
    if (!await tauriAvailable()) return { expandedBottomInset: 48, taskbarVisible: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ expandedBottomInset: number; taskbarVisible: boolean }>('desktop_layout_metrics')
  },
  async scanDshPaths() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<Array<{ rootPath: string; source: string }>>('scan_dsh_paths')
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
