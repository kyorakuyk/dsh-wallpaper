use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use crate::app_core::{AppAction, AppCore, WallpaperHostMode, WallpaperHostStatus};

#[cfg(windows)]
use crate::lock_screen_backup::{
    self, discard_backup_after_failed_takeover, ensure_backup_for_takeover, has_stale_backup,
    inspect_backup, managed_image_is_active, remove_backup_after_verified_restore,
    restore_snapshot_path, same_local_file_uri, LockScreenBackupLease, LockScreenBackupManifest,
    LockScreenBackupState,
};

#[cfg(windows)]
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[cfg(windows)]
use windows::{
    core::{BOOL, HSTRING, PCWSTR},
    Storage::StorageFile,
    System::UserProfile::{LockScreen, UserProfilePersonalizationSettings},
    Win32::{
        Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, HWND, LPARAM, LRESULT, POINT,
            RECT, WPARAM,
        },
        Graphics::{
            Dwm::{
                DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_COLOR_NONE,
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
            },
            Gdi::ClientToScreen,
        },
        Storage::Packaging::Appx::GetCurrentPackageFullName,
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetClientRect, GetCursorPos,
            GetDesktopWindow, GetForegroundWindow, GetParent, GetWindowLongPtrW, GetWindowRect,
            IsWindow, IsWindowVisible, SendMessageTimeoutW, SetParent, SetWindowLongPtrW,
            SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE, HTTRANSPARENT,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SEND_MESSAGE_TIMEOUT_FLAGS, SMTO_NORMAL,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
            SW_HIDE, SW_SHOWNA, WM_ACTIVATE, WM_NCACTIVATE, WM_NCDESTROY, WM_NCHITTEST,
            WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME,
            WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_TOOLWINDOW,
            WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
            WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
        UI::{
            Accessibility::{CUIAutomation, IUIAutomation, UIA_ListItemControlTypeId},
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        },
    },
};

#[cfg(windows)]
const PROGMAN_SPAWN_WORKERW: u32 = 0x052C;

#[cfg(windows)]
static WALLPAPER_RECOVERY_QUEUED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
#[cfg(windows)]
static INNER_WORKSPACE_ACTIVE: AtomicBool = AtomicBool::new(false);

const MAX_INTERACTION_REGIONS: usize = 128;
const MIN_SCALE_FACTOR: f64 = 0.5;
const MAX_SCALE_FACTOR: f64 = 8.0;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRegionInput {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalInteractionRegion {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl PhysicalInteractionRegion {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Clone, Debug, Default)]
struct InteractionRegionState {
    session: u64,
    revision: u64,
    scale_factor: f64,
    regions: Vec<PhysicalInteractionRegion>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRegionUpdateResult {
    pub revision: u64,
    pub region_count: usize,
    pub stale: bool,
}

static INTERACTION_REGIONS: OnceLock<RwLock<InteractionRegionState>> = OnceLock::new();

static INTERACTION_REGION_SESSION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(windows)]
static DESKTOP_FOREGROUND_STATE: OnceLock<RwLock<Option<bool>>> = OnceLock::new();

#[cfg(windows)]
static WALLPAPER_HOST_STATUS: OnceLock<RwLock<WallpaperHostStatus>> = OnceLock::new();

fn interaction_regions() -> &'static RwLock<InteractionRegionState> {
    INTERACTION_REGIONS.get_or_init(|| RwLock::new(InteractionRegionState::default()))
}

#[cfg(windows)]
fn desktop_foreground_state() -> &'static RwLock<Option<bool>> {
    DESKTOP_FOREGROUND_STATE.get_or_init(|| RwLock::new(None))
}

#[cfg(windows)]
fn wallpaper_host_status_state() -> &'static RwLock<WallpaperHostStatus> {
    WALLPAPER_HOST_STATUS.get_or_init(|| RwLock::new(WallpaperHostStatus::default()))
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
                WallpaperHostMode::WorkerW => log::info!(
                    "wallpaper host ready: mode=workerw generation={} recoveries={}",
                    status.generation,
                    status.recovery_count
                ),
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
            let _ = app.emit("app-snapshot", &snapshot);
        }
    }
}

fn scale_interaction_region(
    region: InteractionRegionInput,
    scale_factor: f64,
) -> Option<PhysicalInteractionRegion> {
    if !scale_factor.is_finite()
        || !(MIN_SCALE_FACTOR..=MAX_SCALE_FACTOR).contains(&scale_factor)
        || !region.x.is_finite()
        || !region.y.is_finite()
        || !region.width.is_finite()
        || !region.height.is_finite()
        || region.width <= 0.0
        || region.height <= 0.0
    {
        return None;
    }
    let left = (region.x * scale_factor).floor();
    let top = (region.y * scale_factor).floor();
    let right = ((region.x + region.width) * scale_factor).ceil();
    let bottom = ((region.y + region.height) * scale_factor).ceil();
    if left < i32::MIN as f64
        || top < i32::MIN as f64
        || right > i32::MAX as f64
        || bottom > i32::MAX as f64
        || right <= left
        || bottom <= top
    {
        return None;
    }
    Some(PhysicalInteractionRegion {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    })
}

fn point_hits_interaction_region(regions: &[PhysicalInteractionRegion], x: i32, y: i32) -> bool {
    regions.iter().any(|region| region.contains(x, y))
}

pub fn update_interaction_regions(
    regions: Vec<InteractionRegionInput>,
    scale_factor: f64,
    session: u64,
    revision: u64,
) -> Result<InteractionRegionUpdateResult, String> {
    if regions.len() > MAX_INTERACTION_REGIONS {
        return Err(format!(
            "interaction region count exceeds {MAX_INTERACTION_REGIONS}"
        ));
    }
    if !scale_factor.is_finite() || !(MIN_SCALE_FACTOR..=MAX_SCALE_FACTOR).contains(&scale_factor) {
        return Err(format!("invalid interaction scale factor: {scale_factor}"));
    }
    let physical_regions: Vec<_> = regions
        .into_iter()
        .enumerate()
        .map(|(index, region)| {
            scale_interaction_region(region, scale_factor)
                .ok_or_else(|| format!("invalid interaction region at index {index}"))
        })
        .collect::<Result<_, _>>()?;
    let mut state = interaction_regions()
        .write()
        .map_err(|_| "interaction region state poisoned".to_string())?;
    if session != state.session || revision < state.revision {
        return Ok(InteractionRegionUpdateResult {
            revision: state.revision,
            region_count: state.regions.len(),
            stale: true,
        });
    }
    state.revision = revision;
    state.scale_factor = scale_factor;
    state.regions = physical_regions;
    drop(state);
    Ok(InteractionRegionUpdateResult {
        revision,
        region_count: interaction_regions()
            .read()
            .map(|state| state.regions.len())
            .unwrap_or_default(),
        stale: false,
    })
}

pub fn begin_interaction_region_session() -> Result<u64, String> {
    let session = INTERACTION_REGION_SESSION.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
    let mut state = interaction_regions()
        .write()
        .map_err(|_| "interaction region state poisoned".to_string())?;
    state.session = session;
    state.revision = 0;
    state.scale_factor = 1.0;
    state.regions.clear();
    Ok(session)
}

#[cfg(windows)]
pub fn configure_settings_window(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    apply_settings_window_frame_policy(hwnd)?;
    log_settings_window_metrics(hwnd, "configure");
    unsafe {
        if !SetWindowSubclass(hwnd, Some(settings_window_subclass), 0x4453_4853, 0).as_bool() {
            return Err("无法监听设置窗口边框状态".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn log_settings_window_metrics(hwnd: HWND, stage: &str) {
    unsafe {
        let mut outer = RECT::default();
        let mut client = RECT::default();
        let mut origin = POINT::default();
        if GetWindowRect(hwnd, &mut outer).is_ok()
            && GetClientRect(hwnd, &mut client).is_ok()
            && ClientToScreen(hwnd, &mut origin).as_bool()
        {
            log::info!(
                "settings frame {stage}: outer=({},{} {}x{}), client-origin-offset=({},{}), client={}x{}",
                outer.left,
                outer.top,
                outer.right - outer.left,
                outer.bottom - outer.top,
                origin.x - outer.left,
                origin.y - outer.top,
                client.right - client.left,
                client.bottom - client.top,
            );
        }
    }
}

#[cfg(windows)]
fn apply_settings_window_frame_policy(hwnd: HWND) -> Result<(), String> {
    unsafe {
        // The rounded outline is rendered by the settings web UI. Windows 11
        // otherwise adds its own one-pixel DWM border around the transparent
        // top-level HWND, which shows up as a white halo outside the CSS curve.
        let border_color = DWMWA_COLOR_NONE;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            std::ptr::from_ref(&border_color).cast(),
            std::mem::size_of_val(&border_color) as u32,
        )
        .map_err(|error| format!("无法关闭设置窗口系统边框：{error}"))?;

        // A decoration-less yet resizable Tao window still owns a two-pixel
        // non-client strip at its top. Give DWM the exact opaque host color
        // so it cannot expose the desktop/white fallback during repaints.
        // COLORREF is 0x00BBGGRR: #0d1625.
        let caption_color: u32 = 0x0025_160d;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            std::ptr::from_ref(&caption_color).cast(),
            std::mem::size_of_val(&caption_color) as u32,
        )
        .map_err(|error| format!("无法设置设置窗口顶部颜色：{error}"))?;

        // Let CSS own the rounded rectangle as well. Applying a second DWM
        // corner mask produces bright antialiasing pixels outside that curve.
        let corner_preference = DWMWCP_DONOTROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner_preference).cast(),
            std::mem::size_of_val(&corner_preference) as u32,
        )
        .map_err(|error| format!("无法关闭设置窗口系统圆角：{error}"))?;
    }
    Ok(())
}

#[cfg(windows)]
unsafe extern "system" fn settings_window_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    match message {
        // DWM may restore the active accent border after processing either of
        // these messages. Reapply the policy after the default window proc.
        WM_ACTIVATE | WM_NCACTIVATE => {
            let _ = apply_settings_window_frame_policy(hwnd);
            log_settings_window_metrics(hwnd, "activate");
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(settings_window_subclass), 0x4453_4853);
        }
        _ => {}
    }
    result
}

#[cfg(not(windows))]
pub fn configure_settings_window(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
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
fn desktop_icon_layer() -> Option<HWND> {
    enumerate_desktop_windows()
        .ok()?
        .into_iter()
        .find_map(|item| {
            if !item.candidate.hosts_desktop_icons {
                return None;
            }
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

#[cfg(windows)]
fn request_wallpaper_worker() -> Result<HWND, String> {
    let progman = unsafe { FindWindowW(windows::core::w!("Progman"), PCWSTR::null()) }
        .map_err(|error| format!("无法找到 Explorer Progman 窗口：{error}"))?;

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
        if unsafe { IsWindowVisible(worker).as_bool() } {
            return Ok(worker);
        }
        if unsafe { IsWindowVisible(progman).as_bool() } {
            return Ok(progman);
        }
    }

    // Windows 11 的新版 Explorer 使用 0xD 参数的两步协议。
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
    if let Ok(worker) = locate_wallpaper_worker() {
        if unsafe { IsWindowVisible(worker).as_bool() } {
            return Ok(worker);
        }
        if unsafe { IsWindowVisible(progman).as_bool() } {
            return Ok(progman);
        }
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
    let class_len = unsafe { GetClassNameW(hwnd, &mut class) };
    (class_len > 0).then(|| String::from_utf16_lossy(&class[..class_len as usize]))
}

#[cfg(windows)]
fn resize_wallpaper_to_parent(background: HWND, worker: HWND) -> Result<(), String> {
    let mut bounds = RECT::default();
    unsafe { GetClientRect(worker, &mut bounds) }
        .map_err(|error| format!("无法读取 WorkerW 尺寸：{error}"))?;
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width <= 0 || height <= 0 {
        return Err(format!("WorkerW 客户区尺寸无效：{width}x{height}"));
    }
    unsafe {
        SetWindowPos(
            background,
            None,
            0,
            0,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
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
fn place_wallpaper_behind_desktop_icons(background: HWND, parent: HWND) -> Result<(), String> {
    if window_class(parent).as_deref() != Some("Progman") {
        return Ok(());
    }
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
    unsafe {
        // hWndInsertAfter places the wallpaper immediately behind the icon
        // view in the child Z-order, while remaining above Progman's painted
        // system wallpaper background.
        SetWindowPos(
            background,
            Some(icon_view),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .map_err(|error| format!("无法将壁纸放到桌面图标层后方：{error}"))?;
    }
    Ok(())
}

#[cfg(windows)]
fn attach_hwnd_to_workerw(background: HWND) -> Result<HWND, String> {
    let worker = request_wallpaper_worker()?;
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
    place_wallpaper_behind_desktop_icons(background, worker)?;
    unsafe {
        let _ = ShowWindow(background, SW_SHOWNA);
    }
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
    if !unsafe { IsWindow(Some(background)).as_bool() } {
        return (WallpaperHostAction::Recreate, Some(background));
    }
    let parent = unsafe { GetParent(background) }.ok();
    let parent_exists = parent
        .map(|hwnd| unsafe { IsWindow(Some(hwnd)).as_bool() })
        .unwrap_or(false);
    let parent_class = parent.and_then(window_class);
    let parent_is_worker = is_valid_wallpaper_parent_class(parent_class.as_deref())
        && parent
            .map(|hwnd| unsafe { IsWindowVisible(hwnd).as_bool() })
            .unwrap_or(false);
    let worker_available = parent_class.as_deref() == Some("Progman")
        && locate_wallpaper_worker()
            .map(|worker| unsafe { IsWindowVisible(worker).as_bool() })
            .unwrap_or(false);

    let size_matches = parent
        .filter(|_| parent_exists && parent_is_worker)
        .and_then(|worker| {
            let mut parent_rect = RECT::default();
            let mut child_rect = RECT::default();
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
    WebviewWindowBuilder::new(
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
    .map_err(|error| format!("无法重建背景 WebView：{error}"))
}

#[cfg(windows)]
fn recover_wallpaper_host(app: &tauri::AppHandle) -> Result<(), String> {
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
    match &result {
        Ok(()) => {
            if let Some(window) = app.get_webview_window("background") {
                let mode = window
                    .hwnd()
                    .ok()
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
    // A force-quit during the inner workspace must never leave Explorer's
    // desktop layer hidden on the next startup.
    restore_desktop_icons();
    recover_wallpaper_host(&app)?;
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if WALLPAPER_RECOVERY_QUEUED.swap(true, Ordering::AcqRel) {
            continue;
        }
        let recovery_app = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            let (action, _) =
                inspect_wallpaper_host(recovery_app.get_webview_window("background").as_ref());
            if action != WallpaperHostAction::None {
                if let Err(error) = recover_wallpaper_host(&recovery_app) {
                    log::error!("wallpaper host recovery failed: {error}");
                }
            }
            WALLPAPER_RECOVERY_QUEUED.store(false, Ordering::Release);
        }) {
            WALLPAPER_RECOVERY_QUEUED.store(false, Ordering::Release);
            log::error!("unable to schedule wallpaper host recovery: {error}");
        }
    });
    Ok(())
}

#[cfg(windows)]
pub fn register_session_events(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("background")
        .ok_or("background window missing")?;
    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
    let app_ptr = Box::into_raw(Box::new(app.clone())) as usize;
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).map_err(|e| e.to_string())?;
        if !SetWindowSubclass(hwnd, Some(session_subclass_proc), 1, app_ptr).as_bool() {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            drop(Box::from_raw(app_ptr as *mut tauri::AppHandle));
            return Err("无法安装 Windows 会话消息处理器".into());
        }
    }
    let _ = app.emit("system-session", "resume");
    Ok(())
}

#[cfg(windows)]
fn dispatch_system_action(app: &tauri::AppHandle, action: AppAction) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    let _ = app.emit("app-snapshot", &snapshot);
}

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
                dispatch_system_action(app, AppAction::Lock);
                let _ = app.emit("system-session", "locked");
            }
            WTS_SESSION_UNLOCK => {
                dispatch_system_action(app, AppAction::Unlock { play_wake: true });
                let _ = app.emit("system-session", "unlocked");
            }
            _ => {}
        },
        WM_POWERBROADCAST => match wparam.0 as u32 {
            PBT_APMSUSPEND => {
                dispatch_system_action(app, AppAction::Lock);
                let _ = app.emit("system-session", "suspend");
            }
            PBT_APMRESUMEAUTOMATIC => {
                dispatch_system_action(app, AppAction::Unlock { play_wake: true });
                let _ = app.emit("system-session", "resume");
            }
            _ => {}
        },
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
unsafe extern "system" fn interaction_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    root_hwnd_value: usize,
) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            // WebView2 owns its renderer HWNDs from a separate process. A
            // subclass installed on one of those children cannot reliably
            // forward DefSubclassProc through its browser-side window
            // procedure. In the inner workspace Explorer's icon layer is
            // hidden, so the whole wallpaper host is intentionally the input
            // target: returning HTCLIENT here is explicit and does not depend
            // on a child subclass succeeding.
            if INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire) {
                return LRESULT(1); // HTCLIENT
            }
            let packed = lparam.0 as u32;
            let screen_x = (packed as u16 as i16) as i32;
            let screen_y = ((packed >> 16) as u16 as i16) as i32;
            let root_hwnd = if root_hwnd_value == 0 {
                hwnd
            } else {
                HWND(root_hwnd_value as *mut core::ffi::c_void)
            };
            let mut origin = POINT::default();
            if ClientToScreen(root_hwnd, &mut origin).as_bool() {
                // The inner workspace deliberately hides Explorer's icons.
                // Do not rely on fragile WebView child-region coordinates
                // there: the whole desktop surface may receive input so the
                // textarea and controls retain ordinary browser behaviour.
                // In the front workspace the surface remains transparent and
                // Explorer owns icons, selection, and the context menu.
                let local_x = screen_x - origin.x;
                let local_y = screen_y - origin.y;
                let hit = interaction_regions()
                    .read()
                    .map(|state| point_hits_interaction_region(&state.regions, local_x, local_y))
                    .unwrap_or(false);
                if !hit {
                    return LRESULT(HTTRANSPARENT as isize);
                }
            } else {
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(interaction_subclass_proc), subclass_id);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

#[cfg(windows)]
fn install_desktop_hit_testing(root_hwnd: HWND) -> Result<(), String> {
    unsafe {
        if !SetWindowSubclass(
            root_hwnd,
            Some(interaction_subclass_proc),
            2,
            root_hwnd.0 as usize,
        )
        .as_bool()
        {
            return Err("无法安装桌面交互命中测试".into());
        }
        // Do not recursively subclass WebView2's renderer windows. They are
        // owned by a browser helper process, and SetWindowSubclass can fail or
        // leave an unreliable cross-process hook. The root host decides the
        // desktop hit test; normal client routing then delivers input to the
        // renderer.
    }
    Ok(())
}

#[cfg(windows)]
pub fn start_foreground_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let foreground = unsafe { GetForegroundWindow() };
        let foreground_class = window_class(foreground);
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
                let snapshot =
                    core.dispatch(AppAction::DesktopForegroundChanged(desktop_foreground));
                // Foreground changes are informative only.  Re-showing or
                // hiding the WebView here makes the composition flicker and,
                // more importantly, turns an ordinary focus change into an
                // implicit visibility command.  Explicit tray/settings
                // commands and privacy transitions own that responsibility.
                let _ = app.emit("app-snapshot", &snapshot);
            }
        }
        if app.get_webview_window("background").is_none() {
            break;
        }
    });
}

/// Uses UI Automation, rather than ListView messages with a pointer owned by
/// Explorer, to distinguish desktop icons from empty desktop space. This is a
/// supported cross-process accessibility boundary and never consumes input.
#[cfg(windows)]
fn cursor_is_over_desktop_blank(automation: &IUIAutomation) -> bool {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        let Ok(element) = automation.ElementFromPoint(point) else {
            return false;
        };
        // ElementFromPoint can return an icon label/text child rather than the
        // ListItem itself. Check the short parent chain before considering the
        // point blank; no Explorer memory or window messages are involved.
        let walker = automation.ControlViewWalker().ok();
        let mut current = Some(element);
        for _ in 0..4 {
            let Some(element) = current else {
                break;
            };
            if element.CurrentControlType().ok() == Some(UIA_ListItemControlTypeId) {
                return false;
            }
            current = walker
                .as_ref()
                .and_then(|tree| tree.GetParentElement(&element).ok());
        }
        true
    }
}

#[cfg(windows)]
pub fn start_desktop_workspace_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        // UIA requires COM initialization on the monitor thread. A prior COM
        // mode is harmless: UIA can still be created on that thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let automation = unsafe {
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        };
        let Ok(automation) = automation else {
            log::warn!("无法初始化 Windows UI Automation；表/里桌面双击切换暂不可用");
            return;
        };
        log::info!("表/里桌面双击监控已启动（UI Automation）");

        let mut was_down = false;
        let mut last_blank_click: Option<std::time::Instant> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(16));
            if app.get_webview_window("background").is_none() {
                break;
            }
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            if down && !was_down {
                let foreground = unsafe { GetForegroundWindow() };
                let on_desktop = foreground == unsafe { GetDesktopWindow() }
                    || is_desktop_foreground_class(window_class(foreground).as_deref());
                if on_desktop && cursor_is_over_desktop_blank(&automation) {
                    let now = std::time::Instant::now();
                    if last_blank_click.is_some_and(|previous| {
                        now.duration_since(previous) <= std::time::Duration::from_millis(500)
                    }) {
                        last_blank_click = None;
                        let entering = !INNER_WORKSPACE_ACTIVE.fetch_xor(true, Ordering::AcqRel);
                        if let Err(error) = set_desktop_icons_visible(!entering) {
                            // If Explorer has restarted or the icon view cannot
                            // be found, preserve a truthful state and do not
                            // enter a half-working inner desktop.
                            INNER_WORKSPACE_ACTIVE.store(false, Ordering::Release);
                            log::warn!("无法切换表/里桌面图标层：{error}");
                            continue;
                        }
                        log::info!(
                            "桌面空白双击：切换至{}桌面",
                            if entering { "里" } else { "表" }
                        );
                        let _ = app.emit(
                            "desktop-workspace-toggle",
                            if entering { "enter" } else { "leave" },
                        );
                    } else {
                        last_blank_click = Some(now);
                    }
                } else {
                    last_blank_click = None;
                }
            }
            was_down = down;
        }
    });
}

#[cfg(windows)]
pub async fn set_lock_screen(app: &tauri::AppHandle, enabled: bool) -> Result<String, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    if enabled {
        let has_package_identity = has_package_identity()?;
        if !can_attempt_lock_screen_takeover(has_package_identity) {
            return Err("当前是无 MSIX 包身份的正式桌面版。为确保锁屏接管可验证且可恢复，请安装 MSIX 包后再启用；NSIS 版不会修改锁屏。".into());
        }
        if !UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())? {
            return Err("当前 Windows 策略不允许应用修改锁屏图片".into());
        }
        let original = LockScreen::OriginalImageFile()
            .map_err(|_| "无法读取当前锁屏图片；为避免无法恢复，已取消接管。".to_string())?
            .AbsoluteUri()
            .map_err(|_| "无法读取当前锁屏图片；为避免无法恢复，已取消接管。".to_string())?
            .to_string();
        let managed_path =
            lock_screen_backup::asset_directory(&config_dir).join("dsh-wallpaper-sleep.png");
        let backup_state = inspect_backup(&config_dir);
        let managed_image_active = managed_image_is_active(Some(&original), &managed_path);
        if has_stale_backup(&backup_state, managed_image_active) {
            return Err("检测到接管期间锁屏已由用户或其他程序更改。为避免覆盖当前锁屏，应用不会再次接管；原备份已保留。请先在 Windows 设置中确认锁屏图片，再决定是否清理或恢复。".into());
        }
        if managed_image_active && matches!(backup_state, LockScreenBackupState::Missing) {
            return Err("当前锁屏已经是本应用的熟睡画面，但原锁屏备份不存在。为避免把托管图片误当作原图，已拒绝再次接管；请先在 Windows 设置中手动选择原图。".into());
        }
        // Prepare the application-owned managed file before creating a backup.
        // A missing bundle resource or failed copy must not leave a valid
        // restore manifest behind while the user's original image is still
        // active (which would later look like a stale takeover).
        let bundled_sleep_image = bundled_sleep_resource_candidates()
            .into_iter()
            .find_map(|resource| {
                app.path()
                    .resolve(resource, tauri::path::BaseDirectory::Resource)
                    .ok()
                    .filter(|path| path.is_file())
            })
            // `tauri dev` does not copy bundle resources next to target/debug.
            // The source public directory remains the canonical development
            // resource location, while packaged builds take the branch above.
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| {
                        cwd.join("public")
                            .join("personas/wake-frames/variant-anima/sleep.png")
                    })
                    .filter(|path| path.is_file())
            })
            .or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map(|root| {
                        root.join("public")
                            .join("personas/wake-frames/variant-anima/sleep.png")
                    })
                    .filter(|path| path.is_file())
            })
            .ok_or_else(|| "未找到内置的锁屏睡眠图片。请重新安装 dsh-wallpaper。".to_string())?;

        // WinRT's lock-screen API is unreliable with paths inside a Tauri
        // resource bundle (especially during `tauri dev`): it can receive a
        // virtual/masked resource path and return 0x800700A1.  Hand Windows a
        // normal, current-user-owned file instead.
        let managed_dir = lock_screen_backup::asset_directory(&config_dir);
        std::fs::create_dir_all(&managed_dir)
            .map_err(|_| "无法创建锁屏图片存放目录".to_string())?;
        let managed_path = managed_dir.join("dsh-wallpaper-sleep.png");
        std::fs::copy(&bundled_sleep_image, &managed_path)
            .map_err(|_| "无法准备锁屏图片。请检查应用安装目录是否完整。".to_string())?;
        let captured_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "系统时间无效，已取消锁屏接管。".to_string())?
            .as_millis() as u64;
        let lease = ensure_backup_for_takeover(&config_dir, &original, captured_at)?;
        // A user can change their lock screen while the resource copy and
        // snapshot above are running.  Re-read immediately before the only
        // setter and refuse to overwrite a newer choice.  This cannot remove
        // the unavoidable kernel-level race, but it makes the application
        // itself fail closed across its full preflight transaction.
        let current_before_set = LockScreen::OriginalImageFile()
            .ok()
            .and_then(|uri| uri.AbsoluteUri().ok())
            .map(|uri| uri.to_string());
        let Some(current_before_set) = current_before_set else {
            return abort_lock_screen_takeover_before_set(
                &config_dir,
                &lease,
                "Windows 未能在写入前重新读取锁屏状态；应用没有修改锁屏，已取消接管。",
            );
        };
        if !same_local_file_uri(&original, &current_before_set) {
            return abort_lock_screen_takeover_before_set(
                &config_dir,
                &lease,
                "检测到锁屏图片在接管准备期间已被用户或其他程序更改；应用没有覆盖新图片。",
            );
        }
        let set_result = (|| -> Result<(), String> {
            let path_string = HSTRING::from(managed_path.to_string_lossy().as_ref());
            let file = StorageFile::GetFileFromPathAsync(&path_string)
                .map_err(|_| "Windows 无法读取准备好的锁屏图片。".to_string())?
                .get()
                .map_err(|_| "Windows 无法打开准备好的锁屏图片。".to_string())?;
            let settings = UserProfilePersonalizationSettings::Current()
                .map_err(|_| "Windows 无法打开锁屏个性化设置。".to_string())?;
            let changed = settings
                .TrySetLockScreenImageAsync(&file)
                .map_err(|_| {
                    "Windows 拒绝设置锁屏图片（可能被组织策略或 Spotlight 管理）。".to_string()
                })?
                .get()
                .map_err(|_| "Windows 未能完成锁屏图片设置。".to_string())?;
            if !changed {
                // Some Windows 11 editions deny TrySet… to unpackaged desktop
                // apps even with no policy configured. The older LockScreen API
                // is the compatible path for a local static PNG.
                LockScreen::SetImageFileAsync(&file)
                    .map_err(|_| "Windows 拒绝设置锁屏图片。请检查系统策略，或在 Windows 设置中手动选择该图片。".to_string())?
                    .get()
                    .map_err(|_| "Windows 拒绝设置锁屏图片。请检查系统策略，或在 Windows 设置中手动选择该图片。".to_string())?;
            }
            Ok(())
        })();
        finish_lock_screen_takeover_attempt(&config_dir, &lease, &managed_path, set_result)
    } else {
        let managed_path =
            lock_screen_backup::asset_directory(&config_dir).join("dsh-wallpaper-sleep.png");
        let current_image_uri = LockScreen::OriginalImageFile()
            .ok()
            .and_then(|uri| uri.AbsoluteUri().ok())
            .map(|uri| uri.to_string());
        let backup_state = inspect_backup(&config_dir);
        let managed_image_active =
            managed_image_is_active(current_image_uri.as_deref(), &managed_path);
        if has_stale_backup(&backup_state, managed_image_active) {
            return Ok("已停止本应用的锁屏接管状态。检测到当前锁屏已由用户或其他程序更改，因此未覆盖它；原备份已保留。".into());
        }
        if let LockScreenBackupState::Valid(manifest) = backup_state {
            if let Ok(path) = restore_snapshot_path(&config_dir, &manifest) {
                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(
                    path.to_string_lossy().as_ref(),
                ))
                .map_err(|e| e.to_string())?
                .get()
                .map_err(|e| e.to_string())?;
                let settings =
                    UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
                let restored = settings
                    .TrySetLockScreenImageAsync(&file)
                    .map_err(|e| e.to_string())?
                    .get()
                    .map_err(|e| e.to_string())?;
                if restored {
                    return finish_verified_lock_screen_restore(&config_dir, &manifest, &path);
                }
                // Match the enable path: unpackaged desktop processes on
                // some Windows 11 builds report false from TrySet… while the
                // compatibility LockScreen API succeeds for the same file.
                if LockScreen::SetImageFileAsync(&file)
                    .is_ok_and(|operation| operation.get().is_ok())
                {
                    return finish_verified_lock_screen_restore(&config_dir, &manifest, &path);
                }
            }
        }
        Err("未能恢复原静态锁屏图片；接管仍保持启用，原备份没有被删除。请稍后重试或在 Windows 设置中手动恢复。".into())
    }
}

#[cfg(windows)]
fn finish_lock_screen_takeover_attempt(
    config_dir: &std::path::Path,
    lease: &LockScreenBackupLease,
    managed_image: &std::path::Path,
    set_result: Result<(), String>,
) -> Result<String, String> {
    // Never discard a newly captured restore point merely because the
    // verification query failed. The setter can succeed even when a later
    // OriginalImageFile read is unavailable; in that indeterminate case the
    // backup is the only safe recovery path.
    let current_is_managed = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| managed_image_is_active(Some(&uri.to_string()), managed_image));

    match (set_result, current_is_managed) {
        // Even when an API reports an error, the authoritative ownership check
        // says Windows is using our image.  Keep the recovery point.
        (_, Some(true)) => Ok("锁屏图片已设置；密码页继续由 Windows 原生模糊处理。".into()),
        // A successful setter plus an ambiguous or mismatched later read is
        // not proof that the setter failed.  Query propagation can lag the
        // completed WinRT operation, and Windows can normalize a path in ways
        // we do not recognize.  Retaining the only original-image snapshot is
        // safer than trying to make the transaction look clean.
        (Ok(()), Some(false)) => Err("Windows 未确认锁屏已切换为本应用的熟睡画面；恢复点已保留，应用没有删除任何原图备份。请刷新检查后再决定是否恢复或重试。".into()),
        (Ok(()), None) => Err("Windows 已完成锁屏设置请求，但无法读取最终状态；恢复点已保留，应用没有删除任何原图备份。请刷新检查后再决定是否恢复或重试。".into()),
        // Only an explicit setter failure combined with a positive read of a
        // non-managed image can roll back a backup created in *this* attempt.
        // It never deletes a recovery point inherited from an earlier run.
        (Err(error), Some(false)) => abort_lock_screen_takeover_before_set(config_dir, lease, &error),
        (Err(error), None) => Err(format!(
            "{error} 同时无法确认锁屏最终状态；恢复点已保留，应用没有删除任何原图备份。"
        )),
    }
}

#[cfg(windows)]
fn abort_lock_screen_takeover_before_set(
    config_dir: &std::path::Path,
    lease: &LockScreenBackupLease,
    reason: &str,
) -> Result<String, String> {
    match discard_backup_after_failed_takeover(config_dir, lease) {
        Ok(()) => Err(reason.into()),
        Err(_) => Err(format!(
            "{reason} 本次接管创建的恢复点未能清理，已保留以避免丢失原图。"
        )),
    }
}

#[cfg(windows)]
fn finish_verified_lock_screen_restore(
    config_dir: &std::path::Path,
    manifest: &LockScreenBackupManifest,
    expected_image: &std::path::Path,
) -> Result<String, String> {
    let current_uri = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| uri.to_string());
    if !managed_image_is_active(current_uri.as_deref(), expected_image) {
        return Err("Windows 未确认原锁屏图片已恢复；接管备份已保留，请稍后重试或在 Windows 设置中手动恢复。".into());
    }
    remove_backup_after_restore(config_dir, manifest)?;
    Ok("已恢复接管前的静态锁屏图片。".into())
}

/// Returns whether this process has a Windows package identity. Windows
/// documents `APPMODEL_ERROR_NO_PACKAGE` as the explicit unpackaged result;
/// the length probe is sufficient, so no package name is retained or exposed.
#[cfg(windows)]
fn has_package_identity() -> Result<bool, String> {
    let mut length = 0u32;
    let status = unsafe { GetCurrentPackageFullName(&mut length, None) };
    if status == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(false);
    }
    if status == ERROR_INSUFFICIENT_BUFFER || status.is_ok() {
        return Ok(true);
    }
    Err(format!(
        "无法判断当前应用的 MSIX 包身份（Windows 错误码 {}）；为避免错误接管，已取消操作。",
        status.0
    ))
}

#[cfg(windows)]
fn can_attempt_lock_screen_takeover(has_package_identity: bool) -> bool {
    has_package_identity
}

#[cfg(windows)]
fn bundled_sleep_resource_candidates() -> [&'static str; 2] {
    [
        "_up_/public/personas/wake-frames/variant-anima/sleep.png",
        // Compatibility for older test bundles assembled before the current
        // Tauri resource mapping was documented and verified.
        "personas/wake-frames/variant-anima/sleep.png",
    ]
}

#[cfg(windows)]
fn remove_backup_after_restore(
    config_dir: &std::path::Path,
    manifest: &LockScreenBackupManifest,
) -> Result<(), String> {
    remove_backup_after_verified_restore(config_dir, manifest)
}

/// Read-only preflight for lock-screen ownership. It never calls a WinRT setter.
#[cfg(windows)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScreenDiagnostics {
    pub supported: bool,
    pub package_identity: bool,
    pub takeover_available: bool,
    pub original_image_uri: Option<String>,
    pub backup_exists: bool,
    pub backup_valid: bool,
    pub stale_backup: bool,
    pub managed_image_ready: bool,
    pub managed_image_active: bool,
    pub development_build: bool,
    pub warnings: Vec<String>,
}

#[cfg(windows)]
pub fn lock_screen_diagnostics(app: &tauri::AppHandle) -> Result<LockScreenDiagnostics, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let managed_image =
        lock_screen_backup::asset_directory(&config_dir).join("dsh-wallpaper-sleep.png");
    let original_image_uri = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| uri.to_string());
    let backup_state = inspect_backup(&config_dir);
    let backup_exists = !matches!(backup_state, LockScreenBackupState::Missing);
    let backup_valid = matches!(backup_state, LockScreenBackupState::Valid(_));
    let managed_image_active =
        managed_image_is_active(original_image_uri.as_deref(), &managed_image);
    let stale_backup = has_stale_backup(&backup_state, managed_image_active);
    let supported = UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())?;
    let package_identity = has_package_identity()?;
    let takeover_available = supported && can_attempt_lock_screen_takeover(package_identity);
    let mut warnings = Vec::new();
    if !supported {
        warnings.push("当前 Windows 策略不允许应用修改锁屏图片。".into());
    }
    if backup_exists && !backup_valid {
        warnings.push("已发现不完整的锁屏备份；应用会拒绝新的接管，以防覆盖唯一的恢复点。".into());
    }
    if stale_backup {
        warnings.push("检测到原锁屏备份，但当前锁屏已由用户或其他程序更改；应用不会恢复或再次接管，以免覆盖当前图片。备份已保留。".into());
    }
    if original_image_uri
        .as_deref()
        .is_none_or(|uri| !uri.starts_with("file:///"))
    {
        warnings.push("当前锁屏可能由 Windows Spotlight 或其他动态来源管理，Windows API 无法可靠还原该动态状态。".into());
    }
    if !package_identity {
        warnings.push("当前进程没有 MSIX 包身份；正式桌面版不会接管锁屏。请安装 MSIX 包。".into());
    }
    Ok(LockScreenDiagnostics {
        supported,
        package_identity,
        takeover_available,
        original_image_uri,
        backup_exists,
        backup_valid,
        stale_backup,
        managed_image_ready: managed_image.is_file(),
        managed_image_active,
        development_build: cfg!(debug_assertions),
        warnings,
    })
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
pub fn register_session_events(_: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub async fn set_lock_screen(_: &tauri::AppHandle, _: bool) -> Result<String, String> {
    Err("Lock screen integration only supports Windows".into())
}
#[cfg(not(windows))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScreenDiagnostics {
    pub supported: bool,
    pub package_identity: bool,
    pub takeover_available: bool,
    pub original_image_uri: Option<String>,
    pub backup_exists: bool,
    pub backup_valid: bool,
    pub stale_backup: bool,
    pub managed_image_ready: bool,
    pub managed_image_active: bool,
    pub development_build: bool,
    pub warnings: Vec<String>,
}
#[cfg(not(windows))]
pub fn lock_screen_diagnostics(_: &tauri::AppHandle) -> Result<LockScreenDiagnostics, String> {
    Ok(LockScreenDiagnostics {
        supported: false,
        package_identity: false,
        takeover_available: false,
        original_image_uri: None,
        backup_exists: false,
        backup_valid: false,
        stale_backup: false,
        managed_image_ready: false,
        managed_image_active: false,
        development_build: false,
        warnings: vec!["锁屏接管仅支持 Windows。".into()],
    })
}
#[cfg(not(windows))]
pub fn start_foreground_monitor(_: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_builds_are_always_eligible_for_lock_screen_takeover() {
        assert!(can_attempt_lock_screen_takeover(true));
    }

    #[test]
    fn unpackaged_lock_screen_takeover_is_never_eligible() {
        assert!(!can_attempt_lock_screen_takeover(false));
    }

    #[test]
    fn packaged_sleep_resource_uses_tauris_verified_windows_path_first() {
        assert_eq!(
            bundled_sleep_resource_candidates()[0],
            "_up_/public/personas/wake-frames/variant-anima/sleep.png"
        );
    }

    static REGION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    #[test]
    fn scales_logical_interaction_regions_outward_to_physical_pixels() {
        assert_eq!(
            scale_interaction_region(
                InteractionRegionInput {
                    x: 10.25,
                    y: 20.5,
                    width: 100.1,
                    height: 40.2,
                },
                1.5,
            ),
            Some(PhysicalInteractionRegion {
                left: 15,
                top: 30,
                right: 166,
                bottom: 92,
            })
        );
    }

    #[test]
    fn hit_test_uses_half_open_rectangles() {
        let region = PhysicalInteractionRegion {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40,
        };
        assert!(point_hits_interaction_region(&[region], 10, 20));
        assert!(point_hits_interaction_region(&[region], 29, 39));
        assert!(!point_hits_interaction_region(&[region], 30, 39));
        assert!(!point_hits_interaction_region(&[region], 29, 40));
    }

    #[test]
    fn empty_regions_make_the_entire_overlay_transparent() {
        assert!(!point_hits_interaction_region(&[], 100, 100));
    }

    #[test]
    fn rejects_invalid_regions_and_scale_factors() {
        assert!(scale_interaction_region(
            InteractionRegionInput {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 10.0,
            },
            1.0,
        )
        .is_none());
        assert!(scale_interaction_region(
            InteractionRegionInput {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            f64::NAN,
        )
        .is_none());
    }

    #[test]
    fn stale_region_updates_cannot_replace_newer_geometry() {
        let _guard = REGION_TEST_LOCK.lock().expect("region test lock");
        let session = begin_interaction_region_session().expect("region session");
        let current_revision = interaction_regions()
            .read()
            .expect("interaction region state")
            .revision;
        let newer = current_revision.saturating_add(100);
        let first = update_interaction_regions(
            vec![InteractionRegionInput {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            }],
            1.0,
            session,
            newer,
        )
        .expect("new region update");
        assert!(!first.stale);
        let stale = update_interaction_regions(Vec::new(), 1.0, session, newer - 1)
            .expect("stale region update");
        assert!(stale.stale);
        assert_eq!(stale.revision, newer);
        assert_eq!(stale.region_count, 1);
    }

    #[test]
    fn previous_region_session_cannot_clear_the_current_surface() {
        let _guard = REGION_TEST_LOCK.lock().expect("region test lock");
        let previous = begin_interaction_region_session().expect("previous region session");
        let current = begin_interaction_region_session().expect("current region session");
        let accepted = update_interaction_regions(
            vec![InteractionRegionInput {
                x: 5.0,
                y: 6.0,
                width: 30.0,
                height: 40.0,
            }],
            1.0,
            current,
            1,
        )
        .expect("current region update");
        assert!(!accepted.stale);
        let stale_cleanup = update_interaction_regions(Vec::new(), 1.0, previous, u64::MAX)
            .expect("stale session cleanup");
        assert!(stale_cleanup.stale);
        assert_eq!(stale_cleanup.region_count, 1);
    }
}
