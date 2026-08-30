//! Very small native first-frame guard for the packaged wallpaper.
//!
//! Windows shows the Explorer desktop as soon as the secure lock screen is
//! dismissed.  A Tauri/WebView2 surface needs a little longer to create its
//! controller and paint its first frame.  This module fills only that short
//! gap with immutable packaged artwork.  It stays as a hidden, inputless
//! hand-off surface after the first paint so a later session unlock can show a
//! cached eye-open frame before WebView2 reacts. It never replaces the shell
//! and fails closed when the asset or WorkerW is unavailable.

#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
#[cfg(windows)]
use std::sync::OnceLock;
#[cfg(windows)]
use std::time::Instant;

#[cfg(windows)]
use windows::core::{w, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, InvalidateRect, SelectObject,
    SetStretchBltMode, StretchBlt, HALFTONE, HBITMAP, HGDIOBJ, PAINTSTRUCT, SRCCOPY,
};
#[cfg(windows)]
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateBitmapFromFile, GdipCreateHBITMAPFromBitmap, GdipDisposeImage, GdipGetImageHeight,
    GdipGetImageWidth, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, GpBitmap, GpImage,
};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::System::Threading::{
    GetCurrentProcess, SetPriorityClass, ABOVE_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, PostMessageW,
    RegisterClassExW, SetWindowLongPtrW, SetWindowPos, ShowWindow, CREATESTRUCTW, GWLP_USERDATA,
    SWP_NOACTIVATE, SW_HIDE, SW_SHOWNA, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_ERASEBKGND,
    WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WNDCLASSEXW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};

#[cfg(windows)]
use crate::windows_integration::request_wallpaper_worker;

#[cfg(windows)]
const BOOTSTRAP_CLASS: PCWSTR = w!("DSHWallpaperNativeBootstrap");
#[cfg(windows)]
const HIDE_MESSAGE: u32 = WM_APP + 0x5D;
#[cfg(windows)]
const SHOW_WAKE_MESSAGE: u32 = WM_APP + 0x5E;
#[cfg(windows)]
const SHOW_SLEEP_MESSAGE: u32 = WM_APP + 0x5F;
#[cfg(windows)]
const DESTROY_MESSAGE: u32 = WM_APP + 0x60;
#[cfg(windows)]
static BOOTSTRAP_HWND: AtomicIsize = AtomicIsize::new(0);
#[cfg(windows)]
static STARTUP_PRIORITY_BOOSTED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static BOOTSTRAP_STARTED: OnceLock<Instant> = OnceLock::new();
#[cfg(windows)]
static BOOTSTRAP_READY_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(windows)]
static BOOTSTRAP_READY_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
struct BootstrapWindowState {
    sleep_bitmap: HBITMAP,
    sleep_width: i32,
    sleep_height: i32,
    wake_bitmap: Option<HBITMAP>,
    wake_width: i32,
    wake_height: i32,
    show_wake: bool,
}

#[cfg(windows)]
const SLEEP_ASSET: &str = "personas/wake-frames/variant-anima/sleep.png";
#[cfg(windows)]
const WAKE_ASSET: &str = "personas/wake-frames/variant-anima/frame-2-eyes.png";

#[cfg(windows)]
fn elapsed_ms() -> u128 {
    BOOTSTRAP_STARTED
        .get()
        .map(|started| started.elapsed().as_millis())
        .unwrap_or_default()
}

/// Give only the short startup/handoff window a modest scheduling boost.  Do
/// not use REALTIME_PRIORITY_CLASS: a wallpaper must never starve Explorer or
/// user applications. The boost is returned to normal as soon as the native
/// frame is handed to the WebView.
#[cfg(windows)]
pub fn boost_startup_priority() {
    if STARTUP_PRIORITY_BOOSTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Err(error) = unsafe { SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS) }
    {
        STARTUP_PRIORITY_BOOSTED.store(false, Ordering::Release);
        log::debug!("startup priority boost unavailable: {error}");
    }
}

#[cfg(windows)]
pub fn restore_startup_priority() {
    if !STARTUP_PRIORITY_BOOSTED.swap(false, Ordering::AcqRel) {
        return;
    }
    if let Err(error) = unsafe { SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS) } {
        log::debug!("startup priority restore failed: {error}");
    }
}

#[cfg(windows)]
fn asset_candidates(relative: &str, package_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join("_up_").join("public").join(relative));
            // The isolated MSIX test layout places the Vite output directly
            // under `dist`, while a normal Tauri resource bundle keeps it in
            // `_up_/public`; support both without probing arbitrary paths.
            candidates.push(parent.join("dist").join(relative));
            candidates.push(parent.join("Assets").join(package_name));
        }
    }
    // Development/NSIS builds keep the canonical public tree beside the
    // Tauri crate. This is a best-effort fallback only; lock-screen writes
    // remain MSIX-gated in the owning integration module.
    if let Some(root) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        candidates.push(root.join("public").join(relative));
    }
    candidates
}

#[cfg(windows)]
fn find_asset(relative: &str, package_name: &str) -> Option<PathBuf> {
    asset_candidates(relative, package_name)
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(windows)]
fn to_wide(path: &Path) -> Vec<u16> {
    path.to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn load_bitmap(path: &Path) -> Result<(HBITMAP, i32, i32), String> {
    let wide = to_wide(path);
    unsafe {
        let mut token = 0usize;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        let startup = GdiplusStartup(&mut token, &input, null_mut());
        if startup.0 != 0 {
            return Err(format!("GDI+ 初始化失败（状态码 {}）", startup.0));
        }

        let mut image: *mut GpBitmap = null_mut();
        let loaded = GdipCreateBitmapFromFile(PCWSTR(wide.as_ptr()), &mut image);
        if loaded.0 != 0 || image.is_null() {
            GdiplusShutdown(token);
            return Err(format!("无法读取首帧图片（状态码 {}）", loaded.0));
        }

        let mut width = 0u32;
        let mut height = 0u32;
        let dimensions_ok = GdipGetImageWidth(image as *mut GpImage, &mut width).0 == 0
            && GdipGetImageHeight(image as *mut GpImage, &mut height).0 == 0
            && width > 0
            && height > 0;
        if !dimensions_ok {
            let _ = GdipDisposeImage(image as *mut GpImage);
            GdiplusShutdown(token);
            return Err("首帧图片尺寸无效".into());
        }

        let mut bitmap = HBITMAP::default();
        let converted = GdipCreateHBITMAPFromBitmap(image as *mut _, &mut bitmap, 0);
        let _ = GdipDisposeImage(image as *mut GpImage);
        GdiplusShutdown(token);
        if converted.0 != 0 || bitmap.0.is_null() {
            return Err(format!("无法准备首帧位图（状态码 {}）", converted.0));
        }
        Ok((bitmap, width as i32, height as i32))
    }
}

#[cfg(windows)]
unsafe extern "system" fn bootstrap_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            LRESULT(1)
        }
        WM_PAINT => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BootstrapWindowState;
            let mut paint = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut paint);
            if !state_ptr.is_null() && !hdc.0.is_null() {
                let state = &*state_ptr;
                let (bitmap, source_width, source_height) = if state.show_wake {
                    state
                        .wake_bitmap
                        .map(|bitmap| (bitmap, state.wake_width, state.wake_height))
                        .unwrap_or((state.sleep_bitmap, state.sleep_width, state.sleep_height))
                } else {
                    (state.sleep_bitmap, state.sleep_width, state.sleep_height)
                };
                let mut client = RECT::default();
                if GetClientRect(hwnd, &mut client).is_ok() {
                    let width = client.right - client.left;
                    let height = client.bottom - client.top;
                    if width > 0 && height > 0 {
                        let memory = CreateCompatibleDC(Some(hdc));
                        if !memory.0.is_null() {
                            let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
                            let _ = SetStretchBltMode(hdc, HALFTONE);
                            let _ = StretchBlt(
                                hdc,
                                0,
                                0,
                                width,
                                height,
                                Some(memory),
                                0,
                                0,
                                source_width,
                                source_height,
                                SRCCOPY,
                            );
                            let _ = SelectObject(memory, previous);
                            let _ = DeleteDC(memory);
                        }
                    }
                }
            }
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        SHOW_WAKE_MESSAGE => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BootstrapWindowState;
            if !state_ptr.is_null() {
                (*state_ptr).show_wake = (*state_ptr).wake_bitmap.is_some();
            }
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        SHOW_SLEEP_MESSAGE => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BootstrapWindowState;
            if !state_ptr.is_null() {
                (*state_ptr).show_wake = false;
            }
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        HIDE_MESSAGE => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BootstrapWindowState;
            if !state_ptr.is_null() {
                (*state_ptr).show_wake = false;
            }
            let _ = ShowWindow(hwnd, SW_HIDE);
            restore_startup_priority();
            log::info!("native bootstrap hidden after {} ms", elapsed_ms());
            LRESULT(0)
        }
        DESTROY_MESSAGE => {
            log::info!("native bootstrap destroyed after {} ms", elapsed_ms());
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = DestroyWindow(hwnd);
            restore_startup_priority();
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BootstrapWindowState;
            if !state_ptr.is_null() {
                let state = Box::from_raw(state_ptr);
                if !state.sleep_bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(state.sleep_bitmap.0));
                }
                if let Some(bitmap) = state.wake_bitmap {
                    if !bitmap.0.is_null() {
                        let _ = DeleteObject(HGDIOBJ(bitmap.0));
                    }
                }
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            BOOTSTRAP_HWND.store(0, Ordering::Release);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

#[cfg(windows)]
fn register_class() -> Result<(), String> {
    let instance = unsafe {
        GetModuleHandleW(None)
            .map(|module| HINSTANCE(module.0))
            .unwrap_or_default()
    };
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(bootstrap_window_proc),
        hInstance: instance,
        lpszClassName: BOOTSTRAP_CLASS,
        ..Default::default()
    };
    let result = unsafe { RegisterClassExW(&class) };
    if result == 0 {
        // A second call in the same process is harmless; the class is global
        // to the module and may already have been registered by a retry.
        let error = unsafe { windows::Win32::Foundation::GetLastError() };
        if error.0 != 1410 {
            return Err(format!("无法注册首帧窗口类（错误码 {}）", error.0));
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn prepare() {
    if BOOTSTRAP_HWND.load(Ordering::Acquire) != 0 {
        return;
    }
    let _ = BOOTSTRAP_STARTED.set(Instant::now());
    let Some(path) = find_asset(SLEEP_ASSET, "LockScreenSleep.png") else {
        log::warn!("native bootstrap skipped: packaged sleep image not found");
        return;
    };
    boost_startup_priority();
    let worker = match request_wallpaper_worker() {
        Ok(worker) => worker,
        Err(error) => {
            restore_startup_priority();
            log::warn!("native bootstrap skipped: {error}");
            return;
        }
    };
    let (bitmap, width, height) = match load_bitmap(&path) {
        Ok(bitmap) => bitmap,
        Err(error) => {
            restore_startup_priority();
            log::warn!("native bootstrap skipped: {error}");
            return;
        }
    };
    let wake_bitmap = find_asset(WAKE_ASSET, "WakeFrame2Eyes.png").and_then(|wake_path| {
        match load_bitmap(&wake_path) {
            Ok(bitmap) => Some(bitmap),
            Err(error) => {
                log::debug!("native wake frame preload skipped: {error}");
                None
            }
        }
    });
    if let Err(error) = register_class() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            if let Some((wake_bitmap, _, _)) = wake_bitmap.as_ref() {
                let _ = DeleteObject(HGDIOBJ(wake_bitmap.0));
            }
        }
        restore_startup_priority();
        log::warn!("native bootstrap skipped: {error}");
        return;
    }
    let (wake_handle, wake_width, wake_height) = wake_bitmap
        .map(|(bitmap, width, height)| (Some(bitmap), width, height))
        .unwrap_or((None, width, height));
    let state = Box::into_raw(Box::new(BootstrapWindowState {
        sleep_bitmap: bitmap,
        sleep_width: width,
        sleep_height: height,
        wake_bitmap: wake_handle,
        wake_width,
        wake_height,
        show_wake: false,
    }));
    let exstyle = WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0);
    let style = WINDOW_STYLE(WS_CHILD.0);
    let hwnd = unsafe {
        CreateWindowExW(
            exstyle,
            BOOTSTRAP_CLASS,
            w!("DSH Wallpaper bootstrap"),
            style,
            0,
            0,
            1,
            1,
            Some(worker),
            None,
            Some(HINSTANCE::default()),
            Some(state.cast()),
        )
    };
    let hwnd = match hwnd {
        Ok(hwnd) => hwnd,
        Err(error) => {
            unsafe {
                let state = Box::from_raw(state);
                let _ = DeleteObject(HGDIOBJ(state.sleep_bitmap.0));
                if let Some(bitmap) = state.wake_bitmap {
                    let _ = DeleteObject(HGDIOBJ(bitmap.0));
                }
            }
            restore_startup_priority();
            log::warn!("native bootstrap skipped: 无法创建首帧窗口（{error}）");
            return;
        }
    };
    let mut bounds = RECT::default();
    let sized = unsafe { GetClientRect(worker, &mut bounds).is_ok() };
    let width = (bounds.right - bounds.left).max(1);
    let height = (bounds.bottom - bounds.top).max(1);
    let positioned = unsafe {
        SetWindowPos(
            hwnd,
            Some(windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM),
            0,
            0,
            if sized { width } else { 1 },
            if sized { height } else { 1 },
            SWP_NOACTIVATE,
        )
        .is_ok()
    };
    if !positioned {
        log::warn!("native bootstrap created but could not size to WorkerW");
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    BOOTSTRAP_HWND.store(hwnd.0 as isize, Ordering::Release);
    BOOTSTRAP_READY_MS.store(elapsed_ms().min(u64::MAX as u128) as u64, Ordering::Release);
    log::info!(
        "native bootstrap frame ready: asset={}, wake_frame_cached={}, elapsed_ms={}, size={}x{}",
        path.display(),
        wake_handle.is_some(),
        elapsed_ms(),
        width,
        height
    );
}

/// The logger plugin is initialized after `main()` enters the Tauri builder,
/// so the earliest `prepare()` log can be lost. Emit the measured ready time
/// once the application logger is live.
#[cfg(windows)]
pub fn report_ready() {
    let ready_ms = BOOTSTRAP_READY_MS.load(Ordering::Acquire);
    if ready_ms > 0 && !BOOTSTRAP_READY_REPORTED.swap(true, Ordering::AcqRel) {
        log::info!("native bootstrap frame ready: elapsed_ms={}", ready_ms);
    }
}

#[cfg(windows)]
pub fn release() -> Result<(), String> {
    let raw = BOOTSTRAP_HWND.load(Ordering::Acquire);
    if raw == 0 {
        restore_startup_priority();
        return Ok(());
    }
    unsafe {
        PostMessageW(
            Some(HWND(raw as *mut _)),
            HIDE_MESSAGE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .map_err(|error| format!("无法释放原生首帧层：{error}"))
}

/// Display the cached eye-open frame without waiting for WebView2. The
/// background renderer hides the surface again after painting its matching
/// frame. Keeping the HWND alive makes this useful for every subsequent
/// lock/unlock cycle, not only the first process launch.
#[cfg(windows)]
pub fn start_wake() -> Result<(), String> {
    let raw = BOOTSTRAP_HWND.load(Ordering::Acquire);
    if raw == 0 {
        return Ok(());
    }
    unsafe {
        PostMessageW(
            Some(HWND(raw as *mut _)),
            SHOW_WAKE_MESSAGE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .map_err(|error| format!("无法显示原生苏醒首帧：{error}"))
}

/// Reset the hand-off layer to the sleep frame for lock/suspend transitions.
#[cfg(windows)]
pub fn show_sleep() -> Result<(), String> {
    let raw = BOOTSTRAP_HWND.load(Ordering::Acquire);
    if raw == 0 {
        return Ok(());
    }
    unsafe {
        PostMessageW(
            Some(HWND(raw as *mut _)),
            SHOW_SLEEP_MESSAGE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .map_err(|error| format!("无法恢复原生睡眠首帧：{error}"))
}

#[cfg(not(windows))]
pub fn prepare() {}

#[cfg(not(windows))]
pub fn report_ready() {}

#[cfg(not(windows))]
pub fn boost_startup_priority() {}

#[cfg(not(windows))]
pub fn restore_startup_priority() {}

#[cfg(not(windows))]
pub fn release() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn start_wake() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn show_sleep() -> Result<(), String> {
    Ok(())
}
