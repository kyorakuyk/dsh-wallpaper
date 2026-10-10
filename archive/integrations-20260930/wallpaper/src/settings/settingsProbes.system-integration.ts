// 归档片段（B4，2026-10）：`wallpaper/src/settings/settingsProbes.ts` 中系统集成（1B：TranslucentTB / 登录过渡底图）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/integrations-20260930/README.md` 的 B4 小节。

// ---- 原 25-26 行：`SETTINGS_PROBES` 的第一项（`managedDsh` 之前） ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30），这条探针退出。恢复办法：取消注释。
  'translucentTb',

// ---- 原 47-48 行：`PAGE_PROBES` 内，`general` 之后。恢复时用这一行替换现役的 `connections` ----
    // FREEZE(1B)：
  connections: ['managedDsh', 'translucentTb', 'deepseekWebAdapterConfig'],

// ---- 原 65-66 行：`LOW_PRIORITY_PROBES` 的第一项 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30），这条探针退出。恢复办法：取消注释。
  'translucentTb',

// ---- 原 199-200 行：`PROBE_ERROR_MESSAGES` 的第一项 ----
  // FREEZE(1B)：
  translucentTb: 'settings.probe.translucent-tb',
