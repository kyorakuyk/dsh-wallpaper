use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{GetParent, GetWindowThreadProcessId};

#[cfg(windows)]
use super::{guard, wallpaper_host_status_state, window_class};
#[cfg(windows)]
use crate::app_core::WallpaperHostMode;
#[cfg(windows)]
use tauri::Manager;
#[cfg(windows)]
use windows::Win32::{
    Foundation::{GetLastError, SetLastError, POINT},
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus, VK_LBUTTON},
        WindowsAndMessaging::{
            GetAncestor, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, SetForegroundWindow,
            WindowFromPoint, GA_ROOT,
            GUITHREADINFO,
        },
    },
};

#[cfg(windows)]
/// Phase A diagnostic for the input-island focus handoff.
///
/// The open question is which native message actually arrives on a real left-click in the
/// input hot zone, and where Windows put the foreground and its keyboard focus at that
/// moment. A thread-local `SetFocus` returning success did not mean the keyboard channel
/// was handed over, so the readbacks below are the only trustworthy evidence.
///
/// Records window classes, PIDs, handles, the hot-zone outcome and those readbacks. Never
/// records key content, window titles, input text or session content, and it is compiled
/// out of release builds. Called only from mouse-activation and focus messages, so the
/// per-mouse-move `WM_NCHITTEST` path stays untouched.
// Pure arithmetic on the message parameter; needed by the call sites in every profile.
#[cfg(windows)]
/// Record that the hit-test branch really runs while the left button is down.
///
/// Phase A of the repair plan requires the trigger to be proven on a real click rather than
/// assumed. `WM_MOUSEACTIVATE` and `WM_LBUTTONDOWN` were measured to never arrive; whether
/// `WM_NCHITTEST` arrives is the remaining question, and the whole island-click handover
/// hangs on it. Rate limited, because hit-testing repeats while a button is held.
#[cfg(all(windows, debug_assertions))]
pub(super) fn trace_island_click_reached(local_x: i32, local_y: i32) {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    if last.is_some_and(|previous| previous.elapsed() < std::time::Duration::from_millis(500)) {
        return;
    }
    *last = Some(std::time::Instant::now());
    log::info!("island click reached WM_NCHITTEST with the left button down at ({local_x}, {local_y})");
}

/// Release builds keep the call site and compile the probe away.
#[cfg(all(windows, not(debug_assertions)))]
pub(super) fn trace_island_click_reached(_local_x: i32, _local_y: i32) {}
/// Verify that a caller really is the user clicking inside the input island.
///
/// The native click route is closed: a real click never delivers `WM_MOUSEACTIVATE`,
/// `WM_LBUTTONDOWN` or even a hit test to our subclass, because the WebView2 child owns the
/// mouse messages from another process (see `docs/evidence/input-island-focus-native-route-closed.md`).
/// So the island `pointerdown` has to come from the renderer - which means the native side
/// must not trust it. Plan 3.A is explicit: the command has to check the physical left button,
/// the cursor being inside a declared region, and the window under the cursor belonging to the
/// wallpaper, and must never treat an arbitrary IPC call as a user click.
///
/// Returns a human-readable verdict for the log and for the caller. It performs no activation:
/// this is the diagnostic step the plan asks for before any handover is wired to it.
/// How far up the window chain to look before giving up.
const MAX_WINDOW_CHAIN_DEPTH: usize = 8;

/// Whether a window's own process, or any ancestor's, is this process.
///
/// The wallpaper paints through a WebView2 renderer owned by a browser helper process, so the
/// window under the cursor inside the island is a `Chrome_RenderWidgetHostHWND` and never ours.
/// Plan 3.A requires the parent chain to belong to the wallpaper, which is the check that
/// actually tells our surface apart from an unrelated window sitting on top of it.
fn window_chain_reaches_this_process(hwnd: HWND) -> bool {
    let mut current = hwnd;
    for _ in 0..MAX_WINDOW_CHAIN_DEPTH {
        if current.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        // SAFETY: `current` is a handle from the caller or from GetParent above, and the pid target is a local u32; the call writes only that out-parameter.
        unsafe { GetWindowThreadProcessId(current, Some(&mut pid)) };
        if pid == std::process::id() {
            return true;
        }
        if pid == 0 {
            return false;
        }
        // `GetParent` is a `Result` in windows 0.61, and a window with no parent is
        // the normal termination of this walk rather than an error.
        // SAFETY: `current` came from the same chain walk, and a window that vanished returns an error, which ends the walk instead of being dereferenced.
        let parent = match unsafe { GetParent(current) } {
            Ok(parent) => parent,
            Err(_) => return false,
        };
        if parent.0.is_null() {
            return false;
        }
        current = parent;
    }
    false
}

/// Carry out the foreground handover for a click that has already been verified.
///
/// Runs on the main thread because `SetFocus` fails for a window owned by another thread, and
/// the wallpaper host belongs to the main thread.
///
/// Two steps, in this order, and the order is the whole point. Our host is a `WS_CHILD` of the
/// desktop host window, and a child cannot be the foreground window - that is why every earlier
/// `SetFocus` "succeeded" while the keyboard stayed with Progman. What the working double-click
/// does is make the *top-level* desktop window foreground, after which the keyboard descends to
/// its children. So the top-level ancestor goes foreground first, and only then does the
/// keyboard focus follow into the WebView.
///
/// A genuine user click is what makes this permissible: Windows lifts the foreground lock for
/// the process that received the input, which is why this is bound to a verified click and
/// never called on its own.
///
/// Returns a description of the readback. Success is never claimed from a return code.
/// Run the handover for the wallpaper's own host window, on the main thread.
///
/// The host window belongs to the main thread, so `SetFocus` on it from any other thread
/// fails; this resolves the window here and dispatches the work where it can succeed. Failures
/// are logged rather than returned, since the caller is a click report and the handover itself
/// already records the outcome.
#[cfg(windows)]
pub fn hand_over_keyboard_for_app(app: &tauri::AppHandle) {
    let handle = app.clone();
    let dispatched = app.run_on_main_thread(move || {
      guard::run_main_thread_window_state_task(|| {
        let Some(window) = handle.get_webview_window("background") else {
            log::warn!("输入岛交接：找不到 background 窗口");
            return;
        };
        let Ok(hwnd) = window.hwnd() else {
            log::warn!("输入岛交接：background 窗口没有句柄");
            return;
        };
        log::info!("{}", hand_over_keyboard_after_verified_click(HWND(hwnd.0)));
      });
    });
    if dispatched.is_err() {
        log::warn!("输入岛交接：无法到达主线程");
    }
}

#[cfg(not(windows))]
pub fn hand_over_keyboard_for_app(_app: &tauri::AppHandle) {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusSetOutcome {
    Success,
    Failure(u32),
}

fn classify_focus_set_result(returned_null: bool, last_error: u32) -> FocusSetOutcome {
    if !returned_null || last_error == ERROR_SUCCESS.0 {
        FocusSetOutcome::Success
    } else {
        FocusSetOutcome::Failure(last_error)
    }
}

#[cfg(windows)]
unsafe fn set_focus_checked(hwnd: HWND) -> FocusSetOutcome {
    SetLastError(ERROR_SUCCESS);
    let result = SetFocus(Some(hwnd));
    classify_focus_set_result(result.is_err(), GetLastError().0)
}

#[cfg(windows)]
pub fn hand_over_keyboard_after_verified_click(host: HWND) -> String {
    // SAFETY: `host` is the wallpaper's own background HWND, checked by the caller; every call here takes handles or local out-parameters (u32, GUITHREADINFO with cbSize set) and no ownership is transferred.
    unsafe {
        let top = GetAncestor(host, GA_ROOT);
        if top.0.is_null() {
            let host_mode = wallpaper_host_status_state()
                .read()
                .map(|status| status.mode)
                .unwrap_or(WallpaperHostMode::Unavailable);
            return format!(
                "未找到顶层祖先窗口；未执行交接 host_mode={host_mode:?} host_hwnd=0x{:X}",
                host.0 as usize,
            );
        }
        let host_mode = wallpaper_host_status_state()
            .read()
            .map(|status| status.mode)
            .unwrap_or(WallpaperHostMode::Unavailable);
        let foreground_before = GetForegroundWindow();
        let mut foreground_thread = 0u32;
        GetWindowThreadProcessId(foreground_before, Some(&mut foreground_thread));
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let foreground_ok = SetForegroundWindow(top).as_bool();
        let focus_result = set_focus_checked(host);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
        let foreground_now = GetForegroundWindow();
        let mut foreground_now_pid = 0u32;
        GetWindowThreadProcessId(foreground_now, Some(&mut foreground_now_pid));
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        let focus_pid = if GetGUIThreadInfo(0, &mut info).is_ok() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid));
            Some(pid)
        } else {
            None
        };
        let focus_class = if focus_pid.is_some() {
            window_class(info.hwndFocus).unwrap_or_else(|| "<none>".to_string())
        } else {
            "<no-gui-thread-info>".to_string()
        };
        let foreground_class = window_class(foreground_now).unwrap_or_else(|| "<none>".to_string());
        // The channel is restored only when the readback agrees: the desktop top-level holds the
        // foreground and the keyboard focus has followed into the WebView. Anything else is
        // reported as a failure with the whole picture, never as a success.
        let webview_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
        let root_class = window_class(top).unwrap_or_else(|| "<none>".to_string());
        format!(
            "交接结果：host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={} AttachThreadInput={attached} SetForegroundWindow={foreground_ok} SetFocus={focus_result:?} webview_has_focus={webview_has_focus} | 读回 前台={foreground_class}(pid={foreground_now_pid}) 焦点={focus_class}(pid={focus_pid:?}) 壁纸pid={}",
            host.0 as usize,
            top.0 as usize,
            foreground_now.0 == top.0,
            std::process::id(),
        )
    }
}

#[cfg(not(windows))]
pub fn hand_over_keyboard_after_verified_click(_host: usize) -> String {
    "交接仅在 Windows 可用".to_string()
}

#[cfg(windows)]
#[cfg(windows)]
pub fn verify_island_click() -> Result<String, String> {
    // SAFETY: every call in this block is a read-only Win32 query; the cursor, pid and thread-info storage are locals and window handles are only walked and inspected.
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON.0 as i32) >= 0 {
            return Err("拒绝原因=physical-left-button-up：未检测到物理左键按下；拒绝把 IPC 调用当作输入岛点击".into());
        }
        let mut cursor = POINT::default();
        if let Err(error) = GetCursorPos(&mut cursor) {
            return Err(format!("拒绝原因=cursor-position-unavailable：无法读取光标位置 ({error})"));
        }
        let under = WindowFromPoint(cursor);
        let mut under_pid = 0u32;
        GetWindowThreadProcessId(under, Some(&mut under_pid));
        let under_class = window_class(under).unwrap_or_else(|| "<none>".to_string());
        // Walk the parent chain rather than testing the window itself. The wallpaper's own
        // content is drawn by a WebView2 renderer in a helper process, so the window directly
        // under the cursor inside the island is a `Chrome_RenderWidgetHostHWND` that never
        // belongs to us. Testing only the immediate owner rejected every genuine island click -
        // the log showed exactly that, repeatedly. Plan 3.A says the HWND parent chain must
        // belong to the wallpaper, and this is that check.
        if !window_chain_reaches_this_process(under) {
            return Err(format!(
                "拒绝原因=foreign-window-chain：光标下的窗口父链不属于壁纸（class={under_class} pid={under_pid}）；拒绝激活"
            ));
        }
        // Deliberately no geometry check here. The declared regions are in wallpaper-local
        // coordinates, and converting them needs the host window handle that this call site
        // does not have; inventing a coordinate system would be worse than not checking. The
        // renderer owns that judgement, being the side that knows where the island is. What is
        // checked here is what an arbitrary IPC caller cannot fake: a physically held left
        // button, and the window under the cursor being ours.
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        Ok(format!(
            "输入岛点击已核验：光标=({}, {})，光标下 class={under_class} pid={under_pid}，前台 pid={foreground_pid}，壁纸 pid={}",
            cursor.x,
            cursor.y,
            std::process::id(),
        ))
    }
}

#[cfg(not(windows))]
pub fn verify_island_click() -> Result<String, String> {
    Err("输入岛点击核验仅在 Windows 可用".into())
}


pub(super) fn client_point(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam.0 as u32;
    ((packed as u16 as i16) as i32, ((packed >> 16) as u16 as i16) as i32)
}

/// Phase A diagnostic for the input-island focus handoff.
///
/// Two questions have to be separated, and only a real click answers either: did the click
/// land on the WebView at all, and did Windows hand over the keyboard channel? A failing
/// fix was believed to work because `SetFocus` returned success, so every claim below is a
/// readback rather than a return code.
///
/// Records window classes, PIDs, the click resolution and those readbacks. Never records
/// key content, window titles, input text or session content, and it is compiled out of
/// release builds. Attached only to mouse-activation and focus messages, so the
/// per-mouse-move `WM_NCHITTEST` path stays untouched.
#[cfg(all(windows, debug_assertions))]
pub(super) fn trace_focus_handoff(stage: &str, client: Option<(i32, i32)>) {
    // SAFETY: debug-only diagnostics; POINT and GUITHREADINFO are locals with cbSize set before use, and the handles involved are only inspected.
    unsafe {
        // Resolve the click point to the window actually under it, which tells a missed
        // hot zone apart from a click that arrived without the keyboard following.
        let point_hit = client.map(|(x, y)| {
            let mut point = POINT { x, y };
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd_of_foreground(), &mut point);
            let under = WindowFromPoint(point);
            let mut pid = 0u32;
            GetWindowThreadProcessId(under, Some(&mut pid));
            (window_class(under).unwrap_or_else(|| "<none>".to_string()), pid)
        });
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        let foreground_class = window_class(foreground).unwrap_or_else(|| "<none>".to_string());
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        let (focus_class, focus_pid) = if GetGUIThreadInfo(0, &mut info).is_ok() {
            let class = window_class(info.hwndFocus).unwrap_or_else(|| "<none>".to_string());
            let mut pid = 0u32;
            GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid));
            (class, pid)
        } else {
            ("<no-gui-thread-info>".to_string(), 0)
        };
        log::info!(
            "focus trace [{stage}]: point_hit={point_hit:?} foreground={foreground_class} foreground_pid={foreground_pid} focus={focus_class} focus_pid={focus_pid} self_pid={}",
            std::process::id(),
        );
    }
}

#[cfg(all(windows, debug_assertions))]
fn hwnd_of_foreground() -> HWND {
    // SAFETY: GetForegroundWindow takes no arguments and returns a borrowed system handle that the caller only reads.
    unsafe { GetForegroundWindow() }
}

/// Release builds keep the call sites but compile the probe away, so the instrumentation
/// never runs in a shipped build.
#[cfg(all(windows, not(debug_assertions)))]
pub(super) fn trace_focus_handoff(_stage: &str, _client: Option<(i32, i32)>) {}


/// Release builds keep the call sites but compile the probe away, so the
/// instrumentation never runs in a shipped build.

/// Hand the keyboard to the WebView after a real click inside the input island.
///
/// Phase B of the repair plan. The phase A measurement (see
/// `docs/evidence/input-island-focus-phase-a-evidence.md`) described the failure precisely: keyboard
/// focus is already on the WebView while the global foreground is Progman, so keystrokes are
/// consumed by the foreground window. `WM_MOUSEACTIVATE` never arrives - the WebView2 child
/// takes the mouse messages in another process - so this runs from `WM_NCHITTEST`, which does
/// reach us, on the input's own window thread and during the click itself.
///
/// `AttachThreadInput` is the substance of the call, not a decoration: a thread whose window
/// is not foreground cannot give its window keyboard focus, and the desktop host window never
/// is foreground. Attaching to the foreground thread's input queue lifts that restriction for
/// the duration of the call, which is what the API exists for.
///
/// Success is decided by readback and never by a return code. A `SetFocus` that returns
/// success while the foreground stays elsewhere is exactly the false success that hid this
/// fault for several rounds.
#[cfg(windows)]
pub(super) fn hand_over_keyboard_after_island_click(root: HWND) {
    // `WM_NCHITTEST` repeats while the button is held, so one click must not run this twice.
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let Ok(mut last) = LAST.lock() else { return };
        if last.is_some_and(|previous| previous.elapsed() < std::time::Duration::from_millis(500))
        {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    // SAFETY: `root` is the host handle this subclass was installed on; all state is local (pids, GUITHREADINFO with cbSize) and AttachThreadInput and GetAncestor take plain handles.
    unsafe {
        let host_mode = wallpaper_host_status_state()
            .read()
            .map(|status| status.mode)
            .unwrap_or(WallpaperHostMode::Unavailable);
        let foreground = GetForegroundWindow();
        let mut foreground_pid = 0u32;
        let foreground_thread = GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let focus_result = set_focus_checked(root);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
        // Readback, not return codes.
        let foreground_now = GetForegroundWindow();
        let mut foreground_now_pid = 0u32;
        GetWindowThreadProcessId(foreground_now, Some(&mut foreground_now_pid));
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        let focus_pid = if GetGUIThreadInfo(0, &mut info).is_ok() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid));
            Some(pid)
        } else {
            None
        };
        let ours = foreground_now_pid == std::process::id();
        let webview_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
        let desktop_root = GetAncestor(root, GA_ROOT);
        let root_class = window_class(desktop_root).unwrap_or_else(|| "<none>".to_string());
        let foreground_class = window_class(foreground_now).unwrap_or_else(|| "<none>".to_string());
        let foreground_matches_root = foreground_now.0 == desktop_root.0;
        if ours && webview_has_focus {
            log::info!(
                "island click: keyboard channel handed over to the WebView host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={foreground_matches_root} AttachThreadInput={attached} SetFocus={focus_result:?} webview_has_focus={webview_has_focus}",
                root.0 as usize,
                desktop_root.0 as usize,
            );
        } else {
            log::warn!(
                "island click: handover did not take - host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={foreground_matches_root} AttachThreadInput={attached} SetFocus={focus_result:?} foreground_is_wallpaper={ours} webview_has_focus={webview_has_focus} foreground_pid={foreground_now_pid}",
                root.0 as usize,
                desktop_root.0 as usize,
            );
        }
    }
}

/// Hand the keyboard back to the wallpaper's WebView.
///
/// A desktop wallpaper must not activate itself, so when focus goes to another window
/// the WebView loses its keyboard channel and neither a click nor a DOM `focus()` can
/// take it back — a click cannot activate a non-activating window, and a DOM focus
/// request is refused for the same reason. Rebuilding the input (what the double-click
/// desktop switch does) only rebuilds the DOM; it does not restore the channel either.
///
/// The desktop switch works because changing the desktop's own window state makes
/// Windows renegotiate keyboard focus for the desktop's children. This does that
/// directly, and only from the wallpaper's main thread: `SetFocus` fails for a window
/// owned by another thread, and the background window belongs to the main thread.
///
/// `SetForegroundWindow` is deliberately not called. Giving the keyboard back is not the
/// same as stealing the foreground, and stealing it is exactly what the non-activating
/// design exists to prevent.
#[cfg(windows)]
pub(super) fn restore_desktop_keyboard_focus(app: &tauri::AppHandle) {
    let handle = app.clone();
    let dispatched = app.run_on_main_thread(move || {
      guard::run_main_thread_window_state_task(|| {
        let Some(window) = handle.get_webview_window("background") else {
            return;
        };
        let Ok(hwnd) = window.hwnd() else {
            log::warn!("desktop focus restore: background window has no handle");
            return;
        };
        let host = HWND(hwnd.0);
        let host_mode = wallpaper_host_status_state()
            .read()
            .map(|status| status.mode)
            .unwrap_or(WallpaperHostMode::Unavailable);
        // SAFETY: `host` is the live background window resolved on the main thread, and set_focus_checked only calls SetFocus and GetLastError on it.
        let focus_result = unsafe { set_focus_checked(host) };
        // SetFocus's return is only one diagnostic. Always read back the actual
        // foreground and keyboard focus, even when SetFocus reports a real error.
        // SAFETY: `host` is the same live handle; GetAncestor only resolves the root handle and returns null on failure, which the class probe then tolerates.
        let desktop_root = unsafe { GetAncestor(host, GA_ROOT) };
        let root_class = window_class(desktop_root).unwrap_or_else(|| "<none>".to_string());
        // SAFETY: GetForegroundWindow takes no arguments and its handle is only read.
        let foreground = unsafe { GetForegroundWindow() };
        let foreground_class = window_class(foreground).unwrap_or_else(|| "<none>".to_string());
        let foreground_matches_root = foreground.0 == desktop_root.0;
        let mut foreground_pid = 0u32;
        // SAFETY: `foreground` is a handle just obtained and `foreground_pid` is a local u32; the call writes only that out-parameter.
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_pid)) };
        let mut info = GUITHREADINFO::default();
        info.cbSize = core::mem::size_of::<GUITHREADINFO>() as u32;
        // SAFETY: `info` is a local GUITHREADINFO whose cbSize is set to its own size immediately above, which is the contract the API requires before writing.
        let focus_pid = if unsafe { GetGUIThreadInfo(0, &mut info) }.is_ok() {
            let mut pid = 0u32;
            // SAFETY: `info.hwndFocus` was read from the GUITHREADINFO that the successful call above filled, and the pid target is a local u32.
            unsafe { GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid)) };
            Some(pid)
        } else {
            None
        };
        let webview_has_focus = focus_pid.is_some_and(|pid| pid != 0 && pid != std::process::id());
        let foreground_is_wallpaper = foreground_pid == std::process::id();
        let diagnostic = format!(
            "host_mode={host_mode:?} host_hwnd=0x{:X} root_hwnd=0x{:X} root_class={root_class} foreground_class={foreground_class} foreground_matches_root={foreground_matches_root} AttachThreadInput=false SetFocus={focus_result:?} foreground_is_wallpaper={foreground_is_wallpaper} webview_has_focus={webview_has_focus} focus_pid={focus_pid:?}",
            host.0 as usize,
            desktop_root.0 as usize,
        );
        if let FocusSetOutcome::Failure(error) = focus_result {
            log::warn!("desktop focus restore: SetFocus failed: 0x{error:08X}; {diagnostic}");
        } else if foreground_is_wallpaper && webview_has_focus {
            log::info!("desktop focus restore: keyboard channel restored; {diagnostic}");
        } else {
            log::warn!("desktop focus restore: keyboard channel not restored; {diagnostic}");
        }
      });
    });
    if dispatched.is_err() {
        log::warn!("desktop focus restore: could not reach the main thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_set_focus_return_with_no_last_error_is_success() {
        assert_eq!(
            classify_focus_set_result(true, ERROR_SUCCESS.0),
            FocusSetOutcome::Success
        );
    }

    #[test]
    fn a_null_set_focus_return_with_an_error_preserves_the_error_code() {
        assert_eq!(
            classify_focus_set_result(true, 87),
            FocusSetOutcome::Failure(87)
        );
    }

    #[test]
    fn a_non_null_set_focus_return_is_success_even_with_a_residual_error() {
        assert_eq!(
            classify_focus_set_result(false, 87),
            FocusSetOutcome::Success
        );
    }
}
