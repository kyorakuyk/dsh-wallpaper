//! Durable, fail-closed persistence for Windows lock-screen ownership.
//!
//! This module deliberately contains no WinRT or Tauri calls.  It owns the
//! filesystem contract around a lock-screen takeover so the native adapter can
//! be kept small and the failure cases can be tested without changing a real
//! user's lock screen.
//!
//! A text URI is not a sufficient backup: users can delete or move the source
//! image after enabling the wallpaper.  We therefore retain a private copy of
//! the original static image and a versioned manifest.  Dynamic sources such as
//! Spotlight have no file that we can faithfully restore and must be rejected
//! before a takeover is attempted.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

pub const LOCK_SCREEN_BACKUP_SCHEMA_VERSION: u32 = 1;
pub const LOCK_SCREEN_BACKUP_MANIFEST: &str = "lock-screen-backup.json";
pub const LOCK_SCREEN_LEGACY_BACKUP: &str = "lock-screen-backup.txt";
pub const LOCK_SCREEN_ASSET_DIRECTORY: &str = "lock-screen";

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The persisted state required to restore a static lock screen safely.
///
/// `snapshot_file` is always a filename relative to
/// `<config>/lock-screen/`; it is never an arbitrary path supplied by a file
/// on disk.  This makes a corrupted manifest unable to make cleanup or
/// restore access another user file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScreenBackupManifest {
    pub version: u32,
    pub original_image_uri: String,
    pub snapshot_file: String,
    pub captured_at_unix_ms: u64,
}

/// Result of inspecting on-disk lock-screen ownership state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockScreenBackupState {
    Missing,
    Valid(LockScreenBackupManifest),
    /// A v0 backup only saved a URI.  It may be migrated *before* a new
    /// takeover if that source file still exists, but is not a reliable
    /// restoration guarantee on its own.
    LegacyUri {
        original_image_uri: String,
    },
    Invalid {
        reason: String,
    },
}

/// A backup acquired for one lock-screen takeover attempt.
///
/// The ownership bit matters when the native caller has to roll an attempt
/// back: a failed setter may discard only a snapshot created by *this*
/// attempt, never a pre-existing restore point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockScreenBackupLease {
    pub manifest: LockScreenBackupManifest,
    created_for_this_attempt: bool,
}

pub fn manifest_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LOCK_SCREEN_BACKUP_MANIFEST)
}

pub fn legacy_manifest_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LOCK_SCREEN_LEGACY_BACKUP)
}

pub fn asset_directory(config_dir: &Path) -> PathBuf {
    config_dir.join(LOCK_SCREEN_ASSET_DIRECTORY)
}

/// Converts the local `file:` URIs returned by `LockScreen::OriginalImageFile`
/// into a Windows path.  HTTP, `ms-appx`, and other dynamic/non-local sources
/// are intentionally rejected.
pub fn local_file_uri_to_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("锁屏原图路径为空".into());
    }

    if !value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        return Err("当前锁屏不是可备份的本地静态图片".into());
    }

    let encoded = &value[7..];
    let decoded = urlencoding::decode(encoded)
        .map_err(|_| "锁屏原图 URI 包含无效的百分号编码".to_string())?;
    if decoded.contains('\0') {
        return Err("锁屏原图路径无效".into());
    }

    // file:///C:/image.png -> C:\image.png
    // file://localhost/C:/image.png -> C:\image.png
    // file://server/share/image.png -> \\server\share\image.png
    let mut path = decoded.as_ref();
    if let Some(rest) = path.strip_prefix("localhost/") {
        path = rest;
    }
    if let Some(rest) = path.strip_prefix("LOCALHOST/") {
        path = rest;
    }

    let is_drive_after_slash = path.as_bytes().get(0) == Some(&b'/')
        && path.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic)
        && path.as_bytes().get(2) == Some(&b':');
    if is_drive_after_slash {
        path = &path[1..];
    }

    let path = if path.starts_with('/') {
        // A URI with a host becomes an UNC path once its leading slash is
        // retained. `file:////server/share` also lands here harmlessly.
        format!(r"\\{}", path.trim_start_matches('/').replace('/', r"\"))
    } else if path.as_bytes().get(1) == Some(&b':') {
        path.replace('/', r"\")
    } else {
        // RFC 8089 host form: file://server/share/image.png
        format!(r"\\{}", path.replace('/', r"\"))
    };

    if path.trim_matches('\\').is_empty() {
        return Err("锁屏原图路径无效".into());
    }
    Ok(PathBuf::from(path))
}

/// Returns whether WinRT's current `file:` URI points at our managed image.
/// URI encoding, slash style, drive-letter case, and existing symlinks are
/// normalized so diagnostics cannot accidentally claim a failed takeover was
/// successful.
pub fn managed_image_is_active(current_image_uri: Option<&str>, managed_image: &Path) -> bool {
    let Some(current_image_uri) = current_image_uri else {
        return false;
    };
    let Ok(current) = local_file_uri_to_path(current_image_uri) else {
        return false;
    };
    equivalent_windows_paths(&current, managed_image)
}

/// Compares two Windows `file:` URIs without relying on their serialized
/// spelling.  WinRT can change slash direction, URI escaping, drive-letter
/// case, and symlink spelling between two reads of the same image.
pub fn same_local_file_uri(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (local_file_uri_to_path(left), local_file_uri_to_path(right))
    else {
        return false;
    };
    equivalent_windows_paths(&left, &right)
}

/// Returns true when a restoration state exists, but Windows is no longer
/// using this application's managed sleep image.
///
/// Both the current durable manifest and a legacy URI-only marker are
/// ownership claims from an earlier takeover.  A legacy marker cannot safely
/// be migrated after the user has chosen another lock screen: doing so would
/// turn an old URI into a new restore point and could later overwrite the
/// user's current image.  Treat it as stale until the managed image is
/// positively active again.
pub fn has_stale_backup(backup: &LockScreenBackupState, managed_image_active: bool) -> bool {
    matches!(
        backup,
        LockScreenBackupState::Valid(_) | LockScreenBackupState::LegacyUri { .. }
    ) && !managed_image_active
}

pub fn inspect_backup(config_dir: &Path) -> LockScreenBackupState {
    let manifest = manifest_path(config_dir);
    if manifest.exists() {
        return read_manifest(&manifest)
            .and_then(|manifest| validate_manifest(config_dir, manifest))
            .map(LockScreenBackupState::Valid)
            .unwrap_or_else(|reason| LockScreenBackupState::Invalid { reason });
    }

    let legacy = legacy_manifest_path(config_dir);
    if !legacy.exists() {
        return LockScreenBackupState::Missing;
    }
    match fs::read_to_string(&legacy) {
        Ok(value) if !value.trim().is_empty() => match local_file_uri_to_path(value.trim()) {
            Ok(path) if path.is_file() => LockScreenBackupState::LegacyUri {
                original_image_uri: value.trim().to_string(),
            },
            Ok(_) => LockScreenBackupState::Invalid {
                reason: "旧锁屏备份指向的原图已不存在".into(),
            },
            Err(reason) => LockScreenBackupState::Invalid { reason },
        },
        Ok(_) => LockScreenBackupState::Invalid {
            reason: "旧锁屏备份为空".into(),
        },
        Err(error) => LockScreenBackupState::Invalid {
            reason: format!("无法读取旧锁屏备份：{error}"),
        },
    }
}

/// Ensures a durable image snapshot exists.  Existing verified backups are
/// never overwritten.  Corrupted state fails closed rather than risking the
/// user's only restore point.
#[cfg(test)]
pub fn ensure_backup(
    config_dir: &Path,
    current_original_image_uri: &str,
    captured_at_unix_ms: u64,
) -> Result<LockScreenBackupManifest, String> {
    Ok(
        ensure_backup_for_takeover(config_dir, current_original_image_uri, captured_at_unix_ms)?
            .manifest,
    )
}

/// Acquires a durable backup for a pending takeover and records whether this
/// call created it. Use [`discard_backup_after_failed_takeover`] if a later
/// resource or WinRT operation fails before Windows confirms the managed
/// image is active.
pub fn ensure_backup_for_takeover(
    config_dir: &Path,
    current_original_image_uri: &str,
    captured_at_unix_ms: u64,
) -> Result<LockScreenBackupLease, String> {
    match inspect_backup(config_dir) {
        LockScreenBackupState::Valid(manifest) => Ok(LockScreenBackupLease {
            manifest,
            created_for_this_attempt: false,
        }),
        LockScreenBackupState::Invalid { reason } => {
            Err(format!("现有锁屏备份不完整，已拒绝覆盖当前锁屏：{reason}"))
        }
        LockScreenBackupState::LegacyUri { original_image_uri } => {
            capture_backup(config_dir, &original_image_uri, captured_at_unix_ms).map(|manifest| {
                LockScreenBackupLease {
                    manifest,
                    created_for_this_attempt: true,
                }
            })
        }
        LockScreenBackupState::Missing => {
            capture_backup(config_dir, current_original_image_uri, captured_at_unix_ms).map(
                |manifest| LockScreenBackupLease {
                    manifest,
                    created_for_this_attempt: true,
                },
            )
        }
    }
}

/// Removes only a restore point created by the supplied failed attempt.
///
/// The persisted manifest must exactly match the lease before it is removed.
/// That comparison prevents an old lease from deleting a backup created by a
/// different attempt. Snapshot deletion is best effort after removing the
/// manifest; an unreachable private copy is safer than leaving a valid stale
/// manifest that could later overwrite the user's current lock screen.
pub fn discard_backup_after_failed_takeover(
    config_dir: &Path,
    lease: &LockScreenBackupLease,
) -> Result<(), String> {
    if !lease.created_for_this_attempt {
        return Ok(());
    }

    let path = manifest_path(config_dir);
    let persisted = read_manifest(&path)?;
    if persisted != lease.manifest {
        return Err("锁屏备份状态已被其他操作更改，拒绝清理本次失败接管的备份".into());
    }
    let snapshot = snapshot_path(config_dir, &lease.manifest.snapshot_file)?;
    fs::remove_file(&path).map_err(|error| format!("无法回滚锁屏备份状态：{error}"))?;
    let _ = fs::remove_file(snapshot);
    Ok(())
}

/// Returns the private snapshot that native code must pass to WinRT when
/// restoring.  Do not use `original_image_uri` for restoration: the original
/// source may have moved since takeover.
pub fn restore_snapshot_path(
    config_dir: &Path,
    manifest: &LockScreenBackupManifest,
) -> Result<PathBuf, String> {
    let manifest = validate_manifest(config_dir, manifest.clone())?;
    let snapshot = snapshot_path(config_dir, &manifest.snapshot_file)?;
    if !snapshot.is_file() {
        return Err("锁屏备份图片已不存在".into());
    }
    Ok(snapshot)
}

/// Removes the manifest after native code has positively verified that Windows
/// is using the restored image.  Snapshot cleanup is best effort: retaining an
/// unreachable private copy is safer than claiming a restore that did not
/// happen.
pub fn remove_backup_after_verified_restore(
    config_dir: &Path,
    manifest: &LockScreenBackupManifest,
) -> Result<(), String> {
    let snapshot = restore_snapshot_path(config_dir, manifest)?;
    fs::remove_file(manifest_path(config_dir))
        .map_err(|error| format!("已恢复锁屏，但无法清理备份状态：{error}"))?;
    let _ = fs::remove_file(snapshot);
    let _ = fs::remove_file(legacy_manifest_path(config_dir));
    Ok(())
}

fn capture_backup(
    config_dir: &Path,
    original_image_uri: &str,
    captured_at_unix_ms: u64,
) -> Result<LockScreenBackupManifest, String> {
    let source = local_file_uri_to_path(original_image_uri)?;
    if !source.is_file() {
        return Err("当前锁屏原图不是可读取的静态文件，无法安全接管".into());
    }

    let destination_dir = asset_directory(config_dir);
    fs::create_dir_all(&destination_dir)
        .map_err(|error| format!("无法创建锁屏备份目录：{error}"))?;
    let extension = safe_extension(&source);
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let snapshot_file = format!("original-{captured_at_unix_ms}-{sequence}.{extension}");
    let snapshot = snapshot_path(config_dir, &snapshot_file)?;
    copy_file_without_overwrite(&source, &snapshot)
        .map_err(|error| format!("无法保存锁屏原图副本：{error}"))?;

    let manifest = LockScreenBackupManifest {
        version: LOCK_SCREEN_BACKUP_SCHEMA_VERSION,
        original_image_uri: original_image_uri.trim().to_string(),
        snapshot_file,
        captured_at_unix_ms,
    };
    if let Err(error) = write_new_manifest(config_dir, &manifest) {
        let _ = fs::remove_file(snapshot);
        return Err(error);
    }
    Ok(manifest)
}

fn read_manifest(path: &Path) -> Result<LockScreenBackupManifest, String> {
    let raw = fs::read(path).map_err(|error| format!("无法读取锁屏备份状态：{error}"))?;
    serde_json::from_slice(&raw).map_err(|error| format!("锁屏备份状态格式无效：{error}"))
}

fn validate_manifest(
    config_dir: &Path,
    manifest: LockScreenBackupManifest,
) -> Result<LockScreenBackupManifest, String> {
    if manifest.version != LOCK_SCREEN_BACKUP_SCHEMA_VERSION {
        return Err(format!("不支持的锁屏备份版本：{}", manifest.version));
    }
    local_file_uri_to_path(&manifest.original_image_uri)?;
    let snapshot = snapshot_path(config_dir, &manifest.snapshot_file)?;
    if !snapshot.is_file() {
        return Err("锁屏备份图片已不存在".into());
    }
    Ok(manifest)
}

fn snapshot_path(config_dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(file_name);
    if candidate.is_absolute()
        || candidate.file_name().is_none()
        || candidate.file_name().is_none_or(|name| name != file_name)
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("锁屏备份引用了不安全的文件名".into());
    }
    Ok(asset_directory(config_dir).join(candidate))
}

fn safe_extension(source: &Path) -> String {
    let candidate = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 12
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        });
    candidate.unwrap_or_else(|| "img".into())
}

fn copy_file_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()
}

fn write_new_manifest(
    config_dir: &Path,
    manifest: &LockScreenBackupManifest,
) -> Result<(), String> {
    let path = manifest_path(config_dir);
    if path.exists() {
        return Err("锁屏备份状态已存在，拒绝覆盖".into());
    }
    fs::create_dir_all(config_dir).map_err(|error| format!("无法创建配置目录：{error}"))?;
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = config_dir.join(format!(".{LOCK_SCREEN_BACKUP_MANIFEST}.{sequence}.tmp"));
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("无法序列化锁屏备份状态：{error}"))?;
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法写入锁屏备份状态：{error}"));
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法提交锁屏备份状态：{error}"));
    }
    Ok(())
}

fn equivalent_windows_paths(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .replace('/', r"\")
            .trim_end_matches('\\')
            .to_string()
    };
    normalize(&left).eq_ignore_ascii_case(&normalize(&right))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn decodes_a_windows_file_uri_with_spaces() {
        assert_eq!(
            local_file_uri_to_path("file:///C:/Users/Alice/Lock%20Screen.png").unwrap(),
            PathBuf::from(r"C:\Users\Alice\Lock Screen.png")
        );
    }

    #[test]
    fn rejects_a_non_file_uri() {
        assert!(local_file_uri_to_path("https://example.test/image.png").is_err());
        assert!(local_file_uri_to_path("ms-appx:///Assets/lock.png").is_err());
    }

    #[test]
    fn capture_keeps_a_private_restore_copy_after_source_is_removed() {
        let root = tempdir().unwrap();
        let source = root.path().join("original image.png");
        fs::write(&source, b"original lock screen").unwrap();
        let uri = format!(
            "file:///{}",
            source
                .to_string_lossy()
                .replace('\\', "/")
                .replace(' ', "%20")
        );
        let config = root.path().join("config");

        let manifest = ensure_backup(&config, &uri, 42).unwrap();
        let snapshot = restore_snapshot_path(&config, &manifest).unwrap();
        assert_eq!(fs::read(&snapshot).unwrap(), b"original lock screen");
        fs::remove_file(source).unwrap();
        assert_eq!(
            fs::read(restore_snapshot_path(&config, &manifest).unwrap()).unwrap(),
            b"original lock screen"
        );
        assert_eq!(
            inspect_backup(&config),
            LockScreenBackupState::Valid(manifest)
        );
    }

    #[test]
    fn existing_verified_backup_is_not_overwritten() {
        let root = tempdir().unwrap();
        let first = root.path().join("first.png");
        let second = root.path().join("second.png");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let first_uri = format!("file:///{}", first.to_string_lossy().replace('\\', "/"));
        let second_uri = format!("file:///{}", second.to_string_lossy().replace('\\', "/"));
        let config = root.path().join("config");

        let initial = ensure_backup(&config, &first_uri, 10).unwrap();
        let repeat = ensure_backup(&config, &second_uri, 20).unwrap();
        assert_eq!(repeat, initial);
        assert_eq!(
            fs::read(restore_snapshot_path(&config, &repeat).unwrap()).unwrap(),
            b"first"
        );
    }

    #[test]
    fn refuses_to_overwrite_a_corrupt_backup_manifest() {
        let root = tempdir().unwrap();
        let config = root.path().join("config");
        fs::create_dir_all(&config).unwrap();
        fs::write(manifest_path(&config), b"not json").unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"source").unwrap();
        let uri = format!("file:///{}", source.to_string_lossy().replace('\\', "/"));

        assert!(ensure_backup(&config, &uri, 1)
            .unwrap_err()
            .contains("拒绝覆盖"));
    }

    #[test]
    fn rejects_manifest_path_traversal() {
        let root = tempdir().unwrap();
        let manifest = LockScreenBackupManifest {
            version: LOCK_SCREEN_BACKUP_SCHEMA_VERSION,
            original_image_uri: "file:///C:/original.png".into(),
            snapshot_file: "../unrelated.png".into(),
            captured_at_unix_ms: 1,
        };
        assert!(restore_snapshot_path(root.path(), &manifest).is_err());
    }

    #[test]
    fn recognizes_managed_uri_despite_encoding_and_case() {
        let managed = PathBuf::from(r"C:\Users\Alice\DSH Wallpaper\sleep.png");
        assert!(managed_image_is_active(
            Some("file:///c:/users/alice/DSH%20Wallpaper/sleep.png"),
            &managed
        ));
    }

    #[test]
    fn local_file_uri_comparison_normalizes_encoding_and_case() {
        assert!(same_local_file_uri(
            "file:///C:/Users/Alice/Lock%20Screen.png",
            "file:///c:/users/alice/Lock%20Screen.png"
        ));
        assert!(!same_local_file_uri(
            "file:///C:/Users/Alice/one.png",
            "file:///C:/Users/Alice/two.png"
        ));
        assert!(!same_local_file_uri(
            "https://example.test/image.png",
            "file:///C:/Users/Alice/image.png"
        ));
    }

    #[test]
    fn restoration_state_becomes_stale_when_current_lock_screen_is_not_managed() {
        let root = tempdir().unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"source").unwrap();
        let uri = format!("file:///{}", source.to_string_lossy().replace('\\', "/"));
        let config = root.path().join("config");
        let backup = inspect_backup(&config);
        assert!(!has_stale_backup(&backup, false));

        ensure_backup(&config, &uri, 1).unwrap();
        let backup = inspect_backup(&config);
        assert!(has_stale_backup(&backup, false));
        assert!(!has_stale_backup(&backup, true));
    }

    #[test]
    fn legacy_marker_is_stale_until_the_managed_image_is_active() {
        let root = tempdir().unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"source").unwrap();
        let config = root.path().join("config");
        fs::create_dir_all(&config).unwrap();
        let uri = format!("file:///{}", source.to_string_lossy().replace('\\', "/"));
        fs::write(legacy_manifest_path(&config), format!("{uri}\n")).unwrap();

        let state = inspect_backup(&config);
        assert!(matches!(state, LockScreenBackupState::LegacyUri { .. }));
        assert!(has_stale_backup(&state, false));
        assert!(!has_stale_backup(&state, true));
    }

    #[test]
    fn failed_takeover_discards_only_the_backup_created_by_that_attempt() {
        let root = tempdir().unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"source").unwrap();
        let uri = format!("file:///{}", source.to_string_lossy().replace('\\', "/"));
        let config = root.path().join("config");

        let fresh = ensure_backup_for_takeover(&config, &uri, 1).unwrap();
        assert!(fresh.created_for_this_attempt);
        discard_backup_after_failed_takeover(&config, &fresh).unwrap();
        assert_eq!(inspect_backup(&config), LockScreenBackupState::Missing);

        let existing = ensure_backup_for_takeover(&config, &uri, 2).unwrap();
        assert!(existing.created_for_this_attempt);
        let reused = ensure_backup_for_takeover(&config, &uri, 3).unwrap();
        assert!(!reused.created_for_this_attempt);
        discard_backup_after_failed_takeover(&config, &reused).unwrap();
        assert_eq!(
            inspect_backup(&config),
            LockScreenBackupState::Valid(existing.manifest)
        );
    }

    #[test]
    fn verified_restore_cleanup_removes_manifest_and_snapshot() {
        let root = tempdir().unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"source").unwrap();
        let uri = format!("file:///{}", source.to_string_lossy().replace('\\', "/"));
        let config = root.path().join("config");
        let manifest = ensure_backup(&config, &uri, 1).unwrap();
        remove_backup_after_verified_restore(&config, &manifest).unwrap();
        assert_eq!(inspect_backup(&config), LockScreenBackupState::Missing);
    }
}
