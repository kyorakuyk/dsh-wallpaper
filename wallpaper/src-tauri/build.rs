fn main() {
    // Application commands do not receive an ACL entry unless they are
    // declared here. Keep this list in lockstep with `generate_handler!` so
    // capabilities can grant a command to one WebView without implicitly
    // granting it to every window in the application.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_snapshot",
            "desktop_displays",
            "set_interaction_enabled",
            "select_backend",
            "dispatch_app_action",
            "publish_settings",
            "lite_settings_get",
            "lite_settings_save",
            "lite_image_import",
            "lite_image_resolve",
            "set_desktop_wallpaper_fallback",
            "desktop_wallpaper_fallback_status",
            "notify_appearance_changed",
            "set_lock_screen_enabled",
            "clear_stale_lock_screen_backup",
            "get_lock_screen_diagnostics",
            "set_autostart",
            "autostart_status",
            "scan_dsh_paths",
            // Lists the harness execution subjects (shells with their own
            // checkout, and source trees) the shim can offer and start.
            "scan_harness_targets",
            // The last confirmed subject list, with when it was confirmed.
            "harness_target_catalog",
            // Starts the chosen execution subject (a shell, or a source tree) and
            // the unattended counterpart of that start.
            "launch_harness_target",
            // Idempotent 「拉起 UI」: start the subject if it is gone, show a window
            // the wallpaper hid, then bring it forward.
            "ensure_harness_ui",
            "autostart_harness_target",
            "launch_dsh",
            "autostart_managed_dsh",
            "managed_dsh_autostart_status",
            "managed_dsh_status",
            "stop_managed_dsh",
            "translucent_tb_status",
            "launch_translucent_tb",
            "open_translucent_tb_install",
            "open_windows_lock_screen_settings",
            // 设置中心直接输入 API Key（取代了原来的系统凭据对话框）。
            "save_api_key",
            "desktop_workspace_status",
            "open_project_memory",
            // 「打开 TUI」：在一个新终端窗口里拉起本机的 dst（设置中心专属）。
            "open_subject_tui",
            "clear_user_data",
            "api_key_status",
            "show_deepseek_login",
            "native_bootstrap_generation",
            "release_native_bootstrap",
            "deepseek_web_ensure",
            "deepseek_web_status",
            "deepseek_web_history",
            "deepseek_web_adapter_config_status",
            "open_deepseek_web_adapter_config",
            "reset_deepseek_web_adapter_config",
            "open_settings_window",
            "start_settings_drag",
            "hide_settings_window",
            "begin_interaction_region_session",
            "update_interaction_regions",
            // 悬浮球单击：进入里桌面（球是独立顶层窗口，只授予它这一条命令）。
            "enter_inner_workspace_from_ball",
            // 输入岛的「X」：离开里桌面。与桌面双击共用同一个原生实现——同一条状态迁移只能
            // 有一份，否则前端把界面搬回表桌面、原生那个事实原地不动，球就会一直以为岛还占着
            // 场面（实测：点 X 之后球再也弹不出来、也唤不起输入岛）。
            "leave_inner_workspace",
            "desktop_layout_metrics",
            "send_chat",
            "cancel_chat",
            "connect_harness",
            "harness_history",
            "harness_presets",
            "harness_set_preset",
            "harness_controls",
            // Model enumeration: the Harness side forwards the host's catalog through the
            // Bridge, the API side reads the compatible endpoint's own /models list.
            "harness_models",
            "api_models",
            // Pushes the wallpaper's model choice into the host, so the host's default
            // model follows the wallpaper instead of the two drifting apart.
            "harness_set_model",
            "harness_set_permission",
            // Verifies that the renderer's island pointerdown is a real user click before any
            // keyboard handover may act on it (repair plan 3.A).
            "verify_island_click",
            "api_history",
            "list_api_conversations",
            "delete_api_conversation",
            "clear_api_history",
            "probe_harness",
            "scan_harness_endpoints_command",
            "set_harness_endpoint",
            "raise_client_window",
            "open_client_in_browser",
            "open_external_link",
            "harness_endpoint_listening",
            "harness_endpoint_window",
            "appearance_get_state",
            "appearance_list_themes",
            "appearance_list_assets",
            "appearance_activate_theme",
            "appearance_set_override",
            "appearance_clear_override",
            "appearance_import_paths",
            "appearance_classify_asset",
            "appearance_export_current_theme",
            "appearance_resolve_asset",
            "appearance_resolve_library_asset",
            // 更新检测（完整版专属，见 src/update/ 的模块说明）。没登记在这里，命令就不会有
            // ACL 条目，界面每一次调用都会被能力系统拒掉 —— 0.3.1 那次就是这么发出去的。
            // `update_dismiss` 是气泡与设置页共用的那一条「忽略」；两条都要在窗口 capability
            // 里授权（permissions/autogenerated/ 与 capabilities/*.json 三处一起改）。
            "update_check",
            "update_dismiss",
        ]),
    ))
    .expect("failed to build Tauri application manifest")
}
