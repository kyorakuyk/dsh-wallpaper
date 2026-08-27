import type { AutostartStatus, LockScreenDiagnostics, TranslucentTbStatus } from '../native/runtime.ts'

async function invokeNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

export async function releaseNativeBootstrap(): Promise<void> {
  await invokeNative('release_native_bootstrap')
}

export async function lockScreenDiagnostics(): Promise<LockScreenDiagnostics> {
  return invokeNative<LockScreenDiagnostics>('get_lock_screen_diagnostics')
}

export async function setLockScreen(enabled: boolean): Promise<string> {
  return invokeNative<string>('set_lock_screen_enabled', { enabled })
}

export async function autostartStatus(): Promise<AutostartStatus> {
  return invokeNative<AutostartStatus>('autostart_status')
}

export async function setAutostart(enabled: boolean): Promise<AutostartStatus> {
  return invokeNative<AutostartStatus>('set_autostart', { enabled })
}

export async function translucentTbStatus(): Promise<TranslucentTbStatus> {
  return invokeNative<TranslucentTbStatus>('translucent_tb_status')
}

export async function launchTranslucentTb(): Promise<void> {
  await invokeNative('launch_translucent_tb')
}

export async function openTranslucentTbInstall(): Promise<void> {
  await invokeNative('open_translucent_tb_install')
}

export async function openWindowsLockScreenSettings(): Promise<void> {
  await invokeNative('open_windows_lock_screen_settings')
}

export type LiteImageSlot = 'background' | 'portrait'

export interface LiteImageData {
  mimeType: string
  bytesBase64: string
}

export async function chooseImage(slot: LiteImageSlot): Promise<string | undefined> {
  if (!('__TAURI_INTERNALS__' in window)) return undefined
  const { open } = await import('@tauri-apps/plugin-dialog')
  const selected = await open({
    title: slot === 'background' ? '选择壁纸背景' : '选择立绘',
    multiple: false,
    directory: false,
    filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
  })
  return typeof selected === 'string' ? selected : undefined
}

export async function importImage(slot: LiteImageSlot, path: string): Promise<void> {
  await invokeNative('lite_image_import', { slot, path })
}

export async function resolveImage(slot: LiteImageSlot): Promise<string | undefined> {
  if (!('__TAURI_INTERNALS__' in window)) return undefined
  const image = await invokeNative<LiteImageData | null>('lite_image_resolve', { slot })
  return image ? `data:${image.mimeType};base64,${image.bytesBase64}` : undefined
}
