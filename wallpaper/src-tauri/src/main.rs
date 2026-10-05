#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 启动时间线的锚点：整个进程里最早能记的位置，就在这一行。只读时钟，不改变任何
    // 启动顺序；它量的是「进程被 Windows 拉起来的那一刻」到后面各个时间点的毫秒差。
    dsh_wallpaper_lib::startup_timeline_entry();
    // Desktop repair helper: a separate, short-lived invocation of this same
    // binary whose only job is to undo the desktop changes a crashed wallpaper
    // left behind. Handled before any Tauri/Tao/WebView initialisation, because
    // the point is that it works while the wallpaper is gone and stays as small
    // and as harmless as possible.
    //
    // It performs one idempotent repair and exits, and never restarts the
    // wallpaper — so a crash cannot become a restart loop.
    if dsh_wallpaper_lib::desktop_repair::is_repair_invocation() {
        let parent = std::env::args()
            .nth(2)
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let outcome = dsh_wallpaper_lib::desktop_repair::run_repair_helper(parent);
        dsh_wallpaper_lib::desktop_repair::record_repair_outcome(&outcome);
        return;
    }

    // 在 Tauri/Tao 初始化前设置 Per-Monitor V2 DPI 感知（进程最早入口）。
    // 否则 Windows 做 DPI 虚拟化，屏幕被当作 1707x1067，物理全屏无法正确建立；
    // V2 还会让混合缩放显示器收到正确的 DPI 变更通知。
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            // Keep a safe fallback for older Windows builds where V2 is not
            // available; the process must remain non-virtualized.
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
        }
    }
    dsh_wallpaper_lib::prepare_native_bootstrap();
    dsh_wallpaper_lib::run()
}
