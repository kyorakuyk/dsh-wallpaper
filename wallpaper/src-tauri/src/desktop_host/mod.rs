#[cfg(windows)]
mod events;
#[cfg(windows)]
pub(crate) use events::{dispatch_system_action, emit_system_session, emit_to_background};
#[cfg(windows)]
pub(crate) use crate::BACKGROUND_WINDOW_LABEL;

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
