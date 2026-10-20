// harness 命令按职责分文件；这里只留模块声明与重导出。
pub(crate) mod discovery;
pub(crate) mod managed;
pub(crate) mod session;
pub(crate) mod ui;

// 测试搬进 tests.rs，模块名保持 `dsh_autostart_tests`。
#[cfg(test)]
#[cfg(not(feature = "lite"))]
#[path = "tests.rs"]
mod dsh_autostart_tests;

// D 组（discovery）：外部仍按 commands::harness::<名字> 引用扫描与候选类型。
#[allow(unused_imports)]
pub(crate) use discovery::{harness_target_catalog, scan_dsh_paths, scan_harness_targets};
pub(crate) use discovery::{scan_dsh_paths_blocking, DshPathCandidate};

// M 组（managed）：类型与纯函数被 harness_launch / harness_targets / lib.rs 直接调用。
pub(crate) use managed::{
    classify_dsh_launch_failure, shutdown_managed_dsh, spawn_managed_dsh, ManagedDshAutostartState,
    ManagedDshState,
};
#[allow(unused_imports)]
pub(crate) use managed::{
    autostart_managed_dsh, launch_dsh, managed_dsh_autostart_status, managed_dsh_status,
    stop_managed_dsh, ManagedDshStatus,
};

// U 组（ui）：注册走子模块路径；重导出只为保持 commands::harness::<命令名> 这个名字。
#[allow(unused_imports)]
pub(crate) use ui::{
    autostart_harness_target, ensure_harness_ui, ensure_profile_bridge, launch_harness_target,
};

// 重导出保持外部看到的是 commands::harness::<命令名>（决定 4）。
// handler 无法穿过重导出找到 #[tauri::command] 生成的宏，故注册改用子模块路径（决定 4 允许的例外）。
#[allow(unused_imports)]
pub(crate) use session::{
    connect_harness, harness_controls, harness_history, harness_models, harness_presets,
    harness_set_model, harness_set_permission, harness_set_preset,
};
