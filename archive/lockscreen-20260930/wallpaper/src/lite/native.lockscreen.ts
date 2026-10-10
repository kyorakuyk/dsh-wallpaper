// 归档片段（B4，2026-10）：`wallpaper/src/lite/native.ts` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原 1-2 行：文件开头。下面这行 import 同时带着 1B 的 `TranslucentTbStatus`；只恢复锁屏时去掉它 ----
// FREEZE(1A)：锁屏退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
import type { AutostartStatus, LockScreenDiagnostics, TranslucentTbStatus } from '../native/runtime.ts'

// ---- 原 28-31 行：`releaseNativeBootstrap` 之后 ----
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
export async function lockScreenDiagnostics(): Promise<LockScreenDiagnostics> {
  return invokeNative<LockScreenDiagnostics>('get_lock_screen_diagnostics')
}

// ---- 原 33-36 行：紧接上一段 ----
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
export async function setLockScreen(enabled: boolean): Promise<string> {
  return invokeNative<string>('set_lock_screen_enabled', { enabled })
}

// ---- 原 48-51 行：`desktopWallpaperFallbackStatus`（1B）之后、`autostartStatus` 之前 ----
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
export async function clearStaleLockScreenBackup(confirmed: boolean): Promise<string> {
  return invokeNative<string>('clear_stale_lock_screen_backup', { confirmed })
}

// ---- 原 76-79 行：`openTranslucentTbInstall`（1B）之后、`LiteImageSlot` 之前 ----
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
export async function openWindowsLockScreenSettings(): Promise<void> {
  await invokeNative('open_windows_lock_screen_settings')
}
