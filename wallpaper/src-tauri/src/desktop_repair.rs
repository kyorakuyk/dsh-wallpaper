//! Desktop repair after an abnormal exit.
//!
//! A resident wallpaper makes two changes that outlive its process:
//!
//! * it hides Explorer's desktop icon layer while the inner workspace is active, and
//! * it parents its window into the desktop host and takes over that region's hit
//!   testing.
//!
//! `RunEvent::Exit` runs `shutdown_native_state` and undoes both, and
//! `start_wallpaper_host` also restores the icons on the next launch — so a user who
//! restarts the wallpaper recovers. Neither helps at the moment that matters: a
//! crash or a forced kill leaves the desktop iconless and unresponsive until the
//! user works out that restarting the wallpaper is the cure.
//!
//! This module closes that window with a *separate, tiny* process, spawned at
//! startup, that outlives the wallpaper and repairs the desktop the moment the
//! wallpaper dies badly.
//!
//! Two properties are deliberate and load-bearing:
//!
//! * **It never restarts anything.** The helper performs one idempotent repair and
//!   exits. There is no retry loop, so an auto-restart cascade — a crash loop that
//!   grows resource use and takes other applications down with it — cannot arise
//!   here by construction.
//! * **It distinguishes "crashed" from "exited cleanly" with a per-run marker.**
//!   The marker is written as the *last* step of a clean exit. Each helper carries
//!   its parent's PID and generation, so a replacement wallpaper cannot clear the
//!   marker an older helper is waiting to read.

use std::path::{Path, PathBuf};

/// Set on the command line to run as the repair helper instead of as the wallpaper.
pub const REPAIR_ARGUMENT: &str = "--desktop-repair";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RepairRunId {
    pid: u32,
    generation: u128,
}

impl RepairRunId {
    fn current() -> Self {
        static CURRENT: std::sync::OnceLock<RepairRunId> = std::sync::OnceLock::new();
        *CURRENT.get_or_init(|| {
            let generation = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(1)
                .max(1);
            Self {
                pid: std::process::id(),
                generation,
            }
        })
    }

    fn to_argument(self) -> String {
        format!("{}-{}", self.pid, self.generation)
    }

    fn from_argument(argument: &str) -> Option<Self> {
        let (pid, generation) = argument.split_once('-')?;
        let run_id = Self {
            pid: pid.parse().ok()?,
            generation: generation.parse().ok()?,
        };
        (run_id.pid != 0 && run_id.generation != 0).then_some(run_id)
    }
}

fn write_marker_for_run(root: &Path, run_id: RepairRunId) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    std::fs::write(marker_path_for_run(root, run_id), b"clean")
}

fn repair_run_id_for_parent(parent_pid: u32, argument: Option<&str>) -> Option<RepairRunId> {
    let run_id = RepairRunId::from_argument(argument?)?;
    (run_id.pid == parent_pid).then_some(run_id)
}

fn marker_path_for_run(root: &Path, run_id: RepairRunId) -> PathBuf {
    root.join(format!(
        "clean-exit-{}-{}.marker",
        run_id.pid, run_id.generation
    ))
}

fn clear_marker_for_run(root: &Path, run_id: RepairRunId) -> std::io::Result<()> {
    match std::fs::remove_file(marker_path_for_run(root, run_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Lives under the app's own `DSHWallpaper` directory in `LOCALAPPDATA` (the
/// lock-screen config directory it once shared a root with has been archived).
/// Resolved without Tauri so the helper can find it on its own.
pub fn repair_state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(root.join("DSHWallpaper").join("desktop-repair"))
}

/// Delete the marker so its presence means "the running process exited cleanly".
///
/// Called at startup, before the helper could observe it. Only this process's
/// PID/generation marker is touched; markers belonging to already-running helpers
/// must remain readable across a wallpaper restart.
pub fn clear_clean_exit_marker() {
    let Some(dir) = repair_state_dir() else {
        return;
    };
    match clear_marker_for_run(&dir, RepairRunId::current()) {
        Ok(()) => log::debug!("desktop repair: cleared this run's stale clean-exit marker"),
        Err(error) => log::warn!("desktop repair: could not clear this run's marker: {error}"),
    }
}

/// Record that this process released the desktop on its way out.
///
/// Must be the last action of a clean shutdown: writing it earlier would invite
/// the helper to skip a repair for mutations that had not been undone yet.
pub fn mark_clean_exit() {
    let Some(dir) = repair_state_dir() else {
        return;
    };
    if let Err(error) = write_marker_for_run(&dir, RepairRunId::current()) {
        log::warn!("desktop repair: could not write marker: {error}");
    }
}

/// Record what the helper did, for support and for verifying this path really runs.
///
/// A file rather than the log stream on purpose: the helper is detached and
/// short-lived, so on a release build its stderr has nowhere to go. Writing it
/// also makes "the helper ran and repaired" a checkable fact after the fact.
pub fn record_repair_outcome(outcome: &str) {
    let Some(dir) = repair_state_dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let _ = std::fs::write(dir.join("last-repair.log"), format!("{stamp} {outcome}\n"));
}

/// Whether this process was started as the repair helper.
pub fn is_repair_invocation() -> bool {
    std::env::args().any(|argument| argument == REPAIR_ARGUMENT)
}

/// Run the helper's whole job and return a line for the log.
///
/// `parent_pid` is the wallpaper that spawned this helper. Waiting on it is what
/// makes the repair *timely*: the helper is dormant until the wallpaper is gone.
pub fn run_repair_helper(parent_pid: u32, run_id_argument: Option<&str>) -> String {
    wait_for_process_exit(parent_pid);
    let clean_exit_was_recorded = repair_run_id_for_parent(parent_pid, run_id_argument)
        .and_then(|run_id| {
            let dir = repair_state_dir()?;
            let marker_path = marker_path_for_run(&dir, run_id);
            let was_recorded = marker_path.is_file();
            if let Err(error) = clear_marker_for_run(&dir, run_id) {
                log::warn!("desktop repair: could not consume this run's marker: {error}");
            }
            Some(was_recorded)
        })
        .unwrap_or(false);
    if clean_exit_was_recorded {
        // The wallpaper undid its own changes and said so. Repairing now would be
        // harmless but would also hide a real regression in the exit path, so it
        // is reported instead.
        return format!("parent={parent_pid} outcome=clean-exit-no-repair");
    }
    let notes = crate::windows_integration::repair_desktop_after_abnormal_exit();
    format!("parent={parent_pid} outcome=repaired {}", notes.join(","))
}

/// Block until `pid` is gone.
///
/// Waits on the process *handle* rather than polling `OpenProcess` in a loop: a
/// handle becomes signalled exactly once, so this costs no CPU while the wallpaper
/// is healthy and cannot miss a short-lived process.
pub fn wait_for_process_exit(pid: u32) {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
    };
    // Bounded so a helper can never wait forever if the handle cannot be opened
    // for an unrelated reason.
    const OPEN_RETRY_BUDGET: u32 = 30;
    const RETRY_DELAY_MS: u64 = 1_000;

    let mut attempts = 0;
    loop {
        // SAFETY: OpenProcess returns an owned process HANDLE on success; the error branch receives no handle to use or close.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
        match handle {
            Ok(handle) => {
                // SAFETY: handle is the live PROCESS_SYNCHRONIZE handle returned by OpenProcess and remains open until the wait completes.
                let waited = unsafe { WaitForSingleObject(handle, INFINITE) };
                // SAFETY: handle was acquired by OpenProcess and is closed exactly once after WaitForSingleObject returns.
                unsafe {
                    let _ = CloseHandle(handle);
                }
                if waited == WAIT_OBJECT_0 {
                    return;
                }
                // A failed wait is not worth retrying indefinitely.
                return;
            }
            Err(_) => {
                // The parent is already gone, which is the common case when the
                // helper is spawned and the wallpaper is killed immediately.
                attempts += 1;
                if attempts > OPEN_RETRY_BUDGET {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS));
            }
        }
    }
}

/// Spawn the repair helper, detached from this process.
///
/// Failure is reported and ignored: the helper is a safety net, and the existing
/// next-launch restore still covers a crash when the helper could not start.
pub fn spawn_repair_helper() {
    use std::process::Command;
    let Ok(executable) = std::env::current_exe() else {
        log::warn!("desktop repair: cannot resolve own executable; helper not started");
        return;
    };
    let child = Command::new(executable)
        .arg(REPAIR_ARGUMENT)
        .arg(std::process::id().to_string())
        .arg(RepairRunId::current().to_argument())
        .spawn();
    match child {
        // Intentionally not waited on: the helper must survive this process.
        Ok(child) => log::info!("desktop repair helper started: pid={}", child.id()),
        Err(error) => log::warn!("desktop repair helper failed to start: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marker is the only evidence distinguishing a crash from a clean exit;
    /// the lifecycle must stay scoped to one process generation.
    #[test]
    fn marker_lifecycle_is_scoped_to_one_process_generation() {
        let root = tempfile::tempdir().expect("temporary marker root");
        let previous_run = RepairRunId {
            pid: 1_010,
            generation: 10,
        };
        let current_run = RepairRunId {
            pid: 2_020,
            generation: 20,
        };
        let previous_marker = marker_path_for_run(root.path(), previous_run);
        let current_marker = marker_path_for_run(root.path(), current_run);

        write_marker_for_run(root.path(), previous_run).expect("previous clean exit marker");
        clear_marker_for_run(root.path(), current_run).expect("clear current run marker");
        assert!(
            previous_marker.is_file(),
            "current run cleared old helper marker"
        );
        assert!(!current_marker.exists());

        write_marker_for_run(root.path(), current_run).expect("current clean exit marker");
        assert!(current_marker.is_file());
        clear_marker_for_run(root.path(), current_run).expect("consume current run marker");
        assert!(!current_marker.exists());
        assert!(
            previous_marker.is_file(),
            "consuming current marker cleared old helper marker"
        );
    }

    /// The helper must recognise its own invocation and never mistake the normal
    /// launch for one, or every wallpaper start would repair a healthy desktop.
    #[test]
    fn the_repair_argument_is_specific() {
        assert_eq!(REPAIR_ARGUMENT, "--desktop-repair");
        assert!(!REPAIR_ARGUMENT.is_empty());
    }

    /// The helper must be able to locate its state directory without Tauri, since
    /// it runs while the wallpaper is gone.
    #[test]
    fn the_state_directory_does_not_depend_on_tauri() {
        if let Some(dir) = repair_state_dir() {
            assert!(dir.ends_with("desktop-repair"), "unexpected dir: {}", dir.display());
        }
    }

    #[test]
    fn clearing_a_new_run_marker_preserves_the_old_helper_marker() {
        let root = tempfile::tempdir().expect("temporary marker root");
        let old_run = RepairRunId {
            pid: 101,
            generation: 1,
        };
        let new_run = RepairRunId {
            pid: 202,
            generation: 2,
        };
        let old_marker = marker_path_for_run(root.path(), old_run);
        std::fs::write(&old_marker, b"clean").expect("old helper marker");

        clear_marker_for_run(root.path(), new_run).expect("clear only the new run marker");

        assert!(
            old_marker.is_file(),
            "new startup removed the old helper marker"
        );
    }

    #[test]
    fn concurrent_helpers_only_clear_their_own_run_marker() {
        use std::sync::{Arc, Barrier};

        let root = tempfile::tempdir().expect("temporary marker root");
        let root_path = root.path().to_path_buf();
        let old_run = RepairRunId {
            pid: 303,
            generation: 3,
        };
        let new_run = RepairRunId {
            pid: 404,
            generation: 4,
        };
        let barrier = Arc::new(Barrier::new(2));

        let old_root = root_path.clone();
        let old_barrier = Arc::clone(&barrier);
        let old_helper = std::thread::spawn(move || {
            std::fs::write(marker_path_for_run(&old_root, old_run), b"clean")
                .expect("old helper marker");
            old_barrier.wait();
            old_barrier.wait();
        });

        let new_barrier = Arc::clone(&barrier);
        let new_root = root_path.clone();
        let new_helper = std::thread::spawn(move || {
            std::fs::write(marker_path_for_run(&new_root, new_run), b"clean")
                .expect("new helper marker");
            new_barrier.wait();
            clear_marker_for_run(&new_root, new_run).expect("clear new helper marker");
            new_barrier.wait();
        });

        old_helper.join().expect("old helper thread");
        new_helper.join().expect("new helper thread");

        assert!(
            marker_path_for_run(root.path(), old_run).is_file(),
            "new helper removed the old run marker"
        );
    }

    #[test]
    fn repair_run_argument_round_trips_and_rejects_invalid_values() {
        let run_id = RepairRunId {
            pid: 505,
            generation: 5_000_000_000,
        };

        assert_eq!(run_id.to_argument(), "505-5000000000");
        assert_eq!(
            RepairRunId::from_argument(&run_id.to_argument()),
            Some(run_id)
        );
        assert_eq!(RepairRunId::from_argument("../../clean-exit.marker"), None);
        assert_eq!(RepairRunId::from_argument("505-0"), None);
    }

    #[test]
    fn helper_accepts_only_the_run_id_for_its_parent_pid() {
        let run_id = RepairRunId {
            pid: 606,
            generation: 6_000_000_000,
        };
        let argument = run_id.to_argument();

        assert_eq!(
            repair_run_id_for_parent(run_id.pid, Some(&argument)),
            Some(run_id)
        );
        assert_eq!(repair_run_id_for_parent(707, Some(&argument)), None);
        assert_eq!(repair_run_id_for_parent(run_id.pid, None), None);
    }

    #[test]
    fn writing_a_run_marker_creates_only_that_generation() {
        let root = tempfile::tempdir().expect("temporary marker root");
        let run_id = RepairRunId {
            pid: 808,
            generation: 8_000_000_000,
        };
        let other_run = RepairRunId {
            pid: 909,
            generation: 9_000_000_000,
        };

        write_marker_for_run(root.path(), run_id).expect("write clean-exit marker");

        assert_eq!(
            std::fs::read(marker_path_for_run(root.path(), run_id)).expect("read marker"),
            b"clean"
        );
        assert!(!marker_path_for_run(root.path(), other_run).exists());
    }

    #[test]
    fn current_run_id_is_stable_and_bound_to_this_process() {
        let first = RepairRunId::current();
        let second = RepairRunId::current();

        assert_eq!(first, second);
        assert_eq!(first.pid, std::process::id());
        assert!(first.generation > 0);
    }
}
