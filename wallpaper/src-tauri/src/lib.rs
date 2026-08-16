mod app_core;
mod appearance;
mod chat;
mod windows_integration;

use app_core::{Activity, AppAction, AppCore, AppSnapshot, BackendMode, HarnessAvailability};
use std::sync::{OnceLock, RwLock};
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

fn emit_app_snapshot(app: &tauri::AppHandle, snapshot: &AppSnapshot) {
    let _ = app.emit("app-snapshot", snapshot);
}

fn dispatch_ui_action(app: &tauri::AppHandle, action: AppAction, request_focus: bool) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    if snapshot.interaction.visible {
        let _ = windows_integration::show_interaction(app, request_focus);
    }
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
    if snapshot.interaction.visible {
        let _ = windows_integration::show_interaction(app, true);
    }
    emit_app_snapshot(app, &snapshot);
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
    if snapshot.interaction.visible {
        let _ = windows_integration::show_interaction(&app, false);
    } else {
        windows_integration::hide_interaction(&app);
    }
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
    let request_focus = action == "open-chat";
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
    if snapshot.interaction.visible {
        windows_integration::show_interaction(&app, request_focus)?;
    } else {
        windows_integration::hide_interaction(&app);
    }
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
async fn set_lock_screen_enabled(app: tauri::AppHandle, enabled: bool) -> Result<String, String> {
    windows_integration::set_lock_screen(&app, enabled).await
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

#[tauri::command]
fn save_api_key(key: String) -> Result<(), String> {
    keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|e| e.to_string())?
        .set_password(&key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn show_deepseek_login(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("deepseek-login") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("deepseek-webview2");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let login = WebviewWindowBuilder::new(
        &app,
        "deepseek-login",
        WebviewUrl::External(
            "https://chat.deepseek.com"
                .parse()
                .map_err(|e| format!("DeepSeek URL 无效：{e}"))?,
        ),
    )
    .title("DeepSeek 登录")
    .inner_size(980.0, 760.0)
    .decorations(true) // 带系统标题栏与关闭按钮
    .data_directory(data_dir)
    .on_navigation(|url| {
        matches!(
            url.host_str(),
            Some("chat.deepseek.com") | Some("deepseek.com") | Some("www.deepseek.com")
        )
    })
    .build()
    .map_err(|e| e.to_string())?;
    // 关闭后允许重建（Tauri 默认销毁窗口）
    let _ = login;
    Ok(())
}

#[tauri::command]
fn show_interaction(app: tauri::AppHandle) -> Result<(), String> {
    windows_integration::show_interaction(&app, true)
}

#[tauri::command]
fn hide_interaction(app: tauri::AppHandle) {
    windows_integration::hide_interaction(&app)
}

#[tauri::command]
fn update_interaction_regions(
    regions: Vec<windows_integration::InteractionRegionInput>,
    scale_factor: f64,
    revision: u64,
) -> Result<windows_integration::InteractionRegionUpdateResult, String> {
    windows_integration::update_interaction_regions(regions, scale_factor, revision)
}

#[tauri::command]
fn get_desktop_geometry(app: tauri::AppHandle) -> Result<windows_integration::DesktopGeometry, String> {
    let window = app
        .get_webview_window("interaction")
        .or_else(|| app.get_webview_window("background"))
        .ok_or("desktop window missing")?;
    windows_integration::desktop_geometry(&window)
}

#[tauri::command]
fn apply_interaction_placement(
    app: tauri::AppHandle,
    placement: windows_integration::InteractionPlacement,
) -> Result<windows_integration::InteractionPlacement, String> {
    let window = app.get_webview_window("interaction").ok_or("interaction window missing")?;
    windows_integration::apply_interaction_placement(&window, placement)
}

#[tauri::command]
async fn send_chat(
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    mode: String,
    text: String,
    conversation_id: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
) -> Result<Option<String>, String> {
    match mode.as_str() {
        "deepseek-api" => chat::send_api(
            app,
            state,
            text,
            base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            model.unwrap_or_else(|| "deepseek-chat".into()),
            conversation_id,
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
) -> Result<String, String> {
    chat::harness_connect(app, state, resume_session_id).await
}

#[tauri::command]
async fn harness_history(
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    chat::harness_history(state).await
}

static HARNESS_STATUS_CACHE: OnceLock<RwLock<serde_json::Value>> = OnceLock::new();

fn harness_status_cache() -> &'static RwLock<serde_json::Value> {
    HARNESS_STATUS_CACHE
        .get_or_init(|| RwLock::new(serde_json::json!({ "availability": "offline" })))
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
                return serde_json::json!({
                    "availability": "bridge-ready",
                    "bridgeVersion": data.get("bridgeVersion"),
                    "model": data.get("model"),
                    "provider": data.get("provider"),
                    "reasoningEffort": data.get("reasoningEffort"),
                });
            }
        }
    }
    match client.get("http://127.0.0.1:3080/").send().await {
        Ok(_) => serde_json::json!({ "availability": "web-only" }),
        Err(_) => serde_json::json!({ "availability": "offline" }),
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
            set_autostart,
            save_api_key,
            show_deepseek_login,
            show_interaction,
            hide_interaction,
            update_interaction_regions,
            get_desktop_geometry,
            apply_interaction_placement,
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
            appearance::commands::appearance_resolve_asset
        ])
        .setup(|app| {
            if let Err(error) = windows_integration::start_wallpaper_host(app.handle().clone()) {
                log::error!("WorkerW wallpaper host failed: {error}");
            }
            if let Some(background) = app.get_webview_window("background") {
                let _ = background;
            }
            if let Some(interaction) = app.get_webview_window("interaction") {
                if let Err(error) = windows_integration::configure_desktop_interaction(&interaction) {
                    log::error!("desktop interaction configuration failed: {error}");
                }
            }
            if let Err(error) = windows_integration::register_session_events(app.handle()) {
                log::warn!("session notification failed: {error}");
            }
            windows_integration::start_foreground_monitor(app.handle().clone());
            windows_integration::start_desktop_workspace_monitor(app.handle().clone());
            start_harness_monitor(app.handle().clone());
            let menu = MenuBuilder::new(app)
                .text("show", "显示对话")
                .text("hide", "隐藏对话")
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
                        dispatch_tray_ui_action(app, AppAction::OpenChat);
                    }
                    "hide" => dispatch_ui_action(app, AppAction::CloseChat, false),
                    "deepseek-web" | "deepseek-api" | "harness" => {
                        let _ = app.emit("tray-backend", event.id().as_ref());
                    }
                    "settings" => {
                        dispatch_tray_ui_action(app, AppAction::OpenSettings);
                    }
                    "lock" => {
                        #[cfg(windows)]
                        {
                            let _ = std::process::Command::new("rundll32.exe")
                                .arg("user32.dll,LockWorkStation")
                                .spawn();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(event, TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }) {
                        dispatch_tray_ui_action(tray.app_handle(), AppAction::OpenChat);
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
