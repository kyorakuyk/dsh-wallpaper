mod chat;
mod windows_integration;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;

#[tauri::command]
async fn set_lock_screen_enabled(app: tauri::AppHandle, enabled: bool) -> Result<String, String> { windows_integration::set_lock_screen(&app, enabled).await }

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(windows)] {
        use std::process::Command;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // 注册表 Run 键：开机自启 dsh-wallpaper
        //  add:    reg add HKCU\...\Run /V dsh-wallpaper /D "<exe>" /F
        //  delete: reg delete HKCU\...\Run /V dsh-wallpaper /F
        let mut cmd = Command::new("reg");
        if enabled {
            cmd.args([
                "add", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V", "dsh-wallpaper", "/D", &exe.to_string_lossy(), "/F",
            ]);
        } else {
            cmd.args([
                "delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V", "dsh-wallpaper", "/F",
            ]);
        }
        let status = cmd.status().map_err(|e| e.to_string())?;
        if !status.success() { return Err("更新当前用户开机自启失败".into()); }
    }
    Ok(())
}

#[tauri::command]
fn save_api_key(key: String) -> Result<(), String> { keyring::Entry::new("dsh-wallpaper", "deepseek-api").map_err(|e| e.to_string())?.set_password(&key).map_err(|e| e.to_string()) }

#[tauri::command]
fn show_deepseek_login(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("deepseek-login") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("deepseek-webview2");
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let login = WebviewWindowBuilder::new(&app, "deepseek-login", WebviewUrl::External("https://chat.deepseek.com".parse().map_err(|e| format!("DeepSeek URL 无效：{e}"))?))
        .title("DeepSeek 登录")
        .inner_size(980.0, 760.0)
        .decorations(true) // 带系统标题栏与关闭按钮
        .data_directory(data_dir)
        .on_navigation(|url| matches!(url.host_str(), Some("chat.deepseek.com") | Some("deepseek.com") | Some("www.deepseek.com")))
        .build().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn show_interaction(app: tauri::AppHandle) -> Result<(), String> { windows_integration::show_interaction(&app) }

#[tauri::command]
fn hide_interaction(app: tauri::AppHandle) { windows_integration::hide_interaction(&app) }

#[tauri::command]
async fn send_chat(app: tauri::AppHandle, state: tauri::State<'_, chat::ChatState>, mode: String, text: String, _conversation_id: Option<String>, base_url: Option<String>, model: Option<String>) -> Result<(), String> {
    match mode.as_str() {
        "deepseek-api" => chat::send_api(app, state, text, base_url.unwrap_or_else(|| "https://api.deepseek.com".into()), model.unwrap_or_else(|| "deepseek-chat".into())).await,
        "harness" => chat::harness_send(state, text).await,
        _ => Err("DeepSeek 网页桥接需要登录 WebView 和 DOM adapter".into()),
    }
}

#[tauri::command]
async fn cancel_chat(state: tauri::State<'_, chat::ChatState>, mode: String) -> Result<(), String> {
    if mode == "harness" { chat::harness_cancel(state).await } else { chat::cancel_api(&state); Ok(()) }
}

#[tauri::command]
async fn connect_harness(app: tauri::AppHandle, state: tauri::State<'_, chat::ChatState>, resume_session_id: Option<String>) -> Result<String, String> { chat::harness_connect(app, state, resume_session_id).await }

#[tauri::command]
async fn harness_history(state: tauri::State<'_, chat::ChatState>) -> Result<serde_json::Value, String> { chat::harness_history(state).await }

#[tauri::command]
async fn probe_harness() -> serde_json::Value {
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_millis(1200)).build() {
        Ok(client) => client,
        Err(_) => return serde_json::json!({ "availability": "offline" }),
    };
    if let Ok(response) = client.get("http://127.0.0.1:3080/api/wallpaper/v1/status").send().await {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // DPI 感知已在 main.rs 进程入口设置（Per-Monitor DPI Aware）。
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .manage(chat::ChatState::default())
        .invoke_handler(tauri::generate_handler![set_lock_screen_enabled, set_autostart, save_api_key, show_deepseek_login, show_interaction, hide_interaction, send_chat, cancel_chat, connect_harness, harness_history, probe_harness])
        .setup(|app| {
            let background = app.get_webview_window("background").expect("background window");
            if let Err(error) = windows_integration::attach_to_workerw(&background) { log::warn!("WorkerW attach failed: {error}"); }
            // SetParent 到 WorkerW 后，等 WebView 完成初始化再通过 Tauri 重新显示，
            // 避免 Tauri 的可见性状态与原生窗口不一致导致壁纸层不可见。
            let bg_handle = background.clone();
            std::thread::spawn(move || {
                // 首次启动存在 WebView 初始化竞态：多次延迟 show() 确保最终可见。
                // Tauri 稳定后强制原生物理全屏（避免逻辑/物理像素混淆导致非全屏）。
                for delay_ms in [600u64, 1500, 3000, 5000] {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    let _ = windows_integration::force_fullscreen(&bg_handle);
                    let _ = bg_handle.show();
                }
            });
            if let Err(error) = windows_integration::register_session_events(app.handle()) { log::warn!("session notification failed: {error}"); }
            windows_integration::start_foreground_monitor(app.handle().clone());
            if app.get_webview_window("interaction").is_none() {
                let _ = WebviewWindowBuilder::new(app, "interaction", WebviewUrl::App("index.html?surface=interaction".into())).transparent(true).decorations(false).visible(false).build();
            }
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
                    "show" => { let _ = windows_integration::show_interaction(app); }
                    "hide" => windows_integration::hide_interaction(app),
                    "deepseek-web" | "deepseek-api" | "harness" => { let _ = app.emit("tray-backend", event.id().as_ref()); }
                    "settings" => { let _ = windows_integration::show_interaction(app); let _ = app.emit("tray-settings", ()); }
                    "lock" => { #[cfg(windows)] { let _ = std::process::Command::new("rundll32.exe").arg("user32.dll,LockWorkStation").spawn(); } }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() { tray = tray.icon(icon.clone()); }
            let _ = tray.build(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build dsh-wallpaper")
        .run(|_, _| {});
}
