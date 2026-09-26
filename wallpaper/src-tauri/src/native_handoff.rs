//! Small, platform-independent lifecycle state for the native/WebView hand-off.
//!
//! Window handles and watchdog timers live in `native_bootstrap`; this type is
//! the authority for which renderer generation is allowed to hide the cover.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandoffPhase {
    Dormant,
    AwaitingScene,
    Released,
    Locked,
    Destroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeHandoffState {
    generation: u64,
    phase: HandoffPhase,
}

impl Default for NativeHandoffState {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: HandoffPhase::Dormant,
        }
    }
}

impl NativeHandoffState {
    fn advance(&mut self, phase: HandoffPhase) -> u64 {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.phase = phase;
        self.generation
    }

    pub(crate) fn begin(&mut self) -> u64 {
        self.advance(HandoffPhase::AwaitingScene)
    }

    pub(crate) fn lock(&mut self) -> u64 {
        self.advance(HandoffPhase::Locked)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn is_pending(&self, generation: u64) -> bool {
        generation == self.generation && self.phase == HandoffPhase::AwaitingScene
    }

    pub(crate) fn is_released(&self, generation: u64) -> bool {
        generation == self.generation && self.phase == HandoffPhase::Released
    }

    pub(crate) fn is_locked(&self, generation: u64) -> bool {
        generation == self.generation && self.phase == HandoffPhase::Locked
    }

    pub(crate) fn invalidate_pending(&mut self) -> Option<u64> {
        (self.phase == HandoffPhase::AwaitingScene).then(|| self.begin())
    }

    pub(crate) fn release(&mut self, generation: u64) -> bool {
        if !self.is_pending(generation) {
            return false;
        }
        self.phase = HandoffPhase::Released;
        true
    }

    pub(crate) fn restore_pending(&mut self, generation: u64) -> bool {
        if !self.is_released(generation) {
            return false;
        }
        self.phase = HandoffPhase::AwaitingScene;
        true
    }

    pub(crate) fn expire(&mut self, generation: u64) -> bool {
        if !self.is_pending(generation) {
            return false;
        }
        self.phase = HandoffPhase::Destroyed;
        true
    }

    pub(crate) fn destroy(&mut self) {
        self.phase = HandoffPhase::Destroyed;
    }
}

#[cfg(test)]
mod tests {
    use super::{HandoffPhase, NativeHandoffState};

    #[test]
    fn an_old_frame_cannot_release_after_lock_and_unlock() {
        let mut state = NativeHandoffState::default();
        let startup = state.begin();
        assert!(state.release(startup));

        state.lock();
        let unlock = state.begin();

        assert!(unlock > startup);
        assert!(!state.release(startup));
        assert_eq!(state.phase, HandoffPhase::AwaitingScene);
        assert!(state.release(unlock));
    }

    #[test]
    fn a_pending_host_change_invalidates_only_the_old_generation() {
        let mut state = NativeHandoffState::default();
        let first = state.begin();
        let second = state.invalidate_pending().expect("pending generation");

        assert!(second > first);
        assert!(!state.release(first));
        assert!(state.release(second));
        assert_eq!(state.invalidate_pending(), None);
    }

    #[test]
    fn duplicate_release_and_stale_watchdog_expiry_are_noops() {
        let mut state = NativeHandoffState::default();
        let first = state.begin();
        assert!(state.release(first));
        assert!(!state.release(first));
        assert!(!state.expire(first));

        let second = state.begin();
        assert!(!state.expire(first));
        assert_eq!(state.phase, HandoffPhase::AwaitingScene);
        assert!(state.expire(second));
        assert_eq!(state.phase, HandoffPhase::Destroyed);
    }

    #[test]
    fn generation_wrap_never_uses_the_zero_sentinel() {
        let mut state = NativeHandoffState {
            generation: u64::MAX,
            phase: HandoffPhase::Locked,
        };
        assert_eq!(state.begin(), 1);
    }
}
