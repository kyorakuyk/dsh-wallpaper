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
    /// Something answers on 3080 but it is not a wallpaper Bridge.
    WebOnly,
    /// A Bridge answered and is still composing its service set. Waiting is the
    /// correct user action, so this must not look like a missing Bridge.
    BridgeLoading,
    /// The Bridge is mounted but its bearer token is unavailable, usually an
    /// ACL or Credential problem. Restarting the wallpaper may fix it; nothing
    /// the user can do in Harness mode will.
    BridgeAuthUnavailable,
    /// The Bridge answered with a contract this wallpaper cannot use (protocol
    /// version or capability set). Updating or reinstalling the Bridge is the
    /// fix, and retrying will not help.
    BridgeIncompatible,
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
    /// Explicit user choice. Only tray/settings controls may change it.
    pub enabled: bool,
    /// Derived visibility of the desktop interaction UI inside the shared
    /// WorkerW background host. This is not a separate native window.
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
    /// Stable, non-sensitive reason for a non-ready Harness state. It never
    /// carries an exception body, a path, or a token.
    pub harness_reason_code: Option<String>,
    /// A Bridge that was ready is not answering, and its death is not confirmed.
    ///
    /// The one fact the renderer cannot work out for itself: it decides the amber
    /// light, while `harness` deliberately stays `bridge-ready` so the mode switch,
    /// the model list and the session keep treating the subject as present.
    pub harness_probing: bool,
    pub wallpaper_host: WallpaperHostStatus,
    pub interaction: InteractionState,
    /// 输入岛是否被当前布局固定在桌面上（中央玻璃悬浮）。
    ///
    /// 悬浮球据此判定"岛在前台"，而不依赖热区列表 —— 那个列表在重挂载与相位切换时有空窗期。
    pub island_pinned: bool,
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
            harness_reason_code: None,
            harness_probing: false,
            wallpaper_host: WallpaperHostStatus::default(),
            interaction: InteractionState::default(),
            island_pinned: false,
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
    /// 中央玻璃悬浮把输入岛**固定在桌面上**（`ConversationBubble` 的 `persistent`）。
    ///
    /// 悬浮球的职责是"把岛叫出来"，岛常驻时它没有职责。判据不能只看热区列表：岛在重挂载、
    /// 相位切换时会有几十毫秒不在列表里，球恰好会抓住那个空窗弹出来（实测日志
    /// `island_visible=false region_count=0` 之后紧接 `floating ball: shown reason=approach`）。
    /// 因此布局把它作为**状态**报给原生，而不是让原生从瞬时列表里推断。
    SetIslandPinned(bool),
    DesktopForegroundChanged(bool),
    SelectBackend(BackendMode),
    SetActivity(Activity),
    SetHarnessAvailability(HarnessAvailability),
    /// Sets the availability and its reason code together, so a diagnostic can
    /// never be published without the state it explains.
    SetHarnessDiagnostic {
        availability: HarnessAvailability,
        reason_code: Option<String>,
        /// True while a ready Bridge is failing without confirmed death.
        probing: bool,
    },
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
            AppAction::SetIslandPinned(pinned) => state.island_pinned = pinned,
            AppAction::DesktopForegroundChanged(desktop_foreground) => {
                state.interaction.desktop_foreground = desktop_foreground;
            }
            AppAction::SelectBackend(backend) => state.backend = backend,
            AppAction::SetActivity(activity) => state.activity = activity,
            AppAction::SetHarnessAvailability(availability) => {
                state.harness = availability;
                state.harness_reason_code = None;
                state.harness_probing = false;
            }
            AppAction::SetHarnessDiagnostic { availability, reason_code, probing } => {
                state.harness = availability;
                state.harness_reason_code = reason_code;
                state.harness_probing = probing;
            }
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

    #[test]
    fn explicit_chat_stays_visible_when_the_desktop_loses_foreground() {
        let core = AppCore::default();
        core.dispatch(AppAction::BootReady { play_wake: false });
        core.dispatch(AppAction::OpenChat);
        let snapshot = core.dispatch(AppAction::DesktopForegroundChanged(false));
        assert_eq!(snapshot.phase, SystemPhase::Chatting);
        assert!(snapshot.interaction.visible);
        assert_eq!(snapshot.phase, SystemPhase::Chatting);
    }

    #[test]
    fn settings_state_does_not_hide_an_explicitly_visible_overlay() {
        let core = AppCore::default();
        core.dispatch(AppAction::BootReady { play_wake: false });
        core.dispatch(AppAction::OpenSettings);
        let snapshot = core.dispatch(AppAction::DesktopForegroundChanged(false));
        assert!(snapshot.interaction.settings_open);
        assert!(snapshot.interaction.visible);
    }

    /// 中央玻璃悬浮下岛是常驻的，悬浮球据此永不弹出。
    ///
    /// 两件必须成立的事：这个标记要真的进快照（原生读的就是它），而且**不能**被相位切换清掉——
    /// 锁屏/苏醒时岛会短暂不渲染，若那时标记被清，球就会在最不该出现的时刻弹出来。
    #[test]
    fn the_pinned_island_flag_survives_phase_changes() {
        let core = AppCore::default();
        assert!(!core.snapshot().island_pinned);
        core.dispatch(AppAction::SetIslandPinned(true));
        assert!(core.snapshot().island_pinned);
        core.dispatch(AppAction::Lock);
        assert_eq!(core.snapshot().phase, SystemPhase::Locked);
        assert!(core.snapshot().island_pinned);
        core.dispatch(AppAction::Unlock { play_wake: true });
        assert!(core.snapshot().island_pinned);
        core.dispatch(AppAction::SetIslandPinned(false));
        assert!(!core.snapshot().island_pinned);
    }
}

fn can_show_interaction(state: &AppSnapshot) -> bool {
    state.interaction.enabled && matches!(state.phase, SystemPhase::Idle | SystemPhase::Chatting)
}

fn recompute_interaction_visibility(state: &mut AppSnapshot) {
    state.interaction.visible = can_show_interaction(state);
}

fn close_transient_surfaces(state: &mut AppSnapshot) {
    state.interaction.history_expanded = false;
    state.interaction.settings_open = false;
}
