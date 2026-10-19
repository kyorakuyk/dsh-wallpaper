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
            commands::desktop::get_app_snapshot,
            commands::desktop::dispatch_app_action,
            commands::lite::lite_settings_get,
            commands::lite::lite_settings_save,
            commands::lite::lite_image_import,
            commands::lite::lite_image_resolve,
            commands::settings::set_autostart,
            commands::settings::autostart_status,
            commands::desktop::native_bootstrap_generation,
            commands::desktop::release_native_bootstrap,
            commands::desktop::start_settings_drag,
            commands::desktop::hide_settings_window
        ])
    };
}

#[cfg(not(feature = "lite"))]
macro_rules! register_edition_commands {
    ($builder:expr) => {
        $builder.invoke_handler(tauri::generate_handler![
            commands::desktop::get_app_snapshot,
            commands::desktop::desktop_displays,
            commands::desktop::set_interaction_enabled,
            commands::desktop::select_backend,
            commands::desktop::dispatch_app_action,
            commands::desktop::publish_settings,
            commands::harness::ensure_profile_bridge,
            notify_appearance_changed,
            commands::settings::set_autostart,
            commands::settings::autostart_status,
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
            commands::settings::save_api_key,
            commands::settings::api_key_status,
            commands::settings::desktop_workspace_status,
            commands::settings::open_project_memory,
            commands::settings::open_subject_tui,
            commands::settings::clear_user_data,
            commands::deepseek_web::show_deepseek_login,
            commands::desktop::native_bootstrap_generation,
            commands::desktop::release_native_bootstrap,
            commands::deepseek_web::deepseek_web_ensure,
            commands::deepseek_web::deepseek_web_status,
            commands::deepseek_web::deepseek_web_history,
            commands::deepseek_web::deepseek_web_adapter_config_status,
            commands::deepseek_web::open_deepseek_web_adapter_config,
            commands::deepseek_web::reset_deepseek_web_adapter_config,
            commands::desktop::open_settings_window,
            commands::desktop::start_settings_drag,
            commands::desktop::hide_settings_window,
            commands::desktop::begin_interaction_region_session,
            commands::desktop::update_interaction_regions,
            commands::desktop::desktop_layout_metrics,
            commands::desktop::enter_inner_workspace_from_ball,
            commands::desktop::leave_inner_workspace,
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
            commands::desktop::raise_client_window,
            commands::desktop::open_client_in_browser,
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
                        commands::desktop::dispatch_tray_ui_action(app, AppAction::OpenChat);
                    }
                    "hide" => {
                        commands::desktop::dispatch_ui_action(app, AppAction::SetInteractionEnabled(false), false)
                    }
                    "deepseek-web" | "deepseek-api" | "harness" => {
                        let _ = app.emit_to(
                            EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
                            "tray-backend",
                            event.id().as_ref(),
                        );
                    }
                    "settings" => {
                        commands::desktop::dispatch_tray_ui_action(app, AppAction::OpenSettings);
                        if let Err(error) = commands::desktop::show_settings_window(app) {
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
                        commands::desktop::dispatch_tray_ui_action(app, AppAction::OpenSettings);
                        if let Err(error) = commands::desktop::show_settings_window(app) {
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
