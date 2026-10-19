#[cfg(not(feature = "lite"))]
mod api_persistence;
mod app_core;
#[cfg(not(feature = "lite"))]
mod appearance;
#[cfg(not(feature = "lite"))]
mod chat;
mod client_window;
mod commands;
mod crash_report;
pub mod desktop_repair;
#[cfg(not(feature = "lite"))]
mod deepseek_web;
// 打开转写里的外部链接：白名单在这个模块里，见其头部说明。
mod external_link;
#[cfg(not(feature = "lite"))]
mod deepseek_web_config;
#[cfg(not(feature = "lite"))]
mod harness_targets;
#[cfg(not(feature = "lite"))]
mod harness_launch;
#[cfg(not(feature = "lite"))]
mod harness_catalog;
// 悬浮球是「折叠态胶囊」的新家，而胶囊只属于完整版；它的创建/监控也只挂在完整版
// 的 setup 块里。像上面 api_persistence / chat 那样按 edition 收窄，否则整个模块
// 在 Lite 构建里全部不可达，会给 Lite 门禁凭空加一串 dead_code 警告。
#[cfg(not(feature = "lite"))]
mod floating_ball;
mod native_bootstrap;
mod native_handoff;
// 启动时间线的锚点与统一写法（只测量，不改启动顺序）：见模块头部说明。
mod startup_timeline;
// 更新检测（第一片：原生侧）：版本读取、GitHub 检查、状态文件与 6 小时节流。完整版专属 ——
// 取数用的是 `reqwest`，那是 `full` 特性后面的依赖，Lite 只给提示（计划书 §七）。
#[cfg(not(feature = "lite"))]
mod update;
mod windows_integration;

#[cfg(not(feature = "lite"))]
use app_core::BackendMode;
use app_core::{Activity, AppAction, AppCore, AppSnapshot, HarnessAvailability};
use std::sync::OnceLock;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, EventTarget, Manager, WebviewUrl, WebviewWindowBuilder};

/// Paint the immutable first frame before Tauri/WebView2 starts. This is a
/// best-effort user-mode guard; it uses a bounded WorkerW discovery window and
/// keeps a valid Progman fallback until the Explorer host can be recovered.
pub fn prepare_native_bootstrap() {
    native_bootstrap::prepare();
}

/// 启动时间线的进程入口锚点。给两个二进制（完整版 `main.rs` 与 Lite）的第一行用，
/// 让「相对进程入口的毫秒」在一个进程里只有一个原点。只读时钟，不改变任何启动顺序。
pub fn startup_timeline_entry() {
    startup_timeline::anchor_process_entry();
}

const SETTINGS_WINDOW_LABEL: &str = "settings";

/// The WorkerW-backed wallpaper WebView is the only surface that may send,
/// cancel, or read chat conversations. Capabilities provide the primary ACL;
/// this guard makes that boundary survive an accidental future capability
/// change or a newly added WebView.
const BACKGROUND_WINDOW_LABEL: &str = "background";

/// The binary entry point fixes the product edition before Tauri starts. Keep
/// it in a process-local cell so native commands can fail closed if an old or
/// compromised Lite renderer tries to invoke a full-edition action.
static LITE_EDITION: OnceLock<bool> = OnceLock::new();

fn is_lite_edition() -> bool {
    *LITE_EDITION.get_or_init(|| false)
}

/// Settings are always authored by the dedicated settings surface and then
/// delivered to the wallpaper by Rust.  This prevents a renderer from
/// selecting an arbitrary event target through `core:event:emit_to`.
const SETTINGS_CHANGED_EVENT: &str = "settings-changed";
#[cfg(not(feature = "lite"))]
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

/// 悬浮球是独立顶层窗口，只允许它请求「进里桌面」。
///
/// 命令表面尽量小：球没有别的能力，也不需要别的能力。
#[cfg(not(feature = "lite"))]
fn require_ball(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == floating_ball::BALL_LABEL {
        Ok(())
    } else {
        Err("该命令只允许悬浮球调用。".into())
    }
}

/// These commands expose no conversation body or system-setting write:
/// the settings center needs a current runtime snapshot, and the wallpaper's
/// DeepSeek entry needs to create/show one fixed, domain-restricted WebView.
/// Keep the shared surface restricted to the two declared WebViews.
fn require_wallpaper_surface(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        BACKGROUND_WINDOW_LABEL | SETTINGS_WINDOW_LABEL => Ok(()),
        _ => Err("该命令只允许壁纸或设置中心调用。".into()),
    }
}

/// Owner of the durable API transcript archive. The wallpaper composer reads
/// it (`api_history`) and the settings center lists and prunes it, so both
/// declared WebViews are allowed and nothing else is.
///
/// Listing returns only conversation metadata — id, message count, bytes and
/// last activity — never a message body, and deletion is the only mutation. A
/// caller that is allowed to destroy the archive is by construction allowed to
/// see its size.
#[cfg(not(feature = "lite"))]
fn require_api_history_owner(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        BACKGROUND_WINDOW_LABEL | SETTINGS_WINDOW_LABEL => Ok(()),
        _ => Err("该命令只允许壁纸或设置中心调用。".into()),
    }
}

/// `keyring`'s Windows backend stores string secrets in a Generic Credential
/// as a UTF-16 little-endian byte blob. CredUI returns UTF-16 code units, so
/// encode them explicitly rather than relying on the host representation when
/// passing a raw `CredentialBlob` to `CredWriteW`.
#[cfg(all(windows, not(feature = "lite")))]
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
#[cfg(all(windows, not(feature = "lite")))]
fn secure_zero_u16(secret: &mut [u16]) {
    for value in secret {
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(all(windows, not(feature = "lite")))]
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
    let window = if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        window
    } else {
        // Keep the resident wallpaper to one WebView at startup. Both the
        // full and Lite editions create the settings surface only on demand;
        // `hide_settings_window` destroys it for Lite and hides it for full.
        let title = if is_lite_edition() {
            "DSH Wallpaper Lite Settings"
        } else {
            "DSH Wallpaper Settings"
        };
        WebviewWindowBuilder::new(
            app,
            SETTINGS_WINDOW_LABEL,
            WebviewUrl::App("index.html?surface=settings".into()),
        )
        .title(title)
        .inner_size(920.0, 680.0)
        .min_inner_size(760.0, 560.0)
        .center()
        .decorations(false)
        .resizable(true)
        .transparent(true)
        // WebView 的**背衬底色**必须是面板深色，不能留默认的白。
        //
        // 起因（用户实测）：快速拖动窗口时会出现白色虚影。原因是窗口被做成透明之后，
        // CSS 根节点也是透明的，于是 WebView2 自己的默认底色（白）成了"还没画上内容
        // 的地方"的底色——拖动/重绘跟不上时那片区域就闪白。
        // 现在背衬是 #0d1625：来不及重绘时露出来的是深色，与面板几乎无差别；
        // 圆角处 CSS 曲线之外的那几像素也由 DWM 的系统圆角裁掉，不会露出方形。
        .background_color(tauri::window::Color(0x0d, 0x16, 0x25, 0xff))
        .focusable(true)
        .skip_taskbar(false)
        .visible(false)
        .build()
        .map_err(|error| format!("无法创建设置中心：{error}"))?
    };
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    // Tao/Windows can restore the active-window accent border as part of
    // SetForegroundWindow. Apply our DWM policy after activation so the CSS
    // outline remains the only visible settings frame.
    windows_integration::configure_settings_window(&window)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_settings_window(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_background(&caller)?;
    if let Some(core) = app.try_state::<AppCore>() {
        let snapshot = core.dispatch(AppAction::OpenSettings);
        emit_app_snapshot(&app, &snapshot);
    }
    show_settings_window(&app)
}

#[tauri::command]
fn native_bootstrap_generation(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    let generation = native_bootstrap::generation();
    native_bootstrap::record_startup_diagnostic(&format!(
        "event=handoff-generation-query generation={generation}"
    ));
    Ok(generation)
}

#[tauri::command]
fn release_native_bootstrap(caller: tauri::WebviewWindow, generation: u64) -> Result<bool, String> {
    require_background(&caller)?;
    let raw = caller.hwnd().map_err(|error| error.to_string())?;
    native_bootstrap::release(generation, raw.0 as isize)
}

#[tauri::command]
fn get_app_snapshot(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, AppCore>,
) -> Result<AppSnapshot, String> {
    require_wallpaper_surface(&caller)?;
    Ok(state.snapshot())
}

/// Enumerate the current physical monitor topology for the full desktop
/// edition. Settings and the background surface share this read-only command;
/// no display or wallpaper setting is changed here.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn desktop_displays(
    caller: tauri::WebviewWindow,
) -> Result<Vec<windows_integration::DesktopDisplayInfo>, String> {
    require_wallpaper_surface(&caller)?;
    windows_integration::desktop_displays()
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
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
#[cfg(not(feature = "lite"))]
fn select_backend(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    backend: String,
) -> Result<AppSnapshot, String> {
    // 设置中心也要能切**正在运行的那个壁纸**的 chat 模式（网页桥接 / API / Harness）——
    // 用户实测提出的正是这条：运行中的壁纸没有 web↔api 的开关，输入岛那个开关只做
    // Harness↔网页。这里的白名单就是那条既有边界（壁纸宿主 + 设置中心），球与网页窗口仍然不行。
    // 切换成后照常 `emit_app_snapshot`：背景端就是靠这份快照改 `runtime.backend` 的（托盘菜单
    // 走的也是同一条路），所以这里不需要第二条通路。
    require_wallpaper_surface(&caller)?;
    if is_lite_edition() {
        return Err("Lite 版不提供后端或模型切换。".into());
    }
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
    if is_lite_edition()
        && !matches!(
            action.as_str(),
            "boot-ready" | "lock" | "unlock" | "wake-done" | "recover"
        )
    {
        return Err("Lite 版不提供会话或桌面交互动作。".into());
    }
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
        "set-island-pinned" => AppAction::SetIslandPinned(match value.as_deref() {
            Some("true") => true,
            Some("false") => false,
            _ => return Err("invalid island pinned flag".into()),
        }),
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
            Some("bridge-loading") => HarnessAvailability::BridgeLoading,
            Some("bridge-auth-unavailable") => HarnessAvailability::BridgeAuthUnavailable,
            Some("bridge-incompatible") => HarnessAvailability::BridgeIncompatible,
            Some("bridge-ready") => HarnessAvailability::BridgeReady,
            _ => return Err("invalid harness availability".into()),
        }),
        _ => return Err(format!("unknown app action: {action}")),
    };
    // Let the native hand-off layer get ahead of the renderer on both a cold
    // start and a later session unlock.  The call is idempotent when the WTS
    // callback already displayed the frame, and the WebView hides it only
    // after painting its matching wake frame.
    match &action {
        AppAction::BootReady { play_wake: true } | AppAction::Unlock { play_wake: true } => {
            let _ = native_bootstrap::start_wake();
        }
        AppAction::Lock => {
            let _ = native_bootstrap::show_sleep();
        }
        _ => {}
    }
    let snapshot = state.dispatch(action);
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
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

/// Lite keeps its small, non-sensitive settings in a native file shared by
/// the background and settings WebViews.  Tauri gives each WebView an
/// isolated browser storage partition, so using localStorage here would make
/// a setting appear to reset whenever the wallpaper surface restarts.
#[cfg(feature = "lite")]
fn lite_settings_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法定位 Lite 设置目录：{error}"))?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建 Lite 设置目录：{error}"))?;
    Ok(directory.join("lite-settings.json"))
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_settings_get(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_wallpaper_surface(&caller)?;
    let path = lite_settings_file(&app)?;
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(serde_json::json!({}));
    };
    if bytes.len() > 128 * 1024 {
        return Ok(serde_json::json!({}));
    }
    let value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|_| {
        log::warn!("Lite 设置文件无法解析，将在前端恢复默认值");
        serde_json::json!({})
    });
    Ok(if value.is_object() {
        value
    } else {
        serde_json::json!({})
    })
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_settings_save(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    settings: serde_json::Value,
) -> Result<(), String> {
    require_settings(&caller)?;
    if !settings.is_object() {
        return Err("Lite 设置必须是 JSON 对象。".into());
    }
    // Whitelist the release surface at the native boundary as well. This
    // prevents a stale/full renderer or a malformed caller from persisting
    // connection, conversation, or credential fields into Lite storage.
    let allowed = [
        "version",
        "background",
        "portrait",
        "animationsEnabled",
        "animationSpeed",
        "playWakeOnEveryUnlock",
        "skipWakeAnimation",
        "desktopWallpaperFallback",
        "autostart",
    ];
    let object = settings.as_object().expect("object checked above");
    let sanitized = object
        .iter()
        .filter(|(key, _)| allowed.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    let settings = serde_json::Value::Object(sanitized);
    let bytes = serde_json::to_vec(&settings).map_err(|error| error.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("Lite 设置内容过大。".into());
    }
    let path = lite_settings_file(&app)?;
    std::fs::write(&path, bytes).map_err(|error| format!("无法保存 Lite 设置：{error}"))?;
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        SETTINGS_CHANGED_EVENT,
        settings,
    );
    Ok(())
}

#[cfg(feature = "lite")]
fn lite_image_directory(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法定位 Lite 素材目录：{error}"))?
        .join("assets");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建 Lite 素材目录：{error}"))?;
    Ok(directory)
}

#[cfg(feature = "lite")]
fn lite_image_extension(path: &std::path::Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| "自定义素材必须是 PNG、JPEG 或 WebP 图片。".to_string())?;
    match extension.as_str() {
        "png" => Ok("png"),
        "jpg" | "jpeg" => Ok("jpg"),
        "webp" => Ok("webp"),
        _ => Err("自定义素材必须是 PNG、JPEG 或 WebP 图片。".into()),
    }
}

#[cfg(feature = "lite")]
fn lite_image_slot_name(slot: &str) -> Result<&'static str, String> {
    match slot {
        "background" => Ok("background"),
        "portrait" => Ok("portrait"),
        _ => Err("Lite 素材位置无效。".into()),
    }
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_image_import(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    slot: String,
    path: String,
) -> Result<(), String> {
    require_settings(&caller)?;
    let slot = lite_image_slot_name(slot.trim())?;
    let source =
        std::fs::canonicalize(path.trim()).map_err(|error| format!("无法读取所选图片：{error}"))?;
    if !source.is_file() {
        return Err("所选路径不是图片文件。".into());
    }
    let metadata =
        std::fs::metadata(&source).map_err(|error| format!("无法读取图片大小：{error}"))?;
    if metadata.len() > 32 * 1024 * 1024 {
        return Err("图片不能超过 32 MB。".into());
    }
    let extension = lite_image_extension(&source)?;
    let directory = lite_image_directory(&app)?;
    let temporary = directory.join(format!(".{slot}-{}.tmp", std::process::id()));
    let destination = directory.join(format!("{slot}.{extension}"));
    let backup = directory.join(format!(".{slot}-{}.bak", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let _ = std::fs::remove_file(&backup);
    std::fs::copy(&source, &temporary).map_err(|error| format!("无法导入图片：{error}"))?;
    // Remove only the old, known slot files after the new copy has completed;
    // a failed copy therefore leaves the previous valid asset untouched.
    for old_extension in ["png", "jpg", "webp"] {
        let old = directory.join(format!("{slot}.{old_extension}"));
        if old != destination {
            let _ = std::fs::remove_file(old);
        }
    }
    let had_previous = if destination.is_file() {
        std::fs::rename(&destination, &backup).is_ok()
    } else {
        false
    };
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        let _ = std::fs::remove_file(&temporary);
        if had_previous {
            let _ = std::fs::rename(&backup, &destination);
        }
        return Err(format!("无法保存 Lite 图片：{error}"));
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_image_resolve(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    slot: String,
) -> Result<Option<serde_json::Value>, String> {
    require_wallpaper_surface(&caller)?;
    let slot = lite_image_slot_name(slot.trim())?;
    let directory = lite_image_directory(&app)?;
    let candidate = ["png", "jpg", "webp"]
        .into_iter()
        .map(|extension| directory.join(format!("{slot}.{extension}")))
        .find(|path| path.is_file());
    let Some(path) = candidate else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|error| format!("无法读取 Lite 图片：{error}"))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Lite 图片超过 32 MB。".into());
    }
    use base64::Engine;
    let extension = lite_image_extension(&path)?;
    let mime_type = match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => unreachable!(),
    };
    Ok(Some(serde_json::json!({
        "mimeType": mime_type,
        "bytesBase64": base64::engine::general_purpose::STANDARD.encode(bytes),
    })))
}

#[cfg(all(test, feature = "lite"))]
mod lite_asset_tests {
    use super::{lite_image_extension, lite_image_slot_name};

    #[test]
    fn accepts_only_supported_lite_image_extensions() {
        assert_eq!(
            lite_image_extension(std::path::Path::new("wallpaper.PNG")),
            Ok("png")
        );
        assert_eq!(
            lite_image_extension(std::path::Path::new("portrait.jpeg")),
            Ok("jpg")
        );
        assert!(lite_image_extension(std::path::Path::new("payload.exe")).is_err());
    }

    #[test]
    fn image_slots_are_closed_to_the_two_public_components() {
        assert_eq!(lite_image_slot_name("background"), Ok("background"));
        assert_eq!(lite_image_slot_name("portrait"), Ok("portrait"));
        assert!(lite_image_slot_name("sleep").is_err());
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
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

fn set_autostart_blocking(enabled: bool) -> Result<windows_integration::AutostartStatus, String> {
    // The whole operation lives next to the Windows calls it makes: which path
    // is authoritative, what is written, and — new — the read-back that decides
    // whether the settings page may call the change applied. This wrapper only
    // keeps it off the UI thread.
    windows_integration::set_autostart(enabled)
}

#[tauri::command]
async fn set_autostart(
    caller: tauri::WebviewWindow,
    enabled: bool,
) -> Result<windows_integration::AutostartStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || set_autostart_blocking(enabled))
        .await
        .map_err(|error| format!("开机自启操作未完成：{error}"))?
}

#[tauri::command]
async fn autostart_status(
    caller: tauri::WebviewWindow,
) -> Result<windows_integration::AutostartStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(windows_integration::autostart_status)
        .await
        .map_err(|error| format!("读取开机自启状态未完成：{error}"))?
}

/// 「桌面会话」目录名。与桥那边（`bridge/src/index.ts`）保持一致：**同一个名字**既是工作区标题，
/// 也是壁纸数据目录下的那个子目录名。
#[cfg(not(feature = "lite"))]
const DESKTOP_WORKSPACE_DIRECTORY_NAME: &str = "桌面会话";

/// 桌面会话工作区落点的自检信息（只读）。
///
/// 位置规则与桥一致：**壁纸自己的数据目录**（`%LOCALAPPDATA%\com.dsh.wallpaper`，也就是打包标识）
/// 下的「桌面会话」。安装目录不能用——MSIX 每次升级会把整个安装目录替换掉，写在那里的东西必丢；
/// 数据目录则运行时可写、升级保留、卸载也留得下（用户明确要求"卸载之后也保留桌面会话的数据"）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn desktop_workspace_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
    Ok(desktop_workspace_status_from(&data_dir))
}

/// 项目记忆的文件名。**必须与桥里的 `PROJECT_MEMORY_FILE_NAME` 一致** —— 桥按它读/注入，
/// 设置中心按它打开；两处不同的话，用户在设置里看到的和助手实际用的会是两个文件。
#[cfg(not(feature = "lite"))]
const PROJECT_MEMORY_FILE_NAME: &str = "项目记忆.md";

/// 在资源管理器里打开「项目记忆」：文件在就选中它，不在就打开工作区目录（顺带建出来）。
///
/// 为什么要有这个按钮：记忆文件的**绝对路径不该出现在桌面会话里**（那是聊天面，不是文件管理器），
/// 但用户确实需要一个地方去改它 —— 那个地方就是设置，路径也只在这里出现。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_project_memory(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
    let workspace = data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME);
    std::fs::create_dir_all(&workspace).map_err(|error| format!("无法创建桌面会话目录：{error}"))?;
    let memory = workspace.join(PROJECT_MEMORY_FILE_NAME);
    // 有文件就选中它（用户一眼看到要改的东西）；没有就把目录打开，别替他造一个空文件。
    let argument = if memory.is_file() {
        format!("/select,{}", memory.display())
    } else {
        workspace.display().to_string()
    };
    std::process::Command::new("explorer.exe")
        .arg(&argument)
        .spawn()
        .map_err(|error| format!("无法打开资源管理器：{error}"))?;
    Ok(serde_json::json!({
        "opened": argument,
        "memoryFile": memory.display().to_string(),
        "memoryExists": memory.is_file(),
    }))
}

/// 「打开 TUI」：把本机的 TUI（`dst`）在一个**新的终端窗口**里拉起来。
///
/// 为什么这件事必须由原生做：能不能开终端窗口、以及**本机到底有没有装 TUI**，都只有原生能回答。
/// 而这条命令的契约是「缺什么就说什么」：找不到 `dst` 时返回 `opened: false` 加一句怎么办，
/// **绝不静默改成打开浏览器** —— 那等于替用户换了一条他没选的路。
///
/// 与 `open_project_memory` 同样的门：**只有设置中心能调**（`require_settings`）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_subject_tui(
    caller: tauri::WebviewWindow,
    args: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let args = harness_launch::normalize_launch_args(args)?;
    let launchers = harness_targets::tui_launcher_paths(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("PATH").ok().as_deref(),
    );
    let Some(launcher) = launchers.first() else {
        return Ok(serde_json::json!({
            "opened": false,
            "reason": "not-installed",
            "message": "本机没有找到 TUI（dst）。安装：npm i -g @deepseek-harness-tui/dsh-tui",
        }));
    };
    let (program, command_args) = harness_launch::tui_launch_command(launcher, &args);
    std::process::Command::new(&program)
        .args(&command_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("无法拉起 TUI：{error}"))?;
    Ok(serde_json::json!({
        "opened": true,
        "launcher": launcher.display().to_string(),
    }))
}

/// 「清除全部用户数据」里，**能由本应用安全删掉**的那几处。
///
/// 刻意不含 WebView2 配置目录（设置与网页登录态都住在里面）：那个目录正被运行中的进程占用，
/// 在这里删它只会删到一半；那条路径由命令自己回报给界面，让用户"退出应用后删掉"。
#[cfg(not(feature = "lite"))]
fn clearable_user_data_targets(data_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME)]
}

/// 清除本应用自己的用户数据：桌面会话工作区 + 凭据管理器里那条 API Key。
///
/// 只做这两件**能确定删干净**的事，并把其余（设置 / 网页登录态）连同路径回报出去 —— 卸载时
/// MSIX 没有自定义卸载界面可以问，所以这个显式入口就是"想清干净的时候能清干净"的答案。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn clear_user_data(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;

    let mut removed: Vec<String> = Vec::new();
    for target in clearable_user_data_targets(&data_dir) {
        if !target.exists() {
            continue;
        }
        std::fs::remove_dir_all(&target).map_err(|error| format!("无法删除 {}：{error}", target.display()))?;
        removed.push(target.display().to_string());
    }

    let credential_removed = remove_api_key_credential()?;

    Ok(serde_json::json!({
        "removed": removed,
        "credentialRemoved": credential_removed,
        // 这两处不在本进程的删除范围内，如实回报路径，让用户自己决定。
        "manual": [
            { "what": "设置与网页登录态", "path": data_dir.display().to_string() },
            { "what": "桥接凭据", "path": "~/.dsh/wallpaper" },
        ],
    }))
}

/// 删除凭据管理器里那条 API Key；本来就不存在也算成功（返回 `false`）。
#[cfg(all(windows, not(feature = "lite")))]
fn remove_api_key_credential() -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = HSTRING::from("deepseek-api.dsh-wallpaper");
    match unsafe { CredDeleteW(&target, CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(all(not(windows), not(feature = "lite")))]
fn remove_api_key_credential() -> Result<bool, String> {
    Err("仅 Windows 支持凭据管理器。".into())
}

#[cfg(all(test, not(feature = "lite")))]
mod desktop_workspace_tests {
    use super::{desktop_workspace_status_from, DESKTOP_WORKSPACE_DIRECTORY_NAME, PROJECT_MEMORY_FILE_NAME};

    /// 位置规则只有一条：数据目录下的「桌面会话」。
    ///
    /// 这条断言的价值在于它同时否掉了两个诱人的位置：安装目录（MSIX 升级时整体替换，写那里的必丢）
    /// 和 DSH 自己的目录（不是"壁纸的目录"，清理时说不清该不该留）。
    #[test]
    fn the_workspace_lives_beside_the_apps_own_data() {
        let status = desktop_workspace_status_from(std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper"));
        assert_eq!(
            status["workspaceDirectory"].as_str().unwrap(),
            format!(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\{DESKTOP_WORKSPACE_DIRECTORY_NAME}")
        );
        assert_eq!(status["dataDirectory"].as_str().unwrap(), r"C:\Users\me\AppData\Local\com.dsh.wallpaper");
        // 「清除全部用户数据」要删的那条凭据在凭据管理器里，不在文件系统上——把名字一并报出来。
        assert_eq!(status["credentialTarget"].as_str().unwrap(), "deepseek-api.dsh-wallpaper");
        // 打包前目录还不存在时也必须给出路径（桥会按同一条规则重建）。
        assert_eq!(status["workspaceExists"].as_bool(), Some(false));
        // 「项目记忆」的路径也要报出来（设置里那个「打开项目记忆」按钮用它），而且**必须与桥读的
        // 是同一个名字**：两处不同的话，用户在设置里打开的和助手实际用的是两个文件。
        assert_eq!(
            status["memoryFile"].as_str().unwrap(),
            format!(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\{DESKTOP_WORKSPACE_DIRECTORY_NAME}\{PROJECT_MEMORY_FILE_NAME}")
        );
        assert_eq!(status["memoryExists"].as_bool(), Some(false));
    }
}

/// 纯函数：只负责"路径怎么算"，好让测试直接钉住它（真实的数据目录是运行时才知道的）。
#[cfg(not(feature = "lite"))]
fn desktop_workspace_status_from(data_dir: &std::path::Path) -> serde_json::Value {
    let workspace = data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME);
    let memory = workspace.join(PROJECT_MEMORY_FILE_NAME);
    serde_json::json!({
        "dataDirectory": data_dir.display().to_string(),
        "workspaceDirectory": workspace.display().to_string(),
        "workspaceExists": workspace.is_dir(),
        // 助手维护的「项目记忆」（说话人格等长期要求就落在这里）；设置里给它一个打开入口，
        // 桌面会话里不贴路径。
        "memoryFile": memory.display().to_string(),
        "memoryExists": memory.is_file(),
        // 清除全部用户数据时要一并删掉的那条凭据（在凭据管理器里，不在文件系统上）。
        "credentialTarget": "deepseek-api.dsh-wallpaper",
    })
}

/// Set the API-key credential in Windows Credential Manager.
/// 把 API Key 写进当前用户的**通用凭据**（target / 用户名与 `keyring` 的 Windows 映射一致，
/// 所以 API 客户端读到的是同一条；keyring 的 `set_password` 把 UTF-16 小端字节当成 blob，
/// 这里必须写成同一个形状）。
#[cfg(all(windows, not(feature = "lite")))]
fn write_api_key_credential(key: &str) -> Result<(), String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::FILETIME,
            Security::Credentials::{
                CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC,
            },
        },
    };

    const CREDENTIAL_TARGET: &str = "deepseek-api.dsh-wallpaper";
    const CREDENTIAL_USERNAME: &str = "deepseek-api";

    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("API Key 不能为空。".into());
    }
    // CredUI 那条路的上限是 256 个 UTF-16 码元；这里直接输入也守同一个界，
    // 免得把一条明显不是 Key 的长文本写进凭据管理器。
    let mut units: Vec<u16> = trimmed.encode_utf16().collect();
    if units.len() > 256 {
        secure_zero_u16(&mut units);
        return Err("API Key 过长（最多 256 个字符）。".into());
    }
    let mut target = CREDENTIAL_TARGET.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut username = CREDENTIAL_USERNAME.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut blob = credential_blob_from_prompt_password(&units);
    let byte_len = u32::try_from(blob.len()).map_err(|_| "API Key 长度无效。".to_string())?;
    let mut credential = CREDENTIALW {
        Flags: CRED_FLAGS::default(),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        Comment: PWSTR::null(),
        LastWritten: FILETIME::default(),
        CredentialBlobSize: byte_len,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_ENTERPRISE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(username.as_mut_ptr()),
    };
    let result = unsafe { CredWriteW(&mut credential, 0) }
        .map_err(|error| format!("无法保存 API Key 到 Windows 凭据管理器：{error}"));
    secure_zero_bytes(&mut blob);
    secure_zero_u16(&mut units);
    secure_zero_u16(&mut target);
    secure_zero_u16(&mut username);
    result
}

/// 展示用的脱敏形式：只留头 3 与尾 4，中间一律替换。
///
/// 这是**唯一**被允许离开原生进程的 Key 形态——渲染端没有任何命令能把明文读回来，
/// 设置中心显示的也只是这个字符串。短 Key 不做"留一半"的漂亮处理：本来就短，全遮。
#[cfg(all(windows, not(feature = "lite")))]
fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    let len = chars.len();
    let tail = |count: usize| -> String { chars[len - count..].iter().collect() };
    match len {
        0 => String::new(),
        // 短到没什么可藏的：整串遮掉，别为了好看泄漏一半。
        1..=6 => "•".repeat(len),
        7..=12 => format!("{}{}", "•".repeat(len - 2), tail(2)),
        _ => format!("{}••••••••{}", chars[..3].iter().collect::<String>(), tail(4)),
    }
}

/// Stores the key the user typed in Settings. This is the one place a plaintext API key
/// crosses the Tauri IPC boundary, and it does so **once, on the way in**: the renderer
/// never receives a key back (`api_key_status` answers with `mask_api_key` only), and the
/// chat client reads the credential natively.
///
/// 这条路径取代了原来那个系统凭据对话框（`prompt_for_api_key`）：用户实测明确要求"在我们
/// 的设置窗内输入"，宁可放弃"明文从不经过渲染端"这条性质，换一个正常的输入体验。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn save_api_key(caller: tauri::WebviewWindow, key: String) -> Result<(), String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以更新 API Key。".into());
    }

    #[cfg(windows)]
    {
        write_api_key_credential(&key)
    }

    #[cfg(not(windows))]
    {
        let _ = key;
        Err("API Key 仅支持 Windows 凭据管理器。".into())
    }
}

/// Whether a key is stored, and the masked form to show in Settings.
///
/// 回答里**永远没有明文**：脱敏在原生侧做完，渲染端拿到的最多是一串点和首尾几个字符。
/// 没有 Key 时返回 `present: false`，而不是报错——"还没配"是正常状态。
#[cfg(all(windows, not(feature = "lite")))]
fn api_key_status_from_store() -> Result<serde_json::Value, String> {
    let entry = keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|_| "无法访问 Windows 凭据管理器。请检查系统凭据服务后重试。".to_string())?;
    match entry.get_password() {
        Ok(key) => Ok(serde_json::json!({ "present": true, "masked": mask_api_key(&key) })),
        Err(_) => Ok(serde_json::json!({ "present": false })),
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn api_key_status(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以读取 API Key 状态。".into());
    }

    #[cfg(windows)]
    {
        api_key_status_from_store()
    }

    #[cfg(not(windows))]
    {
        Ok(serde_json::json!({ "present": false }))
    }
}

#[cfg(all(test, windows))]
#[cfg(not(feature = "lite"))]
mod api_credential_tests {
    use super::{credential_blob_from_prompt_password, mask_api_key};

    #[test]
    fn serializes_the_same_utf16_little_endian_shape_as_keyring_windows() {
        assert_eq!(
            credential_blob_from_prompt_password(&[0x0073, 0x006B, 0x4F60]),
            vec![0x73, 0x00, 0x6B, 0x00, 0x60, 0x4F]
        );
    }

    /// 设置中心显示的是这个字符串——它必须"认得出是哪一条"，同时**不能**泄漏中间那段，
    /// 因为这是渲染端唯一能拿到的 Key 形态（没有任何命令能读回明文）。
    #[test]
    fn never_reveals_the_middle_of_a_stored_key() {
        assert_eq!(mask_api_key("sk-1234567890abcdef"), "sk-••••••••cdef");
        // 首尾照旧看得见，中间那段原样字符一个都不许出现。
        let masked = mask_api_key("sk-1234567890abcdef");
        assert!(!masked.contains("1234567890"));
        assert!(masked.ends_with("cdef"));
        // 前后空白不是 Key 的一部分。
        assert_eq!(mask_api_key("  sk-1234567890abcdef  "), "sk-••••••••cdef");
    }

    /// 短 Key 不玩"留一半"的漂亮处理：全遮，宁可少显示也不多泄漏。
    #[test]
    fn hides_short_keys_entirely_rather_than_half_of_them() {
        assert_eq!(mask_api_key(""), "");
        assert_eq!(mask_api_key("abc"), "•••");
        assert_eq!(mask_api_key("abcdef"), "••••••");
        // 再长一点只留最后两个字符。
        assert_eq!(mask_api_key("abcdefghi"), "•••••••hi");
    }

    /// 真机探针：设置中心那个「测试」按钮背后就是这一次调用（`/models` 要密钥，200 即密钥可用，
    /// 返回体就是可用模型列表）。忽略是刻意的：它要联网，而且依赖这台机器真的存了 Key。
    ///
    /// `cargo test --lib api_credential -- --ignored --nocapture`
    #[test]
    #[ignore = "calls the real DeepSeek API with this machine's stored key"]
    fn this_machine_reports_what_testing_the_stored_api_key_would_answer() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let status = crate::api_key_status_from_store();
        match &status {
            Ok(value) => eprintln!("stored key: {value}"),
            Err(error) => eprintln!("stored key unreadable: {error}"),
        }
        let answer = rt.block_on(crate::chat::api_models("https://api.deepseek.com".into()));
        match &answer {
            Ok(payload) => {
                let models = payload.get("models").and_then(serde_json::Value::as_array);
                eprintln!(
                    "models -> supported={} count={}",
                    payload.get("supported").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    models.map(Vec::len).unwrap_or(0)
                );
                for model in models.unwrap_or(&vec![]).iter().take(12) {
                    eprintln!("  {} / {}", model.get("id").and_then(serde_json::Value::as_str).unwrap_or("?"), model.get("name").and_then(serde_json::Value::as_str).unwrap_or("?"));
                }
            }
            Err(error) => eprintln!("models rejected -> {error}"),
        }
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn show_deepseek_login(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web::show_login(&app))
        .await
        .map_err(|error| format!("DeepSeek 应用内页面启动未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_ensure(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
) -> Result<(), String> {
    require_background(&caller)?;
    tauri::async_runtime::spawn_blocking(move || {
        deepseek_web::ensure(&app, conversation_id, new_conversation.unwrap_or(false))
    })
    .await
    .map_err(|error| format!("DeepSeek 网页预热未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web::WebStatus, String> {
    require_background(&caller)?;
    deepseek_web::status(&app).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_history(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
) -> Result<deepseek_web::WebHistory, String> {
    require_background(&caller)?;
    deepseek_web::history(&app, conversation_id, new_conversation.unwrap_or(false)).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_adapter_config_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web_config::status(&app))
        .await
        .map_err(|error| format!("读取 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn open_deepseek_web_adapter_config(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web_config::open(&app))
        .await
        .map_err(|error| format!("打开 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn reset_deepseek_web_adapter_config(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web_config::reset(&app))
        .await
        .map_err(|error| format!("恢复 DeepSeek 网页配置未完成：{error}"))?
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
    let window = app
        .get_webview_window(SETTINGS_WINDOW_LABEL)
        .ok_or_else(|| "settings window missing".to_string())?;
    if is_lite_edition() {
        window.destroy().map_err(|error| error.to_string())
    } else {
        window.hide().map_err(|error| error.to_string())
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn begin_interaction_region_session(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    windows_integration::begin_interaction_region_session()
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
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

/// 单击悬浮球：进入里桌面（并把球收回）。
///
/// 这是 §1.1 拍板的「表桌面单击胶囊 = 进入里桌面 + 弹出输入岛」的原生那一半：
/// 隐藏 Explorer 的图标层并发出 `desktop-workspace-toggle` 的 `"enter"`，
/// `App.tsx` 收到后会展开输入岛并打开对话。**与桌面空白双击共用同一个实现**
/// （`windows_integration::enter_inner_workspace`），两条入场路径不会分叉。
///
/// 球的收回走请求位，由监控线程在下一拍执行：窗口只由一个线程移动。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn enter_inner_workspace_from_ball(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_ball(&caller)?;
    windows_integration::enter_inner_workspace(&app)?;
    floating_ball::request_ball_retract();
    Ok(())
}

/// 离开里桌面：输入岛右上角的「X」走这条命令。
///
/// **必须落回原生**，因为"现在在不在里桌面"是原生的事实（`INNER_WORKSPACE_ACTIVE`）：
/// 前端自己把界面搬回表桌面，原生那个事实会原地不动——实测后果很实在，点 X 之后
/// 悬浮球再也弹不出来、点球也唤不起输入岛（`enter_inner_workspace` 看到"已经在里桌面"
/// 就直接返回，`desktop-workspace-toggle` 的 `enter` 事件根本不会发出去）。
///
/// 与桌面空白双击那条路**共用同一个实现**，状态迁移也就只有一份。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn leave_inner_workspace(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_background(&caller)?;
    windows_integration::leave_inner_workspace(&app)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn desktop_layout_metrics(
    caller: tauri::WebviewWindow,
    display_id: Option<String>,
) -> Result<windows_integration::DesktopLayoutMetrics, String> {
    require_background(&caller)?;
    Ok(windows_integration::desktop_layout_metrics(
        &caller,
        display_id.as_deref(),
    ))
}






/// Verify that the renderer's island `pointerdown` really is a user click.
///
/// The native click route is closed - a real click reaches neither `WM_MOUSEACTIVATE` nor
/// `WM_LBUTTONDOWN` nor a hit test, because the WebView2 child owns the mouse messages from
/// another process (see `docs/evidence/input-island-focus-native-route-closed.md`). The island event
/// therefore has to be reported by the renderer, and plan 3.A requires the native side not to
/// trust that report: it re-checks the physical left button and the window under the cursor.
///
/// Restricted to the wallpaper surface, so neither the Lite surface nor a remote page can ask
/// for it. It performs no activation yet - this is the diagnostic step of plan 3.A.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn verify_island_click(app: tauri::AppHandle, caller: tauri::WebviewWindow) -> Result<String, String> {
    require_wallpaper_surface(&caller)?;
    // Log both outcomes. Logging only the success made a rejected report
    // indistinguishable from a report that was never sent, which is exactly the
    // ambiguity the repair plan forbids swallowing.
    let verdict = match windows_integration::verify_island_click() {
        Ok(verdict) => verdict,
        Err(error) => {
            log::warn!("island click report rejected: {error}");
            return Err(error);
        }
    };
    log::info!("{verdict}");
    // A verified click is the authorisation: a genuine input event lifts the foreground
    // lock, which is what makes the handover permissible, and why it is bound to a click
    // and never called on its own.
    windows_integration::hand_over_keyboard_for_app(&app);
    Ok(verdict)
}




/// Bring the running DSH client's own Windows window forward.
///
/// The rule splits by client shape: a windowless client (CLI / webui on 3080) is
/// opened in the default browser by the renderer, while the two desktop clients
/// are raised here. Doing the desktop half natively means the window raised is
/// resolved from the port the wallpaper is actually connected to, so it is the
/// same client whose session the user is talking to — and no stored executable
/// path can go stale.
///
/// This never launches anything. Starting a client is a separate, heavier action
/// with its own single-flight and trust decisions; raising must not become a back
/// door that spawns processes.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn raise_client_window(
    caller: tauri::WebviewWindow,
    port: u16,
) -> Result<client_window::RaiseOutcome, String> {
    require_wallpaper_surface(&caller)?;
    if port == 0 {
        return Err("接入端点端口无效".into());
    }
    Ok(client_window::raise_client_window(port))
}

/// Open the windowless client's web UI in the user's default browser.
///
/// The rule splits by client shape: the CLI / webui shape has no Windows window to
/// raise, so its interface is the browser at the endpoint it listens on (3080 by
/// default). The two desktop clients never take this path.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_client_in_browser(
    caller: tauri::WebviewWindow,
    port: u16,
    path: Option<String>,
) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    // 没给路径、或给的就是裸根 `/` 时，用**这次启动打印出来的门票**：`dsh web` 的浏览器围栏要求
    // URL 上带 token，裸端口只会得到那句 "dsh web authentication required"。门票由启动时捕获
    // （见 `harness_launch::known_web_handoff`），前端不需要知道它存在。
    //
    // 把 `/` 也当作"没指定"是刻意的：渲染层传参默认值就是 `'/'`（实测），而对一个带围栏的宿主
    // 来说，裸根本来就不是任何人想要的结果 —— 它只会换来一句道歉。
    let requested = path.as_deref().unwrap_or("/");
    let target = if requested == "/" {
        harness_launch::known_web_handoff(port).unwrap_or_else(|| requested.to_string())
    } else {
        requested.to_string()
    };
    client_window::open_loopback_url(port, &target)
}

/// Open a link from the transcript in the user's default browser.
///
/// The address is model output, so it never reaches the shell unvalidated: see
/// `external_link::validate` for what counts as openable. This command only adds the
/// surface check — the link lives on the wallpaper's chat surface, not in settings.
#[tauri::command]
fn open_external_link(caller: tauri::WebviewWindow, url: String) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    external_link::open(&url).map(|_| ())
}



#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = LITE_EDITION.set(false);
    run_with_edition(false);
}

/// Entry point used by the first public Lite package. The two editions share
/// the audited Windows desktop host, but Lite does not start chat, DeepSeek
/// WebView, Harness probing, or desktop-workspace monitors.
pub fn run_lite() {
    let _ = LITE_EDITION.set(true);
    run_with_edition(true);
}

#[cfg(feature = "lite")]
macro_rules! register_edition_commands {
    ($builder:expr) => {
        $builder.invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            dispatch_app_action,
            lite_settings_get,
            lite_settings_save,
            lite_image_import,
            lite_image_resolve,
            set_autostart,
            autostart_status,
            native_bootstrap_generation,
            release_native_bootstrap,
            start_settings_drag,
            hide_settings_window
        ])
    };
}

#[cfg(not(feature = "lite"))]
macro_rules! register_edition_commands {
    ($builder:expr) => {
        $builder.invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            desktop_displays,
            set_interaction_enabled,
            select_backend,
            dispatch_app_action,
            publish_settings,
            commands::harness::ensure_profile_bridge,
            notify_appearance_changed,
            set_autostart,
            autostart_status,
            commands::harness::scan_dsh_paths,
            commands::harness::scan_harness_targets,
            commands::harness::harness_target_catalog,
            commands::harness::launch_harness_target,
            commands::harness::ensure_harness_ui,
            commands::harness::autostart_harness_target,
            commands::harness::launch_dsh,
            commands::harness::autostart_managed_dsh,
            commands::harness::managed_dsh_autostart_status,
            commands::harness::managed_dsh_status,
            commands::harness::stop_managed_dsh,
            save_api_key,
            api_key_status,
            desktop_workspace_status,
            open_project_memory,
            open_subject_tui,
            clear_user_data,
            show_deepseek_login,
            native_bootstrap_generation,
            release_native_bootstrap,
            deepseek_web_ensure,
            deepseek_web_status,
            deepseek_web_history,
            deepseek_web_adapter_config_status,
            open_deepseek_web_adapter_config,
            reset_deepseek_web_adapter_config,
            open_settings_window,
            start_settings_drag,
            hide_settings_window,
            begin_interaction_region_session,
            update_interaction_regions,
            desktop_layout_metrics,
            enter_inner_workspace_from_ball,
            leave_inner_workspace,
            commands::chat::send_chat,
            commands::chat::cancel_chat,
            commands::harness::connect_harness,
            commands::harness::harness_history,
            commands::harness::harness_presets,
            commands::harness::harness_set_preset,
            commands::harness::harness_controls,
            commands::harness::harness_models,
            commands::harness::harness_set_model,
            commands::chat::api_models,
            commands::harness::harness_set_permission,
            commands::chat::api_history,
            commands::chat::list_api_conversations,
            commands::chat::delete_api_conversation,
            commands::chat::clear_api_history,
            commands::harness_status::probe_harness,
            commands::harness_status::scan_harness_endpoints_command,
            commands::harness_status::set_harness_endpoint,
            verify_island_click,
            raise_client_window,
            open_client_in_browser,
            open_external_link,
            commands::harness_status::harness_endpoint_listening,
            commands::harness_status::harness_endpoint_window,
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
            appearance::commands::appearance_resolve_library_asset,
            // 更新检测：壁纸宿主（立绘气泡）与设置中心（系统页）各调一次；返回值只有结构化码与
            // 数字，文案由界面按语言说。`update_dismiss` 是气泡与设置页共用的那一条「忽略」；
            // `update_download` / `update_install` 是第三片的「下载」与「点击安装」（进度走
            // `update-download` 那条全局事件，见 `update/download.rs`）。
            update::commands::update_check,
            update::commands::update_dismiss,
            update::commands::update_download,
            update::commands::update_install
        ])
    };
}

fn run_with_edition(lite: bool) {
    // 崩溃自证（2026-10-05）：装上 SEH 处理器与 panic hook，让下一次「莫名其妙自动退出」
    // 在 logs 目录里留下一份带模块基址与调用链的报告。装得越早覆盖越全，而且必须在
    // Tauri/WebView2 起来之前 —— 顶层过滤器只有一个槽位，晚装就可能被别人顶掉。
    crash_report::install(lite);
    startup_timeline::log_point("crash_report 就绪（进程入口的第二站）");
    // DPI 感知已在 main.rs 进程入口设置（Per-Monitor DPI Aware）。
    if !windows_integration::acquire_shared_wallpaper_host() {
        startup_timeline::log_point("未拿到单实例宿主权，进程在此退出");
        return;
    }
    startup_timeline::log_point("单实例宿主权已确认，即将创建 Tauri Builder");
    let mut builder = tauri::Builder::default()
        // This must be registered before plugins that start resident services
        // or create native windows. A second launch then exits before it can
        // create another WorkerW host or duplicate tray icon.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            log::info!("second dsh-wallpaper launch redirected to existing instance");
        }))
        // The log plugin defaults to Trace. The resident Harness probe and
        // WebView2 internals would otherwise append a DEBUG connection line
        // every few seconds, adding needless I/O and obscuring real startup
        // failures in the small rotating log.
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .manage(AppCore::default());
    // 这一条量的是「Tauri Builder 装配完插件与状态」这一步到哪里为止：插件链里已经含了
    // 日志插件（所以这一条起，日志终于落到文件里）与单实例插件。
    startup_timeline::log_point("Tauri Builder 插件链装配完成（logging 与单实例插件已注册）");

    // The Lite process has no chat or theme-library surface. Avoid opening
    // SQLite, allocating the encrypted API store, or creating a managed DSH
    // child state at startup; those services are registered only by the full
    // edition. Commands remain compiled for the full build, but the Lite
    // capability files do not grant them to either Lite WebView.
    #[cfg(not(feature = "lite"))]
    {
        let appearance_paths =
            appearance::AppearancePaths::from_local_app_data().expect("local app data unavailable");
        appearance_paths
            .create()
            .expect("failed to create appearance directories");
        let appearance_repository =
            appearance::AppearanceRepository::open(&appearance_paths.catalog)
                .expect("failed to open appearance catalog");
        let appearance_importer = appearance::AppearanceImporter::new(appearance_paths.clone())
            .expect("failed to initialize appearance importer");
        let appearance_exporter = appearance::AppearanceExporter::new(appearance_paths.clone())
            .expect("failed to initialize appearance exporter");
        builder = builder
            .manage(appearance::AppearanceState::with_runtime_io(
                appearance_repository,
                appearance_importer,
                appearance_exporter,
                appearance_paths,
            ))
            .manage(chat::ChatState::default())
            .manage(deepseek_web::DeepSeekWebState::default())
            .manage(commands::harness::ManagedDshState::default())
            .manage(commands::harness::ManagedDshAutostartState::default());
    }

    register_edition_commands!(builder)
        .setup(move |app| {
            native_bootstrap::report_tauri_ready();
            startup_timeline::mark("setup-enter", "Tauri setup 回调进入");
            // 崩溃报告写到哪：启动时留一行，用户照着它就能找到下次崩溃的那份报告。
            #[cfg(windows)]
            match crash_report::report_directory() {
                Some(directory) => log::info!(
                    "崩溃自证已就绪：崩溃报告会写到 {directory}\\crash-<时间戳>.txt"
                ),
                None => log::warn!("崩溃自证未就绪：报告目录不可用"),
            }
            // "这个孩子是不是我启动的"要跨壁纸重启成立，就得把记录落在本地数据目录里 ——
            // 这个路径只有 Tauri 算得准（打包应用会被重定向），不能靠环境变量硬拼。
            // `harness_launch` 是 `#[cfg(not(feature = "lite"))]` 的模块，所以这里必须同样受门控：
            // Lite 目标里它根本不存在（CI 抓到的就是这个 E0433 —— 我本地只跑默认特性，看不见）。
            #[cfg(not(feature = "lite"))]
            {
                match app.path().app_local_data_dir() {
                    Ok(dir) => harness_launch::set_records_path(dir.join("managed-dsh.json")),
                    Err(error) => log::warn!("managed-child record path unavailable: {error}"),
                }
                // 更新目录的"顺手清理"（0.4.6 把标识符拼了两遍，状态与安装包落在嵌套目录里）
                // 在启动时就做一次：不按任何按钮的一次启动也会把旧目录收干净。它只记日志、
                // 不返回错误，失败绝不拖住启动链。
                update::commands::reconcile_updates_at_startup(app.handle());
                startup_timeline::mark("update-reconcile-done", "更新目录启动清理完成");
            }
            // 窗口句柄槽位的第一次写入：这一行必须在三条监视线程启动之前。
            // 线程不许自己回 Tauri 取窗口（那会 clone wry `Context` 的引用计数，见
            // `windows_integration` 的说明），只能读这个槽位；而槽位要在窗口创建与重建处
            // 由主线程写入。`background` 是 Tauri 按配置建好的，此时句柄已经能取到；
            // 悬浮球还要晚一步才建，`ensure_ball` 会自己再写一次。
            windows_integration::publish_current_window_channels(app.handle(), None);
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
            #[cfg(not(feature = "lite"))]
            {
                windows_integration::start_foreground_monitor(app.handle().clone());
                windows_integration::start_desktop_workspace_monitor(app.handle().clone());
                commands::harness_status::start_harness_monitor(app.handle().clone());
                startup_timeline::mark("harness-monitor-started", "Harness 监视已启动");
                // 悬浮球（interaction-handover §3.1 的增量 1）：独立顶层窗口，
                // 平时停在屏幕之外，鼠标靠近底边才滑入。它是人工入口而不是常驻
                // 服务，因此创建失败只记日志，绝不打断启动链（也绝不 `?`）。
                match floating_ball::ensure_ball(app.handle()) {
                    Ok(_) => floating_ball::start_ball_monitor(app.handle().clone()),
                    Err(error) => log::warn!("floating ball unavailable: {error}"),
                }
                startup_timeline::mark("floating-ball-ready", "悬浮球创建与监视已就绪");
            }
            // Keep an enabled pre-StartupTask installation running across a
            // package update. The migration is best-effort and leaves the
            // legacy Run entry intact if Windows asks for user approval or
            // denies the new task.
            tauri::async_runtime::spawn_blocking(|| {
                match windows_integration::migrate_legacy_autostart() {
                    Ok(Some(status)) => log::info!(
                        "autostart migration check complete: source={}, enabled={}",
                        status.source,
                        status.enabled
                    ),
                    Ok(None) => {}
                    Err(error) => log::warn!("autostart migration deferred: {error}"),
                }
            });
            let menu = if lite {
                MenuBuilder::new(app)
                    .text("settings", "设置")
                    .separator()
                    .text("lock", "锁定 Windows")
                    .separator()
                    .text("quit", "退出")
                    .build()?
            } else {
                MenuBuilder::new(app)
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
                    .build()?
            };
            let mut tray = TrayIconBuilder::with_id("dsh-wallpaper")
                .menu(&menu)
                .tooltip(if lite {
                    "DSH Wallpaper Lite"
                } else {
                    "DSH Wallpaper"
                })
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
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                shutdown_native_state(app);
            }
        });
}

/// Restore everything this process changed on the user's desktop.
///
/// Hiding the settings window or minimizing the wallpaper is not an exit and
/// must not run any of this: the tray quit item and a real window close both
/// reach `RunEvent::ExitRequested`, while tray "hide" only dispatches an
/// action. Every step is idempotent, because `ExitRequested` can be followed by
/// `Exit`.
fn shutdown_native_state(app: &tauri::AppHandle) {
    static SHUTDOWN: std::sync::Once = std::sync::Once::new();
    SHUTDOWN.call_once(|| {
        // 1. The desktop icon layer is deliberately hidden while the wallpaper
        //    is resident; leaving it hidden after exit would strand the user
        //    with an iconless desktop.
        #[cfg(windows)]
        windows_integration::restore_desktop_icons();

        // 2. Destroy the native first-frame hand-off window. Hiding it is not
        //    enough at exit: its GDI bitmaps and the boosted process priority
        //    would outlive the WebView.
        if let Err(error) = native_bootstrap::destroy() {
            log::warn!("native bootstrap teardown failed: {error}");
        }

        // 3. Stop only the DSH instances this process launched. An external DSH —
        //    or someone else's client on our port — belongs to the user and is
        //    never touched. 现在是**每一个**我们自己启动的实例：并行实例意味着出口这里也可能
        //    不止一个，漏掉一个就会在壁纸退出后留下一台没人认领的宿主。
        #[cfg(not(feature = "lite"))]
        commands::harness::shutdown_managed_dsh(app);

        // 4. Release native session/notification resources owned by this
        //    process so a restart does not inherit a stale registration.
        windows_integration::unregister_session_events(app);

        // 5. Last, record that the desktop was released. The repair helper reads
        //    this marker after this process is gone: its presence means the
        //    mutations above were undone, its absence means this process died
        //    early and the desktop still needs repairing. Writing it any earlier
        //    would let the helper skip a repair for work that had not happened yet.
        desktop_repair::mark_clean_exit();

        log::info!("dsh-wallpaper native state restored on exit");
    });
}
