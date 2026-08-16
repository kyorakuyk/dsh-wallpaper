use std::sync::RwLock;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemPhase {
    #[default]
    Booting,
    Locked,
    Waking,
    Idle,
    Chatting,
    AuthRequired,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendMode {
    #[default]
    DeepseekWeb,
    DeepseekApi,
    Harness,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Activity {
    #[default]
    Idle,
    Sending,
    Thinking,
    Streaming,
    Tool,
    Done,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessAvailability {
    #[default]
    Offline,
    WebOnly,
    BridgeReady,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WallpaperHostMode {
    #[default]
    Starting,
    WorkerW,
    ProgmanFallback,
    Recovering,
    Unavailable,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperHostStatus {
    pub mode: WallpaperHostMode,
    pub generation: u64,
    pub recovery_count: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionState {
    /// Persistent user preference. Foreground changes must not overwrite it.
    pub enabled: bool,
    /// Derived visibility of the transparent interaction surface.
    pub visible: bool,
    pub desktop_foreground: bool,
    pub history_expanded: bool,
    pub settings_open: bool,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            enabled: true,
            visible: false,
            desktop_foreground: true,
            history_expanded: false,
            settings_open: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub revision: u64,
    pub phase: SystemPhase,
    pub backend: BackendMode,
    pub activity: Activity,
    pub harness: HarnessAvailability,
    pub wallpaper_host: WallpaperHostStatus,
    pub interaction: InteractionState,
    /// When true, message content must not be rendered by either WebView.
    pub privacy_screen: bool,
    pub error: Option<String>,
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            phase: SystemPhase::Booting,
            backend: BackendMode::DeepseekWeb,
            activity: Activity::Idle,
            harness: HarnessAvailability::Offline,
            wallpaper_host: WallpaperHostStatus::default(),
            interaction: InteractionState::default(),
            privacy_screen: false,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppAction {
    BootReady { play_wake: bool },
    Lock,
    Unlock { play_wake: bool },
    WakeDone,
    OpenChat,
    CloseChat,
    OpenSettings,
    CloseSettings,
    ToggleHistory,
    SetInteractionEnabled(bool),
    DesktopForegroundChanged(bool),
    SelectBackend(BackendMode),
    SetActivity(Activity),
    SetHarnessAvailability(HarnessAvailability),
    SetWallpaperHost(WallpaperHostStatus),
    AuthRequired,
    AuthReady,
    Fail(String),
    Recover,
}

#[derive(Default)]
pub struct AppCore {
    state: RwLock<AppSnapshot>,
}

impl AppCore {
    pub fn snapshot(&self) -> AppSnapshot {
        self.state.read().expect("app core state poisoned").clone()
    }

    pub fn dispatch(&self, action: AppAction) -> AppSnapshot {
        let mut state = self.state.write().expect("app core state poisoned");
        let before = state.clone();

        match action {
            AppAction::BootReady { play_wake } => {
                state.phase = if play_wake {
                    SystemPhase::Waking
                } else {
                    SystemPhase::Idle
                };
                state.privacy_screen = false;
                close_transient_surfaces(&mut state);
            }
            AppAction::Lock => {
                state.phase = SystemPhase::Locked;
                state.privacy_screen = true;
                state.interaction.visible = false;
                close_transient_surfaces(&mut state);
            }
            AppAction::Unlock { play_wake } => {
                state.phase = if play_wake {
                    SystemPhase::Waking
                } else {
                    SystemPhase::Idle
                };
                state.privacy_screen = false;
                close_transient_surfaces(&mut state);
            }
            AppAction::WakeDone if state.phase == SystemPhase::Waking => {
                state.phase = SystemPhase::Idle;
            }
            AppAction::OpenChat if state.phase == SystemPhase::Idle => {
                state.phase = SystemPhase::Chatting;
                state.interaction.settings_open = false;
            }
            AppAction::CloseChat if state.phase == SystemPhase::Chatting => {
                state.phase = SystemPhase::Idle;
                state.interaction.history_expanded = false;
            }
            AppAction::OpenSettings if can_show_interaction(&state) => {
                state.interaction.settings_open = true;
                state.interaction.history_expanded = false;
                if state.phase == SystemPhase::Chatting {
                    state.phase = SystemPhase::Idle;
                }
            }
            AppAction::CloseSettings => state.interaction.settings_open = false,
            AppAction::ToggleHistory if state.phase == SystemPhase::Chatting => {
                state.interaction.history_expanded = !state.interaction.history_expanded;
            }
            AppAction::SetInteractionEnabled(enabled) => {
                state.interaction.enabled = enabled;
                if !enabled {
                    close_transient_surfaces(&mut state);
                    if state.phase == SystemPhase::Chatting {
                        state.phase = SystemPhase::Idle;
                    }
                }
            }
            AppAction::DesktopForegroundChanged(desktop_foreground) => {
                state.interaction.desktop_foreground = desktop_foreground;
                if !desktop_foreground {
                    // Returning to the desktop may restore the collapsed surface, but never
                    // reopen chat, history, or settings that had been visible before.
                    close_transient_surfaces(&mut state);
                    if state.phase == SystemPhase::Chatting {
                        state.phase = SystemPhase::Idle;
                    }
                }
            }
            AppAction::SelectBackend(backend) => state.backend = backend,
            AppAction::SetActivity(activity) => state.activity = activity,
            AppAction::SetHarnessAvailability(availability) => state.harness = availability,
            AppAction::SetWallpaperHost(status) => state.wallpaper_host = status,
            AppAction::AuthRequired => {
                state.phase = SystemPhase::AuthRequired;
                state.interaction.settings_open = false;
            }
            AppAction::AuthReady if state.phase == SystemPhase::AuthRequired => {
                state.phase = SystemPhase::Idle;
            }
            AppAction::Fail(message) => {
                state.phase = SystemPhase::Error;
                state.error = Some(message);
                close_transient_surfaces(&mut state);
            }
            AppAction::Recover => {
                state.phase = SystemPhase::Idle;
                state.activity = Activity::Idle;
                state.error = None;
            }
            _ => {}
        }

        recompute_interaction_visibility(&mut state);
        if *state != before {
            state.revision = state.revision.saturating_add(1);
        }
        state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_host_status_is_versioned_in_the_snapshot() {
        let core = AppCore::default();
        let status = WallpaperHostStatus {
            mode: WallpaperHostMode::WorkerW,
            generation: 2,
            recovery_count: 1,
            last_error: None,
        };
        let snapshot = core.dispatch(AppAction::SetWallpaperHost(status.clone()));
        assert_eq!(snapshot.wallpaper_host, status);
        assert_eq!(snapshot.revision, 1);
    }
}

fn can_show_interaction(state: &AppSnapshot) -> bool {
    state.interaction.enabled
        && state.interaction.desktop_foreground
        && matches!(state.phase, SystemPhase::Idle | SystemPhase::Chatting)
}

fn recompute_interaction_visibility(state: &mut AppSnapshot) {
    state.interaction.visible = can_show_interaction(state);
}

fn close_transient_surfaces(state: &mut AppSnapshot) {
    state.interaction.history_expanded = false;
    state.interaction.settings_open = false;
}
