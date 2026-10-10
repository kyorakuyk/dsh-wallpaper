// 归档片段（B4，2026-10）：`wallpaper/src/native/runtime.ts` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原第 30 行：现役导出 `LockScreenDiagnostics`（`NativeSendOptions` 之后、`ManagedDshInstance` 的文档注释之前）；B4 时已无现役引用，从现役删除 ----
export interface LockScreenDiagnostics { supported: boolean; packageIdentity: boolean; takeoverAvailable: boolean; originalImageUri?: string; backupExists: boolean; backupValid: boolean; staleBackup: boolean; managedImageReady: boolean; managedImageActive: boolean; developmentBuild: boolean; warnings: string[] }

// ---- 原 453-458 行：`interface NativeRuntime` 内，`isNative` 之后、`setAutostart` 之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  setLockScreen(enabled: boolean): Promise<string>
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  clearStaleLockScreenBackup(confirmed: boolean): Promise<string>
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  lockScreenDiagnostics(): Promise<LockScreenDiagnostics>

// ---- 原 470-471 行：`interface NativeRuntime` 内，TranslucentTB 三个方法（1B）之后、`saveApiKey` 的文档注释之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  openWindowsLockScreenSettings(): Promise<void>

// ---- 原 723-740 行：`nativeRuntime` 对象内，`isNative` 之后、`setAutostart` 之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  async setLockScreen(enabled) {
    if (!await tauriAvailable()) return '浏览器预览不支持设置系统锁屏。'
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('set_lock_screen_enabled', { enabled })
  },
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  async clearStaleLockScreenBackup(confirmed) {
    if (!await tauriAvailable()) return '浏览器预览不支持清理系统锁屏恢复点。'
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('clear_stale_lock_screen_backup', { confirmed })
  },
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  async lockScreenDiagnostics() {
    if (!await tauriAvailable()) return { supported: false, packageIdentity: false, takeoverAvailable: false, backupExists: false, backupValid: false, staleBackup: false, managedImageReady: false, managedImageActive: false, developmentBuild: false, warnings: ['浏览器预览不支持系统锁屏诊断。'] }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<LockScreenDiagnostics>('get_lock_screen_diagnostics')
  },

// ---- 原 769-774 行：`nativeRuntime` 对象内，TranslucentTB 三个方法（1B）之后、`saveApiKey` 之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  async openWindowsLockScreenSettings() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_windows_lock_screen_settings')
  },
