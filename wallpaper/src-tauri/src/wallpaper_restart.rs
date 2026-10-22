/// A controlled Tauri restart keeps the managed children alive; other exits stop them.
#[cfg(any(not(feature = "lite"), test))]
pub(crate) fn should_shutdown_managed_dsh(exit_code: Option<i32>) -> bool {
    exit_code != Some(tauri::RESTART_EXIT_CODE)
}

pub(crate) const REQUIRED_STABLE_HOST_SAMPLES: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestartHostKind {
    WorkerW,
    Progman,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RestartHostCandidate {
    pub(crate) handle: usize,
    pub(crate) kind: RestartHostKind,
}

#[cfg(any(windows, test))]
pub(crate) fn stable_restart_host(
    worker: Option<usize>,
    progman: Option<usize>,
) -> Option<RestartHostCandidate> {
    worker
        .filter(|handle| *handle != 0)
        .map(|handle| RestartHostCandidate {
            handle,
            kind: RestartHostKind::WorkerW,
        })
        .or_else(|| {
            progman
                .filter(|handle| *handle != 0)
                .map(|handle| RestartHostCandidate {
                    handle,
                    kind: RestartHostKind::Progman,
                })
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestartGateDecision {
    Idle,
    Waiting { stable_samples: u8 },
    RequestRestart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WallpaperHostRecoveryError {
    RegistrationConflict(String),
    Other(String),
}

impl WallpaperHostRecoveryError {
    pub(crate) fn from_tauri(error: tauri::Error) -> Self {
        let message = format!("无法重建背景 WebView：{error}");
        match error {
            tauri::Error::WindowLabelAlreadyExists(_)
            | tauri::Error::WebviewLabelAlreadyExists(_) => Self::RegistrationConflict(message),
            _ => Self::Other(message),
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::RegistrationConflict(message) | Self::Other(message) => message,
        }
    }
}

impl std::fmt::Display for WallpaperHostRecoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl From<String> for WallpaperHostRecoveryError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

#[derive(Debug, Default)]
pub(crate) struct RestartGate {
    pending: bool,
    last_host: Option<RestartHostCandidate>,
    stable_samples: u8,
    restart_requested: bool,
}

impl RestartGate {
    pub(crate) const fn new() -> Self {
        Self {
            pending: false,
            last_host: None,
            stable_samples: 0,
            restart_requested: false,
        }
    }

    pub(crate) fn registration_conflict(&mut self) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.last_host = None;
        self.stable_samples = 0;
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending
    }

    pub(crate) fn observe_host(
        &mut self,
        host: Option<RestartHostCandidate>,
    ) -> RestartGateDecision {
        if !self.pending || self.restart_requested {
            return RestartGateDecision::Idle;
        }

        match host.filter(|candidate| candidate.handle != 0) {
            Some(candidate) if self.last_host == Some(candidate) => {
                self.stable_samples = self.stable_samples.saturating_add(1);
            }
            Some(candidate) => {
                self.last_host = Some(candidate);
                self.stable_samples = 1;
            }
            None => {
                self.last_host = None;
                self.stable_samples = 0;
            }
        }

        if self.stable_samples >= REQUIRED_STABLE_HOST_SAMPLES {
            self.restart_requested = true;
            RestartGateDecision::RequestRestart
        } else {
            RestartGateDecision::Waiting {
                stable_samples: self.stable_samples,
            }
        }
    }

    #[cfg(test)]
    fn observe_worker(&mut self, worker: Option<usize>) -> RestartGateDecision {
        self.observe_host(worker.map(|handle| RestartHostCandidate {
            handle,
            kind: RestartHostKind::WorkerW,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        should_shutdown_managed_dsh, stable_restart_host, RestartGate, RestartGateDecision,
        RestartHostCandidate, RestartHostKind, WallpaperHostRecoveryError,
    };

    #[test]
    fn controlled_restart_preserves_managed_dsh() {
        assert!(!should_shutdown_managed_dsh(Some(tauri::RESTART_EXIT_CODE)));
    }

    #[test]
    fn ordinary_exits_still_stop_managed_dsh() {
        assert!(should_shutdown_managed_dsh(None));
        assert!(should_shutdown_managed_dsh(Some(0)));
    }

    #[test]
    fn restart_waits_for_three_stable_worker_samples() {
        let mut gate = RestartGate::new();
        gate.registration_conflict();

        assert_eq!(
            gate.observe_worker(Some(0x1234)),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_worker(Some(0x1234)),
            RestartGateDecision::Waiting { stable_samples: 2 }
        );
        assert_eq!(
            gate.observe_worker(Some(0x1234)),
            RestartGateDecision::RequestRestart
        );
    }

    #[test]
    fn restart_gate_uses_progman_when_no_visible_workerw_exists() {
        let host = stable_restart_host(None, Some(0x9876));
        assert_eq!(
            host,
            Some(RestartHostCandidate {
                handle: 0x9876,
                kind: RestartHostKind::Progman,
            })
        );

        let mut gate = RestartGate::new();
        gate.registration_conflict();
        assert_eq!(
            gate.observe_host(host),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_host(host),
            RestartGateDecision::Waiting { stable_samples: 2 }
        );
        assert_eq!(
            gate.observe_host(host),
            RestartGateDecision::RequestRestart
        );
    }

    #[test]
    fn restart_gate_resets_when_host_kind_changes_even_if_handle_is_reused() {
        let worker = RestartHostCandidate {
            handle: 0x1234,
            kind: RestartHostKind::WorkerW,
        };
        let progman = RestartHostCandidate {
            handle: 0x1234,
            kind: RestartHostKind::Progman,
        };
        let mut gate = RestartGate::new();
        gate.registration_conflict();

        assert_eq!(
            gate.observe_host(Some(worker)),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_host(Some(worker)),
            RestartGateDecision::Waiting { stable_samples: 2 }
        );
        assert_eq!(
            gate.observe_host(Some(progman)),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_host(Some(progman)),
            RestartGateDecision::Waiting { stable_samples: 2 }
        );
        assert_eq!(
            gate.observe_host(Some(progman)),
            RestartGateDecision::RequestRestart
        );
    }

    #[test]
    fn restart_host_prefers_workerw_and_falls_back_from_null_handles() {
        assert_eq!(
            stable_restart_host(Some(0x1234), Some(0x5678)),
            Some(RestartHostCandidate {
                handle: 0x1234,
                kind: RestartHostKind::WorkerW,
            })
        );
        assert_eq!(
            stable_restart_host(Some(0), Some(0x5678)),
            Some(RestartHostCandidate {
                handle: 0x5678,
                kind: RestartHostKind::Progman,
            })
        );
        assert_eq!(stable_restart_host(None, Some(0)), None);
    }

    #[test]
    fn missing_or_changed_worker_resets_the_stability_window() {
        let mut gate = RestartGate::new();
        gate.registration_conflict();

        assert_eq!(
            gate.observe_worker(Some(0x1234)),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_worker(None),
            RestartGateDecision::Waiting { stable_samples: 0 }
        );
        assert_eq!(
            gate.observe_worker(Some(0x1234)),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_worker(Some(0x5678)),
            RestartGateDecision::Waiting { stable_samples: 1 }
        );
        assert_eq!(
            gate.observe_worker(Some(0x5678)),
            RestartGateDecision::Waiting { stable_samples: 2 }
        );
        assert_eq!(
            gate.observe_worker(Some(0x5678)),
            RestartGateDecision::RequestRestart
        );
    }

    #[test]
    fn a_pending_restart_is_requested_only_once() {
        let mut gate = RestartGate::new();
        gate.registration_conflict();
        gate.observe_worker(Some(0x1234));
        gate.observe_worker(Some(0x1234));
        assert_eq!(
            gate.observe_worker(Some(0x1234)),
            RestartGateDecision::RequestRestart
        );

        assert_eq!(gate.observe_worker(Some(0x1234)), RestartGateDecision::Idle);
        assert!(gate.is_pending());
    }

    #[test]
    fn only_tauri_label_collisions_are_registration_conflicts() {
        assert!(matches!(
            WallpaperHostRecoveryError::from_tauri(tauri::Error::WindowLabelAlreadyExists(
                "background".into()
            )),
            WallpaperHostRecoveryError::RegistrationConflict(_)
        ));
        assert!(matches!(
            WallpaperHostRecoveryError::from_tauri(tauri::Error::WebviewLabelAlreadyExists(
                "background".into()
            )),
            WallpaperHostRecoveryError::RegistrationConflict(_)
        ));
        assert!(matches!(
            WallpaperHostRecoveryError::from_tauri(tauri::Error::CannotReparentWebviewWindow),
            WallpaperHostRecoveryError::Other(_)
        ));
    }
}
