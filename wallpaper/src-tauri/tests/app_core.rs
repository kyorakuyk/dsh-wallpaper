#[path = "../src/app_core.rs"]
mod app_core;

use app_core::{
    AppAction, AppCore, BackendMode, SystemPhase, WallpaperHostMode, WallpaperHostStatus,
};

#[test]
fn boot_and_unlock_only_show_interaction_after_wake_finishes() {
    let core = AppCore::default();

    let waking = core.dispatch(AppAction::BootReady { play_wake: true });
    assert_eq!(waking.phase, SystemPhase::Waking);
    assert!(!waking.interaction.visible);

    let idle = core.dispatch(AppAction::WakeDone);
    assert_eq!(idle.phase, SystemPhase::Idle);
    assert!(idle.interaction.visible);
}

#[test]
fn lock_enables_privacy_and_collapses_transient_ui() {
    let core = AppCore::default();
    core.dispatch(AppAction::BootReady { play_wake: false });
    core.dispatch(AppAction::OpenChat);
    core.dispatch(AppAction::ToggleHistory);

    let locked = core.dispatch(AppAction::Lock);
    assert_eq!(locked.phase, SystemPhase::Locked);
    assert!(locked.privacy_screen);
    assert!(!locked.interaction.visible);
    assert!(!locked.interaction.history_expanded);
    assert!(!locked.interaction.settings_open);
}

#[test]
fn returning_to_desktop_never_reopens_chat_or_settings() {
    let core = AppCore::default();
    core.dispatch(AppAction::BootReady { play_wake: false });
    core.dispatch(AppAction::OpenChat);
    core.dispatch(AppAction::ToggleHistory);

    let hidden = core.dispatch(AppAction::DesktopForegroundChanged(false));
    assert_eq!(hidden.phase, SystemPhase::Idle);
    assert!(!hidden.interaction.visible);
    assert!(!hidden.interaction.history_expanded);

    let restored = core.dispatch(AppAction::DesktopForegroundChanged(true));
    assert_eq!(restored.phase, SystemPhase::Idle);
    assert!(restored.interaction.visible);
    assert!(!restored.interaction.history_expanded);
    assert!(!restored.interaction.settings_open);
}

#[test]
fn disabling_interaction_is_a_persistent_user_preference() {
    let core = AppCore::default();
    core.dispatch(AppAction::BootReady { play_wake: false });
    core.dispatch(AppAction::SetInteractionEnabled(false));
    core.dispatch(AppAction::DesktopForegroundChanged(false));

    let restored = core.dispatch(AppAction::DesktopForegroundChanged(true));
    assert!(!restored.interaction.enabled);
    assert!(!restored.interaction.visible);
}

#[test]
fn wallpaper_host_status_is_part_of_the_versioned_snapshot() {
    let core = AppCore::default();
    let status = WallpaperHostStatus {
        mode: WallpaperHostMode::ProgmanFallback,
        generation: 3,
        recovery_count: 2,
        last_error: None,
    };
    let snapshot = core.dispatch(AppAction::SetWallpaperHost(status.clone()));
    assert_eq!(snapshot.wallpaper_host, status);
    assert_eq!(snapshot.revision, 1);
}

#[test]
fn backend_selection_does_not_change_system_phase() {
    let core = AppCore::default();
    core.dispatch(AppAction::BootReady { play_wake: false });

    let snapshot = core.dispatch(AppAction::SelectBackend(BackendMode::Harness));
    assert_eq!(snapshot.phase, SystemPhase::Idle);
    assert_eq!(snapshot.backend, BackendMode::Harness);
}
