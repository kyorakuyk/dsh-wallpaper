// 归档片段（B4，2026-10）：`wallpaper/src/settings/settingsProbes.ts` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原 29-31 行：`SETTINGS_PROBES` 内，`deepseekWebAdapterConfig` 之后、`autostartStatus` 之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
  // 恢复办法：取消这里的注释，并把下面 PAGE_PROBES.system 与 PROBE_ERROR_MESSAGES 里同名的两处一起还原。
  'lockScreenDiagnostics',

// ---- 原 53-54 行：`PAGE_PROBES` 内，`history` 之后。恢复时用这一行替换现役的 `system: ['autostartStatus', 'updateStatus']`，并保留 `updateStatus` ----
  // FREEZE(1A)：同上。
  system: ['lockScreenDiagnostics', 'autostartStatus'],

// ---- 原 203-204 行：`PROBE_ERROR_MESSAGES` 内，`deepseekWebAdapterConfig` 之后、`autostartStatus` 之前 ----
  // FREEZE(1A)：同上。
  lockScreenDiagnostics: 'settings.probe.lock-screen',
