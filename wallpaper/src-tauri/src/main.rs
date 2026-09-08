#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
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
