#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Keep DPI handling identical to the full edition. Without per-monitor V2
    // awareness Windows can virtualize the WorkerW size and leave a border;
    // V2 is also needed when the user mixes display scaling factors.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
        }
    }
    dsh_wallpaper_lib::prepare_native_bootstrap();
    dsh_wallpaper_lib::run_lite();
}
