//! Very small native first-frame guard for the packaged wallpaper.
//!
//! Windows shows the Explorer desktop as soon as the secure lock screen is
//! dismissed.  A Tauri/WebView2 surface needs a little longer to create its
//! controller and paint its first frame.  This module fills only that short
//! gap with the immutable `sleep.png` image, then destroys itself when the
//! background WebView reports that it has painted.  It never accepts input,
//! never replaces the shell, and fails closed when the asset or WorkerW is
//! unavailable.

#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use std::sync::atomic::{AtomicIsize, Ordering};
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
    BeginPaint, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, SelectObject,
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
const RELEASE_MESSAGE: u32 = WM_APP + 0x5D;
#[cfg(windows)]
static BOOTSTRAP_HWND: AtomicIsize = AtomicIsize::new(0);
#[cfg(windows)]
static BOOTSTRAP_STARTED: OnceLock<Instant> = OnceLock::new();
#[cfg(windows)]
static BOOTSTRAP_READY_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(windows)]
static BOOTSTRAP_READY_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
struct BootstrapWindowState {
    bitmap: HBITMAP,
    width: i32,
    height: i32,
}

#[cfg(windows)]
fn elapsed_ms() -> u128 {
    BOOTSTRAP_STARTED
        .get()
        .map(|started| started.elapsed().as_millis())
        .unwrap_or_default()
}

#[cfg(windows)]
fn asset_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(
                parent
                    .join("_up_")
                    .join("public")
                    .join("personas")
                    .join("wake-frames")
                    .join("variant-anima")
                    .join("sleep.png"),
            );
            candidates.push(parent.join("Assets").join("LockScreenSleep.png"));
        }
    }
    candidates
}

#[cfg(windows)]
fn find_asset() -> Option<PathBuf> {
    asset_candidates().into_iter().find(|path| path.is_file())
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
                let mut client = RECT::default();
                if GetClientRect(hwnd, &mut client).is_ok() {
                    let width = client.right - client.left;
                    let height = client.bottom - client.top;
                    if width > 0 && height > 0 {
                        let memory = CreateCompatibleDC(Some(hdc));
                        if !memory.0.is_null() {
                            let previous = SelectObject(memory, HGDIOBJ(state.bitmap.0));
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
                                state.width,
                                state.height,
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
        RELEASE_MESSAGE => {
            log::info!("native bootstrap released after {} ms", elapsed_ms());
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BootstrapWindowState;
            if !state_ptr.is_null() {
                let state = Box::from_raw(state_ptr);
                if !state.bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(state.bitmap.0));
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
    let Some(path) = find_asset() else {
        log::warn!("native bootstrap skipped: packaged sleep image not found");
        return;
    };
    let worker = match request_wallpaper_worker() {
        Ok(worker) => worker,
        Err(error) => {
            log::warn!("native bootstrap skipped: {error}");
            return;
        }
    };
    let (bitmap, width, height) = match load_bitmap(&path) {
        Ok(bitmap) => bitmap,
        Err(error) => {
            log::warn!("native bootstrap skipped: {error}");
            return;
        }
    };
    if let Err(error) = register_class() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        log::warn!("native bootstrap skipped: {error}");
        return;
    }
    let state = Box::into_raw(Box::new(BootstrapWindowState {
        bitmap,
        width,
        height,
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
                let _ = DeleteObject(HGDIOBJ(state.bitmap.0));
            }
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
        "native bootstrap frame ready: asset={}, elapsed_ms={}, size={}x{}",
        path.display(),
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
        return Ok(());
    }
    unsafe {
        PostMessageW(
            Some(HWND(raw as *mut _)),
            RELEASE_MESSAGE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .map_err(|error| format!("无法释放原生首帧层：{error}"))
}

#[cfg(not(windows))]
pub fn prepare() {}

#[cfg(not(windows))]
pub fn report_ready() {}

#[cfg(not(windows))]
pub fn release() -> Result<(), String> {
    Ok(())
}
