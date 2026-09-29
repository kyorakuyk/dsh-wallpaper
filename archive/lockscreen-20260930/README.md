# 归档：锁屏相关（2026-09-30）

用户决定**壁纸不再触碰锁屏**：现在做的和用户自己在 Windows 设置里换一张没有区别，却多出风险，
而且不能回滚。所以整块退出——整文件进这里归档，代码块在原处按 FREEZE 注释。

| 归档文件 | 原位置 |
| --- | --- |
| wallpaper/src-tauri/src/bin/lockscreen_probe.rs | 独立的锁屏诊断探针（cargo feature `lockscreen-probe`） |
| scripts/build-lockscreen-probe.ps1 | 构建并安装那个探针包的脚本 |
| packaging/msix/LockScreenProbe.AppxManifest.xml | 探针的 MSIX 清单 |

恢复办法：把文件放回原位，并取消 `wallpaper/src-tauri/Cargo.toml` 里那段注释
（`lockscreen-probe` feature 与 `[[bin]]`）。其余仍在原处、按 FREEZE 注释的代码块见
`docs/plans/release-scope-cleanup-plan.md` 第一节。