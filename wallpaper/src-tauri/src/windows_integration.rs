use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use crate::app_core::{AppAction, AppCore, WallpaperHostMode, WallpaperHostStatus};

#[cfg(windows)]
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[cfg(windows)]
use windows::{
    core::{BOOL, HSTRING, PCWSTR},
    Storage::StorageFile,
    System::UserProfile::{LockScreen, UserProfilePersonalizationSettings},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
            },
            Gdi::{ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW,
                MonitorFromWindow, SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, RGN_OR},
        },
        System::{
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
            Threading::GetCurrentProcessId,
        },
        UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA},
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetClientRect,
            GetDesktopWindow, GetForegroundWindow, GetParent, GetWindowLongPtrW, GetWindowRect,
            GetWindowThreadProcessId, IsWindow, IsWindowVisible, SendMessageTimeoutW, SetForegroundWindow, SetParent, SetWindowLongPtrW,
            SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SEND_MESSAGE_TIMEOUT_FLAGS, SMTO_NORMAL,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
            HWND_NOTOPMOST, HWND_TOPMOST, SW_SHOWNA, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_NCHITTEST,
            WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_WTSSESSION_CHANGE, WS_BORDER, WS_CAPTION,
            WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
            WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_NOACTIVATE,
            WS_EX_STATICEDGE, WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
    },
};

#[cfg(windows)]
const PROGMAN_SPAWN_WORKERW: u32 = 0x052C;

#[cfg(windows)]
static WALLPAPER_RECOVERY_QUEUED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static INTERACTION_BOUNDS_SYNC_QUEUED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static INTERACTION_REVEAL_GRACE: OnceLock<RwLock<Option<std::time::Instant>>> = OnceLock::new();

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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarGeometry {
    pub edge: String,
    pub bounds: Option<GeometryRect>,
    pub auto_hide: bool,
    pub visible: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopGeometry {
    pub monitor_id: String,
    pub monitor_bounds: GeometryRect,
    pub work_area: GeometryRect,
    pub scale_factor: f64,
    pub taskbar: TaskbarGeometry,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionPlacement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

static INTERACTION_REGIONS: OnceLock<RwLock<InteractionRegionState>> = OnceLock::new();

#[cfg(windows)]
static DESKTOP_FOREGROUND_STATE: OnceLock<RwLock<Option<bool>>> = OnceLock::new();

#[cfg(windows)]
static WALLPAPER_HOST_STATUS: OnceLock<RwLock<WallpaperHostStatus>> = OnceLock::new();

#[cfg(windows)]
static GEOMETRY_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn interaction_regions() -> &'static RwLock<InteractionRegionState> {
    INTERACTION_REGIONS.get_or_init(|| RwLock::new(InteractionRegionState::default()))
}

fn geometry_rect(rect: RECT) -> GeometryRect {
    GeometryRect {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    }
}

#[cfg(windows)]
pub fn desktop_geometry(window: &WebviewWindow) -> Result<DesktopGeometry, String> {
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("无法读取显示器工作区".into());
        }

        let mut appbar = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };
        let has_taskbar = SHAppBarMessage(ABM_GETTASKBARPOS, &mut appbar) != 0;
        let auto_hide = SHAppBarMessage(ABM_GETSTATE, &mut appbar) & ABS_AUTOHIDE as usize != 0;
        let taskbar_bounds = has_taskbar.then(|| geometry_rect(appbar.rc));
        let edge = if !has_taskbar {
            "unknown"
        } else if appbar.rc.top <= info.rcMonitor.top && appbar.rc.bottom < info.rcMonitor.bottom {
            "top"
        } else if appbar.rc.bottom >= info.rcMonitor.bottom && appbar.rc.top > info.rcMonitor.top {
            "bottom"
        } else if appbar.rc.left <= info.rcMonitor.left {
            "left"
        } else {
            "right"
        };
        Ok(DesktopGeometry {
            monitor_id: format!("monitor-{:X}", monitor.0 as usize),
            monitor_bounds: geometry_rect(info.rcMonitor),
            work_area: geometry_rect(info.rcWork),
            scale_factor: window.scale_factor().unwrap_or(1.0),
            taskbar: TaskbarGeometry {
                edge: edge.into(),
                bounds: taskbar_bounds,
                auto_hide,
                visible: has_taskbar && !auto_hide,
            },
            revision: GEOMETRY_REVISION.fetch_add(1, Ordering::Relaxed) + 1,
        })
    }
}

#[cfg(not(windows))]
pub fn desktop_geometry(_: &tauri::WebviewWindow) -> Result<DesktopGeometry, String> {
    Err("desktop geometry only supports Windows".into())
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
    if revision < state.revision {
        return Ok(InteractionRegionUpdateResult {
            revision: state.revision,
            region_count: state.regions.len(),
            stale: true,
        });
    }
    state.revision = revision;
    state.scale_factor = scale_factor;
    state.regions = physical_regions;
    let regions_for_window = state.regions.clone();
    drop(state);
    #[cfg(windows)]
    apply_interaction_window_region(&regions_for_window)?;
    Ok(InteractionRegionUpdateResult {
        revision,
        region_count: regions_for_window.len(),
        stale: false,
    })
}

#[cfg(windows)]
fn apply_interaction_window_region(regions: &[PhysicalInteractionRegion]) -> Result<(), String> {
    let Some(hwnd) = interaction_window_hwnd() else {
        return Ok(());
    };
    unsafe {
        let combined = CreateRectRgn(0, 0, 0, 0);
        if combined.is_invalid() {
            return Err("无法创建交互窗口区域".into());
        }
        for region in regions {
            let part = CreateRectRgn(region.left, region.top, region.right, region.bottom);
            if !part.is_invalid() {
                let _ = CombineRgn(Some(combined), Some(combined), Some(part), RGN_OR);
                let _ = DeleteObject(part.into());
            }
        }
        if SetWindowRgn(hwnd, Some(combined), true) == 0 {
            let _ = DeleteObject(combined.into());
            return Err("无法更新交互窗口区域".into());
        }
        // On success ownership of the region belongs to Windows.
    }
    Ok(())
}

#[cfg(windows)]
static INTERACTION_WINDOW_HWND: OnceLock<RwLock<Option<isize>>> = OnceLock::new();

#[cfg(windows)]
fn interaction_window_hwnd() -> Option<HWND> {
    INTERACTION_WINDOW_HWND
        .get_or_init(|| RwLock::new(None))
        .read()
        .ok()
        .and_then(|value| *value)
        .map(|value| HWND(value as *mut core::ffi::c_void))
}

#[cfg(windows)]
fn remember_interaction_window(hwnd: HWND) {
    if let Ok(mut value) = INTERACTION_WINDOW_HWND
        .get_or_init(|| RwLock::new(None))
        .write()
    {
        *value = Some(hwnd.0 as isize);
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
    attach_hwnd_to_workerw(background).map(|_| ())
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
    let worker_available =
        parent_class.as_deref() == Some("Progman")
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
    if snapshot.interaction.visible {
        let _ = show_interaction(app, false);
    } else {
        hide_interaction(app);
    }
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
                hide_interaction(app);
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
                hide_interaction(app);
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
                let local_x = screen_x - origin.x;
                let local_y = screen_y - origin.y;
                let hit = interaction_regions()
                    .read()
                    .map(|state| point_hits_interaction_region(&state.regions, local_x, local_y))
                    .unwrap_or(false);
                // Progman fallback cannot use HTTRANSPARENT: Windows may skip
                // Explorer's sibling icon view and hit an application behind
                // the desktop. Mouse messages outside hot regions are instead
                // forwarded explicitly to the native desktop list view above.
                let _ = hit;
            } else {
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            // 这些消息可能在 Explorer 仍在更新布局时到达，由后台健康检查
            // 在下一轮主线程同步 Overlay 尺寸，避免在窗口过程中重入。
            INTERACTION_BOUNDS_SYNC_QUEUED.store(true, Ordering::Release);
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(interaction_subclass_proc), subclass_id);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

#[cfg(windows)]
unsafe extern "system" fn install_interaction_subclass_on_child(
    child: HWND,
    root_hwnd: LPARAM,
) -> BOOL {
    // WebView2 creates several nested Chrome HWNDs. Mouse messages land on the
    // deepest render widget, so subclassing only the outer Tauri HWND cannot
    // make the unused desktop area transparent.
    let _ = SetWindowSubclass(
        child,
        Some(interaction_subclass_proc),
        2,
        root_hwnd.0 as usize,
    );
    BOOL(1)
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
        let _ = EnumChildWindows(
            Some(root_hwnd),
            Some(install_interaction_subclass_on_child),
            LPARAM(root_hwnd.0 as isize),
        );
    }
    Ok(())
}

#[cfg(windows)]
pub fn configure_desktop_interaction(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    remember_interaction_window(hwnd);
    unsafe {
        let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, exstyle | WS_EX_TOOLWINDOW.0 as isize);
    }
    size_interaction_to_desktop(hwnd)?;
    let regions = interaction_regions()
        .read()
        .map(|state| state.regions.clone())
        .unwrap_or_default();
    apply_interaction_window_region(&regions)
}

#[cfg(windows)]
pub fn notify_desktop_geometry_changed(app: &tauri::AppHandle) {
    let _ = app.emit("desktop-geometry-changed", ());
}

#[cfg(windows)]
fn size_interaction_to_desktop(hwnd: HWND) -> Result<(), String> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("无法读取交互窗口工作区".into());
        }
        let bounds = info.rcWork;
        SetWindowPos(
            hwnd,
            None,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn apply_interaction_placement(
    window: &WebviewWindow,
    requested: InteractionPlacement,
) -> Result<InteractionPlacement, String> {
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("无法读取交互窗口工作区".into());
        }
        let work_width = info.rcWork.right - info.rcWork.left;
        let work_height = info.rcWork.bottom - info.rcWork.top;
        let min_width = 48.min(work_width.max(1));
        let min_height = 40.min(work_height.max(1));
        let max_width = ((work_width as f64 * 0.9).round() as i32).max(min_width);
        let max_height = ((work_height as f64 * 0.9).round() as i32).max(min_height);
        let width = requested.width.clamp(min_width, max_width);
        let height = requested.height.clamp(min_height, max_height);
        let x = requested.x.clamp(0, (work_width - width).max(0));
        let y = requested.y.clamp(0, (work_height - height).max(0));
        SetWindowPos(
            hwnd,
            None,
            info.rcWork.left + x,
            info.rcWork.top + y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
        )
        .map_err(|error| error.to_string())?;
        // SetWindowRgn is expressed in window-local coordinates, but Windows
        // offsets an existing region when this top-level window is moved from
        // its former full-work-area position. Clear it before the WebView
        // publishes fresh local component rectangles for the new viewport.
        let _ = SetWindowRgn(hwnd, None, true);
        Ok(InteractionPlacement { x, y, width, height })
    }
}

#[cfg(not(windows))]
pub fn apply_interaction_placement(
    _: &WebviewWindow,
    _: InteractionPlacement,
) -> Result<InteractionPlacement, String> {
    Err("interaction placement only supports Windows".into())
}

#[cfg(windows)]
pub fn show_interaction(app: &tauri::AppHandle, request_focus: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("interaction")
        .ok_or("interaction window missing")?;
    window.show().map_err(|e| e.to_string())?;
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    unsafe {
        // Win+D and Explorer's WorkerW reshuffle can leave a WS_EX_TOOLWINDOW
        // below the desktop icon host even while IsWindowVisible is true.
        // Pulse through TOPMOST and immediately return to a normal top-level
        // window: this raises it above Explorer without making it permanently
        // float over applications.
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
        .map_err(|error| error.to_string())?;
        SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
        .map_err(|error| error.to_string())?;
    }
    if request_focus {
        if let Ok(mut deadline) = INTERACTION_REVEAL_GRACE
            .get_or_init(|| RwLock::new(None))
            .write()
        {
            *deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(900));
        }
        // A tray command runs while Shell_TrayWnd owns the foreground. Calling
        // the Win32 foreground API from that input callback transfers the
        // activation to our compact interaction HWND before the monitor can
        // interpret the taskbar as an unrelated application.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            if !SetForegroundWindow(hwnd).as_bool() {
                window.set_focus().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn hide_interaction(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("interaction") {
        let _ = window.hide();
    }
}

#[cfg(windows)]
pub fn start_foreground_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || {
      loop {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let foreground = unsafe { GetForegroundWindow() };
        let mut process_id = 0u32;
        if !foreground.0.is_null() {
            unsafe {
                GetWindowThreadProcessId(foreground, Some(&mut process_id));
            }
        }
        let ours = process_id == unsafe { GetCurrentProcessId() };
        let foreground_class = window_class(foreground);
        let shell = foreground == unsafe { GetDesktopWindow() }
            || is_desktop_foreground_class(foreground_class.as_deref());
        let reveal_grace = INTERACTION_REVEAL_GRACE
            .get_or_init(|| RwLock::new(None))
            .read()
            .ok()
            .and_then(|deadline| *deadline)
            .is_some_and(|deadline| std::time::Instant::now() <= deadline);
        let tray_transition = reveal_grace
            && matches!(foreground_class.as_deref(), Some("Shell_TrayWnd") | Some("NotifyIconOverflowWindow"));
        let desktop_foreground = shell
            || tray_transition
            || (ours
                && app
                    .get_webview_window("interaction")
                    .and_then(|window| window.hwnd().ok())
                    .map(|hwnd| HWND(hwnd.0) == foreground)
                    .unwrap_or(false));
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
                if snapshot.interaction.visible {
                    let _ = show_interaction(&app, false);
                } else {
                    hide_interaction(&app);
                }
                let _ = app.emit("app-snapshot", &snapshot);
            }
        } else if !desktop_foreground {
            hide_interaction(&app);
        }

        if INTERACTION_BOUNDS_SYNC_QUEUED.swap(false, Ordering::AcqRel) {
            notify_desktop_geometry_changed(&app);
        }
        if app.get_webview_window("interaction").is_none() {
            break;
        }
      }
    });
}

#[cfg(windows)]
pub async fn set_lock_screen(app: &tauri::AppHandle, enabled: bool) -> Result<String, String> {
    let backup_file = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("lock-screen-backup.txt");
    if enabled {
        if !UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())? {
            return Err("当前 Windows 策略不允许应用修改锁屏图片".into());
        }
        if !backup_file.exists() {
            if let Ok(uri) = LockScreen::OriginalImageFile() {
                if let Ok(original) = uri.AbsoluteUri() {
                    if let Some(parent) = backup_file.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&backup_file, original.to_string())
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        let path = app
            .path()
            .resolve(
                "personas/wake-frames/variant-anima/sleep.png",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| e.to_string())?;
        let path_string = HSTRING::from(path.to_string_lossy().as_ref());
        let file = StorageFile::GetFileFromPathAsync(&path_string)
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        let settings = UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
        let changed = settings
            .TrySetLockScreenImageAsync(&file)
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        if !changed {
            return Err("Windows 或组织策略拒绝修改锁屏图片".into());
        }
        Ok("锁屏图片已设置；密码页继续由 Windows 原生模糊处理。".into())
    } else {
        let original = std::fs::read_to_string(&backup_file)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if let Some(original) = original {
            let path = original
                .strip_prefix("file:///")
                .or_else(|| original.strip_prefix("file://"))
                .unwrap_or(&original)
                .replace('/', "\\");
            if std::path::Path::new(&path).exists() {
                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))
                    .map_err(|e| e.to_string())?
                    .get()
                    .map_err(|e| e.to_string())?;
                let settings =
                    UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
                if settings
                    .TrySetLockScreenImageAsync(&file)
                    .map_err(|e| e.to_string())?
                    .get()
                    .map_err(|e| e.to_string())?
                {
                    let _ = std::fs::remove_file(&backup_file);
                    return Ok("已恢复接管前的静态锁屏图片。".into());
                }
            }
        }
        Ok("已停止接管后续锁屏图片；未能恢复原静态图片。Spotlight 状态无法由 Windows API 可靠备份和恢复。".into())
    }
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
pub fn configure_desktop_interaction(_: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn show_interaction(_: &tauri::AppHandle, _: bool) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn hide_interaction(_: &tauri::AppHandle) {}
#[cfg(not(windows))]
pub fn start_foreground_monitor(_: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

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
            newer,
        )
        .expect("new region update");
        assert!(!first.stale);
        let stale =
            update_interaction_regions(Vec::new(), 1.0, newer - 1).expect("stale region update");
        assert!(stale.stale);
        assert_eq!(stale.revision, newer);
        assert_eq!(stale.region_count, 1);
    }
}
