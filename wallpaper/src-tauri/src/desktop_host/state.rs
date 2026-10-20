use super::hit_region::InteractionRegionState;
use std::sync::{OnceLock, RwLock};

#[cfg(windows)]
use super::WallpaperHostStatus;
#[cfg(windows)]
use std::sync::atomic::AtomicBool;

#[cfg(windows)]
pub(super) static INNER_WORKSPACE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(super) static INTERACTION_REGIONS: OnceLock<RwLock<InteractionRegionState>> = OnceLock::new();

#[cfg(windows)]
pub(super) static WALLPAPER_HOST_STATUS: OnceLock<RwLock<WallpaperHostStatus>> = OnceLock::new();

/// 监控线程用的"当前窗口句柄槽位"。
///
/// 一、它解决什么问题。三条监视线程（`start_desktop_workspace_monitor`、
/// `start_foreground_monitor`、`floating_ball::start_ball_monitor`）不许在轮询里回 Tauri
/// 取窗口（`get_webview_window` 会 `Webview::clone`，与 wry 的 `Context` 共用同一份引用
/// 计数，见本文件顶部那段说明），于是句柄只能以**纯值**形式带进线程。此前那个纯值是在
/// 线程启动时解析一次就固定下来的，代价是：`background` 一旦被销毁并重建
/// （`recover_wallpaper_host` 的 `Recreate` 分支，自愈线程每 2 秒跑一次），线程手里的旧
/// 句柄永远不再有效，`IsWindow` 会一直回答否，于是桌面工作区监视与前台监视退场、悬浮球
/// 不再弹出——功能在窗口重建后**永久**失效，而继续取窗口又是要修的崩溃。句柄放进共享槽位
/// 就把这两件事同时解决了：值的来源仍是主线程的解析，值的传递只是主线程的一次原子写。
///
/// 二、读的一侧绝不碰 Tauri。监视线程每拍只读这个原子值，拿到的是整数，不携带任何引用
/// 计数。**这里没有任何"重新解析句柄"的路径**：重新解析必然要在后台线程调
/// `get_webview_window`，那正是本轮要根治的崩溃来源，所以宁可让线程"看到 0 就等"，
/// 也不许它自己去取窗口对象。
///
/// 三、谁写。句柄在**窗口创建的地方**由主线程写入：`lib.rs` 的启动接线
/// （`publish_current_window_channels`，写 `background` 那一格）、`create_background_window`
/// （重建 `background` 之后，同样只写那一格）、`floating_ball::ensure_ball`（球的唯一出生点，
/// 写 `ball` 那一格）。这三处是全仓仅有的窗口出生点。
///
/// 四、句柄为 0 表示什么。0 是"此刻没有窗口"，不是"永久没有窗口"：`background` 由 Tauri
/// 配置创建，而悬浮球在启动接线里晚于本槽位的第一写。读取方拿到 0 或已销毁句柄时该退场
/// 还是该等，由各线程自己按语义决定（见 `background_handle_is_usable`）。
#[cfg(windows)]
pub(super) static BACKGROUND_WINDOW_HANDLE: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

/// 见 `BACKGROUND_WINDOW_HANDLE`。球自己的窗口句柄也走同一套槽位。
#[cfg(windows)]
pub(super) static BALL_WINDOW_HANDLE: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);
