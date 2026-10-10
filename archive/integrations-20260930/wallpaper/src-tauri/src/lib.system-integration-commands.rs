// Archived by B3 of the freeze isolation (refactor/freeze-isolation).
// Source: wallpaper/src-tauri/src/lib.rs at commit 3c92772 (tag `pre-freeze-isolation`).
// These are the system-integration (FREEZE 1B) items that were commented out in
// lib.rs: the login-transition desktop wallpaper fallback commands and the
// TranslucentTB commands, restored here as ordinary Rust (the `// ` prefix removed).
// The file is not part of any crate module tree; unresolved names in an IDE are expected.
// How to wire them back: archive/integrations-20260930/README.md, section B3.

// ---- lib.rs 23-24 (3c92772): module declaration
// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定，见 docs/plans/release-scope-cleanup-plan.md）。恢复办法：取消注释。
mod desktop_fallback;

// ---- lib.rs 1904-1931 (3c92772)
// The `///` block is the doc comment of `set_desktop_wallpaper_fallback`; in lib.rs it
// had been left live above the commented command and so attached to `mod lite_asset_tests`.
/// The Lite release can optionally align Explorer's ordinary desktop
/// wallpaper with the packaged sleep artwork. This masks the short interval
/// before WorkerW/WebView2 paints after login. The source is resolved inside
/// Rust and the renderer can only choose the boolean setting.
// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定，见 docs/plans/release-scope-cleanup-plan.md）。恢复办法：取消注释。
#[tauri::command]
#[cfg(feature = "lite")]
fn set_desktop_wallpaper_fallback(
    caller: tauri::WebviewWindow,
    enabled: bool,
) -> Result<String, String> {
    require_settings(&caller)?;
    if enabled {
        let source = desktop_fallback::bundled_sleep_source()?;
        desktop_fallback::set_fallback(Some(&source), true)
    } else {
        desktop_fallback::set_fallback(None, false)
    }
}

#[tauri::command]
#[cfg(feature = "lite")]
fn desktop_wallpaper_fallback_status(
    caller: tauri::WebviewWindow,
) -> Result<desktop_fallback::DesktopWallpaperFallbackStatus, String> {
    require_settings(&caller)?;
    Ok(desktop_fallback::status())
}

// ---- lib.rs 2034-2161 (3c92772)
// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定，见 docs/plans/release-scope-cleanup-plan.md）。恢复办法：取消注释。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslucentTbStatus {
    installed: bool,
    running: bool,
    source: Option<String>,
}

#[cfg(windows)]
fn hide_child_console(command: &mut std::process::Command) {
    std::os::windows::process::CommandExt::creation_flags(command, 0x08000000);
}

fn translucent_tb_status_blocking() -> Result<TranslucentTbStatus, String> {
    #[cfg(windows)]
    {
        let mut tasklist = std::process::Command::new("tasklist");
        tasklist.args(["/FI", "IMAGENAME eq TranslucentTB.exe", "/FO", "CSV", "/NH"]);
        hide_child_console(&mut tasklist);
        let running = tasklist.output().ok().is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("translucenttb.exe")
        });

        let mut where_command = std::process::Command::new("where.exe");
        where_command.arg("ttb.exe");
        hide_child_console(&mut where_command);
        let alias = where_command
            .output()
            .ok()
            .is_some_and(|output| output.status.success());

        // AppX discovery is occasionally slow on a busy Windows session, so
        // this whole probe runs on a blocking worker and the child console is
        // explicitly suppressed. The settings WebView remains responsive.
        let mut packaged_command = std::process::Command::new("powershell.exe");
        packaged_command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "if (Get-AppxPackage -Name TranslucentTB -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }",
        ]);
        hide_child_console(&mut packaged_command);
        let packaged = packaged_command
            .status()
            .ok()
            .is_some_and(|status| status.success());

        return Ok(TranslucentTbStatus {
            installed: alias || packaged,
            running,
            source: if alias {
                Some("execution-alias".into())
            } else if packaged {
                Some("msix".into())
            } else {
                None
            },
        });
    }

    #[cfg(not(windows))]
    Ok(TranslucentTbStatus {
        installed: false,
        running: false,
        source: None,
    })
}

#[tauri::command]
async fn translucent_tb_status(
    caller: tauri::WebviewWindow,
) -> Result<TranslucentTbStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(translucent_tb_status_blocking)
        .await
        .map_err(|error| format!("读取透明任务栏状态未完成：{error}"))?
}

#[tauri::command]
fn launch_translucent_tb(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_settings(&caller)?;
    std::process::Command::new("ttb.exe")
        .spawn()
        .map(|_| ())
        .map_err(|_| {
            "未找到 TranslucentTB。请先从 Microsoft Store 安装并启用 ttb.exe 执行别名。".into()
        })
}

#[tauri::command]
fn open_translucent_tb_install(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_settings(&caller)?;
    // `explorer.exe <uri>` may treat the Store URI as a filesystem path and
    // open Documents instead. Ask ShellExecute to resolve the URI protocol.
    #[cfg(windows)]
    {
        let store_uri = "ms-windows-store://pdp/?ProductId=9PF4KZ2VN4W9";
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Process",
                store_uri,
            ])
            .status()
            .map_err(|error| format!("无法启动 Microsoft Store：{error}"))?;
        if status.success() {
            return Ok(());
        }

        // A Store-disabled Windows installation still gets a useful route.
        std::process::Command::new("rundll32.exe")
            .args([
                "url.dll,FileProtocolHandler",
                "https://apps.microsoft.com/detail/9PF4KZ2VN4W9",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 TranslucentTB 下载页：{error}"))
    }
    #[cfg(not(windows))]
    Err("TranslucentTB 仅支持 Windows。".into())
}

// ---- Wiring (not standalone code; listed verbatim from the original file)
//
// lib.rs 4893-4894, 4901-4903: lite `generate_handler!` entries
// (the first pair after `lite_image_resolve`, the TranslucentTB group after `autostart_status`):
//               // set_desktop_wallpaper_fallback,
//               // desktop_wallpaper_fallback_status,
//               // translucent_tb_status,
//               // launch_translucent_tb,
//               // open_translucent_tb_install,
//
// lib.rs 4943-4945: full `generate_handler!` entries (after `stop_managed_dsh`;
// the fallback commands are `#[cfg(feature = "lite")]` and were never in the full handler):
//               // translucent_tb_status,
//               // launch_translucent_tb,
//               // open_translucent_tb_install,
//
// build.rs (`tauri_build::AppManifest::new().commands`), original lines 18-19 and 44-46:
//   "set_desktop_wallpaper_fallback",
//   "desktop_wallpaper_fallback_status",
//   "translucent_tb_status",
//   "launch_translucent_tb",
//   "open_translucent_tb_install",
//
// Kept live in lib.rs on purpose: the Lite allowed settings key "desktopWallpaperFallback"
// (persisted-settings compatibility).
