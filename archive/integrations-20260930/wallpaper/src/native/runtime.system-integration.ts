// 归档片段（B4，2026-10）：`wallpaper/src/native/runtime.ts` 中系统集成（1B：TranslucentTB / 登录过渡底图）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/integrations-20260930/README.md` 的 B4 小节。

// ---- 原 28-29 行：`NativeSendOptions` 之后（原第 30 行是锁屏的 `LockScreenDiagnostics`，见锁屏归档） ----
// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一项随之冻结。恢复办法：取消注释。
export interface TranslucentTbStatus { installed: boolean; running: boolean; source?: string }

// ---- 原 466-469 行：`interface NativeRuntime` 内，`ensureProfileBridge` 之后 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一项随之冻结。恢复办法：取消注释。
  translucentTbStatus(): Promise<TranslucentTbStatus>
  launchTranslucentTb(): Promise<void>
  openTranslucentTbInstall(): Promise<void>

// ---- 原 755-768 行：`nativeRuntime` 对象内，`ensureProfileBridge` 之后 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一项随之冻结。恢复办法：取消注释。
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
