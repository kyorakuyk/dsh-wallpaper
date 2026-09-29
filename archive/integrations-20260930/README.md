# 归档：系统集成（2026-09-30）

用户决定**系统集成暂时只保留开机自启**，其余整块退出——整文件归档到这里，代码块在原处按 FREEZE 注释。

| 归档文件 | 原位置 | 为什么 |
| --- | --- | --- |
| `wallpaper/src-tauri/src/desktop_fallback.rs`（582 行） | `wallpaper/src-tauri/src/` | 登录过渡底图：临时把 Explorer 桌面换成熟睡画面。它只被 `lib.rs` 里两条 `#[cfg(feature = "lite")]` 命令使用，那两条命令连同它的 `mod` 一起冻结了 |

同一批冻结的还有（仍在原位、按 FREEZE 注释）：`lib.rs` 里的 `TranslucentTbStatus` 与三个 TranslucentTB 命令、
两条桌面底图命令、以及它们在两处 `invoke_handler` 里的八个注册项。前端侧（完整版"透明任务栏"卡片、Lite 的
`04 · COMPATIBILITY` 卡片与"登录过渡底图"行、两个门面的包装、设置字段）已经先一步冻结。

**开机自启**（`set_autostart` / `autostart_status` 与 `HKCU\...\Run` 那条链）**保持不动**。

恢复办法：`git mv` 回来，并取消 `lib.rs` 里对应段的注释（每段前面都写了恢复提示）。