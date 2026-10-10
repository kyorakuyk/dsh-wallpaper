// 归档片段（B4，2026-10）：`wallpaper/src/settings/store.ts` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原第 192 行：`interface WallpaperSettings` 内，`skipWakeAnimation` 之后、`autostart` 之前 ----
  lockScreenEnabled: boolean

// ---- 原第 255 行：`DEFAULT_SETTINGS` 内，`skipWakeAnimation` 之后、`autostart` 之前 ----
  lockScreenEnabled: false,

// ---- 原第 391 行：`normalizeSettings` 的返回对象内，`skipWakeAnimation` 之后、`autostart` 之前 ----
    lockScreenEnabled: settingsBool(value.lockScreenEnabled, DEFAULT_SETTINGS.lockScreenEnabled),
