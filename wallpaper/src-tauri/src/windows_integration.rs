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
                DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_COLOR_NONE,
                DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
            },
            Gdi::{
                ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW,
                MonitorFromWindow, SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, RGN_OR,
            },
        },
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
            Threading::GetCurrentProcessId,
        },
        UI::{
            Accessibility::{CUIAutomation, IUIAutomation, UIA_ListItemControlTypeId},
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        },
        UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA},
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetCursorPos,
            GetClientRect, GetDesktopWindow, GetForegroundWindow, GetParent, GetWindowLongPtrW,
            GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
            SendMessageTimeoutW, SetParent, SetWindowLongPtrW, SetWindowPos,
            ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_TOP,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SEND_MESSAGE_TIMEOUT_FLAGS, SMTO_NORMAL,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
            SW_HIDE, SW_SHOWNA, WM_ACTIVATE, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDBLCLK,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCACTIVATE, WM_NCDESTROY, WM_NCHITTEST,
            WM_POWERBROADCAST, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETTINGCHANGE,
            WM_WTSSESSION_CHANGE, WS_BORDER, WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_APPWINDOW,
            WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_NOACTIVATE, WS_EX_STATICEDGE,
            WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP,
            WS_SYSMENU, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
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

#[cfg(windows)]
static INNER_WORKSPACE_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static INTERACTION_REVEAL_PENDING: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static INTERACTION_REVEAL_FOCUS: AtomicBool = AtomicBool::new(false);

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

static INTERACTION_REGION_SESSION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

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
    let regions_for_window = state.regions.clone();
    drop(state);
    #[cfg(windows)]
    apply_interaction_window_region(&regions_for_window)?;
    #[cfg(windows)]
    if !regions_for_window.is_empty() && INTERACTION_REVEAL_PENDING.swap(false, Ordering::AcqRel) {
        reveal_interaction_window(INTERACTION_REVEAL_FOCUS.swap(false, Ordering::AcqRel))?;
    }
    Ok(InteractionRegionUpdateResult {
        revision,
        region_count: regions_for_window.len(),
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
fn apply_interaction_window_region(regions: &[PhysicalInteractionRegion]) -> Result<(), String> {
    let Some(hwnd) = interaction_window_hwnd() else {
        return Ok(());
    };
    unsafe {
        // React reports rectangles in WebView/client coordinates. HRGN uses
        // coordinates relative to the *outer* HWND instead. Even after all
        // visible frame styles have been removed, Windows/Tao can retain an
        // invisible resize frame around a top-level WebView window. If the
        // client offset is ignored, the region includes that non-client strip;
        // Windows paints it as a white L-shaped activation frame when the user
        // opens or clicks the conversation surface.
        let mut window_rect = RECT::default();
        GetWindowRect(hwnd, &mut window_rect)
            .map_err(|error| format!("无法读取交互窗口边界：{error}"))?;
        let mut client_origin = POINT::default();
        if !ClientToScreen(hwnd, &mut client_origin).as_bool() {
            return Err("无法换算交互窗口客户区坐标".into());
        }
        let client_offset_x = client_origin.x - window_rect.left;
        let client_offset_y = client_origin.y - window_rect.top;

        let combined = CreateRectRgn(0, 0, 0, 0);
        if combined.is_invalid() {
            return Err("无法创建交互窗口区域".into());
        }
        for region in regions {
            let part = CreateRectRgn(
                region.left + client_offset_x,
                region.top + client_offset_y,
                region.right + client_offset_x,
                region.bottom + client_offset_y,
            );
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
        // non-client strip at its top (measured as client-origin y=2). Windows
        // paints it with the accent/white caption color unless we explicitly
        // supply the caption color. Match the CSS surface instead of removing
        // the resize frame, which would expose an even larger transparent rim.
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

#[cfg(windows)]
fn prepare_hidden_interaction_window(hwnd: HWND) -> Result<(), String> {
    unsafe {
        // Do not let Win11 apply its top-level rounded-frame clip to this
        // transparent host. The visible shape is owned exclusively by the
        // component HRGN published from React.
        let corner_preference = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner_preference).cast(),
            std::mem::size_of_val(&corner_preference) as u32,
        );
        let border_color = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            std::ptr::from_ref(&border_color).cast(),
            std::mem::size_of_val(&border_color) as u32,
        );
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let is_desktop_child = GetParent(hwnd).ok().is_some();
        SetWindowLongPtrW(
            hwnd,
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
                | if is_desktop_child { WS_CHILD.0 as isize } else { WS_POPUP.0 as isize },
        );
        let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            (exstyle
                & !(WS_EX_CLIENTEDGE.0 as isize
                    | WS_EX_DLGMODALFRAME.0 as isize
                    | WS_EX_STATICEDGE.0 as isize
                    | WS_EX_WINDOWEDGE.0 as isize
                    | WS_EX_APPWINDOW.0 as isize))
                | WS_EX_TOOLWINDOW.0 as isize,
        );
        // The window is hidden when this runs, so recalculating the non-client
        // frame cannot leak a white/title-bar frame to the desktop.
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
fn reveal_interaction_window(request_focus: bool) -> Result<(), String> {
    let Some(hwnd) = interaction_window_hwnd() else {
        return Ok(());
    };
    unsafe {
        // Activation can restore Tao's native caption/frame after the window
        // was initially configured. Reapply *both* the popup style and the
        // component-only region immediately before making it visible. DWM
        // color alone cannot remove the "DSH Wallpaper Interaction" caption.
        prepare_hidden_interaction_window(hwnd)?;
        let regions = interaction_regions()
            .read()
            .map_err(|_| "interaction region state poisoned".to_string())?
            .regions
            .clone();
        apply_interaction_window_region(&regions)?;
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        )
        .map_err(|error| error.to_string())?;
        // This is a WorkerW child rather than an always-on-top popup. Normal
        // application windows must cover it; the desktop hit region remains
        // usable when Explorer is foreground.
        let _ = request_focus;
    }
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

/// The icon view is owned by Explorer. We only change its visibility while the
/// user is in the inner workspace: no files, positions, or Explorer settings
/// are altered. It is always restored before a normal application exit.
#[cfg(windows)]
fn desktop_icon_list_view() -> Option<HWND> {
    enumerate_desktop_windows().ok()?.into_iter().find_map(|item| {
        if !item.candidate.hosts_desktop_icons {
            return None;
        }
        let def_view = unsafe {
            FindWindowExW(
                Some(item.hwnd),
                None,
                windows::core::w!("SHELLDLL_DefView"),
                PCWSTR::null(),
            )
        }
        .ok()
        .filter(|hwnd| !hwnd.0.is_null())?;
        unsafe {
            FindWindowExW(
                Some(def_view),
                None,
                windows::core::w!("SysListView32"),
                PCWSTR::null(),
            )
        }
        .ok()
        .filter(|hwnd| !hwnd.0.is_null())
    })
}

#[cfg(windows)]
fn set_desktop_icons_visible(visible: bool) -> Result<(), String> {
    let icon_view = desktop_icon_list_view().ok_or("未找到 Explorer 桌面图标层")?;
    unsafe {
        let _ = ShowWindow(icon_view, if visible { SW_SHOWNA } else { SW_HIDE });
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
    prepare_hidden_interaction_window(hwnd)?;
    let worker = attach_hwnd_to_workerw(hwnd)?;
    resize_wallpaper_to_parent(hwnd, worker)?;
    unsafe { let _ = ShowWindow(hwnd, SW_HIDE); }
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
        // The floating workspace is a transparent desktop host, not a normal
        // dialog: it needs the complete work area so the history can dissolve
        // into the screen edge.
        let max_width = work_width.max(min_width);
        let max_height = work_height.max(min_height);
        let width = requested.width.clamp(min_width, max_width);
        let height = requested.height.clamp(min_height, max_height);
        let x = requested.x.clamp(0, (work_width - width).max(0));
        let y = requested.y.clamp(0, (work_height - height).max(0));
        let desired_left = info.rcWork.left + x;
        let desired_top = info.rcWork.top + y;

        // The requested placement describes the WebView/client viewport, not
        // the outer HWND. Measure the retained Tao/Windows frame *before*
        // moving the window, then calculate the final outer rectangle once.
        // A previous two-step resize (requested outer size, then compensated
        // outer size) emitted two WebView resize events. The frontend reacted
        // to each one by applying placement again, creating a visible feedback
        // loop between the two sizes.
        let mut window_rect = RECT::default();
        let mut client_rect = RECT::default();
        GetWindowRect(hwnd, &mut window_rect)
            .map_err(|error| format!("无法读取交互窗口边界：{error}"))?;
        GetClientRect(hwnd, &mut client_rect)
            .map_err(|error| format!("无法读取交互窗口客户区：{error}"))?;
        let mut client_origin = POINT::default();
        if !ClientToScreen(hwnd, &mut client_origin).as_bool() {
            return Err("无法换算交互窗口客户区坐标".into());
        }
        let outer_width = window_rect.right - window_rect.left;
        let outer_height = window_rect.bottom - window_rect.top;
        let client_width = client_rect.right - client_rect.left;
        let client_height = client_rect.bottom - client_rect.top;
        let inset_x = client_origin.x - window_rect.left;
        let inset_y = client_origin.y - window_rect.top;
        let non_client_width = outer_width - client_width;
        let non_client_height = outer_height - client_height;
        let target_left = desired_left - inset_x;
        let target_top = desired_top - inset_y;
        let target_width = width + non_client_width;
        let target_height = height + non_client_height;
        if window_rect.left != target_left
            || window_rect.top != target_top
            || outer_width != target_width
            || outer_height != target_height
        {
            SetWindowPos(
                hwnd,
                None,
                target_left,
                target_top,
                target_width,
                target_height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|error| format!("无法补偿交互窗口客户区边框：{error}"))?;
        }

        // Keep the last component HRGN in force until React republishes its
        // post-layout rectangles. Clearing it here exposes the whole native
        // host for one frame while the window is visible.
        Ok(InteractionPlacement {
            x,
            y,
            width,
            height,
        })
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
    // Conversation is rendered by the already attached WorkerW background
    // host. Keeping a second WebView2 host in the desktop hierarchy causes
    // duplicate rendering and competing layout updates.
    let _ = (app, request_focus);
    return Ok(());
    #[allow(unreachable_code)]
    let window = app
        .get_webview_window("interaction")
        .ok_or("interaction window missing")?;
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    if request_focus {
        if let Ok(mut deadline) = INTERACTION_REVEAL_GRACE
            .get_or_init(|| RwLock::new(None))
            .write()
        {
            *deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(900));
        }
    }
    let regions = interaction_regions()
        .read()
        .map_err(|_| "interaction region state poisoned".to_string())?
        .regions
        .clone();
    if unsafe { IsWindowVisible(hwnd).as_bool() } {
        INTERACTION_REVEAL_PENDING.store(false, Ordering::Release);
        INTERACTION_REVEAL_FOCUS.store(false, Ordering::Release);
        reveal_interaction_window(request_focus)?;
    } else if !regions.is_empty() {
        // A foreground-app transition hides only the native HWND; React and its
        // component geometry remain alive. Waiting for another MutationObserver
        // publication here can deadlock the window in a permanently hidden
        // state because no DOM or layout change is required when returning to
        // the desktop. Reassert the known-good frame and region, then reveal it
        // immediately. The pending path below is reserved for first render or a
        // genuinely empty surface.
        prepare_hidden_interaction_window(hwnd)?;
        apply_interaction_window_region(&regions)?;
        INTERACTION_REVEAL_PENDING.store(false, Ordering::Release);
        INTERACTION_REVEAL_FOCUS.store(false, Ordering::Release);
        reveal_interaction_window(request_focus)?;
    } else {
        // Do not expose the transparent WebView host before React has laid out
        // its actual controls and published a non-empty native window region.
        INTERACTION_REVEAL_FOCUS.fetch_or(request_focus, Ordering::AcqRel);
        INTERACTION_REVEAL_PENDING.store(true, Ordering::Release);
    }
    if request_focus {
        // A tray command runs while Shell_TrayWnd owns the foreground. Calling
        // the Win32 foreground API from that input callback transfers the
        // activation to our compact interaction HWND before the monitor can
        // interpret the taskbar as an unrelated application.
    }
    Ok(())
}

#[cfg(windows)]
pub fn hide_interaction(app: &tauri::AppHandle) {
    let _ = app;
    return;
    #[allow(unreachable_code)]
    INTERACTION_REVEAL_PENDING.store(false, Ordering::Release);
    INTERACTION_REVEAL_FOCUS.store(false, Ordering::Release);
    if let Some(window) = app.get_webview_window("interaction") {
        if let Ok(raw) = window.hwnd() {
            let hwnd = HWND(raw.0);
            unsafe {
                // Tauri/Tao's hide path may reconstruct top-level decorations
                // when activation changes. Hide the established native HWND
                // directly so its popup/tool-window style remains authoritative.
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            let _ = prepare_hidden_interaction_window(hwnd);
        }
    }
}

#[cfg(windows)]
pub fn start_foreground_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
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
            && matches!(
                foreground_class.as_deref(),
                Some("Shell_TrayWnd") | Some("NotifyIconOverflowWindow")
            );
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
                // Foreground changes are informative only.  Re-showing or
                // hiding the WebView here makes the composition flicker and,
                // more importantly, turns an ordinary focus change into an
                // implicit visibility command.  Explicit tray/settings
                // commands and privacy transitions own that responsibility.
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
            let Some(element) = current else { break; };
            if element.CurrentControlType().ok() == Some(UIA_ListItemControlTypeId) {
                return false;
            }
            current = walker.as_ref().and_then(|tree| tree.GetParentElement(&element).ok());
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
                    if last_blank_click.is_some_and(|previous| now.duration_since(previous) <= std::time::Duration::from_millis(500)) {
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
                        log::info!("桌面空白双击：切换至{}桌面", if entering { "里" } else { "表" });
                        let _ = app.emit("desktop-workspace-toggle", if entering { "enter" } else { "leave" });
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
        const SLEEP_RESOURCE: &str = "personas/wake-frames/variant-anima/sleep.png";
        let bundled_sleep_image = app
            .path()
            .resolve(SLEEP_RESOURCE, tauri::path::BaseDirectory::Resource)
            .ok()
            .filter(|path| path.is_file())
            // `tauri dev` does not copy bundle resources next to target/debug.
            // The source public directory remains the canonical development
            // resource location, while packaged builds take the branch above.
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.join("public").join(SLEEP_RESOURCE))
                    .filter(|path| path.is_file())
            })
            .or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map(|root| root.join("public").join(SLEEP_RESOURCE))
                    .filter(|path| path.is_file())
            })
            .ok_or_else(|| "未找到内置的锁屏睡眠图片。请重新安装 dsh-wallpaper。".to_string())?;

        // WinRT's lock-screen API is unreliable with paths inside a Tauri
        // resource bundle (especially during `tauri dev`): it can receive a
        // virtual/masked resource path and return 0x800700A1.  Hand Windows a
        // normal, current-user-owned file instead.
        let managed_dir = backup_file
            .parent()
            .ok_or_else(|| "无法确定锁屏图片存放目录".to_string())?
            .join("lock-screen");
        std::fs::create_dir_all(&managed_dir)
            .map_err(|_| "无法创建锁屏图片存放目录".to_string())?;
        let managed_path = managed_dir.join("dsh-wallpaper-sleep.png");
        std::fs::copy(&bundled_sleep_image, &managed_path)
            .map_err(|_| "无法准备锁屏图片。请检查应用安装目录是否完整。".to_string())?;
        let path_string = HSTRING::from(managed_path.to_string_lossy().as_ref());
        let file = StorageFile::GetFileFromPathAsync(&path_string)
            .map_err(|_| "Windows 无法读取准备好的锁屏图片。".to_string())?
            .get()
            .map_err(|_| "Windows 无法打开准备好的锁屏图片。".to_string())?;
        let settings = UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
        let changed = settings
            .TrySetLockScreenImageAsync(&file)
            .map_err(|_| "Windows 拒绝设置锁屏图片（可能被组织策略或 Spotlight 管理）。".to_string())?
            .get()
            .map_err(|_| "Windows 未能完成锁屏图片设置。".to_string())?;
        if !changed {
            // Some Windows 11 editions deny TrySet… to unpackaged desktop
            // apps even with no policy configured. The older LockScreen API
            // is the compatible path for a local static PNG.
            LockScreen::SetImageFileAsync(&file)
                .map_err(|_| "Windows 不允许此未打包桌面应用接管锁屏图片。请使用 MSIX 安装包，或在 Windows 设置中手动选择该图片。".to_string())?
                .get()
                .map_err(|_| "Windows 不允许此未打包桌面应用接管锁屏图片。请使用 MSIX 安装包，或在 Windows 设置中手动选择该图片。".to_string())?;
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
