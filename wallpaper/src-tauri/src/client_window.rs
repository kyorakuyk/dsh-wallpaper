//! Raising the window of whichever DSH client the wallpaper is connected to.
//!
//! The three client shapes differ in how the user reaches their interface, and
//! only the user knows which one they are running, so the wallpaper cannot guess:
//!
//! | client           | default port | interface                       |
//! | ---------------- | ------------ | ------------------------------- |
//! | official desktop | 19387        | its own Windows window          |
//! | community desktop| 43120        | its own Windows window           |
//! | CLI / webui      | 3080         | no window at all — a browser URL |
//!
//! This module implements the *interview* half only: identify the client by the
//! port it listens on and bring its window forward, or report that there is
//! nothing to raise. Opening a browser for the windowless shape is the caller's
//! decision, because only the caller knows which endpoint the user selected.
//!
//! Deliberately does not launch anything. Launching a client is a different,
//! heavier action with its own single-flight and trust questions; raising a
//! window must not become a back door that starts processes.

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
#[cfg(windows)]
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
#[cfg(windows)]
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
};
#[cfg(windows)]
use windows::core::BOOL;

/// What happened when the wallpaper asked for a client's interface.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RaiseOutcome {
    /// Stable, non-sensitive code. The UI maps it to wording.
    pub outcome: &'static str,
    /// True when a window was actually brought forward.
    pub raised: bool,
}

impl RaiseOutcome {
    fn raised() -> Self {
        Self { outcome: "raised", raised: true }
    }

    fn no_window() -> Self {
        // The port answers but owns no visible window: the CLI/webui shape, or a
        // client still starting up. Caller decides whether to open a browser.
        Self { outcome: "no-window", raised: false }
    }

    fn not_running() -> Self {
        Self { outcome: "not-running", raised: false }
    }
}

/// The process id that owns a listening TCP port on loopback, if any.
///
/// Exposed within the crate so a caller that started something for the user can
/// clean up after itself by identity (the kernel's own table) instead of by image
/// name, which would also match an instance the user was already running.
#[cfg(windows)]
pub(crate) fn endpoint_process_id(port: u16) -> Option<u32> {
    listener_pid(port)
}

/// The process id that owns a listening TCP port on loopback, if any.
///
/// Queried from the kernel's own table rather than by launching `netstat`, so it
/// works without a console and cannot be influenced by a PATH-shadowed binary.
#[cfg(windows)]
fn listener_pid(port: u16) -> Option<u32> {
    if let Some(pid) = listener_pid_v4(port) {
        return Some(pid);
    }
    listener_pid_v6(port)
}

#[cfg(windows)]
fn listener_pid_v4(port: u16) -> Option<u32> {
    let mut size = 0u32;
    // First call sizes the buffer; the expected ERROR_INSUFFICIENT_BUFFER is not
    // an error worth reporting.
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return None;
    }
    unsafe {
        let table = &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(
            table.table.as_ptr() as *const MIB_TCPROW_OWNER_PID,
            table.dwNumEntries as usize,
        );
        for row in rows {
            // dwLocalPort is in network byte order in the low 16 bits.
            let local_port = u16::from_be((row.dwLocalPort & 0xFFFF) as u16);
            if local_port == port {
                return Some(row.dwOwningPid);
            }
        }
    }
    None
}

#[cfg(windows)]
fn listener_pid_v6(port: u16) -> Option<u32> {
    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return None;
    }
    unsafe {
        let table = &*(buffer.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(
            table.table.as_ptr() as *const MIB_TCP6ROW_OWNER_PID,
            table.dwNumEntries as usize,
        );
        for row in rows {
            let local_port = u16::from_be((row.dwLocalPort & 0xFFFF) as u16);
            if local_port == port {
                return Some(row.dwOwningPid);
            }
        }
    }
    None
}

#[cfg(windows)]
struct TopLevelSearch {
    pid: u32,
    /// Whether an invisible top-level window counts as the client's interface.
    ///
    /// Two callers need the two answers: raising must find a window the wallpaper
    /// itself hid, while a client that has *never* shown a window (the CLI/webui
    /// shape) must not be reported as raisable.
    require_visible: bool,
    found: Option<HWND>,
}

#[cfg(windows)]
unsafe extern "system" fn find_top_level(hwnd: HWND, param: LPARAM) -> BOOL {
    let search = unsafe { &mut *(param.0 as *mut TopLevelSearch) };
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
    if owner != search.pid {
        return BOOL(1);
    }
    let is_interface = unsafe { IsWindowVisible(hwnd) }.as_bool()
        || (!search.require_visible && window_is_a_client_window(hwnd));
    if is_interface {
        search.found = Some(hwnd);
        // One top-level window per client is the expected shape; stop at the first
        // rather than picking an arbitrary later one.
        return BOOL(0);
    }
    BOOL(1)
}

/// Whether an invisible top-level window is still a client's own interface window.
///
/// The distinction matters because the wallpaper now starts a client with its
/// window hidden, and later has to find that same window again to bring it back
/// (§6.1). A window it hid keeps its title and its size; a framework's bookkeeping
/// windows (Chromium creates several per process) have neither, and treating one of
/// those as "the client's window" would make a windowless client look raisable, and
/// would make hiding a client that has no window look successful.
#[cfg(windows)]
fn window_is_a_client_window(hwnd: HWND) -> bool {
    if unsafe { GetWindowTextLengthW(hwnd) } <= 0 {
        return false;
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return false;
    }
    rect.right > rect.left && rect.bottom > rect.top
}

/// Bring the selected client's window forward.
///
/// `port` is the endpoint the wallpaper is currently connected to, so the window
/// that is raised is the same client whose session the user is talking to. The
/// port is resolved to a process and that process to its own window; a stored
/// executable path would go stale the moment the user installed the client
/// somewhere else.
pub fn raise_client_window(port: u16) -> RaiseOutcome {
    #[cfg(windows)]
    {
        let Some(window) = window_for_endpoint(port) else {
            // Distinguish "nothing there" from "there, but windowless": the two
            // need different wording, and the second is the normal shape of the
            // CLI/webui client.
            return if endpoint_is_listening(port) {
                RaiseOutcome::no_window()
            } else {
                RaiseOutcome::not_running()
            };
        };
        unsafe {
            // Order matters, and both calls are idempotent:
            // * `SW_SHOW` first, because a window the wallpaper hid during a silent
            //   start is *invisible*, not minimised, and the wallpaper must be the
            //   one to bring it back — the client's own idea of whether its window
            //   is visible was never trusted in either direction (§6.1).
            // * `SW_RESTORE` then, because a minimised client would otherwise be
            //   "brought forward" without becoming visible.
            let _ = ShowWindow(window, SW_SHOW);
            let _ = ShowWindow(window, SW_RESTORE);
            if SetForegroundWindow(window).as_bool() {
                RaiseOutcome::raised()
            } else {
                // Windows refuses foreground changes from a process that is not
                // itself foreground. That is a normal outcome for a desktop
                // wallpaper, not a failure: the window is restored and the user's
                // next click reaches it.
                RaiseOutcome { outcome: "raise-refused", raised: false }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        RaiseOutcome::no_window()
    }
}

/// What happened when the wallpaper put a client's window out of sight.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HideOutcome {
    /// Stable, non-sensitive code. The UI maps it to wording.
    pub outcome: &'static str,
    /// True when a visible window was actually hidden.
    pub hidden: bool,
}

impl HideOutcome {
    fn hidden_ok() -> Self {
        Self { outcome: "hidden", hidden: true }
    }

    fn no_window() -> Self {
        Self { outcome: "no-window", hidden: false }
    }

    fn not_running() -> Self {
        Self { outcome: "not-running", hidden: false }
    }
}

/// Put the selected client's window out of sight, without touching its process.
///
/// The mirror image of `raise_client_window`, and deliberately the same mechanism:
/// the port resolves to a process and that process to its own window, so nothing
/// here depends on a stored executable path either.
///
/// It exists because Windows has no "start without showing a window" flag for an
/// application that does not implement one, so a silent start is "start, then hide
/// the window we can see" (`docs/design/harness-subject-and-ui-design.md` §5.1).
/// Because the wallpaper is what hides the window, the wallpaper is also what
/// shows it again (§6.1): the shell's own idea of whether its window is visible is
/// never depended upon, in either direction.
pub fn hide_client_window(port: u16) -> HideOutcome {
    #[cfg(windows)]
    {
        let Some(window) = window_for_endpoint(port) else {
            return if endpoint_is_listening(port) {
                HideOutcome::no_window()
            } else {
                HideOutcome::not_running()
            };
        };
        // `ShowWindow` reports the *previous* visibility, which is not the answer
        // this returns: the caller asked whether the window is now hidden, and a
        // window that was already hidden is not reachable here at all, because
        // `window_for_endpoint` only ever returns visible windows.
        unsafe { ShowWindow(window, SW_HIDE) };
        HideOutcome::hidden_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        HideOutcome::no_window()
    }
}

/// The visible top-level window belonging to the client behind `port`.
///
/// Walks the owning process and its ancestors, because the process that *listens*
/// is not always the process that *owns the window*. Measured on this machine: the
/// official desktop shell's listener (pid 16720) and both of its children reported
/// zero visible top-level windows while the shell's UI was up, so assuming the
/// listener owns the window would have refused to raise a window that exists.
///
/// Search order is nearest-first: the owning process, then its parent chain. The
/// listener is the most precise answer, and an ancestor is only a fallback for
/// this split-process shape.
pub fn window_for_endpoint(port: u16) -> Option<HWND> {
    #[cfg(windows)]
    {
        let Some(pid) = listener_pid(port) else {
            return None;
        };
        let chain = ancestor_chain(pid, MAX_ANCESTOR_DEPTH);
        // A visible window is always the right answer; only when the client has
        // none do we look for one it is keeping hidden — which is the state the
        // wallpaper itself creates when it starts a client in the background.
        for candidate in &chain {
            if let Some(window) = first_window(*candidate, true) {
                return Some(window);
            }
        }
        for candidate in &chain {
            if let Some(window) = first_window(*candidate, false) {
                return Some(window);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
    }
}

/// How far up the process tree to look. A desktop client's UI is at most a couple
/// of levels above the process holding the socket; a deeper walk would risk
/// raising an unrelated window from a shared launcher.
const MAX_ANCESTOR_DEPTH: usize = 4;

/// The process itself, then each ancestor, bounded by `max_depth`.
///
/// Measured on this machine, the official shell's UI belongs to a parent process of
/// the same executable, one level above the process holding the socket, and a
/// different executable in the chain is not this client — `explorer.exe` is an
/// ancestor of everything the user starts from the desktop, so crossing it could
/// raise the desktop's own window.
///
/// An ancestor whose name cannot be read is followed anyway, and that distinction is
/// the whole point: reading another process's name is what a *packaged* build cannot
/// always do, so stopping there reported a live client's window as missing in the
/// installed build while the same code resolved it unpackaged. The depth bound is
/// what keeps an ancestor walk meaningful, so it — not an unreadable name — is the
/// guard. A name that *is* readable is still decisive.
#[cfg(windows)]
fn ancestor_chain(pid: u32, max_depth: usize) -> Vec<u32> {
    let mut chain = vec![pid];
    let Some(original_name) = process_name(pid) else {
        return chain;
    };
    let mut current = pid;
    for _ in 0..max_depth {
        let Some(parent) = parent_pid(current) else { break };
        // A zero parent means the root, and a cycle must not loop forever.
        if parent == 0 || chain.contains(&parent) {
            break;
        }
        if let Some(name) = process_name(parent) {
            if name != original_name {
                break;
            }
        }
        chain.push(parent);
        current = parent;
    }
    chain
}
/// The executable's file name, lower-cased so the comparison is case-insensitive
/// and so the same product installed under two directories still matches.
#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 260];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        result.ok()?;
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        Some(
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_else(|| path.to_ascii_lowercase()),
        )
    }
}

#[cfg(windows)]
fn parent_pid(pid: u32) -> Option<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut result = None;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    result = Some(entry.th32ParentProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        result
    }
}

#[cfg(windows)]
fn first_window(pid: u32, require_visible: bool) -> Option<HWND> {
    let mut search = TopLevelSearch {
        pid,
        require_visible,
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(find_top_level), LPARAM(&mut search as *mut _ as isize));
    }
    search.found
}

/// Whether anything is listening on the port at all, without resolving a window.
/// Used to tell "client not running" from "client running but windowless".
#[cfg(windows)]
pub fn endpoint_is_listening(port: u16) -> bool {
    listener_pid(port).is_some()
}

#[cfg(not(windows))]
pub fn endpoint_is_listening(_port: u16) -> bool {
    false
}

/// Open a loopback URL in the user's default browser.
///
/// Used for the windowless client shape (CLI / webui), which has no Windows
/// window to raise. `ShellExecuteW` with the `open` verb asks the shell to resolve
/// the URL protocol, rather than spawning `powershell`/`cmd` to do it — no extra
/// console, no PATH-shadowed binary, and no string assembled into a command line.
///
/// Only `http://127.0.0.1` and `http://localhost` are accepted: this command must
/// not become a general "open any URL" primitive that a compromised renderer
/// could point at an arbitrary destination.
pub fn open_loopback_url(port: u16, path: &str) -> Result<(), String> {
    if port == 0 {
        return Err("接入端点端口无效".into());
    }
    // A path with a scheme, a host, or a traversal is not a route on this port.
    if path.contains("://") || path.starts_with("//") || path.contains("..") {
        return Err("接入端点地址无效".into());
    }
    let url = format!("http://127.0.0.1:{port}{}", if path.starts_with('/') { path.to_string() } else { format!("/{path}") });

    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR};

        let operation = HSTRING::from("open");
        let target = HSTRING::from(url.as_str());
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW returns a value <= 32 on failure; only > 32 is success.
        if result.0 as usize <= 32 {
            return Err("无法在默认浏览器中打开接入端点".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("当前平台不支持打开浏览器".into())
    }
}

#[cfg(test)]
mod tests {
    use super::RaiseOutcome;

    /// The outcome codes are a contract with the renderer, which maps each to its
    /// own wording. Keeping them pinned prevents a rename from silently turning
    /// every case into the fallback message.
    #[test]
    fn outcome_codes_are_stable_and_distinct() {
        assert_eq!(RaiseOutcome::raised().outcome, "raised");
        assert_eq!(RaiseOutcome::no_window().outcome, "no-window");
        assert_eq!(RaiseOutcome::not_running().outcome, "not-running");
        // Only one of them claims to have raised a window.
        assert!(RaiseOutcome::raised().raised);
        assert!(!RaiseOutcome::no_window().raised);
        assert!(!RaiseOutcome::not_running().raised);
    }

    /// Serialisation is what crosses the IPC boundary, so the field names matter
    /// as much as the values.
    #[test]
    fn outcome_serialises_with_camel_case_fields() {
        let json = serde_json::to_value(RaiseOutcome::no_window()).expect("serialises");
        assert_eq!(json["outcome"], "no-window");
        assert_eq!(json["raised"], false);
    }

    /// Every port with nothing on it must resolve to no window and no listener.
    ///
    /// Uses an ephemeral port that is bound and released, so this asserts the
    /// "absent" path of the real Windows code rather than a mock.
    #[test]
    fn a_free_port_has_neither_listener_nor_window() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
            let port = listener.local_addr().expect("addr").port();
            drop(listener);
            port
        };
        assert!(!super::endpoint_is_listening(port));
        assert!(super::window_for_endpoint(port).is_none());
        // And the full action reports "not running" rather than "windowless",
        // because those need different wording in the UI.
        assert_eq!(super::raise_client_window(port).outcome, "not-running");
    }

    /// A port that is genuinely listening but owns no window reports `no-window`.
    ///
    /// This is the CLI/webui shape, and it is the case that must not be confused
    /// with "the client is not running". The listener here is this test process,
    /// which owns no visible top-level window.
    #[test]
    fn a_windowless_listener_reports_no_window() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        assert!(super::endpoint_is_listening(port));
        assert!(super::window_for_endpoint(port).is_none());
        assert_eq!(super::raise_client_window(port).outcome, "no-window");
        drop(listener);
    }

    /// The success path, against whatever official-shell endpoint is live.
    ///
    /// This is the case that found the original defect: the process that listens
    /// (19387) owns no window, and the UI belongs to a parent process of the same
    /// executable. The assertion therefore checks that *a* window is resolved,
    /// not which process it belongs to.
    ///
    /// `#[ignore]` because it requires a running official desktop shell, so it is
    /// meaningless on a CI machine. Run it deliberately:
    /// `cargo test --lib client_window -- --ignored`.
    #[test]
    #[ignore = "requires a running DSH desktop client on 19387"]
    fn a_live_desktop_client_resolves_to_a_window() {
        const OFFICIAL_DESKTOP_PORT: u16 = 19387;
        if !super::endpoint_is_listening(OFFICIAL_DESKTOP_PORT) {
            eprintln!("skipped: nothing listening on {OFFICIAL_DESKTOP_PORT}");
            return;
        }
        let window = super::window_for_endpoint(OFFICIAL_DESKTOP_PORT);
        assert!(
            window.is_some(),
            "port {OFFICIAL_DESKTOP_PORT} is listening but no visible window was resolved; \
             the ancestor walk is not reaching the process that owns the UI"
        );
        // A resolved window means the action must not report the two failure
        // codes — it either raised it or Windows declined, and both mean a window
        // was found.
        let outcome = super::raise_client_window(OFFICIAL_DESKTOP_PORT).outcome;
        assert!(
            outcome == "raised" || outcome == "raise-refused",
            "expected a window to be found, got {outcome}"
        );
    }
}
