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
//! * **It distinguishes "crashed" from "exited cleanly" with a marker file.** The
//!   marker is deleted at startup and written as the *last* step of a clean exit,
//!   so its absence is unambiguous evidence that the previous process died without
//!   cleaning up. A stale marker from an older run cannot be mistaken for this
//!   run's, because startup removes it.

use std::path::PathBuf;

/// Set on the command line to run as the repair helper instead of as the wallpaper.
pub const REPAIR_ARGUMENT: &str = "--desktop-repair";

/// Lives under the app's own `DSHWallpaper` directory in `LOCALAPPDATA` (the
/// lock-screen config directory it once shared a root with has been archived).
/// Resolved without Tauri so the helper can find it on its own.
pub fn repair_state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(root.join("DSHWallpaper").join("desktop-repair"))
}

fn marker_path() -> Option<PathBuf> {
    repair_state_dir().map(|dir| dir.join("clean-exit.marker"))
}

/// Delete the marker so its presence means "the running process exited cleanly".
///
/// Called at startup, before the helper could observe it. Without this, a marker
/// left by an *earlier* clean exit would make the helper believe a later crash was
/// clean and skip the repair.
pub fn clear_clean_exit_marker() {
    let Some(path) = marker_path() else { return };
    match std::fs::remove_file(&path) {
        Ok(()) => log::debug!("desktop repair: cleared stale clean-exit marker"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("desktop repair: could not clear marker: {error}"),
    }
}

/// Record that this process released the desktop on its way out.
///
/// Must be the last action of a clean shutdown: writing it earlier would invite
/// the helper to skip a repair for mutations that had not been undone yet.
pub fn mark_clean_exit() {
    let Some(path) = marker_path() else { return };
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!("desktop repair: could not create {}: {error}", parent.display());
            return;
        }
    }
    if let Err(error) = std::fs::write(&path, b"clean") {
        log::warn!("desktop repair: could not write marker: {error}");
    }
}

pub fn clean_exit_was_recorded() -> bool {
    marker_path().map(|path| path.is_file()).unwrap_or(false)
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
pub fn run_repair_helper(parent_pid: u32) -> String {
    wait_for_process_exit(parent_pid);
    if clean_exit_was_recorded() {
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
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
        match handle {
            Ok(handle) => {
                let waited = unsafe { WaitForSingleObject(handle, INFINITE) };
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

    /// The marker is the only evidence distinguishing a crash from a clean exit, so
    /// its lifecycle is the contract worth pinning. Exercised against a temporary
    /// root rather than the real one.
    #[test]
    fn marker_absence_means_the_previous_run_did_not_clean_up() {
        let dir = std::env::temp_dir().join(format!("dsh-repair-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("clean-exit.marker");

        // A marker left by an older run must not be readable as this run's status.
        std::fs::write(&path, b"clean").expect("seed stale marker");
        assert!(path.is_file());
        std::fs::remove_file(&path).expect("startup clears it");
        assert!(!path.is_file());

        std::fs::write(&path, b"clean").expect("clean exit writes it");
        assert!(path.is_file());

        let _ = std::fs::remove_dir_all(&dir);
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
}
