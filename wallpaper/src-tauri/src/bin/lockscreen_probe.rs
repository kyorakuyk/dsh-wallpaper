//! Deliberately tiny packaged lock-screen probe.
//!
//! This executable does not create a Tauri runtime, WebView, tray icon, or
//! touch dsh-wallpaper's lock-screen recovery manifest.  Its only system
//! mutation is the single documented WinRT setter at the end of `main`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use std::{collections::BTreeMap, fs, path::PathBuf};
#[cfg(windows)]
use windows::{
    core::HSTRING,
    Storage::StorageFile,
    System::UserProfile::{LockScreen, UserProfilePersonalizationSettings},
    Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED},
};

#[cfg(windows)]
fn output_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DSHWallpaperLockScreenProbe")
}

#[cfg(windows)]
fn local_path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let value = uri.strip_prefix("file:///")?;
    let decoded = urlencoding::decode(value).ok()?;
    Some(PathBuf::from(decoded.replace('/', "\\")))
}

#[cfg(windows)]
fn main() {
    // The report is the test's output contract.  It is intentionally written
    // even for partial failures so the caller does not need to infer what the
    // packaged process reached.
    let mut report = BTreeMap::<String, serde_json::Value>::new();
    report.insert("probeVersion".into(), serde_json::json!(1));
    report.insert("packagedProcess".into(), serde_json::json!(true));

    let report_dir = output_dir();
    let _ = fs::create_dir_all(&report_dir);
    let report_path = report_dir.join("report.json");

    let result = (|| -> Result<(), String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| format!("CoInitializeEx: {error}"))?;
        let supported = UserProfilePersonalizationSettings::IsSupported()
            .map_err(|error| format!("IsSupported: {error}"))?;
        report.insert("isSupported".into(), serde_json::json!(supported));
        if !supported {
            return Err("Windows reports UserProfilePersonalizationSettings unsupported".into());
        }

        let original_uri = LockScreen::OriginalImageFile()
            .map_err(|error| format!("OriginalImageFile: {error}"))?
            .AbsoluteUri()
            .map_err(|error| format!("OriginalImageFile.AbsoluteUri: {error}"))?
            .to_string();
        report.insert("originalImageUri".into(), serde_json::json!(original_uri));

        // Preserve a separate, probe-owned copy before the only mutation.
        let backup_path = report_dir.join("original-lockscreen-backup");
        let backup_result = local_path_from_file_uri(&original_uri)
            .filter(|path| path.is_file())
            .map(|source| fs::copy(source, &backup_path));
        report.insert(
            "backup".into(),
            serde_json::json!({
                "path": backup_path,
                "copied": backup_result.as_ref().is_some_and(|result| result.is_ok()),
            }),
        );

        let executable_dir = std::env::current_exe()
            .map_err(|error| format!("current_exe: {error}"))?
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| "current_exe has no parent directory".to_string())?;
        let bundled_image = executable_dir.join("Assets").join("sleep.png");
        if !bundled_image.is_file() {
            return Err("bundled probe image is missing".into());
        }
        // A/B test #3: use the user's Pictures directory. It is the standard
        // user-owned photo location Windows exposes for personalization, in
        // contrast to the ordinary AppData location rejected by test #2.
        // A unique filename avoids replacing an image the primary app owns.
        let pictures = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(output_dir)
            .join("Pictures")
            .join("DSHWallpaperLockScreenProbe");
        fs::create_dir_all(&pictures)
            .map_err(|error| format!("create probe Pictures directory: {error}"))?;
        let image = pictures.join(format!("lockscreen-probe-pictures-{}.png", std::process::id()));
        fs::copy(&bundled_image, &image)
            .map_err(|error| format!("copy probe image to Pictures: {error}"))?;
        report.insert("probeMode".into(), serde_json::json!("pictures-copy"));
        report.insert("bundledImagePath".into(), serde_json::json!(bundled_image));
        report.insert("probeImagePath".into(), serde_json::json!(image));
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(image.to_string_lossy().as_ref()))
            .map_err(|error| format!("GetFileFromPathAsync: {error}"))?
            .get()
            .map_err(|error| format!("GetFileFromPathAsync.get: {error}"))?;
        report.insert("storageFileOpened".into(), serde_json::json!(true));

        let settings = UserProfilePersonalizationSettings::Current()
            .map_err(|error| format!("Current: {error}"))?;
        let changed = settings
            .TrySetLockScreenImageAsync(&file)
            .map_err(|error| format!("TrySetLockScreenImageAsync: {error}"))?
            .get()
            .map_err(|error| format!("TrySetLockScreenImageAsync.get: {error}"))?;
        report.insert("trySetReturned".into(), serde_json::json!(changed));
        Ok(())
    })();

    if let Err(error) = result {
        report.insert("error".into(), serde_json::json!(error));
    }
    let _ = fs::write(
        report_path,
        serde_json::to_vec_pretty(&report).unwrap_or_else(|_| b"{\"error\":\"report serialization failed\"}".to_vec()),
    );
}

#[cfg(not(windows))]
fn main() {}
