#[cfg(windows)]
use super::{
    assert_not_acting_as_main_thread, background_handle_is_usable, background_window_handle_now,
    emit_to_background, enumerate_desktop_windows,
};
#[cfg(windows)]
use super::cursor_is_on_desktop_surface;
#[cfg(windows)]
use super::hit_region::cursor_hits_interaction_region;
#[cfg(windows)]
use super::state::INNER_WORKSPACE_ACTIVE;
#[cfg(windows)]
use std::sync::atomic::Ordering;
#[cfg(windows)]
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::{HWND, POINT},
        System::Com::{CoInitializeEx, COINIT_MULTITHREADED},
        UI::{
            Accessibility::{AccessibleObjectFromWindow, IAccessible},
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                FindWindowExW, GetCursorPos, ShowWindow, SW_HIDE, SW_SHOWNA,
            },
        },
    },
};

// 本功能的判据与配对见 docs/evidence/desktop-blank-double-click.md：判据是 MSAA 三态
// （Icon / Blank / Unknown，只有 Blank 允许切换），配对是两态状态机（严格两两匹配）。
// 中途曾按用户要求把这一整块冻结并回退过一次，那一代实现仍在 git 历史里（c9a9665 到 dc49983），
// 但其中的做法已被该文档判定为在 UIA 上原地调参的一类弯路，不要照它重走。

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
pub(super) fn set_desktop_icons_visible(visible: bool) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
