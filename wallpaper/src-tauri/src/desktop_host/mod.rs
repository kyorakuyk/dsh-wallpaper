#[cfg(windows)]
use crate::app_core::WallpaperHostStatus;

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
mod workerw;
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use hit_region::{
    begin_interaction_region_session, island_visible_from_regions, update_interaction_regions,
    InteractionRegionInput, InteractionRegionUpdateResult,
};
#[allow(unused_imports)]
pub(crate) use hit_region::ISLAND_REGION_ID;
#[cfg(windows)]
use hit_region::{interaction_regions, MAX_SCALE_FACTOR, MIN_SCALE_FACTOR};
#[cfg(windows)]
#[allow(unused_imports)]
pub(crate) use workspace::{desktop_icon_layer, desktop_point_verdict};
#[cfg(windows)]
pub(crate) use workspace::set_desktop_icons_visible;
#[cfg(windows)]
pub(crate) use crate::desktop_repair::repair_desktop_after_abnormal_exit;
#[allow(unused_imports)]
pub(crate) use workspace::{DesktopPointVerdict, PairState, pair_transition};
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use workspace::{enter_inner_workspace, leave_inner_workspace};
#[cfg(windows)]
pub(crate) use workspace::restore_desktop_icons;
#[cfg(windows)]
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use workspace::start_desktop_workspace_monitor;
pub(crate) use workerw::{
    acquire_shared_wallpaper_host, register_session_events, start_wallpaper_host,
    unregister_session_events,
};
#[allow(unused_imports)]
pub(crate) use workerw::attach_to_workerw;
#[cfg(windows)]
#[allow(unused_imports)]
pub(crate) use workerw::{background_window_handle_now, publish_background_window_handle};
#[cfg(windows)]
#[cfg_attr(feature = "lite", allow(unused_imports))]
pub(crate) use workerw::{ball_window_handle_now, publish_ball_window_handle};
#[cfg(windows)]
pub(crate) use workerw::{
    publish_current_window_channels, request_wallpaper_worker, visible_wallpaper_worker,
};

#[cfg(windows)]
#[allow(unused_imports)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};
use windows::{
    Win32::{
        Foundation::POINT,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            Accessibility::{CUIAutomation, IUIAutomation, UIA_ListItemControlTypeId},
            WindowsAndMessaging::{
                GetCursorPos, GetForegroundWindow, WindowFromPoint,
            },
        },
    },
};

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
