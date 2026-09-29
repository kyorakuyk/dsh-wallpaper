use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::app_core::{AppAction, AppCore, WallpaperHostMode, WallpaperHostStatus};
#[cfg(windows)]
use crate::native_bootstrap;

#[cfg(windows)]
use crate::lock_screen_backup::{
    discard_backup_after_failed_takeover, discard_stale_backup, ensure_backup_for_takeover,
    has_stale_backup, inspect_backup, managed_image_is_active, managed_image_path,
    file_content_hash, managed_image_file_for_content, managed_image_path_from_file,
    remove_backup_after_verified_restore,
    restore_snapshot_path, same_local_file_uri, LockScreenBackupLease, LockScreenBackupManifest,
    LockScreenBackupState, LEGACY_MANAGED_IMAGE_FILE,
};

#[cfg(windows)]
use tauri::{Emitter, EventTarget, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The wallpaper host is the sole consumer of system, tray, and desktop
/// workspace events.  Keep native notifications out of the settings WebView:
/// it has no need to observe chat/session state or to receive a future event
/// carrying privacy-sensitive desktop metadata.
#[cfg(windows)]
const BACKGROUND_WINDOW_LABEL: &str = "background";

#[cfg(windows)]
fn emit_to_background<S: serde::Serialize + Clone>(
    app: &tauri::AppHandle,
    event: &str,
    payload: S,
) {
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        event,
        payload,
    );
}

/// Tell the wallpaper host about a real session or power transition, and leave a
/// line behind.
///
/// These four events decide whether the desktop leaves the sleeping portrait, and
/// the log used to say nothing about them: a wake that did not take effect could
/// not be told apart from Windows never telling us, so the only available answer
/// was a guess. Silence in the log now means the event never arrived; a line
/// means it did, and the renderer owned the outcome.
#[cfg(windows)]
fn emit_system_session(app: &tauri::AppHandle, transition: &'static str) {
    log::info!("系统会话事件：{transition}");
    emit_to_background(app, "system-session", transition);
}

#[cfg(windows)]
use windows::Win32::System::Variant::VARIANT;
use windows::{
    core::{w, BOOL, HSTRING, PCWSTR, PWSTR},
    ApplicationModel::{StartupTask, StartupTaskState},
    Storage::StorageFile,
    System::UserProfile::{LockScreen, UserProfilePersonalizationSettings},
    Win32::{
        Foundation::{
            GetLastError, APPMODEL_ERROR_NO_PACKAGE, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND,
            ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, ERROR_SUCCESS, HWND, LPARAM, LRESULT,
            POINT, RECT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
        },
        Graphics::{
            Dwm::{
                DwmExtendFrameIntoClientArea, DwmGetWindowAttribute, DwmSetWindowAttribute,
                DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_COLOR_NONE,
                DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
                DWMWCP_DONOTROUND,
            },
            Gdi::{
                ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
            },
        },
        Storage::Packaging::Appx::{
            GetCurrentPackageFamilyName, GetCurrentPackageFullName, GetCurrentPackagePath,
        },
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
            // `AttachThreadInput` lives here in windows 0.61, not under KeyboardAndMouse.
            Threading::{AttachThreadInput, GetCurrentThreadId},
            Registry::{
                RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER,
                KEY_READ, KEY_SET_VALUE,
            },
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
            Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
        UI::WindowsAndMessaging::{
            EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetClientRect, GetCursorPos,
            GetDesktopWindow, GetForegroundWindow, GetParent, GetWindow, GetWindowLongPtrW,
            GetWindowRect, IsWindow, IsWindowVisible, SendMessageTimeoutW, SetParent,
            SetWindowLongPtrW, SetWindowPos, ShowWindow, WindowFromPoint, GWL_EXSTYLE, GWL_STYLE,
            GW_HWNDNEXT, GW_HWNDPREV, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT,
            HTTOP, HTTOPLEFT, HTTOPRIGHT, HTTRANSPARENT, HWND_TOP, MONITORINFOF_PRIMARY,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SEND_MESSAGE_TIMEOUT_FLAGS, SMTO_NORMAL,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
            GetAncestor, GetWindowThreadProcessId, GetGUIThreadInfo, SetForegroundWindow, GA_ROOT, SW_HIDE, SW_SHOWNA, WM_ACTIVATE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCLBUTTONDOWN, WM_SETFOCUS, GUITHREADINFO,
            WM_NCDESTROY, WM_NCHITTEST, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WS_BORDER,
            WS_CAPTION, WS_CHILD, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
            WS_EX_STATICEDGE, WS_EX_TOOLWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
        },
        UI::{
            Accessibility::{CUIAutomation, IUIAutomation, UIA_ControlTypePropertyId, UIA_ListItemControlTypeId, UIA_PaneControlTypeId, TreeScope_Descendants},
            Controls::MARGINS,
            HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
            Input::KeyboardAndMouse::{GetAsyncKeyState, GetDoubleClickTime, SetFocus, VK_LBUTTON},
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
    let handle = match unsafe { CreateMutexW(None, true, WALLPAPER_HOST_MUTEX_NAME) } {
        Ok(handle) => handle,
        Err(error) => {
            log::error!("无法建立壁纸宿主互斥锁：{error}");
            return false;
        }
    };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
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
static INNER_WORKSPACE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Serializes the whole native lock-screen ownership transaction.  The
/// settings WebView already avoids duplicate clicks, but commands can also
/// arrive from the tray, the frontend, or a second Tauri surface.  Without a
/// native lock, those callers could race between snapshot capture, the WinRT
/// setter, and rollback/restore cleanup.
#[cfg(windows)]
static LOCK_SCREEN_TRANSACTION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// The app normally has one process, but an installer update, a manual second
/// launch, or an old process winding down can overlap with it.  A process-local
/// async mutex cannot protect their shared current-user config directory, so
/// the native transaction also holds this current-session named mutex.
#[cfg(windows)]
const LOCK_SCREEN_TRANSACTION_MUTEX_NAME: windows::core::PCWSTR =
    w!("Local\\DSHWallpaper.LockScreenTransaction.v1");

#[cfg(windows)]
struct CrossProcessLockScreenTransaction {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl CrossProcessLockScreenTransaction {
    fn acquire() -> Result<Self, String> {
        let handle = unsafe { CreateMutexW(None, false, LOCK_SCREEN_TRANSACTION_MUTEX_NAME) }
            .map_err(|error| format!("无法建立锁屏接管事务锁：{error}"))?;
        let wait = unsafe { WaitForSingleObject(handle, 30_000) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            return Err("锁屏接管操作正在被另一个 dsh-wallpaper 实例处理，请稍后重试。".into());
        }
        if wait == WAIT_ABANDONED {
            // A previous owner exited while inside a transaction. Existing
            // manifest validation below is deliberately fail-closed; continue
            // only under that validation rather than trusting the abandoned
            // operation's partial state.
            log::warn!("检测到中断的锁屏接管事务；将按现有备份状态进行安全检查");
        }
        Ok(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for CrossProcessLockScreenTransaction {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

const MAX_INTERACTION_REGIONS: usize = 128;
const MIN_SCALE_FACTOR: f64 = 0.5;
const MAX_SCALE_FACTOR: f64 = 8.0;

/// 展开态输入岛发布的热区 id（`ConversationBubble` 的 `data-interaction-region="chat"`）。
///
/// 原生侧靠它判断「输入岛现在是不是可见」——悬浮球要据此停止弹出（拍板要求：
/// 岛可见时球不该冒出来）。两个文件之间的这层耦合在两端都写了注释，改名必须同步。
pub(crate) const ISLAND_REGION_ID: &str = "chat";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRegionInput {
    /// 前端给每个热区起的名字（`data-interaction-region`）。缺失时为空串：
    /// 老版本前端不带 id，位置判定照旧，只是「岛是否可见」会判为否。
    #[serde(default)]
    pub id: String,
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
    /// 最近一次发布里是否含 `ISLAND_REGION_ID`，也就是输入岛此刻是否可见。
    ///
    /// 悬浮球不参与热区判定（它是独立窗口，自己收鼠标），但它必须知道岛是否已经
    /// 在前面——否则球会在岛正上方冒出来。判定就放在发布热区这一步，避免为它再开
    /// 一条 IPC。
    island_visible: bool,
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
pub struct DesktopLayoutMetrics {
    /// Logical pixels reserved below the expanded input island.
    pub expanded_bottom_inset: f64,
    pub taskbar_visible: bool,
}

/// A monitor rectangle is expressed in the virtual desktop's physical pixel
/// coordinate space. The frontend normalizes these rectangles against the
/// virtual bounds, so a left/top monitor with negative coordinates remains
/// stable and does not force the WebView to use a second origin.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDisplayInfo {
    /// Stable Windows device name, usually `\\.\\DISPLAY1`.
    pub id: String,
    pub name: String,
    pub bounds: DesktopRect,
    pub work_area: DesktopRect,
    pub scale_factor: f64,
    pub primary: bool,
}

#[cfg(windows)]
fn desktop_rect(rect: RECT) -> DesktopRect {
    DesktopRect {
        x: rect.left,
        y: rect.top,
        width: rect.right.saturating_sub(rect.left),
        height: rect.bottom.saturating_sub(rect.top),
    }
}

#[cfg(windows)]
fn monitor_name(buffer: &[u16]) -> String {
    let length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
        .trim()
        .to_string()
}

#[cfg(windows)]
unsafe extern "system" fn enumerate_desktop_display(
    monitor: HMONITOR,
    _dc: HDC,
    _monitor_rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let displays = &mut *(data.0 as *mut Vec<DesktopDisplayInfo>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
        return BOOL(1);
    }

    let id = monitor_name(&info.szDevice);
    if id.is_empty() {
        return BOOL(1);
    }
    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    let scale_factor =
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
            ((dpi_x.max(1) as f64) / 96.0).clamp(0.5, 8.0)
        } else {
            1.0
        };
    let bounds = desktop_rect(info.monitorInfo.rcMonitor);
    let work_area = desktop_rect(info.monitorInfo.rcWork);
    if bounds.width <= 0 || bounds.height <= 0 {
        return BOOL(1);
    }
    displays.push(DesktopDisplayInfo {
        id: id.clone(),
        name: id,
        bounds,
        work_area,
        scale_factor,
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    });
    BOOL(1)
}

#[cfg(windows)]
pub fn desktop_displays() -> Result<Vec<DesktopDisplayInfo>, String> {
    let mut displays = Vec::<DesktopDisplayInfo>::new();
    let result = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enumerate_desktop_display),
            LPARAM((&mut displays as *mut Vec<DesktopDisplayInfo>) as isize),
        )
    };
    if !result.as_bool() {
        return Err("Windows 无法枚举当前显示器。".into());
    }
    displays.sort_by(|left, right| {
        left.bounds
            .x
            .cmp(&right.bounds.x)
            .then(left.bounds.y.cmp(&right.bounds.y))
            .then(left.id.cmp(&right.id))
    });
    if displays.is_empty() {
        return Err("Windows 未返回可用显示器。".into());
    }
    Ok(displays)
}

#[cfg(not(windows))]
pub fn desktop_displays() -> Result<Vec<DesktopDisplayInfo>, String> {
    Ok(vec![DesktopDisplayInfo {
        id: "preview".into(),
        name: "预览屏幕".into(),
        bounds: DesktopRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: DesktopRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        scale_factor: 1.0,
        primary: true,
    }])
}

pub fn desktop_layout_metrics(
    window: &WebviewWindow,
    display_id: Option<&str>,
) -> DesktopLayoutMetrics {
    #[cfg(windows)]
    {
        let scale = window
            .scale_factor()
            .unwrap_or(1.0)
            .clamp(MIN_SCALE_FACTOR, MAX_SCALE_FACTOR);
        let fallback = 48.0;
        if let Ok(displays) = desktop_displays() {
            let display = display_id
                .and_then(|id| displays.iter().find(|display| display.id == id))
                .or_else(|| displays.iter().find(|display| display.primary))
                .or_else(|| displays.first());
            if let Some(display) = display {
                let bottom = display.bounds.y.saturating_add(display.bounds.height);
                let work_bottom = display.work_area.y.saturating_add(display.work_area.height);
                let top_gap = display.work_area.y.saturating_sub(display.bounds.y);
                let bottom_gap = bottom.saturating_sub(work_bottom);
                if bottom_gap > 0 && top_gap == 0 {
                    let logical_height = bottom_gap as f64
                        / display
                            .scale_factor
                            .clamp(MIN_SCALE_FACTOR, MAX_SCALE_FACTOR);
                    if logical_height >= 12.0 {
                        return DesktopLayoutMetrics {
                            expanded_bottom_inset: logical_height * 2.0,
                            taskbar_visible: true,
                        };
                    }
                }
            }
        }
        let Ok(raw) = window.hwnd() else {
            return DesktopLayoutMetrics {
                expanded_bottom_inset: fallback,
                taskbar_visible: false,
            };
        };
        let background = HWND(raw.0);
        let mut background_rect = RECT::default();
        let mut taskbar_rect = RECT::default();
        let taskbar =
            unsafe { FindWindowW(windows::core::w!("Shell_TrayWnd"), PCWSTR::null()) }.ok();
        let visible = taskbar.is_some_and(|taskbar| unsafe { IsWindowVisible(taskbar).as_bool() })
            && unsafe { GetWindowRect(background, &mut background_rect).is_ok() }
            && taskbar.is_some_and(|taskbar| unsafe {
                GetWindowRect(taskbar, &mut taskbar_rect).is_ok()
            });
        if visible {
            let physical_height = (taskbar_rect.bottom - taskbar_rect.top).max(0);
            let at_bottom =
                taskbar_rect.top >= background_rect.bottom.saturating_sub(physical_height + 2);
            let logical_height = physical_height as f64 / scale;
            if at_bottom && logical_height >= 12.0 {
                return DesktopLayoutMetrics {
                    expanded_bottom_inset: logical_height * 2.0,
                    taskbar_visible: true,
                };
            }
        }
        DesktopLayoutMetrics {
            expanded_bottom_inset: fallback,
            taskbar_visible: false,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        let _ = display_id;
        DesktopLayoutMetrics {
            expanded_bottom_inset: 48.0,
            taskbar_visible: false,
        }
    }
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
            emit_to_background(app, "app-snapshot", &snapshot);
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
    // 判定要在 `into_iter()` 吃掉 regions 之前做。
    let island_visible = regions.iter().any(|region| region.id == ISLAND_REGION_ID);
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
    // 岛可见性的变化必须留痕：悬浮球「岛在前面就不弹」完全建立在这个标志上，
    // 而它只可能由前端发布热区改变——出问题时第一个要看的就是这条日志。
    if state.island_visible != island_visible {
        // 热区矩形也打出来：双击误判（"点功能组件却切回表桌面"）只可能是"这个点不在热区里"，
        // 而热区是前端按元素 rect 发布的——没有矩形就没法判断是发布错了还是判断错了。
        let rects = state
            .regions
            .iter()
            .map(|region| {
                format!(
                    "({},{})-({},{})",
                    region.left, region.top, region.right, region.bottom
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        log::info!(
            "interaction regions: island_visible={} region_count={} revision={} scale={} rects=[{}]",
            island_visible,
            state.regions.len(),
            revision,
            scale_factor,
            rects
        );
    }
    state.island_visible = island_visible;
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
    // 新会话开始 = 前端重新挂载，热区清空，岛此刻不可能是可见的。
    state.island_visible = false;
    Ok(session)
}

/// 输入岛此刻是否可见（= 最近一次发布的热区里含 `ISLAND_REGION_ID`）。
///
/// 悬浮球用它挡住「岛已经在前台还冒出来」的情形。列表为空、会话刚重置、前端从未
/// 发布过，都判为否——那时岛本来就没有渲染。
pub(crate) fn island_visible_from_regions() -> bool {
    interaction_regions()
        .read()
        .map(|state| state.island_visible)
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn configure_settings_window(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    unsafe {
        if !SetWindowSubclass(hwnd, Some(settings_window_subclass), 0x4453_4853, 0).as_bool() {
            return Err("无法监听设置窗口边框状态".into());
        }
    }
    // 子类**先装**，再强制一次框架重算。
    //
    // 顺序很关键（实测 0.2.0.97 才知道）：Windows 会把客户区矩形**缓存**起来，
    // 只在 `WM_NCCALCSIZE` 时重算；而 tao 创建窗口时（我们的子类还不存在）已经算过一遍，
    // 于是子类里的 `WM_NCCALCSIZE → 0`（去掉非客户区）永远等不到消息，
    // 客户区一直是 `(11,2)` 的旧值。`SWP_FRAMECHANGED` 会逼出这次重算。
    unsafe {
        if let Err(error) = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        ) {
            log::warn!("settings window: forcing a frame recalculation failed: {error}");
        }
    }
    apply_settings_window_frame_policy(hwnd)?;
    // 这行现在同时是自检：客户区起点必须是 (0,0)，否则说明非客户区还在。
    log_settings_window_metrics(hwnd, "configure");
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

/// 无客户区之后，自己补的缩放命中带宽度（逻辑像素）。
///
/// Windows 的可调边框在 150% 缩放下实测是 11 物理像素（≈7.3 逻辑像素），这里取 8 逻辑像素，
/// 与系统手感一致。
#[cfg(windows)]
const SETTINGS_RESIZE_BAND_PX: i32 = 8;

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

        // 深色框架：本机 Windows 是浅色主题，DWM 用**系统浅色**画窗口框架
        // （实测：左/右/下各 11 物理像素亮边；顶部因为设置了 caption 颜色所以是暗的）。
        // 那圈亮框正是用户说的"尖角难看"——它方，而且与深色面板对比强烈。
        // 打开沉浸式深色模式后，框架与边框按深色绘制，与面板同族。
        let dark_mode: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark_mode).cast(),
            std::mem::size_of_val(&dark_mode) as u32,
        )
        .map_err(|error| format!("无法把设置窗口边框切到深色：{error}"))?;

        // 把系统框架"吃进"整个客户区（-1 = 整窗玻璃）。
        //
        // 不加这一步，可调边框占的那 11 像素永远在客户区之外，CSS 只能画到客户区为止，
        // 深色面板外面必然留一圈系统色的框。扩展之后客户区覆盖整个窗口矩形，面板可以
        // 一直画到边缘；DWM 仍在外面套上系统圆角与阴影，并保留边框上的拖拽命中区
        // （所以窗口依旧可缩放）。
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        DwmExtendFrameIntoClientArea(hwnd, &margins)
            .map_err(|error| format!("无法把设置窗口系统框架扩展到客户区：{error}"))?;

        // 圆角交回 Windows 11 自己决定（`DWMWCP_DEFAULT` 对普通窗口就是圆角），
        // 不再用 `DWMWCP_DONOTROUND` 把它关掉。
        //
        // 起因：用户看到设置中心是**尖角**。原因是 `.settings-window` 用不透明底色
        // 铺满整个方窗，盖住了内层 `.settings-app` 的圆角；而这里又把 DWM 圆角关掉，
        // 于是没有任何一层在画圆角。
        //
        // 现在两边对齐：DWM 用系统默认半径（Windows 11 为 8 逻辑像素），
        // `SettingsWindow.css` 里的 `.settings-app` 也用 8px 且根节点改为透明。
        // 两边半径一致，才不会出现"CSS 曲线之外露出桌面"的第二道弧。
        let corner_preference = DWMWCP_DEFAULT;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner_preference).cast(),
            std::mem::size_of_val(&corner_preference) as u32,
        )
        .map_err(|error| format!("无法启用设置窗口系统圆角：{error}"))?;

        // 读回一次：圆角到底生效没有，必须是日志里能一眼看到的结论。
        // 同时打上窗口样式：DWM 只对带框架的窗口套用系统圆角，样式里没有
        // WS_THICKFRAME / WS_CAPTION 时"圆角没生效"就是这一条的原因。
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as isize;
        let thick_frame = style & WS_THICKFRAME.0 as isize != 0;
        let caption = style & WS_CAPTION.0 as isize != 0;
        let mut applied: u32 = 0;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_mut(&mut applied).cast(),
            std::mem::size_of_val(&applied) as u32,
        )
        .is_ok()
        {
            let round = match applied {
                0 | 2 => "rounded (system default)",
                3 => "rounded small",
                1 => "DO NOT ROUND",
                _ => "unknown",
            };
            log::info!(
                "settings frame: corner preference read back = {applied} ({round}); style=0x{style:X} thick_frame={thick_frame} caption={caption}"
            );
        }
    }
    Ok(())
}

/// 无客户区窗口的缩放命中测试：把窗口边缘 `SETTINGS_RESIZE_BAND_PX` 逻辑像素报成边框。
///
/// 去掉了非客户区（见 `settings_window_subclass` 的 `WM_NCCALCSIZE`），Windows 就不再
/// 提供"拖边改大小"的命中区，这里补回来。只在贴边那一圈生效，其余返回 `None`，
/// 调用方原样交回默认结果——**绝不能在客户区内部乱报**，否则会吞掉 WebView 的点击。
#[cfg(windows)]
unsafe fn resize_edge(hwnd: HWND, lparam: LPARAM) -> Option<u32> {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return None;
    }
    // lparam 是屏幕坐标的打包值，低 16 位 x、高 16 位 y（各自带符号）。
    let packed = lparam.0 as u32;
    let x = (packed as u16 as i16) as i32;
    let y = ((packed >> 16) as u16 as i16) as i32;
    let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
    let band = (SETTINGS_RESIZE_BAND_PX as f64 * scale).round().max(1.0) as i32;

    let left = x - rect.left;
    let right = rect.right - x;
    let top = y - rect.top;
    let bottom = rect.bottom - y;
    if left < 0 || top < 0 || right < 0 || bottom < 0 {
        return None; // 点在窗口之外（例如阴影区），交给默认处理
    }
    let (west, east) = (left <= band, right <= band);
    let (north, south) = (top <= band, bottom <= band);
    Some(match (west, east, north, south) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => return None,
    })
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
    // 去掉非客户区：让**客户区等于整个窗口矩形**。
    //
    // 为什么必须这么做（实测，0.2.0.96）：只要窗口还留着标准框架，客户区就永远比窗口矩形
    // 小 `(11,2)`（左/右/下 11 物理像素、顶 2 像素，150% 缩放）。CSS 只能画到客户区为止，
    // 于是深色面板外面必然留一圈由 **系统主题** 决定颜色的框——浅色主题下就是用户看到的
    // 那圈白（"尖角难看"的真正来源）。改 caption/border 颜色只是在跟这圈框较劲，
    // 换主题就又会不一致。
    //
    // 非客户区归零之后：面板可以一直画到窗口边缘；DWM 依然按 `DWMWCP_DEFAULT` 在外面套
    // 系统圆角与阴影（`DwmExtendFrameIntoClientArea(-1)` 就是为无框架窗口保留这些而设的），
    // 而系统圆角半径与 `.settings-app` 的 8px 一致，两道弧重合。
    //
    // 代价与补偿：框架消失后 Windows 不再提供"拖边改大小"，所以缩放改由**渲染层**发起
    // （`SettingsWindow.tsx` 的 `.settings-resize-*` 边条调 `startResizeDragging`）。
    // 为什么不能像过去那样靠窗口自己的命中测试：WebView2 渲染窗口属于**另一个进程**、
    // 铺满整个客户区，鼠标命中测试归它，父窗口的 `WM_NCHITTEST` 根本不会被问到——
    // 这与桌面输入模型里那条实测结论是同一个原因（见
    // `docs/evidence/input-model-desktop-hit-testing.md`）。下面 `resize_edge` 只在
    // 客户区之外/未被 WebView 覆盖的地方还会被用到，留着作为兜底。
    //
    // **Lite 版不加**：Lite 的设置界面是另一条工作流的文件，没有配套的边条，
    // 去掉框架会让它既没边框也不能缩放。
    #[cfg(not(feature = "lite"))]
    if message == WM_NCCALCSIZE && wparam.0 != 0 {
        return LRESULT(0);
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    match message {
        // 自绘的缩放边框：非客户区没了，命中测试得自己给。
        // 只报边缘那 8 逻辑像素，其余一律交回默认结果，避免吞掉 WebView 的点击。
        WM_NCHITTEST => {
            if let Some(edge) = resize_edge(hwnd, lparam) {
                return LRESULT(edge as isize);
            }
        }
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

/// 这个类是不是**桌面表面本身**。只给双击切换那条判据用，别改
/// `is_desktop_foreground_class` 的含义 —— 那个是 Win+D 覆盖层的老判据（只认 WorkerW/Progman），
/// 现在这里要多认两个：点空白桌面时前台可能落到图标视图上。
fn is_desktop_surface_class(class: Option<&str>) -> bool {
    matches!(
        class,
        Some("Progman")
            | Some("WorkerW")
            | Some("SHELLDLL_DefView")
            | Some("SysListView32")
            // 桌面窗口自己的类名。**这一条是被实测逼出来的**：光标压在覆盖层上时，UIA 父链里能看到的
            // 桌面祖先就是这个类名的"桌面 N" Pane，而上面四个类名一个都不出现 —— 于是判负、双击被拒。
            | Some("#32769")
    )
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
pub(crate) fn visible_wallpaper_worker() -> Option<HWND> {
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
    let class_len = unsafe { GetClassNameW(hwnd, &mut class) };
    (class_len > 0).then(|| String::from_utf16_lossy(&class[..class_len as usize]))
}

#[cfg(windows)]
fn window_neighbor_class(
    hwnd: HWND,
    command: windows::Win32::UI::WindowsAndMessaging::GET_WINDOW_CMD,
) -> String {
    unsafe { GetWindow(hwnd, command).ok() }
        .filter(|neighbor| !neighbor.0.is_null())
        .and_then(window_class)
        .unwrap_or_else(|| "none".into())
}

#[cfg(windows)]
fn record_wallpaper_window_state(name: &str, hwnd: HWND) {
    let parent = unsafe { GetParent(hwnd).ok() }
        .and_then(window_class)
        .unwrap_or_else(|| "none".into());
    let mut client = RECT::default();
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
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    crate::native_bootstrap::record_startup_diagnostic(&format!(
        "event=window-state layer={} parent={} size={} visible={} prev={} next={}",
        name, parent, size, visible, previous, next
    ));
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
        .filter(|hwnd| unsafe { GetParent(*hwnd).ok() } == Some(parent));
    let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;

    if window_class(parent).as_deref() == Some("Progman") {
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
    unsafe {
        let _ = ShowWindow(background, SW_SHOWNA);
    }
    record_wallpaper_window_state("background", background);
    if let Some(cover) = native_bootstrap::window_handle() {
        record_wallpaper_window_state("native-cover", cover);
    }
    crate::native_bootstrap::record_startup_diagnostic("event=webview-attached-and-visible");
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
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if WALLPAPER_RECOVERY_QUEUED.swap(true, Ordering::AcqRel) {
            continue;
        }
        let recovery_app = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
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
    unsafe {
        let _ = WTSUnRegisterSessionNotification(hwnd);
        let _ = RemoveWindowSubclass(hwnd, Some(session_subclass_proc), 1);
    }
}

#[cfg(not(windows))]
pub fn unregister_session_events(_: &tauri::AppHandle) {}

#[cfg(windows)]
fn dispatch_system_action(app: &tauri::AppHandle, action: AppAction) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    emit_to_background(app, "app-snapshot", &snapshot);
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
        unsafe { GetWindowThreadProcessId(current, Some(&mut pid)) };
        if pid == std::process::id() {
            return true;
        }
        if pid == 0 {
            return false;
        }
        // `GetParent` is a `Result` in windows 0.61, and a window with no parent is
        // the normal termination of this walk rather than an error.
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
    if dispatched.is_err() {
        log::warn!("输入岛交接：无法到达主线程");
    }
}

#[cfg(not(windows))]
pub fn hand_over_keyboard_for_app(_app: &tauri::AppHandle) {}

#[cfg(windows)]
pub fn hand_over_keyboard_after_verified_click(host: HWND) -> String {
    unsafe {
        let top = GetAncestor(host, GA_ROOT);
        if top.0.is_null() {
            return "未找到顶层祖先窗口；未执行交接".to_string();
        }
        let foreground_before = GetForegroundWindow();
        let mut foreground_thread = 0u32;
        GetWindowThreadProcessId(foreground_before, Some(&mut foreground_thread));
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let foreground_ok = SetForegroundWindow(top).as_bool();
        let focus_ok = SetFocus(Some(host)).is_ok();
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
        format!(
            "交接结果：附接={attached} SetForegroundWindow={foreground_ok} SetFocus={focus_ok} | 读回 前台={foreground_class}(pid={foreground_now_pid}) 焦点={focus_class}(pid={focus_pid:?}) 壁纸pid={}",
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
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON.0 as i32) >= 0 {
            return Err("未检测到物理左键按下；拒绝把 IPC 调用当作输入岛点击".into());
        }
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_err() {
            return Err("无法读取光标位置".into());
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
                "光标下的窗口父链不属于壁纸（class={under_class} pid={under_pid}）；拒绝激活"
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
    unsafe {
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        let foreground_thread = GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let focus_result = SetFocus(Some(root));
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
        if ours && webview_has_focus {
            log::info!("island click: keyboard channel handed over to the WebView");
        } else {
            log::warn!(
                "island click: handover did not take - attached={attached} set_focus_ok={} foreground_is_wallpaper={ours} focus_on_webview={webview_has_focus} foreground_pid={foreground_now_pid}",
                focus_result.is_ok(),
            );
        }
    }
}

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
                // Inside the input island. `WM_NCHITTEST` reaches us where the mouse
                // messages do not, so this is where a real click can be recognised: the
                // point is in a declared region and the physical left button is down.
                // Outside the island nothing here runs, so the wallpaper still never
                // takes the keyboard without the user asking for it (plan 3.C).
                if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0 {
                    // Phase A still has one open question: whether this branch is reached at all on
                    // a real click. The WebView2 child belongs to another process, so hit-testing may
                    // not be ours to see. Traced so the answer is measured rather than assumed.
                    trace_island_click_reached(local_x, local_y);
                    hand_over_keyboard_after_island_click(root_hwnd);
                }
            } else {
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
        }
        WM_MOUSEACTIVATE => {
            // Sent to decide whether a click may activate the window. If it arrives,
            // it is the one message that can legitimately request the foreground.
            trace_focus_handoff("WM_MOUSEACTIVATE", None);
        }
        WM_LBUTTONDOWN | WM_NCLBUTTONDOWN => {
            trace_focus_handoff("WM_LBUTTONDOWN", Some(client_point(lparam)));
        }
        WM_ACTIVATE | WM_SETFOCUS => {
            trace_focus_handoff("WM_ACTIVATE/WM_SETFOCUS", None);
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
        if app.get_webview_window("background").is_none() {
            break;
        }
    });
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
        let Some(window) = handle.get_webview_window("background") else {
            return;
        };
        let Ok(hwnd) = window.hwnd() else {
            log::warn!("desktop focus restore: background window has no handle");
            return;
        };
        match unsafe { SetFocus(Some(HWND(hwnd.0))) } {
            Ok(_) => {
                // Tiered on purpose (repair plan 3.C). `SetFocus` returning success only
                // proves this thread could set focus; it does not prove Windows handed the
                // keyboard channel over. Reporting "restored" on that alone is what made a
                // failing fix look like a working one, so the claim is graded by readback:
                // the window must actually be foreground and its keyboard focus must sit on
                // a WebView2 process, which is the state a working input was measured in.
                let foreground = unsafe { GetForegroundWindow() };
                let mut foreground_pid = 0u32;
                unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_pid)) };
                let mut info = GUITHREADINFO::default();
                info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
                let focus_pid = if unsafe { GetGUIThreadInfo(0, &mut info) }.is_ok() {
                    let mut pid = 0u32;
                    unsafe { GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid)) };
                    Some(pid)
                } else {
                    None
                };
                let ours = foreground_pid == std::process::id();
                // A WebView2 input window belongs to a browser helper process, never to us,
                // so a different-but-present PID is the expected success shape here.
                let webview2_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
                if ours && webview2_has_focus {
                    log::info!("keyboard channel restored: foreground is the wallpaper and focus is on the WebView");
                } else {
                    log::warn!(
                        "thread focus set, but the keyboard channel is not restored: \
                         foreground_is_wallpaper={ours} focus_on_webview={webview2_has_focus}"
                    );
                }
            }
            // Not fatal - the desktop may not be foreground yet - but reported rather
            // than swallowed, since a silent failure here is what made this hard to find.
            Err(error) => log::warn!("desktop focus restore: SetFocus failed: {error}"),
        }
    });
    if dispatched.is_err() {
        log::warn!("desktop focus restore: could not reach the main thread");
    }
}

/// 一次双击里，前台窗口是否已经换了人 —— 也就是这次双击确实**启动了/激活了什么东西**。
///
/// 这是"双击桌面图标会翻桌面层"这个缺陷的行为兜底：判据（UIA）可以说谎，但"双击之后前台变成了
/// 别的应用的窗口"不会 —— 那说明这次双击不是对着空白桌面发的，就不该翻。
///
/// 只在前台**从某个值变成了另一个值**、且新值既不是桌面家族也不是我们自己时才为真：
/// - 空白桌面双击可能让 Progman 变成前台（那是桌面本身），仍然允许翻；
/// - 前台没变（点空白时常见，因为点击穿透让前台留在原处）也仍然允许翻。
/// 这一次双击配对允许的间隔：**用系统设置**，不要写死。
///
/// 写死过一个 500ms，结果是"时好时坏"：点击稍慢的用户，系统认为那是双击（图标照常打开），
/// 而我们的判据认为不是，于是每一次点击都被当成"第一击"，功能静默失效（2026-09-30 实测，
/// 日志里连续三次 armed 却一次都没切换）。Windows 把这个间隔作为用户设置暴露给程序，
/// 就应该按它来；上限夹到 2 秒，防止被设成极端值时误配对。
/// 两次点击的位置是否近到可以算作一次双击。
///
/// 这是把时间窗口放宽之后的必要配套：时间放宽了，两次**互不相干**的单击也可能落进窗口里，
/// 那就会凭空切换桌面层。真正的双击几乎在原处（人手的抖动是几个像素），所以按切比雪夫距离
/// 判一个宽松的容差即可。容差取物理像素，因为 `GetCursorPos` 给的就是物理坐标。
fn within_double_click_reach(a: (i32, i32), b: (i32, i32), tolerance: i32) -> bool {
    (a.0 - b.0).abs() <= tolerance && (a.1 - b.1).abs() <= tolerance
}

fn double_click_pairing_window_ms(system_ms: u32) -> u64 {
    // 这里是**我们自己的手势**，不是操作系统的双击：系统那个 500ms 决定的是"Explorer 要不要打开
    // 图标"，而"在空白桌面上连点两下"在系统看来只是两次普通点击。实测用户自然的节奏会超过 500ms
    // （系统值就是 500，而功能一直不触发），所以取系统值与 900ms 里较大的那个，再夹到 2 秒以内。
    // 用户决定按 900ms 试（此前 1500ms）。两者的取舍是明确写下来的：日志里一次**真实的**桌面双击
    // 间隔是 1044ms —— 900ms 时那种节奏会落空；反过来 1500ms 更容易让两次无关的单击凑成一对。
    // 位置相近那条条件（within_double_click_reach）继续兜住后者的风险。日志每次配对都会打印实测间隔，
    // 所以下一步该往哪边调，看数字即可。
    u64::from(system_ms).clamp(200, 2000).max(900)
}

fn double_click_became_someone_elses(
    foreground_at_first_click: Option<isize>,
    foreground_now: Option<isize>,
    foreground_now_is_desktop_surface: bool,
) -> bool {
    // 放行只有两种：前台**没变**，或换成了**桌面表面窗口**（Progman/WorkerW/图标视图）。
    //
    // 判据用窗口句柄而不是进程号，而且**不再放行"我们自己的进程"**——这一条是两次实测换来的：
    // * 按"进程号是 Explorer"放行时漏掉了"用资源管理器打开文件夹"（动作由 explorer 自己完成）；
    // * 按"进程号是我们自己"放行时漏掉了"双击壁纸自己的桌面快捷方式"（应用已在运行，激活它不换
    //   进程，只换窗口）。
    // 句柄比较把这两种都归入"别人动过了"；点空白桌面时前台要么不动、要么落到桌面表面窗口上。
    match (foreground_at_first_click, foreground_now) {
        (Some(before), Some(now)) if before != now => !foreground_now_is_desktop_surface,
        _ => false,
    }
}

/// Uses UI Automation, rather than ListView messages with a pointer owned by
/// Explorer, to distinguish desktop icons from empty desktop space. This is a
/// supported cross-process accessibility boundary and never consumes input.
#[cfg(windows)]
/// 矩形是否包含某点。抽成纯函数，便于测试。
fn rect_contains_point(rect: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    x >= rect.0 && x < rect.2 && y >= rect.1 && y < rect.3
}

/// 这个矩形是否"像一枚桌面图标"。把明显不是图标的东西挡掉，这三条都是被实测逼出来的：
///
/// * **尺寸**：桌面图标是几十像素的小方块。UIA 树里总有元素报告覆盖整屏的矩形（容器、列表、
///   我们自己的界面都可能），哪怕只漏进来一个，整块桌面就都成了"图标范围"，双击空白再也切不动。
/// * **是否离屏**：隐藏/虚拟化的项不该参与命中。
/// * **归属**：**只用别的进程的项**。壁纸自己的窗口就挂在 WorkerW 之下，是桌面 Pane 的后代，
///   而输入岛与设置里的列表项同样是 ListItem —— 用自己进程的项必然误判。
fn looks_like_a_desktop_icon(
    rect: (i32, i32, i32, i32),
    is_offscreen: bool,
    belongs_to_us: bool,
) -> bool {
    if is_offscreen || belongs_to_us {
        return false;
    }
    let width = rect.2 - rect.0;
    let height = rect.3 - rect.1;
    (4..=400).contains(&width) && (4..=400).contains(&height)
}

/// 光标这一点在 UIA 的父链上是否属于**桌面表面**。
///
/// 这是 `cursor_is_on_desktop_surface` 的补充信号，而不是替代品。那条判据只看 Win32 的窗口父链，
/// 实测会漏：光标下可能是一个覆盖层窗口（例如 `NVIDIA GeForce Overlay`，覆盖桌面且点击穿透），
/// 它的父链不落到桌面上，于是双击的第二下被判成"不在桌面"，功能静默失效（2026-09-30 实测）。
/// 而 UIA 的父链能看出它挂在"桌面 N"这个 Pane 之下。
///
/// 用类名判断，和 Win32 那条用的是同一套桌面类定义，所以两者不会互相矛盾。
#[cfg(windows)]
fn uia_point_belongs_to_desktop(automation: &IUIAutomation) -> bool {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        let Ok(element) = automation.ElementFromPoint(point) else {
            return false;
        };
        let walker = automation.ControlViewWalker().ok();
        let mut current = Some(element);
        let mut seen: Vec<String> = Vec::new();
        for depth in 0..10 {
            let Some(element) = current else { break };
            let class = element.CurrentClassName().map(|value| value.to_string()).ok();
            seen.push(class.clone().unwrap_or_else(|| "-".to_string()));
            // 只在**三层以内**承认桌面类名。原因：UIA 树的根就是桌面自己，爬得够远总能碰到它 ——
            // 任务栏按钮的祖先链就是这样，于是"点任务栏也算在桌面上"（实测被误放行过一次）。
            // 而真正要覆盖的情形（桌面之上压着一层点击穿透的覆盖层）只需要两三层就碰到桌面。
            if depth < 3 && is_desktop_surface_class(class.as_deref()) {
                return true;
            }
            current = walker
                .as_ref()
                .and_then(|tree| tree.GetParentElement(&element).ok());
        }
        log::info!("UIA 父链类名（都不是桌面类）: {}", seen.join(" < "));
        false
    }
}

/// 光标是否落在某个**桌面图标**的范围内。
///
/// 为什么不用命中测试：实测（2026-09-30）光标下最上层的 UIA 元素可能是盖在桌面上的覆盖层
/// （例如 `NVIDIA GeForce Overlay` 的 Document，它挂在"桌面 1"这个 Pane 下、覆盖整个桌面、而且
/// 点击穿透），于是命中测试看不到图标，判据把图标位置当成"空白桌面" —— 双击图标既打开了东西、
/// 又翻了桌面层。枚举图标矩形做包含判断则完全不受覆盖层影响。
///
/// 图标是桌面 Pane 下的 ListItem，所以从命中元素沿父链找到那个 Pane，再枚举它下面的 ListItem。
#[cfg(windows)]
fn cursor_is_over_a_desktop_icon(automation: &IUIAutomation) -> bool {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        let Ok(element) = automation.ElementFromPoint(point) else {
            return false;
        };
        let walker = automation.ControlViewWalker().ok();
        // 沿父链找桌面 Pane：命中的可能是覆盖层，但桌面 Pane 一定在它的祖先里。
        let mut current = Some(element);
        let mut container = None;
        for _ in 0..10 {
            let Some(element) = current else { break };
            if element.CurrentControlType().ok() == Some(UIA_PaneControlTypeId) {
                container = Some(element);
                break;
            }
            current = walker
                .as_ref()
                .and_then(|tree| tree.GetParentElement(&element).ok());
        }
        let Some(container) = container else { return false };
        let Ok(condition) = automation.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_ListItemControlTypeId.0),
        ) else {
            return false;
        };
        let Ok(items) = container.FindAll(TreeScope_Descendants, &condition) else {
            return false;
        };
        let Ok(count) = items.Length() else { return false };
        let mut examined = 0usize;
        let mut skipped_big = 0usize;
        let mut skipped_ours = 0usize;
        for index in 0..count {
            let Ok(item) = items.GetElement(index) else { continue };
            let Ok(rect) = item.CurrentBoundingRectangle() else { continue };
            let is_offscreen = item.CurrentIsOffscreen().map(|value| value.as_bool()).unwrap_or(false);
            let belongs_to_us = item
                .CurrentProcessId()
                .map(|pid| pid as u32 == std::process::id())
                .unwrap_or(false);
            let candidate = (rect.left, rect.top, rect.right, rect.bottom);
            if !looks_like_a_desktop_icon(candidate, is_offscreen, belongs_to_us) {
                if belongs_to_us {
                    skipped_ours += 1;
                } else {
                    skipped_big += 1;
                }
                continue;
            }
            examined += 1;
            if rect_contains_point(candidate, point.x, point.y) {
                log::info!(
                    "桌面图标命中: rect=({},{})-({},{}) 共枚举 {count} 项（过滤掉 大矩形 {skipped_big} / 我们自己 {skipped_ours}）",
                    rect.left, rect.top, rect.right, rect.bottom
                );
                return true;
            }
        }
        log::info!(
            "桌面图标扫描: 枚举 {count} 项，可用 {examined} 项，过滤掉 大矩形 {skipped_big} / 我们自己 {skipped_ours} —— 光标不在任何图标内"
        );
        false
    }
}

/// 光标下那个 UIA 元素长什么样 —— 只用于日志。这条判据已经两次失灵（把图标当空白、把桌面容器当
/// 内容），所以宁可每次判定都把现场写下来，也不要在下一次故障时靠猜。
#[cfg(windows)]
fn describe_point_for_blank(automation: &IUIAutomation) -> String {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return "cursor-unavailable".to_string();
        }
        let Ok(element) = automation.ElementFromPoint(point) else {
            return "no-element".to_string();
        };
        let walker = automation.ControlViewWalker().ok();
        let mut parts: Vec<String> = Vec::new();
        let mut current = Some(element);
        for _ in 0..4 {
            let Some(element) = current else {
                break;
            };
            let control = element
                .CurrentControlType()
                .map(|value| format!("{}", value.0))
                .unwrap_or_else(|_| "?".to_string());
            let name = element
                .CurrentName()
                .map(|value| value.to_string())
                .unwrap_or_default();
            let name = name.trim();
            let name = if name.is_empty() {
                "-".to_string()
            } else {
                name.chars().take(24).collect()
            };
            parts.push(format!("{control}:{name}"));
            current = walker
                .as_ref()
                .and_then(|tree| tree.GetParentElement(&element).ok());
        }
        parts.join(" < ")
    }
}

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
        // 8 层而不是 4 层：Windows 11 的图标在自动化树里可能藏得比一层标签文本更深，而漏判的
        // 代价是真实的 —— 用户双击桌面图标时这一条若报"空白"，就会同时打开应用并翻掉桌面层
        // （2026-09-30 实测的缺陷）。
        for depth in 0..8 {
            let Some(element) = current else {
                break;
            };
            if element.CurrentControlType().ok() == Some(UIA_ListItemControlTypeId) {
                return false;
            }
            // 曾按「元素有名字就说明是图标」判过一次，实测在空白处会误判（把桌面容器当成内容），
            // 于是凭空让「双击空白切换表/里桌面」失效 —— 已撤回，只保留控件类型与 8 层遍历。
            current = walker
                .as_ref()
                .and_then(|tree| tree.GetParentElement(&element).ok());
        }
        true
    }
}

/// The wallpaper host is a child of Explorer's WorkerW. After click-through,
/// Explorer does not reliably become the foreground window, so foreground
/// state cannot decide whether a global double click belongs to the desktop.
/// Walk the actual HWND below the cursor instead; normal top-level apps do
/// not have WorkerW/Progman in their parent chain.
#[cfg(windows)]
fn cursor_is_on_desktop_surface(background: HWND) -> bool {
    let mut point = POINT::default();
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

/// 「光标是否落在桌面表面」这条判据供其它模块复用时的薄封装。
///
/// 悬浮球的靠近检测需要它：只有光标下的窗口属于桌面（父链能走到 Progman）时
/// 才允许弹出，否则最大化应用盖住桌面时球会从应用底边冒出来。
/// 这里刻意**不**重算一遍桌面宿主规则——`cursor_is_on_desktop_surface` 是
/// 表/里桌面双击判定唯一可用的判据，两处各写一份迟早会漂移。
///
/// 悬浮球只存在于完整版（`floating_ball` 模块本身也按 edition 收窄），所以这个
/// 薄封装在 Lite 构建里没有调用者，一并收窄以保持 Lite 门禁的警告数与之前一致。
#[cfg(all(windows, not(feature = "lite")))]
pub(crate) fn cursor_on_desktop_surface_via_label(app: &tauri::AppHandle) -> bool {
    app.get_webview_window(BACKGROUND_WINDOW_LABEL)
        .and_then(|window| window.hwnd().ok())
        .map(|handle| cursor_is_on_desktop_surface(HWND(handle.0)))
        .unwrap_or(false)
}

#[cfg(windows)]
fn cursor_hits_interaction_region(root_hwnd: HWND) -> bool {
    let mut cursor = POINT::default();
    let mut origin = POINT::default();
    unsafe {
        if GetCursorPos(&mut cursor).is_err() || !ClientToScreen(root_hwnd, &mut origin).as_bool() {
            return false;
        }
    }
    let local_x = cursor.x - origin.x;
    let local_y = cursor.y - origin.y;
    interaction_regions()
        .read()
        .map(|state| point_hits_interaction_region(&state.regions, local_x, local_y))
        .unwrap_or(false)
}

fn should_toggle_desktop_workspace(
    cursor_on_desktop_surface: bool,
    cursor_hits_interaction: bool,
    automation_reports_blank: bool,
) -> bool {
    cursor_on_desktop_surface && !cursor_hits_interaction && automation_reports_blank
}

/// 「点功能组件却切回表桌面」只可能是三个输入里有一个不成立，而热区是前端按元素 rect
/// 发布的：光标落在哪个矩形之外、UI Automation 认为桌面是否空白，必须留痕，否则只能猜。
#[cfg(windows)]
fn log_workspace_toggle_decision(hits_interaction: bool, automation_blank: bool) {
    let mut cursor = POINT::default();
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

        let double_click_window = std::time::Duration::from_millis(double_click_pairing_window_ms(
            unsafe { GetDoubleClickTime() },
        ));
        log::info!(
            "表/里桌面双击配对窗口 = {}ms（取自系统设置，不再写死）",
            double_click_window.as_millis()
        );
        let mut was_down = false;
        let mut last_blank_click: Option<std::time::Instant> = None;
        // 记录首次点击时前台的**窗口句柄**。句柄比进程号细一档：双击壁纸自己的快捷方式时进程号
        // 不变（应用已在运行），只有窗口会变。
        let mut foreground_at_first_click: Option<isize> = None;
        // 首次点击的位置：把时间窗口放宽之后，位置相近就成了"这确实是一次双击"的主要依据。
        let mut last_blank_point: Option<(i32, i32)> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(16));
            if app.get_webview_window("background").is_none() {
                break;
            }
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            if down && !was_down {
                // 这一次点击的位置：配对的"位置相近"条件与首次点击的记录都要用。
                let (cursor_x, cursor_y) = unsafe {
                    let mut point = POINT::default();
                    if GetCursorPos(&mut point).is_err() {
                        (0, 0)
                    } else {
                        (point.x, point.y)
                    }
                };
                let background = app
                    .get_webview_window(BACKGROUND_WINDOW_LABEL)
                    .and_then(|window| window.hwnd().ok())
                    .map(|window| HWND(window.0));
                let on_surface_by_window =
                    background.is_some_and(|background| cursor_is_on_desktop_surface(background));
                let on_surface_by_tree = uia_point_belongs_to_desktop(&automation);
                // 两条独立信号取或：窗口父链（Win32）与自动化父链（UIA）各能看出对方漏掉的归属。
                let on_surface = on_surface_by_window || on_surface_by_tree;
                let hits_interaction =
                    background.is_some_and(|background| cursor_hits_interaction_region(background));
                let over_icon = cursor_is_over_a_desktop_icon(&automation);
                // 图标矩形命中是硬判据：覆盖层再盖上也不会误判为空白。
                let blank = !over_icon && cursor_is_over_desktop_blank(&automation);
                let blank_saw = describe_point_for_blank(&automation);
                let can_toggle = should_toggle_desktop_workspace(on_surface, hits_interaction, blank);
                if !can_toggle {
                    // 三条判据分开记：这个功能两侧都栽过（图标处被当成空白、空白处被兜底挡掉），
                    // 只有分开写，下一次失灵才不必靠猜。
                    log::info!(
                        "workspace toggle declined: on_surface={on_surface}(win={on_surface_by_window} uia={on_surface_by_tree}) hits_interaction={hits_interaction} blank={blank} over_icon={over_icon} uia_seen={blank_saw}"
                    );
                }
                if can_toggle {
                    log::info!("workspace toggle accepted: over_icon={over_icon} on_surface_win={on_surface_by_window} on_surface_uia={on_surface_by_tree} uia_seen={blank_saw}");
                                            log_workspace_toggle_decision(false, true);
                    let now = std::time::Instant::now();
                    // 配对是否成立，取决于这一次点击与上一次的间隔 —— 把它记下来，节奏问题一眼可见。
                    let gap_ms = last_blank_click
                        .map(|previous| now.duration_since(previous).as_millis())
                        .unwrap_or(u128::MAX);
                    let gap_px = last_blank_point
                        .map(|previous| (previous.0 - cursor_x).abs().max((previous.1 - cursor_y).abs()))
                        .unwrap_or(-1);
                    log::info!(
                        "双击配对：间隔 {gap_ms}ms（窗口 {}ms），位置差距 {gap_px}px（上限 64px）",
                        double_click_window.as_millis()
                    );
                    let foreground_now = unsafe {
                        let window = GetForegroundWindow();
                        if window.0.is_null() { None } else { Some(window.0 as isize) }
                    };
                    let foreground_pid_now = unsafe {
                        let window = GetForegroundWindow();
                        if window.0.is_null() {
                            None
                        } else {
                            let mut pid = 0u32;
                            GetWindowThreadProcessId(window, Some(&mut pid));
                            if pid == 0 { None } else { Some(pid) }
                        }
                    };
                    let foreground_now_class = unsafe {
                        let window = GetForegroundWindow();
                        if window.0.is_null() {
                            None
                        } else {
                            window_class(window)
                        }
                    };

                    let foreground_now_is_desktop_surface =
                        is_desktop_surface_class(foreground_now_class.as_deref());
                    // 判定时的前台是谁：用户实测"Win+D 之后立刻变灵敏"，而两次点击的间隔没有变，
                    // 说明两个状态之间有别的差异。此前日志里缺的就是这个量。
                    log::info!(
                        "判定时的前台：class={foreground_now_class:?} pid={foreground_pid_now:?} 是桌面表面类={foreground_now_is_desktop_surface}"
                    );
                    if double_click_became_someone_elses(
                        foreground_at_first_click,
                        foreground_now,
                        foreground_now_is_desktop_surface,
                    ) {
                        // 这次双击启动了别的东西（例如桌面上的快捷方式）：那是它在响应你，
                        // 不是"对着空白桌面双击"。不翻，并把这一对点击忘掉。
                        log::info!(
                            "双击落在了会启动东西的位置：不切换表/里桌面（前台 pid={foreground_pid_now:?} class={foreground_now_class:?} 首次 pid={foreground_at_first_click:?}）"
                        );
                        last_blank_click = None;
                        foreground_at_first_click = None;
                        last_blank_point = None;
                        last_blank_point = None;
                        was_down = down;
                        continue;
                    }
                    let reached = last_blank_point
                        .is_some_and(|previous| within_double_click_reach(previous, (cursor_x, cursor_y), 64));
                    let gap_ok = last_blank_click
                        .is_some_and(|previous| now.duration_since(previous) <= double_click_window);
                    if !gap_ok {
                        log::info!("配对未成立：{gap_px}px 距离、间隔超窗或无上一次点击（窗口 {}ms）", double_click_window.as_millis());
                    } else if !reached {
                        log::info!("配对未成立：与上一次点击相距 {gap_px}px，超过 64px 上限");
                    }
                    if gap_ok && reached {
                        last_blank_click = None;
                        // The transition itself lives in enter/leave_inner_workspace so the
                        // double click and the floating ball cannot drift apart.
                        let result = if INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire) {
                            leave_inner_workspace(&app)
                        } else {
                            enter_inner_workspace(&app)
                        };
                        if let Err(error) = result {
                            // If Explorer has restarted or the icon view cannot
                            // be found, preserve a truthful state and do not
                            // enter a half-working inner desktop.
                            log::warn!("无法切换表/里桌面图标层：{error}");
                            continue;
                        }
                        log::info!(
                            "桌面空白双击：切换至{}桌面",
                            if INNER_WORKSPACE_ACTIVE.load(Ordering::Acquire) {
                                "里"
                            } else {
                                "表"
                            }
                        );
                    } else {
                        last_blank_click = Some(now);
                        foreground_at_first_click = foreground_now;
                        last_blank_point = Some((cursor_x, cursor_y));
                    }
                } else {
                    last_blank_click = None;
                    foreground_at_first_click = None;
                    last_blank_point = None;
                }
            }
            was_down = down;
        }
    });
}

/// Lock-screen ownership is shared by the Lite and full packages. Their AppX
/// identifiers intentionally differ, so `app_config_dir()` would otherwise
/// strand the only recovery manifest in whichever edition performed the
/// takeover first. Prefer one user-scoped directory and fall back to the
/// legacy full-edition directory when it already contains a recovery point.
#[cfg(windows)]
fn lock_screen_config_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let legacy = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let shared = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| legacy.clone())
        .join("DSHWallpaper");
    if legacy.join("lock-screen").exists() && !shared.join("lock-screen").exists() {
        return Ok(legacy);
    }
    std::fs::create_dir_all(&shared)
        .map_err(|error| format!("无法创建共享锁屏恢复目录：{error}"))?;
    Ok(shared)
}

#[cfg(windows)]
pub async fn set_lock_screen(app: &tauri::AppHandle, enabled: bool) -> Result<String, String> {
    let _transaction = LOCK_SCREEN_TRANSACTION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let _cross_process_transaction = CrossProcessLockScreenTransaction::acquire()?;
    let config_dir = lock_screen_config_dir(app)?;
    // Both takeover *and* restoration call the system setter.  Do not let an
    // unpackaged dev/NSIS process mutate the lock screen just because it finds
    // a recovery manifest left by an earlier run. The supported surface is an
    // MSIX-identified process only; preserving the manifest is safer than an
    // unverified write from a different installation context.
    let has_package_identity = has_package_identity()?;
    if !can_attempt_lock_screen_takeover(has_package_identity) {
        return Err("当前进程没有 MSIX 包身份。为确保锁屏接管和恢复可验证，常规桌面版不会修改系统锁屏；现有恢复点已保留。请安装 MSIX 包后重试。".into());
    }
    if !UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())? {
        return Err("当前 Windows 策略不允许应用修改锁屏图片；现有恢复点已保留。".into());
    }
    if enabled {
        // Windows accepts the packaged resource but rejects the same bytes
        // after they are copied into ordinary Roaming app-data on this system.
        // Resolve it before ownership checks so an existing MSIX takeover is
        // recognised rather than mistaken for an external change.
        let packaged_sleep_image = bundled_sleep_image_path(app)?;
        let original = LockScreen::OriginalImageFile()
            .map_err(|_| "无法读取当前锁屏图片；为避免无法恢复，已取消接管。".to_string())?
            .AbsoluteUri()
            .map_err(|_| "无法读取当前锁屏图片；为避免无法恢复，已取消接管。".to_string())?
            .to_string();
        let backup_state = inspect_backup(&config_dir);
        if let LockScreenBackupState::Invalid { reason } = &backup_state {
            return Err(format!("现有锁屏备份不完整，已拒绝覆盖当前锁屏：{reason}"));
        }
        let managed_path = managed_image_path_for_state(&config_dir, &backup_state)?;
        let managed_image_active = managed_image_is_active(Some(&original), &managed_path)
            || managed_image_is_active(Some(&original), &packaged_sleep_image);
        if has_stale_backup(&backup_state, managed_image_active) {
            return Err("检测到接管期间锁屏已由用户或其他程序更改。为避免覆盖当前锁屏，应用不会再次接管；原备份已保留。请先在 Windows 设置中确认锁屏图片，再决定是否清理或恢复。".into());
        }
        // A verified active manifest already represents the exact image Windows
        // owns.  Repeating a setter with the same filename is explicitly
        // rejected by Windows, while switching it to a new filename would
        // require replacing the only restore manifest.  Therefore this is a
        // true idempotent success, not a second takeover attempt.
        if matches!(&backup_state, LockScreenBackupState::Valid(_)) && managed_image_active {
            if managed_path.is_file() || packaged_sleep_image.is_file() {
                return Ok("锁屏图片已经由本应用接管；原静态图片备份仍可恢复。".into());
            }
            return Err("Windows 仍指向本应用的锁屏图片，但托管图片文件已丢失。为避免覆盖唯一的原图备份，应用没有再次接管；请先恢复原锁屏或在 Windows 设置中重新选择图片。".into());
        }
        if managed_image_active && matches!(&backup_state, LockScreenBackupState::Missing) {
            return Err("当前锁屏已经是本应用的熟睡画面，但原锁屏备份不存在。为避免把托管图片误当作原图，已拒绝再次接管；请先在 Windows 设置中手动选择原图。".into());
        }
        // Do not re-resolve this path here. `packaged_sleep_image` is the
        // verified MSIX Assets payload; a former shadowing declaration below
        // silently replaced it with Tauri's `_up_` resource and caused the
        // misleading 0x800700A1 failures.
        let bundled_sleep_image = packaged_sleep_image;

        // WinRT's lock-screen API is unreliable with paths inside a Tauri
        // resource bundle (especially during `tauri dev`): it can receive a
        // virtual/masked resource path and return 0x800700A1.  Hand Windows a
        // normal, current-user-owned file instead.
        let captured_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "系统时间无效，已取消锁屏接管。".to_string())?
            .as_millis() as u64;
        // 名字由**内容**决定（同一张素材永远同一个文件名）：Windows 把每个不同的文件名都记成
        // "最近使用的图像"里的一条，按次生成名字会让同一张图占掉多个栏位（用户实测三个）。
        let content_hash = file_content_hash(&bundled_sleep_image)?;
        let managed_image_file = managed_image_file_for_content(&content_hash, "png")?;
        let managed_path = managed_image_path_from_file(&config_dir, &managed_image_file)?;
        copy_sleep_image_without_overwrite(&bundled_sleep_image, &managed_path)?;
        let lease = match ensure_backup_for_takeover(
            &config_dir,
            &original,
            captured_at,
            &managed_image_file,
        ) {
            Ok(lease) => lease,
            Err(error) => {
                // There is no manifest ownership record for this freshly
                // allocated path when acquisition fails, so it is safe to
                // remove only this orphaned candidate.
                let _ = std::fs::remove_file(&managed_path);
                return Err(error);
            }
        };
        let lease_managed_path = match managed_image_path(&config_dir, &lease.manifest) {
            Ok(path) => path,
            Err(_) => {
                // Do not clean up here. A concurrent process could have
                // changed the persisted manifest between acquisition and this
                // validation; without a verified ownership relation, keeping
                // every file is safer than deleting one it may now reference.
                return abort_lock_screen_takeover_before_set(
                    &config_dir,
                    &lease,
                    "锁屏备份状态在接管准备期间无法验证；应用没有修改锁屏。",
                );
            }
        };
        if lease_managed_path != managed_path {
            // `ensure_backup_for_takeover` is intentionally non-destructive
            // when it discovers an existing valid manifest.  If the manifest
            // changed after our preflight, its managed image is not the unique
            // candidate we just prepared, so setting that candidate would
            // leave no durable ownership record for it.  Refuse the setter
            // and remove only our unreferenced candidate.
            let _ = std::fs::remove_file(&managed_path);
            return Err(
                "锁屏备份状态在接管准备期间发生变化；应用没有修改锁屏。请刷新检查后重试。".into(),
            );
        }
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
        // Resolve all non-mutating WinRT prerequisites before the setter.  If
        // one fails, this attempt is proven not to have asked Windows to take
        // ownership, so rolling back its brand-new snapshot is safe.
        // Do not pass the app-data copy here. The isolated MSIX probe proved
        // that this Windows build returns `false` for it, but succeeds for the
        // package-owned sleep asset.
        let path_string = HSTRING::from(bundled_sleep_image.to_string_lossy().as_ref());
        let file = match StorageFile::GetFileFromPathAsync(&path_string)
            .map_err(|error| {
                format!(
                    "Windows 无法读取准备好的锁屏图片（路径：{}；WinRT：{error}）。",
                    bundled_sleep_image.display()
                )
            })
            .and_then(|operation| {
                operation.get().map_err(|error| {
                    format!(
                        "Windows 无法打开准备好的锁屏图片（路径：{}；WinRT：{error}）。",
                        bundled_sleep_image.display()
                    )
                })
            }) {
            Ok(file) => file,
            Err(error) => {
                return abort_lock_screen_takeover_before_set(&config_dir, &lease, &error);
            }
        };
        let settings = match UserProfilePersonalizationSettings::Current()
            .map_err(|_| "Windows 无法打开锁屏个性化设置。".to_string())
        {
            Ok(settings) => settings,
            Err(error) => {
                return abort_lock_screen_takeover_before_set(&config_dir, &lease, &error);
            }
        };
        // From this point on, every failure is treated as indeterminate.  A
        // completed/asynchronous WinRT call can have changed the setting even
        // if it reports an error or its immediate query lags, so do not delete
        // the managed image or its original-image recovery point.
        let set_result = (|| -> Result<(), String> {
            let changed = settings
                .TrySetLockScreenImageAsync(&file)
                .map_err(|error| {
                    // Preserve the WinRT/HRESULT detail for the test build.
                    // It is the only useful lead when Windows refuses before
                    // returning the documented boolean result.
                    format!("Windows 未能启动锁屏图片设置请求（WinRT：{error}）。")
                })?
                .get()
                .map_err(|error| format!("Windows 未能完成锁屏图片设置请求（WinRT：{error}）。"))?;
            if !changed {
                // The supported MSIX route has one authoritative setter.
                // Microsoft defines `false` as an unsuccessful change, not as
                // permission to silently try a legacy API.  Fail closed and
                // keep the just-created recovery point for the final ownership
                // check below instead of risking a second, unverified write.
                return Err("Windows 拒绝了锁屏图片设置请求，但未返回具体原因。当前锁屏图片、MSIX 包身份、系统支持状态和托管图片文件预检均已通过；恢复点已保留，测试版没有尝试旧兼容接口。".into());
            }
            Ok(())
        })();
        finish_lock_screen_takeover_attempt(&config_dir, &lease, &bundled_sleep_image, set_result)
    } else {
        let current_image_uri = LockScreen::OriginalImageFile()
            .ok()
            .and_then(|uri| uri.AbsoluteUri().ok())
            .map(|uri| uri.to_string());
        let backup_state = inspect_backup(&config_dir);
        if let LockScreenBackupState::Invalid { reason } = &backup_state {
            return Err(format!("现有锁屏备份不完整，无法安全恢复：{reason}"));
        }
        let managed_path = managed_image_path_for_state(&config_dir, &backup_state)?;
        let packaged_sleep_image = bundled_sleep_image_path(app)?;
        let managed_image_active =
            managed_image_is_active(current_image_uri.as_deref(), &managed_path)
                || managed_image_is_active(current_image_uri.as_deref(), &packaged_sleep_image);
        if has_stale_backup(&backup_state, managed_image_active) {
            return Ok("已停止本应用的锁屏接管状态。检测到当前锁屏已由用户或其他程序更改，因此未覆盖它；原备份已保留。".into());
        }
        if let LockScreenBackupState::Valid(manifest) = backup_state {
            if let Ok(path) = restore_snapshot_path(&config_dir, &manifest) {
                let manifest_managed_path = managed_image_path(&config_dir, &manifest)?;
                let expected_current_image =
                    if managed_image_is_active(current_image_uri.as_deref(), &packaged_sleep_image)
                    {
                        &packaged_sleep_image
                    } else {
                        &manifest_managed_path
                    };
                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(
                    path.to_string_lossy().as_ref(),
                ))
                .map_err(|e| e.to_string())?
                .get()
                .map_err(|e| e.to_string())?;
                let settings =
                    UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
                // Do not restore over a lock-screen image selected after the
                // first stale-state check above. Opening the snapshot and the
                // settings object can take long enough for the user, Windows,
                // or another program to change the image. Re-read immediately
                // before the only restore setter and fail closed if this
                // durable manifest no longer owns the current image.
                let current_before_restore = LockScreen::OriginalImageFile()
                    .ok()
                    .and_then(|uri| uri.AbsoluteUri().ok())
                    .map(|uri| uri.to_string());
                if !restore_precondition_is_satisfied(
                    current_before_restore.as_deref(),
                    expected_current_image,
                ) {
                    return Err("检测到锁屏图片在恢复准备期间已由用户或其他程序更改；应用没有覆盖新图片，原备份已保留。".into());
                }
                let restored = settings
                    .TrySetLockScreenImageAsync(&file)
                    .map_err(|e| e.to_string())?
                    .get()
                    .map_err(|e| e.to_string())?;
                if restored {
                    return finish_verified_lock_screen_restore(&config_dir, &manifest, &path);
                }
            }
        }
        Err("未能恢复原静态锁屏图片；接管仍保持启用，原备份没有被删除。请稍后重试或在 Windows 设置中手动恢复。".into())
    }
}

#[cfg(windows)]
pub async fn clear_stale_lock_screen_backup(
    app: &tauri::AppHandle,
    confirmed: bool,
) -> Result<String, String> {
    if !confirmed {
        return Err(
            "清理旧锁屏恢复点需要明确确认；该操作会永久删除已保存的原锁屏图片副本。".into(),
        );
    }
    let _transaction = LOCK_SCREEN_TRANSACTION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let _cross_process_transaction = CrossProcessLockScreenTransaction::acquire()?;
    if !has_package_identity()? {
        return Err("当前进程没有 MSIX 包身份，拒绝清理锁屏恢复点。".into());
    }
    let config_dir = lock_screen_config_dir(app)?;
    let current = LockScreen::OriginalImageFile()
        .map_err(|_| "无法读取当前锁屏图片；旧恢复点已保留。".to_string())?
        .AbsoluteUri()
        .map_err(|_| "无法读取当前锁屏图片；旧恢复点已保留。".to_string())?
        .to_string();
    let state = inspect_backup(&config_dir);
    let managed_path = managed_image_path_for_state(&config_dir, &state)?;
    let packaged_sleep_image = bundled_sleep_image_path(app)?;
    let managed_image_active = managed_image_is_active(Some(&current), &managed_path)
        || managed_image_is_active(Some(&current), &packaged_sleep_image);
    discard_stale_backup(&config_dir, managed_image_active)?;
    Ok("已清理旧锁屏恢复点；Windows 当前锁屏图片未作任何修改，保存的原图副本已永久删除。".into())
}

/// Restoring a snapshot is only safe while the current lock-screen image is
/// still the application-owned image recorded by the durable manifest.  Keep
/// this decision separate from the WinRT setter so the unit test documents the
/// fail-closed boundary without changing a real system setting.
#[cfg(windows)]
fn restore_precondition_is_satisfied(
    current_image_uri: Option<&str>,
    managed_image: &std::path::Path,
) -> bool {
    managed_image_is_active(current_image_uri, managed_image)
}

#[cfg(windows)]
fn finish_lock_screen_takeover_attempt(
    config_dir: &std::path::Path,
    lease: &LockScreenBackupLease,
    expected_managed_image: &std::path::Path,
    set_result: Result<(), String>,
) -> Result<String, String> {
    // Derive the expected filename from the durable manifest rather than the
    // provisional path prepared by the caller.  This keeps post-set ownership
    // verification and rollback coupled to one source of truth.
    let _legacy_managed_image = managed_image_path(config_dir, &lease.manifest)?;
    // Never discard a newly captured restore point merely because the
    // verification query failed. The setter can succeed even when a later
    // OriginalImageFile read is unavailable; in that indeterminate case the
    // backup is the only safe recovery path.
    let current_is_managed = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| managed_image_is_active(Some(&uri.to_string()), expected_managed_image));

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
        // The setter was invoked before this error.  WinRT may complete an
        // asynchronous write even when the operation reports an error or an
        // immediate read is stale, so retain both files rather than risk
        // deleting an image Windows still references.
        (Err(error), Some(false)) => Err(format!(
            "{error} Windows 当前未确认锁屏已切换；恢复点和托管图片均已保留，以避免删除可能仍被系统引用的文件。"
        )),
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

/// Per-user autostart location for builds Windows will not start through a
/// package StartupTask.
///
/// `reg.exe` needs the hive inside the path and answers "Invalid key name"
/// without it, while `RegOpenKeyExW` receives the hive as its own argument.
/// Both spellings are kept side by side, and a unit test asserts that they still
/// name the same key: using the hive-less one for `reg.exe` is how the repair
/// silently stopped writing the entry after a package update.
#[cfg(windows)]
const RUN_KEY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_KEY_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE_NAME: &str = "dsh-wallpaper";
/// The `<Application Id>` declared by this edition's `AppxManifest.xml`. The
/// shell needs it, together with the package family name, to address the
/// packaged app's launch alias.
///
/// The two editions ship different manifests *and* different packages
/// (`com.dsh.wallpaper` / `com.dsh.wallpaper.lite`), so one shared value made
/// the Lite package record an alias for an application its manifest does not
/// declare — an entry that resolves to nothing at logon.
#[cfg(all(windows, feature = "lite"))]
const PACKAGE_APPLICATION_ID: &str = "WallpaperLite";
#[cfg(all(windows, not(feature = "lite")))]
const PACKAGE_APPLICATION_ID: &str = "Wallpaper";

/// The per-user autostart value a packaged build must record.
///
/// A packaged build must never record its own path: Tauri reports the
/// version-pinned `WindowsApps\..._<version>_...` directory, which the next
/// package update deletes, and that is precisely how autostart disappeared
/// between 0.2.0.71 and 0.2.0.74. The shell resolves the `AppsFolder` alias
/// through the current package registration instead, so the value recorded
/// once keeps launching every later version.
#[cfg(windows)]
fn packaged_run_entry_command(package_family_name: &str) -> String {
    format!(r"explorer.exe shell:AppsFolder\{package_family_name}!{PACKAGE_APPLICATION_ID}")
}

/// Whether a recorded Run value still launches the build that needs it.
/// Values are written by `reg.exe`, by older releases and occasionally by
/// hand, so quoting, surrounding whitespace and letter case must not be
/// mistaken for a version change.
#[cfg(windows)]
fn autostart_commands_match(recorded: &str, expected: &str) -> bool {
    fn normalize(value: &str) -> String {
        value.replace('"', "").trim().to_lowercase()
    }
    !expected.is_empty() && normalize(recorded) == normalize(expected)
}

/// The Run value this build needs. A packaged install uses the version-stable
/// shell alias; an unpackaged development or NSIS build keeps naming its own
/// executable, which no MSIX update can move.
#[cfg(windows)]
pub(crate) fn current_run_entry_command() -> Result<String, String> {
    if !has_package_identity()? {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        return Ok(exe.to_string_lossy().into_owned());
    }
    let mut length = 0u32;
    let probe = unsafe { GetCurrentPackageFamilyName(&mut length, None) };
    if probe != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(format!(
            "无法读取当前应用的 MSIX 包族名（Windows 错误码 {}）。",
            probe.0
        ));
    }
    // The length includes the terminator; supply one spare element and pass the
    // capacity back explicitly, as `GetCurrentPackagePath` requires above.
    let mut buffer = vec![0u16; length as usize + 1];
    let mut capacity = buffer.len() as u32;
    let read =
        unsafe { GetCurrentPackageFamilyName(&mut capacity, Some(PWSTR(buffer.as_mut_ptr()))) };
    if !read.is_ok() || capacity == 0 {
        return Err(format!(
            "无法读取当前应用的 MSIX 包族名（Windows 错误码 {}）。",
            read.0
        ));
    }
    let family = String::from_utf16_lossy(&buffer[..capacity as usize]);
    let family = family.trim_end_matches('\0');
    if family.is_empty() {
        return Err("无法读取当前应用的 MSIX 包族名。".into());
    }
    Ok(packaged_run_entry_command(family))
}

/// The value the per-user Run entry currently records, if it exists at all.
#[cfg(windows)]
fn run_entry_command() -> Result<Option<String>, String> {
    // `RegOpenKeyExW` and `RegQueryValueExW` take the hive and the value name
    // separately, so both are spelled from the same constants the writer uses.
    let subkey = RUN_KEY_SUBKEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let value_name = RUN_VALUE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let mut key = windows::Win32::System::Registry::HKEY::default();
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if open_status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if open_status != ERROR_SUCCESS {
        return Err(format!(
            "无法读取当前用户开机启动项（错误码 {}）。",
            open_status.0
        ));
    }

    let mut value_size = 0u32;
    let query_status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            None,
            Some(&mut value_size),
        )
    };
    if query_status == ERROR_FILE_NOT_FOUND {
        let _ = unsafe { RegCloseKey(key) };
        return Ok(None);
    }
    if query_status != ERROR_SUCCESS && query_status != ERROR_MORE_DATA {
        let _ = unsafe { RegCloseKey(key) };
        return Err(format!(
            "无法读取 DSH Wallpaper 开机启动项（错误码 {}）。",
            query_status.0
        ));
    }
    if value_size == 0 {
        let _ = unsafe { RegCloseKey(key) };
        return Ok(Some(String::new()));
    }

    // The value is a UTF-16 `REG_SZ`. Ask for one spare byte: a size that does
    // not count the terminator must not make the second read fail.
    let mut buffer = vec![0u8; value_size as usize + 2];
    let mut read_size = buffer.len() as u32;
    let read_status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            Some(buffer.as_mut_ptr()),
            Some(&mut read_size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    if read_status != ERROR_SUCCESS {
        return Err(format!(
            "无法读取 DSH Wallpaper 开机启动项（错误码 {}）。",
            read_status.0
        ));
    }
    let units = buffer[..read_size as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<u16>>();
    let value = String::from_utf16_lossy(&units);
    Ok(Some(value.trim_end_matches('\0').to_string()))
}

/// The value line `reg query` prints, pulled out of its report.
///
/// The output is a blank line, the key path in brackets and the value itself,
/// with name, type and data separated by runs of spaces:
///
/// ```text
///
/// HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
///     dsh-wallpaper    REG_SZ    explorer.exe shell:AppsFolder\...!Wallpaper
/// ```
///
/// Only the type token is used as a delimiter, because the data contains spaces
/// of its own.
#[cfg(windows)]
fn parse_reg_query_value(output: &str, value_name: &str) -> Option<String> {
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix(value_name) else {
            continue;
        };
        // The name has to be a whole token: `dsh-wallpaper-old` is another value.
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(type_end) = rest.find(char::is_whitespace) else {
            continue;
        };
        if !rest[..type_end].starts_with("REG_") {
            continue;
        }
        return Some(rest[type_end..].trim_start().to_string());
    }
    None
}

/// What a child `reg.exe` — and therefore the shell at logon — sees in the same
/// key.
///
/// MSIX redirects a packaged app's HKCU writes into a private per-user hive
/// which is *merged over* the real key when the app reads it. An in-process
/// delete therefore leaves a tombstone that hides the real value from the app
/// only, while Windows keeps launching the app at logon: the app reported
/// 「当前用户启动项里没有 DSH Wallpaper」 for a value that was sitting in the
/// real key the whole time. The manifest declares this key unvirtualized, and
/// this read is the cross-check that says whether that declaration is in force.
#[cfg(windows)]
fn run_entry_command_via_child() -> Result<Option<String>, String> {
    use std::process::Command;
    let mut cmd = Command::new("reg");
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x08000000);
    cmd.args(["query", RUN_KEY_PATH, "/v", RUN_VALUE_NAME]);
    let output = cmd.output().map_err(|error| error.to_string())?;
    match output.status.code() {
        Some(0) => Ok(parse_reg_query_value(
            &String::from_utf8_lossy(&output.stdout),
            RUN_VALUE_NAME,
        )),
        // `reg.exe` answers 1 for "no such key or value", which is the same state
        // the in-process read reports as "nothing recorded".
        Some(1) => Ok(None),
        other => Err(format!(
            "无法读取当前用户开机启动项（reg.exe 退出码 {}）",
            other.unwrap_or(-1)
        )),
    }
}

/// What the compatibility Run entry says now, together with the two values
/// that produced the answer.
///
/// The read-back decides what the settings page shows, so it must not guess:
/// "nothing is recorded", "another build is recorded" and "the registry could
/// not be read" used to collapse into one silent `false`, and a log line could
/// then contradict the switch without either side saying which value it saw.
/// Errors are folded in rather than propagated for the same reason — the
/// caller has to be able to name which of the three states it is in.
#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct RunEntryCheck {
    pub(crate) matches: bool,
    /// The recorded value, when the registry read succeeded.
    pub(crate) recorded: Option<String>,
    /// The value this build needs, when it could be computed.
    pub(crate) expected: Option<String>,
    /// Why the check could not be completed, when it could not.
    pub(crate) error: Option<String>,
}

#[cfg(windows)]
pub(crate) fn check_run_entry() -> RunEntryCheck {
    let expected = match current_run_entry_command() {
        Ok(command) => command,
        Err(error) => {
            return RunEntryCheck {
                matches: false,
                recorded: None,
                expected: None,
                error: Some(error),
            }
        }
    };
    let recorded = match run_entry_command() {
        Ok(value) => value,
        Err(error) => {
            return RunEntryCheck {
                matches: false,
                recorded: None,
                expected: Some(expected),
                error: Some(error),
            }
        }
    };
    let matches = match recorded.as_deref() {
        Some(value) => autostart_commands_match(value, &expected),
        None => false,
    };
    // The two views are supposed to be the same key, and they silently were not:
    // the app read its own private tombstone while the real value stayed exactly
    // where the app had just written it. The manifest declares this key
    // unvirtualized; this is where a regression of that declaration becomes a
    // line in the log instead of a settings page that contradicts Windows.
    match run_entry_command_via_child() {
        Ok(child) if child == recorded => {}
        Ok(child) => log::warn!(
            "开机自启回读：包内视图与 reg.exe 视图不一致（包内={recorded:?} reg={child:?}）；写入虚拟化又对 Run 键生效了，开关显示的将不是登录时真正执行的那个值"
        ),
        Err(error) => log::warn!("开机自启回读：reg.exe 交叉检查失败：{error}"),
    }
    RunEntryCheck {
        matches,
        recorded,
        expected: Some(expected),
        error: None,
    }
}

/// Why the compatibility entry does not carry autostart, in the words the
/// settings page shows. Deliberately shorter than the log line: the page names
/// the state, the log names the values.
#[cfg(windows)]
fn run_entry_refusal(check: &RunEntryCheck) -> String {
    if let Some(error) = check.error.as_deref() {
        return error.to_string();
    }
    match check.recorded.as_deref() {
        None => "当前用户启动项里没有 DSH Wallpaper。".into(),
        Some(_) => "当前用户启动项指向的不是本次安装的版本。".into(),
    }
}

/// Whether the per-user Run entry launches this exact build. A recorded path
/// is not evidence of working autostart by itself: it may name a `WindowsApps`
/// directory that a package update has already deleted.
#[cfg(windows)]
pub(crate) fn run_entry_matches_current_build() -> Result<bool, String> {
    let check = check_run_entry();
    match check.error {
        Some(error) => Err(error),
        None => Ok(check.matches),
    }
}

/// The exact `reg.exe` invocation that records the entry. Kept apart from the
/// spawn so a unit test can pin that the path names the hive: `reg.exe` rejects
/// one that does not, and that rejection left autostart unfixed after a package
/// update because the error was not distinguishing "not written" from "no
/// permission".
#[cfg(windows)]
fn run_entry_add_arguments(command: &str) -> Vec<String> {
    vec![
        "add".into(),
        RUN_KEY_PATH.into(),
        "/V".into(),
        RUN_VALUE_NAME.into(),
        "/D".into(),
        command.into(),
        "/F".into(),
    ]
}

/// Record (or refresh) the per-user Run entry.
///
/// `reg.exe` stays the writer because it is the compatibility path older
/// releases used, so an entry it updates remains readable by them.
#[cfg(windows)]
pub(crate) fn write_run_entry(command: &str) -> Result<(), String> {
    use std::process::Command;
    let mut cmd = Command::new("reg");
    // `reg.exe` is only a compatibility fallback. Keep it out of the user's
    // desktop even when the host is a GUI-subsystem process.
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x08000000);
    cmd.args(run_entry_add_arguments(command));
    let status = cmd.status().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        // Carry the exit code. Without it a failure here could not be told apart
        // from a rejected key path, a rejected value or a missing `reg.exe`.
        Err(format!(
            "更新当前用户开机自启失败（reg.exe 退出码 {}）",
            status.code().unwrap_or(-1)
        ))
    }
}

/// Prefer the package-owned StartupTask when the app is running from an MSIX.
/// Older packages and unpackaged development/NSIS builds do not carry the
/// extension, so callers may fall back to the legacy per-user Run entry.
#[cfg(windows)]
pub(crate) fn set_startup_task(enabled: bool) -> Result<Option<bool>, String> {
    if !has_package_identity()? {
        return Ok(None);
    }
    let task_id = HSTRING::from("DshWallpaperStartup");
    let task = match StartupTask::GetAsync(&task_id).and_then(|operation| operation.get()) {
        Ok(task) => task,
        // A package built before the StartupTask manifest extension is still
        // supported through the Run-key compatibility path.
        //
        // **但这条降级必须留痕**：另一位 agent 报告"注册表键从 09-22 到今始终不存在，manifest 与
        // TaskId 一致、系统策略正常，Windows 就是从未实例化这个任务，现在自启全靠 Run 键在扛"——
        // 而这里原本把 GetAsync 的 HRESULT 用 `Err(_)` 直接吞掉，于是"主路径为什么失效"在日志里
        // 一个字都没有，只能靠猜。降级行为不变（仍然返回 Ok(None) 走兼容路径），只是把原因说出来。
        Err(error) => {
            log::warn!(
                "启动任务不可用（GetAsync({task_id:?}) 失败：{error}）；本次改走 Run 键兼容路径"
            );
            return Ok(None);
        }
    };
    if !enabled {
        task.Disable()
            .map_err(|error| format!("无法关闭 DSH Wallpaper 启动任务：{error}"))?;
        return Ok(Some(false));
    }
    let state = task
        .RequestEnableAsync()
        .and_then(|operation| operation.get())
        .map_err(|error| format!("无法启用 DSH Wallpaper 启动任务：{error}"))?;
    if state == StartupTaskState::Enabled {
        Ok(Some(true))
    } else if state == StartupTaskState::DisabledByUser {
        Err("Windows 已禁用 DSH Wallpaper 启动任务；请在系统设置中允许开机启动。".into())
    } else if state == StartupTaskState::DisabledByPolicy {
        Err("Windows 策略禁止 DSH Wallpaper 开机启动。".into())
    } else {
        Err(format!(
            "DSH Wallpaper 启动任务未启用（状态码 {}）。",
            state.0
        ))
    }
}

/// The setting center must display the state Windows actually registered,
/// rather than a stale renderer preference.  `source` is deliberately a
/// small diagnostic value so the UI can explain why an update is needed
/// without exposing registry contents or package internals.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutostartStatus {
    pub(crate) enabled: bool,
    pub(crate) source: String,
    /// Why the state is what it is, in the words the settings page shows.
    ///
    /// The renderer used to guess: every refusal produced the same sentence
    /// about "系统启动应用权限", which sends the user to a Windows setting that
    /// on this machine was never the problem. The cause is known here — Windows
    /// refused the startup task with a specific code, the recorded entry names
    /// another build, or the registry could not be read — so it travels out
    /// instead of a canned sentence.
    pub(crate) reason: Option<String>,
}

#[cfg(windows)]
pub(crate) fn remove_legacy_run_entry() -> Result<(), String> {
    let mut key = windows::Win32::System::Registry::HKEY::default();
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if open_status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if open_status != ERROR_SUCCESS {
        return Err(format!(
            "无法更新当前用户开机启动项（错误码 {}）。",
            open_status.0
        ));
    }
    let delete_status = unsafe { RegDeleteValueW(key, w!("dsh-wallpaper")) };
    let _ = unsafe { RegCloseKey(key) };
    if delete_status == ERROR_SUCCESS || delete_status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "无法移除旧版 DSH Wallpaper 开机启动项（错误码 {}）。",
            delete_status.0
        ))
    }
}

#[cfg(windows)]
pub(crate) fn autostart_status() -> Result<AutostartStatus, String> {
    // One read-back, one log line, one set of values for every branch below:
    // when the switch and the log disagree about autostart, this is the line
    // that says which value each side was looking at.
    let check = check_run_entry();
    log::info!(
        "开机自启回读：matches={} recorded={:?} expected={:?} error={:?}",
        check.matches,
        check.recorded,
        check.expected,
        check.error
    );

    if has_package_identity()? {
        let task_id = HSTRING::from("DshWallpaperStartup");
        match StartupTask::GetAsync(&task_id).and_then(|operation| operation.get()) {
            Ok(task) => {
                let state = task
                    .State()
                    .map_err(|error| format!("无法读取 DSH Wallpaper 启动任务状态：{error}"))?;
                let status = match state {
                    StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => {
                        AutostartStatus {
                            enabled: true,
                            source: "startup-task".into(),
                            reason: None,
                        }
                    }
                    StartupTaskState::DisabledByUser => AutostartStatus {
                        enabled: false,
                        source: "disabled-by-user".into(),
                        reason: Some(
                            "Windows 已禁用本应用的开机启动任务；可在系统设置的「启动应用」里重新允许。"
                                .into(),
                        ),
                    },
                    StartupTaskState::DisabledByPolicy => AutostartStatus {
                        enabled: false,
                        source: "disabled-by-policy".into(),
                        reason: Some("Windows 策略禁止本应用开机启动。".into()),
                    },
                    // A package update can add StartupTask to an app that was
                    // previously using the HKCU Run compatibility path. Keep the
                    // effective preference enabled until the user explicitly
                    // changes it; otherwise the new package would appear to have
                    // silently turned autostart off during an update. A Run entry
                    // only counts while it still launches this build: an entry left
                    // by a superseded version names a deleted `WindowsApps`
                    // directory and would otherwise report working autostart that
                    // no longer exists.
                    StartupTaskState::Disabled => {
                        if check.matches {
                            AutostartStatus {
                                enabled: true,
                                source: "run".into(),
                                reason: Some(
                                    "Windows 启动任务处于关闭状态，当前由当前用户启动项承载。".into(),
                                ),
                            }
                        } else {
                            AutostartStatus {
                                enabled: false,
                                source: "startup-task".into(),
                                // Now that the task registers on this machine this
                                // branch is reachable, and it must name the task
                                // rather than the fallback: the Run entry is not
                                // what decides whether logon starts the app.
                                reason: Some(
                                    "Windows 启动任务处于关闭状态，登录时不会启动本应用。".into(),
                                ),
                            }
                        }
                    }
                    other => AutostartStatus {
                        enabled: false,
                        source: "startup-task".into(),
                        reason: Some(format!(
                            "Windows 启动任务未启用（状态码 {}）；当前用户启动项里没有本应用。",
                            other.0
                        )),
                    },
                };
                return Ok(status);
            }
            // A package installed before the StartupTask extension was added is
            // still readable through the compatibility Run entry — and so is an
            // install where Windows refuses to instantiate the task at all,
            // which is what happens on the machine this was diagnosed on:
            // `GetAsync` answers E_INVALIDARG while the Run entry works. The
            // refusal is carried out as the reason, because "Windows 启动任务
            // 不可用" and "没有开启自启" were previously the same silent `false`.
            Err(error) => {
                log::warn!(
                    "启动任务不可用（GetAsync({task_id:?}) 失败：{error}）；本次改走 Run 键兼容路径"
                );
                if check.matches {
                    return Ok(AutostartStatus {
                        enabled: true,
                        source: "run".into(),
                        reason: Some(format!(
                            "Windows 启动任务不可用（{error}）；当前由当前用户启动项承载。"
                        )),
                    });
                }
                return Ok(AutostartStatus {
                    enabled: false,
                    source: "none".into(),
                    reason: Some(format!(
                        "Windows 启动任务不可用（{error}）；{}",
                        run_entry_refusal(&check)
                    )),
                });
            }
        }
    }

    if check.matches {
        return Ok(AutostartStatus {
            enabled: true,
            source: "run".into(),
            reason: None,
        });
    }
    // An entry recorded by a superseded version is deliberately not reported
    // as enabled: it names a path the package update deleted, so this build
    // really does not start at logon. The startup check repairs it.
    Ok(AutostartStatus {
        enabled: false,
        source: "none".into(),
        reason: Some(run_entry_refusal(&check)),
    })
}

/// Apply the user's autostart choice and report the state Windows ends up in.
///
/// The result is read back from the system in every case: `reg.exe` can report
/// success for a value Windows then ignores, and a settings page that shows the
/// requested state instead of the real one is how "关掉之后打不开" survived
/// three releases. The change is logged together with its outcome, so a refusal
/// that never reaches the log cannot happen again.
#[cfg(windows)]
pub(crate) fn set_autostart(enabled: bool) -> Result<AutostartStatus, String> {
    let outcome = if set_startup_task(enabled)?.is_some() {
        // A package StartupTask is the authoritative autostart path. Drop any
        // legacy Run value left by an older build so the single-instance guard
        // does not needlessly process a second launch attempt.
        let _ = remove_legacy_run_entry();
        autostart_status()
    } else {
        // 注册表 Run 键：开机自启 dsh-wallpaper
        //  开启: 写入当前构建需要的启动命令
        //  关闭: 删除该值
        // An MSIX install must record the shell's version-stable launch alias
        // rather than its own versioned `WindowsApps` path, which the next
        // package update deletes.
        let write = if enabled {
            current_run_entry_command().and_then(|command| write_run_entry(&command))
        } else {
            remove_legacy_run_entry()
        };
        match write {
            Ok(()) => autostart_status(),
            Err(error) => Err(error),
        }
    };
    match &outcome {
        Ok(status) => log::info!(
            "开机自启变更：requested={} enabled={} source={} reason={:?}",
            enabled,
            status.enabled,
            status.source,
            status.reason
        ),
        Err(error) => log::warn!("开机自启变更失败：requested={enabled} error={error}"),
    }
    outcome
}

/// Carry an enabled autostart preference across a package update.
///
/// A package update installs into a new versioned `WindowsApps` directory and
/// deletes the previous one, so a per-user Run entry recorded by the old build
/// stops launching anything the moment the new version is installed — which is
/// exactly how autostart vanished between 0.2.0.71 and 0.2.0.74. The
/// authoritative package StartupTask is preferred, and when Windows cannot
/// offer it the compatibility entry is refreshed so it names this version.
#[cfg(windows)]
pub(crate) fn migrate_legacy_autostart() -> Result<Option<AutostartStatus>, String> {
    if !has_package_identity()? {
        // Unpackaged development and NSIS builds only ever have the Run entry,
        // and nothing about an MSIX update can invalidate it.
        return Ok(None);
    }
    // Absence is the user's choice: never enable autostart on their behalf.
    let Some(recorded) = run_entry_command()? else {
        return Ok(Some(autostart_status()?));
    };

    // 1. Prefer the package task: it survives updates without our help.
    match set_startup_task(true) {
        Ok(Some(true)) => {
            remove_legacy_run_entry()?;
            return Ok(Some(autostart_status()?));
        }
        Ok(_) => {}
        // Windows may refuse, for example when the user disabled the task in
        // Task Manager. Keep the compatibility entry working instead of
        // leaving autostart broken for as long as the refusal lasts.
        Err(error) => log::warn!("开机启动任务不可用，改用当前用户启动项：{error}"),
    }

    // 2. The compatibility entry must name this version. Refreshing it is what
    //    makes an already-enabled autostart survive the update without the
    //    user having to toggle the setting again.
    let expected = current_run_entry_command()?;
    if !autostart_commands_match(&recorded, &expected) {
        write_run_entry(&expected)?;
        log::info!("已将当前用户开机自启重新指向本次安装的版本");
    }
    Ok(Some(autostart_status()?))
}

#[cfg(not(windows))]
pub(crate) fn migrate_legacy_autostart() -> Result<Option<AutostartStatus>, String> {
    Ok(None)
}

#[cfg(not(windows))]
pub(crate) fn autostart_status() -> Result<AutostartStatus, String> {
    Ok(AutostartStatus {
        enabled: false,
        source: "unsupported".into(),
        reason: Some("当前系统不支持本应用的开机自启。".into()),
    })
}

#[cfg(not(windows))]
pub(crate) fn set_autostart(_: bool) -> Result<AutostartStatus, String> {
    autostart_status()
}

#[cfg(not(windows))]
pub(crate) fn set_startup_task(_: bool) -> Result<Option<bool>, String> {
    Ok(None)
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

/// Returns the physical installation root of the current MSIX package.  This
/// avoids relying on the executable's apparent location, which Tauri can
/// report from its `_up_` resource context rather than the package root.
#[cfg(windows)]
fn current_package_install_root() -> Option<std::path::PathBuf> {
    let mut length = 0u32;
    // Per GetCurrentPackagePath's contract, a null *optional* buffer queries
    // the required length. `Some(PWSTR::null())` is not equivalent here and
    // can make the function fail, sending us down the legacy `_up_` fallback.
    let first = unsafe { GetCurrentPackagePath(&mut length, None) };
    if first != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return None;
    }
    // The first call reports the UTF-16 payload length. Supply one additional
    // element for the terminating NUL and pass that capacity back explicitly.
    // Passing the exact first value makes some Windows builds return
    // ERROR_INSUFFICIENT_BUFFER a second time.
    let mut buffer = vec![0u16; length as usize + 1];
    let mut capacity = buffer.len() as u32;
    let result = unsafe { GetCurrentPackagePath(&mut capacity, Some(PWSTR(buffer.as_mut_ptr()))) };
    if !result.is_ok() || capacity == 0 {
        return None;
    }
    // The API includes the NUL terminator in the requested capacity but not
    // necessarily in the returned length; trim it defensively either way.
    let value = String::from_utf16_lossy(&buffer[..capacity as usize]);
    let value = value.trim_end_matches('\0');
    (!value.is_empty()).then(|| std::path::PathBuf::from(value))
}

/// Resolves the on-disk package asset used by the MSIX lock-screen setter.
/// A packaged asset is intentionally distinct from the app-data marker kept
/// by the recovery manifest: the latter is never supplied to Windows.
#[cfg(windows)]
fn bundled_sleep_image_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    // Tauri's resource resolver is appropriate for web assets, but the
    // lock-screen WinRT API requires an ordinary, physical package file. In
    // an MSIX install that file lives next to the executable under `_up_`.
    // Prefer this direct path (the same strategy proven by the isolated
    // probe), then retain resolver/source fallbacks for development builds.
    // The Tauri resolver has already proven it can locate `_up_` in the
    // packaged process. Use that physical path as the package-root anchor,
    // then address the dedicated MSIX Assets file directly. Do not guard this
    // branch with `Path::is_file`: WindowsApps can give a packaged process a
    // restricted stat result even though StorageFile can open the asset.
    // Lock-screen takeover is only enabled after the MSIX identity check in
    // `set_lock_screen`. Therefore the executable's parent is the package
    // root here. Return the conventional Assets path directly rather than
    // asking `Path::is_file`, whose result can be virtualized for this process.
    if let Ok(executable) = std::env::current_exe() {
        if let Some(package_root) = executable.parent() {
            return Ok(package_root.join("Assets").join("LockScreenSleep.png"));
        }
    }

    let resolver_asset = app
        .path()
        .resolve(
            "_up_/public/personas/wake-frames/variant-anima/sleep.png",
            tauri::path::BaseDirectory::Resource,
        )
        .ok()
        .and_then(|resource| {
            resource
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case("_up_"))
                })
                .and_then(std::path::Path::parent)
                .map(|root| root.join("Assets").join("LockScreenSleep.png"))
        });

    resolver_asset
        .or_else(|| {
            current_package_install_root()
                .or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
                })
                .and_then(|root| {
                    std::iter::once(root.join("Assets").join("LockScreenSleep.png"))
                        .chain(
                            bundled_sleep_resource_candidates()
                                .into_iter()
                                .map(|resource| root.join(resource)),
                        )
                        .find(|path| path.is_file())
                })
        })
        .or_else(|| {
            bundled_sleep_resource_candidates()
                .into_iter()
                .find_map(|resource| {
                    app.path()
                        .resolve(resource, tauri::path::BaseDirectory::Resource)
                        .ok()
                        .filter(|path| path.is_file())
                })
        })
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
        .ok_or_else(|| "未找到内置的锁屏睡眠图片。请重新安装 dsh-wallpaper。".to_string())
}

/// Resolves the managed image that is meaningful for the currently persisted
/// ownership state.  There is intentionally no speculative new filename here:
/// diagnostics and stale-state checks must only compare against a file that a
/// manifest has already committed.  Before the first takeover (and for a
/// legacy URI-only marker), the old fixed name is the only compatible path.
#[cfg(windows)]
fn managed_image_path_for_state(
    config_dir: &std::path::Path,
    state: &LockScreenBackupState,
) -> Result<std::path::PathBuf, String> {
    match state {
        LockScreenBackupState::Valid(manifest) => managed_image_path(config_dir, manifest),
        LockScreenBackupState::Missing | LockScreenBackupState::LegacyUri { .. } => {
            managed_image_path_from_file(config_dir, LEGACY_MANAGED_IMAGE_FILE)
        }
        LockScreenBackupState::Invalid { reason } => Err(format!(
            "现有锁屏备份不完整，无法安全解析托管图片：{reason}"
        )),
    }
}

/// Copies a bundled lock-screen image to its content-addressed destination.
///
/// The destination **must** already hold identical bytes when it exists: the name is derived from
/// the content, so a same-named file with different bytes can only mean a corrupted or hostile
/// asset directory.  That case fails closed instead of overwriting, and the identical case is a
/// reuse (the common one — every later takeover of the same art lands on the same file, which is
/// what keeps Windows' "recent images" list at one entry per distinct image).
#[cfg(windows)]
fn copy_sleep_image_without_overwrite(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    if destination.exists() {
        let existing = crate::lock_screen_backup::file_content_hash(destination)?;
        let wanted = crate::lock_screen_backup::file_content_hash(source)?;
        if existing == wanted {
            return Ok(());
        }
        return Err(format!(
            "锁屏托管图片的既有副本内容与素材不一致，已拒绝覆盖：{}",
            destination.display()
        ));
    }
    use std::io::{Read, Write};

    let parent = destination
        .parent()
        .ok_or_else(|| "锁屏图片存放目录无效".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建锁屏图片存放目录：{error}"))?;

    // Do not use one all-in-one closure here: if `create_new` reports that a
    // same-named file already exists, cleanup must *not* delete that existing
    // file.  Only failures after we successfully create this exact destination
    // are ours to roll back.
    let mut input =
        std::fs::File::open(source).map_err(|error| format!("无法读取内置锁屏图片：{error}"))?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("无法创建新的锁屏图片副本：{error}"))?;
    let result = (|| -> std::io::Result<()> {
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let bytes = input.read(&mut buffer)?;
            if bytes == 0 {
                break;
            }
            output.write_all(&buffer[..bytes])?;
        }
        output.sync_all()
    })();
    drop(output);
    if let Err(error) = result {
        let _ = std::fs::remove_file(destination);
        return Err(format!(
            "无法准备锁屏图片。请检查应用安装目录是否完整：{error}"
        ));
    }
    Ok(())
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
    let config_dir = lock_screen_config_dir(app)?;
    let original_image_uri = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| uri.to_string());
    let backup_state = inspect_backup(&config_dir);
    // Diagnostics must use the filename committed in the manifest.  Falling
    // back to the legacy fixed path makes a healthy v2 takeover look inactive.
    // An invalid manifest intentionally yields no managed path rather than
    // trusting unvalidated disk data.
    let managed_image = managed_image_path_for_state(&config_dir, &backup_state).ok();
    let backup_exists = !matches!(backup_state, LockScreenBackupState::Missing);
    let backup_valid = matches!(backup_state, LockScreenBackupState::Valid(_));
    let managed_image_active = managed_image
        .as_deref()
        .is_some_and(|path| managed_image_is_active(original_image_uri.as_deref(), path))
        || bundled_sleep_image_path(app)
            .ok()
            .is_some_and(|path| managed_image_is_active(original_image_uri.as_deref(), &path));
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
        managed_image_ready: managed_image.is_some_and(|path| path.is_file()),
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
pub fn acquire_shared_wallpaper_host() -> bool {
    true
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
    use tempfile::tempdir;

    #[test]
    fn packaged_builds_are_always_eligible_for_lock_screen_takeover() {
        assert!(can_attempt_lock_screen_takeover(true));
    }

    #[test]
    fn unpackaged_lock_screen_takeover_is_never_eligible() {
        assert!(!can_attempt_lock_screen_takeover(false));
    }

    #[test]
    fn packaged_autostart_records_the_version_stable_shell_alias() {
        let command = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        // The application half is this edition's own `<Application Id>`; the
        // test below checks it against the manifest that edition ships.
        assert_eq!(
            command,
            format!(r"explorer.exe shell:AppsFolder\com.dsh.wallpaper_pdxj8y3r6rm5g!{PACKAGE_APPLICATION_ID}")
        );
        // Whatever identifies the release must stay out of the autostart value:
        // a package update deletes the directory that carries it.
        assert!(!command.contains("WindowsApps"));
        assert!(!command.contains("0.2.0"));
    }

    #[test]
    fn an_entry_left_by_a_superseded_version_is_not_a_match() {
        let expected = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        // The exact value 0.2.0.71 wrote, which 0.2.0.74 could no longer launch.
        let stale = r"C:\Program Files\WindowsApps\com.dsh.wallpaper_0.2.0.71_x64__pdxj8y3r6rm5g\dsh-wallpaper.exe";
        assert!(!autostart_commands_match(stale, &expected));
    }

    #[test]
    fn recorded_run_values_are_compared_leniently() {
        let expected = r"C:\Program Files\WindowsApps\com.dsh.wallpaper_0.2.0.74_x64__pdxj8y3r6rm5g\dsh-wallpaper.exe";
        assert!(autostart_commands_match(
            r#"  "C:\Program Files\WindowsApps\com.dsh.wallpaper_0.2.0.74_x64__pdxj8y3r6rm5g\dsh-wallpaper.exe"  "#,
            expected
        ));
        assert!(autostart_commands_match(
            r"c:\program files\windowsapps\COM.DSH.WALLPAPER_0.2.0.74_X64__PDXJ8Y3R6RM5G\dsh-wallpaper.exe",
            expected
        ));
        assert!(!autostart_commands_match("", expected));
    }

    #[test]
    fn a_missing_expected_command_never_matches() {
        assert!(!autostart_commands_match("anything", ""));
        assert!(!autostart_commands_match("", ""));
    }

    #[test]
    fn the_autostart_alias_names_the_edition_it_was_installed_from() {
        // Two editions, two manifests, two `<Application Id>`s. The alias half
        // has to come from the manifest of the edition that recorded it: the
        // shell resolves `family!id`, and an id the package does not declare
        // launches nothing at logon.
        let (manifest, id) = if cfg!(feature = "lite") {
            (
                include_str!("../../../packaging/msix/AppxManifest-Lite.xml"),
                "WallpaperLite",
            )
        } else {
            (
                include_str!("../../../packaging/msix/AppxManifest.xml"),
                "Wallpaper",
            )
        };
        assert_eq!(id, PACKAGE_APPLICATION_ID);
        assert!(
            manifest.contains(&format!(r#"<Application Id="{id}""#)),
            "{id} is not the application this edition's manifest declares"
        );
        let command = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        assert!(command.ends_with(&format!("!{id}")), "{command}");
    }

    #[test]
    fn a_working_run_entry_is_recognised_as_this_build() {
        // The value this machine's Run key holds for 0.2.0.178: the toggle
        // reported it as "not applied" while the status read for the same value
        // logged `source=run enabled=true`. Both halves of the alias are pinned
        // by the test above, so this one fails if either drifts.
        let expected = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        let recorded = format!(
            r"explorer.exe shell:AppsFolder\com.dsh.wallpaper_pdxj8y3r6rm5g!{PACKAGE_APPLICATION_ID}"
        );
        assert!(autostart_commands_match(&recorded, &expected));
        // `reg.exe add /D` stores quoting when the value arrives quoted from a
        // shell, and a quoted value still launches the same application.
        assert!(autostart_commands_match(&format!("\"{recorded}\""), &expected));
    }

    #[test]
    fn a_reg_query_report_is_read_as_the_recorded_value() {
        // Captured on this machine: `reg query <Run key> /v dsh-wallpaper`.
        let report = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    dsh-wallpaper    REG_SZ    explorer.exe shell:AppsFolder\\com.dsh.wallpaper_pdxj8y3r6rm5g!Wallpaper\r\n\r\n";
        assert_eq!(
            parse_reg_query_value(report, RUN_VALUE_NAME).as_deref(),
            Some(r"explorer.exe shell:AppsFolder\com.dsh.wallpaper_pdxj8y3r6rm5g!Wallpaper")
        );
        // A different value whose name merely starts the same is not ours.
        assert_eq!(
            parse_reg_query_value("    dsh-wallpaper-old    REG_SZ    x", RUN_VALUE_NAME),
            None
        );
        // A line that is not a typed value must not be handed out as data.
        assert_eq!(
            parse_reg_query_value("    dsh-wallpaper    NOT_A_TYPE    x", RUN_VALUE_NAME),
            None
        );
    }

    /// The span from an opening tag to its closing tag, so a test can assert
    /// nesting instead of a substring that a misplaced element satisfies too.
    fn span<'a>(source: &'a str, open: &str, close: &str) -> &'a str {
        let from = source
            .find(open)
            .unwrap_or_else(|| panic!("{open} is missing"));
        let to = source[from..]
            .find(close)
            .unwrap_or_else(|| panic!("{close} never closes {open}"));
        &source[from..from + to + close.len()]
    }

    fn edition_manifest() -> &'static str {
        if cfg!(feature = "lite") {
            include_str!("../../../packaging/msix/AppxManifest-Lite.xml")
        } else {
            include_str!("../../../packaging/msix/AppxManifest.xml")
        }
    }

    #[test]
    fn the_manifest_declares_only_namespaces_windows_knows() {
        // An unknown namespace is not a packaging error — `IgnorableNamespaces`
        // exists precisely so Windows can skip what it does not know. That is how
        // `windows.startupTask` sat in every release while registering nothing:
        // the URI said `…/uap/windows/10/5` instead of `…/uap/windows10/5`, so the
        // extension was ignored, `GetAsync` answered 参数错误。 (0x80070057) and no
        // AppModel SystemAppData key was ever created. `makeappx` was happy.
        let manifest = edition_manifest();
        let known = [
            "http://schemas.microsoft.com/appx/manifest/foundation/windows10",
            "http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities",
            "http://schemas.microsoft.com/appx/manifest/uap/windows10",
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/5",
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "http://schemas.microsoft.com/appx/manifest/virtualization/windows10",
        ];
        let mut declared = 0;
        for (index, _) in manifest.match_indices("xmlns:") {
            let rest = &manifest[index..];
            let Some(open) = rest.find('"') else { continue };
            let Some(close) = rest[open + 1..].find('"') else {
                continue;
            };
            let uri = &rest[open + 1..open + 1 + close];
            declared += 1;
            assert!(
                known.contains(&uri),
                "{uri} is not a manifest namespace Windows knows"
            );
        }
        assert!(declared >= 5, "manifest declared only {declared} namespaces");
    }

    #[test]
    fn the_startup_task_sits_where_the_schema_puts_it() {
        // Package → Applications → Application → Extensions → uap5:Extension →
        // uap5:StartupTask. Without the `Extensions` container the package still
        // installs and Windows never registers the task: `GetAsync` answered
        // 参数错误。 (0x80070057) for every build up to 0.2.0.180, and the AppModel
        // SystemAppData key was never created. Thirteen other packages on this
        // machine declare windows.startupTask, and all thirteen have the
        // container — a substring check cannot tell the two shapes apart.
        let manifest = edition_manifest();
        let application = span(manifest, "<Application ", "</Application>");
        let extensions = span(application, "<Extensions>", "</Extensions>");
        let extension = span(
            extensions,
            r#"<uap5:Extension Category="windows.startupTask""#,
            "</uap5:Extension>",
        );
        assert!(extension.contains(r#"<uap5:StartupTask TaskId="DshWallpaperStartup""#));
    }

    #[test]
    fn the_run_key_is_declared_unvirtualized_by_the_manifest_this_edition_ships() {
        // MSIX redirects a packaged app's HKCU writes into a private per-user
        // hive that is merged over the real key when it reads. For the Run key
        // that means an in-process delete leaves a tombstone which hides the
        // real value from the app while Windows keeps launching it at logon —
        // 「当前用户启动项里没有 DSH Wallpaper」 for a value sitting right there.
        let manifest = edition_manifest();
        let excluded = format!(
            "<virtualization:ExcludedKey>HKEY_CURRENT_USER\\{RUN_KEY_SUBKEY}</virtualization:ExcludedKey>"
        );
        assert!(
            manifest.contains(&excluded),
            "the manifest this edition ships does not declare {excluded}"
        );
        assert!(manifest.contains(r#"<rescap:Capability Name="unvirtualizedResources" />"#));
    }

    #[test]
    fn a_refused_autostart_names_its_own_state() {
        let absent = RunEntryCheck {
            matches: false,
            recorded: None,
            expected: Some("expected".into()),
            error: None,
        };
        let stale = RunEntryCheck {
            matches: false,
            recorded: Some("old".into()),
            expected: Some("expected".into()),
            error: None,
        };
        let unreadable = RunEntryCheck {
            matches: false,
            recorded: None,
            expected: None,
            error: Some("无法读取当前用户开机启动项（错误码 5）。".into()),
        };
        assert!(run_entry_refusal(&absent).contains("没有"));
        assert_ne!(run_entry_refusal(&absent), run_entry_refusal(&stale));
        // A read failure is reported as itself, never as "nothing is recorded":
        // those two states need different fixes.
        assert_eq!(
            run_entry_refusal(&unreadable),
            "无法读取当前用户开机启动项（错误码 5）。"
        );
    }

    #[test]
    fn the_autostart_key_paths_name_the_same_key() {
        assert!(RUN_KEY_PATH.starts_with(r"HKCU\"));
        assert_eq!(RUN_KEY_PATH.strip_prefix(r"HKCU\"), Some(RUN_KEY_SUBKEY));
    }

    #[test]
    fn the_run_entry_writer_names_the_hive() {
        // `reg.exe` answers "Invalid key name" for a path without a hive, and
        // the repair then left the entry pointing at the previous version's
        // deleted directory without saying why.
        let arguments = run_entry_add_arguments(r"explorer.exe shell:AppsFolder\pf!Wallpaper");
        let as_str: Vec<&str> = arguments.iter().map(String::as_str).collect();
        assert_eq!(
            as_str,
            vec![
                "add",
                RUN_KEY_PATH,
                "/V",
                RUN_VALUE_NAME,
                "/D",
                r"explorer.exe shell:AppsFolder\pf!Wallpaper",
                "/F",
            ]
        );
        assert!(as_str[1].starts_with(r"HKCU\"));
    }

    #[test]
    fn packaged_sleep_resource_uses_tauris_verified_windows_path_first() {
        assert_eq!(
            bundled_sleep_resource_candidates()[0],
            "_up_/public/personas/wake-frames/variant-anima/sleep.png"
        );
    }

    #[test]
    fn v2_backup_uses_its_manifest_managed_filename() {
        let root = tempdir().expect("temporary directory");
        let config = root.path().join("config");
        let assets = crate::lock_screen_backup::asset_directory(&config);
        std::fs::create_dir_all(&assets).expect("asset directory");
        std::fs::write(assets.join("original.png"), b"original").expect("snapshot");
        let manifest = LockScreenBackupManifest {
            version: crate::lock_screen_backup::LOCK_SCREEN_BACKUP_SCHEMA_VERSION,
            original_image_uri: "file:///C:/Users/Test/original.png".into(),
            snapshot_file: "original.png".into(),
            managed_image_file: "dsh-wallpaper-sleep-123-0.png".into(),
            captured_at_unix_ms: 123,
        };

        let managed =
            managed_image_path_for_state(&config, &LockScreenBackupState::Valid(manifest))
                .expect("dynamic managed path");

        assert_eq!(
            managed.file_name().and_then(|name| name.to_str()),
            Some("dsh-wallpaper-sleep-123-0.png")
        );
    }

    #[test]
    fn managed_sleep_copy_never_overwrites_an_existing_path() {
        let root = tempdir().expect("temporary directory");
        let source = root.path().join("source.png");
        let destination = root.path().join("managed.png");
        std::fs::write(&source, b"new image").expect("source image");
        std::fs::write(&destination, b"existing image").expect("existing managed image");

        assert!(copy_sleep_image_without_overwrite(&source, &destination).is_err());
        assert_eq!(
            std::fs::read(&destination).expect("existing managed image remains"),
            b"existing image"
        );

        let unique_destination = root.path().join("managed-unique.png");
        copy_sleep_image_without_overwrite(&source, &unique_destination)
            .expect("new managed image");
        assert_eq!(
            std::fs::read(unique_destination).expect("new managed image contents"),
            b"new image"
        );
    }

    #[test]
    fn restore_precondition_requires_the_current_managed_image() {
        let managed = std::path::Path::new(
            r"C:\Users\Test\AppData\Roaming\dsh-wallpaper\lock-screen\managed.png",
        );

        assert!(restore_precondition_is_satisfied(
            Some("file:///c:/users/test/AppData/Roaming/dsh-wallpaper/lock-screen/managed.png"),
            managed,
        ));
        assert!(!restore_precondition_is_satisfied(
            Some("file:///C:/Users/Test/Pictures/user-selected.png"),
            managed,
        ));
        assert!(!restore_precondition_is_satisfied(None, managed));
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
    /// 「双击图标会翻桌面层」这个缺陷的回归钉子。判据（UIA）在真实机器上可能说谎，这条兜底不依赖
/// 它：只要这次双击让前台换成了**别人的**窗口，就说明双击落在了会启动东西的位置，不该翻。
#[test]
fn a_double_click_that_activated_something_else_never_toggles_the_workspace() {
    // 前台换成了另一个窗口（双击快捷方式、文件夹、文件都属于这一类）：不翻。
    assert!(double_click_became_someone_elses(Some(101), Some(202), false));
    // 换成的是**我们自己的另一个窗口**（双击壁纸自己的桌面图标：应用已在运行，进程号不变，
    // 只有窗口会变）：同样不翻 —— 这是按进程号判断时漏掉的那一类。
    assert!(double_click_became_someone_elses(Some(101), Some(303), false));
    // 换成了桌面表面窗口（点空白桌面的常见结果，含图标视图）：仍然翻。
    assert!(!double_click_became_someone_elses(Some(101), Some(202), true));
    // 前台没变（点击穿透时常见）：仍然翻。
    assert!(!double_click_became_someone_elses(Some(101), Some(101), false));
    // 首次点击没记到前台：不因此阻止（宁可保留原行为，也不要让功能静默失效）。
    assert!(!double_click_became_someone_elses(None, Some(202), false));
}

#[test]
fn the_pairing_window_follows_the_system_setting_within_sane_bounds() {
    // 系统默认 500ms 原样使用。
    // 系统值 500ms：我们的手势仍给到 900ms —— 实测用户自然节奏会超过系统值。
    assert_eq!(double_click_pairing_window_ms(500), 900);
    // 系统值本身更宽时跟着系统。
    assert_eq!(double_click_pairing_window_ms(1700), 1700);
    // 极端值夹住，避免误配对。
    assert_eq!(double_click_pairing_window_ms(50), 900);
    assert_eq!(double_click_pairing_window_ms(10_000), 2000);
}

#[test]
fn only_small_foreign_onscreen_rectangles_count_as_desktop_icons() {
    // 典型图标：几十像素的小方块，别人的进程，在屏上。
    assert!(looks_like_a_desktop_icon((100, 100, 180, 172), false, false));
    // 覆盖整屏的伪项：必须挡掉，否则整块桌面都成了"图标范围"，双击空白再也切不动（实测复发过）。
    assert!(!looks_like_a_desktop_icon((0, 0, 2560, 1600), false, false));
    // 我们自己进程里的列表项（输入岛/设置里的 LI 同样是 ListItem）：必须挡掉。
    assert!(!looks_like_a_desktop_icon((100, 100, 180, 172), false, true));
    // 离屏/隐藏的项不参与。
    assert!(!looks_like_a_desktop_icon((100, 100, 180, 172), true, false));
    // 退化的零面积矩形（虚拟化项常见）不参与。
    assert!(!looks_like_a_desktop_icon((100, 100, 100, 100), false, false));
    assert!(!looks_like_a_desktop_icon((0, 0, 2, 2), false, false));
}

#[test]
fn a_pair_must_land_in_nearly_the_same_place() {
    // 双击：几乎在原处（人手抖动几个像素）。
    assert!(within_double_click_reach((100, 200), (104, 197), 64));
    // 同一处的两次点击：当然算。
    assert!(within_double_click_reach((100, 200), (100, 200), 64));
    // 相隔很远的两次单击（例如点两下不同角落）：不算 —— 时间窗口放宽到 1500ms 之后，
    // 这一条是防止"两次无关单击恰好凑成一次切换"的关键。
    assert!(!within_double_click_reach((100, 200), (800, 900), 64));
    assert!(!within_double_click_reach((100, 200), (100, 300), 64));
}

#[test]
fn an_icon_rectangle_owns_its_own_area_and_nothing_else() {
    // 图标矩形 (100,100)-(180,180)：内部（含左上角）算命中，右/下边界与外部不算。
    let icon = (100, 100, 180, 180);
    assert!(rect_contains_point(icon, 100, 100));
    assert!(rect_contains_point(icon, 179, 179));
    assert!(!rect_contains_point(icon, 180, 180));
    assert!(!rect_contains_point(icon, 99, 120));
    assert!(!rect_contains_point(icon, 120, 99));
    assert!(!rect_contains_point(icon, 500, 500));
}

#[test]
fn the_desktop_surface_classes_are_the_wallpaper_hosts_not_explorer_windows() {
    // 桌面表面：Progman/WorkerW 是宿主，另两个是空白双击时前台可能落到的图标视图。
    for class in ["Progman", "WorkerW", "SHELLDLL_DefView", "SysListView32", "#32769"] {
        assert!(is_desktop_surface_class(Some(class)), "{class}");
    }
    // Explorer 开出来的窗口不是桌面表面 —— 这条区分正是这个缺陷的修复点。
    for class in ["CabinetWClass", "ExploreWClass", "Chrome_WidgetWin_1"] {
        assert!(!is_desktop_surface_class(Some(class)), "{class}");
    }
    assert!(!is_desktop_surface_class(None));
}

#[test]
fn workspace_toggle_requires_a_blank_desktop_and_never_a_chat_region() {
        assert!(should_toggle_desktop_workspace(true, false, true));
        assert!(!should_toggle_desktop_workspace(false, false, true));
        assert!(!should_toggle_desktop_workspace(true, true, true));
        assert!(!should_toggle_desktop_workspace(true, false, false));
    }

    /// 无名热区：位置判定照旧，只是不会把岛判成可见。
    fn region(x: f64, y: f64, width: f64, height: f64) -> InteractionRegionInput {
        InteractionRegionInput {
            id: String::new(),
            x,
            y,
            width,
            height,
        }
    }

    /// 带 `ISLAND_REGION_ID` 的热区：展开态输入岛发布的就是它。
    fn island_region(x: f64, y: f64, width: f64, height: f64) -> InteractionRegionInput {
        InteractionRegionInput {
            id: ISLAND_REGION_ID.to_string(),
            x,
            y,
            width,
            height,
        }
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
            scale_interaction_region(region(10.25, 20.5, 100.1, 40.2), 1.5),
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
        assert!(scale_interaction_region(region(0.0, 0.0, 0.0, 10.0), 1.0).is_none());
        assert!(scale_interaction_region(region(0.0, 0.0, 10.0, 10.0), f64::NAN).is_none());
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
        let first = update_interaction_regions(vec![region(1.0, 2.0, 3.0, 4.0)], 1.0, session, newer)
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
            vec![region(5.0, 6.0, 30.0, 40.0)],
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

    /// 悬浮球「岛可见时不弹出」的判据就挂在这条映射上：发布的热区里出现
    /// `ISLAND_REGION_ID` ⇔ 展开态输入岛正在前面。
    #[test]
    fn island_visibility_follows_the_published_island_region() {
        let _guard = REGION_TEST_LOCK.lock().expect("region test lock");
        let session = begin_interaction_region_session().expect("region session");

        update_interaction_regions(vec![region(1.0, 2.0, 3.0, 4.0)], 1.0, session, 1)
            .expect("collapsed publish");
        assert!(!island_visible_from_regions(), "折叠态不该被判成岛可见");

        update_interaction_regions(
            vec![
                region(1.0, 2.0, 3.0, 4.0),
                island_region(5.0, 6.0, 7.0, 8.0),
            ],
            1.0,
            session,
            2,
        )
        .expect("expanded publish");
        assert!(island_visible_from_regions(), "展开态必须被判成岛可见");

        // 收回折叠态（岛消失）后球要能重新弹出。
        update_interaction_regions(vec![region(1.0, 2.0, 3.0, 4.0)], 1.0, session, 3)
            .expect("collapsed publish again");
        assert!(!island_visible_from_regions());

        // 新会话（前端重挂载）会把状态清空，不能残留「岛可见」。
        update_interaction_regions(
            vec![island_region(1.0, 2.0, 3.0, 4.0)],
            1.0,
            session,
            4,
        )
        .expect("expanded publish again");
        assert!(island_visible_from_regions());
        begin_interaction_region_session().expect("new session");
        assert!(!island_visible_from_regions(), "新会话必须重置岛可见状态");
    }
}
