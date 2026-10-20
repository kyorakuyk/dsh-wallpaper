use std::sync::{OnceLock, RwLock};

// 本功能的判据与配对见 docs/evidence/desktop-blank-double-click.md：判据是 MSAA 三态
// （Icon / Blank / Unknown，只有 Blank 允许切换），配对是两态状态机（严格两两匹配）。
// 中途曾按用户要求把这一整块冻结并回退过一次，那一代实现仍在 git 历史里（c9a9665 到 dc49983），
// 但其中的做法已被该文档判定为在 UIA 上原地调参的一类弯路，不要照它重走。

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::app_core::{AppAction, AppCore, WallpaperHostMode, WallpaperHostStatus};
#[cfg(windows)]
use crate::native_bootstrap;

#[cfg(windows)]
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[cfg(windows)]
mod events;
#[cfg(windows)]
pub(crate) use events::{
    dispatch_system_action, emit_system_session, emit_to_background, BACKGROUND_WINDOW_LABEL,
};

#[cfg(windows)]
mod guard;
#[cfg(windows)]
pub(crate) use guard::assert_not_acting_as_main_thread;

mod display;
#[cfg(windows)]
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use display::desktop_layout_metrics_for_hwnd;
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use display::{
    desktop_displays, desktop_layout_metrics, DesktopDisplayInfo, DesktopLayoutMetrics, DesktopRect,
};

mod autostart;
pub(crate) use autostart::{
    autostart_status, migrate_legacy_autostart, set_autostart, AutostartStatus,
};
#[cfg(not(feature = "lite"))]
pub(crate) use autostart::{current_package_version, uninstall_display_version, PackageVersionProbe};

mod settings_frame;
pub(crate) use settings_frame::configure_settings_window;

mod state;
mod hit_region;
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use hit_region::{
    begin_interaction_region_session, island_visible_from_regions, update_interaction_regions,
    InteractionRegionInput, InteractionRegionUpdateResult,
};
#[allow(unused_imports)]
pub(crate) use hit_region::ISLAND_REGION_ID;
#[cfg(windows)]
use hit_region::{
    cursor_hits_interaction_region, install_desktop_hit_testing, interaction_regions,
    MAX_SCALE_FACTOR, MIN_SCALE_FACTOR,
};
#[cfg(windows)]
use state::{
    BALL_WINDOW_HANDLE, BACKGROUND_WINDOW_HANDLE, INNER_WORKSPACE_ACTIVE,
    WALLPAPER_HOST_STATUS,
};

#[cfg(windows)]
use windows::core::Interface;
use windows::{
    core::{w, BOOL, PCWSTR},
    Win32::{
        Foundation::{
            GetLastError, SetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HWND, LPARAM, LRESULT,
            POINT, RECT, WPARAM,
        },
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND},
            Gdi::ClientToScreen,
        },
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
            // `AttachThreadInput` lives here in windows 0.61, not under KeyboardAndMouse.
            Threading::{AttachThreadInput, GetCurrentThreadId},
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
            Threading::CreateMutexW,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetClientRect, GetCursorPos,
            GetDesktopWindow, GetForegroundWindow, GetParent, GetWindow, GetWindowLongPtrW,
            IsWindow, IsWindowVisible, SendMessageTimeoutW, SetParent,
            SetWindowLongPtrW, SetWindowPos, ShowWindow, WindowFromPoint, GWL_EXSTYLE, GWL_STYLE,
            GW_HWNDNEXT, GW_HWNDPREV, HWND_TOP,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SEND_MESSAGE_TIMEOUT_FLAGS, SMTO_NORMAL,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
            GetAncestor, GetWindowThreadProcessId, GetGUIThreadInfo, SetForegroundWindow, GA_ROOT, SW_HIDE, SW_SHOWNA, WM_DISPLAYCHANGE, WM_DPICHANGED, GUITHREADINFO,
            WM_NCDESTROY, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WS_BORDER,
            WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
            WS_EX_STATICEDGE, WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
        UI::{
            Accessibility::{AccessibleObjectFromWindow, IAccessible, CUIAutomation, IUIAutomation, UIA_ListItemControlTypeId},
            Input::KeyboardAndMouse::{
            GetAsyncKeyState, SetFocus, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        },
    },
};

#[cfg(windows)]
const PROGMAN_SPAWN_WORKERW: u32 = 0x052C;
#[cfg(windows)]
const WORKERW_RETRY_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const WORKERW_RETRY_WINDOW: Duration = Duration::from_millis(1_500);

/// Full and Lite are separate packages, but they must never both own the
/// Explorer wallpaper host. A shared user-session mutex closes that gap while
/// still allowing the normal per-package single-instance plugin to handle
/// duplicate launches within one edition.
#[cfg(windows)]
const WALLPAPER_HOST_MUTEX_NAME: windows::core::PCWSTR = w!("Local\\DSHWallpaper.DesktopHost.v1");

#[cfg(windows)]
// Store only the raw value in the static: the windows crate's HANDLE wraps a
// raw pointer and intentionally is not Send/Sync, while this process-local
// guard must be visible from the startup path without a mutex of its own.
static WALLPAPER_HOST_MUTEX: OnceLock<isize> = OnceLock::new();

#[cfg(windows)]
pub fn acquire_shared_wallpaper_host() -> bool {
    // SAFETY: CreateMutexW takes a static `w!` name and no out-parameters; the returned handle is checked for errors before use.
    let handle = match unsafe { CreateMutexW(None, true, WALLPAPER_HOST_MUTEX_NAME) } {
        Ok(handle) => handle,
        Err(error) => {
            log::error!("无法建立壁纸宿主互斥锁：{error}");
            return false;
        }
    };
    // SAFETY: GetLastError reads thread-local state only and runs immediately after CreateMutexW, before another Win32 call can overwrite it.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: this closes the handle created above, which was not shared; a duplicate-name result does not transfer ownership. The result is ignored on purpose.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
        log::warn!("检测到另一个 DSH Wallpaper 版本正在运行；本实例不会接管桌面宿主");
        return false;
    }
    let _ = WALLPAPER_HOST_MUTEX.set(handle.0 as isize);
    true
}

#[cfg(windows)]
static WALLPAPER_RECOVERY_QUEUED: AtomicBool = AtomicBool::new(false);







#[cfg(windows)]
static DESKTOP_FOREGROUND_STATE: OnceLock<RwLock<Option<bool>>> = OnceLock::new();



#[cfg(windows)]
fn desktop_foreground_state() -> &'static RwLock<Option<bool>> {
    DESKTOP_FOREGROUND_STATE.get_or_init(|| RwLock::new(None))
}

#[cfg(windows)]
fn wallpaper_host_status_state() -> &'static RwLock<WallpaperHostStatus> {
    WALLPAPER_HOST_STATUS.get_or_init(|| RwLock::new(WallpaperHostStatus::default()))
}

/// 主线程写入当前 `background` / `ball` 的句柄（0 表示此刻没有）。
///
/// 两格是"此刻真实存在的窗口"这个语义，所以**重建 `background` 的路径不许只写半格**：
/// 那条路径上球通常还活着，把它写成 0 等于让球监控线程以为球没了而退场。因此重建点用
/// `publish_background_window_handle`，`ball` 那一格由 `floating_ball::ensure_ball` 自己维护。
#[cfg(windows)]
fn publish_window_channels(background: Option<HWND>, ball: Option<HWND>) {
    publish_background_window_handle(background);
    BALL_WINDOW_HANDLE.store(ball.map_or(0, |handle| handle.0 as isize), Ordering::Release);
}

/// 只写 `background` 那一格（重建路径专用，理由见 `publish_window_channels`）。
#[cfg(windows)]
pub(crate) fn publish_background_window_handle(background: Option<HWND>) {
    BACKGROUND_WINDOW_HANDLE.store(
        background.map_or(0, |handle| handle.0 as isize),
        Ordering::Release,
    );
}

/// 把槽位的两格一起刷成"此刻真实存在的窗口"。
///
/// **只在主线程调用**：`background` 那半格要走 `get_webview_window`（即 `Webview::clone`，
/// 见本文件顶部那段说明），后台线程一旦调它就是要修的崩溃本身。球的半格由调用方给出：
/// 悬浮球模块在 Lite 目标里根本不存在（`lib.rs` 里 `mod floating_ball` 受
/// `not(feature = "lite")` 门控），所以这里不能自己去点名它。
/// 调用点是 `lib.rs` 的启动接线（窗口由 Tauri 配置建好之后、三条监视线程启动之前）。
#[cfg(windows)]
pub(crate) fn publish_current_window_channels(app: &tauri::AppHandle, ball: Option<HWND>) {
    let background = app
        .get_webview_window(BACKGROUND_WINDOW_LABEL)
        .and_then(|window| window.hwnd().ok())
        .map(|handle| HWND(handle.0));
    publish_window_channels(background, ball);
}

/// 当前 `background` 句柄的纯值形式。0 = 此刻没有窗口。
///
/// 只读原子量，不碰 Tauri、不碰窗口对象，后台线程可以每拍调用。
#[cfg(windows)]
pub(crate) fn background_window_handle_now() -> isize {
    BACKGROUND_WINDOW_HANDLE.load(Ordering::Acquire)
}

/// 当前悬浮球句柄的纯值形式。0 = 此刻没有窗口。
#[cfg(windows)]
pub(crate) fn ball_window_handle_now() -> isize {
    BALL_WINDOW_HANDLE.load(Ordering::Acquire)
}

/// 主线程写入当前悬浮球句柄（0 = 此刻没有）。**只在主线程调用**，由悬浮球模块在
/// 它唯一的创建点 `ensure_ball` 里写。
#[cfg(windows)]
pub(crate) fn publish_ball_window_handle(ball: Option<HWND>) {
    BALL_WINDOW_HANDLE.store(ball.map_or(0, |handle| handle.0 as isize), Ordering::Release);
}

/// 槽位里的句柄此刻是不是**一个能用的桌面宿主窗口**。
///
/// 判据刻意**不**把"句柄在但已销毁"当作不可用：那正是重建的中间态
/// （`recover_wallpaper_host` 的 `Recreate` 分支先 `destroy` 再 `create`）。
/// 句柄只解析一次的旧写法在这段里把线程结束了，重建完成也回不来——这就是本轮要修的
/// "窗口重建后功能失效"。因此这里只在**槽位为空**（连一个候选都没有）时说不；
/// 其余情况一律认为"等一下就会有"。
///
/// 三条监视线程据此决定这一拍做不做事：
///   * `background` 相关的两条（桌面工作区、前台）：句柄不可用就跳过这一拍、**继续等**，
///     因为 `background` 在正常运行期始终存在，重建由主线程每 2 秒一次的自愈线程完成，
///     "等"不会变成永久的无主轮询。
///   * 悬浮球的球句柄语义**不同**，由 `floating_ball::current_ball_handle` 自己判断
///     （全仓只有 `ensure_ball` 一个出生点，球没了就是进程在收尾，该退场）。
#[cfg(windows)]
fn background_handle_is_usable(handle: isize) -> bool {
    if handle == 0 {
        return false;
    }
    keep_watching_after_tick(
        true,
        // SAFETY: the integer was published by the main thread as a window handle; IsWindow only inspects it and reports false for a stale one.
        unsafe { IsWindow(Some(HWND(handle as *mut core::ffi::c_void))).as_bool() },
    )
}

/// `background_handle_is_usable` 里那个与 `IsWindow` 无关的部分。
///
/// 它存在的唯一理由是可测：`IsWindow` 需要一个真正的窗口句柄，而单测里造不出
/// （句柄空间是会话级的，随便写一个整数当句柄是在赌另一个进程没有占用它，那种测试只会
/// 偶发失败，见本模块测试里的说明）。所以判据被拆成"槽位里有没有候选"与"候选是不是活的"
/// 两步，前一步可以在这里断言，"已销毁"那一步由调用方传进来。
///
/// 现在的答案只在槽位为空时为否（理由见 `background_handle_is_usable`）。保留两个参数是
/// 为了让测试能把四种组合都摆出来，防止将来有人把"已销毁"误当成不可用。
#[cfg(windows)]
fn keep_watching_after_tick(handle_present: bool, _handle_alive: bool) -> bool {
    handle_present
}

#[cfg(windows)]
fn publish_wallpaper_host_status(
    app: &tauri::AppHandle,
    mode: WallpaperHostMode,
    recovered: bool,
    error: Option<String>,
) {
    let status = wallpaper_host_status_state()
        .write()
        .map(|mut status| {
            let mode_changed = status.mode != mode;
            let error_changed = status.last_error != error;
            if mode_changed || error_changed || recovered {
                status.generation = status.generation.saturating_add(1);
            }
            if recovered {
                status.recovery_count = status.recovery_count.saturating_add(1);
            }
            status.mode = mode;
            status.last_error = error;
            status.clone()
        })
        .unwrap_or_else(|_| WallpaperHostStatus {
            mode: WallpaperHostMode::Unavailable,
            generation: 0,
            recovery_count: 0,
            last_error: Some("wallpaper host status state poisoned".into()),
        });
    if let Some(core) = app.try_state::<AppCore>() {
        let before = core.snapshot().wallpaper_host;
        if before != status {
            match status.mode {
                WallpaperHostMode::WorkerW => {
                    let point = crate::startup_timeline::sample();
                    log::info!(
                        "wallpaper host ready: mode=workerw generation={} recoveries={} absolute={} elapsed_ms={}",
                        status.generation,
                        status.recovery_count,
                        point.wall_clock,
                        point.anchor_ms
                    );
                    // 这一行说的是「宿主窗口就绪」：从主机进程入口算起多少毫秒。
                    // 它只在状态真的变化时到达这里，不是每轮轮询都打。
                    crate::native_bootstrap::record_startup_diagnostic(
                        "event=host-ready-workerw 宿主窗口就绪（mode=workerw）",
                    );
                }
                WallpaperHostMode::ProgmanFallback => log::warn!(
                    "wallpaper host degraded: mode=progman generation={} recoveries={}",
                    status.generation,
                    status.recovery_count
                ),
                WallpaperHostMode::Recovering => log::warn!(
                    "wallpaper host recovering: generation={} recoveries={}",
                    status.generation,
                    status.recovery_count
                ),
                WallpaperHostMode::Unavailable => log::error!(
                    "wallpaper host unavailable: {}",
                    status.last_error.as_deref().unwrap_or("unknown error")
                ),
                WallpaperHostMode::Starting => {}
            }
            let snapshot = core.dispatch(AppAction::SetWallpaperHost(status));
            emit_to_background(app, "app-snapshot", &snapshot);
        }
    }
}





#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DesktopWindowCandidate {
    is_worker: bool,
    hosts_desktop_icons: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WallpaperHostAction {
    None,
    Resize,
    Reattach,
    Recreate,
}

fn is_valid_wallpaper_parent_class(class: Option<&str>) -> bool {
    matches!(class, Some("WorkerW") | Some("Progman"))
}

fn is_desktop_foreground_class(class: Option<&str>) -> bool {
    // Explorer can own several WorkerW windows. Win+D is free to activate any
    // of them, so comparing against FindWindowW("WorkerW") (the first match)
    // incorrectly hides the overlay on otherwise valid desktop surfaces.
    matches!(class, Some("WorkerW") | Some("Progman"))
}

fn should_upgrade_wallpaper_parent(current_class: Option<&str>, worker_available: bool) -> bool {
    current_class == Some("Progman") && worker_available
}

fn select_wallpaper_worker(candidates: &[DesktopWindowCandidate]) -> Option<usize> {
    let icon_host = candidates
        .iter()
        .position(|candidate| candidate.hosts_desktop_icons);
    let ordered_worker = icon_host.and_then(|icon_host| {
        candidates
            .iter()
            .enumerate()
            .skip(icon_host + 1)
            .find_map(|(index, candidate)| {
                (candidate.is_worker && !candidate.hosts_desktop_icons).then_some(index)
            })
    });
    ordered_worker.or_else(|| {
        candidates
            .iter()
            .enumerate()
            .find_map(|(index, candidate)| {
                (candidate.is_worker && !candidate.hosts_desktop_icons).then_some(index)
            })
    })
}

fn decide_wallpaper_host_action(
    background_exists: bool,
    parent_exists: bool,
    parent_is_worker: bool,
    size_matches: bool,
) -> WallpaperHostAction {
    if !background_exists {
        WallpaperHostAction::Recreate
    } else if !parent_exists || !parent_is_worker {
        WallpaperHostAction::Reattach
    } else if !size_matches {
        WallpaperHostAction::Resize
    } else {
        WallpaperHostAction::None
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct EnumeratedWindow {
    hwnd: HWND,
    candidate: DesktopWindowCandidate,
}

#[cfg(windows)]
unsafe extern "system" fn collect_desktop_windows(window: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Vec<EnumeratedWindow>);
    let mut class = [0u16; 64];
    let class_len = GetClassNameW(window, &mut class);
    let is_worker =
        class_len > 0 && String::from_utf16_lossy(&class[..class_len as usize]) == "WorkerW";
    // `FindWindowExW` returns `Ok(HWND(0))` when no matching child exists in
    // windows-rs.  Checking only `Result::is_ok()` therefore marks every
    // top-level window as an icon host and makes WorkerW selection fail.
    let hosts_desktop_icons = FindWindowExW(
        Some(window),
        None,
        windows::core::w!("SHELLDLL_DefView"),
        PCWSTR::null(),
    )
    .ok()
    .map(|hwnd| !hwnd.0.is_null())
    .unwrap_or(false);
    windows.push(EnumeratedWindow {
        hwnd: window,
        candidate: DesktopWindowCandidate {
            is_worker,
            hosts_desktop_icons,
        },
    });
    BOOL(1)
}

#[cfg(windows)]
fn enumerate_desktop_windows() -> Result<Vec<EnumeratedWindow>, String> {
    let mut windows = Vec::new();
    // SAFETY: LPARAM carries `&mut windows`, which stays alive and unaliased for this synchronous enumeration, and the callback only pushes into that vector.
    unsafe {
        EnumWindows(
            Some(collect_desktop_windows),
            LPARAM(&mut windows as *mut Vec<EnumeratedWindow> as isize),
        )
        .map_err(|error| format!("无法枚举 Windows 桌面窗口：{error}"))?;
    }
    Ok(windows)
}

#[cfg(windows)]
fn locate_wallpaper_worker() -> Result<HWND, String> {
    let windows = enumerate_desktop_windows()?;
    let candidates: Vec<_> = windows.iter().map(|item| item.candidate).collect();
    select_wallpaper_worker(&candidates)
        .map(|index| windows[index].hwnd)
        .ok_or_else(|| "Explorer 未枚举独立 WorkerW 壁纸宿主".to_string())
}

#[cfg(windows)]
pub(crate) fn visible_wallpaper_worker() -> Option<HWND> {
    // SAFETY: `worker` was produced by the enumeration in this call; IsWindow only inspects the handle and reports false for one that disappeared in between.
    locate_wallpaper_worker().ok().filter(|worker| unsafe {
        IsWindow(Some(*worker)).as_bool() && IsWindowVisible(*worker).as_bool()
    })
}

#[cfg(windows)]
fn wait_for_visible_wallpaper_worker() -> Option<HWND> {
    let deadline = Instant::now() + WORKERW_RETRY_WINDOW;
    loop {
        if let Some(worker) = visible_wallpaper_worker() {
            return Some(worker);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(WORKERW_RETRY_INTERVAL);
    }
}

/// The desktop icon *layer* is owned by Explorer.  It contains both the
/// visible list items and an otherwise transparent full-screen hit-test
/// surface (`SHELLDLL_DefView`).  In the inner workspace we must hide the
/// whole layer, not only `SysListView32`: leaving DefView visible makes the
/// desktop look empty but it still consumes every mouse move and click before
/// they can reach the wallpaper WebView.
///
/// We do not move, delete, or modify individual icon items.  The exact same
/// Explorer layer is restored when returning to the front workspace or when
/// the application exits.
#[cfg(windows)]
pub(crate) fn desktop_icon_layer() -> Option<HWND> {
    enumerate_desktop_windows()
        .ok()?
        .into_iter()
        .find_map(|item| {
            if !item.candidate.hosts_desktop_icons {
                return None;
            }
            // SAFETY: `item.hwnd` came from the enumeration just above and the class and child names are static `w!` strings; a missing child is dropped by the null check.
            unsafe {
                FindWindowExW(
                    Some(item.hwnd),
                    None,
                    windows::core::w!("SHELLDLL_DefView"),
                    PCWSTR::null(),
                )
            }
            .ok()
            .filter(|hwnd| !hwnd.0.is_null())
        })
}

#[cfg(windows)]
fn desktop_icon_list_view_in_layer(icon_layer: HWND) -> Option<HWND> {
    // SAFETY: `icon_layer` is the caller's live DefView handle and the child class name is a static `w!` string; windows-rs hands the string over as a NUL-terminated PCWSTR for the duration of the call.
    unsafe {
        FindWindowExW(
            Some(icon_layer),
            None,
            windows::core::w!("SysListView32"),
            PCWSTR::null(),
        )
    }
    .ok()
    .filter(|hwnd| !hwnd.0.is_null())
}

#[cfg(windows)]
fn set_desktop_icons_visible(visible: bool) -> Result<(), String> {
    let icon_layer = desktop_icon_layer().ok_or("未找到 Explorer 桌面图标层")?;
    // SAFETY: `icon_layer` is a non-null handle returned by the lookup above and `icon_list` is the child found under it; ShowWindow only requests a visibility state and shares no Rust memory.
    unsafe {
        if visible {
            // Older development builds hid SysListView32 directly.  Restore it
            // as well so a forced restart cannot leave the normal desktop
            // visually empty after this implementation moved to DefView.
            let _ = ShowWindow(icon_layer, SW_SHOWNA);
            if let Some(icon_list) = desktop_icon_list_view_in_layer(icon_layer) {
                let _ = ShowWindow(icon_list, SW_SHOWNA);
            }
        } else {
            let _ = ShowWindow(icon_layer, SW_HIDE);
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn restore_desktop_icons() {
    INNER_WORKSPACE_ACTIVE.store(false, Ordering::Release);
    if let Err(error) = set_desktop_icons_visible(true) {
        log::warn!("未能恢复 Explorer 桌面图标：{error}");
    }
}

/// 进入里桌面：隐藏 Explorer 的图标层，并通知壁纸前端。
///
/// **这是进入里桌面的唯一实现**：桌面空白双击与悬浮球单击都必须走它，否则两条
/// 入场路径会慢慢分叉（`App.tsx` 收到 `desktop-workspace-toggle` 的 `"enter"` 后
/// 会展开输入岛并打开对话，那是 §1.1 拍板的行为）。
/// 幂等：已经在里桌面就直接返回成功，不重复 `ShowWindow`、不重复发事件。
#[cfg(windows)]
pub(crate) fn enter_inner_workspace(app: &tauri::AppHandle) -> Result<(), String> {
    if INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire) {
        return Ok(());
    }
    // 先真正把图标层藏掉，成功了再承认「已进入」：顺序反了会在失败时留下
    // 「自认为在里桌面、实际还在表桌面」的假状态。
    set_desktop_icons_visible(false)?;
    INNER_WORKSPACE_ACTIVE.store(true, Ordering::Release);
    emit_to_background(app, "desktop-workspace-toggle", "enter");
    Ok(())
}

/// 离开里桌面：恢复图标层并通知前端。与 [`enter_inner_workspace`] 对称，同样幂等。
#[cfg(windows)]
pub(crate) fn leave_inner_workspace(app: &tauri::AppHandle) -> Result<(), String> {
    if !INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire) {
        return Ok(());
    }
    set_desktop_icons_visible(true)?;
    INNER_WORKSPACE_ACTIVE.store(false, Ordering::Release);
    emit_to_background(app, "desktop-workspace-toggle", "leave");
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn enter_inner_workspace(_: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn leave_inner_workspace(_: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

/// Undo every desktop mutation this process may have left behind, from a process
/// that did not make them.
///
/// `restore_desktop_icons` is enough when the caller is the wallpaper itself: it
/// also clears this process's own inner-workspace flag, and its own WebView would
/// hide the icons again immediately anyway. A separate repair helper has neither
/// concern, so it must additionally repaint the desktop host — killing a process
/// that had taken over the desktop's own input can leave the host showing a stale
/// frame with no icons and an unresponsive hit test until something invalidates
/// it. Redrawing is what makes the desktop usable again without a sign-out.
///
/// Idempotent by construction: showing an already-visible window and invalidating
/// an already-valid one are both no-ops that cannot fail into a loop. That is the
/// property this whole repair path depends on.
#[cfg(windows)]
pub fn repair_desktop_after_abnormal_exit() -> Vec<String> {
    let mut notes = Vec::new();

    // 1. The icon layer the wallpaper hides while the inner workspace is active.
    match set_desktop_icons_visible(true) {
        Ok(()) => notes.push("icons-restored".into()),
        Err(error) => notes.push(format!("icons-failed:{error}")),
    }

    // 2. Force the desktop host to repaint, so a stale composited frame cannot
    //    keep hiding the icons or an outdated hit test cannot keep eating input.
    use windows::Win32::Graphics::Gdi::{
        RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW,
    };
    // SAFETY: both window handles are resolved here and only passed to RedrawWindow, whose other arguments are None and integer flags; a missing desktop handle is detected by is_invalid and never used.
    unsafe {
        let desktop = GetDesktopWindow();
        if !desktop.is_invalid() {
            let _ = RedrawWindow(
                Some(desktop),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_ERASE,
            );
            notes.push("desktop-repainted".into());
        } else {
            notes.push("desktop-handle-missing".into());
        }

        // Progman owns the desktop background and the icon view in the degraded
        // host mode this machine uses, so it needs the invalidation explicitly.
        if let Ok(program_manager) = FindWindowW(w!("Progman"), None) {
            if !program_manager.is_invalid() {
                let _ = RedrawWindow(
                    Some(program_manager),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_ERASE,
                );
                notes.push("progman-repainted".into());
            }
        }
    }

    notes
}

#[cfg(windows)]
pub(crate) fn request_wallpaper_worker() -> Result<HWND, String> {
    // SAFETY: FindWindowW receives a static class name and a null window name; the returned handle is only passed to SendMessageTimeoutW below and is never freed here.
    let progman = unsafe { FindWindowW(windows::core::w!("Progman"), PCWSTR::null()) }
        .map_err(|error| format!("无法找到 Explorer Progman 窗口：{error}"))?;

    // SAFETY: `progman` is the handle resolved above, the message payloads are plain integers, and `message_result` is local storage alive across this synchronous call; the reply is discarded on purpose.
    unsafe {
        let mut message_result = 0usize;
        let _ = SendMessageTimeoutW(
            progman,
            PROGMAN_SPAWN_WORKERW,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1_000,
            Some(&mut message_result),
        );
    }

    if let Ok(worker) = locate_wallpaper_worker() {
        // SAFETY: `worker` was just located by enumeration; IsWindowVisible only inspects it and answers FALSE for a window that is already gone.
        if unsafe { IsWindowVisible(worker).as_bool() } {
            return Ok(worker);
        }
    }

    // Windows 11 的新版 Explorer 使用 0xD 参数的两步协议。
    // SAFETY: `progman` is still the handle resolved above, WPARAM and LPARAM carry only integers, and `message_result` is a fresh local per iteration, so the synchronous send touches no borrowed data.
    unsafe {
        for lparam in [0isize, 1] {
            let mut message_result = 0usize;
            let _ = SendMessageTimeoutW(
                progman,
                PROGMAN_SPAWN_WORKERW,
                WPARAM(0xD),
                LPARAM(lparam),
                SEND_MESSAGE_TIMEOUT_FLAGS(SMTO_NORMAL.0),
                1_000,
                Some(&mut message_result),
            );
        }
    }
    if let Some(worker) = wait_for_visible_wallpaper_worker() {
        return Ok(worker);
    }

    // 部分 Windows 11 Explorer 版本确实不创建独立 WorkerW。只有标准和
    // Win11 两套协议均失败后才降级到 Progman，仍保持为桌面子窗口，
    // 不回退为普通顶层窗口。
    if !progman.0.is_null() {
        log::warn!(
            "Explorer 未枚举独立 WorkerW，暂以 Progman 作为壁纸降级宿主：0x{:X}",
            progman.0 as usize
        );
        return Ok(progman);
    }
    Err("Explorer 未提供 WorkerW 或 Progman 壁纸宿主".to_string())
}

#[cfg(windows)]
fn window_class(hwnd: HWND) -> Option<String> {
    let mut class = [0u16; 64];
    // SAFETY: the destination is a local 64-element buffer, and windows-rs passes its length to GetClassNameW so the write is bounded; the returned count is used only to slice that same buffer.
    let class_len = unsafe { GetClassNameW(hwnd, &mut class) };
    (class_len > 0).then(|| String::from_utf16_lossy(&class[..class_len as usize]))
}

#[cfg(windows)]
fn window_neighbor_class(
    hwnd: HWND,
    command: windows::Win32::UI::WindowsAndMessaging::GET_WINDOW_CMD,
) -> String {
    // SAFETY: `hwnd` is the caller's handle and `command` is a GET_WINDOW_CMD constant; GetWindow only resolves a neighbouring handle and reports failure instead of faulting.
    unsafe { GetWindow(hwnd, command).ok() }
        .filter(|neighbor| !neighbor.0.is_null())
        .and_then(window_class)
        .unwrap_or_else(|| "none".into())
}

#[cfg(windows)]
fn record_wallpaper_window_state(name: &str, hwnd: HWND) {
    // SAFETY: `hwnd` is the live handle passed by the caller; GetParent only resolves the parent handle and a null result falls back to the `none` label.
    let parent = unsafe { GetParent(hwnd).ok() }
        .and_then(window_class)
        .unwrap_or_else(|| "none".into());
    let mut client = RECT::default();
    // SAFETY: `client` is a local RECT that outlives the call and `hwnd` is the caller's handle; a failed read takes the unavailable branch instead of exposing uninitialized data.
    let size = if unsafe { GetClientRect(hwnd, &mut client).is_ok() } {
        format!(
            "{}x{}",
            client.right - client.left,
            client.bottom - client.top
        )
    } else {
        "unavailable".into()
    };
    let previous = window_neighbor_class(hwnd, GW_HWNDPREV);
    let next = window_neighbor_class(hwnd, GW_HWNDNEXT);
    // SAFETY: `hwnd` is the same caller-owned handle used by the reads above; IsWindowVisible only inspects it.
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    crate::native_bootstrap::record_startup_diagnostic(&format!(
        "event=window-state layer={} parent={} size={} visible={} prev={} next={}",
        name, parent, size, visible, previous, next
    ));
}

#[cfg(windows)]
fn resize_wallpaper_to_parent(background: HWND, worker: HWND) -> Result<(), String> {
    let mut bounds = RECT::default();
    // SAFETY: `worker` is the live desktop host handle passed in and `bounds` is local storage that outlives the call; a failure is propagated rather than reading the rectangle.
    unsafe { GetClientRect(worker, &mut bounds) }
        .map_err(|error| format!("无法读取 WorkerW 尺寸：{error}"))?;
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width <= 0 || height <= 0 {
        return Err(format!("WorkerW 客户区尺寸无效：{width}x{height}"));
    }
    // SAFETY: both handles are desktop host windows owned by the caller, every out-parameter is a local RECT or POINT alive across the calls, and a failed ClientToScreen returns before the coordinates are used.
    unsafe {
        SetWindowPos(
            background,
            None,
            0,
            0,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|error| format!("无法调整 WorkerW 子窗口尺寸：{error}"))?;

        // SetWindowPos sizes the outer HWND, while WebView2 renders inside its
        // client area. Tauri's hidden drag/resize frame can survive the style
        // conversion and shrink that client area by several DPI-scaled pixels.
        // Measure the real client origin/size and compensate the outer window
        // so the WebView client exactly covers the desktop host client area.
        let mut child_client = RECT::default();
        GetClientRect(background, &mut child_client)
            .map_err(|error| format!("无法读取壁纸客户区尺寸：{error}"))?;
        let child_width = child_client.right - child_client.left;
        let child_height = child_client.bottom - child_client.top;
        let mut parent_origin = POINT::default();
        let mut child_origin = POINT::default();
        if !ClientToScreen(worker, &mut parent_origin).as_bool()
            || !ClientToScreen(background, &mut child_origin).as_bool()
        {
            return Err("无法换算壁纸客户区坐标".into());
        }
        let inset_x = child_origin.x - parent_origin.x;
        let inset_y = child_origin.y - parent_origin.y;
        let extra_width = width - child_width;
        let extra_height = height - child_height;
        if inset_x != 0 || inset_y != 0 || extra_width != 0 || extra_height != 0 {
            SetWindowPos(
                background,
                None,
                -inset_x,
                -inset_y,
                width + extra_width,
                height + extra_height,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            )
            .map_err(|error| format!("无法补偿壁纸客户区边框：{error}"))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn place_wallpaper_layers(background: HWND, parent: HWND) -> Result<(), String> {
    let cover = crate::native_bootstrap::window_handle()
        // SAFETY: `hwnd` comes from the native cover handle slot and is only probed for its parent; a window destroyed in between yields None, which the filter drops.
        .filter(|hwnd| unsafe { GetParent(*hwnd).ok() } == Some(parent));
    let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;

    if window_class(parent).as_deref() == Some("Progman") {
        // SAFETY: `parent` is the Progman handle whose class was just checked, and the child class name is a static `w!` string; a missing child becomes the error returned below.
        let icon_view = unsafe {
            FindWindowExW(
                Some(parent),
                None,
                windows::core::w!("SHELLDLL_DefView"),
                PCWSTR::null(),
            )
        }
        .ok()
        .filter(|hwnd| !hwnd.0.is_null())
        .ok_or("Progman 中缺少 SHELLDLL_DefView")?;
        // SAFETY: every handle used here was resolved and null-checked above, and these calls pass only handles and integer flags, so no Rust-owned buffer is shared with the window manager.
        unsafe {
            if let Some(cover) = cover {
                // Child Z order is front-to-back. Keep Explorer's icon view
                // above the native hand-off cover, and the live WebView below
                // the cover. This places both application surfaces inside the
                // shell wallpaper stack rather than over icons or other apps.
                SetWindowPos(cover, Some(icon_view), 0, 0, 0, 0, flags)
                    .map_err(|error| format!("无法将原生首帧层放到桌面图标下方：{error}"))?;
                SetWindowPos(background, Some(cover), 0, 0, 0, 0, flags)
                    .map_err(|error| format!("无法将背景 WebView 放到原生首帧层下方：{error}"))?;
            } else {
                SetWindowPos(background, Some(icon_view), 0, 0, 0, 0, flags)
                    .map_err(|error| format!("无法将壁纸放到桌面图标层后方：{error}"))?;
            }
        }
        return Ok(());
    }

    if let Some(cover) = cover {
        // SAFETY: `background` and the cover are live wallpaper child windows on this path, and the insert-after argument is another window handle or HWND_TOP, never a Rust pointer.
        unsafe {
            // A WorkerW is already behind the icon host. Put the cover at the
            // front of this wallpaper child stack, then the live WebView
            // directly behind it.
            SetWindowPos(cover, Some(HWND_TOP), 0, 0, 0, 0, flags)
                .map_err(|error| format!("无法将原生首帧层置于壁纸宿主前方：{error}"))?;
            SetWindowPos(background, Some(cover), 0, 0, 0, 0, flags)
                .map_err(|error| format!("无法将背景 WebView 放到原生首帧层下方：{error}"))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn attach_hwnd_to_workerw(background: HWND) -> Result<HWND, String> {
    let worker = request_wallpaper_worker()?;
    let worker_class = window_class(worker).unwrap_or_else(|| "unknown".into());
    crate::native_bootstrap::record_startup_diagnostic(&format!(
        "event=webview-attach-start parent={}",
        worker_class
    ));
    // Move the still-visible native cover first when Explorer has upgraded
    // from Progman to WorkerW. The WebView remains in its old host until the
    // cover is ready in the new one.
    if native_bootstrap::window_handle().is_some() {
        native_bootstrap::reattach_to_workerw().map_err(|error| {
            format!("原生首帧层无法先挂载到新的桌面宿主，暂缓移动 WebView：{error}")
        })?;
    }
    // SAFETY: `background` is the WebView's own live HWND supplied by the caller and `worker` is the located desktop host; the DWM call gets a pointer to a local u32 with its exact size, and the GetParent readback after SetParent decides success.
    unsafe {
        let exstyle = GetWindowLongPtrW(background, GWL_EXSTYLE);
        SetWindowLongPtrW(
            background,
            GWL_EXSTYLE,
            (exstyle
                & !(WS_EX_CLIENTEDGE.0 as isize
                    | WS_EX_DLGMODALFRAME.0 as isize
                    | WS_EX_STATICEDGE.0 as isize
                    | WS_EX_WINDOWEDGE.0 as isize))
                | WS_EX_TOOLWINDOW.0 as isize,
        );

        let style = GetWindowLongPtrW(background, GWL_STYLE);
        SetWindowLongPtrW(
            background,
            GWL_STYLE,
            (style
                & !(WS_POPUP.0 as isize
                    | WS_BORDER.0 as isize
                    | WS_CAPTION.0 as isize
                    | WS_DLGFRAME.0 as isize
                    | WS_MAXIMIZEBOX.0 as isize
                    | WS_MINIMIZEBOX.0 as isize
                    | WS_SYSMENU.0 as isize
                    | WS_THICKFRAME.0 as isize))
                | WS_CHILD.0 as isize,
        );

        // Tauri's window starts life as a top-level Win11 window. DWM can keep
        // its rounded clipping region even after SetParent, leaving the desktop
        // visible around the corners. Explicitly opt out before sizing it.
        let corner_preference = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            background,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner_preference).cast(),
            std::mem::size_of_val(&corner_preference) as u32,
        );

        // SetParent 返回「上一个父窗口」。顶层窗口的上一个父窗口为 NULL，
        // windows-rs 会把这种成功情况包装为 Err，因此以 GetParent 的实际结果为准。
        let _ = SetParent(background, Some(worker));
        let actual_parent = GetParent(background).ok();
        if actual_parent != Some(worker) {
            return Err(format!(
                "无法将背景窗口附着到 WorkerW：期望 0x{:X}，实际 0x{:X}",
                worker.0 as usize,
                actual_parent.map(|parent| parent.0 as usize).unwrap_or(0)
            ));
        }
        SetWindowPos(
            background,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|error| format!("无法刷新壁纸子窗口样式：{error}"))?;
    }
    resize_wallpaper_to_parent(background, worker)?;
    place_wallpaper_layers(background, worker)?;
    // SAFETY: `background` was attached and restyled just above and is still alive; ShowWindow only requests a show state and its result is deliberately ignored.
    unsafe {
        let _ = ShowWindow(background, SW_SHOWNA);
    }
    record_wallpaper_window_state("background", background);
    if let Some(cover) = native_bootstrap::window_handle() {
        record_wallpaper_window_state("native-cover", cover);
    }
    crate::native_bootstrap::record_startup_diagnostic("event=webview-attached-and-visible");
    // WebView 第一次真正可见的时刻就在这里（上面那次 `ShowWindow(SW_SHOWNA)`）。
    crate::startup_timeline::mark("webview-visible", "背景 WebView 已显示（首帧可见）");
    log::info!(
        "background 0x{:X} attached to WorkerW 0x{:X}",
        background.0 as usize,
        worker.0 as usize
    );
    Ok(worker)
}

#[cfg(windows)]
pub fn attach_to_workerw(window: &WebviewWindow) -> Result<(), String> {
    window
        .set_ignore_cursor_events(false)
        .map_err(|error| format!("无法启用桌面交互：{error}"))?;
    let background = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    attach_hwnd_to_workerw(background)?;
    // The same WebView now paints both the background and conversation. Keep
    // the entire scene visible, but only let declared chat regions receive
    // pointer input; all other desktop input falls through to Explorer.
    install_desktop_hit_testing(background)
}

#[cfg(windows)]
fn inspect_wallpaper_host(window: Option<&WebviewWindow>) -> (WallpaperHostAction, Option<HWND>) {
    let Some(window) = window else {
        return (WallpaperHostAction::Recreate, None);
    };
    let Ok(raw) = window.hwnd() else {
        return (WallpaperHostAction::Recreate, None);
    };
    let background = HWND(raw.0);
    // SAFETY: `background` was just resolved from the Tauri window handle; IsWindow only inspects it and a destroyed window answers false, which is the Recreate branch this guard handles.
    if !unsafe { IsWindow(Some(background)).as_bool() } {
        return (WallpaperHostAction::Recreate, Some(background));
    }
    // SAFETY: `background` passed the IsWindow check on this path, so it names a live window; GetParent only resolves the parent handle.
    let parent = unsafe { GetParent(background) }.ok();
    let parent_exists = parent
        // SAFETY: `hwnd` is the parent handle just returned by GetParent and is only inspected; a stale handle answers false and folds into the unwrap_or default.
        .map(|hwnd| unsafe { IsWindow(Some(hwnd)).as_bool() })
        .unwrap_or(false);
    let parent_class = parent.and_then(window_class);
    let parent_is_worker = is_valid_wallpaper_parent_class(parent_class.as_deref())
        && parent
            // SAFETY: `hwnd` is the same parent handle probed above and IsWindowVisible only queries its state; false is the ordinary answer for a hidden window.
            .map(|hwnd| unsafe { IsWindowVisible(hwnd).as_bool() })
            .unwrap_or(false);
    let worker_available = parent_class.as_deref() == Some("Progman")
        && locate_wallpaper_worker()
            // SAFETY: `worker` comes from the enumeration performed in this expression and is only queried for visibility.
            .map(|worker| unsafe { IsWindowVisible(worker).as_bool() })
            .unwrap_or(false);

    let size_matches = parent
        .filter(|_| parent_exists && parent_is_worker)
        .and_then(|worker| {
            let mut parent_rect = RECT::default();
            let mut child_rect = RECT::default();
            // SAFETY: `worker` is the parent that passed the existence and class checks above and `background` is the live host; both RECTs are locals that outlive the two synchronous calls.
            unsafe {
                GetClientRect(worker, &mut parent_rect).ok()?;
                GetClientRect(background, &mut child_rect).ok()?;
            }
            Some(
                parent_rect.right - parent_rect.left == child_rect.right - child_rect.left
                    && parent_rect.bottom - parent_rect.top == child_rect.bottom - child_rect.top,
            )
        })
        .unwrap_or(false);

    (
        if should_upgrade_wallpaper_parent(parent_class.as_deref(), worker_available) {
            WallpaperHostAction::Reattach
        } else {
            decide_wallpaper_host_action(true, parent_exists, parent_is_worker, size_matches)
        },
        parent,
    )
}

#[cfg(windows)]
fn create_background_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    let window = WebviewWindowBuilder::new(
        app,
        "background",
        WebviewUrl::App("index.html?surface=combined".into()),
    )
    .title("DSH Wallpaper")
    .inner_size(1280.0, 720.0)
    .decorations(false)
    .resizable(false)
    .focusable(true)
    .skip_taskbar(true)
    .visible(false)
    .build()
    .map_err(|error| format!("无法重建背景 WebView：{error}"))?;
    crate::native_bootstrap::record_startup_diagnostic(
        "event=background-webview-created visible=false",
    );
    // 句柄槽位必须在这里跟上：三条监视线程读的就是它，写晚一步（或漏写）就等于把刚修好的
    // "重建后功能失效"又放回去。句柄在主线程上取，且这是创建点，不是后台线程的重新解析。
    // 只传 `None` 给球那半格会把它抹成 0，所以这里只写 `background` 那一格：
    // `publish_window_channels` 的两格是"此刻真实存在"的语义，球由 `ensure_ball` 自己写。
    if let Ok(handle) = window.hwnd() {
        publish_background_window_handle(Some(HWND(handle.0)));
    }
    Ok(window)
}

#[cfg(windows)]
fn recover_wallpaper_host(app: &tauri::AppHandle) -> Result<(), String> {
    let previous_handoff_generation = native_bootstrap::generation();
    let existing = app.get_webview_window("background");
    let (action, parent) = inspect_wallpaper_host(existing.as_ref());
    if action != WallpaperHostAction::None {
        publish_wallpaper_host_status(app, WallpaperHostMode::Recovering, false, None);
    }
    let result = match action {
        WallpaperHostAction::None => Ok(()),
        WallpaperHostAction::Resize => {
            let window = existing.ok_or("background window missing")?;
            let background = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
            resize_wallpaper_to_parent(background, parent.ok_or("WorkerW parent missing")?)
        }
        WallpaperHostAction::Reattach => {
            let window = existing.ok_or("background window missing")?;
            attach_to_workerw(&window)
        }
        WallpaperHostAction::Recreate => {
            if let Some(window) = existing {
                let _ = window.destroy();
            }
            let window = create_background_window(app)?;
            attach_to_workerw(&window)?;
            register_session_events(app)?;
            Ok(())
        }
    };
    if result.is_ok() && action != WallpaperHostAction::None {
        let _ = native_bootstrap::invalidate_pending_handoff();
    }
    let current_handoff_generation = native_bootstrap::generation();
    if current_handoff_generation != previous_handoff_generation {
        native_bootstrap::record_startup_diagnostic(&format!(
            "event=handoff-generation-published generation={current_handoff_generation} reason=wallpaper-host-recovered"
        ));
        emit_to_background(app, "native-handoff-generation", current_handoff_generation);
    }
    match &result {
        Ok(()) => {
            if let Some(window) = app.get_webview_window("background") {
                let mode = window
                    .hwnd()
                    .ok()
                    // SAFETY: the handle was just obtained from the recovered window and GetParent only resolves its parent; a missing parent is tolerated by the class lookup below.
                    .and_then(|raw| unsafe { GetParent(HWND(raw.0)) }.ok())
                    .and_then(window_class)
                    .map(|class| {
                        if class == "WorkerW" {
                            WallpaperHostMode::WorkerW
                        } else {
                            WallpaperHostMode::ProgmanFallback
                        }
                    })
                    .unwrap_or(WallpaperHostMode::Unavailable);
                publish_wallpaper_host_status(app, mode, action != WallpaperHostAction::None, None);
            }
        }
        Err(error) => publish_wallpaper_host_status(
            app,
            WallpaperHostMode::Unavailable,
            false,
            Some(error.clone()),
        ),
    }
    result
}

#[cfg(windows)]
pub fn start_wallpaper_host(app: tauri::AppHandle) -> Result<(), String> {
    crate::native_bootstrap::record_startup_diagnostic(&format!(
        "event=wallpaper-host-start existing_background={}",
        app.get_webview_window("background").is_some()
    ));
    // A force-quit during the inner workspace must never leave Explorer's
    // desktop layer hidden on the next startup.
    restore_desktop_icons();
    // Arm the crash net for *this* run: drop any marker an earlier clean exit
    // left, then spawn the helper that outlives this process and repairs the
    // desktop if it dies badly. Order matters — clearing after spawning could let
    // the helper observe a stale marker and wrongly conclude this run exited
    // cleanly.
    crate::desktop_repair::clear_clean_exit_marker();
    crate::desktop_repair::spawn_repair_helper();
    recover_wallpaper_host(&app)?;
    crate::native_bootstrap::record_startup_diagnostic("event=wallpaper-host-attached");
    if let Err(error) = native_bootstrap::reattach_to_workerw() {
        log::warn!("native bootstrap initial reattach failed: {error}");
    }
    std::thread::Builder::new()
        .name("wallpaper-host-recovery".into())
        .spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if WALLPAPER_RECOVERY_QUEUED.swap(true, Ordering::AcqRel) {
            continue;
        }
        let recovery_app = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
          guard::run_main_thread_window_state_task(|| {
            let previous_handoff_generation = native_bootstrap::generation();
            let (action, _) =
                inspect_wallpaper_host(recovery_app.get_webview_window("background").as_ref());
            if action != WallpaperHostAction::None {
                if let Err(error) = recover_wallpaper_host(&recovery_app) {
                    log::error!("wallpaper host recovery failed: {error}");
                }
            }
            match native_bootstrap::reattach_to_workerw() {
                Ok(true) if action == WallpaperHostAction::None => {
                    let _ = native_bootstrap::invalidate_pending_handoff();
                }
                Ok(_) => {}
                Err(error) => log::warn!("native bootstrap reattach failed: {error}"),
            }
            let current_handoff_generation = native_bootstrap::generation();
            if current_handoff_generation != previous_handoff_generation {
                native_bootstrap::record_startup_diagnostic(&format!(
                    "event=handoff-generation-published generation={current_handoff_generation} reason=wallpaper-host-recovery-loop"
                ));
                emit_to_background(
                    &recovery_app,
                    "native-handoff-generation",
                    current_handoff_generation,
                );
            }
            WALLPAPER_RECOVERY_QUEUED.store(false, Ordering::Release);
          });
        }) {
            WALLPAPER_RECOVERY_QUEUED.store(false, Ordering::Release);
            log::error!("unable to schedule wallpaper host recovery: {error}");
        }
    })
        .expect("无法启动壁纸宿主自愈线程");
    Ok(())
}

#[cfg(windows)]
pub fn register_session_events(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("background")
        .ok_or("background window missing")?;
    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
    let app_ptr = Box::into_raw(Box::new(app.clone())) as usize;
    // SAFETY: `hwnd` is the live background window and the subclass proc is a static function; `app_ptr` is the only pointer to a heap AppHandle that either the failure branch inside this block or WM_NCDESTROY later reclaims exactly once.
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).map_err(|e| e.to_string())?;
        if !SetWindowSubclass(hwnd, Some(session_subclass_proc), 1, app_ptr).as_bool() {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            drop(Box::from_raw(app_ptr as *mut tauri::AppHandle));
            return Err("无法安装 Windows 会话消息处理器".into());
        }
    }
    emit_to_background(app, "system-session", "resume");
    Ok(())
}

/// Release the session notification and window subclass installed by
/// `register_session_events`. Idempotent: it is safe to call on output that was
/// never registered, and the `WM_NCDESTROY` cleanup above is harmless when this
/// ran first.
#[cfg(windows)]
pub fn unregister_session_events(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("background") else {
        return;
    };
    let Ok(raw) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(raw.0);
    // SAFETY: `hwnd` was just resolved from the live background window; both calls tolerate a window that was never registered and neither dereferences Rust memory.
    unsafe {
        let _ = WTSUnRegisterSessionNotification(hwnd);
        let _ = RemoveWindowSubclass(hwnd, Some(session_subclass_proc), 1);
    }
}

#[cfg(not(windows))]
pub fn unregister_session_events(_: &tauri::AppHandle) {}

#[cfg(windows)]
unsafe extern "system" fn session_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    app_ptr: usize,
) -> LRESULT {
    let app = &*(app_ptr as *const tauri::AppHandle);
    match message {
        WM_WTSSESSION_CHANGE => match wparam.0 as u32 {
            WTS_SESSION_LOCK => {
                let _ = native_bootstrap::show_sleep();
                emit_to_background(
                    app,
                    "native-handoff-generation",
                    native_bootstrap::generation(),
                );
                dispatch_system_action(app, AppAction::Lock);
                emit_system_session(app, "locked");
            }
            WTS_SESSION_UNLOCK => {
                let _ = native_bootstrap::start_wake();
                emit_to_background(
                    app,
                    "native-handoff-generation",
                    native_bootstrap::generation(),
                );
                dispatch_system_action(app, AppAction::Unlock { play_wake: true });
                emit_system_session(app, "unlocked");
            }
            _ => {}
        },
        WM_POWERBROADCAST => match wparam.0 as u32 {
            PBT_APMSUSPEND => {
                let _ = native_bootstrap::show_sleep();
                emit_to_background(
                    app,
                    "native-handoff-generation",
                    native_bootstrap::generation(),
                );
                dispatch_system_action(app, AppAction::Lock);
                emit_system_session(app, "suspend");
            }
            PBT_APMRESUMEAUTOMATIC => {
                let _ = native_bootstrap::start_wake();
                emit_to_background(
                    app,
                    "native-handoff-generation",
                    native_bootstrap::generation(),
                );
                dispatch_system_action(app, AppAction::Unlock { play_wake: true });
                emit_system_session(app, "resume");
            }
            _ => {}
        },
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            // Display topology and DPI changes can arrive before Explorer has
            // finished updating every monitor rectangle. The frontend
            // debounces this notification and re-queries the authoritative
            // snapshot instead of trusting message payload coordinates.
            emit_to_background(app, "display-changed", ());
        }
        WM_NCDESTROY => {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(session_subclass_proc), subclass_id);
            drop(Box::from_raw(app_ptr as *mut tauri::AppHandle));
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

#[cfg(windows)]
/// Phase A diagnostic for the input-island focus handoff.
///
/// The open question is which native message actually arrives on a real left-click in the
/// input hot zone, and where Windows put the foreground and its keyboard focus at that
/// moment. A thread-local `SetFocus` returning success did not mean the keyboard channel
/// was handed over, so the readbacks below are the only trustworthy evidence.
///
/// Records window classes, PIDs, handles, the hot-zone outcome and those readbacks. Never
/// records key content, window titles, input text or session content, and it is compiled
/// out of release builds. Called only from mouse-activation and focus messages, so the
/// per-mouse-move `WM_NCHITTEST` path stays untouched.
// Pure arithmetic on the message parameter; needed by the call sites in every profile.
#[cfg(windows)]
/// Record that the hit-test branch really runs while the left button is down.
///
/// Phase A of the repair plan requires the trigger to be proven on a real click rather than
/// assumed. `WM_MOUSEACTIVATE` and `WM_LBUTTONDOWN` were measured to never arrive; whether
/// `WM_NCHITTEST` arrives is the remaining question, and the whole island-click handover
/// hangs on it. Rate limited, because hit-testing repeats while a button is held.
#[cfg(all(windows, debug_assertions))]
fn trace_island_click_reached(local_x: i32, local_y: i32) {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.is_some_and(|previous| previous.elapsed() < std::time::Duration::from_millis(500)) {
        return;
    }
    *last = Some(std::time::Instant::now());
    log::info!("island click reached WM_NCHITTEST with the left button down at ({local_x}, {local_y})");
}

/// Release builds keep the call site and compile the probe away.
#[cfg(all(windows, not(debug_assertions)))]
fn trace_island_click_reached(_local_x: i32, _local_y: i32) {}
/// Verify that a caller really is the user clicking inside the input island.
///
/// The native click route is closed: a real click never delivers `WM_MOUSEACTIVATE`,
/// `WM_LBUTTONDOWN` or even a hit test to our subclass, because the WebView2 child owns the
/// mouse messages from another process (see `docs/evidence/input-island-focus-native-route-closed.md`).
/// So the island `pointerdown` has to come from the renderer - which means the native side
/// must not trust it. Plan 3.A is explicit: the command has to check the physical left button,
/// the cursor being inside a declared region, and the window under the cursor belonging to the
/// wallpaper, and must never treat an arbitrary IPC call as a user click.
///
/// Returns a human-readable verdict for the log and for the caller. It performs no activation:
/// this is the diagnostic step the plan asks for before any handover is wired to it.
/// How far up the window chain to look before giving up.
const MAX_WINDOW_CHAIN_DEPTH: usize = 8;

/// Whether a window's own process, or any ancestor's, is this process.
///
/// The wallpaper paints through a WebView2 renderer owned by a browser helper process, so the
/// window under the cursor inside the island is a `Chrome_RenderWidgetHostHWND` and never ours.
/// Plan 3.A requires the parent chain to belong to the wallpaper, which is the check that
/// actually tells our surface apart from an unrelated window sitting on top of it.
fn window_chain_reaches_this_process(hwnd: HWND) -> bool {
    let mut current = hwnd;
    for _ in 0..MAX_WINDOW_CHAIN_DEPTH {
        if current.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        // SAFETY: `current` is a handle from the caller or from GetParent above, and the pid target is a local u32; the call writes only that out-parameter.
        unsafe { GetWindowThreadProcessId(current, Some(&mut pid)) };
        if pid == std::process::id() {
            return true;
        }
        if pid == 0 {
            return false;
        }
        // `GetParent` is a `Result` in windows 0.61, and a window with no parent is
        // the normal termination of this walk rather than an error.
        // SAFETY: `current` came from the same chain walk, and a window that vanished returns an error, which ends the walk instead of being dereferenced.
        let parent = match unsafe { GetParent(current) } {
            Ok(parent) => parent,
            Err(_) => return false,
        };
        if parent.0.is_null() {
            return false;
        }
        current = parent;
    }
    false
}

/// Carry out the foreground handover for a click that has already been verified.
///
/// Runs on the main thread because `SetFocus` fails for a window owned by another thread, and
/// the wallpaper host belongs to the main thread.
///
/// Two steps, in this order, and the order is the whole point. Our host is a `WS_CHILD` of the
/// desktop host window, and a child cannot be the foreground window - that is why every earlier
/// `SetFocus` "succeeded" while the keyboard stayed with Progman. What the working double-click
/// does is make the *top-level* desktop window foreground, after which the keyboard descends to
/// its children. So the top-level ancestor goes foreground first, and only then does the
/// keyboard focus follow into the WebView.
///
/// A genuine user click is what makes this permissible: Windows lifts the foreground lock for
/// the process that received the input, which is why this is bound to a verified click and
/// never called on its own.
///
/// Returns a description of the readback. Success is never claimed from a return code.
/// Run the handover for the wallpaper's own host window, on the main thread.
///
/// The host window belongs to the main thread, so `SetFocus` on it from any other thread
/// fails; this resolves the window here and dispatches the work where it can succeed. Failures
/// are logged rather than returned, since the caller is a click report and the handover itself
/// already records the outcome.
#[cfg(windows)]
pub fn hand_over_keyboard_for_app(app: &tauri::AppHandle) {
    let handle = app.clone();
    let dispatched = app.run_on_main_thread(move || {
      guard::run_main_thread_window_state_task(|| {
        let Some(window) = handle.get_webview_window("background") else {
            log::warn!("输入岛交接：找不到 background 窗口");
            return;
        };
        let Ok(hwnd) = window.hwnd() else {
            log::warn!("输入岛交接：background 窗口没有句柄");
            return;
        };
        log::info!("{}", hand_over_keyboard_after_verified_click(HWND(hwnd.0)));
      });
    });
    if dispatched.is_err() {
        log::warn!("输入岛交接：无法到达主线程");
    }
}

#[cfg(not(windows))]
pub fn hand_over_keyboard_for_app(_app: &tauri::AppHandle) {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusSetOutcome {
    Success,
    Failure(u32),
}

fn classify_focus_set_result(returned_null: bool, last_error: u32) -> FocusSetOutcome {
    if !returned_null || last_error == ERROR_SUCCESS.0 {
        FocusSetOutcome::Success
    } else {
        FocusSetOutcome::Failure(last_error)
    }
}

#[cfg(windows)]
unsafe fn set_focus_checked(hwnd: HWND) -> FocusSetOutcome {
    SetLastError(ERROR_SUCCESS);
    let result = SetFocus(Some(hwnd));
    classify_focus_set_result(result.is_err(), GetLastError().0)
}

#[cfg(windows)]
pub fn hand_over_keyboard_after_verified_click(host: HWND) -> String {
    // SAFETY: `host` is the wallpaper's own background HWND, checked by the caller; every call here takes handles or local out-parameters (u32, GUITHREADINFO with cbSize set) and no ownership is transferred.
    unsafe {
        let top = GetAncestor(host, GA_ROOT);
        if top.0.is_null() {
            let host_mode = wallpaper_host_status_state()
                .read()
                .map(|status| status.mode)
                .unwrap_or(WallpaperHostMode::Unavailable);
            return format!(
                "未找到顶层祖先窗口；未执行交接 host_mode={host_mode:?} host_hwnd=0x{:X}",
                host.0 as usize,
            );
        }
        let host_mode = wallpaper_host_status_state()
            .read()
            .map(|status| status.mode)
            .unwrap_or(WallpaperHostMode::Unavailable);
        let foreground_before = GetForegroundWindow();
        let mut foreground_thread = 0u32;
        GetWindowThreadProcessId(foreground_before, Some(&mut foreground_thread));
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let foreground_ok = SetForegroundWindow(top).as_bool();
        let focus_result = set_focus_checked(host);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
        let foreground_now = GetForegroundWindow();
        let mut foreground_now_pid = 0u32;
        GetWindowThreadProcessId(foreground_now, Some(&mut foreground_now_pid));
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        let focus_pid = if GetGUIThreadInfo(0, &mut info).is_ok() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid));
            Some(pid)
        } else {
            None
        };
        let focus_class = if focus_pid.is_some() {
            window_class(info.hwndFocus).unwrap_or_else(|| "<none>".to_string())
        } else {
            "<no-gui-thread-info>".to_string()
        };
        let foreground_class = window_class(foreground_now).unwrap_or_else(|| "<none>".to_string());
        // The channel is restored only when the readback agrees: the desktop top-level holds the
        // foreground and the keyboard focus has followed into the WebView. Anything else is
        // reported as a failure with the whole picture, never as a success.
        let webview_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
        let root_class = window_class(top).unwrap_or_else(|| "<none>".to_string());
        format!(
            "交接结果：host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={} AttachThreadInput={attached} SetForegroundWindow={foreground_ok} SetFocus={focus_result:?} webview_has_focus={webview_has_focus} | 读回 前台={foreground_class}(pid={foreground_now_pid}) 焦点={focus_class}(pid={focus_pid:?}) 壁纸pid={}",
            host.0 as usize,
            top.0 as usize,
            foreground_now.0 == top.0,
            std::process::id(),
        )
    }
}

#[cfg(not(windows))]
pub fn hand_over_keyboard_after_verified_click(_host: usize) -> String {
    "交接仅在 Windows 可用".to_string()
}

#[cfg(windows)]
#[cfg(windows)]
pub fn verify_island_click() -> Result<String, String> {
    // SAFETY: every call in this block is a read-only Win32 query; the cursor, pid and thread-info storage are locals and window handles are only walked and inspected.
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON.0 as i32) >= 0 {
            return Err("拒绝原因=physical-left-button-up：未检测到物理左键按下；拒绝把 IPC 调用当作输入岛点击".into());
        }
        let mut cursor = POINT::default();
        if let Err(error) = GetCursorPos(&mut cursor) {
            return Err(format!("拒绝原因=cursor-position-unavailable：无法读取光标位置 ({error})"));
        }
        let under = WindowFromPoint(cursor);
        let mut under_pid = 0u32;
        GetWindowThreadProcessId(under, Some(&mut under_pid));
        let under_class = window_class(under).unwrap_or_else(|| "<none>".to_string());
        // Walk the parent chain rather than testing the window itself. The wallpaper's own
        // content is drawn by a WebView2 renderer in a helper process, so the window directly
        // under the cursor inside the island is a `Chrome_RenderWidgetHostHWND` that never
        // belongs to us. Testing only the immediate owner rejected every genuine island click -
        // the log showed exactly that, repeatedly. Plan 3.A says the HWND parent chain must
        // belong to the wallpaper, and this is that check.
        if !window_chain_reaches_this_process(under) {
            return Err(format!(
                "拒绝原因=foreign-window-chain：光标下的窗口父链不属于壁纸（class={under_class} pid={under_pid}）；拒绝激活"
            ));
        }
        // Deliberately no geometry check here. The declared regions are in wallpaper-local
        // coordinates, and converting them needs the host window handle that this call site
        // does not have; inventing a coordinate system would be worse than not checking. The
        // renderer owns that judgement, being the side that knows where the island is. What is
        // checked here is what an arbitrary IPC caller cannot fake: a physically held left
        // button, and the window under the cursor being ours.
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        Ok(format!(
            "输入岛点击已核验：光标=({}, {})，光标下 class={under_class} pid={under_pid}，前台 pid={foreground_pid}，壁纸 pid={}",
            cursor.x,
            cursor.y,
            std::process::id(),
        ))
    }
}

#[cfg(not(windows))]
pub fn verify_island_click() -> Result<String, String> {
    Err("输入岛点击核验仅在 Windows 可用".into())
}


fn client_point(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam.0 as u32;
    ((packed as u16 as i16) as i32, ((packed >> 16) as u16 as i16) as i32)
}

/// Phase A diagnostic for the input-island focus handoff.
///
/// Two questions have to be separated, and only a real click answers either: did the click
/// land on the WebView at all, and did Windows hand over the keyboard channel? A failing
/// fix was believed to work because `SetFocus` returned success, so every claim below is a
/// readback rather than a return code.
///
/// Records window classes, PIDs, the click resolution and those readbacks. Never records
/// key content, window titles, input text or session content, and it is compiled out of
/// release builds. Attached only to mouse-activation and focus messages, so the
/// per-mouse-move `WM_NCHITTEST` path stays untouched.
#[cfg(all(windows, debug_assertions))]
fn trace_focus_handoff(stage: &str, client: Option<(i32, i32)>) {
    // SAFETY: debug-only diagnostics; POINT and GUITHREADINFO are locals with cbSize set before use, and the handles involved are only inspected.
    unsafe {
        // Resolve the click point to the window actually under it, which tells a missed
        // hot zone apart from a click that arrived without the keyboard following.
        let point_hit = client.map(|(x, y)| {
            let mut point = POINT { x, y };
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd_of_foreground(), &mut point);
            let under = WindowFromPoint(point);
            let mut pid = 0u32;
            GetWindowThreadProcessId(under, Some(&mut pid));
            (window_class(under).unwrap_or_else(|| "<none>".to_string()), pid)
        });
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        let foreground_class = window_class(foreground).unwrap_or_else(|| "<none>".to_string());
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        let (focus_class, focus_pid) = if GetGUIThreadInfo(0, &mut info).is_ok() {
            let class = window_class(info.hwndFocus).unwrap_or_else(|| "<none>".to_string());
            let mut pid = 0u32;
            GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid));
            (class, pid)
        } else {
            ("<no-gui-thread-info>".to_string(), 0)
        };
        log::info!(
            "focus trace [{stage}]: point_hit={point_hit:?} foreground={foreground_class} foreground_pid={foreground_pid} focus={focus_class} focus_pid={focus_pid} self_pid={}",
            std::process::id(),
        );
    }
}

#[cfg(all(windows, debug_assertions))]
fn hwnd_of_foreground() -> HWND {
    // SAFETY: GetForegroundWindow takes no arguments and returns a borrowed system handle that the caller only reads.
    unsafe { GetForegroundWindow() }
}

/// Release builds keep the call sites but compile the probe away, so the instrumentation
/// never runs in a shipped build.
#[cfg(all(windows, not(debug_assertions)))]
fn trace_focus_handoff(_stage: &str, _client: Option<(i32, i32)>) {}


/// Release builds keep the call sites but compile the probe away, so the
/// instrumentation never runs in a shipped build.

/// Hand the keyboard to the WebView after a real click inside the input island.
///
/// Phase B of the repair plan. The phase A measurement (see
/// `docs/evidence/input-island-focus-phase-a-evidence.md`) described the failure precisely: keyboard
/// focus is already on the WebView while the global foreground is Progman, so keystrokes are
/// consumed by the foreground window. `WM_MOUSEACTIVATE` never arrives - the WebView2 child
/// takes the mouse messages in another process - so this runs from `WM_NCHITTEST`, which does
/// reach us, on the input's own window thread and during the click itself.
///
/// `AttachThreadInput` is the substance of the call, not a decoration: a thread whose window
/// is not foreground cannot give its window keyboard focus, and the desktop host window never
/// is foreground. Attaching to the foreground thread's input queue lifts that restriction for
/// the duration of the call, which is what the API exists for.
///
/// Success is decided by readback and never by a return code. A `SetFocus` that returns
/// success while the foreground stays elsewhere is exactly the false success that hid this
/// fault for several rounds.
#[cfg(windows)]
fn hand_over_keyboard_after_island_click(root: HWND) {
    // `WM_NCHITTEST` repeats while the button is held, so one click must not run this twice.
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let Ok(mut last) = LAST.lock() else { return };
        if last.is_some_and(|previous| previous.elapsed() < std::time::Duration::from_millis(500))
        {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    // SAFETY: `root` is the host handle this subclass was installed on; all state is local (pids, GUITHREADINFO with cbSize) and AttachThreadInput and GetAncestor take plain handles.
    unsafe {
        let host_mode = wallpaper_host_status_state()
            .read()
            .map(|status| status.mode)
            .unwrap_or(WallpaperHostMode::Unavailable);
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        let foreground_thread = GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let focus_result = set_focus_checked(root);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
        // Readback, not return codes.
        let foreground_now = GetForegroundWindow();
        let mut foreground_now_pid = 0u32;
        GetWindowThreadProcessId(foreground_now, Some(&mut foreground_now_pid));
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        let focus_pid = if GetGUIThreadInfo(0, &mut info).is_ok() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid));
            Some(pid)
        } else {
            None
        };
        let ours = foreground_now_pid == std::process::id();
        let webview_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
        let desktop_root = GetAncestor(root, GA_ROOT);
        let root_class = window_class(desktop_root).unwrap_or_else(|| "<none>".to_string());
        let foreground_class = window_class(foreground_now).unwrap_or_else(|| "<none>".to_string());
        let foreground_matches_root = foreground_now.0 == desktop_root.0;
        if ours && webview_has_focus {
            log::info!(
                "island click: keyboard channel handed over to the WebView host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={foreground_matches_root} AttachThreadInput={attached} SetFocus={focus_result:?} webview_has_focus={webview_has_focus}",
                root.0 as usize,
                desktop_root.0 as usize,
            );
        } else {
            log::warn!(
                "island click: handover did not take - host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={foreground_matches_root} AttachThreadInput={attached} SetFocus={focus_result:?} foreground_is_wallpaper={ours} webview_has_focus={webview_has_focus} foreground_pid={foreground_now_pid}",
                root.0 as usize,
                desktop_root.0 as usize,
            );
        }
    }
}



#[cfg(windows)]
pub fn start_foreground_monitor(app: tauri::AppHandle) {
    // 与前两族监视同形：轮询里**只出现纯值**，不回 Tauri 取运行期窗口状态
    // （`get_webview_window` 会 `Webview::clone`，见本文件顶部那段说明）。
    // 与旧写法的唯一区别是句柄不再在启动时冻结：每拍从句柄槽位读一次，`background`
    // 被销毁并重建之后自动跟上新句柄（见 `BACKGROUND_WINDOW_HANDLE`）。
    // `HWND` 内部是裸指针、不是 `Send`，所以跨线程搬的是它的**整数**形式。
    std::thread::Builder::new()
        .name("foreground-monitor".into())
        .spawn(move || {
        assert_not_acting_as_main_thread();
        loop {
        std::thread::sleep(std::time::Duration::from_millis(120));
        // 窗口重建期间（槽位是 0，或槽位里的句柄刚被销毁）不做事，但继续等：这条线程的
        // 主要职责本来是"桌面成为前台"，退场才是重建之后永久失效的原因。
        let background_raw = background_window_handle_now();
        let window_present = background_handle_is_usable(background_raw);
        if window_present {
        // SAFETY: no arguments and no Rust memory; the returned handle is only compared and class-probed afterwards.
        let foreground = unsafe { GetForegroundWindow() };
        let foreground_class = window_class(foreground);
        // SAFETY: GetDesktopWindow takes no arguments and returns a borrowed system handle that is only compared with the foreground handle.
        let shell = foreground == unsafe { GetDesktopWindow() }
            || is_desktop_foreground_class(foreground_class.as_deref());
        let desktop_foreground = shell;
        if let Some(core) = app.try_state::<AppCore>() {
            let changed = desktop_foreground_state()
                .write()
                .map(|mut previous| {
                    if *previous == Some(desktop_foreground) {
                        false
                    } else {
                        *previous = Some(desktop_foreground);
                        true
                    }
                })
                .unwrap_or(true);
            if changed {
                // Returning to the desktop leaves the WebView without a keyboard channel, and
                // nothing else can give it back: the window is non-activating, so a click cannot,
                // and a DOM focus request is refused. Do it on the one transition that matters.
                if desktop_foreground {
                    restore_desktop_keyboard_focus(&app);
                }
                let snapshot =
                    core.dispatch(AppAction::DesktopForegroundChanged(desktop_foreground));
                // Foreground changes are informative only.  Re-showing or
                // hiding the WebView here makes the composition flicker and,
                // more importantly, turns an ordinary focus change into an
                // implicit visibility command.  Explicit tray/settings
                // commands and privacy transitions own that responsibility.
                emit_to_background(&app, "app-snapshot", &snapshot);
            }
        }
        }
        }
        })
        .expect("无法启动前台监控线程");
}

/// Hand the keyboard back to the wallpaper's WebView.
///
/// A desktop wallpaper must not activate itself, so when focus goes to another window
/// the WebView loses its keyboard channel and neither a click nor a DOM `focus()` can
/// take it back — a click cannot activate a non-activating window, and a DOM focus
/// request is refused for the same reason. Rebuilding the input (what the double-click
/// desktop switch does) only rebuilds the DOM; it does not restore the channel either.
///
/// The desktop switch works because changing the desktop's own window state makes
/// Windows renegotiate keyboard focus for the desktop's children. This does that
/// directly, and only from the wallpaper's main thread: `SetFocus` fails for a window
/// owned by another thread, and the background window belongs to the main thread.
///
/// `SetForegroundWindow` is deliberately not called. Giving the keyboard back is not the
/// same as stealing the foreground, and stealing it is exactly what the non-activating
/// design exists to prevent.
#[cfg(windows)]
fn restore_desktop_keyboard_focus(app: &tauri::AppHandle) {
    let handle = app.clone();
    let dispatched = app.run_on_main_thread(move || {
      guard::run_main_thread_window_state_task(|| {
        let Some(window) = handle.get_webview_window("background") else {
            return;
        };
        let Ok(hwnd) = window.hwnd() else {
            log::warn!("desktop focus restore: background window has no handle");
            return;
        };
        let host = HWND(hwnd.0);
        let host_mode = wallpaper_host_status_state()
            .read()
            .map(|status| status.mode)
            .unwrap_or(WallpaperHostMode::Unavailable);
        // SAFETY: `host` is the live background window resolved on the main thread, and set_focus_checked only calls SetFocus and GetLastError on it.
        let focus_result = unsafe { set_focus_checked(host) };
        // SetFocus's return is only one diagnostic. Always read back the actual
        // foreground and keyboard focus, even when SetFocus reports a real error.
        // SAFETY: `host` is the same live handle; GetAncestor only resolves the root handle and returns null on failure, which the class probe then tolerates.
        let desktop_root = unsafe { GetAncestor(host, GA_ROOT) };
        let root_class = window_class(desktop_root).unwrap_or_else(|| "<none>".to_string());
        // SAFETY: GetForegroundWindow takes no arguments and its handle is only read.
        let foreground = unsafe { GetForegroundWindow() };
        let foreground_class = window_class(foreground).unwrap_or_else(|| "<none>".to_string());
        let foreground_matches_root = foreground.0 == desktop_root.0;
        let mut foreground_pid = 0u32;
        // SAFETY: `foreground` is a handle just obtained and `foreground_pid` is a local u32; the call writes only that out-parameter.
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_pid)) };
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        // SAFETY: `info` is a local GUITHREADINFO whose cbSize is set to its own size immediately above, which is the contract the API requires before writing.
        let focus_pid = if unsafe { GetGUIThreadInfo(0, &mut info) }.is_ok() {
            let mut pid = 0u32;
            // SAFETY: `info.hwndFocus` was read from the GUITHREADINFO that the successful call above filled, and the pid target is a local u32.
            unsafe { GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid)) };
            Some(pid)
        } else {
            None
        };
        let webview_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
        let foreground_is_wallpaper = foreground_pid == std::process::id();
        let diagnostic = format!(
            "host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={foreground_matches_root} AttachThreadInput=false SetFocus={focus_result:?} foreground_is_wallpaper={foreground_is_wallpaper} webview_has_focus={webview_has_focus} focus_pid={focus_pid:?}",
            host.0 as usize,
            desktop_root.0 as usize,
        );
        if let FocusSetOutcome::Failure(error) = focus_result {
            log::warn!("desktop focus restore: SetFocus failed: 0x{error:08X}; {diagnostic}");
        } else if foreground_is_wallpaper && webview_has_focus {
            log::info!("desktop focus restore: keyboard channel restored; {diagnostic}");
        } else {
            log::warn!("desktop focus restore: keyboard channel not restored; {diagnostic}");
        }
      });
    });
    if dispatched.is_err() {
        log::warn!("desktop focus restore: could not reach the main thread");
    }
}

/// 光标处到底是"桌面图标"、"桌面空白"，还是"无法确认"。
///
/// 判据来自**旧接口 MSAA**，不是 UI Automation。实测（2026-09-30，本机）：
///   * UI Automation 从 Explorer 的 `FolderView` 读不到任何子节点（四种读法全为 0），所以那条路
///     永远只会说"这里没有图标"，从而把图标位当成空白 —— 正是最初那个缺陷的根子；
///   * MSAA 的 `AccessibleObjectFromWindow(OBJID_CLIENT)` 一次就给出 18 个子项（带名称与矩形），
///     而在该对象上做 `accHitTest` 时：落在图标上返回子项编号（大于 0），落在空白返回 0。
/// 实测七点零歧义：图标 1/2/3 号 -> 1/2/3，四个空白点 -> 0。
///
/// 三态是刻意的：读不到对象、子项数为 0、调用异常，一律是"无法确认"，绝不退化成"空白"。
/// 判错的代价不对称 —— 把图标当空白会让"打开快捷方式"顺带翻掉桌面层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopPointVerdict {
  /// 双击的语义是两拍：**第一下判定、第二下执行** —— 按下时取三态（这一点是图标、是空白、
/// 还是无法确认），松开时若这次与上一次构成长度/位置都合格的配对，才真正切换。判定只在按下
/// 那一刻取一次，因为点击的目标由按下决定；第一下不合格就直接作废，不会留到第二下去补。
/// 落在某个桌面图标上（或图标标签上）。
  Icon,
  /// 确认是桌面空白处。
  Blank,
  /// 读不到、结构异常或调用失败；调用方必须当作"不切换"。
  Unknown,
}

/// 取 Explorer 的图标列表（`SHELLDLL_DefView` -> `SysListView32`），并拿到它的 MSAA 对象。
#[cfg(windows)]
fn desktop_icon_list_accessible() -> Option<IAccessible> {
  let layer = desktop_icon_layer()?;
  let list = desktop_icon_list_view_in_layer(layer)?;
  let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
  // SAFETY: the interface pointer is written into a local null-initialized slot; a null result is rejected and a non-null COM pointer is adopted exactly once by IAccessible::from_raw, which owns and releases it.
  unsafe {
    AccessibleObjectFromWindow(
      list,
      // OBJID_CLIENT = 0xFFFFFFFC（Windows 头文件里的定义；windows crate 把它放在
      // WindowsAndMessaging 下，这里直接用字面量，避免多引入一个模块）。
      OBJID_CLIENT_ID,
      &IAccessible::IID,
      &mut raw as *mut _ as *mut *mut core::ffi::c_void,
    )
    .ok()?;
    if raw.is_null() {
      return None;
    }
    Some(IAccessible::from_raw(raw))
  }
}

/// 光标处的三态判定。只读，不注入 Explorer、不发鼠标消息。
#[cfg(windows)]
pub(crate) fn desktop_point_verdict() -> DesktopPointVerdict {
  let mut point = POINT::default();
  // SAFETY: `point` is a local POINT that outlives this synchronous call, and a failure takes the Unknown branch instead of reading it.
  if unsafe { GetCursorPos(&mut point) }.is_err() {
    return DesktopPointVerdict::Unknown;
  }
  let Some(accessible) = desktop_icon_list_accessible() else {
    log::info!("桌面空白判定：取不到图标列表的辅助对象，判为无法确认");
    return DesktopPointVerdict::Unknown;
  };
  // SAFETY: `accessible` is an owned COM interface obtained above and still alive; the method is called through its vtable and only returns a count.
  let child_count = match unsafe { accessible.accChildCount() } {
    Ok(count) => count,
    Err(error) => {
      log::info!("桌面空白判定：读取子项数失败（{error}），判为无法确认");
      return DesktopPointVerdict::Unknown;
    }
  };
  if child_count <= 0 {
    log::info!("桌面空白判定：图标列表报告 0 个子项，判为无法确认");
    return DesktopPointVerdict::Unknown;
  }
  // SAFETY: the same owned interface is still alive, the coordinates are plain integers, and the returned VARIANT is owned by Rust and only read below.
  let hit = match unsafe { accessible.accHitTest(point.x, point.y) } {
    Ok(value) => value,
    Err(error) => {
      log::info!("桌面空白判定：accHitTest 失败（{error}），判为无法确认");
      return DesktopPointVerdict::Unknown;
    }
  };
  // accHitTest 的返回值是 VARIANT：本对象内的子项编号是整数，别的元素是对象。
  // SAFETY: the tag field of the returned VARIANT is read first, and the payload field is accessed only in the arm that matches this tag.
  let verdict = match unsafe { hit.Anonymous.Anonymous.vt } {
    windows::Win32::System::Variant::VT_I4 => {
      // SAFETY: this arm runs only when the tag is VT_I4, where the union holds lVal, and the VARIANT still lives on this stack.
      let child = unsafe { hit.Anonymous.Anonymous.Anonymous.lVal };
      if child > 0 {
        DesktopPointVerdict::Icon
      } else {
        DesktopPointVerdict::Blank
      }
    }
    _ => DesktopPointVerdict::Unknown,
  };
  log::info!(
    "桌面空白判定：cursor=({},{}) 子项数={child_count} 结论={:?}",
    point.x,
    point.y,
    verdict
  );
  verdict
}
/// 「光标是否落在桌面表面」：只有光标下的窗口属于桌面（父链能走到桌面宿主本身、
/// 桌面窗口，或类名属于桌面/WorkerW）时才为真。
///
/// 这里刻意**不**重算一遍桌面宿主规则：表/里桌面双击判定用的就是这一条，两处各写
/// 一份迟早会漂移。
///
/// 这条判据**只依赖一个纯值 `HWND`**，不碰任何 Tauri 运行期窗口状态，所以可以安全地
/// 从后台线程调用：悬浮球的靠近检测（`floating_ball::start_ball_monitor`）在主线程把
/// 桌面宿主的句柄解析成整数带进线程，线程里只还原成 `HWND` 再交给这里。
/// **不要**再为它套一层 `app.get_webview_window(...)` 的封装（曾有一个
/// `cursor_on_desktop_surface_via_label`，已删除）：那会把唯一的调用方拽回"后台线程
/// 逐拍 clone wry `Context` 引用计数"的老路上（0.4.7 转储那一族）。
///
/// 句柄已经不在（窗口销毁）时，调用方必须先判否 —— 见
/// `floating_ball::desktop_surface_allows_pop`：下面 `current == background` 不成立时，
/// 判据仍可能命中 `GetDesktopWindow` 与窗口类名两条分支而返回真。
#[cfg(windows)]
pub(crate) fn cursor_is_on_desktop_surface(background: HWND) -> bool {
    let mut point = POINT::default();
    // SAFETY: `background` is a handle the caller proved usable, the POINT and handle locals outlive the calls, and every Win32 call here takes handles only.
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        let mut current = WindowFromPoint(point);
        for _ in 0..16 {
            if current.0.is_null() {
                return false;
            }
            if current == background
                || current == GetDesktopWindow()
                || is_desktop_foreground_class(window_class(current).as_deref())
            {
                return true;
            }
            let Ok(parent) = GetParent(current) else {
                return false;
            };
            if parent.0.is_null() || parent == current {
                return false;
            }
            current = parent;
        }
    }
    false
}

/// 「此刻光标是否落在桌面表面」，句柄取自**槽位**而不是启动时冻结的那一个。
///
/// 悬浮球的靠近检测每拍调它一次。读槽位是纯原子操作，所以这条函数与
/// `cursor_is_on_desktop_surface` 一样可以从后台线程调用；区别只在句柄是不是最新的：
/// `background` 被重建之后（`recover_wallpaper_host` 的 `Recreate`），旧句柄永远不再有效，
/// 球的"只在桌面上弹"这条判据就会一直判否，看起来就是球再也不弹了。槽位让它自动跟上新句柄。
///
/// 判据本身的边界见 `cursor_is_on_desktop_surface`：句柄不在时必须先判否，因为那条函数在
/// `current == background` 不成立时仍可能命中 `GetDesktopWindow` 与窗口类名两条分支。
#[cfg(windows)]
pub(crate) fn desktop_surface_allows_pop_now() -> bool {
    let handle = background_window_handle_now();
    if handle == 0 {
        return false;
    }
    let background = HWND(handle as *mut core::ffi::c_void);
    // SAFETY: the integer came from the atomic handle slot; IsWindow only inspects it and answers false for a destroyed window, which is the state this guard exists for.
    if !unsafe { IsWindow(Some(background)) }.as_bool() {
        return false;
    }
    cursor_is_on_desktop_surface(background)
}



/// 一次双击配对允许的最大间隔。用户实测节奏在 100 到 300 毫秒，900 毫秒留足余量。
/// OBJID_CLIENT：Windows 头文件里的 0xFFFFFFFC（windows crate 把它放在 WindowsAndMessaging 下，
/// 这里直接用字面量，避免为它多引入一个模块）。
const OBJID_CLIENT_ID: u32 = 0xFFFF_FFFC;
const PAIR_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);
/// 一次切换之后的冷却：这段时间内的点击一律不算数，免得快速连点把开关来回拨。
const PAIR_COOLDOWN: std::time::Duration = std::time::Duration::from_millis(350);
/// 两次点击的最大位置差（物理像素）。位置相近才是双击；两次无关单击通常不在一处。
const PAIR_REACH_PX: i32 = 64;
/// 一次"点击"按下的最长时长。更长即为拖动，不参与配对。
const CLICK_MAX_MS: u128 = 400;
/// 一次"点击"期间允许的位移。超过即视为拖动。
const CLICK_MAX_MOVE_PX: i32 = 12;

/// 两次点击的位置是否近到可以算作一次双击。
fn within_double_click_reach(a: (i32, i32), b: (i32, i32), tolerance: i32) -> bool {
  (a.0 - b.0).abs() <= tolerance && (a.1 - b.1).abs() <= tolerance
}

/// 这一次按下到松开是否算一次"干净点击"：够短、没怎么移动。
///
/// 排除拖动是刻意的：拖拽选图标、拖窗口都会产生"按下-移动-松开"，若把它算成一次点击，
/// 两次这样的操作就可能凑成一对而翻掉桌面层。
fn is_clean_click(duration_ms: u128, movement_px: i32, max_ms: u128, max_move_px: i32) -> bool {
  duration_ms <= max_ms && movement_px <= max_move_px
}

/// 按下瞬间是否有组合键。带修饰键的点击是别的意图（例如 Ctrl 多选），不参与配对。
fn modifiers_are_idle(states: [bool; 5]) -> bool {
  !states.iter().any(|pressed| *pressed)
}

/// 两次点击是否构成一次"成立的双击"。四项缺一不可，且**不允许链式重配**：
/// 第二下若不成立，这一对就作废（调用方据此清空状态），不能拿第二下当新的第一下 ——
/// 否则快速连点会形成 1-2、2-3、3-4 这样的重叠配对，一次连点能拨动好几次。
fn pair_is_complete(
  has_previous: bool,
  same_mode: bool,
  within_window: bool,
  within_reach: bool,
) -> bool {
  has_previous && same_mode && within_window && within_reach
}

/// 双击配对的状态机。**严格两两匹配**：一次点击要么上膛、要么与已上膛的那一下组成一次切换，
/// 要么把状态清干净 —— 绝不允许"第二下没配上就当新的第一下"这种链式重配，否则快速连点会形成
/// 1-2、2-3、3-4 这样的重叠配对，一次连点拨动好几次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairState {
  Idle,
  Armed {
    at_ms: u128,
    point: (i32, i32),
    mode_inner: bool,
  },
}

/// 一次合格点击之后，状态该走到哪里、以及是否执行切换。纯函数：不读时钟，时间以毫秒数传入。
pub(crate) fn pair_transition(
  state: PairState,
  now_ms: u128,
  point: (i32, i32),
  mode_inner: bool,
  cooling: bool,
) -> (PairState, bool) {
  if cooling {
    // 冷却期内既不配对也不上膛：刚切过一次，紧接着的连点一律不算数。
    return (PairState::Idle, false);
  }
  match state {
    PairState::Idle => (
      PairState::Armed {
        at_ms: now_ms,
        point,
        mode_inner,
      },
      false,
    ),
    PairState::Armed {
      at_ms,
      point: armed_point,
      mode_inner: armed_mode,
    } => {
      let same_mode = armed_mode == mode_inner;
      let within_window = now_ms.saturating_sub(at_ms) <= PAIR_WINDOW.as_millis();
      let within_reach = within_double_click_reach(armed_point, point, PAIR_REACH_PX);
      if same_mode && within_window && within_reach {
        (PairState::Idle, true)
      } else {
        // 关键：不成立就作废。第二下不会被当成新的第一下。
        (PairState::Idle, false)
      }
    }
  }
}

fn should_toggle_desktop_workspace(
    cursor_on_desktop_surface: bool,
    cursor_hits_interaction: bool,
    blank_is_confirmed: bool,
) -> bool {
    // 第三项必须是**确认**空白（MSAA 三态里的 Blank）：无法确认在调用方就折成 false。
    // 判错的方向不对称 —— 把图标当空白会让"打开快捷方式"顺带翻掉桌面层。
    cursor_on_desktop_surface && !cursor_hits_interaction && blank_is_confirmed
}

/// 「点功能组件却切回表桌面」只可能是三个输入里有一个不成立，而热区是前端按元素 rect
/// 发布的：光标落在哪个矩形之外、UI Automation 认为桌面是否空白，必须留痕，否则只能猜。
#[cfg(windows)]
fn log_workspace_toggle_decision(hits_interaction: bool, automation_blank: bool) {
    let mut cursor = POINT::default();
    // SAFETY: `cursor` is a local POINT written by this call; a failure maps to the sentinel instead of reading it.
    let (cursor_x, cursor_y) = unsafe {
        if GetCursorPos(&mut cursor).is_err() {
            (i32::MIN, i32::MIN)
        } else {
            (cursor.x, cursor.y)
        }
    };
    let rects = interaction_regions()
        .read()
        .map(|state| {
            state
                .regions
                .iter()
                .map(|region| {
                    format!(
                        "({},{})-({},{})",
                        region.left, region.top, region.right, region.bottom
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|_| "<poisoned>".into());
    log::info!(
        "workspace toggle armed: cursor=({cursor_x},{cursor_y}) hits_interaction_region={hits_interaction} automation_blank={automation_blank} rects=[{rects}]"
    );
}

#[cfg(windows)]
pub fn start_desktop_workspace_monitor(app: tauri::AppHandle) {
    // 运行期窗口状态**只能在本线程（主线程）上取**：`get_webview_window` 会 `Webview::clone`
    // —— 那一步会去动 `DetachedWebview` 的引用计数对象，而该对象的计数与 wry 的 `Context`
    // 是同一份共享状态（`Context::clone` 溢出哨兵 = `ud2` / `__fastfail(7)`；见本文件顶部）。后台线程取一次
    // 就等于并发改这份计数，破坏是滞后的：后台线程先打坏，很久以后主线程在窗口消息里拿垃圾
    // 指针崩（本仓库的 0.2.0/0.4.7 两份转储同构，见 dsh-wallpaper.exe.8376.dmp）。
    //
    // 因此句柄在**主线程**解析，线程闭包里只留 `HWND` 这个纯值：它不携带任何引用计数，
    // 跨线程搬运完全安全，而"窗口还在不在"改用 Win32 的 `IsWindow` 直接问。旧代码是在
    // 16ms 轮询里每轮 `app.get_webview_window("background").is_none()` —— 那正是后台线程
    // 逐轮 clone 引用计数的那一处，已删除。
    //
    // 与上一版（提交 `ab9c9f3`）的唯一区别：这个纯值**每拍从句柄槽位重读一次**，而不是
    // 启动时冻结。冻结的写法在 `background` 被销毁并重建之后（`recover_wallpaper_host` 的
    // `Recreate` 分支）手里只剩一个永远无效的旧句柄，`IsWindow` 一直回答否，线程遂退场，
    // "双击桌面进里桌面 / 离开里桌面"就此永久失效。槽位由主线程在创建与重建处写入
    // （见 `BACKGROUND_WINDOW_HANDLE`），线程只做一次原子读 + 一次 `IsWindow`。
    std::thread::Builder::new()
        .name("desktop-workspace-monitor".into())
        .spawn(move || {
        // 防线：这条闭包**绝不允许**再回 Tauri 取运行期窗口状态（见上）。真这么干了，
        // 调试构建会在这里带线程名喊出来，而不是等几个月后在主线程里崩成垃圾指针。
        assert_not_acting_as_main_thread();
        // UIA requires COM initialization on the monitor thread. A prior COM
        // mode is harmless: UIA can still be created on that thread.
        // SAFETY: a null reserved pointer selects the default COM library; the call initializes the calling monitor thread's apartment, and the HRESULT is discarded because an apartment that is already initialized needs no further action.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // 判据改走 MSAA（旧接口）：UI Automation 从 Explorer 的 FolderView 读不到任何子节点，
        // 因此不必再初始化它。MSAA 按需取用，不注入 Explorer。
        log::info!("表/里桌面双击监控已启动（判据：MSAA accHitTest 三态）");


        let mut was_down = false;
        // 上一次合格且干净的点击：时刻、位置、当时的表/里桌面模式。
        // 配对状态机的当前位置，以及本次监控的时间原点（纯函数用毫秒数，便于单测）。
        let mut pair_state = PairState::Idle;
        let started_at = std::time::Instant::now();
        let mut last_toggle_at: Option<u128> = None;
        // 上一次真正切换的时刻：决定冷却，避免快速连点把桌面层来回拨。
        // 本次按下的现场：时刻、位置、按下时是否合格、按下时有无组合键。
        let mut press: Option<(std::time::Instant, (i32, i32), bool, bool)> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(16));
            // 每拍从槽位取当前句柄：窗口重建之后这里拿到的就是新句柄。
            //
            // 这里**不是**"后台线程重新解析窗口"：解析仍然只发生在主线程的创建/重建点，
            // 本线程拿到的只是一个整数。真要在后台线程"重新解析"，就必须调
            // `get_webview_window`，那正是 0.4.7 转储里那条崩溃路径，故绝不允许。
            //
            // 拿不到句柄（0）或槽位里的句柄刚被销毁时**不做事但继续等**，不退场：
            // `recover_wallpaper_host` 会先 `destroy` 再重建 `background`，中间那一小段
            // 就是这个状态；退场正是"重建后功能永久失效"的成因。等着的代价是每拍一次原子读，
            // 而 `background` 在正常运行期始终存在，所以这不是一个会永远转下去的无主轮询。
            let background_raw = background_window_handle_now();
            if !background_handle_is_usable(background_raw) {
                continue;
            }
            let background = HWND(background_raw as *mut core::ffi::c_void);
            // SAFETY: GetAsyncKeyState takes a virtual-key constant and reads global input state; no out-parameter and no Rust memory.
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            // 松开时成形：拖动（按下-移动-松开）天然被排除，不需要额外判断。
if !down && was_down {
                // 松开时成形：拖动（按下-移动-松开）天然被排除，不需要额外判断。
                if let Some((pressed_at, pressed_point, eligible, keys_idle)) = press.take() {
                    let mut released = POINT::default();
                    // SAFETY: `released` is a local POINT written by this call; a failure keeps the pressed point instead of reading it.
                    let released_point = if unsafe { GetCursorPos(&mut released) }.is_err() {
                        pressed_point
                    } else {
                        (released.x, released.y)
                    };
                    let held_ms = pressed_at.elapsed().as_millis();
                    let movement = (released_point.0 - pressed_point.0)
                        .abs()
                        .max((released_point.1 - pressed_point.1).abs());
                    let clean = is_clean_click(held_ms, movement, CLICK_MAX_MS, CLICK_MAX_MOVE_PX);
                    let now_ms = started_at.elapsed().as_millis();
                    let mode_inner = INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire);
                    if !eligible || !keys_idle || !clean {
                        // 不合格的一下把状态清干净：围栏要求点击两两匹配，任何杂音都重新开始。
                        log::info!(
                            "点击不计入配对：合格={eligible} 无组合键={keys_idle} 干净={clean}（时长 {held_ms}ms，位移 {movement}px）"
                        );
                        pair_state = PairState::Idle;
                    } else {
                        let cooling = last_toggle_at
                            .is_some_and(|at| now_ms.saturating_sub(at) < PAIR_COOLDOWN.as_millis());
                        let (next, toggle) =
                            pair_transition(pair_state, now_ms, pressed_point, mode_inner, cooling);
                        pair_state = next;
                        if toggle {
                            last_toggle_at = Some(now_ms);
                            log::info!(
                                "桌面空白双击成立：当前为{}桌面（围栏：两两匹配）",
                                if mode_inner { "里" } else { "表" }
                            );
                            let result = if mode_inner {
                                leave_inner_workspace(&app)
                            } else {
                                enter_inner_workspace(&app)
                            };
                            match result {
                                Err(error) => log::warn!("无法切换表/里桌面图标层：{error}"),
                                Ok(()) => log::info!(
                                    "桌面空白双击：切换至{}桌面",
                                    if INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire) {
                                        "里"
                                    } else {
                                        "表"
                                    }
                                ),
                            }
                        } else if cooling {
                            log::info!("点击不计入配对：距上次切换不足 {}ms", PAIR_COOLDOWN.as_millis());
                        }
                    }
                }
            }
            if down && !was_down {
                // 按下的现场：三条判据 + 组合键状态，全部**在这一刻**取，因为点击的目标由按下决定。
                // SAFETY: the five calls take virtual-key constants and read global input state; no out-parameters and no Rust memory.
                let keys = unsafe {
                    [
                        GetAsyncKeyState(VK_CONTROL.0 as i32) < 0,
                        GetAsyncKeyState(VK_MENU.0 as i32) < 0,
                        GetAsyncKeyState(VK_SHIFT.0 as i32) < 0,
                        GetAsyncKeyState(VK_LWIN.0 as i32) < 0,
                        GetAsyncKeyState(VK_RWIN.0 as i32) < 0,
                    ]
                };
                let mut pressed = POINT::default();
                // SAFETY: `pressed` is a local POINT written by this call; a failure maps to the sentinel instead of reading it.
                let pressed_point = if unsafe { GetCursorPos(&mut pressed) }.is_err() {
                    (i32::MIN, i32::MIN)
                } else {
                    (pressed.x, pressed.y)
                };
                let verdict = desktop_point_verdict();
                // 三条判据在**任何模式下都要成立**。此前这里为"里桌面交给 WebView"留过一条短路，
                // 等于在里桌面下旁路了输入岛等区域的判据 —— 实测表现为"点击穿透被覆盖"与乱配对。
                // 句柄是主线程解析好带下来的纯值；这里不再回 Tauri 取窗口。
                let eligible = !background.0.is_null()
                    && should_toggle_desktop_workspace(
                        cursor_is_on_desktop_surface(background),
                        cursor_hits_interaction_region(background),
                        verdict == DesktopPointVerdict::Blank,
                    );
                press = Some((
                    std::time::Instant::now(),
                    pressed_point,
                    eligible,
                    modifiers_are_idle(keys),
                ));
                if !eligible {
                    log::info!("按下即不合格（verdict={verdict:?}）");
                }
            }
            was_down = down;
        }
        })
        .expect("无法启动表/里桌面双击监控线程");
}

#[cfg(not(windows))]
pub fn attach_to_workerw(_: &tauri::WebviewWindow) -> Result<(), String> {
    Err("WorkerW only exists on Windows".into())
}
#[cfg(not(windows))]
pub fn start_wallpaper_host(_: tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn acquire_shared_wallpaper_host() -> bool {
    true
}
#[cfg(not(windows))]
pub fn register_session_events(_: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn start_foreground_monitor(_: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_set_focus_return_with_no_last_error_is_success() {
        assert_eq!(
            classify_focus_set_result(true, ERROR_SUCCESS.0),
            FocusSetOutcome::Success
        );
    }

    #[test]
    fn a_null_set_focus_return_with_an_error_preserves_the_error_code() {
        assert_eq!(
            classify_focus_set_result(true, 87),
            FocusSetOutcome::Failure(87)
        );
    }

    #[test]
    fn a_non_null_set_focus_return_is_success_even_with_a_residual_error() {
        assert_eq!(
            classify_focus_set_result(false, 87),
            FocusSetOutcome::Success
        );
    }

    /// 句柄槽位的读侧判据：「窗口重建期间」必须继续看，而不是退场。
    ///
    /// 这一条锁的就是本轮修的缺陷本身：句柄只解析一次的旧写法在 `background` 被销毁重建时
    /// 手里是旧句柄，三条线程据此退场，重建之后也回不来。**已销毁必须仍然算"可用"** ——
    /// 那是重建的中间态，不是"这个窗口永远不会再回来"。
    ///
    /// 判据拆成纯函数是因为 `IsWindow` 需要真正的句柄：句柄空间是会话级的，单测里
    /// **不能**拿一个随便写的整数当"已销毁句柄"——那个值有可能正被本机另一个进程的窗口
    /// 占用（实测就撞上过，属于会偶发失败的写法）。所以这里测的是判据的四种输入组合，
    /// `IsWindow` 本身是系统行为，不需要也不应该由本仓库的测试来断言。
    #[cfg(windows)]
    #[test]
    fn a_missing_or_destroyed_window_keeps_the_watching_thread_alive() {
        // 槽位为空：没有候选窗口，这一拍不做事。
        assert!(!keep_watching_after_tick(false, false));
        // 槽位里有句柄、句柄已销毁（重建的中间态）：仍然算"等一下就有"，不许退场。
        assert!(keep_watching_after_tick(true, false));
        // 句柄在且活着：照常做事。
        assert!(keep_watching_after_tick(true, true));
        // 槽位为 0 而"活着"这一位没有被 `IsWindow` 赋值过（调用方不会这样传）：
        // 结论仍按槽位来，不会因为一个无关的布尔位翻成"可用"。
        assert!(!keep_watching_after_tick(false, true));
    }

    /// 悬浮球那半格写的是"此刻真实存在的窗口"，读侧与主线程的写侧必须对上。
    ///
    /// 这条测试只碰纯原子量，不建窗口：它要证明的是「主线程写下的整数能在读侧原样取到」，
    /// 也就是 `ensure_ball` 与监控线程之间的接线语义。
    #[cfg(windows)]
    #[test]
    fn the_ball_handle_slot_round_trips_the_value_the_main_thread_wrote() {
        let previous = ball_window_handle_now();
        publish_ball_window_handle(Some(HWND(0x1234 as *mut core::ffi::c_void)));
        assert_eq!(ball_window_handle_now(), 0x1234);
        publish_ball_window_handle(None);
        assert_eq!(ball_window_handle_now(), 0);
        // 还原现场：槽位是进程级的，别把别的测试（或运行的进程）带歪。
        publish_ball_window_handle(if previous == 0 {
            None
        } else {
            Some(HWND(previous as *mut core::ffi::c_void))
        });
    }

    /// 「此刻光标是否落在桌面表面」在句柄槽位为空（或指向已销毁窗口）时必须判否。
    ///
    /// 判否的理由见 `desktop_surface_allows_pop_now`：那条判据在 `current == background`
    /// 不成立时仍可能命中 `GetDesktopWindow` 与窗口类名两条分支而返回真，所以句柄不可用
    /// 必须先挡在门外。槽位默认是 0（除非别处刚写过），这里显式按 0 断言。
    #[cfg(windows)]
    #[test]
    fn a_missing_desktop_handle_never_allows_a_pop_now() {
        let previous = background_window_handle_now();
        publish_background_window_handle(None);
        assert!(!desktop_surface_allows_pop_now());
        publish_background_window_handle(if previous == 0 {
            None
        } else {
            Some(HWND(previous as *mut core::ffi::c_void))
        });
    }



    fn candidate(is_worker: bool, hosts_desktop_icons: bool) -> DesktopWindowCandidate {
        DesktopWindowCandidate {
            is_worker,
            hosts_desktop_icons,
        }
    }

    #[test]
    fn prefers_the_non_icon_worker_after_the_icon_host() {
        let candidates = [
            candidate(true, false),
            candidate(false, false),
            candidate(true, true),
            candidate(true, false),
        ];
        assert_eq!(select_wallpaper_worker(&candidates), Some(3));
    }

    #[test]
    fn selects_a_non_icon_worker_even_when_it_precedes_the_icon_host() {
        let candidates = [
            candidate(true, false),
            candidate(true, true),
            candidate(false, false),
        ];
        assert_eq!(select_wallpaper_worker(&candidates), Some(0));
    }

    #[test]
    fn never_selects_the_desktop_icon_worker() {
        let candidates = [candidate(false, false), candidate(true, true)];
        assert_eq!(select_wallpaper_worker(&candidates), None);
    }

    #[test]
    fn accepts_progman_only_as_a_valid_desktop_parent() {
        assert!(is_valid_wallpaper_parent_class(Some("WorkerW")));
        assert!(is_valid_wallpaper_parent_class(Some("Progman")));
        assert!(!is_valid_wallpaper_parent_class(Some("Chrome_WidgetWin_1")));
        assert!(!is_valid_wallpaper_parent_class(None));
    }

    #[test]
    fn recognizes_every_explorer_desktop_host_as_foreground() {
        assert!(is_desktop_foreground_class(Some("WorkerW")));
        assert!(is_desktop_foreground_class(Some("Progman")));
        assert!(!is_desktop_foreground_class(Some("Shell_TrayWnd")));
        assert!(!is_desktop_foreground_class(Some("Chrome_WidgetWin_1")));
        assert!(!is_desktop_foreground_class(None));
    }

    #[test]
    /// 围栏的四条转移：上膛、成立、作废、冷却。重点是**作废之后不重新上膛**。
#[test]
fn the_fence_pairs_clicks_two_by_two_and_never_chains() {
  let p = (100, 100);
  // 第一下：上膛，不切换。
  let (s1, t1) = pair_transition(PairState::Idle, 1_000, p, false, false);
  assert!(!t1);
  assert!(matches!(s1, PairState::Armed { .. }));
  // 第二下在窗内、同位、同模式：切换，并回到 Idle。
  let (s2, t2) = pair_transition(s1, 1_200, p, false, false);
  assert!(t2);
  assert_eq!(s2, PairState::Idle);
  // 第二下超出时间窗：作废，不切换；而且**第三下只能算新的第一下**（这正是围栏要的效果）。
  let (s3, t3) = pair_transition(s1, 2_000, p, false, false);
  assert!(!t3);
  assert_eq!(s3, PairState::Idle);
  let (s4, t4) = pair_transition(s3, 2_400, p, false, false);
  assert!(!t4, "作废之后的那一下只能上膛，不能直接成对");
  assert!(matches!(s4, PairState::Armed { .. }));
  // 位置太远：作废。
  let (s5, t5) = pair_transition(s1, 1_200, (900, 900), false, false);
  assert!(!t5);
  assert_eq!(s5, PairState::Idle);
  // 模式不同：作废（表桌面的第一下不该和里桌面的第二下凑对）。
  let (s6, t6) = pair_transition(s1, 1_200, p, true, false);
  assert!(!t6);
  assert_eq!(s6, PairState::Idle);
  // 冷却期内：连上膛都不做。
  let (s7, t7) = pair_transition(PairState::Idle, 1_000, p, false, true);
  assert!(!t7);
  assert_eq!(s7, PairState::Idle);
}

/// 围栏：四项缺一不可，而且不允许链式重配（第二下不成立就作废，不能当新的第一下）。
#[test]
fn a_pair_needs_all_four_conditions_and_never_chains() {
  assert!(pair_is_complete(true, true, true, true));
  assert!(!pair_is_complete(false, true, true, true));
  assert!(!pair_is_complete(true, false, true, true));
  assert!(!pair_is_complete(true, true, false, true));
  assert!(!pair_is_complete(true, true, true, false));
}

/// 配对的两条几何规则：位置相近才算同一次双击。
#[test]
fn a_pair_must_land_in_nearly_the_same_place() {
  assert!(within_double_click_reach((100, 200), (104, 197), PAIR_REACH_PX));
  assert!(within_double_click_reach((100, 200), (100, 200), PAIR_REACH_PX));
  // 相隔很远的两次单击：不算。时间窗口宽松之后，这一条是防止两次无关单击凑成一对的关键。
  assert!(!within_double_click_reach((100, 200), (800, 900), PAIR_REACH_PX));
  assert!(!within_double_click_reach((100, 200), (100, 300), PAIR_REACH_PX));
}

/// 一次点击必须够短、没怎么移动；拖动（按下-移动-松开）不算。
#[test]
fn a_click_that_was_really_a_drag_does_not_count() {
  assert!(is_clean_click(120, 0, CLICK_MAX_MS, CLICK_MAX_MOVE_PX));
  assert!(is_clean_click(399, 12, CLICK_MAX_MS, CLICK_MAX_MOVE_PX));
  // 按住太久：是拖动或长按，不是点击。
  assert!(!is_clean_click(401, 0, CLICK_MAX_MS, CLICK_MAX_MOVE_PX));
  // 按位移出：拖拽选图标、拖窗口都属于这一类。
  assert!(!is_clean_click(120, 13, CLICK_MAX_MS, CLICK_MAX_MOVE_PX));
}

/// 带组合键的点击是别的意图（Ctrl 多选之类），不参与配对。
#[test]
fn a_click_with_a_modifier_never_pairs() {
  assert!(modifiers_are_idle([false, false, false, false, false]));
  assert!(!modifiers_are_idle([true, false, false, false, false]));
  assert!(!modifiers_are_idle([false, false, false, false, true]));
}

#[test]
fn workspace_toggle_requires_a_blank_desktop_and_never_a_chat_region() {
        assert!(should_toggle_desktop_workspace(true, false, true));
        assert!(!should_toggle_desktop_workspace(false, false, true));
        assert!(!should_toggle_desktop_workspace(true, true, true));
        assert!(!should_toggle_desktop_workspace(true, false, false));
    }



    #[test]
    fn upgrades_a_progman_fallback_when_workerw_appears() {
        assert!(should_upgrade_wallpaper_parent(Some("Progman"), true));
        assert!(!should_upgrade_wallpaper_parent(Some("Progman"), false));
        assert!(!should_upgrade_wallpaper_parent(Some("WorkerW"), true));
    }

    #[test]
    fn host_action_is_noop_only_for_a_valid_sized_worker_parent() {
        assert_eq!(
            decide_wallpaper_host_action(true, true, true, true),
            WallpaperHostAction::None
        );
        assert_eq!(
            decide_wallpaper_host_action(true, true, true, false),
            WallpaperHostAction::Resize
        );
        assert_eq!(
            decide_wallpaper_host_action(true, false, false, false),
            WallpaperHostAction::Reattach
        );
        assert_eq!(
            decide_wallpaper_host_action(false, false, false, false),
            WallpaperHostAction::Recreate
        );
    }
















}
