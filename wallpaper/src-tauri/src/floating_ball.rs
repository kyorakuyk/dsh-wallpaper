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
//! 3. **只在桌面表面弹出**：复用 `windows_integration::cursor_is_on_desktop_surface`
//!    （桌面宿主的句柄在主线程解析一次后，以整数形式带进监控线程），最大化应用
//!    盖住桌面时（光标下的窗口不属于桌面）不弹，无需为此新增判断规则。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// 悬浮球的窗口标签。
///
/// 它是这个窗口的唯一身份：前端 `surface.ts` 用 `getCurrentWindow().label` 认出
/// `floating-ball` 并把它归到 surface `ball`（URL 里的 `?surface=ball` 只是预览兜底）。
pub const BALL_LABEL: &str = "floating-ball";

/// 悬浮球尺寸（逻辑像素）。
///
/// 用户要的是**一个极简的小圆球**：正方形窗口 + 圆形容器 + 原生圆形区域裁剪，
/// 于是"窗口矩形 == 球"——球以外一个像素都不占，也就没有需要登记热区的地方。
const BALL_LOGICAL_WIDTH: f64 = 56.0;
const BALL_LOGICAL_HEIGHT: f64 = 56.0;

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

/// 用户点过球之后，多久之内不许再弹出。
///
/// 实测（0.2.0.90）：点击球 ⇒ 命令进入里桌面 ⇒ 球收到收回请求退场，但紧接着**又弹了回来**
/// （日志：`hidden reason=retract-requested` 后 1 拍就是 `shown reason=approach`）。
/// 原因是「岛是否可见」这个判据来自前端发布的热区列表，而进入里桌面时前端会重挂载/重发，
/// 中间存在一个「列表里暂时没有 chat」的窗口期，球恰好在那几十毫秒里被判成"可以弹"。
/// 这里加一个冷却期：点过球就是「交给输入岛」的意图，这段时间内球不再冒头。
/// 与它配套的是下面的「等待岛接管」状态——那条才是真正的判据，冷却只是兜底。
const BALL_POP_COOLDOWN_AFTER_CLICK_MS: u64 = 1500;

/// 点过球之后，最多等多久才算「岛没来接」并允许球重新弹出（安全阀）。
///
/// 正常情况下岛会在几百毫秒内发布 `chat` 热区，`waiting_for_island` 随即解除；
/// 万一岛因为别的原因没出现（例如用户设置变了），也不能让球永远憋着不出现。
const BALL_ISLAND_HANDOVER_TIMEOUT_MS: u64 = 10_000;

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
use crate::windows_integration::{self, assert_not_acting_as_main_thread, ball_window_handle_now, DesktopRect};
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};
#[cfg(windows)]
use windows::Win32::UI::HiDpi::GetDpiForWindow;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetClientRect, GetCursorPos, GetParent, GetShellWindow, GetSystemMetrics,
    GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindow, SetWindowLongPtrW, SetWindowPos,
    WindowFromPoint, GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT, GW_HWNDPREV, HWND_TOP, SM_CXSCREEN,
    SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

/// Z 槽自检只在进程内报一次：它是一次性的结构事实，不是每拍状态。
#[cfg(windows)]
static Z_SLOT_REPORTED: AtomicBool = AtomicBool::new(false);

/// 前端点击悬浮球后要求它立刻退场。
///
/// 窗口只由监控线程移动（单一所有者，避免命令线程与轮询线程同时 `SetWindowPos` 打架），
/// 所以命令侧只置一个请求位，真正收回去的动作发生在下一拍（≤16ms）。
#[cfg(windows)]
static BALL_RETRACT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// 请求悬浮球立刻收回。由「单击球 ⇒ 进里桌面」的命令在成功后调用。
#[cfg(windows)]
pub(crate) fn request_ball_retract() {
    BALL_RETRACT_REQUESTED.store(true, Ordering::Release);
}

#[cfg(windows)]
fn take_retract_request() -> bool {
    BALL_RETRACT_REQUESTED.swap(false, Ordering::AcqRel)
}

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
        // 复用已有的球：句柄槽位照写。球的出生点只有这一个函数，槽位漏写就等于线程
        // 拿不到句柄（见 `windows_integration::BACKGROUND_WINDOW_HANDLE` 的说明）。
        if let Ok(handle) = window.hwnd() {
            windows_integration::publish_ball_window_handle(Some(HWND(handle.0)));
        }
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
    // 创建点写槽位（主线程写、线程读）。这是球的唯一出生点，少写一次球就再也弹不出来。
    windows_integration::publish_ball_window_handle(Some(hwnd));
    apply_ball_window_policy(hwnd);
    match compute_ball_geometry(hwnd) {
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
/// **不新造常量**：底部留白取自 `windows_integration::desktop_layout_metrics_for_hwnd`
/// （逻辑像素）再按窗口缩放因子换成物理像素，因此「1.5 个任务栏高度」在任务栏
/// 隐藏/显示两种情形下自动跟随（contract §3.2 ③ 与 interaction-handover §2③）。
///
/// 参数是**句柄**而不是 `WebviewWindow`：监控线程每 16ms 跑一拍，而
/// `get_webview_window` 会 `Webview::clone`，只能在主线程做（取证见
/// `windows_integration` 开头的说明）。`HWND` 是无引用计数的句柄整数，跨线程安全。
#[cfg(windows)]
fn compute_ball_geometry(hwnd: HWND) -> Option<BallGeometry> {
    let scale = ball_scale_factor(hwnd);
    let (width, height) = ball_physical_size(hwnd);
    let display = primary_display_bounds()?;
    let expanded_bottom_inset =
        windows_integration::desktop_layout_metrics_for_hwnd(Some(hwnd), scale, None)
            .expanded_bottom_inset;
    Some(ball_geometry_from_values(
        scale,
        (width, height),
        display,
        expanded_bottom_inset,
    ))
}

/// 几何的纯计算部分：输入都是从窗口 / 显示器读到的值，没有任何 Win32 调用。
///
/// 抽出来的唯一目的是可测：「主屏尺寸变了」「缩放因子变了」这些分支必须能在单测里
/// 断言（读窗口那一半造不出真窗口，见本模块测试）。
#[cfg(windows)]
fn ball_geometry_from_values(
    scale: f64,
    size: (i32, i32),
    display: DesktopRect,
    expanded_bottom_inset: f64,
) -> BallGeometry {
    let (width, height) = size;
    let display_bottom = display.y.saturating_add(display.height);
    let inset_physical = (expanded_bottom_inset * scale).round() as i32;
    BallGeometry {
        scale,
        primary_size: (display.width, display.height),
        width,
        height,
        x: display.x + (display.width - width) / 2,
        shown_y: display_bottom - inset_physical - height,
        hidden_y: display_bottom + BALL_HIDDEN_Y_OFFSET,
    }
}

/// 「该重算几何了吗」的纯判据：主屏尺寸变了、缩放因子变了，或离上次重算满一个周期。
///
/// 与 `compute_ball_geometry` 分开是为了让缩放变化这条分支可测（单测里造不出真窗口，
/// 但可以造一对几何快照）。`since_last_refresh_ms` 由调用方取整毫秒。
#[cfg(windows)]
fn ball_geometry_refresh_due(
    current: Option<&BallGeometry>,
    primary_size: (i32, i32),
    scale: f64,
    since_last_refresh_ms: u128,
) -> bool {
    match current {
        Some(current) => {
            current.primary_size != primary_size
                || (current.scale - scale).abs() > f64::EPSILON
                || since_last_refresh_ms >= BALL_GEOMETRY_REFRESH_INTERVAL_MS as u128
        }
        None => true,
    }
}

/// 主屏矩形（物理像素）。优先复用 `desktop_displays()`，它已经处理了多屏偏移与逐屏
/// 缩放；只有当 Explorer 正在重启之类导致枚举失败时才退回 Win32 的整屏度量。
#[cfg(windows)]
fn primary_display_bounds() -> Option<DesktopRect> {
    if let Ok(displays) = windows_integration::desktop_displays() {
        if let Some(display) = displays
            .iter()
            .find(|display| display.primary)
            .or_else(|| displays.first())
        {
            return Some(display.bounds);
        }
    }
    // 退回：主屏在虚拟桌面坐标里的原点恒为 (0,0)，尺寸就是 SM_CXSCREEN/SM_CYSCREEN
    // （进程是 Per-Monitor DPI Aware，因此这两个值就是物理像素）。
    // 旧代码这里读的是 Tauri 的 `window.primary_monitor()`，它给的正是主屏的
    // rcMonitor，与这两点定义完全一致。
    let (width, height) = primary_display_size();
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(DesktopRect {
        x: 0,
        y: 0,
        width,
        height,
    })
}

/// 窗口的缩放因子。
///
/// 这里是**纯值**读法，成因写清楚：Tauri 的 `scale_factor()` 最终落到 tao 的
/// `Window::scale_factor()`，那只是 `window_state.scale_factor` 的缓存副本
/// （tao-0.35.3 `platform_impl/windows/window.rs:507`），缓存值在窗口创建时与
/// 收到 `WM_DPICHANGED` 时由 `hwnd_dpi` 写入（同版本 `dpi.rs:71`，即
/// `GetDpiForWindow`，0 退回 96）。因此本函数读的是**同一个来源**，只是少了那次
/// 缓存中转：值相同，且不需要窗口对象。Tauri 那条路要发消息给事件循环，慢且不必。
#[cfg(windows)]
fn ball_scale_factor(hwnd: HWND) -> f64 {
    scale_factor_from_dpi(unsafe { GetDpiForWindow(hwnd) })
}

/// DPI → 缩放因子。0（无效句柄）按 tao 的做法退回 96，即 1.0。
///
/// 单独成函数是为了可测：读 DPI 那一步要有真窗口，换算这一步不需要。
#[cfg(windows)]
fn scale_factor_from_dpi(dpi: u32) -> f64 {
    let scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// 窗口的物理尺寸（像素）。以窗口自报的客户区尺寸为准，取不到时才用逻辑常量 × 缩放。
///
/// `GetClientRect` 就是 tao 的 `inner_size` 实现（tao-0.35.3
/// `platform_impl/windows/window.rs:254` 调 `util::client_rect`），所以这里读到的
/// 是同一个数，只是不再经事件循环中转。
#[cfg(windows)]
fn ball_physical_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width > 0 && height > 0 {
            return (width, height);
        }
    }
    let scale = ball_scale_factor(hwnd);
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

/// 原生窗口策略，一次做完。
///
/// 顺序不能换：**先去掉非客户区**（`apply_ball_ex_style`，含 `SWP_FRAMECHANGED`），
/// **再强制尺寸**（`force_ball_size`），**最后才算窗口区域**——区域是拿窗口的实际
/// 尺寸算的，提前算就会用上被系统夹过的旧尺寸。
///
/// 只跑在主线程上（创建窗口的那条链），所以这里只收句柄这一种输入。
#[cfg(windows)]
fn apply_ball_window_policy(hwnd: HWND) {
    apply_ball_ex_style(hwnd);
    force_ball_size(hwnd);
    refresh_ball_region(hwnd);
    raise_above_progman(hwnd);
}

/// 把窗口尺寸强制成设计尺寸（物理像素）。
///
/// ⚠ 必须显式设置，不能只靠 builder 的 `.inner_size()`：实测（0.2.0.89）窗口被创建成
/// **202×84** 而不是 84×84（56 逻辑像素 × 1.5）。原因是创建那一刻窗口还带着 `WS_CAPTION`
/// ——带标题栏/系统菜单的窗口，Windows 会把宽度夹到 `SM_CXMINTRACK`（本机 150% 缩放下约
/// 204 物理像素，正是标题栏图标所需的最小宽度）。等我们把非客户区去掉后，这个夹取不会
/// 自动消失，窗口就一直比设计尺寸宽一倍多，圆形区域也跟着被拉成椭圆。
#[cfg(windows)]
fn force_ball_size(hwnd: HWND) {
    let scale = ball_scale_factor(hwnd);
    let width = (BALL_LOGICAL_WIDTH * scale).round().max(1.0) as i32;
    let height = (BALL_LOGICAL_HEIGHT * scale).round().max(1.0) as i32;
    let mut current = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut current) }.is_ok()
        && current.right - current.left == width
        && current.bottom - current.top == height
    {
        return;
    }
    if let Err(error) = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    } {
        log::warn!("floating ball: forcing the ball size failed: {error}");
    }
    let mut after = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut after) }.is_ok() {
        log::info!(
            "floating ball: size forced to {}x{} (rect={}x{})",
            width,
            height,
            after.right - after.left,
            after.bottom - after.top
        );
    }
}

/// ex-style：**加** `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，**清** `WS_EX_APPWINDOW`。
///
/// 与 contract §3.1 的窗口配方一致：工具窗（不进任务栏、不进 Alt-Tab）+ 永不激活
/// （球永远不抢前台）。**不加** `WS_EX_TOPMOST`（球要待在普通应用之下），
/// **不加** `WS_EX_TRANSPARENT`（球本身必须收得到鼠标）。
///
/// 幂等，而且**每次弹出都会再断言一次**（见监控线程的两条显示路径）：用户实测回报过
/// "单击球之后球内部出现最小化/恢复/关闭的状态栏"——正是标题栏回来了。样式断言只在创建时
/// 做一次是不够的：只要有任何一条路径让窗口重新带上非客户区，用户看到的就是一个带标题栏的球。
/// 代价是两次 `GetWindowLongPtr`，只在真的不一致时才写样式，所以重复调用不会产生日志噪音。
#[cfg(windows)]
fn apply_ball_ex_style(hwnd: HWND) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = (current | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize)
            & !(WS_EX_APPWINDOW.0 as isize);
        let mut corrected = desired != current;
        if desired != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        // 窗口样式：必须是一个**没有任何非客户区**的顶层弹出窗口。
        //
        // ⚠ 实测（安装版 0.2.0.88，用户截图里能看到 "DSH Wallpaper Ball" 标题栏和一个关闭按钮）：
        // builder 的 `.decorations(false)` 对**创建后一直没显示过**的窗口不生效——实测样式
        // `0x04CB0000` 正是 `WS_OVERLAPPEDWINDOW` 去掉 `WS_THICKFRAME`（`resizable(false)` 的
        // 结果）再加上 `WS_CLIPSIBLINGS`，也就是 `WS_CAPTION`（标题栏）与 `WS_SYSMENU`
        // （关闭按钮）都还在，而 `WS_POPUP` 从未设置过。所以这里**显式定样式**，不再依赖 builder；
        // 改完用 `SWP_FRAMECHANGED` 让外壳重算非客户区。
        let current_style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let non_client = WS_CAPTION.0 as isize
            | WS_SYSMENU.0 as isize
            | WS_MINIMIZEBOX.0 as isize
            | WS_MAXIMIZEBOX.0 as isize
            | WS_THICKFRAME.0 as isize
            | WS_CHILD.0 as isize; // 顶层窗口一旦带上 WS_CHILD 会被挂进父链，必须清掉
        let desired_style = (current_style & !non_client) | WS_POPUP.0 as isize;
        if desired_style != current_style {
            corrected = true;
            SetWindowLongPtrW(hwnd, GWL_STYLE, desired_style);
            if let Err(error) = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            ) {
                log::warn!("floating ball: SWP_FRAMECHANGED after style change failed: {error}");
            }
        }
        // 读回：标题栏到底去掉没有，必须是日志里一眼可见的结论，而不是靠截图判断。
        let final_style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let popup = final_style & WS_POPUP.0 as isize != 0;
        let caption = final_style & WS_CAPTION.0 as isize != 0;
        let sysmenu = final_style & WS_SYSMENU.0 as isize != 0;
        if corrected {
            log::info!(
                "floating ball: window frame corrected exstyle=0x{:X} style=0x{:X} popup={popup} caption={caption} sysmenu={sysmenu}",
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize,
                final_style as usize
            );
        }
        if !popup || caption || sysmenu {
            log::warn!(
                "floating ball: window frame is not borderless (popup={popup} caption={caption} sysmenu={sysmenu}); a title bar will be visible"
            );
        }
    }
}

/// 把窗口区域裁成胶囊形。
///
/// Windows 的命中测试被限制在窗口区域内，因此圆角之外的透明像素既画不出东西，
/// 也**吞不掉桌面点击**——这正是「球的矩形之外完全不挡桌面点击」的前提之一
/// （另一半是隐藏时整窗移出屏幕）。缩放因子变化时必须重建：区域是物理像素的。
#[cfg(windows)]
fn refresh_ball_region(hwnd: HWND) {
    let (width, height) = ball_physical_size(hwnd);
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

/// 「球可以因为这个位置而弹出吗」的最后一条判据：光标必须落在桌面表面。
///
/// 桌面宿主的句柄取自 `windows_integration` 的句柄槽位（`desktop_surface_allows_pop_now`，
/// 由主线程在窗口创建处写入）。**这条线程里不允许再出现 `get_webview_window`**：那会
/// `Webview::clone`，与 `Context` 共用同一份引用计数（见 `start_ball_monitor` 的说明）。
///
/// 句柄为 0（桌面宿主还不存在）时判否，而不是把判据交给一个空句柄去走 Win32 查询 ——
/// 与旧封装 `cursor_on_desktop_surface_via_label` 里 `.unwrap_or(false)` 的结论一致。
/// 句柄已经不在时同理先判否：桌面表面那条判据在 `current == background` 不成立时，仍可能
/// 命中它内部的 `GetDesktopWindow` 与窗口类名两条分支而返回真。
///
/// 这一条是悬浮球在本轮修复里的关键：句柄只解析一次的旧写法下，`background` 被重建之后
/// 这里永远判否，表现就是"球从此不再弹出"。槽位让它每拍都拿到当前句柄。
///
/// 可测的那一半在 `windows_integration`（纯判据）与
/// `floating_ball::the_slot_decides_whether_the_ball_may_pop`（本模块到槽位的接线）。
#[cfg(windows)]
fn desktop_surface_allows_pop() -> bool {
    windows_integration::desktop_surface_allows_pop_now()
}

/// 16ms 靠近检测：光标靠近底部 ⇒ 滑入；离开球体范围 400ms ⇒ 收回。
///
/// 与 `start_foreground_monitor` / `start_desktop_workspace_monitor` 同形：
/// `std::thread::Builder` + `loop` + `sleep`，窗口消失即退出线程。
/// 状态迁移全部落 info 日志，便于在日志文件里直接观察行为。
///
/// ⚠ 运行期窗口状态**只能在主线程上取**：`get_webview_window` 会 `Webview::clone`，
/// 那一步动的是 wry `Context` 的引用计数（`Context::clone` 的溢出哨兵 = `ud2` /
/// `__fastfail(7)`，见 dsh-wallpaper.exe.8376.dmp 的取证）。旧代码在 16ms 轮询里每拍
/// `app.get_webview_window(BALL_LABEL).is_none()` —— 那正是后台线程逐拍 clone 引用
/// 计数的那一处，已删除。句柄在这里（主线程）解析一次，线程闭包里只剩 `HWND`
/// 这个纯值，存活判断改用 Win32 的 `IsWindow`；缩放因子与客户区尺寸也都从该句柄读
/// （见 `ball_scale_factor` / `ball_physical_size` 的注释：读的就是 tao 缓存的那同一个来源）。
///
/// `HWND` 内部是裸指针、不是 `Send`，所以跨线程搬的是它的**整数**形式，进闭包再还原。
///
/// 搬进线程的一共两个句柄：球自己的（`ball_raw`）与桌面宿主的（`background_raw`，
/// 给"只在桌面上弹"那条判据用）。后者此前是在轮询里经
/// `windows_integration::cursor_on_desktop_surface_via_label(&app)` 每拍现取的 ——
/// 那条封装内部又是 `app.get_webview_window("background")`，于是球隐藏、光标进热区
/// 的"准备弹出"路径每 16ms 就后台 clone 一次引用计数；封装与调用点都已删除。
///
/// 防线与它的边界：闭包入口的 `assert_not_acting_as_main_thread()` 只抓得住
/// 「后台线程带着主线程标记」这一种错法（把后台工作塞进主线程闭包，或反过来复用标记），
/// 它**抓不到**「后台线程不带标记、直接去碰运行期窗口状态」—— 那种写法照样会 clone
/// 引用计数，只能靠本模块"线程里只出现纯值句柄"这条纪律来守。
#[cfg(windows)]
pub fn start_ball_monitor(app: AppHandle) {
    // 窗口句柄由**主线程**解析：槽位里的值就是唯一来源（见
    // `windows_integration::BACKGROUND_WINDOW_HANDLE`），这里只在它还是空的时候补刷一次，
    // 正常路径上不多取一次窗口、时序与旧写法一致。**注意这是主线程**：球监控线程绝不允许
    // 调 `get_webview_window`（那会 clone wry `Context` 的引用计数，见上）。
    let window = app.get_webview_window(BALL_LABEL);
    let window_missing = window.is_none();
    let ball_raw = window
        .as_ref()
        .and_then(|window| window.hwnd().ok())
        .map(|handle| handle.0 as isize)
        .unwrap_or_default();
    drop(window);
    if ball_window_handle_now() == 0 {
        windows_integration::publish_ball_window_handle(
            boot_ball_handle(0, ball_raw),
        );
    }
    std::thread::Builder::new()
        .name("floating-ball-monitor".into())
        .spawn(move || {
        // 防线：这条闭包**绝不允许**再回 Tauri 取运行期窗口状态（见上）。真这么干了，
        // 调试构建会在这里带线程名喊出来，而不是等几个月后在主线程里崩成垃圾指针。
        assert_not_acting_as_main_thread();
        // 每拍从槽位取**当前**句柄：球的窗口被销毁并重建（或句柄槽位漏写修复之后）时，
        // 这里拿到的才是新句柄，而不是启动时冻结的旧值。
        let hwnd = match boot_ball_handle(ball_window_handle_now(), ball_raw) {
            Some(handle) => handle,
            None => {
                // 两个失败分支（窗口不存在 / 窗口没有原生句柄）各自保留原来的日志措辞。
                if window_missing {
                    log::warn!("floating ball: monitor not started (window missing)");
                } else {
                    log::warn!("floating ball: monitor not started (no native handle)");
                }
                return;
            }
        };
        log::info!("floating ball: approach monitor started interval={BALL_POLL_INTERVAL_MS}ms");

        let mut state = BallApproachState {
            shown: false,
            last_pointer_inside: std::time::Instant::now(),
            geometry: None,
            region_size: None,
            last_geometry_refresh: std::time::Instant::now(),
            pop_cooldown_until: None,
            cooldown_logged: false,
            // 岛常驻这条只在状态变化时记一次，不刷屏。
            pinned_logged: false,
            // 点过球之后的状态：等输入岛接管。岛一旦发布 `chat` 热区就解除；
            // 超时（安全阀）也解除，免得球永远不出现。
            waiting_for_island_since: None,
        };

        loop {
            std::thread::sleep(std::time::Duration::from_millis(BALL_POLL_INTERVAL_MS));
            // 每拍读槽位，而不是只认启动时那一个句柄。球自己的窗口被重建（或槽位漏写
            // 被修正）之后这里会自动跟上；读的只是一次原子操作，不碰 Tauri。
            let hwnd = match current_ball_handle(hwnd) {
                Some(handle) => handle,
                None => {
                    // 球没了（进程收尾、WebView 崩溃）就结束线程，不留无主轮询。
                    // 与旧写法同义：判据仍然直接问 Win32，不回 Tauri 的窗口注册表。
                    log::info!("floating ball: monitor stopped (window gone)");
                    break;
                }
            };
            approach_tick(&app, hwnd, &mut state);
        }
        })
        .expect("无法启动悬浮球监控线程");
}

/// 悬浮球靠近检测的**一拍**：读几何、判岛与冷却、滑入/滑出。
///
/// 抽成独立函数只是为了把「一拍」和「一条线程的生命周期」分开——本轮修的是后者
/// （句柄改为每拍从槽位读），这里与旧写法逐行一致，行为、时序、日志都不变。
///
/// 它**不**取任何 Tauri 运行期窗口状态：`hwnd` 是主线程写进槽位的纯值
/// （见 `windows_integration::BACKGROUND_WINDOW_HANDLE`），`app` 只用 `try_state`
/// 一类跨线程安全的通道。
#[cfg(windows)]
fn approach_tick(app: &AppHandle, hwnd: HWND, state: &mut BallApproachState) {
    let Some(pointer) = cursor_position() else {
        return;
    };
    let primary_size = primary_display_size();
    let scale = ball_scale_factor(hwnd);
    let refresh = ball_geometry_refresh_due(
        state.geometry.as_ref(),
        primary_size,
        scale,
        state.last_geometry_refresh.elapsed().as_millis(),
    );
    if refresh {
        match compute_ball_geometry(hwnd) {
            Some(mut next) => {
                if state.region_size != Some((next.width, next.height)) {
                    // 缩放因子变了（窗口被拖到别的 DPI，或系统改缩放）：区域要重建，
                    // 否则圆角外的透明像素会继续按旧尺寸吞点击。
                    refresh_ball_region(hwnd);
                    state.region_size = Some((next.width, next.height));
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
                let display_changed = state.geometry
                    .map(|current| {
                        current.primary_size != next.primary_size
                            || (current.scale - next.scale).abs() > f64::EPSILON
                    })
                    .unwrap_or(true);
                if state.shown && !display_changed {
                    if let Some(current) = state.geometry {
                        if current.shown_y != next.shown_y {
                            next.shown_y = current.shown_y;
                        }
                    }
                }
                let repositioned = state.geometry
                    .map(|current| (current.x, current.shown_y) != (next.x, next.shown_y))
                    .unwrap_or(false);
                state.geometry = Some(next);
                if repositioned && state.shown {
                    // 显示中几何真的变了（主屏尺寸 / 缩放变化）：直接摆到新位置，
                    // 不做滑入动画，也不改 Z 序。
                    if !move_ball_position(hwnd, next.x, next.shown_y) {
                        log::warn!("floating ball: reposition failed reason=geometry-changed");
                    }
                }
            }
            None => log::warn!("floating ball: geometry unavailable; keeping last position"),
        }
        state.last_geometry_refresh = std::time::Instant::now();
    }
    let Some(current) = state.geometry else {
        return;
    };

    let now = std::time::Instant::now();
    // 输入岛已经在前面（或前端点了球、要求立刻退场）⇒ 球必须马上退场，且不再弹出。
    //
    // 这是用户明确提出的要求：「当输入岛处于可见状态的时候，悬浮球就不应该弹出来」。
    // 判据来自前端发布的热区列表（含 id `chat` 即展开态），所以不需要为它再加 IPC。
    // 岛常驻（中央玻璃悬浮）⇒ 球没有职责，永远不弹。
    //
    // 这一条是**状态**判据，不是从热区列表推出来的：实测（用户的报告）在中央玻璃悬浮下
    // 岛一直在屏幕上，但前端重挂载时会发布一次空列表，日志里
    // `interaction regions: island_visible=false region_count=0` 之后紧接
    // `floating ball: shown reason=approach` —— 球抓的就是那个空窗。列表判据保留给
    // 任务栏停靠胶囊（那种布局下岛真的会消失）。
    let island_pinned = app
        .try_state::<crate::app_core::AppCore>()
        .map(|core| core.snapshot().island_pinned)
        .unwrap_or(false);
    if island_pinned && !state.pinned_logged {
        state.pinned_logged = true;
        log::info!("floating ball: staying hidden reason=island-pinned (center-glass layout)");
    }
    let regions_visible = windows_integration::island_visible_from_regions();
    let island_visible = island_pinned || regions_visible;
    let retract = take_retract_request();
    if retract {
        // 点过球 = 把场面交给输入岛。冷却期内即使热区列表短暂没有 chat，
        // 球也不许再冒头（原因见 BALL_POP_COOLDOWN_AFTER_CLICK_MS）。
        state.pop_cooldown_until = Some(
            now + std::time::Duration::from_millis(BALL_POP_COOLDOWN_AFTER_CLICK_MS),
        );
        if state.waiting_for_island_since.is_none() {
            state.waiting_for_island_since = Some(now);
            log::info!("floating ball: waiting for the island to take over");
        }
    }
    // 岛接管了就解除等待；超时也解除（安全阀），并且如实记账。
    if let Some(since) = state.waiting_for_island_since {
        if island_visible {
            state.waiting_for_island_since = None;
            log::info!("floating ball: island took over; handover complete");
        } else if now.duration_since(since)
            >= std::time::Duration::from_millis(BALL_ISLAND_HANDOVER_TIMEOUT_MS)
        {
            state.waiting_for_island_since = None;
            log::warn!(
                "floating ball: the island never became visible within {}ms; the ball may pop again",
                BALL_ISLAND_HANDOVER_TIMEOUT_MS
            );
        }
    }
    let waiting_for_island = state.waiting_for_island_since.is_some();
    let in_cooldown = state.pop_cooldown_until.is_some_and(|until| now < until);
    // 冷却期内**两条弹出路径都不许走**（不只「靠近」那条）。
    //
    // 实测（0.2.0.91）：收回是分 6 步滑出去的（36ms），光标此刻正停在球原来的位置上，
    // 于是「光标已在球上」那条分支在滑出过程中又把球显示出来，日志里看起来就是
    // 「刚收回又弹出」。冷却期必须对所有弹出路径生效。
    if in_cooldown {
        if !state.cooldown_logged {
            state.cooldown_logged = true;
            log::info!(
                "floating ball: staying hidden reason=post-click-cooldown {}ms",
                BALL_POP_COOLDOWN_AFTER_CLICK_MS
            );
        }
    } else if state.cooldown_logged {
        state.cooldown_logged = false;
        log::info!("floating ball: post-click-cooldown over; the ball may pop again");
    }
    if state.shown && (island_visible || retract) {
        if !slide_ball(hwnd, current.x, current.shown_y, current.hidden_y) {
            log::warn!("floating ball: slide-out failed reason=island-or-retract");
        }
        raise_above_progman(hwnd);
        state.shown = false;
        log::info!(
            "floating ball: hidden reason={} {}",
            if island_pinned {
                "island-pinned"
            } else if regions_visible {
                "island-visible"
            } else {
                "retract-requested"
            },
            window_rect_text(hwnd)
        );
    } else if !in_cooldown
        && !waiting_for_island
        && (point_inside_ball(hwnd, pointer) || cursor_over_ball(hwnd, pointer))
    {
        state.last_pointer_inside = now;
        if !state.shown {
            // 弹出前再断言一次"没有非客户区"：用户实测回报过球里出现最小化/恢复/关闭的
            // 状态栏（= 标题栏回来了），而窗口样式不该有任何一条路径能悄悄改回来。
            apply_ball_ex_style(hwnd);
            if !slide_ball(hwnd, current.x, current.hidden_y, current.shown_y) {
                log::warn!("floating ball: slide-in failed");
            }
            raise_above_progman(hwnd);
            state.shown = true;
            log::info!(
                "floating ball: shown reason=pointer-in-ball {}",
                window_rect_text(hwnd)
            );
        }
        return;
    }

    let approaching = point_in_trigger(pointer, &current);
    if state.shown {
        // 光标停在底部热区里也算「还在球上」。契约只写了「离开球体范围 400ms 收回」，
        // 但弹出位置比底部热区高出 expanded_bottom_inset（任务栏隐藏时约 1.5 个任务栏
        // 高度），光标一旦停在最底下那几像素，严格执行就会变成「弹出 → 400ms → 收回
        // → 下一拍立刻再弹出」的抖动。这一条是对契约的最小补充（见交付说明）。
        if approaching {
            state.last_pointer_inside = now;
        } else if now.duration_since(state.last_pointer_inside)
            >= std::time::Duration::from_millis(BALL_HIDE_DELAY_MS)
        {
            if !slide_ball(hwnd, current.x, current.shown_y, current.hidden_y) {
                log::warn!("floating ball: slide-out failed");
            }
            raise_above_progman(hwnd);
            state.shown = false;
            log::info!(
                "floating ball: hidden reason=pointer-left {}",
                window_rect_text(hwnd)
            );
        }
    } else if approaching
        && !island_visible
        && !in_cooldown
        && !waiting_for_island
        && desktop_surface_allows_pop()
    {
        // 只在桌面上弹：最大化应用的底边同样贴着屏幕下边缘，少了这一条就会在
        // 应用上面弹出球。判据直接复用桌面宿主那条（光标下的窗口父链能走到 Progman）；
        // 句柄取自句柄槽位（主线程在窗口创建处写入），所以 `background` 被重建之后它自动
        // 跟上新句柄，而不再回 Tauri 取窗口。放在条件链最后一位是有意的：短路求值让它
        // 只在"其他判据都通过、马上要弹"那一拍才真的去读槽位与问 Win32，与旧写法的调用次数一致。
        // 弹出前再断言一次"没有非客户区"，理由见另一条显示路径上的同一句。
        apply_ball_ex_style(hwnd);
        if !slide_ball(hwnd, current.x, current.hidden_y, current.shown_y) {
            log::warn!("floating ball: slide-in failed");
        }
        raise_above_progman(hwnd);
        state.shown = true;
        state.last_pointer_inside = now;
        log::info!(
            "floating ball: shown reason=approach cursor={},{} {}",
            pointer.x,
            pointer.y,
            window_rect_text(hwnd)
        );
    }
}

#[cfg(not(windows))]
pub fn start_ball_monitor(_: AppHandle) {}

/// 悬浮球靠近检测跨拍保留的状态。
///
/// 抽成结构体只是为了让「一拍」成为一个可调用的函数（`approach_tick`）：本轮改的是
/// 线程的生命周期（句柄改为每拍从槽位读），一拍的逻辑本身不动。
#[cfg(windows)]
struct BallApproachState {
    /// 球此刻是否在屏幕上（滑入到位）。
    shown: bool,
    /// 光标最后一次被认定"还在球上/还在热区里"的时刻，决定 400ms 收回。
    last_pointer_inside: std::time::Instant,
    /// 上一次算出的弹出/隐藏几何。
    geometry: Option<BallGeometry>,
    /// 上一次建窗口区域用的尺寸（缩放变化时要重建）。
    region_size: Option<(i32, i32)>,
    last_geometry_refresh: std::time::Instant,
    pop_cooldown_until: Option<std::time::Instant>,
    cooldown_logged: bool,
    pinned_logged: bool,
    waiting_for_island_since: Option<std::time::Instant>,
}

/// 线程启动时用的球句柄：优先用槽位里的当前值，槽位还是空的就退回主线程刚刚解析的值。
///
/// 两个都是纯整数，**不能**在这里回 Tauri 取窗口（`get_webview_window` 会 clone wry
/// `Context` 的引用计数，见 `windows_integration` 开头的说明）。0 表示没有可用句柄。
#[cfg(windows)]
fn boot_ball_handle(slot: isize, parsed_on_main_thread: isize) -> Option<HWND> {
    if slot != 0 {
        Some(HWND(slot as *mut core::ffi::c_void))
    } else if parsed_on_main_thread != 0 {
        Some(HWND(parsed_on_main_thread as *mut core::ffi::c_void))
    } else {
        None
    }
}

/// 这一拍该操作的球句柄：**每拍**从槽位重读一次。
///
/// 为什么放在槽位里：球的窗口句柄不许在后台线程解析（那是要修的崩溃），只解析一次又会在
/// 窗口重建后失效。槽位由主线程在创建点写入（`ensure_ball`），于是这里每次读到的都是
/// 纯值整数、且是最新值。
///
/// 句柄已经不在时返回 `None`：悬浮球的语义与 `background` 不同 —— 全仓只有 `ensure_ball`
/// 一个出生点，且它只在启动接线里调用一次，所以"球没了"就是进程在收尾，线程该退场
/// （与旧写法的语义一致）。见 `windows_integration::keep_watching_after_tick` 的说明。
#[cfg(windows)]
fn current_ball_handle(previous: HWND) -> Option<HWND> {
    let slot = ball_window_handle_now();
    let (slot_handle, slot_alive) = if slot == 0 {
        (None, false)
    } else {
        let fresh = HWND(slot as *mut core::ffi::c_void);
        (Some(fresh), ball_handle_is_alive(fresh))
    };
    ball_tick_handle_decide(slot_handle, slot_alive, previous, ball_handle_is_alive(previous))
}

/// `current_ball_handle` 的**纯判据**：槽位有活着的句柄就用它（那是本拍最新的事实），
/// 槽位为空就沿用上一拍的。
///
/// 抽出来是为了能锁住那几个分支 —— 读槽位与 `IsWindow` 两步都在外面做（前者是原子读，
/// 后者是系统行为），喂给这里的都是纯参数：单测里造不出真窗口，"随便写一个整数当已销毁
/// 句柄"是会偶发失败的写法（句柄空间是会话级的，那个值可能正被本机另一个窗口占用）。
#[cfg(windows)]
fn ball_tick_handle_decide(
    slot_handle: Option<HWND>,
    slot_alive: bool,
    previous: HWND,
    previous_alive: bool,
) -> Option<HWND> {
    match slot_handle {
        Some(handle) => slot_alive.then_some(handle),
        None => previous_alive.then_some(previous),
    }
}

/// 句柄是不是一个活着的窗口。**纯 Win32 判据**，不取任何 Tauri 对象，可测（见测试）。
#[cfg(windows)]
fn ball_handle_is_alive(handle: HWND) -> bool {
    !handle.0.is_null() && unsafe { IsWindow(Some(handle)) }.as_bool()
}

/// 这一组盯的是悬浮球几何里**能测的那一半**：纯计算与纯判据。
///
/// 读窗口那一半（句柄 → 缩放因子 / 客户区尺寸）造不出真窗口，所以单测里不碰；
/// 「这条线程不在主线程上取窗口」这件事也测不了（测试线程不是主线程），它靠的是
/// `windows_integration` 的标记与防线，那里已有对应单测。
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// 本机形态的主屏：1920x1080、原点在虚拟桌面原点、缩放 1.0。
    fn reference_display() -> DesktopRect {
        DesktopRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    /// DPI 换算必须与 tao 一致：96 是 1.0，0（无效句柄）也退回 1.0。
    #[test]
    fn the_dpi_conversion_matches_the_cached_tao_values() {
        assert!((scale_factor_from_dpi(96) - 1.0).abs() < f64::EPSILON);
        assert!((scale_factor_from_dpi(120) - 1.25).abs() < f64::EPSILON);
        assert!((scale_factor_from_dpi(144) - 1.5).abs() < f64::EPSILON);
        assert!((scale_factor_from_dpi(192) - 2.0).abs() < f64::EPSILON);
        // 无效句柄：GetDpiForWindow 返回 0，tao 同样按 96 处理。
        assert!((scale_factor_from_dpi(0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn geometry_centers_the_ball_over_the_primary_display() {
        let geometry = ball_geometry_from_values(1.0, (56, 56), reference_display(), 48.0);
        assert_eq!(geometry.primary_size, (1920, 1080));
        assert_eq!(geometry.width, 56);
        assert_eq!(geometry.height, 56);
        assert_eq!(geometry.x, (1920 - 56) / 2);
        // 下边缘 1080；留白 48 逻辑像素在 1.0 缩放下就是 48 物理像素。
        assert_eq!(geometry.shown_y, 1080 - 48 - 56);
        assert_eq!(geometry.hidden_y, 1080 + BALL_HIDDEN_Y_OFFSET);
    }

    /// 隐藏位置必须让整窗落在主屏之下：契约 §3.1 的几何方式要求隐藏时 0 像素可见。
    #[test]
    fn the_hidden_position_leaves_no_pixel_on_screen() {
        let display = reference_display();
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let height = (BALL_LOGICAL_HEIGHT * scale).round() as i32;
            let geometry = ball_geometry_from_values(scale, (56, height), display, 48.0);
            assert!(
                geometry.hidden_y >= display.y + display.height,
                "缩放 {scale} 下隐藏位置必须仍在下边缘之外"
            );
        }
    }

    /// 缩放变化这条分支：留白按缩放换成物理像素，球的物理尺寸也跟着变，
    /// 于是弹出位置整体上移（下边缘固定）。这三个数一起动，缺一个就会算错。
    #[test]
    fn a_scale_change_moves_the_shown_position_with_the_inset() {
        let display = reference_display();
        let at_100 = ball_geometry_from_values(1.0, (56, 56), display, 48.0);
        let at_150 = ball_geometry_from_values(1.5, (84, 84), display, 48.0);
        assert_eq!(at_100.shown_y, 1080 - 48 - 56);
        // 48 逻辑像素在 1.5 倍下是 72 物理像素，球本身也变成 84 物理像素。
        assert_eq!(at_150.shown_y, 1080 - 72 - 84);
        assert!(at_150.shown_y < at_100.shown_y);
        // 隐藏位置只跟着主屏下边缘走，与缩放无关。
        assert_eq!(at_150.hidden_y, at_100.hidden_y);
    }

    /// 主屏矩形带偏移时（多屏、非原点主屏的算法形态），x 与下边缘都按它的矩形算。
    #[test]
    fn geometry_follows_the_primary_display_rectangle() {
        let display = DesktopRect {
            x: 0,
            y: -240,
            width: 1920,
            height: 1080,
        };
        let geometry = ball_geometry_from_values(1.0, (56, 56), display, 48.0);
        assert_eq!(geometry.x, (1920 - 56) / 2);
        // 下边缘 = -240 + 1080 = 840。
        assert_eq!(geometry.shown_y, 840 - 48 - 56);
        assert_eq!(geometry.hidden_y, 840 + BALL_HIDDEN_Y_OFFSET);
    }

    #[test]
    fn the_first_tick_always_computes_geometry() {
        assert!(ball_geometry_refresh_due(None, (1920, 1080), 1.0, 0));
    }

    /// 缩放变化不等到周期到点就该重算（否则球的物理尺寸换了、位置还是旧的）。
    #[test]
    fn a_scale_change_forces_a_refresh_before_the_interval() {
        let current = ball_geometry_from_values(1.0, (56, 56), reference_display(), 48.0);
        assert!(!ball_geometry_refresh_due(
            Some(&current),
            current.primary_size,
            1.0,
            0
        ));
        assert!(ball_geometry_refresh_due(
            Some(&current),
            current.primary_size,
            1.5,
            0
        ));
    }

    #[test]
    fn a_primary_size_change_forces_a_refresh_before_the_interval() {
        let current = ball_geometry_from_values(1.0, (56, 56), reference_display(), 48.0);
        assert!(ball_geometry_refresh_due(
            Some(&current),
            (2560, 1440),
            1.0,
            0
        ));
    }

    /// 什么都没变时按周期重算：差 1ms 不重算，到点重算。
    #[test]
    fn the_interval_forces_a_refresh_when_nothing_changed() {
        let current = ball_geometry_from_values(1.0, (56, 56), reference_display(), 48.0);
        let just_before = BALL_GEOMETRY_REFRESH_INTERVAL_MS as u128 - 1;
        assert!(!ball_geometry_refresh_due(
            Some(&current),
            current.primary_size,
            1.0,
            just_before
        ));
        assert!(ball_geometry_refresh_due(
            Some(&current),
            current.primary_size,
            1.0,
            BALL_GEOMETRY_REFRESH_INTERVAL_MS as u128
        ));
    }

    /// 空句柄一律判否：句柄槽位里没有桌面宿主时不许弹出。
    ///
    /// 这一条以前藏在封装尾部的 `.unwrap_or(false)` 里，现在由句柄槽位承担：句柄缺失必须
    /// 落到"不弹"，而不是把判据交给一个空句柄去走 Win32 查询。
    ///
    /// 这里**不**测"写一个整数当已销毁句柄"：句柄空间是会话级的，那个值可能正被本机另一个
    /// 窗口占用，那种断言会偶发失败。"句柄不在"这条语义由 `windows_integration` 的
    /// `a_missing_or_destroyed_window_keeps_the_watching_thread_alive` 用纯判据锁住，
    /// `IsWindow` 本身是系统行为，不由本仓库的测试断言。
    #[test]
    fn the_slot_decides_whether_the_ball_may_pop() {
        let previous = windows_integration::background_window_handle_now();
        windows_integration::publish_background_window_handle(None);
        assert!(!desktop_surface_allows_pop());
        // 还原现场：槽位是进程级的，别把别的测试带歪。
        windows_integration::publish_background_window_handle(if previous == 0 {
            None
        } else {
            Some(HWND(previous as *mut core::ffi::c_void))
        });
    }

    /// 判活的边界：空句柄一律为否。
    #[test]
    fn a_null_ball_handle_is_never_alive() {
        assert!(!ball_handle_is_alive(HWND(
            std::ptr::null_mut::<core::ffi::c_void>()
        )));
    }

    /// 每拍取球句柄的三态：读到活着的句柄就用它；读到 0 沿用上一拍的；两者都不在就退场。
    ///
    /// 第三态是既有语义（球真的没了就结束线程，不留无主轮询），前两态是本轮的修复目标
    /// （窗口重建后线程要跟上新句柄，而不是退场）。
    ///
    /// 这里喂的是判据的纯参数，不碰真窗口：句柄空间是会话级的，**不能**拿一个随便写的
    /// 整数当"已销毁句柄"——那个值可能正被本机另一个窗口占用，那种测试会偶发失败。
    /// 跨线程性本身也测不了（测试线程不是主线程），它靠的是"线程里只出现纯值句柄"这条纪律
    /// 与 `windows_integration::assert_not_acting_as_main_thread`。
    #[test]
    fn the_tick_handle_follows_the_slot_and_leaves_when_the_ball_is_gone() {
        let previous = HWND(2 as *mut core::ffi::c_void);
        let fresh = HWND(4 as *mut core::ffi::c_void);
        // 槽位里有活着的句柄（窗口重建后主线程刚写入的新值）：用它。
        assert_eq!(
            ball_tick_handle_decide(Some(fresh), true, previous, true).map(|h| h.0 as isize),
            Some(4)
        );
        // 槽位读到 0：沿用上一拍的句柄（还活着就用它）。
        assert_eq!(
            ball_tick_handle_decide(None, false, previous, true).map(|h| h.0 as isize),
            Some(2)
        );
        // 槽位里的句柄已经不在：退场，不会拿一个坏句柄继续轮询。
        assert!(ball_tick_handle_decide(Some(fresh), false, previous, true).is_none());
        // 槽位为空且上一拍的句柄也没了：同样退场。
        assert!(ball_tick_handle_decide(None, false, previous, false).is_none());
    }

    /// 线程启动时的球句柄：槽位优先，槽位为空才退回主线程刚解析的值，两个都是 0 才算没有。
    ///
    /// 这一支直接决定"监控线程会不会一启动就退场"，所以三个输入组合都要锁住。
    #[test]
    fn the_boot_handle_prefers_the_slot_and_falls_back_to_the_parsed_value() {
        let parsed = 0x1234_isize;
        let slot = 0x5678_isize;
        assert_eq!(
            boot_ball_handle(slot, parsed).map(|handle| handle.0 as isize),
            Some(slot)
        );
        assert_eq!(
            boot_ball_handle(0, parsed).map(|handle| handle.0 as isize),
            Some(parsed)
        );
        assert!(boot_ball_handle(0, 0).is_none());
    }
}
