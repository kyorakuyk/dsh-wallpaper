//! What the last scan found, kept on disk with the time it was confirmed.
//!
//! A scan is expensive and manual (`docs/design/harness-subject-and-ui-design.md`
//! §4.1: scanning and hand-adding are the two entry points, and neither happens on
//! its own), so the settings surface has to be able to show the subjects it already
//! knows about without walking the disk again every time it opens.
//!
//! The one thing a cached list must never do is *look* current. A client can be
//! uninstalled or a source tree moved between two scans, and a list that hides that
//! would send the launcher after something that is no longer there. So the record
//! carries `verified_at_ms` and the UI says when it was confirmed — the same reason
//! the launcher re-resolves a subject from its id instead of trusting a stored path.
//!
//! The file is a cache, never an authority: a missing, unreadable, truncated or
//! future-shaped file is reported as "nothing known yet" and never as an error the
//! user has to fix.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::harness_targets::{HarnessTarget, HarnessTargetScan};

/// Bumped when a stored record stops being readable by an older build.
const CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessTargetCatalog {
    pub schema_version: u32,
    /// When a real scan last confirmed this list, in milliseconds since the epoch.
    pub verified_at_ms: u64,
    pub targets: Vec<HarnessTarget>,
    pub requires_subject_choice: bool,
}

/// Where the catalogue lives, beside the managed DSH log this application writes.
fn catalog_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| {
        root.join("com.dsh.wallpaper")
            .join("harness-targets.json")
    })
}

/// Build the record for a scan that just ran.
fn record(scan: &HarnessTargetScan, verified_at_ms: u64) -> HarnessTargetCatalog {
    HarnessTargetCatalog {
        schema_version: CATALOG_SCHEMA_VERSION,
        verified_at_ms,
        targets: scan.targets.clone(),
        requires_subject_choice: scan.requires_subject_choice,
    }
}

/// Read a catalogue file. Anything unreadable is "nothing known yet".
fn load_from(path: &Path) -> Option<HarnessTargetCatalog> {
    let text = std::fs::read_to_string(path).ok()?;
    let catalog: HarnessTargetCatalog = serde_json::from_str(&text).ok()?;
    if catalog.schema_version != CATALOG_SCHEMA_VERSION {
        // A newer build wrote it, or an older one did: either way this build must
        // not present a shape it does not understand as a verified list.
        return None;
    }
    Some(catalog)
}

/// Write a catalogue file, creating its directory if needed.
fn save_to(path: &Path, catalog: &HarnessTargetCatalog) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(catalog).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// The subjects the last scan confirmed, or `None` when nothing is known.
pub(crate) fn load_catalog() -> Option<HarnessTargetCatalog> {
    catalog_path().and_then(|path| load_from(&path))
}

/// Record a completed scan. Best effort: a cache that cannot be written must not
/// turn a successful scan into a failure the user sees.
pub(crate) fn persist_scan(scan: &HarnessTargetScan) {
    let Some(path) = catalog_path() else {
        return;
    };
    let verified_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    if let Err(error) = save_to(&path, &record(scan, verified_at_ms)) {
        log::warn!("harness target catalogue not written: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_targets::{HarnessTargetKind, ScannedShortcut};

    fn scan_with_one_shell() -> HarnessTargetScan {
        // Reuses the model's own matching rules so the record always holds targets
        // that the rest of the code would have produced, not hand-made ones.
        crate::harness_targets::build_scan_for_tests(&[ScannedShortcut {
            aumid: "com.deepseek.dsh".into(),
            directory: r"C:\Start Menu".into(),
        }])
    }

    #[test]
    fn a_recorded_scan_survives_a_round_trip_with_its_verification_time() {
        let scan = scan_with_one_shell();
        let catalog = record(&scan, 1_700_000_000_000);
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("nested").join("catalog.json");
        save_to(&path, &catalog).expect("saved");

        let loaded = load_from(&path).expect("loaded");
        assert_eq!(loaded, catalog);
        assert_eq!(loaded.verified_at_ms, 1_700_000_000_000);
        assert_eq!(loaded.targets.len(), 1);
        assert_eq!(loaded.targets[0].kind, HarnessTargetKind::EmbeddedShell);
    }

    #[test]
    fn a_missing_or_broken_file_is_nothing_known_rather_than_an_error() {
        let directory = tempfile::tempdir().expect("temp dir");
        let missing = directory.path().join("absent.json");
        assert!(load_from(&missing).is_none());

        let broken = directory.path().join("broken.json");
        std::fs::write(&broken, "{ not json").expect("written");
        assert!(load_from(&broken).is_none());

        // A shape from another schema version must not be offered as verified.
        let wrong_version = directory.path().join("version.json");
        std::fs::write(
            &wrong_version,
            r#"{"schemaVersion":99,"verifiedAtMs":1,"targets":[],"requiresSubjectChoice":false}"#,
        )
        .expect("written");
        assert!(load_from(&wrong_version).is_none());
    }
}
