#[cfg(windows)]
use tauri::{Emitter, Manager, WebviewWindow};

#[cfg(windows)]
use windows::{
    core::{BOOL, HSTRING, PCWSTR},
    Storage::StorageFile,
    System::UserProfile::{LockScreen, UserProfilePersonalizationSettings},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION}, Threading::GetCurrentProcessId},
        UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        UI::WindowsAndMessaging::{
            EnumWindows, FindWindowExW, FindWindowW, GetDesktopWindow, GetForegroundWindow,
            GetWindowLongPtrW, GetWindowThreadProcessId, PostMessageW, SetParent,
            SetWindowLongPtrW, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, WM_NCDESTROY, WM_POWERBROADCAST, WM_USER, WM_WTSSESSION_CHANGE,
            WTS_SESSION_LOCK, WTS_SESSION_UNLOCK, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            ShowWindow, SW_SHOWNA, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
            GetAncestor, GA_ROOT, SystemParametersInfoW, SPI_GETWORKAREA,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        },
    },
};

#[cfg(windows)]
unsafe extern "system" fn find_wallpaper_worker(window: HWND, lparam: LPARAM) -> BOOL {
    // 标准壁纸层算法：枚举 WorkerW，跳过「含 SHELLDLL_DefView 的图标层」，
    // 第一个不含它的 WorkerW 就是壁纸层宿主（Progman 收到 0x052C 后创建）。
    let mut class = [0u16; 64];
    let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(window, &mut class);
    if len == 0 { return BOOL(1); }
    let class_name = String::from_utf16_lossy(&class[..len as usize]);
    if class_name != "WorkerW" { return BOOL(1); }
    let shell_view = FindWindowExW(Some(window), None, windows::core::w!("SHELLDLL_DefView"), PCWSTR::null());
    if shell_view.is_err() {
        // 这个 WorkerW 不含桌面图标层 → 壁纸层宿主
        *(lparam.0 as *mut HWND) = window;
        return BOOL(0);
    }
    BOOL(1)
}

#[cfg(windows)]
pub fn force_fullscreen(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
    unsafe {
        let root = windows::Win32::UI::WindowsAndMessaging::GetAncestor(hwnd, windows::Win32::UI::WindowsAndMessaging::GA_ROOT);
        let target = if root.0.is_null() { hwnd } else { root };
        // 清除边框样式：WS_CAPTION|WS_THICKFRAME|WS_BORDER|WS_DLGFRAME|WS_MINIMIZEBOX|WS_MAXIMIZEBOX
        let style = GetWindowLongPtrW(target, GWL_STYLE);
        let border_mask = (0x00C00000u32 | 0x00040000u32 | 0x00800000u32 | 0x00400000u32 | 0x00020000u32 | 0x00010000u32) as isize;
        SetWindowLongPtrW(target, GWL_STYLE, style & !border_mask);
        // 工作区尺寸（不含任务栏）——壁纸覆盖工作区，任务栏保持可见
        let mut wa = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some(&mut wa as *mut _ as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let width = wa.right - wa.left;
        let height = wa.bottom - wa.top;
        log::info!("force_workarea: target=0x{:X} → {}x{}", target.0 as usize, width, height);
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            target, None, wa.left, wa.top, width, height,
            SWP_NOACTIVATE,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn force_fullscreen(_: &WebviewWindow) -> Result<(), String> { Ok(()) }

#[cfg(windows)]
pub fn attach_to_workerw(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
    unsafe {
        // 1) 扩展样式：TOOLWINDOW + NOACTIVATE（不抢焦点、不进任务栏）
        let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, exstyle | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize);
        // 2) 清除窗口边框样式（解决"四周有边框"）
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let border_mask = (0x00C00000u32 | 0x00040000u32 | 0x00800000u32 | 0x00400000u32 | 0x00020000u32 | 0x00010000u32) as isize;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !border_mask);
        // 3) 工作区尺寸（不含任务栏）+ Progman 之上（Z 序：桌面图标上、普通窗口下）
        //    注意：WebView2 挂 WorkerW 渲染灰屏（DComp 不兼容），故不挂载。
        let mut work_area = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut _ as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let width = work_area.right - work_area.left;
        let height = work_area.bottom - work_area.top;
        if let Ok(progman) = FindWindowW(windows::core::w!("Progman"), PCWSTR::null()) {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd, Some(progman), work_area.left, work_area.top, width, height,
                SWP_NOACTIVATE,
            );
        } else {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd, None, work_area.left, work_area.top, width, height,
                SWP_NOACTIVATE,
            );
        }
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA);
        // 4) 延迟守护线程：反复把窗口设为「工作区尺寸」（不含任务栏）+ Progman 之上。
        //    Tauri 可能在窗口初始化后恢复自身尺寸（fullscreen/初始尺寸），
        //    多次 SetWindowPos 确保最终工作区尺寸生效；同时 ±1px 触发 WebView2 重绘。
        let resize_hwnd = hwnd.0 as isize;
        std::thread::spawn(move || {
            for delay_ms in [500u64, 1200, 2500, 5000] {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                unsafe {
                    let target = HWND(resize_hwnd as *mut core::ffi::c_void);
                    let progman = FindWindowW(windows::core::w!("Progman"), PCWSTR::null())
                        .map(|h| h.0 as isize)
                        .unwrap_or(0);
                    let anchor = if progman != 0 { Some(HWND(progman as *mut core::ffi::c_void)) } else { None };
                    // 工作区尺寸（重新获取，防分辨率变化）
                    let mut wa = windows::Win32::Foundation::RECT::default();
                    let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                        windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
                        0,
                        Some(&mut wa as *mut _ as *mut core::ffi::c_void),
                        windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    );
                    let w = wa.right - wa.left;
                    let h = wa.bottom - wa.top;
                    log::info!("守护线程: work_area={}x{} anchor=0x{:X} target=0x{:X}", w, h, anchor.map(|a| a.0 as usize).unwrap_or(0), target.0 as usize);
                    // 先 ±1px 触发 WebView 重绘
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        target, anchor, wa.left, wa.top, w + 1, h,
                        SWP_NOACTIVATE,
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    // 再设回精确工作区尺寸
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        target, anchor, wa.left, wa.top, w, h,
                        SWP_NOACTIVATE,
                    );
                }
            }
        });
    }
    Ok(())
}

#[cfg(windows)]
pub fn register_session_events(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("background").ok_or("background window missing")?;
    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
    let app_ptr = Box::into_raw(Box::new(app.clone())) as usize;
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).map_err(|e| e.to_string())?;
        if !SetWindowSubclass(hwnd, Some(session_subclass_proc), 1, app_ptr).as_bool() {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            drop(Box::from_raw(app_ptr as *mut tauri::AppHandle));
            return Err("无法安装 Windows 会话消息处理器".into());
        }
    }
    let _ = app.emit("system-session", "resume");
    Ok(())
}

#[cfg(windows)]
unsafe extern "system" fn session_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    app_ptr: usize,
) -> LRESULT {
    let app = &*(app_ptr as *const tauri::AppHandle);
    match message {
        WM_WTSSESSION_CHANGE => match wparam.0 as u32 {
            WTS_SESSION_LOCK => { hide_interaction(app); let _ = app.emit("system-session", "locked"); }
            WTS_SESSION_UNLOCK => { let _ = app.emit("system-session", "unlocked"); }
            _ => {}
        },
        WM_POWERBROADCAST => match wparam.0 as u32 {
            PBT_APMSUSPEND => { hide_interaction(app); let _ = app.emit("system-session", "suspend"); }
            PBT_APMRESUMEAUTOMATIC => { let _ = app.emit("system-session", "resume"); }
            _ => {}
        },
        WM_NCDESTROY => {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(session_subclass_proc), subclass_id);
            drop(Box::from_raw(app_ptr as *mut tauri::AppHandle));
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

#[cfg(windows)]
pub fn show_interaction(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("interaction").ok_or("interaction window missing")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn hide_interaction(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("interaction") { let _ = window.hide(); }
}

#[cfg(windows)]
pub fn start_foreground_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut hidden_by_monitor = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(650));
            let foreground = unsafe { GetForegroundWindow() };
            let mut process_id = 0u32;
            if !foreground.0.is_null() { unsafe { GetWindowThreadProcessId(foreground, Some(&mut process_id)); } }
            let ours = process_id == unsafe { GetCurrentProcessId() };
            let shell = unsafe {
                foreground == GetDesktopWindow()
                    || FindWindowW(windows::core::w!("Progman"), PCWSTR::null()).map(|window| window == foreground).unwrap_or(false)
                    || FindWindowW(windows::core::w!("WorkerW"), PCWSTR::null()).map(|window| window == foreground).unwrap_or(false)
            };
            if !ours && !shell && !foreground.0.is_null() && !hidden_by_monitor {
                if app.get_webview_window("interaction").and_then(|window| window.is_visible().ok()).unwrap_or(false) {
                    hide_interaction(&app);
                    hidden_by_monitor = true;
                }
            } else if shell && hidden_by_monitor {
                let _ = show_interaction(&app);
                hidden_by_monitor = false;
            }
            if app.get_webview_window("background").is_none() { break; }
        }
    });
}

#[cfg(windows)]
pub async fn set_lock_screen(app: &tauri::AppHandle, enabled: bool) -> Result<String, String> {
    let backup_file = app.path().app_config_dir().map_err(|e| e.to_string())?.join("lock-screen-backup.txt");
    if enabled {
        if !UserProfilePersonalizationSettings::IsSupported().map_err(|e| e.to_string())? { return Err("当前 Windows 策略不允许应用修改锁屏图片".into()); }
        if !backup_file.exists() {
            if let Ok(uri) = LockScreen::OriginalImageFile() {
                if let Ok(original) = uri.AbsoluteUri() {
                    if let Some(parent) = backup_file.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                    std::fs::write(&backup_file, original.to_string()).map_err(|e| e.to_string())?;
                }
            }
        }
        let path = app.path().resolve("personas/wake-frames/variant-anima/sleep.png", tauri::path::BaseDirectory::Resource).map_err(|e| e.to_string())?;
        let path_string = HSTRING::from(path.to_string_lossy().as_ref());
        let file = StorageFile::GetFileFromPathAsync(&path_string).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())?;
        let settings = UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
        let changed = settings.TrySetLockScreenImageAsync(&file).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())?;
        if !changed { return Err("Windows 或组织策略拒绝修改锁屏图片".into()); }
        Ok("锁屏图片已设置；密码页继续由 Windows 原生模糊处理。".into())
    } else {
        let original = std::fs::read_to_string(&backup_file).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(original) = original {
            let path = original.strip_prefix("file:///").or_else(|| original.strip_prefix("file://")).unwrap_or(&original).replace('/', "\\");
            if std::path::Path::new(&path).exists() {
                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path)).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())?;
                let settings = UserProfilePersonalizationSettings::Current().map_err(|e| e.to_string())?;
                if settings.TrySetLockScreenImageAsync(&file).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())? {
                    let _ = std::fs::remove_file(&backup_file);
                    return Ok("已恢复接管前的静态锁屏图片。".into());
                }
            }
        }
        Ok("已停止接管后续锁屏图片；未能恢复原静态图片。Spotlight 状态无法由 Windows API 可靠备份和恢复。".into())
    }
}

#[cfg(not(windows))]
pub fn attach_to_workerw(_: &tauri::WebviewWindow) -> Result<(), String> { Err("WorkerW only exists on Windows".into()) }
#[cfg(not(windows))]
pub fn register_session_events(_: &tauri::AppHandle) -> Result<(), String> { Ok(()) }
#[cfg(not(windows))]
pub async fn set_lock_screen(_: &tauri::AppHandle, _: bool) -> Result<String, String> { Err("Lock screen integration only supports Windows".into()) }
#[cfg(not(windows))]
pub fn show_interaction(_: &tauri::AppHandle) -> Result<(), String> { Ok(()) }
#[cfg(not(windows))]
pub fn hide_interaction(_: &tauri::AppHandle) {}
#[cfg(not(windows))]
pub fn start_foreground_monitor(_: tauri::AppHandle) {}
