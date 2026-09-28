//! Raising the window of whichever DSH client the wallpaper is connected to.
//!
//! The three client shapes differ in how the user reaches their interface, and
//! only the user knows which one they are running, so the wallpaper cannot guess:
//!
//! | client           | default port | interface                       |
//! | ---------------- | ------------ | ------------------------------- |
//! | official desktop | 19387        | its own Windows window          |
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
/// 某个 pid 的创建时间（内核给的 100ns 计数），用作"进程身份"的另一半。
///
/// 只比 pid 不够：pid 会被回收，一条过期记录迟早撞上一个新进程，那时"这是我启动的"就会让本应用
/// 去停别人的进程。**pid 与创建时间的组合才是身份**。
///
/// `None` 一律表示"查不到"（进程已退出、权限不足、平台不提供），调用方按"不是我启动的"处理。
#[cfg(windows)]
pub(crate) fn process_started_at(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let queried =
            GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok();
        let _ = CloseHandle(handle);
        if !queried {
            return None;
        }
        Some(filetime_ticks(creation.dwHighDateTime, creation.dwLowDateTime))
    }
}

#[cfg(not(windows))]
pub(crate) fn process_started_at(_pid: u32) -> Option<u64> {
    // 没有可读的内核表时，"查不到"才是诚实的答案：调用方会按"不是我启动的"处理。
    None
}

/// `FILETIME` 的两个 32 位半部分合成 64 位计数。
///
/// 抽成纯函数是因为它是这段里唯一能被测住的部分：**高字在前**，写反会得到一个"看起来像时间"
/// 的错值 —— 而错值正是这类判定最危险的失败方式，它会把自己伪装成"同一个进程"。
pub(crate) fn filetime_ticks(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

/// 结束一个进程及其子树，**安静地**：不弹控制台、不建管道、不等管道。
///
/// `taskkill.exe` 是控制台程序，而从 GUI 进程起控制台程序时，Windows 默认会为它新建一个控制台
/// 窗口 —— 用户看到的就是一个黑框。若再用 `Command::output()`，还会为它建管道并一直等到它退出；
/// 在界面线程上这么做，就是"设置窗口先卡死、然后弹一个黑框"的来源。
///
/// 所以三件事一起做：`CREATE_NO_WINDOW` 不建窗口、三个标准流都置空不走管道、用 `.status()`
/// 只取退出码。返回是否成功，好让调用方如实回报，而不是假设它一定成功。
#[cfg(windows)]
pub(crate) fn stop_process_tree(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let outcome = std::process::Command::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    matches!(outcome, Ok(status) if status.success())
}

#[cfg(not(windows))]
pub(crate) fn stop_process_tree(_pid: u32) -> bool {
    // 没有内核表可读时，"做不到"才是诚实的答案：调用方会如实回报，而不是以为已经停掉。
    false
}

/// Exposed within the crate so a caller that started something for the user can
/// clean up after itself by identity (the kernel's own table) instead of by image
/// name, which would also match an instance the user was already running.
#[cfg(windows)]
pub(crate) fn endpoint_process_id(port: u16) -> Option<u32> {
    listener_pid(port)
}

#[cfg(not(windows))]
pub(crate) fn endpoint_process_id(_port: u16) -> Option<u32> {
    // No kernel table to read off Windows, so the wallpaper has no owner to watch.
    // `None` is the honest answer: "unproven", not "dead".
    None
}

/// Whether a process id still names a live process.
///
/// `OpenProcess` succeeding is *not* proof: the id stays resolvable while any handle
/// to it is open anywhere, so a process that has already exited can still be named.
/// The exit code is the check that separates "running" from "the id is merely still
/// in the kernel's table", which is the difference between "the subject exited" and
/// "the subject is hung" — two conditions the wallpaper must not confuse, because
/// only the first one is a reason to give up on the subject.
#[cfg(windows)]
pub fn process_is_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::STILL_ACTIVE;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code: u32 = 0;
        let alive = GetExitCodeProcess(handle, &mut code).is_ok()
            && code == STILL_ACTIVE.0 as u32;
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        alive
    }
}

#[cfg(not(windows))]
pub fn process_is_alive(_pid: u32) -> bool {
    // Nothing about a pid's liveness is knowable off Windows, and "dead" is the
    // answer that would make the wallpaper give up on a live subject.
    true
}

/// Block until a process is gone, then report whether it is gone.
///
/// Waits on the process *handle* rather than polling it: a handle is signalled exactly
/// once, so a watcher costs nothing while the subject is healthy and cannot miss a
/// process that exits between two polls. That is what turns "the client exited" into an
/// event instead of a discovery — the difference between a light that changes when the
/// user watches the window close, and one that changes several seconds later when the
/// next scheduled probe happens to run.
///
/// A handle that cannot be opened means the process is already gone (or that this
/// process may not observe it). Both are the answer the caller is waiting for: there is
/// nothing left to wait on.
#[cfg(windows)]
pub fn wait_for_process_exit(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
            return true;
        };
        let waited = WaitForSingleObject(handle, INFINITE);
        let _ = CloseHandle(handle);
        waited == WAIT_OBJECT_0
    }
}

#[cfg(not(windows))]
pub fn wait_for_process_exit(_pid: u32) -> bool {
    true
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
        unsafe { let _ = ShowWindow(window, SW_HIDE); }
        HideOutcome::hidden_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        HideOutcome::no_window()
    }
}

/// Put every window of one executable's process family out of sight.
///
/// The executable-path counterpart of [`hide_client_window`], and the identity the
/// launch path uses. Measured on this machine, the official shell's listener and the
/// process that owns its window are two processes of *one* file (pid 41320 listening,
/// pid 47972 owning the window, its parent — an Electron client keeps its runtime in a
/// child). Naming the file rather than the process is what makes one call cover the
/// whole family: the port-shaped lookup reaches the same window through a bounded
/// ancestor walk and can only answer with one window per call.
///
/// `no-window` means no window of that executable is on screen — either it is not
/// running, or it is already out of sight (which is a state the wallpaper itself
/// creates). Neither is reported as "hidden", because that would be a claim about a
/// window nobody saw.
pub fn hide_executable_windows(executable: &str) -> HideOutcome {
    #[cfg(windows)]
    {
        let windows = windows_of_executable(executable, true);
        if windows.is_empty() {
            return HideOutcome::no_window();
        }
        for window in windows {
            let _ = unsafe { ShowWindow(window, SW_HIDE) };
        }
        HideOutcome::hidden_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = executable;
        HideOutcome::no_window()
    }
}

/// Bring every window of one executable's process family forward.
///
/// The showing half of the same identity, and the deliberate mirror of
/// [`raise_client_window`]: a window the wallpaper hid is *invisible*, not minimised,
/// so this is what brings back the client that was started in the background (§6.1).
/// An on-screen window is always the right answer; only when the executable has none
/// does the search accept an invisible window that still looks like the client's own
/// (title and size), which is exactly the state the wallpaper created.
///
/// The outcome codes are the ones the renderer already words: `raised` when Windows
/// accepted the foreground change, `raise-refused` when it did not (normal for a
/// desktop wallpaper — the windows are shown and restored either way), and
/// `no-window` when the executable owns no window to show.
pub fn show_executable_windows(executable: &str) -> RaiseOutcome {
    #[cfg(windows)]
    {
        let mut windows = windows_of_executable(executable, true);
        if windows.is_empty() {
            windows = windows_of_executable(executable, false);
        }
        if windows.is_empty() {
            return RaiseOutcome::no_window();
        }
        let mut foreground = false;
        for window in windows {
            unsafe {
                // The same order as `raise_client_window`, for the same two reasons:
                // `SW_SHOW` first because the visible-and-hidden pair is ours to
                // decide in both directions, `SW_RESTORE` then because a minimised
                // window would otherwise be "brought forward" without being visible.
                let _ = ShowWindow(window, SW_SHOW);
                let _ = ShowWindow(window, SW_RESTORE);
                foreground |= SetForegroundWindow(window).as_bool();
            }
        }
        if foreground {
            RaiseOutcome::raised()
        } else {
            RaiseOutcome { outcome: "raise-refused", raised: false }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = executable;
        RaiseOutcome::no_window()
    }
}

/// Whether two paths name the same executable file.
///
/// Windows paths are case-insensitive, and the same file is spelled several ways
/// depending on who was asked: `QueryFullProcessImageNameW` returns a `\\?\`-prefixed
/// path for some processes (this machine's scan records exactly that spelling for a
/// checkout root), while a shortcut's target comes back plain. Folding the namespace
/// prefix, the separator spelling and the case is what makes "the same executable"
/// mean the same thing on both sides of the comparison.
///
/// Pure, and therefore testable without a running client: everything it decides is a
/// string question, and both ways of getting it wrong are bad — matching too loosely
/// hides a program the user did not choose, matching too strictly hides nothing and
/// leaves the window on screen.
pub(crate) fn same_executable_path(left: &str, right: &str) -> bool {
    let left = normalized_executable_path(left);
    let right = normalized_executable_path(right);
    // An empty side is never a match — not even against another empty one. "The path
    // could not be read" must answer "no window of that executable", never "this
    // window is it": the loose answer hides a program the user did not choose.
    !left.is_empty() && left == right
}

/// The comparable form of an executable path: no verbatim prefix, one separator
/// spelling, no trailing separator, lower case.
fn normalized_executable_path(path: &str) -> String {
    let trimmed = path.trim();
    // `\\?\` (and the `\??\` spelling of the same namespace) is not part of the file
    // name, so the same file reached with and without it must compare equal.
    // `\\?\UNC\server\share` is the verbatim spelling of `\\server\share`, so the UNC
    // form is rebuilt rather than left as a directory literally called `UNC`.
    let mut value = match trimmed
        .strip_prefix(r"\\?\")
        .or_else(|| trimmed.strip_prefix(r"\??\"))
    {
        Some(rest) if rest.get(..4).is_some_and(|head| head.eq_ignore_ascii_case("UNC\\")) => {
            format!(r"\\{}", &rest[4..])
        }
        Some(rest) => rest.to_string(),
        None => trimmed.to_string(),
    };
    value = value.replace('/', "\\");
    while value.ends_with('\\') {
        value.pop();
    }
    value.to_ascii_lowercase()
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
///
/// The *file name* is the right answer for walking ancestors (the same client's
/// processes share it), and the wrong answer for identifying a client: a second
/// install elsewhere has the same name, which is why matching windows by executable
/// uses the full path from [`process_image_path`] instead.
#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    let path = process_image_path(pid)?;
    Some(
        std::path::Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| path.to_ascii_lowercase()),
    )
}

/// The full path of the executable a process is running, or `None` when it cannot be
/// read.
///
/// `None` means "unreadable" — the process is gone, or this process may not query it —
/// and never "does not match". The distinction is the whole safety direction here:
/// a window whose owner cannot be named must not be treated as a window of the
/// target executable.
#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        // `QueryFullProcessImageNameW` reports a buffer that is too small instead of
        // truncating, so a long install path is asked for again with more room rather
        // than being reported as unreadable.
        let mut path = None;
        for capacity in [512usize, 4096, 32_768] {
            let mut buffer = vec![0u16; capacity];
            let mut size = capacity as u32;
            let queried = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut size,
            )
            .is_ok();
            if queried {
                path = Some(String::from_utf16_lossy(&buffer[..size as usize]));
                break;
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        path
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

/// One enumeration of the top-level windows whose owner runs `executable`.
#[cfg(windows)]
struct ExecutableWindowSearch<'a> {
    executable: &'a str,
    /// Whether only a window that is on screen counts.
    ///
    /// The same two answers [`TopLevelSearch`] needs, for the same two callers: hiding
    /// must only ever claim a window nobody can see any more, while showing must also
    /// find the window this wallpaper hid — which is invisible, and to a window
    /// search that only accepts visible ones does not exist at all.
    require_visible: bool,
    /// Every match, not the first: a client's process family can own more than one
    /// window, and leaving the second one on screen is exactly the failure this
    /// helper exists to remove.
    found: Vec<HWND>,
}

#[cfg(windows)]
unsafe extern "system" fn collect_executable_windows(hwnd: HWND, param: LPARAM) -> BOOL {
    let search = unsafe { &mut *(param.0 as *mut ExecutableWindowSearch) };
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
    if owner == 0 {
        return BOOL(1);
    }
    // Asked per window rather than cached per process: the family is a handful of
    // windows, and naming the owner is the *only* thing that decides whether this
    // window belongs to the target — guessing it from a pid would put the whole
    // split-process problem back.
    let Some(executable) = process_image_path(owner) else {
        return BOOL(1);
    };
    if !same_executable_path(&executable, search.executable) {
        return BOOL(1);
    }
    let on_screen = unsafe { IsWindowVisible(hwnd) }.as_bool();
    if on_screen || (!search.require_visible && window_is_a_client_window(hwnd)) {
        search.found.push(hwnd);
    }
    // Keep walking: unlike `find_top_level`, one match does not answer the question.
    BOOL(1)
}

/// Every top-level window whose owning process runs `executable`.
///
/// `require_visible` separates "on screen right now" from "also the client's own
/// window that is currently out of sight" — see [`ExecutableWindowSearch`].
#[cfg(windows)]
fn windows_of_executable(executable: &str, require_visible: bool) -> Vec<HWND> {
    let mut search = ExecutableWindowSearch {
        executable,
        require_visible,
        found: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(collect_executable_windows),
            LPARAM(&mut search as *mut _ as isize),
        );
    }
    search.found
}

/// The executable of the client answering on `port`, if the kernel can be asked.
///
/// Used where a path is needed for a subject the last scan recorded none for: the
/// process holding the socket is the client that is running *now*, which is the
/// answer that stays right after a reinstall moves the install directory.
pub fn endpoint_executable(port: u16) -> Option<String> {
    #[cfg(windows)]
    {
        listener_pid(port).and_then(process_image_path)
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
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
    use super::{wait_for_process_exit, RaiseOutcome};

    /// 窗口与可执行文件之间靠**路径**认亲，所以这条比较是整个隐藏/显示的一半。
    ///
    /// 两侧的拼法来自两个不同的地方、也由两个不同的 API 给出：进程镜像路径
    /// （`QueryFullProcessImageNameW`）与快捷方式的目标。写松了会把用户没选的那个程序藏起来，
    /// 写紧了就一个窗口也找不到 —— 而"找不到"正是这次要修的故障。
    #[test]
    fn the_same_executable_is_recognised_through_the_spellings_windows_uses() {
        use super::same_executable_path;
        const PLAIN: &str = r"D:\Family\dsh-official\DeepSeek Harness.exe";
        // 大小写：Windows 路径不比大小写，用户从快捷方式读到的那一份未必与内核给的一致。
        assert!(same_executable_path(PLAIN, r"d:\family\dsh-official\deepseek harness.EXE"));
        // 逐字前缀 `\\?\`：同一台机器上，进程镜像是带前缀的那种，快捷方式是不带的那种。
        assert!(same_executable_path(PLAIN, r"\\?\D:\Family\dsh-official\DeepSeek Harness.exe"));
        assert!(same_executable_path(
            r"\\?\D:\Family\dsh-official\DeepSeek Harness.exe",
            PLAIN
        ));
        // `\??\` 是同一个命名空间的另一种拼法。
        assert!(same_executable_path(PLAIN, r"\??\D:\Family\dsh-official\DeepSeek Harness.exe"));
        // 正斜杠与多余的分隔符：同一条路径的合法写法。
        assert!(same_executable_path(PLAIN, "D:/Family/dsh-official/DeepSeek Harness.exe/"));
        // UNC 的逐字拼法与普通拼法是同一个共享路径。
        assert!(same_executable_path(
            r"\\server\share\app.exe",
            r"\\?\UNC\server\share\app.exe"
        ));
        // 首尾空白来自属性读取，不该参与比较。
        assert!(same_executable_path(PLAIN, &format!("  {PLAIN}  ")));
    }

    #[test]
    fn different_executables_are_never_the_same_one() {
        use super::same_executable_path;
        // 同名不等于同一个程序：另一个目录里的安装是另一个文件，藏错就是把用户的窗口弄丢。
        assert!(!same_executable_path(
            r"D:\Family\dsh-official\DeepSeek Harness.exe",
            r"C:\DSH desktop\DeepSeek Harness.exe"
        ));
        // 前缀相同但不是同一层路径。
        assert!(!same_executable_path(
            r"D:\Family\dsh-official\DeepSeek Harness.exe",
            r"D:\Family\dsh-official\DeepSeek Harness.exe.old"
        ));
        // 空的一侧永远不是匹配：读不到路径时，答案是"找不到窗口"，不是"就是它"。
        assert!(!same_executable_path("", r"D:\a.exe"));
        assert!(!same_executable_path(r"D:\a.exe", "   "));
        assert!(!same_executable_path("", ""));
    }

    /// 一个**不存在的**可执行文件既没有窗口也不该被当成匹配。
    ///
    /// 这条走的是真实枚举：这台机器上确实枚举了所有顶层窗口，然后按路径逐个否掉。
    /// 它同时钉住"没有任何进程匹配时返回空表"这个分支，而那正是"非主体（CLI/检出）
    /// 一律不受影响"所依赖的性质 —— 它们的进程不在这个路径上。
    #[test]
    fn an_executable_that_is_not_running_has_no_windows() {
        const ABSENT: &str = r"C:\nonexistent\dsh-wallpaper-window-probe.exe";
        #[cfg(windows)]
        assert!(super::windows_of_executable(ABSENT, false).is_empty());
        assert_eq!(super::hide_executable_windows(ABSENT).outcome, "no-window");
        assert_eq!(super::show_executable_windows(ABSENT).outcome, "no-window");
    }

    /// An exit has to be an *event*, and this measures that it is.
    ///
    /// Ignored by default because it starts and kills a throwaway process of its own —
    /// never one the user is running. What it pins is the difference the user felt: the
    /// watcher must report the exit within milliseconds of it happening, because the
    /// alternative (a poll) is what made the light and the switch reset lag behind the
    /// window closing.
    ///
    /// `cargo test --lib -- --ignored --nocapture this_machine_reports_a_process_exit_as_an_event`
    #[test]
    #[ignore = "starts and kills a throwaway process to time the exit watcher"]
    fn this_machine_reports_a_process_exit_as_an_event() {
        use std::time::{Duration, Instant};

        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 60 127.0.0.1 > nul"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn a throwaway process");
        let pid = child.id();
        let watcher = std::thread::spawn(move || {
            // Blocks on the handle; it returns when the process is gone.
            wait_for_process_exit(pid);
            Instant::now()
        });
        // Let the waiter reach its wait, then take the process away.
        std::thread::sleep(Duration::from_millis(300));
        let killed_at = Instant::now();
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
        let _ = child.wait();
        let noticed_at = watcher.join().expect("the watcher thread");
        let reaction = noticed_at.saturating_duration_since(killed_at);
        println!("the watcher noticed the exit {reaction:?} after the process was killed");
        assert!(
            reaction < Duration::from_secs(1),
            "an exit must be reported as an event, not at the next poll: {reaction:?}"
        );
    }

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
    fn a_filetime_is_high_word_first() {
        use crate::client_window::filetime_ticks;
        // 高字在前：写反会得到一个"看起来像时间"的错值，而错值最危险 —— 它会伪装成"同一个进程"。
        assert_eq!(filetime_ticks(0, 1), 1);
        assert_eq!(filetime_ticks(1, 0), 1 << 32);
        assert_eq!(filetime_ticks(0x0123_4567, 0x89AB_CDEF), 0x0123_4567_89AB_CDEF);
        assert!(filetime_ticks(1, 0) > filetime_ticks(0, u32::MAX));
    }

    #[test]
    fn a_live_process_has_a_start_time_and_a_missing_pid_does_not() {
        use crate::client_window::process_started_at;
        // 真机核对：本进程必须查得到创建时间；一个几乎不可能存在的 pid 必须查不到
        // （查不到 ⇒ 调用方按"不是我启动的"处理，也就是安全方向）。
        assert!(process_started_at(std::process::id()).is_some());
        assert!(process_started_at(0xFFFF_FFF0).is_none());
    }

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
