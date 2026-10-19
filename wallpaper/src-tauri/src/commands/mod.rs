#[cfg(not(feature = "lite"))]
pub(super) mod harness;
#[cfg(not(feature = "lite"))]
pub(super) mod harness_status;
#[cfg(not(feature = "lite"))]
pub(super) mod chat;
pub(super) mod settings;
#[cfg(feature = "lite")]
pub(super) mod lite;
#[cfg(not(feature = "lite"))]
pub(super) mod deepseek_web;
pub(super) mod desktop;

// 调用方边界（纯搬运自 `lib.rs`）：只回答"这个调用来自哪个表面"，不碰任何状态。
// 命令模块通过 `super::require_*` 引入；`require_ball` 受 edition 门控。
pub(crate) fn require_background(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == crate::BACKGROUND_WINDOW_LABEL {
        Ok(())
    } else {
        Err("该命令只允许壁纸宿主调用。".into())
    }
}

pub(crate) fn require_settings(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == crate::SETTINGS_WINDOW_LABEL {
        Ok(())
    } else {
        Err("该命令只允许设置中心调用。".into())
    }
}

/// 悬浮球是独立顶层窗口，只允许它请求「进里桌面」。
///
/// 命令表面尽量小：球没有别的能力，也不需要别的能力。
#[cfg(not(feature = "lite"))]
pub(crate) fn require_ball(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == crate::floating_ball::BALL_LABEL {
        Ok(())
    } else {
        Err("该命令只允许悬浮球调用。".into())
    }
}

/// These commands expose no conversation body or system-setting write:
/// the settings center needs a current runtime snapshot, and the wallpaper's
/// DeepSeek entry needs to create/show one fixed, domain-restricted WebView.
/// Keep the shared surface restricted to the two declared WebViews.
pub(crate) fn require_wallpaper_surface(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        crate::BACKGROUND_WINDOW_LABEL | crate::SETTINGS_WINDOW_LABEL => Ok(()),
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
pub(crate) fn require_api_history_owner(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        crate::BACKGROUND_WINDOW_LABEL | crate::SETTINGS_WINDOW_LABEL => Ok(()),
        _ => Err("该命令只允许壁纸或设置中心调用。".into()),
    }
}

// 两份 `generate_handler!` 列表（完整版与 Lite）与命令模块住在一起。命令名就是函数名，
// 前端 `invoke(...)` 不受注册位置影响；Lite 注册 12 项、完整版 83 项，两边都必须与
// 迁移前完全一致 —— `update/commands.rs` 的 ACL 测试与前端源码断言都读这个文件。
// 宏体逐字来自原 `lib.rs`：路径按展开点（`lib.rs`）解析，所以不加 `crate::` 前缀。
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
            commands::harness::ui::ensure_profile_bridge,
            notify_appearance_changed,
            commands::settings::set_autostart,
            commands::settings::autostart_status,
            commands::harness::discovery::scan_dsh_paths,
            commands::harness::discovery::scan_harness_targets,
            commands::harness::discovery::harness_target_catalog,
            commands::harness::ui::launch_harness_target,
            commands::harness::ui::ensure_harness_ui,
            commands::harness::ui::autostart_harness_target,
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
            commands::harness::session::connect_harness,
            commands::harness::session::harness_history,
            commands::harness::session::harness_presets,
            commands::harness::session::harness_set_preset,
            commands::harness::session::harness_controls,
            commands::harness::session::harness_models,
            commands::harness::session::harness_set_model,
            commands::chat::api_models,
            commands::harness::session::harness_set_permission,
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
            commands::desktop::open_external_link,
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

pub(crate) use register_edition_commands;
