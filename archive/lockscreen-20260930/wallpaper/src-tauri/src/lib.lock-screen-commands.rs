// Archived by B3 of the freeze isolation (refactor/freeze-isolation).
// Source: wallpaper/src-tauri/src/lib.rs at commit 3c92772 (tag `pre-freeze-isolation`).
// These are the lock-screen (FREEZE 1A) Tauri commands that were commented out in
// lib.rs, restored here as ordinary Rust (the `// ` prefix removed). The file is not
// part of any crate module tree; unresolved names in an IDE are expected.
// How to wire them back: archive/lockscreen-20260930/README.md, section B3.

// ---- lib.rs 1973-2003 (3c92772)
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
#[tauri::command]
async fn set_lock_screen_enabled(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<String, String> {
    require_settings(&caller)?;
    windows_integration::set_lock_screen(&app, enabled).await
}

#[tauri::command]
async fn clear_stale_lock_screen_backup(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    confirmed: bool,
) -> Result<String, String> {
    require_settings(&caller)?;
    windows_integration::clear_stale_lock_screen_backup(&app, confirmed).await
}

#[tauri::command]
async fn get_lock_screen_diagnostics(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<windows_integration::LockScreenDiagnostics, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || windows_integration::lock_screen_diagnostics(&app))
        .await
        .map_err(|error| format!("读取锁屏诊断未完成：{error}"))?
}

// ---- lib.rs 2163-2187 (3c92772)
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
/// Opens the system-owned lock-screen settings page. During the current
/// MSIX-only test phase Windows accepts the package's bundled sleep image but
/// may reject a user-image restore snapshot; delegating the choice to Windows
/// is clearer and safer than pretending a restore has completed.
#[tauri::command]
fn open_windows_lock_screen_settings(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_settings(&caller)?;
    #[cfg(windows)]
    {
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Process",
                "ms-settings:lockscreen",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 Windows 锁屏设置：{error}"))
    }
    #[cfg(not(windows))]
    Err("锁屏设置仅支持 Windows。".into())
}

// ---- Wiring (not standalone code; listed verbatim from the original file)
//
// lib.rs 1755-1756: Lite allowed settings key, inside `lite_settings_save`
// (between "skipWakeAnimation" and "desktopWallpaperFallback"):
//   // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//   //         "lockScreenEnabled",
//
// lib.rs 4895-4898, 4904-4905: lite `generate_handler!` entries
// (the first group after `lite_image_resolve`/fallback lines, the second after the TranslucentTB lines):
//   // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//   //             set_lock_screen_enabled,
//   //             clear_stale_lock_screen_backup,
//   //             get_lock_screen_diagnostics,
//   // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//   //             open_windows_lock_screen_settings,
//
// lib.rs 4926-4929, 4946-4947: full `generate_handler!` entries
// (the first group after `notify_appearance_changed`, the second before `save_api_key`):
//   // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//   //             set_lock_screen_enabled,
//   //             clear_stale_lock_screen_backup,
//   //             get_lock_screen_diagnostics,
//   // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//   //             open_windows_lock_screen_settings,
//
// build.rs (`tauri_build::AppManifest::new().commands`), original lines 21-23 and 47:
//   "set_lock_screen_enabled",
//   "clear_stale_lock_screen_backup",
//   "get_lock_screen_diagnostics",
//   "open_windows_lock_screen_settings",
