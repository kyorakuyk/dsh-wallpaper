mod api_persistence;
mod app_core;
mod appearance;
mod chat;
mod lock_screen_backup;
mod windows_integration;

use app_core::{Activity, AppAction, AppCore, AppSnapshot, BackendMode, HarnessAvailability};
use std::sync::{OnceLock, RwLock};
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, EventTarget, Manager};

/// Only the settings surface is allowed to request the API-key prompt. This
/// is a defense in depth check: it prevents the desktop wallpaper WebView (or
/// a future WebView) from opening a native credential capture dialog.
const SETTINGS_WINDOW_LABEL: &str = "settings";

/// The WorkerW-backed wallpaper WebView is the only surface that may send,
/// cancel, or read chat conversations. Capabilities provide the primary ACL;
/// this guard makes that boundary survive an accidental future capability
/// change or a newly added WebView.
const BACKGROUND_WINDOW_LABEL: &str = "background";

/// Settings are always authored by the dedicated settings surface and then
/// delivered to the wallpaper by Rust.  This prevents a renderer from
/// selecting an arbitrary event target through `core:event:emit_to`.
const SETTINGS_CHANGED_EVENT: &str = "settings-changed";
const APPEARANCE_CHANGED_EVENT: &str = "appearance-changed";

fn require_background(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == BACKGROUND_WINDOW_LABEL {
        Ok(())
    } else {
        Err("该命令只允许壁纸宿主调用。".into())
    }
}

fn require_settings(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == SETTINGS_WINDOW_LABEL {
        Ok(())
    } else {
        Err("该命令只允许设置中心调用。".into())
    }
}

/// These two commands expose no conversation body or system-setting write:
/// the settings center needs a current runtime snapshot, and the wallpaper's
/// experimental DeepSeek entry needs to launch one fixed public URL.  Keep
/// even that small shared surface restricted to the two declared WebViews.
fn require_wallpaper_surface(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        BACKGROUND_WINDOW_LABEL | SETTINGS_WINDOW_LABEL => Ok(()),
        _ => Err("该命令只允许壁纸或设置中心调用。".into()),
    }
}

/// `keyring`'s Windows backend stores string secrets in a Generic Credential
/// as a UTF-16 little-endian byte blob. CredUI returns UTF-16 code units, so
/// encode them explicitly rather than relying on the host representation when
/// passing a raw `CredentialBlob` to `CredWriteW`.
#[cfg(windows)]
fn credential_blob_from_prompt_password(password: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(password.len().saturating_mul(std::mem::size_of::<u16>()));
    for code_unit in password {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

/// Do not use ordinary slice assignment for a secret: the optimizer is
/// permitted to remove a write whose result is never read. Volatile writes
/// make the cleanup observable to the machine, without adding a plaintext
/// dependency or copying the key through the WebView.
#[cfg(windows)]
fn secure_zero_u16(secret: &mut [u16]) {
    for value in secret {
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(windows)]
fn secure_zero_bytes(secret: &mut [u8]) {
    for value in secret {
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

fn emit_app_snapshot(app: &tauri::AppHandle, snapshot: &AppSnapshot) {
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        "app-snapshot",
        snapshot,
    );
}

fn dispatch_ui_action(app: &tauri::AppHandle, action: AppAction, _request_focus: bool) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    emit_app_snapshot(app, &snapshot);
}

fn dispatch_tray_ui_action(app: &tauri::AppHandle, action: AppAction) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    // Opening the tray menu temporarily makes Shell_TrayWnd the foreground
    // window. Treat an explicit tray command as user intent to reveal the
    // interaction surface; once focused, the normal foreground monitor takes over.
    core.dispatch(AppAction::DesktopForegroundChanged(true));
    let snapshot = core.dispatch(action);
    log::info!(
        "tray interaction request: phase={:?} visible={} settings={} desktop={}",
        snapshot.phase,
        snapshot.interaction.visible,
        snapshot.interaction.settings_open,
        snapshot.interaction.desktop_foreground
    );
    emit_app_snapshot(app, &snapshot);
}

fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("settings window missing")?;
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    // Tao/Windows can restore the active-window accent border as part of
    // SetForegroundWindow. Apply our DWM policy after activation so the CSS
    // outline remains the only visible settings frame.
    windows_integration::configure_settings_window(&window)
}

#[tauri::command]
fn get_app_snapshot(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, AppCore>,
) -> Result<AppSnapshot, String> {
    require_wallpaper_surface(&caller)?;
    Ok(state.snapshot())
}

#[tauri::command]
fn set_interaction_enabled(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    require_settings(&caller)?;
    let snapshot = state.dispatch(AppAction::SetInteractionEnabled(enabled));
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn select_backend(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    backend: String,
) -> Result<AppSnapshot, String> {
    require_background(&caller)?;
    let backend = match backend.as_str() {
        "deepseek-web" => BackendMode::DeepseekWeb,
        "deepseek-api" => BackendMode::DeepseekApi,
        "harness" => BackendMode::Harness,
        _ => return Err(format!("unknown backend: {backend}")),
    };
    let snapshot = state.dispatch(AppAction::SelectBackend(backend));
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn dispatch_app_action(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    action: String,
    play_wake: Option<bool>,
    value: Option<String>,
) -> Result<AppSnapshot, String> {
    require_background(&caller)?;
    let action = match action.as_str() {
        "boot-ready" => AppAction::BootReady {
            play_wake: play_wake.unwrap_or(true),
        },
        "lock" => AppAction::Lock,
        "unlock" => AppAction::Unlock {
            play_wake: play_wake.unwrap_or(true),
        },
        "wake-done" => AppAction::WakeDone,
        "open-chat" => AppAction::OpenChat,
        "close-chat" => AppAction::CloseChat,
        "open-settings" => AppAction::OpenSettings,
        "close-settings" => AppAction::CloseSettings,
        "toggle-history" => AppAction::ToggleHistory,
        "auth-required" => AppAction::AuthRequired,
        "auth-ready" => AppAction::AuthReady,
        "recover" => AppAction::Recover,
        "fail" => AppAction::Fail(value.unwrap_or_else(|| "未知错误".into())),
        "set-activity" => AppAction::SetActivity(match value.as_deref() {
            Some("idle") => Activity::Idle,
            Some("sending") => Activity::Sending,
            Some("thinking") => Activity::Thinking,
            Some("streaming") => Activity::Streaming,
            Some("tool") => Activity::Tool,
            Some("done") => Activity::Done,
            _ => return Err("invalid activity".into()),
        }),
        "set-harness" => AppAction::SetHarnessAvailability(match value.as_deref() {
            Some("offline") => HarnessAvailability::Offline,
            Some("web-only") => HarnessAvailability::WebOnly,
            Some("bridge-ready") => HarnessAvailability::BridgeReady,
            _ => return Err("invalid harness availability".into()),
        }),
        _ => return Err(format!("unknown app action: {action}")),
    };
    let snapshot = state.dispatch(action);
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn publish_settings(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    settings: serde_json::Value,
) -> Result<(), String> {
    require_settings(&caller)?;
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        SETTINGS_CHANGED_EVENT,
        settings,
    );
    Ok(())
}

#[tauri::command]
fn notify_appearance_changed(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_settings(&caller)?;
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        APPEARANCE_CHANGED_EVENT,
        (),
    );
    Ok(())
}

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
) -> Result<String, String> {
    require_settings(&caller)?;
    windows_integration::clear_stale_lock_screen_backup(&app).await
}

#[tauri::command]
fn get_lock_screen_diagnostics(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<windows_integration::LockScreenDiagnostics, String> {
    require_settings(&caller)?;
    windows_integration::lock_screen_diagnostics(&app)
}

#[tauri::command]
fn set_autostart(caller: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    require_settings(&caller)?;
    #[cfg(windows)]
    {
        use std::process::Command;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // 注册表 Run 键：开机自启 dsh-wallpaper
        //  add:    reg add HKCU\...\Run /V dsh-wallpaper /D "<exe>" /F
        //  delete: reg delete HKCU\...\Run /V dsh-wallpaper /F
        let mut cmd = Command::new("reg");
        if enabled {
            cmd.args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "dsh-wallpaper",
                "/D",
                &exe.to_string_lossy(),
                "/F",
            ]);
        } else {
            cmd.args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "dsh-wallpaper",
                "/F",
            ]);
        }
        let status = cmd.status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("更新当前用户开机自启失败".into());
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslucentTbStatus {
    installed: bool,
    running: bool,
    source: Option<String>,
}

#[tauri::command]
fn translucent_tb_status(caller: tauri::WebviewWindow) -> Result<TranslucentTbStatus, String> {
    require_settings(&caller)?;
    #[cfg(windows)]
    {
        let running = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq TranslucentTB.exe", "/FO", "CSV", "/NH"])
            .output()
            .ok()
            .is_some_and(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains("translucenttb.exe")
            });
        let alias = std::process::Command::new("where.exe")
            .arg("ttb.exe")
            .output()
            .ok()
            .is_some_and(|output| output.status.success());
        let packaged = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "if (Get-AppxPackage -Name TranslucentTB -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }"])
            .status().ok().is_some_and(|status| status.success());
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

/// Opens the Windows-owned credential prompt and stores the API key without
/// transporting plaintext through Tauri IPC or the renderer. The generic
/// CredUI target deliberately matches the `keyring` crate's default Windows
/// target naming, so the existing API client reads the same credential.
#[tauri::command]
fn prompt_for_api_key(caller: tauri::WebviewWindow) -> Result<bool, String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以更新 API Key。".into());
    }

    #[cfg(windows)]
    {
        use windows::{
            core::{HSTRING, PCWSTR, PWSTR},
            Win32::{
                Foundation::{ERROR_CANCELLED, FILETIME},
                Security::Credentials::{
                    CredUIPromptForCredentialsW, CredWriteW, CREDENTIALW,
                    CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST,
                    CREDUI_FLAGS_GENERIC_CREDENTIALS, CREDUI_FLAGS_PASSWORD_ONLY_OK, CREDUI_INFOW,
                    CREDUI_MAX_USERNAME_LENGTH, CRED_FLAGS, CRED_PERSIST_ENTERPRISE,
                    CRED_TYPE_GENERIC,
                },
            },
        };

        // The Windows SDK caps CredUI password input at 256 UTF-16 code units
        // plus a terminator.  DeepSeek keys are far shorter, while this avoids
        // an unbounded native buffer when a malformed value is supplied.
        const API_KEY_BUFFER_LEN: usize = 257;
        const CREDENTIAL_TARGET: &str = "deepseek-api.dsh-wallpaper";
        const CREDENTIAL_USERNAME: &str = "deepseek-api";

        let caption = HSTRING::from("更新 DeepSeek API Key");
        let message =
            HSTRING::from("请输入 DeepSeek API Key。该密钥仅保存到当前 Windows 用户的凭据管理器。");
        let target = HSTRING::from(CREDENTIAL_TARGET);
        let username = HSTRING::from(CREDENTIAL_USERNAME);
        let mut password = [0u16; API_KEY_BUFFER_LEN];
        // Password-only mode must not surface a username control, but CredUI
        // still requires a writable username buffer. Give it the documented
        // maximum rather than risking ERROR_INSUFFICIENT_BUFFER on a Windows
        // implementation that fills a default account name internally.
        let mut credential_username = [0u16; CREDUI_MAX_USERNAME_LENGTH as usize + 1];
        let parent = caller
            .hwnd()
            .map_err(|error| format!("无法关联 Windows 凭据对话框：{error}"))?;
        let ui_info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            hwndParent: parent,
            pszMessageText: PCWSTR(message.as_ptr()),
            pszCaptionText: PCWSTR(caption.as_ptr()),
            hbmBanner: Default::default(),
        };
        let flags = CREDUI_FLAGS_GENERIC_CREDENTIALS
            | CREDUI_FLAGS_ALWAYS_SHOW_UI
            | CREDUI_FLAGS_PASSWORD_ONLY_OK
            // We deliberately make CredUI return the password to this native
            // function, then write the exact Generic Credential target below.
            // Microsoft documents that this is the supported path for
            // inspecting a returned password; it is wiped immediately after
            // CredWriteW and never enters WebView IPC.
            | CREDUI_FLAGS_DO_NOT_PERSIST;
        let result = unsafe {
            CredUIPromptForCredentialsW(
                Some(&ui_info),
                &target,
                None,
                0,
                &mut credential_username,
                &mut password,
                None,
                flags,
            )
        };

        if result == ERROR_CANCELLED {
            secure_zero_u16(&mut password);
            secure_zero_u16(&mut credential_username);
            return Ok(false);
        }
        if result.0 != 0 {
            secure_zero_u16(&mut password);
            secure_zero_u16(&mut credential_username);
            return Err(format!("Windows 凭据输入失败（错误代码 {}）。", result.0));
        }

        let save_result = (|| -> Result<(), String> {
            let password_len = password
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(password.len());
            if password_len == 0 {
                return Err("API Key 不能为空。".into());
            }
            // Match `keyring`'s Windows Generic Credential representation:
            // its `set_password` serializes password UTF-16 code units as a
            // little-endian blob and `get_password` reverses that encoding.
            let mut password_blob = credential_blob_from_prompt_password(&password[..password_len]);
            let byte_len = u32::try_from(password_blob.len()).map_err(|_| "API Key 长度无效。")?;
            let mut credential = CREDENTIALW {
                Flags: CRED_FLAGS::default(),
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target.as_ptr() as *mut u16),
                Comment: PWSTR::null(),
                LastWritten: FILETIME::default(),
                CredentialBlobSize: byte_len,
                CredentialBlob: password_blob.as_mut_ptr(),
                Persist: CRED_PERSIST_ENTERPRISE,
                AttributeCount: 0,
                Attributes: std::ptr::null_mut(),
                TargetAlias: PWSTR::null(),
                UserName: PWSTR(username.as_ptr() as *mut u16),
            };
            // `keyring::Entry::new("dsh-wallpaper", "deepseek-api")`
            // resolves to this target on Windows. Keep the credential write
            // native so the renderer never sees plaintext.
            let result = unsafe { CredWriteW(&mut credential, 0) }
                .map_err(|error| format!("无法保存 API Key 到 Windows 凭据管理器：{error}"));
            secure_zero_bytes(&mut password_blob);
            result
        })();
        secure_zero_u16(&mut password);
        secure_zero_u16(&mut credential_username);
        save_result?;
        Ok(true)
    }

    #[cfg(not(windows))]
    {
        let _ = caller;
        Err("API Key 原生凭据输入仅支持 Windows。".into())
    }
}

#[cfg(all(test, windows))]
mod api_credential_tests {
    use super::credential_blob_from_prompt_password;

    #[test]
    fn serializes_the_same_utf16_little_endian_shape_as_keyring_windows() {
        assert_eq!(
            credential_blob_from_prompt_password(&[0x0073, 0x006B, 0x4F60]),
            vec![0x73, 0x00, 0x6B, 0x00, 0x60, 0x4F]
        );
    }
}

#[tauri::command]
fn show_deepseek_login(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    // A remote WebView2 created inside the wallpaper process can block the Tao
    // event loop while Chromium initializes or the page hangs. Keep third-party
    // login isolated from the wallpaper host until the bridge owns a dedicated
    // helper process.
    let _ = app;
    std::process::Command::new("explorer.exe")
        .arg("https://chat.deepseek.com")
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_settings_drag(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_settings(&caller)?;
    app.get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_settings_window(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_settings(&caller)?;
    app.get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn begin_interaction_region_session(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    windows_integration::begin_interaction_region_session()
}

#[tauri::command]
fn update_interaction_regions(
    caller: tauri::WebviewWindow,
    regions: Vec<windows_integration::InteractionRegionInput>,
    scale_factor: f64,
    session: u64,
    revision: u64,
) -> Result<windows_integration::InteractionRegionUpdateResult, String> {
    require_background(&caller)?;
    windows_integration::update_interaction_regions(regions, scale_factor, session, revision)
}

#[tauri::command]
fn desktop_layout_metrics(caller: tauri::WebviewWindow) -> Result<windows_integration::DesktopLayoutMetrics, String> {
    require_background(&caller)?;
    Ok(windows_integration::desktop_layout_metrics(&caller))
}

#[tauri::command]
async fn send_chat(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    mode: String,
    text: String,
    conversation_id: Option<String>,
    request_id: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    price_input_per_million: Option<f64>,
    price_output_per_million: Option<f64>,
) -> Result<Option<String>, String> {
    require_background(&caller)?;
    match mode.as_str() {
        "deepseek-api" => chat::send_api(
            app,
            state,
            text,
            base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            model.unwrap_or_else(|| "deepseek-chat".into()),
            conversation_id,
            request_id,
            price_input_per_million,
            price_output_per_million,
        )
        .await
        .map(Some),
        "harness" => chat::harness_send(state, text).await.map(|_| None),
        _ => Err("DeepSeek 网页桥接需要登录 WebView 和 DOM adapter".into()),
    }
}

#[tauri::command]
fn api_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    conversation_id: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::api_history(state.inner(), &conversation_id)
}

#[tauri::command]
async fn cancel_chat(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    mode: String,
) -> Result<(), String> {
    require_background(&caller)?;
    if mode == "harness" {
        chat::harness_cancel(state).await
    } else {
        chat::cancel_api(&state);
        Ok(())
    }
}

#[tauri::command]
async fn connect_harness(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    resume_session_id: Option<String>,
    connection_id: String,
    model: Option<String>,
) -> Result<String, String> {
    require_background(&caller)?;
    chat::harness_connect(app, state, resume_session_id, connection_id, model).await
}

#[tauri::command]
async fn harness_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_history(state).await
}

static HARNESS_STATUS_CACHE: OnceLock<RwLock<serde_json::Value>> = OnceLock::new();

/// The wallpaper bridge exposes a small, versioned protocol of its own.  A
/// listening service on port 3080 is not sufficient proof that it is our
/// bridge: it may be DSH's regular web UI, an older bridge, or another local
/// service altogether.
const HARNESS_BRIDGE_PROTOCOL_VERSION: u64 = 1;
const REQUIRED_HARNESS_BRIDGE_CAPABILITIES: &[&str] =
    &["sessions", "history", "sse", "cancel", "approval-handoff"];

fn harness_status_cache() -> &'static RwLock<serde_json::Value> {
    HARNESS_STATUS_CACHE
        .get_or_init(|| RwLock::new(serde_json::json!({ "availability": "offline" })))
}

/// Convert a bridge status document into the small status shape exposed to
/// the WebView.  This is intentionally fail-closed: only a bridge that speaks
/// the fresh-session protocol we need can enable Harness mode. `resume` is an
/// optional DSH persistence feature and cannot be required for a new session.
fn compatible_harness_bridge_status(data: &serde_json::Value) -> Option<serde_json::Value> {
    let protocol_version = data
        .get("protocolVersion")
        .and_then(serde_json::Value::as_u64)?;
    if protocol_version != HARNESS_BRIDGE_PROTOCOL_VERSION {
        return None;
    }
    if data.get("dsh").and_then(serde_json::Value::as_str) != Some("online") {
        return None;
    }
    if data
        .get("authentication")
        .and_then(serde_json::Value::as_str)
        != Some("ready")
    {
        return None;
    }

    let capabilities = data
        .get("capabilities")
        .and_then(serde_json::Value::as_array)?;
    // A malformed capability list must not accidentally pass because it has a
    // few expected string values mixed with arbitrary JSON.
    if !capabilities.iter().all(serde_json::Value::is_string) {
        return None;
    }
    if !REQUIRED_HARNESS_BRIDGE_CAPABILITIES.iter().all(|required| {
        capabilities
            .iter()
            .any(|capability| capability.as_str() == Some(*required))
    }) {
        return None;
    }

    let mut status = serde_json::Map::new();
    status.insert(
        "availability".into(),
        serde_json::Value::String("bridge-ready".into()),
    );
    // These are informational only.  Do not let malformed optional metadata
    // make a compatible bridge unusable, or expose non-string JSON to the UI.
    for field in ["bridgeVersion", "model", "provider", "reasoningEffort"] {
        if let Some(value) = data
            .get(field)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            status.insert(field.into(), serde_json::Value::String(value.into()));
        }
    }
    Some(serde_json::Value::Object(status))
}

async fn fetch_harness_status() -> serde_json::Value {
    let client = match reqwest::Client::builder()
        // Status probing decides whether the UI exposes Harness mode. It must
        // be a direct loopback probe too: proxy configuration must not be able
        // to spoof a ready Bridge or route a future authenticated probe away
        // from the local DSH process.
        .no_proxy()
        .timeout(std::time::Duration::from_millis(1200))
        .build()
    {
        Ok(client) => client,
        Err(_) => return serde_json::json!({ "availability": "offline" }),
    };
    if let Ok(response) = client
        .get("http://127.0.0.1:3080/api/wallpaper/v1/status")
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(data) = response.json::<serde_json::Value>().await {
                if let Some(status) = compatible_harness_bridge_status(&data) {
                    return status;
                }
            }
        }
    }
    let root_status = client
        .get("http://127.0.0.1:3080/")
        .send()
        .await
        .ok()
        .map(|response| response.status());
    match root_probe_availability(root_status) {
        // A root endpoint proves only that an HTTP service accepted a
        // successful request. Authentication failures and error pages must
        // not turn an unrelated process on 3080 into a misleading “DSH
        // online” state.
        HarnessAvailability::WebOnly => serde_json::json!({ "availability": "web-only" }),
        HarnessAvailability::Offline | HarnessAvailability::BridgeReady => {
            serde_json::json!({ "availability": "offline" })
        }
    }
}

/// A root-page response is diagnostic only. It is deliberately not part of
/// the compatible Bridge validation above: any non-2xx response (including a
/// gateway, auth challenge, or unrelated service error) must be treated as
/// offline rather than an apparently usable local DSH instance.
fn root_probe_availability(status: Option<reqwest::StatusCode>) -> HarnessAvailability {
    match status {
        Some(status) if status.is_success() => HarnessAvailability::WebOnly,
        _ => HarnessAvailability::Offline,
    }
}

#[cfg(test)]
mod harness_status_tests {
    use super::{compatible_harness_bridge_status, root_probe_availability};
    use crate::app_core::HarnessAvailability;
    use serde_json::json;

    fn valid_status() -> serde_json::Value {
        json!({
            "bridgeVersion": "1.0.0",
            "protocolVersion": 1,
            "dsh": "online",
            "capabilities": [
                "sessions",
                "resume",
                "history",
                "sse",
                "cancel",
                "approval-handoff",
                "future-capability"
            ],
            "authentication": "ready",
            "provider": "deepseek",
            "model": "deepseek-chat",
            "reasoningEffort": "high"
        })
    }

    #[test]
    fn accepts_only_a_ready_compatible_bridge() {
        let status = compatible_harness_bridge_status(&valid_status()).expect("compatible status");
        assert_eq!(status["availability"], "bridge-ready");
        assert_eq!(status["bridgeVersion"], "1.0.0");
        assert_eq!(status["provider"], "deepseek");
        assert_eq!(status["model"], "deepseek-chat");
        assert_eq!(status["reasoningEffort"], "high");
    }

    #[test]
    fn accepts_a_bridge_without_optional_session_persistence() {
        let mut document = valid_status();
        let capabilities = document["capabilities"]
            .as_array_mut()
            .expect("capabilities");
        capabilities.retain(|capability| capability.as_str() != Some("resume"));
        assert!(compatible_harness_bridge_status(&document).is_some());
    }

    #[test]
    fn rejects_incompatible_or_unready_status_documents() {
        let invalid_statuses = [
            json!({}),
            json!({
                "protocolVersion": 2,
                "dsh": "online",
                "capabilities": ["sessions", "resume", "history", "sse", "cancel", "approval-handoff"],
                "authentication": "ready"
            }),
            json!({
                "protocolVersion": "1",
                "dsh": "online",
                "capabilities": ["sessions", "resume", "history", "sse", "cancel", "approval-handoff"],
                "authentication": "ready"
            }),
            json!({
                "protocolVersion": 1,
                "dsh": "offline",
                "capabilities": ["sessions", "resume", "history", "sse", "cancel", "approval-handoff"],
                "authentication": "ready"
            }),
            json!({
                "protocolVersion": 1,
                "dsh": "online",
                "capabilities": ["sessions", "resume", "history", "sse", "cancel"],
                "authentication": "ready"
            }),
            json!({
                "protocolVersion": 1,
                "dsh": "online",
                "capabilities": ["sessions", "resume", "history", "sse", "cancel", "approval-handoff", 3],
                "authentication": "ready"
            }),
            json!({
                "protocolVersion": 1,
                "dsh": "online",
                "capabilities": ["sessions", "resume", "history", "sse", "cancel", "approval-handoff"],
                "authentication": "unavailable"
            }),
        ];

        for document in invalid_statuses {
            assert!(
                compatible_harness_bridge_status(&document).is_none(),
                "unexpected compatible status: {document}"
            );
        }
    }

    #[test]
    fn ignores_malformed_optional_metadata() {
        let mut document = valid_status();
        document["model"] = json!({ "unexpected": true });
        document["provider"] = json!(42);
        document["reasoningEffort"] = json!("");
        let status = compatible_harness_bridge_status(&document).expect("compatible status");
        assert!(status.get("model").is_none());
        assert!(status.get("provider").is_none());
        assert!(status.get("reasoningEffort").is_none());
    }

    #[test]
    fn root_probe_requires_an_inspectable_success_status() {
        assert_eq!(
            root_probe_availability(Some(reqwest::StatusCode::OK)),
            HarnessAvailability::WebOnly
        );
        assert_eq!(
            root_probe_availability(Some(reqwest::StatusCode::NO_CONTENT)),
            HarnessAvailability::WebOnly
        );
        for status in [
            None,
            Some(reqwest::StatusCode::UNAUTHORIZED),
            Some(reqwest::StatusCode::NOT_FOUND),
            Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
        ] {
            assert_eq!(
                root_probe_availability(status),
                HarnessAvailability::Offline
            );
        }
    }
}

#[tauri::command]
fn probe_harness(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    Ok(harness_status_cache()
        .read()
        .map(|status| status.clone())
        .unwrap_or_else(|_| serde_json::json!({ "availability": "offline" })))
}

fn start_harness_monitor(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut consecutive_successes = 0u8;
        let mut consecutive_failures = 0u8;
        let mut last = HarnessAvailability::Offline;
        loop {
            let status = fetch_harness_status().await;
            if let Ok(mut cache) = harness_status_cache().write() {
                *cache = status.clone();
            }
            let observed = match status
                .get("availability")
                .and_then(serde_json::Value::as_str)
            {
                Some("bridge-ready") => HarnessAvailability::BridgeReady,
                Some("web-only") => HarnessAvailability::WebOnly,
                _ => HarnessAvailability::Offline,
            };
            // A compatible Bridge is the only successful probe. `web-only`
            // remains useful diagnostic information, but must settle through
            // the same failure path as offline so a stale ready state cannot
            // keep Harness selectable after the bridge disappears.
            let bridge_ready = observed == HarnessAvailability::BridgeReady;
            if bridge_ready {
                consecutive_successes = consecutive_successes.saturating_add(1);
                consecutive_failures = 0;
                if consecutive_successes >= 2 && last != observed {
                    log::info!("harness availability changed: {:?} -> {:?}", last, observed);
                    last = observed;
                    if let Some(core) = app.try_state::<AppCore>() {
                        let snapshot = core.dispatch(AppAction::SetHarnessAvailability(observed));
                        emit_app_snapshot(&app, &snapshot);
                    }
                }
            } else {
                consecutive_failures = consecutive_failures.saturating_add(1);
                consecutive_successes = 0;
                if consecutive_failures >= 3 && last != observed {
                    log::info!("harness availability changed: {:?} -> {:?}", last, observed);
                    last = observed;
                    if let Some(core) = app.try_state::<AppCore>() {
                        let snapshot = core.dispatch(AppAction::SetHarnessAvailability(observed));
                        emit_app_snapshot(&app, &snapshot);
                    }
                }
            }
            let delay = if !bridge_ready { 2 } else { 5 };
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // DPI 感知已在 main.rs 进程入口设置（Per-Monitor DPI Aware）。
    let appearance_paths =
        appearance::AppearancePaths::from_local_app_data().expect("local app data unavailable");
    appearance_paths
        .create()
        .expect("failed to create appearance directories");
    let appearance_repository = appearance::AppearanceRepository::open(&appearance_paths.catalog)
        .expect("failed to open appearance catalog");
    let appearance_importer = appearance::AppearanceImporter::new(appearance_paths.clone())
        .expect("failed to initialize appearance importer");
    let appearance_exporter = appearance::AppearanceExporter::new(appearance_paths.clone())
        .expect("failed to initialize appearance exporter");
    tauri::Builder::default()
        // This must be registered before plugins that start resident services
        // or create native windows. A second launch then exits before it can
        // create another WorkerW host or duplicate tray icon.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            log::info!("second dsh-wallpaper launch redirected to existing instance");
        }))
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppCore::default())
        .manage(appearance::AppearanceState::with_runtime_io(
            appearance_repository,
            appearance_importer,
            appearance_exporter,
            appearance_paths,
        ))
        .manage(chat::ChatState::default())
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            set_interaction_enabled,
            select_backend,
            dispatch_app_action,
            publish_settings,
            notify_appearance_changed,
            set_lock_screen_enabled,
            clear_stale_lock_screen_backup,
            get_lock_screen_diagnostics,
            set_autostart,
            translucent_tb_status,
            launch_translucent_tb,
            open_translucent_tb_install,
            open_windows_lock_screen_settings,
            prompt_for_api_key,
            show_deepseek_login,
            start_settings_drag,
            hide_settings_window,
            begin_interaction_region_session,
            update_interaction_regions,
            desktop_layout_metrics,
            send_chat,
            cancel_chat,
            connect_harness,
            harness_history,
            api_history,
            probe_harness,
            appearance::commands::appearance_get_state,
            appearance::commands::appearance_list_themes,
            appearance::commands::appearance_list_assets,
            appearance::commands::appearance_activate_theme,
            appearance::commands::appearance_set_override,
            appearance::commands::appearance_clear_override,
            appearance::commands::appearance_import_paths,
            appearance::commands::appearance_classify_asset,
            appearance::commands::appearance_export_current_theme,
            appearance::commands::appearance_resolve_asset,
            appearance::commands::appearance_resolve_library_asset
        ])
        .setup(|app| {
            if let Err(error) = windows_integration::start_wallpaper_host(app.handle().clone()) {
                log::error!("WorkerW wallpaper host failed: {error}");
            }
            if let Some(settings) = app.get_webview_window("settings") {
                if let Err(error) = windows_integration::configure_settings_window(&settings) {
                    log::warn!("settings window frame configuration failed: {error}");
                }
            }
            if let Err(error) = windows_integration::register_session_events(app.handle()) {
                log::warn!("session notification failed: {error}");
            }
            windows_integration::start_foreground_monitor(app.handle().clone());
            windows_integration::start_desktop_workspace_monitor(app.handle().clone());
            start_harness_monitor(app.handle().clone());
            let menu = MenuBuilder::new(app)
                .text("show", "显示中央会话窗")
                .text("hide", "隐藏中央会话窗")
                .separator()
                .text("deepseek-web", "DeepSeek 网页模式")
                .text("deepseek-api", "DeepSeek API 模式")
                .text("harness", "Harness 模式")
                .separator()
                .text("lock", "锁定 Windows")
                .text("settings", "设置")
                .separator()
                .text("quit", "退出")
                .build()?;
            let mut tray = TrayIconBuilder::with_id("dsh-wallpaper")
                .menu(&menu)
                .tooltip("DSH Wallpaper")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(core) = app.try_state::<AppCore>() {
                            core.dispatch(AppAction::SetInteractionEnabled(true));
                        }
                        dispatch_tray_ui_action(app, AppAction::OpenChat);
                    }
                    "hide" => {
                        dispatch_ui_action(app, AppAction::SetInteractionEnabled(false), false)
                    }
                    "deepseek-web" | "deepseek-api" | "harness" => {
                        let _ = app.emit_to(
                            EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
                            "tray-backend",
                            event.id().as_ref(),
                        );
                    }
                    "settings" => {
                        dispatch_tray_ui_action(app, AppAction::OpenSettings);
                        if let Err(error) = show_settings_window(app) {
                            log::error!("failed to show settings window: {error}");
                        }
                    }
                    "lock" => {
                        #[cfg(windows)]
                        {
                            let _ = std::process::Command::new("rundll32.exe")
                                .arg("user32.dll,LockWorkStation")
                                .spawn();
                        }
                    }
                    "quit" => {
                        #[cfg(windows)]
                        windows_integration::restore_desktop_icons();
                        app.exit(0)
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    ) {
                        let app = tray.app_handle();
                        dispatch_tray_ui_action(app, AppAction::OpenSettings);
                        if let Err(error) = show_settings_window(app) {
                            log::error!(
                                "failed to show settings window from tray double-click: {error}"
                            );
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let _ = tray.build(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build dsh-wallpaper")
        .run(|_, _| {});
}
