use super::state::{INNER_WORKSPACE_ACTIVE, INTERACTION_REGIONS};
use super::{
    client_point, hand_over_keyboard_after_island_click, trace_focus_handoff,
    trace_island_click_reached,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use std::sync::RwLock;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetCursorPos, HTTRANSPARENT, WM_ACTIVATE, WM_LBUTTONDOWN, WM_MOUSEACTIVATE,
            WM_NCHITTEST, WM_NCDESTROY, WM_NCLBUTTONDOWN, WM_SETFOCUS,
        },
    },
};

const MAX_INTERACTION_REGIONS: usize = 128;
pub(super) const MIN_SCALE_FACTOR: f64 = 0.5;
pub(super) const MAX_SCALE_FACTOR: f64 = 8.0;

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
pub(super) struct PhysicalInteractionRegion {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

impl PhysicalInteractionRegion {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct InteractionRegionState {
    session: u64,
    revision: u64,
    scale_factor: f64,
    pub(super) regions: Vec<PhysicalInteractionRegion>,
    /// 最近一次发布里是否含 `ISLAND_REGION_ID`，也就是输入岛此刻是否可见。
    ///
    /// 悬浮球不参与热区判定（它是独立窗口，自己收鼠标），但它必须知道岛是否已经
    /// 在前面——否则球会在岛正上方冒出来。判定就放在发布热区这一步，避免为它再开
    /// 一条 IPC。
    island_visible: bool,
    /// **输入岛自己**那块矩形（发布时按 id 挑出来单独留一份）。
    ///
    /// 为什么单独存：内部结构 `PhysicalInteractionRegion` 只保留四个坐标，id 在转换时就丢了，
    /// 而"要不要做键盘交接"必须区分"命中的是岛"还是"命中的是立绘等其它热区"——两者都该归我们
    /// 收点击，但只有前者该抢前台与焦点（实测：立绘若也抢，一次点击会因 `WM_NCHITTEST` 重复
    /// 触发六次交接，表现为"点立绘闪一下"）。
    island_rect: Option<PhysicalInteractionRegion>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRegionUpdateResult {
    pub revision: u64,
    pub region_count: usize,
    pub stale: bool,
}

static INTERACTION_REGION_SESSION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) fn interaction_regions() -> &'static RwLock<InteractionRegionState> {
    INTERACTION_REGIONS.get_or_init(|| RwLock::new(InteractionRegionState::default()))
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

/// 这一点是否落在**输入岛自己**那块矩形里。
///
/// 与 `point_hits_interaction_region` 的分工：后者决定"点击归不归我们"（任何热区都算，立绘也算），
/// 前者决定"要不要做键盘交接、把前台与焦点抢过来"——那件事只有输入岛该做。
fn point_hits_island_rect(state: &InteractionRegionState, x: i32, y: i32) -> bool {
    state
        .island_rect
        .is_some_and(|rect| rect.contains(x, y))
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
    let island_rect = regions
        .iter()
        .find(|region| region.id == ISLAND_REGION_ID)
        .cloned()
        .and_then(|region| scale_interaction_region(region, scale_factor));
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
    state.island_rect = island_rect;
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
                // 点击归属（`hit`）与"要不要抢焦点"是两件事：立绘等其它热区只需要前者。
                let hit_is_island = interaction_regions()
                    .read()
                    .map(|state| point_hits_island_rect(&state, local_x, local_y))
                    .unwrap_or(false);
                // Inside the input island. `WM_NCHITTEST` reaches us where the mouse
                // messages do not, so this is where a real click can be recognised: the
                // point is in a declared region and the physical left button is down.
                // Outside the island nothing here runs, so the wallpaper still never
                // takes the keyboard without the user asking for it (plan 3.C).
                // SAFETY: GetAsyncKeyState takes only a virtual-key constant and reads global input state; it has no out-parameter and touches no Rust memory.
                if hit_is_island && unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0 {
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
pub(super) fn install_desktop_hit_testing(root_hwnd: HWND) -> Result<(), String> {
    // SAFETY: `root_hwnd` is the live background host the caller just attached, the proc is a static function, and the reference data is that same handle encoded as usize, so nothing is owned or freed here.
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
pub(super) fn cursor_hits_interaction_region(root_hwnd: HWND) -> bool {
    let mut cursor = POINT::default();
    let mut origin = POINT::default();
    // SAFETY: `root_hwnd` is the caller's live host handle, both out-parameters are local POINTs alive across the calls, and a failure returns false before any coordinate is used.
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

#[cfg(test)]
mod tests {
    use super::*;

    static REGION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    /// 交接只对岛那块矩形做：立绘等其它热区命中时，点击仍归我们，但不该抢前台与焦点。
#[test]
fn only_the_island_rect_asks_for_the_keyboard() {
  let island = PhysicalInteractionRegion {
    left: 100,
    top: 100,
    right: 300,
    bottom: 300,
  };
  let persona = PhysicalInteractionRegion {
    left: 500,
    top: 500,
    right: 800,
    bottom: 800,
  };
  let state = InteractionRegionState {
    regions: vec![island, persona],
    island_rect: Some(island),
    island_visible: true,
    ..Default::default()
  };
  // 岛内：归属成立，且**该**做交接。
  assert!(point_hits_interaction_region(&state.regions, 150, 150));
  assert!(point_hits_island_rect(&state, 150, 150));
  // 立绘内：归属仍成立（点击归我们），但**不该**做交接 —— 这就是"点立绘闪一下"的修复点。
  assert!(point_hits_interaction_region(&state.regions, 600, 600));
  assert!(!point_hits_island_rect(&state, 600, 600));
  // 两者之外：都不成立。
  assert!(!point_hits_interaction_region(&state.regions, 1000, 1000));
  assert!(!point_hits_island_rect(&state, 1000, 1000));
  // 岛不可见（未发布）时：任何一点都不做交接。
  let empty = InteractionRegionState::default();
  assert!(!point_hits_island_rect(&empty, 150, 150));
}

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
