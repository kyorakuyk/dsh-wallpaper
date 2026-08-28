// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT
//
// Locally modified by dsh-wallpaper. See ../../NOTICE.md.

#[cfg(feature = "semver")]
use crate::semver_compat::semver_compat_string;

use crate::SingleInstanceCallback;
use std::time::{Duration, Instant};
use tauri::{
    plugin::{self, TauriPlugin},
    AppHandle, Manager, RunEvent, Runtime,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
        WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        DataExchange::COPYDATASTRUCT,
        LibraryLoader::GetModuleHandleW,
        Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    },
    UI::WindowsAndMessaging::{
        self as w32wm, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW,
        RegisterClassExW, SendMessageTimeoutW, CREATESTRUCTW, GWLP_USERDATA, GWL_STYLE,
        SMTO_ABORTIFHUNG, WINDOW_LONG_PTR_INDEX, WM_COPYDATA, WM_CREATE, WM_DESTROY,
        WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
        WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
    },
};

const WMCOPYDATA_SINGLE_INSTANCE_DATA: usize = 1542;
// This application only needs a second process to activate the already
// running instance.  Keeping the protocol fixed avoids forwarding arbitrary
// command-line data across a window message boundary.
const ACTIVATION_PAYLOAD: &[u8] = b"activate\0";
const MAX_COPYDATA_BYTES: usize = ACTIVATION_PAYLOAD.len();
const WAIT_SLICE: Duration = Duration::from_millis(50);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const FORWARD_TIMEOUT_MS: u32 = 1_000;

struct MutexHandle {
    raw: isize,
    owns_mutex: bool,
}

struct TargetWindowHandle(isize);

struct UserData<R: Runtime> {
    app: AppHandle<R>,
    callback: Box<SingleInstanceCallback<R>>,
}

impl<R: Runtime> UserData<R> {
    unsafe fn from_hwnd_raw(hwnd: HWND) -> *mut Self {
        GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Self
    }

    unsafe fn from_hwnd<'a>(hwnd: HWND) -> &'a mut Self {
        &mut *Self::from_hwnd_raw(hwnd)
    }

    fn run_callback(&mut self, args: Vec<String>, cwd: String) {
        (self.callback)(&self.app, args, cwd)
    }
}

enum StartupOutcome {
    Primary { owns_mutex: bool },
    SecondaryForwarded,
}

pub fn init<R: Runtime>(callback: Box<SingleInstanceCallback<R>>) -> TauriPlugin<R> {
    plugin::Builder::new("single-instance")
        .setup(|app, _api| {
            #[allow(unused_mut)]
            let mut id = app.config().identifier.clone();
            #[cfg(feature = "semver")]
            {
                id.push('_');
                id.push_str(semver_compat_string(&app.package_info().version).as_str());
            }

            let class_name = encode_wide(format!("{id}-sic"));
            let window_name = encode_wide(format!("{id}-siw"));
            let mutex_name = encode_wide(format!("{id}-sim"));

            let hmutex = unsafe { CreateMutexW(std::ptr::null(), true.into(), mutex_name.as_ptr()) };
            if hmutex.is_null() {
                return Err(Box::<dyn std::error::Error>::from(
                    "single-instance: CreateMutexW failed",
                ));
            }

            let outcome = match unsafe { GetLastError() } {
                ERROR_ALREADY_EXISTS => wait_for_primary_or_take_mutex(
                    hmutex as isize,
                    &class_name,
                    &window_name,
                ),
                _ => Ok(StartupOutcome::Primary { owns_mutex: true }),
            };

            match outcome {
                Ok(StartupOutcome::SecondaryForwarded) => {
                    unsafe { CloseHandle(hmutex) };
                    app.cleanup_before_exit();
                    std::process::exit(0);
                }
                Ok(StartupOutcome::Primary { owns_mutex }) => {
                    if let Err(error) = install_primary_target(
                        app,
                        hmutex as isize,
                        owns_mutex,
                        &class_name,
                        &window_name,
                        callback,
                    ) {
                        unsafe {
                            if owns_mutex {
                                ReleaseMutex(hmutex);
                            }
                            CloseHandle(hmutex);
                        }
                        return Err(error);
                    }
                }
                Err(error) => {
                    unsafe { CloseHandle(hmutex) };
                    return Err(error);
                }
            }

            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                destroy(app);
            }
        })
        .build()
}

fn install_primary_target<R: Runtime>(
    app: &AppHandle<R>,
    hmutex: isize,
    owns_mutex: bool,
    class_name: &[u16],
    window_name: &[u16],
    callback: Box<SingleInstanceCallback<R>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let userdata = Box::into_raw(Box::new(UserData {
        app: app.clone(),
        callback,
    }));
    let hwnd = create_event_target_window::<R>(class_name, window_name, userdata);
    if hwnd.is_null() {
        // CreateWindowExW never took ownership if it failed.
        unsafe { drop(Box::from_raw(userdata)) };
        return Err("single-instance: failed to create IPC target window".into());
    }

    app.manage(MutexHandle { raw: hmutex, owns_mutex });
    app.manage(TargetWindowHandle(hwnd as _));
    Ok(())
}

fn wait_for_primary_or_take_mutex(
    hmutex: isize,
    class_name: &[u16],
    window_name: &[u16],
) -> Result<StartupOutcome, Box<dyn std::error::Error>> {
    let started = Instant::now();

    while started.elapsed() < STARTUP_TIMEOUT {
        let hwnd = unsafe { FindWindowW(class_name.as_ptr(), window_name.as_ptr()) };
        if !hwnd.is_null() {
            forward_to_primary(hwnd)?;
            return Ok(StartupOutcome::SecondaryForwarded);
        }

        let wait = unsafe { WaitForSingleObject(hmutex as _, WAIT_SLICE.as_millis() as u32) };
        match wait {
            WAIT_TIMEOUT => continue,
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                // The former owner exited before publishing its IPC window (or
                // abandoned the mutex). This thread now owns it and may safely
                // become the primary instance.
                return Ok(StartupOutcome::Primary { owns_mutex: true });
            }
            WAIT_FAILED => {
                return Err("single-instance: WaitForSingleObject failed".into());
            }
            _ => {
                return Err("single-instance: unexpected mutex wait result".into());
            }
        }
    }

    // A mutex without a usable IPC target is ambiguous (the target could have
    // been externally destroyed). Do not launch a second resident wallpaper
    // host or tray icon in this state.
    Err("single-instance: existing instance did not publish its IPC target in time".into())
}

fn forward_to_primary(hwnd: HWND) -> Result<(), Box<dyn std::error::Error>> {
    let cds = COPYDATASTRUCT {
        dwData: WMCOPYDATA_SINGLE_INSTANCE_DATA,
        cbData: ACTIVATION_PAYLOAD.len() as _,
        lpData: ACTIVATION_PAYLOAD.as_ptr() as _,
    };
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            0,
            &cds as *const _ as _,
            SMTO_ABORTIFHUNG,
            FORWARD_TIMEOUT_MS,
            &mut result,
        )
    };
    if sent == 0 || result == 0 {
        return Err("single-instance: existing instance did not accept launch forwarding".into());
    }
    Ok(())
}

/// Validates the complete on-wire activation message before a raw pointer is
/// ever interpreted as text.  The explicit terminator and UTF-8 checks keep
/// the receiver safe even if a different process sends WM_COPYDATA directly.
fn is_valid_activation_payload(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > MAX_COPYDATA_BYTES {
        return false;
    }
    let Some(command) = bytes.strip_suffix(&[0]) else {
        return false;
    };
    if command.contains(&0) {
        return false;
    }
    matches!(std::str::from_utf8(command), Ok("activate"))
}

/// `WM_COPYDATA` memory is owned by the sender and remains valid only for the
/// duration of its synchronous SendMessage call.  Keep the unsafe access
/// narrow, validate both the structure and byte count, and consume the slice
/// immediately.
unsafe fn is_valid_copydata_activation(cds_ptr: *const COPYDATASTRUCT) -> bool {
    let Some(cds) = cds_ptr.as_ref() else {
        return false;
    };
    if cds.dwData != WMCOPYDATA_SINGLE_INSTANCE_DATA || cds.lpData.is_null() {
        return false;
    }
    let Ok(byte_count) = usize::try_from(cds.cbData) else {
        return false;
    };
    if !(1..=MAX_COPYDATA_BYTES).contains(&byte_count) {
        return false;
    }
    let bytes = std::slice::from_raw_parts(cds.lpData.cast::<u8>(), byte_count);
    is_valid_activation_payload(bytes)
}

pub fn destroy<R: Runtime, M: Manager<R>>(manager: &M) {
    if let Some(hwnd) = manager.try_state::<TargetWindowHandle>() {
        if hwnd.0 != 0 {
            unsafe { DestroyWindow(hwnd.0 as _) };
        }
    }
    if let Some(hmutex) = manager.try_state::<MutexHandle>() {
        unsafe {
            if hmutex.owns_mutex {
                ReleaseMutex(hmutex.raw as _);
            }
            CloseHandle(hmutex.raw as _);
        }
    }
}

unsafe extern "system" fn single_instance_window_proc<R: Runtime>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam as *const CREATESTRUCTW);
            let userdata = create_struct.lpCreateParams as *const UserData<R>;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, userdata as _);
            0
        }

        WM_COPYDATA => {
            let cds_ptr = lparam as *const COPYDATASTRUCT;
            if !is_valid_copydata_activation(cds_ptr) {
                return 0;
            }
            let userdata = UserData::<R>::from_hwnd_raw(hwnd);
            if userdata.is_null() {
                return 0;
            }
            (*userdata).run_callback(Vec::new(), String::new());
            1
        }

        WM_DESTROY => {
            let userdata = UserData::<R>::from_hwnd_raw(hwnd);
            if !userdata.is_null() {
                drop(Box::from_raw(userdata));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn create_event_target_window<R: Runtime>(
    class_name: &[u16],
    window_name: &[u16],
    userdata: *const UserData<R>,
) -> HWND {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(single_instance_window_proc::<R>),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };

        RegisterClassExW(&class);

        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE
                | WS_EX_TRANSPARENT
                | WS_EX_LAYERED
                // Keep the internal IPC target out of the taskbar, including
                // after an Explorer restart.
                | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            userdata as _,
        );
        if !hwnd.is_null() {
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                // The target must be visible to receive WM_COPYDATA, while the
                // layered/tool-window styles keep it invisible to users.
                (WS_VISIBLE | WS_POPUP) as isize,
            );
        }
        hwnd
    }
}

pub fn encode_wide(string: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    std::os::windows::prelude::OsStrExt::encode_wide(string.as_ref())
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_pointer_width = "32")]
#[allow(non_snake_case)]
unsafe fn SetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    w32wm::SetWindowLongW(hwnd, index, value as _) as _
}

#[cfg(target_pointer_width = "64")]
#[allow(non_snake_case)]
unsafe fn SetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    w32wm::SetWindowLongPtrW(hwnd, index, value)
}

#[cfg(target_pointer_width = "32")]
#[allow(non_snake_case)]
unsafe fn GetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    w32wm::GetWindowLongW(hwnd, index) as _
}

#[cfg(target_pointer_width = "64")]
#[allow(non_snake_case)]
unsafe fn GetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    w32wm::GetWindowLongPtrW(hwnd, index)
}

#[cfg(test)]
mod tests {
    use super::{is_valid_activation_payload, ACTIVATION_PAYLOAD, MAX_COPYDATA_BYTES};

    #[test]
    fn accepts_the_exact_nul_terminated_activation_command() {
        assert!(is_valid_activation_payload(ACTIVATION_PAYLOAD));
    }

    #[test]
    fn rejects_messages_without_a_single_final_nul() {
        assert!(!is_valid_activation_payload(b"activate"));
        assert!(!is_valid_activation_payload(b"activate\0extra"));
        assert!(!is_valid_activation_payload(b"activate\0\0"));
    }

    #[test]
    fn rejects_other_or_non_utf8_commands() {
        assert!(!is_valid_activation_payload(b"launch\0"));
        assert!(!is_valid_activation_payload(&[0xff, 0]));
    }

    #[test]
    fn rejects_oversized_messages_before_they_are_parsed() {
        let mut oversized = vec![b'a'; MAX_COPYDATA_BYTES];
        oversized.push(0);
        assert!(!is_valid_activation_payload(&oversized));
    }
}
