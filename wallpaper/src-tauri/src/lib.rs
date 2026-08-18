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
use tauri::{Emitter, Manager};

fn emit_app_snapshot(app: &tauri::AppHandle, snapshot: &AppSnapshot) {
    let _ = app.emit("app-snapshot", snapshot);
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
fn get_app_snapshot(state: tauri::State<'_, AppCore>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
fn set_interaction_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    enabled: bool,
) -> AppSnapshot {
    let snapshot = state.dispatch(AppAction::SetInteractionEnabled(enabled));
    emit_app_snapshot(&app, &snapshot);
    snapshot
}

#[tauri::command]
fn select_backend(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    backend: String,
) -> Result<AppSnapshot, String> {
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    action: String,
    play_wake: Option<bool>,
    value: Option<String>,
) -> Result<AppSnapshot, String> {
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
async fn set_lock_screen_enabled(app: tauri::AppHandle, enabled: bool) -> Result<String, String> {
    windows_integration::set_lock_screen(&app, enabled).await
}

#[tauri::command]
fn get_lock_screen_diagnostics(
    app: tauri::AppHandle,
) -> Result<windows_integration::LockScreenDiagnostics, String> {
    windows_integration::lock_screen_diagnostics(&app)
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
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
fn translucent_tb_status() -> TranslucentTbStatus {
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
        return TranslucentTbStatus {
            installed: alias || packaged,
            running,
            source: if alias {
                Some("execution-alias".into())
            } else if packaged {
                Some("msix".into())
            } else {
                None
            },
        };
    }
    #[cfg(not(windows))]
    TranslucentTbStatus {
        installed: false,
        running: false,
        source: None,
    }
}

#[tauri::command]
fn launch_translucent_tb() -> Result<(), String> {
    std::process::Command::new("ttb.exe")
        .spawn()
        .map(|_| ())
        .map_err(|_| {
            "未找到 TranslucentTB。请先从 Microsoft Store 安装并启用 ttb.exe 执行别名。".into()
        })
}

#[tauri::command]
fn open_translucent_tb_install() -> Result<(), String> {
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

#[tauri::command]
fn save_api_key(key: String) -> Result<(), String> {
    keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|e| e.to_string())?
        .set_password(&key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn show_deepseek_login(app: tauri::AppHandle) -> Result<(), String> {
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
fn start_settings_drag(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn begin_interaction_region_session() -> Result<u64, String> {
    windows_integration::begin_interaction_region_session()
}

#[tauri::command]
fn update_interaction_regions(
    regions: Vec<windows_integration::InteractionRegionInput>,
    scale_factor: f64,
    session: u64,
    revision: u64,
) -> Result<windows_integration::InteractionRegionUpdateResult, String> {
    windows_integration::update_interaction_regions(regions, scale_factor, session, revision)
}

#[tauri::command]
async fn send_chat(
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
    state: tauri::State<'_, chat::ChatState>,
    conversation_id: String,
) -> Result<serde_json::Value, String> {
    chat::api_history(state.inner(), &conversation_id)
}

#[tauri::command]
async fn cancel_chat(state: tauri::State<'_, chat::ChatState>, mode: String) -> Result<(), String> {
    if mode == "harness" {
        chat::harness_cancel(state).await
    } else {
        chat::cancel_api(&state);
        Ok(())
    }
}

#[tauri::command]
async fn connect_harness(
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    resume_session_id: Option<String>,
    connection_id: String,
) -> Result<String, String> {
    chat::harness_connect(app, state, resume_session_id, connection_id).await
}

#[tauri::command]
async fn harness_history(
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
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
    match client.get("http://127.0.0.1:3080/").send().await {
        Ok(_) => serde_json::json!({ "availability": "web-only" }),
        Err(_) => serde_json::json!({ "availability": "offline" }),
    }
}

#[cfg(test)]
mod harness_status_tests {
    use super::compatible_harness_bridge_status;
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
}

#[tauri::command]
fn probe_harness() -> serde_json::Value {
    harness_status_cache()
        .read()
        .map(|status| status.clone())
        .unwrap_or_else(|_| serde_json::json!({ "availability": "offline" }))
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
            if observed == HarnessAvailability::BridgeReady {
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
            let delay = if last == HarnessAvailability::Offline {
                2
            } else {
                5
            };
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
            set_lock_screen_enabled,
            get_lock_screen_diagnostics,
            set_autostart,
            translucent_tb_status,
            launch_translucent_tb,
            open_translucent_tb_install,
            save_api_key,
            show_deepseek_login,
            start_settings_drag,
            hide_settings_window,
            begin_interaction_region_session,
            update_interaction_regions,
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
                        let _ = app.emit("tray-backend", event.id().as_ref());
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
