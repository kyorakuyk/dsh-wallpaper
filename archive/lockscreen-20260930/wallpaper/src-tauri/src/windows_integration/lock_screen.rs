// ARCHIVED — not compiled. 锁屏接管实现（lock-screen takeover），2026-10 批次 B2 归档。
//
// 来源：wallpaper/src-tauri/src/windows_integration.rs，commit 3c92772（tag `pre-freeze-isolation`）。
// 下面按原顺序逐字保留被移出的代码（含注释与 #[cfg] 属性），逻辑未改动；
// 原行号：283-335、3355-3758、4711-5015、5033-5068；测试 5190-5199、5487-5558。
// 本文件不在任何 crate 的模块树里，IDE 中 import 无法解析属预期。恢复办法见
// archive/lockscreen-20260930/README.md「2026-10 第二批归档（B2）」。
//
// 原 windows_integration.rs 中只被本段使用、已从现役删除的 import（原样写在这里便于恢复）：

#[cfg(windows)]
use crate::lock_screen_backup::{
    discard_backup_after_failed_takeover, discard_stale_backup, ensure_backup_for_takeover,
    has_stale_backup, inspect_backup, managed_image_is_active, managed_image_path,
    file_content_hash, managed_image_file_for_content, managed_image_path_from_file,
    remove_backup_after_verified_restore,
    restore_snapshot_path, same_local_file_uri, LockScreenBackupLease, LockScreenBackupManifest,
    LockScreenBackupState, LEGACY_MANAGED_IMAGE_FILE,
};
#[cfg(windows)]
use windows::{
    Storage::StorageFile,
    System::UserProfile::{LockScreen, UserProfilePersonalizationSettings},
    Storage::Packaging::Appx::GetCurrentPackagePath,
    Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0},
    Win32::System::Threading::{ReleaseMutex, WaitForSingleObject},
};

// 仍留在现役 windows_integration.rs、本段也依赖的项（并回时无需恢复）：
//   std::sync::OnceLock; serde::Serialize; tauri::Manager（`app.path()`）;
//   windows::core::{w, HSTRING, PCWSTR, PWSTR};
//   windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE(全路径)};
//   windows::Win32::System::Threading::CreateMutexW;
//   has_package_identity()（开机自启也在用，留在现役）。
//   `tokio::sync::Mutex` 以全路径使用，无 use 语句。


/// Serializes the whole native lock-screen ownership transaction.  The
/// settings WebView already avoids duplicate clicks, but commands can also
/// arrive from the tray, the frontend, or a second Tauri surface.  Without a
/// native lock, those callers could race between snapshot capture, the WinRT
/// setter, and rollback/restore cleanup.
#[cfg(windows)]
static LOCK_SCREEN_TRANSACTION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// The app normally has one process, but an installer update, a manual second
/// launch, or an old process winding down can overlap with it.  A process-local
/// async mutex cannot protect their shared current-user config directory, so
/// the native transaction also holds this current-session named mutex.
#[cfg(windows)]
const LOCK_SCREEN_TRANSACTION_MUTEX_NAME: windows::core::PCWSTR =
    w!("Local\\DSHWallpaper.LockScreenTransaction.v1");

#[cfg(windows)]
struct CrossProcessLockScreenTransaction {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl CrossProcessLockScreenTransaction {
    fn acquire() -> Result<Self, String> {
        let handle = unsafe { CreateMutexW(None, false, LOCK_SCREEN_TRANSACTION_MUTEX_NAME) }
            .map_err(|error| format!("无法建立锁屏接管事务锁：{error}"))?;
        let wait = unsafe { WaitForSingleObject(handle, 30_000) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            return Err("锁屏接管操作正在被另一个 dsh-wallpaper 实例处理，请稍后重试。".into());
        }
        if wait == WAIT_ABANDONED {
            // A previous owner exited while inside a transaction. Existing
            // manifest validation below is deliberately fail-closed; continue
            // only under that validation rather than trusting the abandoned
            // operation's partial state.
            log::warn!("检测到中断的锁屏接管事务；将按现有备份状态进行安全检查");
        }
        Ok(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for CrossProcessLockScreenTransaction {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Lock-screen ownership is shared by the Lite and full packages. Their AppX
/// identifiers intentionally differ, so `app_config_dir()` would otherwise
/// strand the only recovery manifest in whichever edition performed the
/// takeover first. Prefer one user-scoped directory and fall back to the
/// legacy full-edition directory when it already contains a recovery point.
#[cfg(windows)]
fn lock_screen_config_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let legacy = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let shared = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| legacy.clone())
        .join("DSHWallpaper");
    if legacy.join("lock-screen").exists() && !shared.join("lock-screen").exists() {
        return Ok(legacy);
    }
    std::fs::create_dir_all(&shared)
        .map_err(|error| format!("无法创建共享锁屏恢复目录：{error}"))?;
    Ok(shared)
}

#[cfg(windows)]
pub async fn set_lock_screen(app: &tauri::AppHandle, enabled: bool) -> Result<String, String> {
    let _transaction = LOCK_SCREEN_TRANSACTION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let _cross_process_transaction = CrossProcessLockScreenTransaction::acquire()?;
    let config_dir = lock_screen_config_dir(app)?;
    // Both takeover *and* restoration call the system setter.  Do not let an
    // unpackaged dev/NSIS process mutate the lock screen just because it finds
    // a recovery manifest left by an earlier run. The supported surface is an
    // MSIX-identified process only; preserving the manifest is safer than an
    // unverified write from a different installation context.
    let has_package_identity = has_package_identity()?;
    if !can_attempt_lock_screen_takeover(has_package_identity) {
        return Err("当前进程没有 MSIX 包身份。为确保锁屏接管和恢复可验证，常规桌面版不会修改系统锁屏；现有恢复点已保留。请安装 MSIX 包后重试。".into());
    }
    if !UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())? {
        return Err("当前 Windows 策略不允许应用修改锁屏图片；现有恢复点已保留。".into());
    }
    if enabled {
        // Windows accepts the packaged resource but rejects the same bytes
        // after they are copied into ordinary Roaming app-data on this system.
        // Resolve it before ownership checks so an existing MSIX takeover is
        // recognised rather than mistaken for an external change.
        let packaged_sleep_image = bundled_sleep_image_path(app)?;
        let original = LockScreen::OriginalImageFile()
            .map_err(|_| "无法读取当前锁屏图片；为避免无法恢复，已取消接管。".to_string())?
            .AbsoluteUri()
            .map_err(|_| "无法读取当前锁屏图片；为避免无法恢复，已取消接管。".to_string())?
            .to_string();
        let backup_state = inspect_backup(&config_dir);
        if let LockScreenBackupState::Invalid { reason } = &backup_state {
            return Err(format!("现有锁屏备份不完整，已拒绝覆盖当前锁屏：{reason}"));
        }
        let managed_path = managed_image_path_for_state(&config_dir, &backup_state)?;
        let managed_image_active = managed_image_is_active(Some(&original), &managed_path)
            || managed_image_is_active(Some(&original), &packaged_sleep_image);
        if has_stale_backup(&backup_state, managed_image_active) {
            return Err("检测到接管期间锁屏已由用户或其他程序更改。为避免覆盖当前锁屏，应用不会再次接管；原备份已保留。请先在 Windows 设置中确认锁屏图片，再决定是否清理或恢复。".into());
        }
        // A verified active manifest already represents the exact image Windows
        // owns.  Repeating a setter with the same filename is explicitly
        // rejected by Windows, while switching it to a new filename would
        // require replacing the only restore manifest.  Therefore this is a
        // true idempotent success, not a second takeover attempt.
        if matches!(&backup_state, LockScreenBackupState::Valid(_)) && managed_image_active {
            if managed_path.is_file() || packaged_sleep_image.is_file() {
                return Ok("锁屏图片已经由本应用接管；原静态图片备份仍可恢复。".into());
            }
            return Err("Windows 仍指向本应用的锁屏图片，但托管图片文件已丢失。为避免覆盖唯一的原图备份，应用没有再次接管；请先恢复原锁屏或在 Windows 设置中重新选择图片。".into());
        }
        if managed_image_active && matches!(&backup_state, LockScreenBackupState::Missing) {
            return Err("当前锁屏已经是本应用的熟睡画面，但原锁屏备份不存在。为避免把托管图片误当作原图，已拒绝再次接管；请先在 Windows 设置中手动选择原图。".into());
        }
        // Do not re-resolve this path here. `packaged_sleep_image` is the
        // verified MSIX Assets payload; a former shadowing declaration below
        // silently replaced it with Tauri's `_up_` resource and caused the
        // misleading 0x800700A1 failures.
        let bundled_sleep_image = packaged_sleep_image;

        // WinRT's lock-screen API is unreliable with paths inside a Tauri
        // resource bundle (especially during `tauri dev`): it can receive a
        // virtual/masked resource path and return 0x800700A1.  Hand Windows a
        // normal, current-user-owned file instead.
        let captured_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "系统时间无效，已取消锁屏接管。".to_string())?
            .as_millis() as u64;
        // 名字由**内容**决定（同一张素材永远同一个文件名）：Windows 把每个不同的文件名都记成
        // "最近使用的图像"里的一条，按次生成名字会让同一张图占掉多个栏位（用户实测三个）。
        let content_hash = file_content_hash(&bundled_sleep_image)?;
        let managed_image_file = managed_image_file_for_content(&content_hash, "png")?;
        let managed_path = managed_image_path_from_file(&config_dir, &managed_image_file)?;
        copy_sleep_image_without_overwrite(&bundled_sleep_image, &managed_path)?;
        let lease = match ensure_backup_for_takeover(
            &config_dir,
            &original,
            captured_at,
            &managed_image_file,
        ) {
            Ok(lease) => lease,
            Err(error) => {
                // There is no manifest ownership record for this freshly
                // allocated path when acquisition fails, so it is safe to
                // remove only this orphaned candidate.
                let _ = std::fs::remove_file(&managed_path);
                return Err(error);
            }
        };
        let lease_managed_path = match managed_image_path(&config_dir, &lease.manifest) {
            Ok(path) => path,
            Err(_) => {
                // Do not clean up here. A concurrent process could have
                // changed the persisted manifest between acquisition and this
                // validation; without a verified ownership relation, keeping
                // every file is safer than deleting one it may now reference.
                return abort_lock_screen_takeover_before_set(
                    &config_dir,
                    &lease,
                    "锁屏备份状态在接管准备期间无法验证；应用没有修改锁屏。",
                );
            }
        };
        if lease_managed_path != managed_path {
            // `ensure_backup_for_takeover` is intentionally non-destructive
            // when it discovers an existing valid manifest.  If the manifest
            // changed after our preflight, its managed image is not the unique
            // candidate we just prepared, so setting that candidate would
            // leave no durable ownership record for it.  Refuse the setter
            // and remove only our unreferenced candidate.
            let _ = std::fs::remove_file(&managed_path);
            return Err(
                "锁屏备份状态在接管准备期间发生变化；应用没有修改锁屏。请刷新检查后重试。".into(),
            );
        }
        // A user can change their lock screen while the resource copy and
        // snapshot above are running.  Re-read immediately before the only
        // setter and refuse to overwrite a newer choice.  This cannot remove
        // the unavoidable kernel-level race, but it makes the application
        // itself fail closed across its full preflight transaction.
        let current_before_set = LockScreen::OriginalImageFile()
            .ok()
            .and_then(|uri| uri.AbsoluteUri().ok())
            .map(|uri| uri.to_string());
        let Some(current_before_set) = current_before_set else {
            return abort_lock_screen_takeover_before_set(
                &config_dir,
                &lease,
                "Windows 未能在写入前重新读取锁屏状态；应用没有修改锁屏，已取消接管。",
            );
        };
        if !same_local_file_uri(&original, &current_before_set) {
            return abort_lock_screen_takeover_before_set(
                &config_dir,
                &lease,
                "检测到锁屏图片在接管准备期间已被用户或其他程序更改；应用没有覆盖新图片。",
            );
        }
        // Resolve all non-mutating WinRT prerequisites before the setter.  If
        // one fails, this attempt is proven not to have asked Windows to take
        // ownership, so rolling back its brand-new snapshot is safe.
        // Do not pass the app-data copy here. The isolated MSIX probe proved
        // that this Windows build returns `false` for it, but succeeds for the
        // package-owned sleep asset.
        let path_string = HSTRING::from(bundled_sleep_image.to_string_lossy().as_ref());
        let file = match StorageFile::GetFileFromPathAsync(&path_string)
            .map_err(|error| {
                format!(
                    "Windows 无法读取准备好的锁屏图片（路径：{}；WinRT：{error}）。",
                    bundled_sleep_image.display()
                )
            })
            .and_then(|operation| {
                operation.get().map_err(|error| {
                    format!(
                        "Windows 无法打开准备好的锁屏图片（路径：{}；WinRT：{error}）。",
                        bundled_sleep_image.display()
                    )
                })
            }) {
            Ok(file) => file,
            Err(error) => {
                return abort_lock_screen_takeover_before_set(&config_dir, &lease, &error);
            }
        };
        let settings = match UserProfilePersonalizationSettings::Current()
            .map_err(|_| "Windows 无法打开锁屏个性化设置。".to_string())
        {
            Ok(settings) => settings,
            Err(error) => {
                return abort_lock_screen_takeover_before_set(&config_dir, &lease, &error);
            }
        };
        // From this point on, every failure is treated as indeterminate.  A
        // completed/asynchronous WinRT call can have changed the setting even
        // if it reports an error or its immediate query lags, so do not delete
        // the managed image or its original-image recovery point.
        let set_result = (|| -> Result<(), String> {
            let changed = settings
                .TrySetLockScreenImageAsync(&file)
                .map_err(|error| {
                    // Preserve the WinRT/HRESULT detail for the test build.
                    // It is the only useful lead when Windows refuses before
                    // returning the documented boolean result.
                    format!("Windows 未能启动锁屏图片设置请求（WinRT：{error}）。")
                })?
                .get()
                .map_err(|error| format!("Windows 未能完成锁屏图片设置请求（WinRT：{error}）。"))?;
            if !changed {
                // The supported MSIX route has one authoritative setter.
                // Microsoft defines `false` as an unsuccessful change, not as
                // permission to silently try a legacy API.  Fail closed and
                // keep the just-created recovery point for the final ownership
                // check below instead of risking a second, unverified write.
                return Err("Windows 拒绝了锁屏图片设置请求，但未返回具体原因。当前锁屏图片、MSIX 包身份、系统支持状态和托管图片文件预检均已通过；恢复点已保留，测试版没有尝试旧兼容接口。".into());
            }
            Ok(())
        })();
        finish_lock_screen_takeover_attempt(&config_dir, &lease, &bundled_sleep_image, set_result)
    } else {
        let current_image_uri = LockScreen::OriginalImageFile()
            .ok()
            .and_then(|uri| uri.AbsoluteUri().ok())
            .map(|uri| uri.to_string());
        let backup_state = inspect_backup(&config_dir);
        if let LockScreenBackupState::Invalid { reason } = &backup_state {
            return Err(format!("现有锁屏备份不完整，无法安全恢复：{reason}"));
        }
        let managed_path = managed_image_path_for_state(&config_dir, &backup_state)?;
        let packaged_sleep_image = bundled_sleep_image_path(app)?;
        let managed_image_active =
            managed_image_is_active(current_image_uri.as_deref(), &managed_path)
                || managed_image_is_active(current_image_uri.as_deref(), &packaged_sleep_image);
        if has_stale_backup(&backup_state, managed_image_active) {
            return Ok("已停止本应用的锁屏接管状态。检测到当前锁屏已由用户或其他程序更改，因此未覆盖它；原备份已保留。".into());
        }
        if let LockScreenBackupState::Valid(manifest) = backup_state {
            if let Ok(path) = restore_snapshot_path(&config_dir, &manifest) {
                let manifest_managed_path = managed_image_path(&config_dir, &manifest)?;
                let expected_current_image =
                    if managed_image_is_active(current_image_uri.as_deref(), &packaged_sleep_image)
                    {
                        &packaged_sleep_image
                    } else {
                        &manifest_managed_path
                    };
                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(
                    path.to_string_lossy().as_ref(),
                ))
                .map_err(|e| e.to_string())?
                .get()
                .map_err(|e| e.to_string())?;
                let settings =
                    UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
                // Do not restore over a lock-screen image selected after the
                // first stale-state check above. Opening the snapshot and the
                // settings object can take long enough for the user, Windows,
                // or another program to change the image. Re-read immediately
                // before the only restore setter and fail closed if this
                // durable manifest no longer owns the current image.
                let current_before_restore = LockScreen::OriginalImageFile()
                    .ok()
                    .and_then(|uri| uri.AbsoluteUri().ok())
                    .map(|uri| uri.to_string());
                if !restore_precondition_is_satisfied(
                    current_before_restore.as_deref(),
                    expected_current_image,
                ) {
                    return Err("检测到锁屏图片在恢复准备期间已由用户或其他程序更改；应用没有覆盖新图片，原备份已保留。".into());
                }
                let restored = settings
                    .TrySetLockScreenImageAsync(&file)
                    .map_err(|e| e.to_string())?
                    .get()
                    .map_err(|e| e.to_string())?;
                if restored {
                    return finish_verified_lock_screen_restore(&config_dir, &manifest, &path);
                }
            }
        }
        Err("未能恢复原静态锁屏图片；接管仍保持启用，原备份没有被删除。请稍后重试或在 Windows 设置中手动恢复。".into())
    }
}

#[cfg(windows)]
pub async fn clear_stale_lock_screen_backup(
    app: &tauri::AppHandle,
    confirmed: bool,
) -> Result<String, String> {
    if !confirmed {
        return Err(
            "清理旧锁屏恢复点需要明确确认；该操作会永久删除已保存的原锁屏图片副本。".into(),
        );
    }
    let _transaction = LOCK_SCREEN_TRANSACTION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let _cross_process_transaction = CrossProcessLockScreenTransaction::acquire()?;
    if !has_package_identity()? {
        return Err("当前进程没有 MSIX 包身份，拒绝清理锁屏恢复点。".into());
    }
    let config_dir = lock_screen_config_dir(app)?;
    let current = LockScreen::OriginalImageFile()
        .map_err(|_| "无法读取当前锁屏图片；旧恢复点已保留。".to_string())?
        .AbsoluteUri()
        .map_err(|_| "无法读取当前锁屏图片；旧恢复点已保留。".to_string())?
        .to_string();
    let state = inspect_backup(&config_dir);
    let managed_path = managed_image_path_for_state(&config_dir, &state)?;
    let packaged_sleep_image = bundled_sleep_image_path(app)?;
    let managed_image_active = managed_image_is_active(Some(&current), &managed_path)
        || managed_image_is_active(Some(&current), &packaged_sleep_image);
    discard_stale_backup(&config_dir, managed_image_active)?;
    Ok("已清理旧锁屏恢复点；Windows 当前锁屏图片未作任何修改，保存的原图副本已永久删除。".into())
}

/// Restoring a snapshot is only safe while the current lock-screen image is
/// still the application-owned image recorded by the durable manifest.  Keep
/// this decision separate from the WinRT setter so the unit test documents the
/// fail-closed boundary without changing a real system setting.
#[cfg(windows)]
fn restore_precondition_is_satisfied(
    current_image_uri: Option<&str>,
    managed_image: &std::path::Path,
) -> bool {
    managed_image_is_active(current_image_uri, managed_image)
}

#[cfg(windows)]
fn finish_lock_screen_takeover_attempt(
    config_dir: &std::path::Path,
    lease: &LockScreenBackupLease,
    expected_managed_image: &std::path::Path,
    set_result: Result<(), String>,
) -> Result<String, String> {
    // Derive the expected filename from the durable manifest rather than the
    // provisional path prepared by the caller.  This keeps post-set ownership
    // verification and rollback coupled to one source of truth.
    let _legacy_managed_image = managed_image_path(config_dir, &lease.manifest)?;
    // Never discard a newly captured restore point merely because the
    // verification query failed. The setter can succeed even when a later
    // OriginalImageFile read is unavailable; in that indeterminate case the
    // backup is the only safe recovery path.
    let current_is_managed = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| managed_image_is_active(Some(&uri.to_string()), expected_managed_image));

    match (set_result, current_is_managed) {
        // Even when an API reports an error, the authoritative ownership check
        // says Windows is using our image.  Keep the recovery point.
        (_, Some(true)) => Ok("锁屏图片已设置；密码页继续由 Windows 原生模糊处理。".into()),
        // A successful setter plus an ambiguous or mismatched later read is
        // not proof that the setter failed.  Query propagation can lag the
        // completed WinRT operation, and Windows can normalize a path in ways
        // we do not recognize.  Retaining the only original-image snapshot is
        // safer than trying to make the transaction look clean.
        (Ok(()), Some(false)) => Err("Windows 未确认锁屏已切换为本应用的熟睡画面；恢复点已保留，应用没有删除任何原图备份。请刷新检查后再决定是否恢复或重试。".into()),
        (Ok(()), None) => Err("Windows 已完成锁屏设置请求，但无法读取最终状态；恢复点已保留，应用没有删除任何原图备份。请刷新检查后再决定是否恢复或重试。".into()),
        // The setter was invoked before this error.  WinRT may complete an
        // asynchronous write even when the operation reports an error or an
        // immediate read is stale, so retain both files rather than risk
        // deleting an image Windows still references.
        (Err(error), Some(false)) => Err(format!(
            "{error} Windows 当前未确认锁屏已切换；恢复点和托管图片均已保留，以避免删除可能仍被系统引用的文件。"
        )),
        (Err(error), None) => Err(format!(
            "{error} 同时无法确认锁屏最终状态；恢复点已保留，应用没有删除任何原图备份。"
        )),
    }
}

#[cfg(windows)]
fn abort_lock_screen_takeover_before_set(
    config_dir: &std::path::Path,
    lease: &LockScreenBackupLease,
    reason: &str,
) -> Result<String, String> {
    match discard_backup_after_failed_takeover(config_dir, lease) {
        Ok(()) => Err(reason.into()),
        Err(_) => Err(format!(
            "{reason} 本次接管创建的恢复点未能清理，已保留以避免丢失原图。"
        )),
    }
}

#[cfg(windows)]
fn finish_verified_lock_screen_restore(
    config_dir: &std::path::Path,
    manifest: &LockScreenBackupManifest,
    expected_image: &std::path::Path,
) -> Result<String, String> {
    let current_uri = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| uri.to_string());
    if !managed_image_is_active(current_uri.as_deref(), expected_image) {
        return Err("Windows 未确认原锁屏图片已恢复；接管备份已保留，请稍后重试或在 Windows 设置中手动恢复。".into());
    }
    remove_backup_after_restore(config_dir, manifest)?;
    Ok("已恢复接管前的静态锁屏图片。".into())
}

#[cfg(windows)]
fn can_attempt_lock_screen_takeover(has_package_identity: bool) -> bool {
    has_package_identity
}

#[cfg(windows)]
fn bundled_sleep_resource_candidates() -> [&'static str; 2] {
    [
        "_up_/public/personas/wake-frames/variant-anima/sleep.png",
        // Compatibility for older test bundles assembled before the current
        // Tauri resource mapping was documented and verified.
        "personas/wake-frames/variant-anima/sleep.png",
    ]
}

/// Returns the physical installation root of the current MSIX package.  This
/// avoids relying on the executable's apparent location, which Tauri can
/// report from its `_up_` resource context rather than the package root.
#[cfg(windows)]
fn current_package_install_root() -> Option<std::path::PathBuf> {
    let mut length = 0u32;
    // Per GetCurrentPackagePath's contract, a null *optional* buffer queries
    // the required length. `Some(PWSTR::null())` is not equivalent here and
    // can make the function fail, sending us down the legacy `_up_` fallback.
    let first = unsafe { GetCurrentPackagePath(&mut length, None) };
    if first != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return None;
    }
    // The first call reports the UTF-16 payload length. Supply one additional
    // element for the terminating NUL and pass that capacity back explicitly.
    // Passing the exact first value makes some Windows builds return
    // ERROR_INSUFFICIENT_BUFFER a second time.
    let mut buffer = vec![0u16; length as usize + 1];
    let mut capacity = buffer.len() as u32;
    let result = unsafe { GetCurrentPackagePath(&mut capacity, Some(PWSTR(buffer.as_mut_ptr()))) };
    if !result.is_ok() || capacity == 0 {
        return None;
    }
    // The API includes the NUL terminator in the requested capacity but not
    // necessarily in the returned length; trim it defensively either way.
    let value = String::from_utf16_lossy(&buffer[..capacity as usize]);
    let value = value.trim_end_matches('\0');
    (!value.is_empty()).then(|| std::path::PathBuf::from(value))
}

/// Resolves the on-disk package asset used by the MSIX lock-screen setter.
/// A packaged asset is intentionally distinct from the app-data marker kept
/// by the recovery manifest: the latter is never supplied to Windows.
#[cfg(windows)]
fn bundled_sleep_image_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    // Tauri's resource resolver is appropriate for web assets, but the
    // lock-screen WinRT API requires an ordinary, physical package file. In
    // an MSIX install that file lives next to the executable under `_up_`.
    // Prefer this direct path (the same strategy proven by the isolated
    // probe), then retain resolver/source fallbacks for development builds.
    // The Tauri resolver has already proven it can locate `_up_` in the
    // packaged process. Use that physical path as the package-root anchor,
    // then address the dedicated MSIX Assets file directly. Do not guard this
    // branch with `Path::is_file`: WindowsApps can give a packaged process a
    // restricted stat result even though StorageFile can open the asset.
    // Lock-screen takeover is only enabled after the MSIX identity check in
    // `set_lock_screen`. Therefore the executable's parent is the package
    // root here. Return the conventional Assets path directly rather than
    // asking `Path::is_file`, whose result can be virtualized for this process.
    if let Ok(executable) = std::env::current_exe() {
        if let Some(package_root) = executable.parent() {
            return Ok(package_root.join("Assets").join("LockScreenSleep.png"));
        }
    }

    let resolver_asset = app
        .path()
        .resolve(
            "_up_/public/personas/wake-frames/variant-anima/sleep.png",
            tauri::path::BaseDirectory::Resource,
        )
        .ok()
        .and_then(|resource| {
            resource
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case("_up_"))
                })
                .and_then(std::path::Path::parent)
                .map(|root| root.join("Assets").join("LockScreenSleep.png"))
        });

    resolver_asset
        .or_else(|| {
            current_package_install_root()
                .or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
                })
                .and_then(|root| {
                    std::iter::once(root.join("Assets").join("LockScreenSleep.png"))
                        .chain(
                            bundled_sleep_resource_candidates()
                                .into_iter()
                                .map(|resource| root.join(resource)),
                        )
                        .find(|path| path.is_file())
                })
        })
        .or_else(|| {
            bundled_sleep_resource_candidates()
                .into_iter()
                .find_map(|resource| {
                    app.path()
                        .resolve(resource, tauri::path::BaseDirectory::Resource)
                        .ok()
                        .filter(|path| path.is_file())
                })
        })
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|cwd| {
                    cwd.join("public")
                        .join("personas/wake-frames/variant-anima/sleep.png")
                })
                .filter(|path| path.is_file())
        })
        .or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|root| {
                    root.join("public")
                        .join("personas/wake-frames/variant-anima/sleep.png")
                })
                .filter(|path| path.is_file())
        })
        .ok_or_else(|| "未找到内置的锁屏睡眠图片。请重新安装 dsh-wallpaper。".to_string())
}

/// Resolves the managed image that is meaningful for the currently persisted
/// ownership state.  There is intentionally no speculative new filename here:
/// diagnostics and stale-state checks must only compare against a file that a
/// manifest has already committed.  Before the first takeover (and for a
/// legacy URI-only marker), the old fixed name is the only compatible path.
#[cfg(windows)]
fn managed_image_path_for_state(
    config_dir: &std::path::Path,
    state: &LockScreenBackupState,
) -> Result<std::path::PathBuf, String> {
    match state {
        LockScreenBackupState::Valid(manifest) => managed_image_path(config_dir, manifest),
        LockScreenBackupState::Missing | LockScreenBackupState::LegacyUri { .. } => {
            managed_image_path_from_file(config_dir, LEGACY_MANAGED_IMAGE_FILE)
        }
        LockScreenBackupState::Invalid { reason } => Err(format!(
            "现有锁屏备份不完整，无法安全解析托管图片：{reason}"
        )),
    }
}

/// Copies a bundled lock-screen image to its content-addressed destination.
///
/// The destination **must** already hold identical bytes when it exists: the name is derived from
/// the content, so a same-named file with different bytes can only mean a corrupted or hostile
/// asset directory.  That case fails closed instead of overwriting, and the identical case is a
/// reuse (the common one — every later takeover of the same art lands on the same file, which is
/// what keeps Windows' "recent images" list at one entry per distinct image).
#[cfg(windows)]
fn copy_sleep_image_without_overwrite(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    if destination.exists() {
        let existing = crate::lock_screen_backup::file_content_hash(destination)?;
        let wanted = crate::lock_screen_backup::file_content_hash(source)?;
        if existing == wanted {
            return Ok(());
        }
        return Err(format!(
            "锁屏托管图片的既有副本内容与素材不一致，已拒绝覆盖：{}",
            destination.display()
        ));
    }
    use std::io::{Read, Write};

    let parent = destination
        .parent()
        .ok_or_else(|| "锁屏图片存放目录无效".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建锁屏图片存放目录：{error}"))?;

    // Do not use one all-in-one closure here: if `create_new` reports that a
    // same-named file already exists, cleanup must *not* delete that existing
    // file.  Only failures after we successfully create this exact destination
    // are ours to roll back.
    let mut input =
        std::fs::File::open(source).map_err(|error| format!("无法读取内置锁屏图片：{error}"))?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("无法创建新的锁屏图片副本：{error}"))?;
    let result = (|| -> std::io::Result<()> {
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let bytes = input.read(&mut buffer)?;
            if bytes == 0 {
                break;
            }
            output.write_all(&buffer[..bytes])?;
        }
        output.sync_all()
    })();
    drop(output);
    if let Err(error) = result {
        let _ = std::fs::remove_file(destination);
        return Err(format!(
            "无法准备锁屏图片。请检查应用安装目录是否完整：{error}"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn remove_backup_after_restore(
    config_dir: &std::path::Path,
    manifest: &LockScreenBackupManifest,
) -> Result<(), String> {
    remove_backup_after_verified_restore(config_dir, manifest)
}

/// Read-only preflight for lock-screen ownership. It never calls a WinRT setter.
#[cfg(windows)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScreenDiagnostics {
    pub supported: bool,
    pub package_identity: bool,
    pub takeover_available: bool,
    pub original_image_uri: Option<String>,
    pub backup_exists: bool,
    pub backup_valid: bool,
    pub stale_backup: bool,
    pub managed_image_ready: bool,
    pub managed_image_active: bool,
    pub development_build: bool,
    pub warnings: Vec<String>,
}

#[cfg(windows)]
pub fn lock_screen_diagnostics(app: &tauri::AppHandle) -> Result<LockScreenDiagnostics, String> {
    let config_dir = lock_screen_config_dir(app)?;
    let original_image_uri = LockScreen::OriginalImageFile()
        .ok()
        .and_then(|uri| uri.AbsoluteUri().ok())
        .map(|uri| uri.to_string());
    let backup_state = inspect_backup(&config_dir);
    // Diagnostics must use the filename committed in the manifest.  Falling
    // back to the legacy fixed path makes a healthy v2 takeover look inactive.
    // An invalid manifest intentionally yields no managed path rather than
    // trusting unvalidated disk data.
    let managed_image = managed_image_path_for_state(&config_dir, &backup_state).ok();
    let backup_exists = !matches!(backup_state, LockScreenBackupState::Missing);
    let backup_valid = matches!(backup_state, LockScreenBackupState::Valid(_));
    let managed_image_active = managed_image
        .as_deref()
        .is_some_and(|path| managed_image_is_active(original_image_uri.as_deref(), path))
        || bundled_sleep_image_path(app)
            .ok()
            .is_some_and(|path| managed_image_is_active(original_image_uri.as_deref(), &path));
    let stale_backup = has_stale_backup(&backup_state, managed_image_active);
    let supported = UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())?;
    let package_identity = has_package_identity()?;
    let takeover_available = supported && can_attempt_lock_screen_takeover(package_identity);
    let mut warnings = Vec::new();
    if !supported {
        warnings.push("当前 Windows 策略不允许应用修改锁屏图片。".into());
    }
    if backup_exists && !backup_valid {
        warnings.push("已发现不完整的锁屏备份；应用会拒绝新的接管，以防覆盖唯一的恢复点。".into());
    }
    if stale_backup {
        warnings.push("检测到原锁屏备份，但当前锁屏已由用户或其他程序更改；应用不会恢复或再次接管，以免覆盖当前图片。备份已保留。".into());
    }
    if original_image_uri
        .as_deref()
        .is_none_or(|uri| !uri.starts_with("file:///"))
    {
        warnings.push("当前锁屏可能由 Windows Spotlight 或其他动态来源管理，Windows API 无法可靠还原该动态状态。".into());
    }
    if !package_identity {
        warnings.push("当前进程没有 MSIX 包身份；正式桌面版不会接管锁屏。请安装 MSIX 包。".into());
    }
    Ok(LockScreenDiagnostics {
        supported,
        package_identity,
        takeover_available,
        original_image_uri,
        backup_exists,
        backup_valid,
        stale_backup,
        managed_image_ready: managed_image.is_some_and(|path| path.is_file()),
        managed_image_active,
        development_build: cfg!(debug_assertions),
        warnings,
    })
}

#[cfg(not(windows))]
pub async fn set_lock_screen(_: &tauri::AppHandle, _: bool) -> Result<String, String> {
    Err("Lock screen integration only supports Windows".into())
}
#[cfg(not(windows))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScreenDiagnostics {
    pub supported: bool,
    pub package_identity: bool,
    pub takeover_available: bool,
    pub original_image_uri: Option<String>,
    pub backup_exists: bool,
    pub backup_valid: bool,
    pub stale_backup: bool,
    pub managed_image_ready: bool,
    pub managed_image_active: bool,
    pub development_build: bool,
    pub warnings: Vec<String>,
}
#[cfg(not(windows))]
pub fn lock_screen_diagnostics(_: &tauri::AppHandle) -> Result<LockScreenDiagnostics, String> {
    Ok(LockScreenDiagnostics {
        supported: false,
        package_identity: false,
        takeover_available: false,
        original_image_uri: None,
        backup_exists: false,
        backup_valid: false,
        stale_backup: false,
        managed_image_ready: false,
        managed_image_active: false,
        development_build: false,
        warnings: vec!["锁屏接管仅支持 Windows。".into()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(windows)]
    #[test]
    fn packaged_builds_are_always_eligible_for_lock_screen_takeover() {
        assert!(can_attempt_lock_screen_takeover(true));
    }

    #[test]
    fn unpackaged_lock_screen_takeover_is_never_eligible() {
        assert!(!can_attempt_lock_screen_takeover(false));
    }

    #[test]
    fn packaged_sleep_resource_uses_tauris_verified_windows_path_first() {
        assert_eq!(
            bundled_sleep_resource_candidates()[0],
            "_up_/public/personas/wake-frames/variant-anima/sleep.png"
        );
    }

    #[test]
    fn v2_backup_uses_its_manifest_managed_filename() {
        let root = tempdir().expect("temporary directory");
        let config = root.path().join("config");
        let assets = crate::lock_screen_backup::asset_directory(&config);
        std::fs::create_dir_all(&assets).expect("asset directory");
        std::fs::write(assets.join("original.png"), b"original").expect("snapshot");
        let manifest = LockScreenBackupManifest {
            version: crate::lock_screen_backup::LOCK_SCREEN_BACKUP_SCHEMA_VERSION,
            original_image_uri: "file:///C:/Users/Test/original.png".into(),
            snapshot_file: "original.png".into(),
            managed_image_file: "dsh-wallpaper-sleep-123-0.png".into(),
            captured_at_unix_ms: 123,
        };

        let managed =
            managed_image_path_for_state(&config, &LockScreenBackupState::Valid(manifest))
                .expect("dynamic managed path");

        assert_eq!(
            managed.file_name().and_then(|name| name.to_str()),
            Some("dsh-wallpaper-sleep-123-0.png")
        );
    }

    #[test]
    fn managed_sleep_copy_never_overwrites_an_existing_path() {
        let root = tempdir().expect("temporary directory");
        let source = root.path().join("source.png");
        let destination = root.path().join("managed.png");
        std::fs::write(&source, b"new image").expect("source image");
        std::fs::write(&destination, b"existing image").expect("existing managed image");

        assert!(copy_sleep_image_without_overwrite(&source, &destination).is_err());
        assert_eq!(
            std::fs::read(&destination).expect("existing managed image remains"),
            b"existing image"
        );

        let unique_destination = root.path().join("managed-unique.png");
        copy_sleep_image_without_overwrite(&source, &unique_destination)
            .expect("new managed image");
        assert_eq!(
            std::fs::read(unique_destination).expect("new managed image contents"),
            b"new image"
        );
    }

    #[test]
    fn restore_precondition_requires_the_current_managed_image() {
        let managed = std::path::Path::new(
            r"C:\Users\Test\AppData\Roaming\dsh-wallpaper\lock-screen\managed.png",
        );

        assert!(restore_precondition_is_satisfied(
            Some("file:///c:/users/test/AppData/Roaming/dsh-wallpaper/lock-screen/managed.png"),
            managed,
        ));
        assert!(!restore_precondition_is_satisfied(
            Some("file:///C:/Users/Test/Pictures/user-selected.png"),
            managed,
        ));
        assert!(!restore_precondition_is_satisfied(None, managed));
    }
}
