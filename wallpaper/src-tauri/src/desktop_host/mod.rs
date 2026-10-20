use std::sync::{OnceLock, RwLock};

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
mod foreground;
#[cfg_attr(all(windows, feature = "lite"), allow(unused_imports))]
pub(crate) use foreground::{hand_over_keyboard_for_app, verify_island_click};
#[allow(unused_imports)]
pub(crate) use foreground::hand_over_keyboard_after_verified_click;
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use foreground::start_foreground_monitor;
#[cfg(windows)]
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use foreground::desktop_surface_allows_pop_now;
#[cfg(windows)]
pub(crate) use foreground::cursor_is_on_desktop_surface;
mod hit_region;
mod workspace;
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use hit_region::{
    begin_interaction_region_session, island_visible_from_regions, update_interaction_regions,
    InteractionRegionInput, InteractionRegionUpdateResult,
};
#[allow(unused_imports)]
pub(crate) use hit_region::ISLAND_REGION_ID;
#[cfg(windows)]
use hit_region::{
    install_desktop_hit_testing, interaction_regions,
    MAX_SCALE_FACTOR, MIN_SCALE_FACTOR,
};
#[cfg(windows)]
use workspace::set_desktop_icons_visible;
#[cfg(windows)]
#[allow(unused_imports)]
pub(crate) use workspace::{desktop_icon_layer, desktop_point_verdict};
#[allow(unused_imports)]
pub(crate) use workspace::{DesktopPointVerdict, PairState, pair_transition};
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use workspace::{enter_inner_workspace, leave_inner_workspace};
#[cfg(windows)]
pub(crate) use workspace::restore_desktop_icons;
#[cfg(windows)]
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use workspace::start_desktop_workspace_monitor;
#[cfg(windows)]
use state::{
    BALL_WINDOW_HANDLE, BACKGROUND_WINDOW_HANDLE, WALLPAPER_HOST_STATUS,
};

#[cfg(windows)]
#[allow(unused_imports)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};
use windows::{
    core::{w, BOOL, PCWSTR},
    Win32::{
        Foundation::{
            GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT,
            POINT, RECT, WPARAM,
        },
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND},
            Gdi::ClientToScreen,
        },
        System::{
            Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
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
            SW_SHOWNA, WM_DISPLAYCHANGE, WM_DPICHANGED,
            WM_NCDESTROY, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WS_BORDER,
            WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
            WS_EX_STATICEDGE, WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
        UI::{
            Accessibility::{CUIAutomation, IUIAutomation, UIA_ListItemControlTypeId},
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
#[cfg(test)]
mod tests {
    use super::*;

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
