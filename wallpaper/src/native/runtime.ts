import type { BackendMode, ChatEvent, ChatMessage } from '../domain/types.ts'
import type { HarnessStatus } from '../connect/harness.ts'

export interface NativeSendOptions {
  conversationId?: string
  baseUrl?: string
  model?: string
}

export interface TranslucentTbStatus { installed: boolean; running: boolean; source?: string }
export interface LockScreenDiagnostics { supported: boolean; originalImageUri?: string; backupExists: boolean; backupValid: boolean; managedImageReady: boolean; managedImageActive: boolean; developmentBuild: boolean; warnings: string[] }

export interface NativeRuntime {
  isNative: boolean
  setLockScreen(enabled: boolean): Promise<string>
  lockScreenDiagnostics(): Promise<LockScreenDiagnostics>
  setAutostart(enabled: boolean): Promise<void>
  translucentTbStatus(): Promise<TranslucentTbStatus>
  launchTranslucentTb(): Promise<void>
  openTranslucentTbInstall(): Promise<void>
  saveApiKey(key: string): Promise<void>
  requestDeepSeekLogin(): Promise<void>
  listenSystem(listener: (event: 'locked' | 'unlocked' | 'suspend' | 'resume') => void): Promise<() => void>
  listenChat(listener: (event: ChatEvent) => void): Promise<() => void>
  sendChat(mode: BackendMode, text: string, options?: NativeSendOptions): Promise<string | undefined>
  cancelChat(mode: BackendMode): Promise<void>
  connectHarness(resumeSessionId?: string): Promise<string>
  harnessHistory(): Promise<ChatMessage[]>
  apiHistory(conversationId: string): Promise<ChatMessage[]>
  listenTray(listener: (event: { type: 'backend'; backend: BackendMode } | { type: 'settings' }) => void): Promise<() => void>
  probeHarness(): Promise<HarnessStatus>
}

async function tauriAvailable(): Promise<boolean> {
  return '__TAURI_INTERNALS__' in window
}

export const nativeRuntime: NativeRuntime = {
  isNative: '__TAURI_INTERNALS__' in window,
  async setLockScreen(enabled) {
    if (!await tauriAvailable()) return '浏览器预览不支持设置系统锁屏。'
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('set_lock_screen_enabled', { enabled })
  },
  async lockScreenDiagnostics() {
    if (!await tauriAvailable()) return { supported: false, backupExists: false, backupValid: false, managedImageReady: false, managedImageActive: false, developmentBuild: false, warnings: ['浏览器预览不支持系统锁屏诊断。'] }
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
  async saveApiKey(key) {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持 Windows 凭据管理器')
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('save_api_key', { key })
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
    return listen<ChatEvent>('chat-event', (event) => listener(event.payload))
  },
  async sendChat(mode, text, options) {
    const { invoke } = await import('@tauri-apps/api/core')
    return (await invoke<string | null>('send_chat', { mode, text, conversationId: options?.conversationId, baseUrl: options?.baseUrl, model: options?.model })) ?? undefined
  },
  async cancelChat(mode) {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('cancel_chat', { mode })
  },
  async connectHarness(resumeSessionId) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('connect_harness', { resumeSessionId })
  },
  async harnessHistory() {
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ messages: Array<{ id: string; role: 'user' | 'assistant'; content: string }> }>('harness_history')
    return result.messages.map((message) => ({ ...message, createdAt: Date.now() }))
  },
  async apiHistory(conversationId) {
    if (!await tauriAvailable()) return []
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ messages: Array<{ role: 'user' | 'assistant'; content: string }> }>('api_history', { conversationId })
    return result.messages.map((message) => ({ ...message, id: crypto.randomUUID(), createdAt: Date.now() }))
  },
  async listenTray(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    const backendDispose = await listen<BackendMode>('tray-backend', (event) => listener({ type: 'backend', backend: event.payload }))
    const settingsDispose = await listen('tray-settings', () => listener({ type: 'settings' }))
    return () => { backendDispose(); settingsDispose() }
  },
  async probeHarness() {
    if (!await tauriAvailable()) return { availability: 'offline' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessStatus>('probe_harness')
  },
}
