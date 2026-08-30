//! Optional Explorer wallpaper fallback for the first post-login hand-off.
//!
//! The WorkerW WebView cannot paint until the process and WebView2 have
//! started.  Keeping the ordinary Windows desktop wallpaper on the same
//! sleep artwork removes the visual flash of the user's previous wallpaper in
//! that small window.  This module changes the system wallpaper only after an
//! explicit Lite setting, records the original local file, and refuses to
//! overwrite a wallpaper changed by the user or another application.

#[cfg(windows)]
use std::{fs, path::{Path, PathBuf}};

#[cfg(windows)]
use serde::{Deserialize, Serialize};

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE,
    SPIF_UPDATEINIFILE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

#[cfg(windows)]
const STATE_VERSION: u32 = 1;
#[cfg(windows)]
const STATE_FILE: &str = "desktop-wallpaper-fallback.json";
#[cfg(windows)]
const FALLBACK_DIRECTORY: &str = "desktop-wallpaper-fallback";
#[cfg(windows)]
const FALLBACK_FILE: &str = "sleep.png";
#[cfg(windows)]
const SLEEP_ASSET: &str = "personas/wake-frames/variant-anima/sleep.png";
#[cfg(windows)]
const SLEEP_PACKAGE_ASSET: &str = "LockScreenSleep.png";

#[cfg(windows)]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopWallpaperBackup {
    version: u32,
    original_path: String,
    managed_path: String,
}

/// Read-only state exposed to the Lite settings surface. `managed_active` is
/// deliberately separate from `enabled`: a valid recovery point may remain
/// after the user changes the wallpaper outside the app, in which case the
/// app must report the conflict instead of silently overwriting it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWallpaperFallbackStatus {
    pub managed_active: bool,
    pub backup_exists: bool,
    pub warning: Option<String>,
}

#[cfg(windows)]
fn shared_root() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| "无法定位当前用户的本地应用数据目录。".to_string())?;
    Ok(base.join("DSHWallpaper"))
}

#[cfg(windows)]
fn state_path(root: &Path) -> PathBuf {
    root.join(STATE_FILE)
}

#[cfg(windows)]
fn managed_path(root: &Path) -> PathBuf {
    root.join(FALLBACK_DIRECTORY).join(FALLBACK_FILE)
}

#[cfg(windows)]
fn read_backup(root: &Path) -> Result<Option<DesktopWallpaperBackup>, String> {
    let path = state_path(root);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| format!("无法读取桌面底图恢复点：{error}"))?;
    let backup: DesktopWallpaperBackup = serde_json::from_slice(&bytes)
        .map_err(|error| format!("桌面底图恢复点损坏：{error}"))?;
    if backup.version != STATE_VERSION
        || backup.original_path.trim().is_empty()
        || backup.managed_path.trim().is_empty()
        || backup.original_path.contains('\0')
        || backup.managed_path.contains('\0')
    {
        return Err("桌面底图恢复点版本或路径无效；为避免覆盖用户壁纸，操作已停止。".into());
    }
    // The managed image is always the fixed file owned by this module. A
    // corrupted manifest must not be able to make a later disable operation
    // treat an arbitrary user path as our own wallpaper.
    if !same_path(&backup.managed_path, &managed_path(root)) {
        return Err("桌面底图恢复点未指向应用自己的托管文件；为避免覆盖用户壁纸，操作已停止。".into());
    }
    // Restoring to a path that no longer exists cannot succeed safely. Treat
    // it as invalid rather than allowing a partial recovery transaction.
    if !Path::new(&backup.original_path).is_file() {
        return Err("桌面底图恢复点指向的原壁纸已不存在；为避免覆盖，操作已停止。".into());
    }
    Ok(Some(backup))
}

#[cfg(windows)]
fn write_backup(root: &Path, backup: &DesktopWallpaperBackup) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| format!("无法创建桌面底图恢复目录：{error}"))?;
    let bytes = serde_json::to_vec_pretty(backup).map_err(|error| error.to_string())?;
    let temporary = root.join(format!(".{STATE_FILE}.{}.tmp", std::process::id()));
    let destination = state_path(root);
    let _ = fs::remove_file(&temporary);
    fs::write(&temporary, bytes).map_err(|error| format!("无法保存桌面底图恢复点：{error}"))?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法提交桌面底图恢复点：{error}"));
    }
    Ok(())
}

#[cfg(windows)]
fn current_wallpaper_path() -> Result<Option<String>, String> {
    // SPI_GETDESKWALLPAPER accepts a caller-provided buffer.  A long buffer
    // also handles extended paths without truncating the user's original.
    const BUFFER_LEN: usize = 32_768;
    let mut buffer = vec![0u16; BUFFER_LEN];
    unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            BUFFER_LEN as u32,
            Some(buffer.as_mut_ptr().cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .map_err(|error| format!("Windows 无法读取当前桌面壁纸：{error}"))?;
    }
    let length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    let value = String::from_utf16_lossy(&buffer[..length]).trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

#[cfg(windows)]
fn normalized_path(value: &str) -> String {
    let path = Path::new(value);
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn same_path(left: &str, right: &Path) -> bool {
    normalized_path(left) == normalized_path(&right.to_string_lossy())
}

#[cfg(windows)]
fn source_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            // Tauri bundles the web assets below `_up_/public`; the isolated
            // MSIX test layout also keeps a copy directly under `dist` and a
            // WinRT-friendly copy under `Assets`.
            candidates.push(parent.join("_up_").join("public").join(SLEEP_ASSET));
            candidates.push(parent.join("dist").join(SLEEP_ASSET));
            candidates.push(parent.join("Assets").join(SLEEP_PACKAGE_ASSET));
        }
    }
    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join("public").join(SLEEP_ASSET));
    }
    if let Some(root) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        candidates.push(root.join("public").join(SLEEP_ASSET));
    }
    candidates
}

/// Resolve only the packaged, immutable sleep artwork. No path supplied by a
/// renderer is accepted, so enabling the fallback cannot be redirected to an
/// arbitrary user file.
#[cfg(windows)]
pub fn bundled_sleep_source() -> Result<PathBuf, String> {
    source_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "未找到内置睡眠底图；请重新安装 DSH Wallpaper Lite。".into())
}

#[cfg(not(windows))]
pub fn bundled_sleep_source() -> Result<PathBuf, String> {
    Err("登录过渡底图仅支持 Windows。".into())
}

#[cfg(windows)]
fn set_wallpaper(path: &Path) -> Result<(), String> {
    let wide = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
        .map_err(|error| format!("Windows 无法设置登录过渡底图：{error}"))
    }
}

#[cfg(windows)]
fn copy_fallback(source: &Path, root: &Path) -> Result<PathBuf, String> {
    if !source.is_file() {
        return Err("找不到内置睡眠底图，未修改系统桌面壁纸。".into());
    }
    let directory = root.join(FALLBACK_DIRECTORY);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建登录过渡底图目录：{error}"))?;
    let destination = managed_path(root);
    let temporary = directory.join(format!(".sleep-{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temporary);
    fs::copy(source, &temporary).map_err(|error| format!("无法准备登录过渡底图：{error}"))?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法提交登录过渡底图：{error}"));
    }
    Ok(destination)
}

/// Enable/disable the Explorer fallback. `source` is resolved by the native
/// caller to a bundled, immutable sleep image when enabling; disabling does
/// not depend on the package still containing that source file, so recovery
/// remains possible after a partial update or damaged resource bundle.
#[cfg(windows)]
pub fn set_fallback(source: Option<&Path>, enabled: bool) -> Result<String, String> {
    let root = shared_root()?;
    let existing = read_backup(&root)?;
    if enabled {
        let source = source.ok_or_else(|| "缺少内置睡眠底图，未启用登录过渡底图。".to_string())?;
        let current = current_wallpaper_path()?.ok_or_else(|| {
            "当前桌面壁纸由幻灯片/动态来源管理，无法安全备份；未启用登录过渡底图。".to_string()
        })?;
        if let Some(backup) = &existing {
            let managed = Path::new(&backup.managed_path);
            if !same_path(&current, managed) {
                return Err("检测到桌面壁纸已由用户或其他程序更改；为避免覆盖，未启用登录过渡底图。".into());
            }
            if managed.is_file() {
                return Ok("登录过渡底图已经启用。".into());
            }
        } else if !Path::new(&current).is_file() {
            return Err("当前桌面壁纸文件不可读取，无法建立安全恢复点；未启用登录过渡底图。".into());
        }

        let managed = copy_fallback(source, &root)?;
        let backup = existing.unwrap_or(DesktopWallpaperBackup {
            version: STATE_VERSION,
            original_path: current,
            managed_path: managed.to_string_lossy().into_owned(),
        });
        // Persist the original before asking Windows to change the wallpaper.
        // If the setter becomes indeterminate, retaining this record is safer
        // than pretending the user's only recovery path was never captured.
        write_backup(&root, &backup)?;
        set_wallpaper(&managed)?;
        Ok("已启用登录过渡底图；应用启动时 Explorer 会先显示睡眠画面。".into())
    } else {
        let Some(backup) = existing else {
            return Ok("登录过渡底图当前未启用。".into());
        };
        let current = current_wallpaper_path()?.ok_or_else(|| {
            "无法确认当前桌面壁纸；为避免覆盖用户选择，未执行恢复。".to_string()
        })?;
        let managed = Path::new(&backup.managed_path);
        if !same_path(&current, managed) {
            return Err("检测到桌面壁纸已由用户或其他程序更改；未覆盖当前壁纸，恢复点仍保留。".into());
        }
        let original = Path::new(&backup.original_path);
        if !original.is_file() {
            return Err("原桌面壁纸文件已不存在；未删除恢复点，也未覆盖当前壁纸。".into());
        }
        set_wallpaper(original)?;
        fs::remove_file(state_path(&root))
            .map_err(|error| format!("已恢复桌面壁纸，但无法清理恢复点：{error}"))?;
        Ok("已关闭登录过渡底图并恢复原桌面壁纸。".into())
    }
}

/// Inspect ownership without changing the user's desktop. A stale recovery
/// point is intentionally retained so a later explicit recovery flow can
/// decide what to do; this function only reports it.
#[cfg(windows)]
pub fn status() -> DesktopWallpaperFallbackStatus {
    let root = match shared_root() {
        Ok(root) => root,
        Err(error) => {
            return DesktopWallpaperFallbackStatus {
                warning: Some(error),
                ..Default::default()
            }
        }
    };
    let backup = match read_backup(&root) {
        Ok(value) => value,
        Err(error) => {
            return DesktopWallpaperFallbackStatus {
                backup_exists: state_path(&root).is_file(),
                warning: Some(error),
                ..Default::default()
            }
        }
    };
    let Some(backup) = backup else {
        return DesktopWallpaperFallbackStatus::default();
    };
    let managed_active = current_wallpaper_path()
        .ok()
        .flatten()
        .is_some_and(|current| same_path(&current, Path::new(&backup.managed_path)));
    DesktopWallpaperFallbackStatus {
        managed_active,
        backup_exists: true,
        warning: (!managed_active).then(|| {
            "恢复点存在，但当前桌面壁纸已不是应用底图；为避免覆盖用户选择，暂不接管。".into()
        }),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn backup(_root: &Path, original: &Path, managed: &Path) -> DesktopWallpaperBackup {
        DesktopWallpaperBackup {
            version: STATE_VERSION,
            original_path: original.to_string_lossy().into_owned(),
            managed_path: managed.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn rejects_a_manifest_that_points_managed_file_elsewhere() {
        let directory = tempdir().expect("temp directory");
        let original = directory.path().join("original.jpg");
        let foreign = directory.path().join("foreign.png");
        fs::write(&original, b"original").expect("original");
        fs::write(&foreign, b"foreign").expect("foreign");
        let state = backup(directory.path(), &original, &foreign);
        write_backup(directory.path(), &state).expect("write state");
        let error = read_backup(directory.path()).expect_err("foreign managed path");
        assert!(error.contains("托管文件"));
    }

    #[test]
    fn accepts_only_the_fixed_managed_path() {
        let directory = tempdir().expect("temp directory");
        let original = directory.path().join("original.jpg");
        let managed = managed_path(directory.path());
        fs::write(&original, b"original").expect("original");
        fs::create_dir_all(managed.parent().expect("managed parent")).expect("managed dir");
        fs::write(&managed, b"managed").expect("managed");
        let state = backup(directory.path(), &original, &managed);
        write_backup(directory.path(), &state).expect("write state");
        assert!(matches!(read_backup(directory.path()), Ok(Some(_))));
    }
}

#[cfg(not(windows))]
pub fn status() -> DesktopWallpaperFallbackStatus {
    DesktopWallpaperFallbackStatus {
        warning: Some("登录过渡底图仅支持 Windows。".into()),
        ..Default::default()
    }
}

#[cfg(not(windows))]
pub fn set_fallback(_: Option<&Path>, _: bool) -> Result<String, String> {
    Err("登录过渡底图仅支持 Windows。".into())
}
