use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[cfg(windows)]
use super::{MAX_SCALE_FACTOR, MIN_SCALE_FACTOR};
#[cfg(windows)]
use windows::core::{BOOL, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
#[cfg(windows)]
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowRect, IsWindowVisible, MONITORINFOF_PRIMARY,
};

#[cfg_attr(feature = "lite", allow(dead_code))]
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
    // SAFETY: the callback receives `&mut displays` through LPARAM; the vector is alive and unaliased for this synchronous call and the callback only pushes into it.
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

/// 底部留白（逻辑像素）与任务栏可见性。
///
/// 这条路径本身就是**纯值**版本：它只问句柄与缩放因子，二者都由调用方给出。
///
/// 为什么要有这个纯值入口：悬浮球监控线程每 16ms 跑一拍，它拿不到也不该拿
/// `WebviewWindow`（`get_webview_window` 会 `Webview::clone`，只能在主线程做），
/// 但它必须能算底部留白。算法留在这里一份，两条调用路径共用 —— 不产生第二份实现，
/// 也就不存在「线程版本和主线程版本算出不同留白」的可能。
///
/// `hwnd` 为 `None` 与旧写法里 `window.hwnd()` 返回 `Err` 等价：直接给兜底留白。
#[cfg(windows)]
#[cfg_attr(feature = "lite", allow(dead_code))]
pub fn desktop_layout_metrics_for_hwnd(
    hwnd: Option<HWND>,
    scale: f64,
    display_id: Option<&str>,
) -> DesktopLayoutMetrics {
    let scale = scale.clamp(MIN_SCALE_FACTOR, MAX_SCALE_FACTOR);
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
    let Some(background) = hwnd else {
        return DesktopLayoutMetrics {
            expanded_bottom_inset: fallback,
            taskbar_visible: false,
        };
    };
    let mut background_rect = RECT::default();
    let mut taskbar_rect = RECT::default();
    // SAFETY: FindWindowW receives a static class name and a null window name; the handle is used only for the reads below and never freed here.
    let taskbar = unsafe { FindWindowW(windows::core::w!("Shell_TrayWnd"), PCWSTR::null()) }.ok();
    // SAFETY: `taskbar` is the handle produced above and only probed; Win32 returns false for a window that has gone away instead of faulting.
    let visible = taskbar.is_some_and(|taskbar| unsafe { IsWindowVisible(taskbar).as_bool() })
        // SAFETY: `background` is a live HWND owned by the caller for this call, and `background_rect` is local stack storage that outlives it.
        && unsafe { GetWindowRect(background, &mut background_rect).is_ok() }
        // SAFETY: `taskbar` comes from the FindWindowW above and `taskbar_rect` is local stack storage that outlives this call.
        && taskbar.is_some_and(|taskbar| unsafe { GetWindowRect(taskbar, &mut taskbar_rect).is_ok() });
    if visible {
        let physical_height = (taskbar_rect.bottom - taskbar_rect.top).max(0);
        let at_bottom = taskbar_rect.top >= background_rect.bottom.saturating_sub(physical_height + 2);
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

#[cfg_attr(feature = "lite", allow(dead_code))]
pub fn desktop_layout_metrics(
    window: &WebviewWindow,
    display_id: Option<&str>,
) -> DesktopLayoutMetrics {
    #[cfg(windows)]
    {
        // 这条入口只跑在主线程上（调用方是启动链与同步命令处理器），所以读窗口自身的
        // 缩放因子与句柄都是合法的；算法与线程那侧共用同一个纯值核。
        let scale = window.scale_factor().unwrap_or(1.0);
        let hwnd = window.hwnd().ok().map(|raw| HWND(raw.0));
        desktop_layout_metrics_for_hwnd(hwnd, scale, display_id)
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
