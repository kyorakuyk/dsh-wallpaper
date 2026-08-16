#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 在 Tauri/Tao 初始化前设置 Per-Monitor DPI 感知（进程最早入口）。
    // 否则 Windows 做 DPI 虚拟化，屏幕被当作 1707x1067，物理全屏无法正确建立。
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
    }
    dsh_wallpaper_lib::run()
}
