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
use windows::Win32::Foundation::{HWND, LPARAM};
#[cfg(windows)]
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
#[cfg(windows)]
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
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
    found: Option<HWND>,
}

#[cfg(windows)]
unsafe extern "system" fn find_top_level(hwnd: HWND, param: LPARAM) -> BOOL {
    let search = unsafe { &mut *(param.0 as *mut TopLevelSearch) };
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
    if owner == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        search.found = Some(hwnd);
        // One visible top-level window per client is the expected shape; stop at
        // the first rather than picking an arbitrary later one.
        return BOOL(0);
    }
    BOOL(1)
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
        let Some(pid) = listener_pid(port) else {
            return RaiseOutcome::not_running();
        };
        let mut search = TopLevelSearch { pid, found: None };
        unsafe {
            let _ = EnumWindows(Some(find_top_level), LPARAM(&mut search as *mut _ as isize));
        }
        let Some(hwnd) = search.found else {
            return RaiseOutcome::no_window();
        };
        unsafe {
            // A minimised client is restored before it is raised, otherwise
            // "bring it forward" visibly does nothing.
            let _ = ShowWindow(hwnd, SW_RESTORE);
            if SetForegroundWindow(hwnd).as_bool() {
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
}
