// 归档片段（B4，2026-10）：`wallpaper/src/lite/types.ts` 中系统集成（1B：TranslucentTB / 登录过渡底图）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/integrations-20260930/README.md` 的 B4 小节。

// ---- 原 14-16 行：`interface LiteSettings` 内，`autostart` 之前。第 14 行的文档注释属于 `desktopWallpaperFallback`（冻结后它误挂在了 `autostart` 上，B4 一并移出） ----
  /** Keep Explorer's desktop wallpaper on the sleep artwork during startup. */
  // FREEZE(1B)：随系统集成冻结（2026-09-30）。
  desktopWallpaperFallback: boolean
