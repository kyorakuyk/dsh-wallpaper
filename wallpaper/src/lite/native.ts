// FREEZE(1A)：锁屏退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
// import type { AutostartStatus, LockScreenDiagnostics, TranslucentTbStatus } from '../native/runtime.ts'
import type { AutostartStatus, TranslucentTbStatus } from '../native/runtime.ts'

export interface DesktopWallpaperFallbackStatus {
  managedActive: boolean
  backupExists: boolean
  warning?: string
}

async function invokeNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

export async function nativeBootstrapGeneration(): Promise<number> {
  return invokeNative<number>('native_bootstrap_generation')
}

export async function releaseNativeBootstrap(generation: number): Promise<boolean> {
  return invokeNative<boolean>('release_native_bootstrap', { generation })
}

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  // export async function lockScreenDiagnostics(): Promise<LockScreenDiagnostics> {
  //   return invokeNative<LockScreenDiagnostics>('get_lock_screen_diagnostics')
  // }

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  // export async function setLockScreen(enabled: boolean): Promise<string> {
  //   return invokeNative<string>('set_lock_screen_enabled', { enabled })
  // }

export async function setDesktopWallpaperFallback(enabled: boolean): Promise<string> {
  return invokeNative<string>('set_desktop_wallpaper_fallback', { enabled })
}

export async function desktopWallpaperFallbackStatus(): Promise<DesktopWallpaperFallbackStatus> {
  return invokeNative<DesktopWallpaperFallbackStatus>('desktop_wallpaper_fallback_status')
}

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  // export async function clearStaleLockScreenBackup(confirmed: boolean): Promise<string> {
  //   return invokeNative<string>('clear_stale_lock_screen_backup', { confirmed })
  // }

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

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  // export async function openWindowsLockScreenSettings(): Promise<void> {
  //   await invokeNative('open_windows_lock_screen_settings')
  // }

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
