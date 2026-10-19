use crate::*;

pub(crate) fn emit_app_snapshot(app: &tauri::AppHandle, snapshot: &AppSnapshot) {
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        "app-snapshot",
        snapshot,
    );
}

pub(crate) fn dispatch_ui_action(app: &tauri::AppHandle, action: AppAction, _request_focus: bool) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    emit_app_snapshot(app, &snapshot);
}

pub(crate) fn dispatch_tray_ui_action(app: &tauri::AppHandle, action: AppAction) {
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

pub(crate) fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
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
pub(crate) fn open_settings_window(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_background(&caller)?;
    if let Some(core) = app.try_state::<AppCore>() {
        let snapshot = core.dispatch(AppAction::OpenSettings);
        emit_app_snapshot(&app, &snapshot);
    }
    show_settings_window(&app)
}

#[tauri::command]
pub(crate) fn native_bootstrap_generation(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    let generation = native_bootstrap::generation();
    native_bootstrap::record_startup_diagnostic(&format!(
        "event=handoff-generation-query generation={generation}"
    ));
    Ok(generation)
}

#[tauri::command]
pub(crate) fn release_native_bootstrap(caller: tauri::WebviewWindow, generation: u64) -> Result<bool, String> {
    require_background(&caller)?;
    let raw = caller.hwnd().map_err(|error| error.to_string())?;
    native_bootstrap::release(generation, raw.0 as isize)
}

#[tauri::command]
pub(crate) fn get_app_snapshot(
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
pub(crate) fn desktop_displays(
    caller: tauri::WebviewWindow,
) -> Result<Vec<windows_integration::DesktopDisplayInfo>, String> {
    require_wallpaper_surface(&caller)?;
    windows_integration::desktop_displays()
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn set_interaction_enabled(
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
pub(crate) fn select_backend(
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
pub(crate) fn dispatch_app_action(
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
pub(crate) fn publish_settings(
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
pub(crate) fn start_settings_drag(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_settings(&caller)?;
    app.get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn hide_settings_window(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
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
pub(crate) fn begin_interaction_region_session(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    windows_integration::begin_interaction_region_session()
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn update_interaction_regions(
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
pub(crate) fn enter_inner_workspace_from_ball(
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
pub(crate) fn leave_inner_workspace(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_background(&caller)?;
    windows_integration::leave_inner_workspace(&app)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn desktop_layout_metrics(
    caller: tauri::WebviewWindow,
    display_id: Option<String>,
) -> Result<windows_integration::DesktopLayoutMetrics, String> {
    require_background(&caller)?;
    Ok(windows_integration::desktop_layout_metrics(
        &caller,
        display_id.as_deref(),
    ))
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn raise_client_window(
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
pub(crate) fn open_client_in_browser(
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

// 从 `lib.rs` 纯搬运（阶段 B）：只在壁纸/设置表面可用，地址先过 `external_link::validate`。
/// Open a link from the transcript in the user's default browser.
///
/// The address is model output, so it never reaches the shell unvalidated: see
/// `external_link::validate` for what counts as openable. This command only adds the
/// surface check — the link lives on the wallpaper's chat surface, not in settings.
#[tauri::command]
pub(crate) fn open_external_link(caller: tauri::WebviewWindow, url: String) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    external_link::open(&url).map(|_| ())
}
