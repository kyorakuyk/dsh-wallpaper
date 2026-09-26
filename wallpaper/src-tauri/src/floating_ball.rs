//! 悬浮球（增量 1）：独立顶层窗口 + 鼠标靠近屏幕底边时弹出。
//!
//! 设计契约见 `docs/plans/interaction-handover.md` §3.1（2026-09-26 拍板）。
//! 本次增量只做「窗口本体 + 原生窗口策略 + 靠近检测与滑入/收回」，窗口内容仍是
//! 占位视觉：本模块不注册任何命令、不发放任何 capability、不与前端发生 IPC。
//!
//! 为什么必须做成独立窗口：实测证明壁纸宿主是 Progman 的直接子窗口、且排在
//! `SHELLDLL_DefView`（图标层）之后，所以画在壁纸场景里的胶囊在表桌面永远收不到
//! 鼠标——点击被 Explorer 的图标层拿走（见 `docs/evidence/input-model-desktop-hit-testing.md`）。
//! 把胶囊移出壁纸场景、做成自己的顶层窗口，这个前提就不存在了。
//!
//! 三条不可动摇的性质：
//! 1. **球的矩形之外一个像素都不挡**：隐藏时窗口整体移出屏幕（几何方式），
//!    并且原生窗口区域裁掉胶囊圆角之外的部分（Windows 把命中测试限制在窗口区域内）。
//! 2. **全程不抢前台**：`WS_EX_NOACTIVATE` + 每次 `SetWindowPos` 都带 `SWP_NOACTIVATE`；
//!    绝不调用 `set_focus` / `SetForegroundWindow` / `ShowWindow(SW_ACTIVATE)`。
//! 3. **只在桌面表面弹出**：复用 `cursor_on_desktop_surface_via_label`，最大化应用
//!    盖住桌面时（光标下的窗口不属于桌面）不弹，无需为此新增判断规则。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// 悬浮球的窗口标签。
///
/// 它是这个窗口的唯一身份：前端 `surface.ts` 用 `getCurrentWindow().label` 认出
/// `floating-ball` 并把它归到 surface `ball`（URL 里的 `?surface=ball` 只是预览兜底）。
pub const BALL_LABEL: &str = "floating-ball";

/// 悬浮球尺寸（逻辑像素）。与胶囊折叠态（190×44）同一量级，高度留出胶囊两端的半圆。
const BALL_LOGICAL_WIDTH: f64 = 216.0;
const BALL_LOGICAL_HEIGHT: f64 = 72.0;

/// 隐藏位置在主屏下边缘之外的偏移（物理像素）。
///
/// 几何方式（contract §3.1 的 (a)）要求隐藏时 0 像素可见，所以整窗下移到一个窗口
/// 高度之外；这里再留 4px，避免屏幕下边缘的 1px 抗锯齿边缘露出来。
const BALL_HIDDEN_Y_OFFSET: i32 = 4;

/// 靠近检测的节拍。球在屏幕外时收不到任何鼠标消息，靠近只能靠全局轮询；
/// 这里沿用 `start_foreground_monitor` / `start_desktop_workspace_monitor` 同族的
/// 16ms 意图（同一个值是有意的：交互延迟与那两条监控线程保持一致）。
const BALL_POLL_INTERVAL_MS: u64 = 16;

/// 光标离开球之后多久收回。
const BALL_HIDE_DELAY_MS: u64 = 400;

/// 「靠近球」的纵向/横向容差（物理像素）。
///
/// ⚠ 实测（2026-09-26，0.2.0.86，本机自动隐藏任务栏）：**不能用「屏幕最底下 6px」当热区**。
/// 光标一到屏幕下边缘，自动隐藏任务栏就会升起接管整条底带（实测 y=1560 起
/// `WindowFromPoint` 返回窗口类 `MSTaskSwWClass`，父链是 `Shell_TrayWnd`）；
/// 而球的「是否在桌面表面」判据 `cursor_is_on_desktop_surface` 只认 `Progman`/`WorkerW`
/// （`is_desktop_foreground_class`），任务栏一律判否——于是球在最需要弹出的位置上
/// 永远弹不出来（实测 39 次采样、0 次上屏）。
///
/// 因此热区改为**球弹出位置所在的那个矩形向外扩一圈**：用户往屏幕下方中央移动时
/// 必然先经过它，而该区域仍在任务栏升起线之上、属于桌面，判据可以通过。
const BALL_TRIGGER_INFLATE_PX: i32 = 16;

/// 滑入/收回的步数与每步间隔：让运动可见，而不是瞬间跳到位。
const BALL_SLIDE_STEPS: i32 = 6;
const BALL_SLIDE_STEP_DELAY_MS: u64 = 6;

/// 几何（主屏矩形、任务栏留白）的重算周期。
///
/// 16ms 的节拍上做一次全显示器枚举是不必要的：显示器拓扑与任务栏状态按人的
/// 时间尺度变化，因此每个节拍只读 `GetCursorPos` + 主屏尺寸（都极廉价），
/// 每秒（或主屏尺寸 / 缩放因子变化时）才重算一次弹出几何。
const BALL_GEOMETRY_REFRESH_INTERVAL_MS: u64 = 1000;

#[cfg(windows)]
use crate::windows_integration::{self, DesktopRect};
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetParent, GetShellWindow, GetSystemMetrics, GetWindow,
    GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, WindowFromPoint,
    GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT, GW_HWNDPREV, HWND_TOP, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    WS_CHILD, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

/// Z 槽自检只在进程内报一次：它是一次性的结构事实，不是每拍状态。
#[cfg(windows)]
static Z_SLOT_REPORTED: AtomicBool = AtomicBool::new(false);

/// 按需创建悬浮球窗口；已经存在就复用它。
///
/// 形态与 `lib.rs` 的 `show_settings_window` 一致（有则取用、无则建），只是球在
/// 创建后就停在屏幕之外的隐藏位置，从不 `show()`。
///
/// 失败语义：只有「建不出 WebView」与「拿不到原生句柄」才返回 `Err`。原生窗口策略
/// （ex-style / 胶囊区域 / Z 槽）失败一律在内部记 `warn!` 而不外传——球是人工入口，
/// 任何一步失败都不许把常驻启动链拖下水（调用方也只用 `log::warn!` 接这里的 `Err`）。
#[cfg(windows)]
pub fn ensure_ball(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(BALL_LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        BALL_LABEL,
        WebviewUrl::App("index.html?surface=ball".into()),
    )
    .title("DSH Wallpaper Ball")
    .inner_size(BALL_LOGICAL_WIDTH, BALL_LOGICAL_HEIGHT)
    .decorations(false)
    .transparent(true)
    .resizable(false)
    .skip_taskbar(true)
    .focusable(false)
    .shadow(false)
    .always_on_top(false)
    .visible(false)
    .build()
    .map_err(|error| format!("无法创建悬浮球窗口：{error}"))?;

    let hwnd = ball_hwnd(&window)?;
    apply_ball_window_policy(&window, hwnd);
    match compute_ball_geometry(&window) {
        Some(geometry) => {
            if !move_ball_position(hwnd, geometry.x, geometry.hidden_y) {
                log::warn!("floating ball: initial off-screen placement failed");
            }
            // 建完窗口后重新压一次 Z 槽：创建瞬间它可能排在普通窗口之上。
            raise_above_progman(hwnd);
            log::info!(
                "floating ball: ready label={BALL_LABEL} scale={} size={}x{} hidden_y={}",
                geometry.scale,
                geometry.width,
                geometry.height,
                geometry.hidden_y
            );
        }
        None => log::warn!("floating ball: geometry unavailable; window left hidden at default position"),
    }
    Ok(window)
}

#[cfg(not(windows))]
pub fn ensure_ball(_: &AppHandle) -> Result<WebviewWindow, String> {
    Err("悬浮球只支持 Windows".into())
}

/// 悬浮球的物理几何（位置与尺寸都是物理像素，与 `SetWindowPos` 同一坐标系）。
#[cfg(windows)]
#[derive(Clone, Copy)]
struct BallGeometry {
    /// 计算时的窗口缩放因子：变化就要重算（并且重建窗口区域）。
    scale: f64,
    /// 计算时的主屏尺寸：主屏变化（分辨率 / 显示器切换）时用来判定缓存失效。
    primary_size: (i32, i32),
    width: i32,
    height: i32,
    /// 弹出位置（窗口左上角）。
    x: i32,
    /// 弹出位置：窗口下边缘停在主屏下边缘之上 `expanded_bottom_inset` 处。
    shown_y: i32,
    /// 隐藏位置：整体位于主屏之下。
    hidden_y: i32,
}

/// 用 `desktopLayoutMetrics` + 主屏矩形算出弹出/隐藏位置。
///
/// **不新造常量**：底部留白取自 `windows_integration::desktop_layout_metrics`
/// （逻辑像素）再按窗口缩放因子换成物理像素，因此「1.5 个任务栏高度」在任务栏
/// 隐藏/显示两种情形下自动跟随（contract §3.2 ③ 与 interaction-handover §2③）。
#[cfg(windows)]
fn compute_ball_geometry(window: &WebviewWindow) -> Option<BallGeometry> {
    let scale = ball_scale_factor(window);
    let (width, height) = ball_physical_size(window);
    let (display, primary_size) = primary_display_bounds(window)?;
    let display_bottom = display.y.saturating_add(display.height);
    let inset_physical =
        (windows_integration::desktop_layout_metrics(window, None).expanded_bottom_inset * scale)
            .round() as i32;
    Some(BallGeometry {
        scale,
        primary_size,
        width,
        height,
        x: display.x + (display.width - width) / 2,
        shown_y: display_bottom - inset_physical - height,
        hidden_y: display_bottom + BALL_HIDDEN_Y_OFFSET,
    })
}

/// 主屏矩形（物理像素）与主屏尺寸。优先复用 `desktop_displays()`，它已经处理了
/// 多屏偏移与逐屏缩放；只有当 Explorer 正在重启之类导致枚举失败时才退回 Tauri 读取。
#[cfg(windows)]
fn primary_display_bounds(window: &WebviewWindow) -> Option<(DesktopRect, (i32, i32))> {
    if let Ok(displays) = windows_integration::desktop_displays() {
        if let Some(display) = displays
            .iter()
            .find(|display| display.primary)
            .or_else(|| displays.first())
        {
            return Some((
                display.bounds,
                (display.bounds.width, display.bounds.height),
            ));
        }
    }
    let monitor = window.primary_monitor().ok().flatten()?;
    let position = *monitor.position();
    let size = *monitor.size();
    let bounds = DesktopRect {
        x: position.x,
        y: position.y,
        width: size.width as i32,
        height: size.height as i32,
    };
    Some((bounds, (bounds.width, bounds.height)))
}

#[cfg(windows)]
fn ball_scale_factor(window: &WebviewWindow) -> f64 {
    match window.scale_factor() {
        Ok(scale) if scale.is_finite() && scale > 0.0 => scale,
        _ => 1.0,
    }
}

/// 窗口的物理尺寸（像素）。以窗口自报的 inner_size 为准，取不到时才用逻辑常量 × 缩放。
#[cfg(windows)]
fn ball_physical_size(window: &WebviewWindow) -> (i32, i32) {
    if let Ok(size) = window.inner_size() {
        if size.width > 0 && size.height > 0 {
            return (size.width as i32, size.height as i32);
        }
    }
    let scale = ball_scale_factor(window);
    (
        (BALL_LOGICAL_WIDTH * scale).round().max(1.0) as i32,
        (BALL_LOGICAL_HEIGHT * scale).round().max(1.0) as i32,
    )
}

#[cfg(windows)]
fn ball_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    window
        .hwnd()
        .map(|handle| HWND(handle.0))
        .map_err(|error| format!("悬浮球窗口没有原生句柄：{error}"))
}

/// 原生窗口策略的三条，一次做完。
#[cfg(windows)]
fn apply_ball_window_policy(window: &WebviewWindow, hwnd: HWND) {
    apply_ball_ex_style(hwnd);
    refresh_ball_region(window, hwnd);
    raise_above_progman(hwnd);
}

/// ex-style：**加** `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，**清** `WS_EX_APPWINDOW`。
///
/// 与 contract §3.1 的窗口配方一致：工具窗（不进任务栏、不进 Alt-Tab）+ 永不激活
/// （球永远不抢前台）。**不加** `WS_EX_TOPMOST`（球要待在普通应用之下），
/// **不加** `WS_EX_TRANSPARENT`（球本身必须收得到鼠标）。
#[cfg(windows)]
fn apply_ball_ex_style(hwnd: HWND) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = (current | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize)
            & !(WS_EX_APPWINDOW.0 as isize);
        if desired != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        // 样式只在窗口仍隐藏时改，下一次 SWP_SHOWWINDOW 生效，因此不需要
        // SWP_FRAMECHANGED（那会额外触发一次非客户区重算，对无边框弹窗没有收益）。
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style & WS_CHILD.0 as isize != 0 {
            // 顶层窗口一旦带上 WS_CHILD，就会被挂进某条父链、从此受宿主裁剪。
            // builder 是无边框弹窗，正常不会出现；真出现就修回来并如实记账。
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                (style & !(WS_CHILD.0 as isize)) | WS_POPUP.0 as isize,
            );
            log::warn!("floating ball: style carried WS_CHILD; restored WS_POPUP");
        } else if style & WS_POPUP.0 as isize == 0 {
            log::warn!("floating ball: window style is not WS_POPUP (style=0x{style:X})");
        }
        log::info!(
            "floating ball: native style exstyle=0x{:X} style=0x{:X}",
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize,
            GetWindowLongPtrW(hwnd, GWL_STYLE) as usize
        );
    }
}

/// 把窗口区域裁成胶囊形。
///
/// Windows 的命中测试被限制在窗口区域内，因此圆角之外的透明像素既画不出东西，
/// 也**吞不掉桌面点击**——这正是「球的矩形之外完全不挡桌面点击」的前提之一
/// （另一半是隐藏时整窗移出屏幕）。缩放因子变化时必须重建：区域是物理像素的。
#[cfg(windows)]
fn refresh_ball_region(window: &WebviewWindow, hwnd: HWND) {
    let (width, height) = ball_physical_size(window);
    // CreateRoundRectRgn 的右/下边界是开区间，+1 才覆盖满 width×height 像素。
    let (right, bottom) = (width + 1, height + 1);
    // 圆角椭圆的直径取整个窗口高度：左右两端于是是完整的半圆（胶囊），
    // 而不是四个小圆角的矩形。前端 `.ball-capsule` 的 border-radius: 999px 与它对应。
    let radius = height.max(1);
    unsafe {
        let region = CreateRoundRectRgn(0, 0, right, bottom, radius, radius);
        if region.is_invalid() {
            log::warn!("floating ball: CreateRoundRectRgn failed");
            return;
        }
        // SetWindowRgn 成功后区域归 Windows 所有，本进程不得再删；返回 0 才是失败，
        // 此时区域仍属调用者，必须自己释放，否则每次失败都漏一个 GDI 对象。
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            log::warn!("floating ball: SetWindowRgn failed");
            let _ = DeleteObject(region.into());
        }
    }
}

/// Z 槽 = **紧贴 Progman 之上**。
///
/// 这样得到的正是「在桌面图标层之上、在所有普通应用之下」，完全不需要 `WS_EX_TOPMOST`
/// （置顶就违反 contract §3.1 第 3 条「应用最大化时被应用盖住」）。
///
/// ⚠ 方向语义（本模块开工时踩过，写下来免得再错）：`SetWindowPos(hwnd, hWndInsertAfter)`
/// 是把 hwnd 放到 **该参考窗口之后（更靠下）**。本仓库自己的 `place_wallpaper_layers`
/// 就是靠这一点把壁纸放到图标层之后的（`SetWindowPos(background, Some(icon_view))`），
/// 实测子窗口栈与之吻合。所以 **不能** 直接写 `Some(progman)`——那会把球放到 Progman
/// 之后，也就是桌面背景之后，球根本不可见。
///
/// 正确做法是插到「Progman 正前方那个窗口」之后：设 X = Progman 的前一个邻居，
/// 传入 X 之后 Z 序变成 `X, 球, Progman`，于是球紧贴 Progman 之上、且仍在所有
/// 普通应用之下。若 Progman 已在带内最前（没有前驱），则用 `HWND_TOP`——此时
/// 「带内最前」与「Progman 之上」是同一件事。
///
/// 本机实测：`FindWindowW("Progman", ..)` 返回 null，只有 `GetShellWindow()` 拿得到
/// Explorer 的桌面窗口，因此固定用后者。
#[cfg(windows)]
fn raise_above_progman(hwnd: HWND) {
    let progman = unsafe { GetShellWindow() };
    if progman.0.is_null() {
        log::warn!("floating ball: GetShellWindow returned no shell window; Z slot unchanged");
        return;
    }
    let in_front_of_progman = unsafe { GetWindow(progman, GW_HWNDPREV) }
        .ok()
        .filter(|handle| !handle.0.is_null());
    // 没有前驱时 Progman 就是带内最前窗口，「插到最前」与「紧贴 Progman 之上」等价。
    let insert_after = in_front_of_progman.unwrap_or(HWND_TOP);
    if let Err(error) = unsafe {
        SetWindowPos(
            hwnd,
            Some(insert_after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
    } {
        log::warn!("floating ball: SetWindowPos(above Progman) failed: {error}");
    }
    report_z_slot(hwnd, progman);
}

/// 读回 Z 序邻居并如实报告（每个进程只报一次）。
///
/// `GW_HWNDPREV` = 排在球**前面**（更靠上层）的窗口；`GW_HWNDNEXT` = 排在球**后面**的。
/// 期望形态是「球的下一个邻居就是 shell 窗口」= 球紧贴桌面之上、所有普通应用之下。
/// 若反过来（球的前一个邻居是 shell 窗口），球就落在桌面之后、根本不可见——
/// 那是必须立刻知道的结论，而不是靠截图猜。
#[cfg(windows)]
fn report_z_slot(hwnd: HWND, progman: HWND) {
    if Z_SLOT_REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let front = unsafe { GetWindow(hwnd, GW_HWNDPREV) }
        .ok()
        .filter(|handle| !handle.0.is_null());
    let back = unsafe { GetWindow(hwnd, GW_HWNDNEXT) }
        .ok()
        .filter(|handle| !handle.0.is_null());
    let describe = |handle: Option<HWND>| match handle {
        Some(handle) => format!("{} 0x{:X}", window_class_name(handle), handle.0 as usize),
        None => "none".to_string(),
    };
    if back == Some(progman) {
        log::info!(
            "floating ball: z-slot ok front={} back={} (shell window) — above the desktop, below every app",
            describe(front),
            describe(back)
        );
    } else if front == Some(progman) {
        log::warn!(
            "floating ball: z-slot is BEHIND the shell window (front={} back={}) — the ball cannot be visible",
            describe(front),
            describe(back)
        );
    } else {
        log::info!(
            "floating ball: z-slot front={} back={} (shell window is elsewhere in the stack)",
            describe(front),
            describe(back)
        );
    }
}

/// 只用于日志的类名读取（`windows_integration` 里那份是私有的，这里不跨模块改动它）。
#[cfg(windows)]
fn window_class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return "?".to_string();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

/// 窗口矩形的日志写法（物理像素），用于把每次滑入/收回落到具体坐标。
#[cfg(windows)]
fn window_rect_text(hwnd: HWND) -> String {
    let mut rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return "rect=?".to_string();
        }
    }
    format!(
        "rect={},{} {}x{}",
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top
    )
}

/// 移动悬浮球窗口：只移动，不抢前台、不改 Z 序（Z 序由 `raise_above_progman` 保证）。
#[cfg(windows)]
fn move_ball_position(hwnd: HWND, x: i32, y: i32) -> bool {
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
        )
        .is_ok()
    }
}

/// 滑入 / 收回：分几步挪动窗口，让运动可见。每一步都不激活、不改 Z 序。
#[cfg(windows)]
fn slide_ball(hwnd: HWND, x: i32, from_y: i32, to_y: i32) -> bool {
    let mut moved = true;
    for step in 1..=BALL_SLIDE_STEPS {
        let y = from_y + (to_y - from_y) * step / BALL_SLIDE_STEPS;
        moved &= move_ball_position(hwnd, x, y);
        std::thread::sleep(std::time::Duration::from_millis(BALL_SLIDE_STEP_DELAY_MS));
    }
    moved
}

#[cfg(windows)]
fn cursor_position() -> Option<POINT> {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return None;
        }
    }
    Some(point)
}

/// 主屏尺寸（物理像素）。进程是 Per-Monitor DPI Aware，所以这里拿到的就是物理像素。
#[cfg(windows)]
fn primary_display_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// 光标是否落在球的窗口矩形内（契约里的粗判据，形状仍由窗口区域精裁）。
#[cfg(windows)]
fn point_inside_ball(hwnd: HWND, point: POINT) -> bool {
    let mut rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
    }
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

/// 光标下的窗口（含祖先链）是否就是悬浮球。
///
/// 球的 WebView2 渲染子窗口属于另一个进程，所以必须沿父链上溯比较，不能只看
/// `WindowFromPoint` 的一次返回值。注意：球是顶层窗口，`GetParent` 为空，因此
/// 这条判据与 `cursor_is_on_desktop_surface`（要求父链能走到 Progman）天然互斥——
/// 球上的点击不会被表/里桌面双击监控误判，不需要为球新增任何规则。
#[cfg(windows)]
fn cursor_over_ball(hwnd: HWND, point: POINT) -> bool {
    unsafe {
        let mut current = WindowFromPoint(point);
        for _ in 0..16 {
            if current.0.is_null() {
                return false;
            }
            if current == hwnd {
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

/// 光标是否落在「球的弹出位置外扩一圈」的矩形里。
///
/// 详细原因见 `BALL_TRIGGER_INFLATE_PX` 的注释：热区必须落在球真正出现的地方
/// （任务栏升起线之上），而不是屏幕最底下那几像素。
#[cfg(windows)]
fn point_in_trigger(point: POINT, geometry: &BallGeometry) -> bool {
    point.x >= geometry.x - BALL_TRIGGER_INFLATE_PX
        && point.x <= geometry.x + geometry.width + BALL_TRIGGER_INFLATE_PX
        && point.y >= geometry.shown_y - BALL_TRIGGER_INFLATE_PX
        && point.y <= geometry.shown_y + geometry.height + BALL_TRIGGER_INFLATE_PX
}

/// 16ms 靠近检测：光标靠近底部 ⇒ 滑入；离开球体范围 400ms ⇒ 收回。
///
/// 与 `start_foreground_monitor` / `start_desktop_workspace_monitor` 同形：
/// `std::thread::spawn` + `loop` + `sleep`，窗口消失即退出线程。
/// 状态迁移全部落 info 日志，便于在日志文件里直接观察行为。
#[cfg(windows)]
pub fn start_ball_monitor(app: AppHandle) {
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window(BALL_LABEL) else {
            log::warn!("floating ball: monitor not started (window missing)");
            return;
        };
        let Ok(hwnd) = ball_hwnd(&window) else {
            log::warn!("floating ball: monitor not started (no native handle)");
            return;
        };
        log::info!("floating ball: approach monitor started interval={BALL_POLL_INTERVAL_MS}ms");

        let mut shown = false;
        let mut last_pointer_inside = std::time::Instant::now();
        let mut geometry: Option<BallGeometry> = None;
        let mut region_size: Option<(i32, i32)> = None;
        let mut last_geometry_refresh = std::time::Instant::now();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(BALL_POLL_INTERVAL_MS));
            // 窗口没了（进程收尾、WebView 崩溃）就结束线程，不留无主轮询。
            if app.get_webview_window(BALL_LABEL).is_none() {
                log::info!("floating ball: monitor stopped (window gone)");
                break;
            }
            let Some(pointer) = cursor_position() else {
                continue;
            };
            let primary_size = primary_display_size();
            let scale = ball_scale_factor(&window);
            let refresh = match geometry {
                Some(current) => {
                    current.primary_size != primary_size
                        || (current.scale - scale).abs() > f64::EPSILON
                        || last_geometry_refresh.elapsed()
                            >= std::time::Duration::from_millis(BALL_GEOMETRY_REFRESH_INTERVAL_MS)
                }
                None => true,
            };
            if refresh {
                match compute_ball_geometry(&window) {
                    Some(mut next) => {
                        if region_size != Some((next.width, next.height)) {
                            // 缩放因子变了（窗口被拖到别的 DPI，或系统改缩放）：区域要重建，
                            // 否则圆角外的透明像素会继续按旧尺寸吞点击。
                            refresh_ball_region(&window, hwnd);
                            region_size = Some((next.width, next.height));
                        }
                        // ⚠ 显示中**不允许**被任务栏留白的变化挪动位置。
                        //
                        // 实测（0.2.0.87）：自动隐藏任务栏在光标靠近底部时升起，此刻
                        // `desktop_layout_metrics` 会报 `taskbar_visible=true`，
                        // 留白从 48 逻辑像素（72 物理）翻倍到 96（144 物理），
                        // 于是球在弹出后自己向上跳一个任务栏高度（实测同一位置先后读到
                        // (1118,1420)-(1442,1528) 与 (1118,1348)-(1442,1456)）。
                        // 球之所以弹出，恰恰是因为光标碰到了底部——这条抖动必然发生。
                        // 因此显示期间沿用弹出那一刻定下的 shown_y；真正的显示环境变化
                        // （主屏尺寸 / 缩放）仍然重新落位。
                        let display_changed = geometry
                            .map(|current| {
                                current.primary_size != next.primary_size
                                    || (current.scale - next.scale).abs() > f64::EPSILON
                            })
                            .unwrap_or(true);
                        if shown && !display_changed {
                            if let Some(current) = geometry {
                                if current.shown_y != next.shown_y {
                                    next.shown_y = current.shown_y;
                                }
                            }
                        }
                        let repositioned = geometry
                            .map(|current| (current.x, current.shown_y) != (next.x, next.shown_y))
                            .unwrap_or(false);
                        geometry = Some(next);
                        if repositioned && shown {
                            // 显示中几何真的变了（主屏尺寸 / 缩放变化）：直接摆到新位置，
                            // 不做滑入动画，也不改 Z 序。
                            if !move_ball_position(hwnd, next.x, next.shown_y) {
                                log::warn!("floating ball: reposition failed reason=geometry-changed");
                            }
                        }
                    }
                    None => log::warn!("floating ball: geometry unavailable; keeping last position"),
                }
                last_geometry_refresh = std::time::Instant::now();
            }
            let Some(current) = geometry else {
                continue;
            };

            let now = std::time::Instant::now();
            if point_inside_ball(hwnd, pointer) || cursor_over_ball(hwnd, pointer) {
                last_pointer_inside = now;
                if !shown {
                    if !slide_ball(hwnd, current.x, current.hidden_y, current.shown_y) {
                        log::warn!("floating ball: slide-in failed");
                    }
                    raise_above_progman(hwnd);
                    shown = true;
                    log::info!(
                        "floating ball: shown reason=pointer-in-ball {}",
                        window_rect_text(hwnd)
                    );
                }
                continue;
            }

            let approaching = point_in_trigger(pointer, &current);
            if shown {
                // 光标停在底部热区里也算「还在球上」。契约只写了「离开球体范围 400ms 收回」，
                // 但弹出位置比底部热区高出 expanded_bottom_inset（任务栏隐藏时约 1.5 个任务栏
                // 高度），光标一旦停在最底下那几像素，严格执行就会变成「弹出 → 400ms → 收回
                // → 下一拍立刻再弹出」的抖动。这一条是对契约的最小补充（见交付说明）。
                if approaching {
                    last_pointer_inside = now;
                } else if now.duration_since(last_pointer_inside)
                    >= std::time::Duration::from_millis(BALL_HIDE_DELAY_MS)
                {
                    if !slide_ball(hwnd, current.x, current.shown_y, current.hidden_y) {
                        log::warn!("floating ball: slide-out failed");
                    }
                    raise_above_progman(hwnd);
                    shown = false;
                    log::info!(
                        "floating ball: hidden reason=pointer-left {}",
                        window_rect_text(hwnd)
                    );
                }
            } else if approaching && windows_integration::cursor_on_desktop_surface_via_label(&app) {
                // 只在桌面上弹：最大化应用的底边同样贴着屏幕下边缘，少了这一条就会在
                // 应用上面弹出球。判据直接复用桌面宿主那条（光标下的窗口父链能走到 Progman）。
                if !slide_ball(hwnd, current.x, current.hidden_y, current.shown_y) {
                    log::warn!("floating ball: slide-in failed");
                }
                raise_above_progman(hwnd);
                shown = true;
                last_pointer_inside = now;
                log::info!(
                    "floating ball: shown reason=approach cursor={},{} {}",
                    pointer.x,
                    pointer.y,
                    window_rect_text(hwnd)
                );
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start_ball_monitor(_: AppHandle) {}
