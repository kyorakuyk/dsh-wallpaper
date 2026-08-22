fn main() {
    // Application commands do not receive an ACL entry unless they are
    // declared here. Keep this list in lockstep with `generate_handler!` so
    // capabilities can grant a command to one WebView without implicitly
    // granting it to every window in the application.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_snapshot",
            "set_interaction_enabled",
            "select_backend",
            "dispatch_app_action",
            "publish_settings",
            "notify_appearance_changed",
            "set_lock_screen_enabled",
            "clear_stale_lock_screen_backup",
            "get_lock_screen_diagnostics",
            "set_autostart",
            "translucent_tb_status",
            "launch_translucent_tb",
            "open_translucent_tb_install",
            "open_windows_lock_screen_settings",
            "prompt_for_api_key",
            "show_deepseek_login",
            "start_settings_drag",
            "hide_settings_window",
            "begin_interaction_region_session",
            "update_interaction_regions",
            "desktop_layout_metrics",
            "send_chat",
            "cancel_chat",
            "connect_harness",
            "harness_history",
            "harness_presets",
            "harness_controls",
            "harness_set_permission",
            "api_history",
            "probe_harness",
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
        ]),
    ))
    .expect("failed to build Tauri application manifest")
}
