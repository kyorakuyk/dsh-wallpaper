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
    EnumWindows, GetClassNameW, GetWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER, SW_HIDE,
    SW_RESTORE, SW_SHOW,
};
#[cfg(windows)]
use windows::core::{w, BOOL, PCWSTR};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint,
    GetDC, InvalidateRect, ReleaseDC, SelectObject, UpdateWindow, HBITMAP, HDC, HGDIOBJ,
    PAINTSTRUCT, SRCCOPY,
};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassExW,
    SetWindowLongPtrW, WindowFromPoint, CREATESTRUCTW, GWLP_USERDATA, SW_SHOWNA,
    WM_ERASEBKGND, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
#[cfg(windows)]
use windows::Win32::Foundation::{HINSTANCE, LRESULT, POINT, WPARAM};

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

/// Bring every window of one executable's process family forward.
///
/// The showing half of the same identity, and the deliberate mirror of
/// [`raise_client_window`]: a window the wallpaper hid is *invisible*, not minimised,
/// so this is what brings back the client that was started in the background (§6.1).
/// An on-screen window is always the right answer; only when the executable has none
/// does the search accept an invisible window that still looks like the client's own
/// (title and size), which is exactly the state the wallpaper created.
///
/// 显示的**只**是界面窗口：菜单、对话框、提示、输入法、托盘宿主窗口都不在这里被"显示"，
/// 与隐藏那一侧同一条规则（[`is_popup_or_helper_class`]）。显示一个已经可见的窗口本来就是
/// 空操作，所以真正的区别在"什么时候显示"：一个弹出来的菜单不该因为我们调用了一次「打开」
/// 而被当成客户端窗口拿到前台。
///
/// The outcome codes are the ones the renderer already words: `raised` when Windows
/// accepted the foreground change, `raise-refused` when it did not (normal for a
/// desktop wallpaper — the windows are shown and restored either way), and
/// `no-window` when the executable owns no window to show.
pub fn show_executable_windows(executable: &str) -> RaiseOutcome {
    #[cfg(windows)]
    {
        let windows = family_windows(executable);
        let mut targets: Vec<WindowHandle> = windows
            .iter()
            .filter(|window| window.visible && window.is_interface_shaped())
            .map(|window| window.handle)
            .collect();
        if targets.is_empty() {
            targets = windows
                .iter()
                .filter(|window| window.is_unshown_client_window())
                .map(|window| window.handle)
                .collect();
        }
        if targets.is_empty() {
            return RaiseOutcome::no_window();
        }
        let mut foreground = false;
        for window in targets {
            unsafe {
                // The same order as `raise_client_window`, for the same two reasons:
                // `SW_SHOW` first because the visible-and-hidden pair is ours to
                // decide in both directions, `SW_RESTORE` then because a minimised
                // window would otherwise be "brought forward" without being visible.
                let _ = ShowWindow(window.hwnd(), SW_SHOW);
                let _ = ShowWindow(window.hwnd(), SW_RESTORE);
                foreground |= SetForegroundWindow(window.hwnd()).as_bool();
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
///
/// 公开给 crate 内的另一个用途：把"用户要的是这个壳的窗口"这件事记下来时，要记成一个**可比较**
/// 的形式，而不是用户当时给的那一份拼法（同一个文件在快捷方式、进程镜像与命令行里可以有三种
/// 拼法，见 [`same_executable_path`]）。
pub(crate) fn normalized_executable_path(path: &str) -> String {
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
    process_snapshot()
        .into_iter()
        .find(|(candidate, _, _)| *candidate == pid)
        .map(|(_, parent, _)| parent)
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

/// 弹出菜单的窗口类名。
///
/// Win32 给每一个 `TrackPopupMenu` 出来的菜单用这一个类名 —— 托盘图标右键菜单、窗口右键菜单、
/// 组合框下拉都在其中。它是**真正的那条线索**：这一类窗口属于弹出它的那个可执行文件，所以
/// "把这个家族所有可见窗口都藏起来"必然把用户刚点开的菜单一起吃掉。
pub(crate) const MENU_CLASS: &str = "#32768";

/// 这个类名是不是"弹出物 / 框架自己的辅助窗口"。
///
/// 三类，各有来历：
///
/// * **菜单与对话框**（`#32768`、`#32770`）与**工具提示**（`tooltips_class32`）——它们出现的
///   全部意义就是让人看见。用户点托盘图标弹出菜单、壳弹出更新对话框，都不是"客户端自己跑出来
///   的窗口"，藏它们是把用户正在读的东西拿走。
/// * **电子壳的宿主窗口**（`Electron_NotifyIconHostWindow`、`Electron_SystemPreferencesHostWindow`
///   —— 按前缀匹配 `electron_*hostwindow`，将来多一个同类也不用改这里）：托盘图标、系统偏好这类
///   面的宿主。它们本来就不可见（藏它们是无操作），但把它们写进"允许隐藏"的名单没有任何好处，
///   而万一藏掉了托盘图标，用户就失去了把窗口找回来的一条路。
/// * **输入法**（`MSCTFIME UI`、`IME`）：属于**用户正在打字**的那条链。
///
/// 读不到类名（空串）也回答"是"：一个我们说不清是什么的窗口，少藏一个永远比多藏一个好。
pub(crate) fn is_popup_or_helper_class(class: &str) -> bool {
    let class = class.trim().to_ascii_lowercase();
    if class.is_empty() {
        return true;
    }
    if class == MENU_CLASS.to_ascii_lowercase().as_str()
        || class == "#32770"
        || class == "tooltips_class32"
    {
        return true;
    }
    if class == "msctfime ui" || class == "ime" {
        return true;
    }
    class.starts_with("electron_") && class.ends_with("hostwindow")
}

/// 一个顶层窗口能不能被隐藏 —— 规则本身，与机器无关。
///
/// 两条，缺一不可：
///
/// * **它得在屏幕上**。藏一个已经不可见的窗口是空操作，而"提前藏好"在 Windows 上没有任何
///   latch 效果（实测：外部隐藏挡不住壳自己的下一次 `show()`），所以只藏看得见的。
/// * **它不能是别人的**。有属主的窗口是菜单、对话框、提示、输入法 —— 它们的存在就是为了让人
///   看见，而且下面那条类名规则是第二道闸（有些壳会给对话框一个无属主的顶层窗口）。
pub(crate) fn is_hideable_top_level(visible: bool, owned: bool, class: &str) -> bool {
    visible && !owned && !is_popup_or_helper_class(class)
}

/// 一个顶层窗口的句柄，连同它属主的进程 id。
///
/// 两个一起记，是因为句柄会被回收：1 毫秒前记下的句柄，用之前必须再确认一遍"它还是那个进程的
/// 窗口"（见 [`hide_window`]）。记下一个裸句柄而不记 pid，就等于赌它不会被别人接手。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowHandle {
    raw: isize,
    pid: u32,
}

impl WindowHandle {
    /// 这个句柄当时属于哪个进程。
    pub(crate) fn pid(self) -> u32 {
        self.pid
    }

    #[cfg(windows)]
    fn hwnd(self) -> HWND {
        HWND(self.raw as *mut core::ffi::c_void)
    }

    /// 一个顶层窗口的句柄，连同当时问出来的属主进程。
    ///
    /// 公开给 crate：真机测量那一条测试自己造一个离屏窗口来量"发现延迟"，它需要用**与产品代码
    /// 相同**的方式构造句柄 —— 否则量到的就不是产品那条路。
    #[cfg(windows)]
    pub(crate) fn from(hwnd: HWND, pid: u32) -> Self {
        Self { raw: hwnd.0 as isize, pid }
    }
}

/// 一次枚举里看到的关于一个顶层窗口的全部事实。
///
/// 每一条都便宜到可以在毫秒级轮询里反复取；而**唯一贵的那一步** —— "这个窗口的属主跑的是哪个
/// 可执行文件"（`OpenProcess` + 查询）—— 被推到"便宜的事实过滤之后"才做。这是让 1 毫秒级的
/// 检查成为可能的那一半。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowFacts {
    pub handle: WindowHandle,
    pub visible: bool,
    /// 有属主（`GW_OWNER` 非空）：菜单、对话框、提示、输入法这类窗口。
    pub owned: bool,
    pub class: String,
    pub has_title: bool,
    pub has_size: bool,
}

impl WindowFacts {
    /// 此刻能不能藏（[`is_hideable_top_level`] 的实例版）。
    pub(crate) fn is_hideable(&self) -> bool {
        is_hideable_top_level(self.visible, self.owned, &self.class)
    }

    /// 用户刚点开的弹出菜单。
    ///
    /// 这条事实只有一个用途：**证明有人正在跟这个壳打交道**。循环自己不会弹菜单，所以看见它
    /// 就是看见了人手 —— 而那次人手的下一步多半就是"把我的窗口还给我"。
    pub(crate) fn is_menu(&self) -> bool {
        self.visible && self.class.trim().eq_ignore_ascii_case(MENU_CLASS)
    }

    /// 形状像"这个程序的界面"，无论此刻可见不可见：无属主，且不是弹出物/辅助窗口。
    pub(crate) fn is_interface_shaped(&self) -> bool {
        !self.owned && !is_popup_or_helper_class(&self.class)
    }

    /// 壳自己 `show()` 会显示的那一个：**已经建好、还没显示**的界面窗口。
    ///
    /// 这是整个竞速里最有价值的一条事实。主窗口在壳启动的早期就用 `show: false` 建好了
    /// （`app.asar!/lib/main.js:10600`、`:11520-11521`），而 `show()` 要等宿主运行时报告就绪
    /// （`:11569-11576`）—— 中间是好几秒。在那几秒里先认出这个句柄，之后的每一次检查都只是
    /// 一次 `IsWindowVisible`（约 1 微秒），而不是一次全量枚举。
    pub(crate) fn is_unshown_client_window(&self) -> bool {
        !self.visible && self.is_interface_shaped() && self.has_title && self.has_size
    }

    /// 退一步的同一个问题：类名是 Chromium 的浏览器窗口，但标题/矩形还没准备好。
    ///
    /// 保留这一档是因为"标题与矩形在第一次显示之前就绪"这件事**没有实测过**（只实测过
    /// 已显示的窗口有标题与矩形）。认错一个隐藏的壳窗口只会让 tick 空转，而认不出它会让竞速
    /// 退回全量枚举 —— 两种误判的代价不对称，所以这里多给一条路。
    pub(crate) fn is_unshown_browser_window(&self) -> bool {
        !self.visible && self.is_interface_shaped() && is_browser_window_class(&self.class)
    }

    /// 还没显示、但看起来就是"壳会显示的那一个"。
    pub(crate) fn is_unshown_interface_window(&self) -> bool {
        self.is_unshown_client_window() || self.is_unshown_browser_window()
    }
}

/// Chromium 给它的浏览器窗口用的类名 —— 电子壳的主窗口就是这个类。
fn is_browser_window_class(class: &str) -> bool {
    class.trim().eq_ignore_ascii_case("Chrome_WidgetWin_1")
}

/// 一个顶层窗口的类名，读不到就是空串（空串在规则里等于"不是界面"）。
#[cfg(windows)]
fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

/// 一个顶层窗口有没有属主。
///
/// `GetWindow(hwnd, GW_OWNER)` 在没有属主时返回空句柄（而不是报错），所以两条都算"没有属主"：
/// 这是安全方向 —— 有属主的窗口我们一律不动，而没有属主的窗口还要过类名那道闸。
#[cfg(windows)]
fn window_is_owned(hwnd: HWND) -> bool {
    unsafe { GetWindow(hwnd, GW_OWNER) }
        .map(|owner| !owner.0.is_null())
        .unwrap_or(false)
}

/// 一次枚举取到的全部事实，装在一个可变的接收器里交给回调。
#[cfg(windows)]
struct FamilyWindowSearch {
    /// 属于目标可执行文件家族的进程。
    ///
    /// 用一次 Toolhelp 快照 + 文件名初筛得到，再用完整路径逐个确认（见
    /// [`family_process_ids`]）。判定仍然是路径，这里只是把"每个窗口都问一次属主是谁"
    /// 换成"每个家族进程问一次"。
    pids: Vec<u32>,
    found: Vec<WindowFacts>,
}

#[cfg(windows)]
unsafe extern "system" fn collect_family_windows(hwnd: HWND, param: LPARAM) -> BOOL {
    let search = unsafe { &mut *(param.0 as *mut FamilyWindowSearch) };
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
    if owner == 0 || !search.pids.contains(&owner) {
        return BOOL(1);
    }
    // 便宜的事实：可见性、属主、类名、标题、矩形。没有一个是 `OpenProcess`。
    let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    let owned = window_is_owned(hwnd);
    let class = class_name(hwnd);
    let has_title = unsafe { GetWindowTextLengthW(hwnd) } > 0;
    let mut rect = RECT::default();
    let has_size = unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok()
        && rect.right > rect.left
        && rect.bottom > rect.top;
    search.found.push(WindowFacts {
        handle: WindowHandle::from(hwnd, owner),
        visible,
        owned,
        class,
        has_title,
        has_size,
    });
    // 继续走：一个家族可能有不止一个窗口，而"第二个还留在屏幕上"正是要消灭的失败。
    BOOL(1)
}

/// 一个可执行文件的进程家族此刻拥有的**全部**顶层窗口及其形状。
///
/// 每个窗口都重新确认属主仍是这个家族：枚举本身不保证自洽（枚举期间进程可以退出），而隐藏一个
/// 别人的窗口是不可逆的坏事（它的主人不知道它去哪了）。
#[cfg(windows)]
pub(crate) fn family_windows(executable: &str) -> Vec<WindowFacts> {
    let pids = family_process_ids(executable);
    if pids.is_empty() {
        return Vec::new();
    }
    let mut search = FamilyWindowSearch { pids, found: Vec::new() };
    unsafe {
        let _ = EnumWindows(
            Some(collect_family_windows),
            LPARAM(&mut search as *mut _ as isize),
        );
    }
    search.found
}

#[cfg(not(windows))]
pub(crate) fn family_windows(_executable: &str) -> Vec<WindowFacts> {
    Vec::new()
}

/// 藏一个句柄，**先重新确认它仍然是那个进程的界面窗口**。
///
/// 四问，任一不成立就不动它：pid 还是不是它（句柄回收）、还在不在屏幕上、有没有属主、
/// 类名是不是弹出物。这不是形式主义：句柄回收之后，同一个数值可能属于别人刚建出来的窗口，
/// 而"藏错窗口"对一个桌面应用来说是最坏的一类错误。
///
/// 「把一整个家族的隐藏窗口都藏掉」**没有**一个独立函数出口，这不是漏了：隐藏那一半现在只有
/// 一处调用者 —— 后台启动的隐藏循环，而那一处需要的是**同一遍枚举**里的另外两件事（"用户刚
/// 点过托盘菜单"与"哪个窗口已经建好但还没显示"）。再开一个只藏窗口的入口就得再枚举一遍窗口，
/// 而枚举是这条路上唯一贵的那一步（一张进程快照 + 一次 `EnumWindows`）。所以那个循环自己遍历
/// 一遍枚举结果，对每一个候选调用这个函数 —— 判定则永远是 [`is_hideable_top_level`]：
/// **在屏幕上、没有属主、类名不是弹出物**。菜单那条规则修的是一个真实故障：把"这个家族所有
/// 可见窗口"一股脑藏掉，会把用户右键托盘时刚弹出来的菜单一起吃掉（`#32768` 是同一个可执行
/// 文件的一个顶层窗口），用户描述为"点一下托盘客户端就崩了"。
pub(crate) fn hide_window(handle: WindowHandle) -> bool {
    #[cfg(windows)]
    {
        let hwnd = handle.hwnd();
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        if owner == 0 || owner != handle.pid {
            return false;
        }
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return false;
        }
        if window_is_owned(hwnd) || is_popup_or_helper_class(&class_name(hwnd)) {
            return false;
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        false
    }
}

/// 这个句柄此刻是不是一个在屏幕上的窗口，而且仍然是那个进程的。
///
/// 竞速期每一毫秒问一次的就是它：两次只读系统调用，不枚举、不 `OpenProcess`。
pub(crate) fn window_is_visible(handle: WindowHandle) -> bool {
    #[cfg(windows)]
    {
        let hwnd = handle.hwnd();
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        owner != 0 && owner == handle.pid && unsafe { IsWindowVisible(hwnd) }.as_bool()
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        false
    }
}

/// 一个被盯着的句柄此刻处于哪一种状态 —— 紧盯着它的那台循环要的就是这三选一。
///
/// 为什么不是"可见/不可见"两选一：那台循环问这个问题的目的是**决定要不要把窗口按下去**，而
/// "不可见"里混着两种完全不同的处境 —— "还是我们那个窗口，只是还没显示"（继续盯）与"它已经不是
/// 我们能碰的东西了"（必须放手，让别人重新认一个）。把后者当成前者，就是在一个我们永远不该动的
/// 句柄上每秒空转一千次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchWindowState {
    /// 在屏幕上，而且仍然是可以藏的那一类：这就是要按下去的那一刻。
    Visible,
    /// 属于那个进程、没有属主、类名也不是弹出物，只是还没显示：继续盯。
    Hidden,
    /// 句柄回收了、换了属主、改了类名，或者进程已经不是它：**不再是我们能碰的窗口**。
    Lost,
}

/// 盯着一个句柄时问的那个问题：它此刻是哪一态。
///
/// 三次只读、跨进程的系统调用（`GetWindowThreadProcessId` + `GetWindow(GW_OWNER)` +
/// `GetClassNameW`），每次都在微秒量级 —— 这是 1 毫秒 tick 之所以可能的第二半。**从不枚举**：
/// 整机有 538 个顶层窗口，枚举一次约 30 毫秒，放进这条路就等于把帧拉长三十倍。
pub(crate) fn watch_window_state(handle: WindowHandle) -> WatchWindowState {
    #[cfg(windows)]
    {
        let hwnd = handle.hwnd();
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        // 句柄回收：同一个数值可能已经属于别人刚建出来的窗口。这条判定必须先过。
        if owner == 0 || owner != handle.pid {
            return WatchWindowState::Lost;
        }
        // 属主与类名：菜单、对话框、输入法，以及"说不清是什么"的窗口（空类名）一律放手。
        if window_is_owned(hwnd) || is_popup_or_helper_class(&class_name(hwnd)) {
            return WatchWindowState::Lost;
        }
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            WatchWindowState::Visible
        } else {
            WatchWindowState::Hidden
        }
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        WatchWindowState::Lost
    }
}

/// 这个端口后面那个客户端此刻有没有窗口在屏幕上。
///
/// 只有"记录里没有可执行文件路径"的降级路径会用到它：那时按端口那条路一次只能回答一个窗口，
/// 但"屏幕上有没有它"这个问题它答得了。
pub(crate) fn endpoint_window_on_screen(port: u16) -> bool {
    #[cfg(windows)]
    {
        window_for_endpoint(port).is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        false
    }
}

/// 把系统计时器分辨率抬到 1 毫秒，并在离开作用域时还回去。
///
/// 为什么需要它：`std::thread::sleep(1ms)` 在默认分辨率（15.6 毫秒）下可能睡十几毫秒，
/// 而那正好是"一帧能被看见多久"这件事的量级 —— 抬分辨率之前，1 毫秒的 tick 是个空头承诺。
/// 从 Windows 10 2004 起它只影响本进程，且这个守卫只在竞速期间存在、出作用域就还，所以它不会
/// 变成那种"某个程序把全系统计时器钉在 1 毫秒"的坏邻居。
pub(crate) struct TimerResolution {
    #[cfg(windows)]
    raised: bool,
}

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}

impl TimerResolution {
    pub(crate) fn new() -> Self {
        #[cfg(windows)]
        {
            // 返回 0 表示成功；失败也只是让 Sleep 的粒度粗一点，没有别的后果。
            Self { raised: unsafe { timeBeginPeriod(1) } == 0 }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if self.raised {
                unsafe { timeEndPeriod(1) };
            }
        }
    }
}

/// 这个可执行文件此刻有哪些进程在跑。
///
/// 两步，方向是"先便宜后贵"：一次 Toolhelp 快照给出**文件名**（每一条进程记录里就有，不需要
/// `OpenProcess`），再用完整路径确认。判定仍然是路径 —— 同名不等于同一个程序，另一份安装是
/// 另一个文件，而那正是"藏错窗口"最常见的成因。
#[cfg(windows)]
pub(crate) fn family_process_ids(executable: &str) -> Vec<u32> {
    let Some(name) = std::path::Path::new(executable)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
    else {
        return Vec::new();
    };
    if name.is_empty() {
        return Vec::new();
    }
    let mut ids = Vec::new();
    for (pid, _parent, exe_name) in process_snapshot() {
        if exe_name != name {
            continue;
        }
        if process_image_path(pid).is_some_and(|path| same_executable_path(&path, executable)) {
            ids.push(pid);
        }
    }
    ids
}

#[cfg(not(windows))]
pub(crate) fn family_process_ids(_executable: &str) -> Vec<u32> {
    Vec::new()
}

/// 这个可执行文件的家族里**任意一个**进程，用来回答"它出现了没有"。
///
/// 回答的是时间而不是身份：哪一个进程不重要（家族是同一个文件），重要的是最早那一刻。调用方
/// 只拿它记一条时间线，不拿它做任何判定。
pub(crate) fn first_process_of_executable(executable: &str) -> Option<u32> {
    family_process_ids(executable).into_iter().min()
}

/// 一张进程表的快照：`(pid, 父 pid, 可执行文件名)`。
///
/// 一次 Toolhelp 遍历同时给出父进程（祖先进程遍历要用）与文件名（家族初筛要用），所以这张表
/// 只写一遍。两者都不需要 `OpenProcess`。
#[cfg(windows)]
fn process_snapshot() -> Vec<(u32, u32, String)> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };

    let mut entries = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return entries;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                entries.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    process_entry_name(&entry.szExeFile),
                ));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    entries
}

/// `PROCESSENTRY32W::szExeFile` 是一个定长宽字符数组，到第一个 0 为止，小写化以便比较。
#[cfg(windows)]
fn process_entry_name(field: &[u16]) -> String {
    let end = field.iter().position(|value| *value == 0).unwrap_or(field.len());
    String::from_utf16_lossy(&field[..end]).trim().to_ascii_lowercase()
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


/// 一次后台启动里，盖在壳窗口将来会出现的那块屏幕上的"冻结画面"。
///
/// 为什么要它：壳的 `show()` 与我们的隐藏之间有一段躲不掉的赛跑 —— 实测三次分别是 80ms、31ms、
/// 以及把枚举移出 tick 之后的个位数毫秒，**用户仍然看得见**。那条路上还有一个物理下限：壳画出
/// 第一帧、我们再跨进程把它按下去，这中间必然经过一次呈现。所以换个轴：不去比它快，而是**让它
/// 出生在一层面底下**。
///
/// 关键在于时间：句柄在壳显示之前几百毫秒就已经存在（实测 +2648ms 拿到句柄、+2970ms 才显示），
/// 所以我们有充裕时间先铺面。而这层面之所以用户看不见，是因为铺之前**先把那块屏幕原样截下来**，
/// 铺上之后画面与刚才逐像素相同 —— 它在视觉上等于不存在，却挡住了壳的窗口。
///
/// 三条自我约束：
/// * **只在"那块地方此刻是我们的画面"时才铺**：如果矩形中心属于别的程序的窗口，就不铺（把别人
///   正在动的东西冻住一秒钟，比闪一下更讨厌）。
/// * **不激活、不吃鼠标**（`WS_EX_NOACTIVATE | WS_EX_TRANSPARENT`）：用户这一瞬间的点击照旧落到
///   原来那个窗口上。
/// * **活得比需要更短**：隐藏一落地就撤（`Drop` 保证任何返回路径都会撤）。
pub(crate) struct LaunchCover {
    #[cfg(windows)]
    hwnd: HWND,
    #[cfg(windows)]
    state: *mut CoverState,
    raised_at: std::time::Instant,
}

#[cfg(windows)]
struct CoverState {
    memory: HDC,
    bitmap: HBITMAP,
    width: i32,
    height: i32,
}

#[cfg(windows)]
const COVER_CLASS: PCWSTR = w!("DSHWallpaperLaunchCover");

#[cfg(windows)]
unsafe extern "system" fn cover_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = lparam.0 as *const CREATESTRUCTW;
            if !create.is_null() {
                let state = (*create).lpCreateParams as *mut CoverState;
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut CoverState;
            let mut paint = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
            if !state.is_null() && !hdc.0.is_null() {
                let state = unsafe { &*state };
                // 逐像素搬回来：这层要的效果就是"和刚才一模一样"。
                let _ = unsafe {
                    BitBlt(hdc, 0, 0, state.width, state.height, Some(state.memory), 0, 0, SRCCOPY)
                };
            }
            let _ = unsafe { EndPaint(hwnd, &paint) };
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// 那块地方此刻是不是"我们的画面"。
///
/// 判断依据是矩形中心点下面的那个窗口属于谁：属于本进程（壁纸自己的界面）或桌面（`Progman`/
/// `WorkerW`，壁纸的背景就画在里面）才算数；属于任何别的程序就不铺 —— 我们不该把别人正在动的
/// 画面冻住，哪怕只有一秒。
#[cfg(windows)]
fn cover_is_our_surface(x: i32, y: i32) -> bool {
    let under = unsafe { WindowFromPoint(POINT { x, y }) };
    if under.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(under, Some(&mut pid)) };
    if pid == unsafe { windows::Win32::System::Threading::GetCurrentProcessId() } {
        return true;
    }
    let class = class_name(under).to_ascii_lowercase();
    class == "progman" || class == "workerw"
}

/// 在 `target` 窗口的矩形上铺一层冻结画面。`None` = 这块地方不该铺（不在屏上，或此刻不是我们的画面）。
#[cfg(windows)]
pub(crate) fn raise_launch_cover(target: WindowHandle) -> Option<LaunchCover> {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(target.hwnd(), &mut rect) }.is_err() {
        return None;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    // 先注册窗口类 —— 少这一步，`CreateWindowExW` 会失败，而失败在这里是**静默**的（只是不铺），
    // 于是我们会以为遮盖生效了。所以它必须先过。
    if !register_cover_class() {
        return None;
    }
    if !cover_is_our_surface(rect.left + width / 2, rect.top + height / 2) {
        return None;
    }

    // 截屏：把这块屏幕原样抄进一块内存位图。
    let screen = unsafe { GetDC(None) };
    if screen.0.is_null() {
        return None;
    }
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    let bitmap = if memory.0.is_null() {
        unsafe { ReleaseDC(None, screen) };
        return None;
    } else {
        unsafe { CreateCompatibleBitmap(screen, width, height) }
    };
    if bitmap.0.is_null() {
        unsafe { DeleteDC(memory) };
        unsafe { ReleaseDC(None, screen) };
        return None;
    }
    let previous = unsafe { SelectObject(memory, HGDIOBJ(bitmap.0)) };
    let captured = unsafe {
        BitBlt(memory, 0, 0, width, height, Some(screen), rect.left, rect.top, SRCCOPY)
    };
    let _ = unsafe { SelectObject(memory, previous) };
    unsafe { ReleaseDC(None, screen) };
    if captured.is_err() {
        unsafe { DeleteObject(HGDIOBJ(bitmap.0)) };
        unsafe { DeleteDC(memory) };
        return None;
    }

    let state = Box::into_raw(Box::new(CoverState { memory, bitmap, width, height }));
    let hwnd = match unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            COVER_CLASS,
            w!("DSH Wallpaper launch cover"),
            WS_POPUP,
            rect.left,
            rect.top,
            width,
            height,
            None,
            None,
            Some(cover_instance()),
            Some(state.cast()),
        )
    } {
        Ok(hwnd) => hwnd,
        Err(error) => {
            log::warn!("launch cover: 无法创建遮盖窗口（{error}）");
            unsafe { drop(Box::from_raw(state)) };
            return None;
        }
    };
    // `SW_SHOWNA` 不激活；随后立刻同步重绘一次，确保它**在壳显示之前**就已经画好 —— 否则就白铺了。
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNA) };
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
    let _ = unsafe { UpdateWindow(hwnd) };
    Some(LaunchCover { hwnd, state, raised_at: std::time::Instant::now() })
}

#[cfg(windows)]
fn cover_instance() -> HINSTANCE {
    match unsafe { GetModuleHandleW(None) } {
        Ok(module) => HINSTANCE(module.0),
        Err(_) => HINSTANCE::default(),
    }
}

#[cfg(windows)]
fn register_cover_class() -> bool {
    use std::sync::OnceLock;
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    *REGISTERED.get_or_init(|| {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(cover_proc),
            hInstance: cover_instance(),
            lpszClassName: COVER_CLASS,
            ..Default::default()
        };
        let atom = unsafe { RegisterClassExW(&class) };
        if atom == 0 {
            log::warn!("launch cover: 窗口类注册失败");
        }
        atom != 0
    })
}

impl Drop for LaunchCover {
    fn drop(&mut self) {
        log::info!(
            "launch cover removed after {}ms",
            self.raised_at.elapsed().as_millis()
        );
        #[cfg(windows)]
        {
            let _ = unsafe { DestroyWindow(self.hwnd) };
            if !self.state.is_null() {
                let state = unsafe { Box::from_raw(self.state) };
                unsafe { DeleteObject(HGDIOBJ(state.bitmap.0)) };
                unsafe { DeleteDC(state.memory) };
            }
        }
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
        assert!(super::family_windows(ABSENT).is_empty());
        assert!(super::family_process_ids(ABSENT).is_empty());
        assert!(super::first_process_of_executable(ABSENT).is_none());
        // 一个不存在的句柄（或别人的句柄）永远不许被当成"可以藏的窗口"。
        assert!(!super::hide_window(super::WindowHandle { raw: 1, pid: 2 }));
        assert!(!super::window_is_visible(super::WindowHandle { raw: 1, pid: 2 }));
        assert_eq!(super::show_executable_windows(ABSENT).outcome, "no-window");
    }

    /// 盯着的句柄每一毫秒问的那一态：三选一，而且"放手"那一条必须先判。
    ///
    /// 这里能钉住的只有"放手"这一侧 —— 造一个真的、可藏的、已经建好但还没显示的窗口不是单元
    /// 测试该做的事（真机那条 `this_machine_measures_the_hide_race` 用离屏窗口量它）。而这一侧
    /// 恰好是最危险的一侧：判错就是把一个我们永远不该碰的句柄留住。
    #[test]
    fn a_handle_that_is_not_ours_is_lost_rather_than_something_to_watch() {
        use super::{watch_window_state, WatchWindowState, WindowHandle};
        // 句柄值不是任何窗口：pid 那一条就否掉了。
        assert_eq!(
            watch_window_state(WindowHandle { raw: 1, pid: 2 }),
            WatchWindowState::Lost
        );
        // 句柄值是本进程的一个真实窗口，但记下来的 pid 不是它的属主 —— 这正是**句柄回收**的
        // 形状：1 毫秒前记下的数值，现在归别人了。记着一个裸句柄就等于赌它不会被接手。
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetWindowThreadProcessId};
            let desktop = unsafe { GetDesktopWindow() };
            let mut owner = 0u32;
            unsafe { GetWindowThreadProcessId(desktop, Some(&mut owner)) };
            assert_ne!(owner, 0);
            #[cfg(windows)]
            let handle = super::WindowHandle::from(desktop, owner.wrapping_add(1));
            assert_eq!(watch_window_state(handle), WatchWindowState::Lost);
            // pid 相符时，桌面窗口被判成 `Visible` —— 这是对的，不是漏判。它永远不会被盯上：
            // 发现路径只在这个家族自己的窗口里按**可执行文件路径**挑，桌面不属于那个家族。这道闸
            // 管的是"弹出物与辅助窗口"，它不假装能判断"这是不是一个界面窗口"；把桌面当成反例
            // 要求它说 `Lost`，等于把一条谁都用不上的规则塞进这条每毫秒都要跑的路径。
            let desktop_handle = super::WindowHandle::from(desktop, owner);
            assert_eq!(watch_window_state(desktop_handle), WatchWindowState::Visible);
        }
    }

    /// 菜单与壳的辅助窗口**永远**不在可隐藏之列 —— 这条规则修的是一个真实故障。
    ///
    /// 用户右键托盘图标，菜单一闪就没了，他描述为"客户端崩了"。原因就是那条"把这个家族所有
    /// 可见窗口都藏起来"的规则：Win32 的弹出菜单是**同一个可执行文件的一个顶层窗口**
    /// （类名 `#32768`），于是壁纸把用户刚点开的菜单一起藏了。鼠标划到菜单上时会同时命中
    /// `Electron_NotifyIconHostWindow`、`MSCTFIME UI`、`IME` 这几个同类。
    #[test]
    fn a_menu_or_a_helper_window_is_never_something_to_hide() {
        use super::{is_hideable_top_level, is_popup_or_helper_class};
        // 这台机器上真实枚举到的那几个类名。
        for class in [
            "#32768",
            "#32770",
            "Electron_NotifyIconHostWindow",
            "Electron_SystemPreferencesHostWindow",
            "MSCTFIME UI",
            "IME",
            "tooltips_class32",
        ] {
            assert!(is_popup_or_helper_class(class), "{class} must not be hideable");
            assert!(!is_hideable_top_level(true, false, class), "{class} must not be hideable");
        }
        // 类名的大小写来自 API，不保证与我们写的字面量一致。
        assert!(is_popup_or_helper_class("electron_notifyiconhostwindow"));
        assert!(is_popup_or_helper_class("  #32768  "));
        // 主窗口的类名照常可藏。
        assert!(is_hideable_top_level(true, false, "Chrome_WidgetWin_1"));
        // 读不到类名 ⇒ 不藏：说不清是什么的窗口，少藏一个永远比多藏一个好。
        assert!(is_popup_or_helper_class(""));
        assert!(!is_hideable_top_level(true, false, ""));
        // 有属主的一律不藏（菜单、对话框、提示、输入法大多数落在这里）。
        assert!(!is_hideable_top_level(true, true, "Chrome_WidgetWin_1"));
        // 不在屏幕上的一律不藏：对不可见窗口施加隐藏是空操作，提前藏也没有 latch 效果。
        assert!(!is_hideable_top_level(false, false, "Chrome_WidgetWin_1"));
    }

    /// "壳会显示的那一个窗口"要认得出来，而且两档之间的取舍不能反。
    #[test]
    fn the_window_the_shell_has_not_shown_yet_is_recognised_by_shape() {
        use super::{is_browser_window_class, WindowFacts, WindowHandle};
        let shape = |visible: bool, owned: bool, class: &str, has_title: bool, has_size: bool| WindowFacts {
            handle: WindowHandle { raw: 1, pid: 2 },
            visible,
            owned,
            class: class.to_string(),
            has_title,
            has_size,
        };
        // 第一档：无属主、不是弹出物、有标题与矩形、还没显示 —— 这就是壳建好但没显示的主窗口。
        let unshown = shape(false, false, "Chrome_WidgetWin_1", true, true);
        assert!(unshown.is_unshown_client_window());
        assert!(unshown.is_unshown_interface_window());
        // 已经显示 ⇒ 不是"还没显示的窗口"（那时该走隐藏/显示那两条路，而不是 tick）。
        assert!(!shape(true, false, "Chrome_WidgetWin_1", true, true).is_unshown_client_window());
        // 第二档：标题或矩形还没就绪，但类名是 Chromium 的浏览器窗口。
        let bare = shape(false, false, "Chrome_WidgetWin_1", false, false);
        assert!(!bare.is_unshown_client_window());
        assert!(bare.is_unshown_browser_window());
        assert!(bare.is_unshown_interface_window());
        // 弹出物与有属主的窗口两档都不认：它们不是"壳会显示的界面"。
        let menu = shape(false, false, "#32768", true, true);
        assert!(!menu.is_unshown_interface_window());
        let owned = shape(false, true, "Chrome_WidgetWin_1", true, true);
        assert!(!owned.is_unshown_interface_window());
        // 别的类名不再是第二档（避免把某个隐藏的框架窗口当成主窗口）。
        assert!(!is_browser_window_class("Chrome_MessageWindow"));
        assert!(!shape(false, false, "Chrome_MessageWindow", false, false).is_unshown_interface_window());
        // 菜单这条事实只认可见的菜单：隐藏的菜单窗口不是"有人在点托盘"。
        assert!(shape(true, true, "#32768", true, true).is_menu());
        assert!(!shape(false, true, "#32768", true, true).is_menu());
        assert!(!shape(true, true, "Chrome_WidgetWin_1", true, true).is_menu());
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
