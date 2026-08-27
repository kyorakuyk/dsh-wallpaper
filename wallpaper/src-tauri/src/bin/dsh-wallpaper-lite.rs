#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Keep DPI handling identical to the full edition. Without per-monitor
    // awareness Windows can virtualize the WorkerW size and leave a border.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
    }
    dsh_wallpaper_lib::prepare_native_bootstrap();
    dsh_wallpaper_lib::run_lite();
}
