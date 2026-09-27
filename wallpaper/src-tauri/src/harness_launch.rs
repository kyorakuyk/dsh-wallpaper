//! The shim's "start" half: turn a chosen execution subject into a running one.
//!
//! [`crate::harness_targets`] answers *what* can be started and how each class is
//! addressed; this module does the starting, and nothing else. The split is
//! deliberate, because it is where the design draws its most useful line
//! (`docs/design/harness-subject-and-ui-design.md` §4–§6):
//!
//! * the **policy** — which class, which alias, whether the window may be kept
//!   out of sight, whether an unattended custom launcher needs consent — is a
//!   pure function, [`plan_launch`], and is unit tested without a machine;
//! * the **mechanism** — spawn, wait to confirm, hide the window — is thin, and
//!   is the part that cannot be shared between classes at all.
//!
//! Three rules are enforced here rather than in the renderer, because a renderer
//! can be wrong:
//!
//! * **Only known shells are launchable.** The launch token is a Windows shell
//!   request, so an id from outside this build's own table is refused rather than
//!   passed through (`known_shell`).
//! * **An existing client is never taken over.** A subject whose port already
//!   answers is reported as running and left completely alone (§4.6).
//! * **A refusal is a code, never prose.** Every outcome is one of the closed
//!   codes below, so the wording lives in one place in the renderer.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::harness_targets::{known_shell, HarnessTargetKind, CLI_ID_PREFIX, SHELL_ID_PREFIX};

/// How long a shell is given to answer before its start is reported as
/// unconfirmed. Generous on purpose: an Electron client's first start after a
/// login is slow, and a false "failed" would be worse than a slow "started".
const SHELL_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const SHELL_START_POLL: std::time::Duration = std::time::Duration::from_millis(250);
/// How long a started shell is given to *paint*, which happens after it starts
/// listening. Same order of magnitude as the start timeout, for the same reason.
const UI_WINDOW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// How long a single-instance shell is given to act on its own focus request before
/// the fallback takes over.
///
/// Short on purpose: this route is the *fast* one. It is a no-op while the client is
/// still booting (its focus path is installed after the instance lock, measured), so
/// waiting here would only delay the path that does work.
const SHELL_FOCUS_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1_500);

/// What the wallpaper did, in the renderer's vocabulary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessLaunchOutcome {
    /// Closed outcome code. `started-unconfirmed` means the shell accepted the
    /// request but nothing answered within the timeout — a real outcome, not a
    /// failure to report as one, because the client may still be starting.
    pub outcome: String,
    pub kind: HarnessTargetKind,
    /// Present only when this process started and owns a child (a checkout).
    pub pid: Option<u32>,
    /// True when the subject's window was put out of sight after starting.
    pub hidden: bool,
}

impl HarnessLaunchOutcome {
    fn new(outcome: &str, kind: HarnessTargetKind) -> Self {
        Self {
            outcome: outcome.into(),
            kind,
            pid: None,
            hidden: false,
        }
    }
}

/// A decided launch, before anything is started.
///
/// Splitting the decision out is what makes the interesting rules testable: which
/// class a stored id resolves to, whether a profile is even meaningful, and
/// whether an unattended start is allowed to run the configured launcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LaunchPlan {
    /// Hand the alias to the Windows shell. No path, no arguments: the shell
    /// resolves it through whatever registration exists at launch time.
    Shell {
        aumid: String,
        alias: String,
        /// Default port, used to wait for the start and to resolve the window.
        port: Option<u16>,
        /// Whether the window is hidden after starting (§5.1).
        hide_window: bool,
    },
    /// Spawn the checkout's own launcher through the managed chain.
    Checkout {
        root_path: String,
        profile: String,
        command: Option<String>,
    },
    /// Spawn a globally installed DSH CLI (`npm i -g @deepseek-ai/dsh`).
    ///
    /// It differs from a checkout in the one way that matters here: there is no tree to
    /// run a launcher *from*, only the launcher itself. Everything else — the profile it
    /// boots, the port it answers on — is the same, which is why the caller cannot tell
    /// the two apart once the command line is decided.
    InstalledCli { launcher: String, profile: String },
}

/// The web app's own hand-off address, per port.
///
/// `dsh web` prints `http://127.0.0.1:<port>/?token=<token>`, and its browser fence then
/// refuses a bare `host:port` with "dsh web authentication required; reopen the URL
/// printed by dsh web". The token is minted per start, so it cannot be remembered across
/// launches — this holds the one the *current* host printed, and the browser route hands
/// it back to the user instead of opening a page that can only apologise.
static WEB_HANDOFF: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u16, String>>> =
    std::sync::OnceLock::new();

fn web_handoffs() -> &'static std::sync::Mutex<std::collections::HashMap<u16, String>> {
    WEB_HANDOFF.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 记录文件的位置，由 lib.rs 在启动时告知（打包应用的本地数据目录只有 Tauri 算得准，
/// 不能靠环境变量硬拼）。没设置时一切"记住/忘记"都退化成空操作：少了持久化，行为回到今天
/// 之前的样子，而不是出错。
static RECORDS_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub(crate) fn set_records_path(path: PathBuf) {
    let _ = RECORDS_PATH.set(path);
}

/// 记住"这个主体当前的孩子是 pid"，并落盘。
///
/// 只动**对齐枪管**的那一格：别的格子原样保留，这样"CLI → 客户端 → 切回 CLI"时壁纸仍然认得
/// CLI 那个孩子。写失败只记日志、不影响启动本身（这份记录是参考，不是启动的前提）。
pub(crate) fn remember_child(subject_id: &str, pid: u32) {
    let Some(path) = RECORDS_PATH.get() else { return };
    let mut cylinder = read_managed_children(path);
    cylinder.remember(ManagedChild {
        subject_id: subject_id.to_string(),
        pid,
        started_at: crate::client_window::process_started_at(pid),
    });
    if let Err(error) = write_managed_children(path, &cylinder) {
        log::warn!("harness managed-child record not written: {error}");
    } else {
        log::info!("harness managed-child remembered: subject={subject_id} pid={pid}");
    }
}

/// 由本应用停掉之后清掉**对齐枪管**的那一格；别的格子不受影响。
pub(crate) fn forget_child(subject_id: &str) {
    let Some(path) = RECORDS_PATH.get() else { return };
    let mut cylinder = read_managed_children(path);
    cylinder.forget(subject_id);
    if let Err(error) = write_managed_children(path, &cylinder) {
        log::warn!("harness managed-child record not written: {error}");
    }
}

/// 这个主体**当前对齐那一格**的记录（不做存活校验，校验由 `owns_live_process` 负责）。
pub(crate) fn recorded_child(subject_id: &str) -> Option<ManagedChild> {
    let path = RECORDS_PATH.get()?;
    read_managed_children(path).aligned(subject_id).cloned()
}

/// 这个主体记着的孩子**此刻是否真的还是同一个进程**。
///
/// 任一不确定（没有记录、读不到记录、进程已退出、创建时间对不上）都返回 `None`：调用方据此
/// 认为"不是我启动的"，于是既不会去停它，也不会声称拥有它。
pub(crate) fn owned_child(subject_id: &str) -> Option<ManagedChild> {
    let child = recorded_child(subject_id)?;
    let live_pid = Some(child.pid).filter(|pid| crate::client_window::process_is_alive(*pid));
    let live_start = live_pid.and_then(crate::client_window::process_started_at);
    owns_live_process(Some(&child), live_pid, live_start).then_some(child)
}

/// 读一条记录。**任何不确定都降级为"没有记录"**：文件不存在、读不了、不是 JSON、字段对不上
/// —— 全部当成空的弹仓。方向是刻意的：这份缓存只是参考不是授权，丢了最坏是"壁纸以为孩子不是
/// 自己的"（少一个按钮可用），绝不会变成"去停别人的进程"。
pub(crate) fn read_managed_children(path: &Path) -> ManagedChildren {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<ManagedChildren>(&text).ok())
        .unwrap_or_default()
}

/// 原子写：先写临时文件再改名。
///
/// 直接覆写会让"写到一半被读"成为一种可能，而半个文件恰好可能解析成一个**缺少某格的**弹仓
/// —— 那正是"壁纸认不出自己的孩子"的另一种成因，且难查。改名在同一分区上是原子的。
pub(crate) fn write_managed_children(path: &Path, children: &ManagedChildren) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(children)?)?;
    std::fs::rename(&temp, path)
}

/// 每个主体一条记录 —— 左轮弹仓：一格一个主体。
///
/// **别的格子一律保留**，只动"对齐枪管"的那一格。理由是一个真实场景：用户先用 CLI 对话、
/// 然后切到客户端、再切回 CLI —— 那时壁纸必须还能认出 CLI 那个孩子是自己的，否则它要么重复
/// 启动一个，要么不敢停自己启动的那个。键是主体 id（`shell:<aumid>` / 目录路径 / `cli:<启动器>`），
/// 不是端口：端口不是契约，而且不同主体可以先后用同一个端口。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ManagedChildren {
    /// 用 `BTreeMap`：落盘后顺序稳定，改动一眼可见。
    #[serde(default)]
    pub children: std::collections::BTreeMap<String, ManagedChild>,
}

impl ManagedChildren {
    /// 当前下拉里选中的那个主体对应的记录（"对齐枪管"的那一格）。其它格子一概不看。
    pub(crate) fn aligned(&self, subject_id: &str) -> Option<&ManagedChild> {
        self.children.get(subject_id)
    }

    /// 成功启动后覆盖**这一格**；别的格子原样保留。
    pub(crate) fn remember(&mut self, child: ManagedChild) {
        self.children.insert(child.subject_id.clone(), child);
    }

    /// 由本应用停掉之后清掉**这一格**；别的格子不受影响。
    pub(crate) fn forget(&mut self, subject_id: &str) {
        self.children.remove(subject_id);
    }
}

/// 一份"这个孩子是壁纸启动的"记录，落在盘上，跨壁纸重启有效。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ManagedChild {
    /// 主体 id（`shell:<aumid>` / 目录路径 / `cli:<启动器>`）—— 不是端口：端口不是契约，
    /// 而且不同主体可以先后用同一个端口。
    pub subject_id: String,
    pub pid: u32,
    /// 进程创建时间（内核给的）。**必须和 pid 一起比**：pid 回收得很快，只比 pid 会让壁纸
    /// 把别人的进程认成自己的孩子，然后去停它 —— 那就破了"绝不接管他人实例"这条底线。
    /// `None` 表示取不到（平台不提供）⇒ 判定一律为"不是我启动的"，宁可少一个按钮可用。
    pub started_at: Option<u64>,
}

/// 这条记录是否真的对应**此刻活着并且是同一个进程**的孩子。
///
/// 纯函数：把"真实进程长什么样"作为参数传进来，判定规则才可以被测试钉住。任何不确定
/// （没有记录、pid 不同、创建时间取不到或不一致）都返回 false —— 这份缓存**只是参考，不是
/// 授权**：坏了、丢了最坏是"壁纸以为不是自己的"（安全方向）。
pub(crate) fn owns_live_process(
    recorded: Option<&ManagedChild>,
    live_pid: Option<u32>,
    live_started_at: Option<u64>,
) -> bool {
    let Some(recorded) = recorded else { return false };
    let Some(live_pid) = live_pid else { return false };
    if recorded.pid != live_pid {
        return false;
    }
    match (recorded.started_at, live_started_at) {
        (Some(recorded_at), Some(live_at)) => recorded_at == live_at,
        _ => false,
    }
}

/// The path-and-query that lets a browser into a web host: `/?token=…`.
///
/// Pure, because this is the one piece that rots silently if DSH changes its wording: a
/// test reads the rule back, instead of a person having to notice a log line.
pub(crate) fn web_handoff_path(line: &str) -> Option<(u16, String)> {
    const PREFIX: &str = "http://127.0.0.1:";
    let rest = &line[line.find(PREFIX)? + PREFIX.len()..];
    let port_len = rest.find(|c: char| !c.is_ascii_digit())?;
    let port: u16 = rest[..port_len].parse().ok()?;
    let after_port = &rest[port_len..];
    let query_at = after_port.find("/?")?;
    let query = after_port[query_at..].split_whitespace().next()?;
    Some((port, query.to_string()))
}

/// The hand-off path a browser should be opened with, when one is known for that port.
pub(crate) fn known_web_handoff(port: u16) -> Option<String> {
    web_handoffs().lock().ok()?.get(&port).cloned()
}

/// Start a globally installed CLI: `<launcher> --profile <profile>`.
///
/// `--profile` is passed explicitly rather than left to the CLI's own default: this
/// subject exists so the wallpaper can reach the *web/app* shape over HTTP, and that is
/// the profile the user's other subjects boot.
///
/// `CREATE_NO_WINDOW` matters here. The launcher is a console program, and a GUI process
/// that spawns one without that flag gets a console window of its own — a black rectangle
/// that appears next to a wallpaper and stays until the CLI exits.
///
/// A `web` host keeps its stdout: that stream is where the browser hand-off is printed,
/// and dropping it is what left the browser opening a page that could only say
/// "authentication required". The reader keeps draining for the child's whole life — a
/// pipe closed early would hand the CLI a write error it does not deserve.
fn launch_installed_cli(launcher: &str, profile: &str) -> HarnessLaunchOutcome {
    let (program, args) = installed_cli_command(Path::new(launcher), profile);
    let wants_handoff = profile.trim() == "web";
    let mut command = std::process::Command::new(&program);
    command
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(if wants_handoff {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    log::info!(
        "harness installed-cli launch: program={} args={args:?}",
        program.display()
    );
    match command.spawn() {
        Ok(mut child) => {
            if wants_handoff {
                if let Some(stdout) = child.stdout.take() {
                    // 记录"这个孩子是壁纸启动的"，就记在**拿到门票的那一刻**：那一刻我们既确认了
                    // 启动确实发生（这条线程只在我们 spawn 之后存在），也从门票里读到了端口，于是
                    // 能顺手问到**端口属主**（真正在服务的 DSH 宿主，而不是刚 spawn 的 cmd 外壳）。
                    //
                    // 为什么非要在这一处记：启动有三条入口（设置里的「打开」、壁纸面的「启动主体」、
                    // 开机自启），只有这一处**知道端口**，也只有这一处能确定"是我们启动的"。
                    let subject = format!("{CLI_ID_PREFIX}{launcher}");
                    std::thread::spawn(move || {
                        use std::io::BufRead;
                        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                            if let Some((port, path)) = web_handoff_path(&line) {
                                log::info!("harness web handoff: port={port} path={path}");
                                if let Ok(mut map) = web_handoffs().lock() {
                                    map.insert(port, path);
                                }
                                // 端口可能比门票晚一点点才被登记到内核表里；短暂轮询，不空等。
                                for _ in 0..40 {
                                    if let Some(pid) = crate::client_window::endpoint_process_id(port) {
                                        remember_child(&subject, pid);
                                        break;
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(250));
                                }
                                // 不 break：继续把管道读干，别让 CLI 因为写不出去而报错。
                            }
                        }
                    });
                }
            }
            // 记录**不在这里**做：这里拿到的是刚 spawn 的外壳（Windows 上启动 `.cmd` 必然如此），
            // 而"孩子"应当指真正在服务的那一个 —— 端口的属主。统一由调用方在端口起来之后记。
            HarnessLaunchOutcome {
                outcome: "started".into(),
                kind: HarnessTargetKind::InstalledCli,
                pid: Some(child.id()),
                hidden: false,
            }
        }
        Err(error) => {
            log::warn!("harness installed-cli launch failed: {error}");
            HarnessLaunchOutcome::new("spawn-failed", HarnessTargetKind::InstalledCli)
        }
    }
}

/// The command that raises the TUI in a console window of its own.
///
/// Deliberately different from the DSH CLI's command line, in three ways that each
/// answer a question a user would otherwise ask:
///
/// * **it is a console program and that console should be visible** — the opposite of
///   `CREATE_NO_WINDOW` above, because "run the TUI" means "put a terminal on screen",
///   and a TUI without a terminal is a process talking to nobody;
/// * **`cmd /k` rather than `/c`** — a TUI that cannot start (missing install, broken
///   profile) prints why and exits, and with `/c` that message would vanish with the
///   window before anyone could read it;
/// * **no `--profile`** — unlike `dsh`, the TUI names its own profile (`dsh-tui`) and
///   forwards the rest to it, so passing one here would be the wallpaper overriding a
///   choice the tool already made.
pub(crate) fn tui_launch_command(launcher: &Path) -> (PathBuf, Vec<String>) {
    (
        PathBuf::from("cmd.exe"),
        vec![
            "/c".to_string(),
            "start".to_string(),
            // The empty title is not decoration: `start` reads its first quoted
            // argument as a window title, and without this an unquoted path would be
            // taken as one.
            String::new(),
            "cmd".to_string(),
            "/k".to_string(),
            launcher.to_string_lossy().into_owned(),
        ],
    )
}

/// The program and arguments that start a **globally installed** DSH CLI.
///
/// npm's Windows launcher is a `.cmd` batch file, and `CreateProcess` cannot run one
/// directly — it needs `cmd /c` in front of it. A `.exe` (a different packaging, or a
/// future npm) is spawned as-is, so this decides by extension instead of assuming.
///
/// Pure, and that is the point: the spawn site stays boring, and the tests can read
/// the exact command line a stored subject would produce on this machine.
fn installed_cli_command(launcher: &Path, profile: &str) -> (PathBuf, Vec<String>) {
    let mut profile_args = vec!["--profile".to_string(), profile.to_string()];
    // `dsh web` 的默认行为是"起服务**并且打开默认浏览器**"。这个决定该由壁纸来做：设置里选的
    // 是浏览器还是终端里的 TUI，而且开机自启时更不该自己弹窗。`--no-open` 是 **web 应用自己的**
    // 旗标，所以只在 `web` 这个档案上带 —— 别的档案的 app 未必认这个参数。
    if profile.trim() == "web" {
        profile_args.push("--no-open".to_string());
    }
    let extension = launcher
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("cmd") | Some("bat") => {
            let mut args = vec!["/c".to_string(), launcher.to_string_lossy().into_owned()];
            args.extend(profile_args);
            (PathBuf::from("cmd.exe"), args)
        }
        _ => (launcher.to_path_buf(), profile_args),
    }
}

/// Who is asking for the start, which is what decides the two rules that differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchTrigger {
    /// A control the user just pressed. The configured launcher is used as given,
    /// and a window is shown — they asked for the thing on screen.
    Manual,
    /// The wallpaper's own unattended start. Only an allowlisted launcher runs
    /// without explicit consent, and a shell that supports it keeps its window out
    /// of sight (§5.1).
    Automatic { trusted_command: bool },
}

impl LaunchTrigger {
    fn is_automatic(self) -> bool {
        matches!(self, Self::Automatic { .. })
    }

    fn trusted_command(self) -> bool {
        matches!(self, Self::Automatic { trusted_command: true })
    }
}

/// Decide how to start `id`, or refuse with a closed code.
///
/// The trigger is the one input that changes behaviour, and it changes it in two
/// places on purpose: only an unattended start hides a window (§5.1 defines that
/// as the startup behaviour), and only an unattended start needs consent for a
/// custom launcher, because running an arbitrary configured program at every login
/// is a different trust decision from a button press.
pub(crate) fn plan_launch(
    id: &str,
    profile: &str,
    command: Option<&str>,
    trigger: LaunchTrigger,
) -> Result<LaunchPlan, &'static str> {
    let command = command.map(str::trim).filter(|value| !value.is_empty());
    if let Some(launcher) = id.trim().strip_prefix(CLI_ID_PREFIX) {
        let launcher = launcher.trim();
        if launcher.is_empty() {
            return Err("unknown-target");
        }
        // 已安装的 CLI 自己就是程序 —— 没有树、没有自定义启动器，所以"要不要用户确认"
        // 这个问题对它不成立：它不是用户随手填的命令，而是扫描扫出来的一个已安装命令。
        return Ok(LaunchPlan::InstalledCli {
            launcher: launcher.to_string(),
            profile: profile.trim().to_string(),
        });
    }
    if let Some(aumid) = id.trim().strip_prefix(SHELL_ID_PREFIX) {
        let Some(shell) = known_shell(aumid) else {
            return Err("unknown-target");
        };
        return Ok(LaunchPlan::Shell {
            aumid: shell.aumid.to_string(),
            alias: shell.alias,
            port: shell.default_port,
            hide_window: trigger.is_automatic() && shell.can_start_hidden,
        });
    }

    // Everything else is a source checkout, whose id *is* its root path. The path
    // is validated by the managed chain (root shape, launcher, profile), so this
    // only refuses the empty case, which would otherwise become "the current
    // directory" by accident.
    let root_path = id.trim();
    if root_path.is_empty() {
        return Err("unknown-target");
    }
    if trigger.is_automatic()
        && !crate::is_allowlisted_auto_start_launcher(command)
        && !trigger.trusted_command()
    {
        return Err("command-not-confirmed");
    }
    Ok(LaunchPlan::Checkout {
        root_path: root_path.to_string(),
        profile: profile.trim().to_string(),
        command: command.map(str::to_string),
    })
}

/// Start the planned subject and report what happened.
pub(crate) fn run_launch(
    plan: &LaunchPlan,
    managed: &crate::ManagedDshState,
) -> HarnessLaunchOutcome {
    match plan {
        LaunchPlan::Shell {
            aumid,
            alias,
            port,
            hide_window,
        } => launch_shell(aumid, alias, *port, *hide_window),
        LaunchPlan::InstalledCli { launcher, profile } => launch_installed_cli(launcher, profile),
        LaunchPlan::Checkout {
            root_path,
            profile,
            command,
        } => match crate::spawn_managed_dsh(managed, root_path, profile, command.as_deref()) {
            // `spawn_managed_dsh` returns the existing pid when this process
            // already owns a running child, so "started" also covers "already
            // managed"; ownership is the same either way.
            Ok(pid) => HarnessLaunchOutcome {
                outcome: "started".into(),
                kind: HarnessTargetKind::Checkout,
                pid: Some(pid),
                hidden: false,
            },
            Err(error) => {
                log::warn!("harness target launch failed: {error}");
                let code = crate::classify_dsh_launch_failure(&error);
                HarnessLaunchOutcome::new(&code, HarnessTargetKind::Checkout)
            }
        },
    }
}

/// Ask the Windows shell to start the client behind one alias.
///
/// Also the way a single-instance shell is asked to focus the window it already
/// owns: launching its alias again is what its own `second-instance` handler acts
/// on, so this reaches a window that resolving the owning process cannot (S6.1).
///
/// `explorer.exe` is a GUI process: it has no use for this process's standard
/// streams, and inheriting them would keep a handle family alive past this call.
fn spawn_alias(alias: &str, aumid: &str) -> bool {
    let mut launch = std::process::Command::new("explorer.exe");
    launch.arg(alias);
    launch
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    log::info!("harness shell launch: program=explorer.exe arg={alias} aumid={aumid}");
    match launch.spawn() {
        Ok(_) => true,
        Err(error) => {
            log::warn!("harness shell launch failed: {error}");
            false
        }
    }
}

/// Start a shell through the Windows shell's own alias.
///
/// `aumid` is used only for logging: the launch request is `alias`, which came
/// from this build's table rather than from the caller.
fn launch_shell(
    aumid: &str,
    alias: &str,
    port: Option<u16>,
    hide_window: bool,
) -> HarnessLaunchOutcome {
    // A client that already answers belongs to the user. Report it and leave it
    // alone: starting a second one is at best wasteful and, for a shell without a
    // single-instance lock, at worst a second window nobody asked for (§4.6).
    if let Some(port) = port {
        if crate::client_window::endpoint_is_listening(port) {
            log::info!("harness shell already running: aumid={aumid} port={port}");
            return HarnessLaunchOutcome::new("already-running", HarnessTargetKind::EmbeddedShell);
        }
    }

    if !spawn_alias(alias, aumid) {
        return HarnessLaunchOutcome::new("spawn-failed", HarnessTargetKind::EmbeddedShell);
    }

    let (confirmed, hidden) = match port {
        Some(port) => wait_for_shell(port, hide_window),
        None => (false, false),
    };
    log::info!(
        "harness shell launched: aumid={aumid} confirmed={confirmed} hidden={hidden}"
    );
    HarnessLaunchOutcome {
        outcome: if confirmed { "started" } else { "started-unconfirmed" }.into(),
        kind: HarnessTargetKind::EmbeddedShell,
        pid: None,
        hidden,
    }
}

/// Wait for a freshly started shell to answer, hiding its window on the way.
///
/// One deadline covers both waits: the window can only be resolved once the
/// client is listening (that is how the port is turned back into a process, and
/// from there into the window it owns), and hiding a window that has not been
/// painted yet is simply retried until the deadline. Hiding never fails the
/// launch — a visible window is the honest outcome of not being able to hide one.
fn wait_for_shell(port: u16, hide_window: bool) -> (bool, bool) {
    let deadline = std::time::Instant::now() + SHELL_START_TIMEOUT;
    let mut confirmed = false;
    let mut hidden = false;
    while std::time::Instant::now() < deadline {
        if !confirmed {
            confirmed = crate::client_window::endpoint_is_listening(port);
        }
        if hide_window && !hidden {
            hidden = crate::client_window::hide_client_window(port).hidden;
        }
        if confirmed && (!hide_window || hidden) {
            break;
        }
        std::thread::sleep(SHELL_START_POLL);
    }
    if hide_window && !hidden {
        log::warn!("harness shell window was never hideable: port={port} confirmed={confirmed}");
    }
    (confirmed, hidden)
}

/// What 「拉起 UI」 found, and what it had to do about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessUiOutcome {
    /// A raise code (`raised`, `raise-refused`, `no-window`), `not-running` when
    /// nothing answered even after starting the subject, or `unknown-target`.
    pub outcome: String,
    pub kind: HarnessTargetKind,
    /// True when this call had to start the subject first (§5.2).
    pub started: bool,
    /// The start's own code when it had to start — the actionable half, because it
    /// names what to fix (missing profile, missing Node, an occupied port).
    pub start_outcome: Option<String>,
}

/// Which class a stored id names. No scan: the id namespace is the model's.
fn subject_kind(subject_id: &str) -> HarnessTargetKind {
    if subject_id.trim().starts_with(SHELL_ID_PREFIX) {
        HarnessTargetKind::EmbeddedShell
    } else {
        HarnessTargetKind::Checkout
    }
}

/// The endpoint that decides whether a subject is running.
///
/// A caller that knows the endpoint the wallpaper is connected to passes it. When it
/// does not, the endpoint the wallpaper is *actually* using is the answer — and it is
/// a safety rule rather than a convenience: the settings surface no longer names a
/// port, so if the user's DSH were listening on a moved port, the subject's own
/// default would look free and 「打开」 would start a *second* instance against the
/// same data profile. That live endpoint is now derived from the configured subject,
/// so this can no longer reach a client the user did not choose (§4.6).
fn ui_port(subject_id: &str, declared: u16) -> u16 {
    if declared != 0 {
        return declared;
    }
    if crate::harness_endpoint_configured() {
        // The settings' subject decides, and through `harness_endpoint_port` it
        // already accounts for a checkout the user moved to another port.
        return crate::harness_endpoint_port();
    }
    // Nothing has been published yet — the settings window can reach a subject
    // before it has pushed the scope — so the subject the caller names is the only
    // statement about where that client answers. Starting on a default port would
    // mean starting a second instance against the same data profile.
    if let Some(aumid) = subject_id.trim().strip_prefix(SHELL_ID_PREFIX) {
        if let Some(port) = known_shell(aumid).and_then(|shell| shell.default_port) {
            return port;
        }
    }
    crate::harness_endpoint_port()
}

/// Make the chosen subject's interface available and foreground.
///
/// Idempotent over the three states the design names (§5.2), in the one order that
/// is correct:
///
/// 1. **subject not running** → start it, with the manual trigger, so this time it
///    is not hidden: the user asked to *see* something;
/// 2. **running, window hidden** → show it — including the case the wallpaper
///    itself created by starting it in the background;
/// 3. **window in the background** → bring it forward.
///
/// The window work is `client_window`'s, so "which window" has exactly one answer
/// in this codebase, and this function never guesses an executable path.
pub(crate) fn ensure_ui(
    subject_id: &str,
    port: u16,
    profile: &str,
    command: Option<&str>,
    managed: &crate::ManagedDshState,
) -> HarnessUiOutcome {
    let kind = subject_kind(subject_id);
    let port = ui_port(subject_id, port);
    // An empty id means "no subject chosen yet": still a legitimate request to
    // reach whatever answers on that endpoint, just nothing to start.
    let plan = if subject_id.trim().is_empty() {
        None
    } else {
        match plan_launch(subject_id, profile, command, LaunchTrigger::Manual) {
            Ok(plan) => Some(plan),
            Err(code) => {
                return HarnessUiOutcome {
                    outcome: code.into(),
                    kind,
                    started: false,
                    start_outcome: None,
                }
            }
        }
    };

    let mut started = false;
    let mut start_outcome = None;
    if !crate::client_window::endpoint_is_listening(port) {
        if let Some(plan) = &plan {
            let launch = run_launch(plan, managed);
            started = true;
            start_outcome = Some(launch.outcome.clone());
            // The start's own wait applies to shells; a checkout comes up on its own
            // schedule, so give it the same grace here before giving up.
            if launch.kind == HarnessTargetKind::Checkout {
                wait_for_endpoint(port, SHELL_START_TIMEOUT);
            }
        }
    }

    if !crate::client_window::endpoint_is_listening(port) {
        // §6.2: the subject is genuinely gone (or was never there). Report it rather
        // than waiting for a window that cannot appear.
        return HarnessUiOutcome {
            outcome: "not-running".into(),
            kind,
            started,
            start_outcome,
        };
    }

    let shell = subject_id
        .trim()
        .strip_prefix(SHELL_ID_PREFIX)
        .and_then(known_shell);
    // (S6.1, measured) A single-instance shell is asked to focus its *own* window
    // first, and only then does this application try to bring the window forward.
    //
    // The order is the fix for "raising the window is slow and it comes up
    // unresponsive": the desktop wallpaper is not the foreground process, so Windows
    // refuses its `SetForegroundWindow` — while the shell itself is entitled to make
    // that change, and its `second-instance` handler is exactly that request. Doing it
    // the other way round also meant waiting out a window poll before the request that
    // was going to work was even made.
    let raise = match &shell {
        Some(shell) if shell.single_instance => {
            if let Some(raised) = ask_shell_to_focus(shell, port) {
                log::info!(
                    "harness shell focused its own window: aumid={} outcome={}",
                    shell.aumid,
                    raised.outcome
                );
                raised
            } else {
                // Its focus path is not installed yet (it is still booting) or it owns
                // no window after all: fall back to resolving the window ourselves.
                let resolved = reveal(port, kind);
                // The client is up by now, so the request can land where it could not
                // before — but only ask once, and only when there is a window to focus.
                if resolved.outcome != "no-window" {
                    log::info!(
                        "harness shell asked to focus again after its boot: aumid={}",
                        shell.aumid
                    );
                    spawn_alias(&shell.alias, shell.aumid);
                }
                resolved
            }
        }
        // A client without the lock has no such request: launching it again opens a
        // second window, so this application's own window work is the only route (§6.1).
        _ => reveal(port, kind),
    };
    // One line for the whole action, because "I pressed it and nothing happened" is
    // otherwise indistinguishable from "the press never reached here" — the raise
    // itself has nothing to log, and the renderer's report is not readable from
    // outside. The subject is named by class, never by path.
    log::info!(
        "harness ui: subject={} port={port} kind={kind:?} started={started} outcome={}",
        if shell.is_some() { "shell" } else if subject_id.trim().is_empty() { "endpoint-only" } else { "checkout" },
        raise.outcome
    );
    HarnessUiOutcome {
        outcome: raise.outcome.into(),
        kind,
        started,
        start_outcome,
    }
}

/// Ask a single-instance shell to bring its own window forward.
///
/// The request *is* the alias launch: the shell's own `second-instance` handler turns a
/// second launch into "focus the window I already own", and it is the only participant
/// entitled to make that foreground change. Our own re-resolution afterwards is how the
/// result becomes an outcome code — the request itself reports nothing.
///
/// `None` means "not decidable yet" (the request could not be made, or no window
/// appeared within [`SHELL_FOCUS_TIMEOUT`]), which sends the caller down its fallback
/// rather than claiming a raise that did not happen.
fn ask_shell_to_focus(
    shell: &crate::harness_targets::ShellApp,
    port: u16,
) -> Option<crate::client_window::RaiseOutcome> {
    if !spawn_alias(&shell.alias, shell.aumid) {
        return None;
    }
    let deadline = std::time::Instant::now() + SHELL_FOCUS_TIMEOUT;
    loop {
        let raise = crate::client_window::raise_client_window(port);
        if raise.outcome != "no-window" {
            // `raise-refused` lands here too, and it is a success for this route: the
            // window exists and is being focused by the client itself, which is the
            // part Windows was refusing *us*.
            return Some(raise);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(SHELL_START_POLL);
    }
}

/// Bring the subject's window forward, waiting for it when its class has one.
///
/// The wait is not optional and it is not a workaround: an Electron client starts
/// listening before it paints, so the first raise attempt after a start would answer
/// `no-window` for a client that is about to show one — measured on this machine,
/// which is why state 1 of the three-state test failed before this wait existed.
///
/// It is bounded by the class: only a shell is expected to own a window, so a
/// checkout reports the answer immediately instead of waiting for a window that
/// does not exist, and the caller can open a browser without a pointless delay.
fn reveal(port: u16, kind: HarnessTargetKind) -> crate::client_window::RaiseOutcome {
    let deadline = if kind == HarnessTargetKind::EmbeddedShell {
        std::time::Instant::now() + UI_WINDOW_TIMEOUT
    } else {
        std::time::Instant::now()
    };
    loop {
        let raise = crate::client_window::raise_client_window(port);
        if raise.outcome == "no-window" && std::time::Instant::now() < deadline {
            std::thread::sleep(SHELL_START_POLL);
            continue;
        }
        return raise;
    }
}

/// Wait for an endpoint to start answering, bounded.
fn wait_for_endpoint(port: u16, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if crate::client_window::endpoint_is_listening(port) {
            return true;
        }
        std::thread::sleep(SHELL_START_POLL);
    }
    crate::client_window::endpoint_is_listening(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_cli_starts_through_cmd_because_npm_ships_a_batch_file() {
        let (program, args) = installed_cli_command(
            Path::new(r"C:\Users\u\AppData\Roaming\npm\dsh.cmd"),
            "web",
        );
        // `CreateProcess` cannot execute a `.cmd`; without `cmd /c` this would fail
        // with "not a valid application" on every machine that has the CLI.
        assert_eq!(program, PathBuf::from("cmd.exe"));
        assert_eq!(
            args,
            vec![
                "/c",
                r"C:\Users\u\AppData\Roaming\npm\dsh.cmd",
                "--profile",
                "web",
                // 少了它，CLI 自己就会打开默认浏览器：设置里"浏览器还是终端里的 TUI"就白选了，
                // 开机自启还会每次弹窗。
                "--no-open"
            ]
        );
        // 别的档案不带它：这是 web 应用自己的旗标，未知档案的 app 未必认这个参数。
        let (_, desktop) = installed_cli_command(Path::new(r"C:\tools\dsh.exe"), "desktop");
        assert_eq!(desktop, vec!["--profile", "desktop"]);
    }

    #[test]
    fn a_native_launcher_is_started_directly() {
        let (program, args) = installed_cli_command(Path::new(r"C:\tools\dsh.exe"), "web");
        assert_eq!(program, PathBuf::from(r"C:\tools\dsh.exe"));
        assert_eq!(args, vec!["--profile", "web", "--no-open"]);
    }

    #[test]
    fn the_tui_is_raised_in_a_console_that_stays_until_it_is_read() {
        let (program, args) = tui_launch_command(Path::new(r"C:\Users\u\AppData\Roaming\npm\dst.cmd"));
        assert_eq!(program, PathBuf::from("cmd.exe"));
        assert_eq!(
            args,
            vec![
                "/c",
                "start",
                // 空标题：`start` 会把第一个带引号的参数当窗口标题，少了它就会把路径当标题。
                "",
                "cmd",
                // `/k` 而不是 `/c`：TUI 起不来时会打印原因然后退出，用 `/c` 那句话会随窗口一起消失。
                "/k",
                r"C:\Users\u\AppData\Roaming\npm\dst.cmd"
            ]
        );
        // TUI 自己指定 profile（dsh-tui）⇒ 这里**不能**替它加 `--profile`。
        assert!(!args.iter().any(|arg| arg == "--profile"));
    }

    #[test]
    fn a_missing_or_broken_record_reads_as_an_empty_cylinder() {
        let dir = std::env::temp_dir().join("dsh-wallpaper-managed-children-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("managed-dsh.json");
        // 没有文件 ⇒ 空弹仓（不是错误）。
        assert_eq!(read_managed_children(&path), ManagedChildren::default());
        // 写进去、读回来，一格不多一格不少。
        let mut cylinder = ManagedChildren::default();
        cylinder.remember(ManagedChild {
            subject_id: "cli:C:\\Users\\u\\AppData\\Roaming\\npm\\dsh.cmd".into(),
            pid: 4242,
            started_at: Some(1_700_000_000),
        });
        write_managed_children(&path, &cylinder).expect("write");
        assert_eq!(read_managed_children(&path), cylinder);
        // 坏文件 ⇒ 空弹仓：降级方向必须安全（宁可少一个按钮可用，不可多一次误杀）。
        std::fs::write(&path, b"{ this is not json").expect("corrupt");
        assert_eq!(read_managed_children(&path), ManagedChildren::default());
        std::fs::write(&path, br#"{"children":{"x":{"subject_id":"x"}}}"#).expect("partial");
        assert_eq!(read_managed_children(&path), ManagedChildren::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_revolver_keeps_every_subject_and_only_fires_the_aligned_one() {
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        let shell = "shell:com.deepseek.dsh";
        let mut cylinder = ManagedChildren::default();
        cylinder.remember(ManagedChild { subject_id: cli.into(), pid: 111, started_at: Some(1) });
        // 切到客户端：CLI 那一格**保留**，只是不再对齐。
        assert!(cylinder.aligned(shell).is_none());
        assert_eq!(cylinder.aligned(cli).map(|child| child.pid), Some(111));
        cylinder.remember(ManagedChild { subject_id: shell.into(), pid: 222, started_at: Some(2) });
        // 切回 CLI：仍然认得那个孩子 —— 这就是"别的格子必须留着"的全部理由。
        assert_eq!(cylinder.aligned(cli).map(|child| child.pid), Some(111));
        assert_eq!(cylinder.aligned(shell).map(|child| child.pid), Some(222));
        // 由本应用停掉客户端那一格，CLI 那一格不受影响。
        cylinder.forget(shell);
        assert!(cylinder.aligned(shell).is_none());
        assert_eq!(cylinder.aligned(cli).map(|child| child.pid), Some(111));
    }

    #[test]
    fn ownership_needs_the_pid_and_the_start_time_to_agree() {
        let child = ManagedChild {
            subject_id: "cli:C:\\Users\\u\\AppData\\Roaming\\npm\\dsh.cmd".into(),
            pid: 4242,
            started_at: Some(1_700_000_000),
        };
        // 两项都对上：是自己的孩子。
        assert!(owns_live_process(Some(&child), Some(4242), Some(1_700_000_000)));
        // pid 相同但创建时间不同 ⇒ 那是回收后的**另一个**进程：绝不能认领，更不能去停它。
        assert!(!owns_live_process(Some(&child), Some(4242), Some(1_700_000_999)));
        // pid 不同 / 进程已不在 / 没有记录。
        assert!(!owns_live_process(Some(&child), Some(4243), Some(1_700_000_000)));
        assert!(!owns_live_process(Some(&child), None, None));
        assert!(!owns_live_process(None, Some(4242), Some(1_700_000_000)));
        // 创建时间任一侧取不到 ⇒ 判定为"不是我启动的"（安全方向，绝不误杀）。
        let unknown = ManagedChild { started_at: None, ..child.clone() };
        assert!(!owns_live_process(Some(&unknown), Some(4242), Some(1_700_000_000)));
        assert!(!owns_live_process(Some(&child), Some(4242), None));
    }

    #[test]
    fn the_browser_handoff_is_read_from_the_line_the_cli_prints() {
        // 实测原文（`dsh web` 启动时打印的那一行）。
        let printed = "dsh web: http://127.0.0.1:3080/?token=w03-O64JxomJCRw9Any3kg9Nap09jNg56tJbeU6pE4s";
        let (port, path) = web_handoff_path(printed).expect("handoff");
        assert_eq!(port, 3080);
        assert_eq!(path, "/?token=w03-O64JxomJCRw9Any3kg9Nap09jNg56tJbeU6pE4s");
        // 换端口也认（端口不是契约）。
        assert_eq!(web_handoff_path("dsh web: http://127.0.0.1:8080/?token=abc").map(|(p, _)| p), Some(8080));
        // 没有门票的普通行、或别的地址，都不该被误认成门票。
        assert!(web_handoff_path("dsh web: opening the default browser").is_none());
        assert!(web_handoff_path("http://127.0.0.1:3080/").is_none());
        assert!(web_handoff_path("https://example.com/?token=abc").is_none());
    }

    #[test]
    fn an_installed_cli_plans_the_launcher_itself_with_its_profile() {
        let id = format!("{CLI_ID_PREFIX}C:\\Users\\u\\AppData\\Roaming\\npm\\dsh.cmd");
        let plan = plan_launch(&id, " web ", None, MANUAL).expect("cli plan");
        match plan {
            LaunchPlan::InstalledCli { launcher, profile } => {
                assert_eq!(launcher, r"C:\Users\u\AppData\Roaming\npm\dsh.cmd");
                assert_eq!(profile, "web", "the profile is trimmed like a checkout's");
            }
            _ => panic!("expected an installed-cli plan"),
        }
        // 空启动器要拒绝，否则会变成"启动当前目录"那种意外。
        assert_eq!(plan_launch("cli:", "web", None, MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("cli:   ", "web", None, AUTO), Err("unknown-target"));
        // 它不需要"用户确认启动器"那一步：它不是用户填的命令，而是扫描扫出来的已安装命令。
        assert!(plan_launch(&id, "web", None, AUTO).is_ok());
    }

    const OFFICIAL_ID: &str = "shell:com.deepseek.dsh";
    const CHECKOUT: &str = r"D:\Family\DeepSeekHarness\deepseek-harness";

    const MANUAL: LaunchTrigger = LaunchTrigger::Manual;
    const AUTO: LaunchTrigger = LaunchTrigger::Automatic { trusted_command: false };
    const AUTO_TRUSTED: LaunchTrigger = LaunchTrigger::Automatic { trusted_command: true };

    #[test]
    fn a_shell_plan_carries_the_alias_this_build_knows() {
        let plan = plan_launch(OFFICIAL_ID, "desktop", None, MANUAL).expect("shell plan");
        assert_eq!(
            plan,
            LaunchPlan::Shell {
                aumid: "com.deepseek.dsh".into(),
                alias: r"shell:AppsFolder\com.deepseek.dsh".into(),
                port: Some(19387),
                hide_window: false,
            }
        );
    }

    #[test]
    fn only_an_unattended_start_keeps_the_window_out_of_sight() {
        // §5.1: the startup path starts the official shell without showing it...
        let automatic = plan_launch(OFFICIAL_ID, "desktop", None, AUTO).expect("shell plan");
        assert!(matches!(automatic, LaunchPlan::Shell { hide_window: true, .. }));
        // ...while a button the user just pressed shows them what they asked for.
        let manual = plan_launch(OFFICIAL_ID, "desktop", None, MANUAL).expect("shell plan");
        assert!(matches!(manual, LaunchPlan::Shell { hide_window: false, .. }));
    }

    #[test]
    fn an_unknown_shell_id_is_refused_instead_of_launched() {
        // The alias string is a shell launch request, so anything outside this
        // build's table must not reach the shell.
        assert_eq!(plan_launch("shell:Notepad", "desktop", None, MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("shell:", "desktop", None, MANUAL), Err("unknown-target"));
        assert_eq!(
            plan_launch("shell:Microsoft.Windows.Explorer", "desktop", None, AUTO),
            Err("unknown-target")
        );
    }

    #[test]
    fn an_empty_subject_is_refused() {
        assert_eq!(plan_launch("   ", "desktop", None, MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("", "desktop", None, AUTO), Err("unknown-target"));
    }

    #[test]
    fn a_shell_ignores_the_profile_setting_which_belongs_to_a_checkout_only() {
        // §4.7: a shell uses its own data, so a leftover profile value must not
        // change — or block — its launch.
        let plan = plan_launch(OFFICIAL_ID, "not a profile at all", None, MANUAL)
            .expect("shell plan");
        assert!(matches!(plan, LaunchPlan::Shell { .. }));
    }

    #[test]
    fn a_checkout_plan_is_its_root_path_with_its_profile() {
        let plan =
            plan_launch(CHECKOUT, " desktop ", Some("node.exe"), MANUAL).expect("checkout plan");
        assert_eq!(
            plan,
            LaunchPlan::Checkout {
                root_path: CHECKOUT.into(),
                profile: "desktop".into(),
                command: Some("node.exe".into()),
            }
        );
    }

    #[test]
    fn an_untrusted_launcher_needs_consent_only_on_the_automatic_path() {
        // Manual: the user just asked for it, so the configured launcher is used.
        assert!(plan_launch(CHECKOUT, "desktop", Some(r"D:\tools\mine.exe"), MANUAL).is_ok());
        // Automatic: refused until the user explicitly agreed.
        assert_eq!(
            plan_launch(CHECKOUT, "desktop", Some(r"D:\tools\mine.exe"), AUTO),
            Err("command-not-confirmed")
        );
        // Agreed → allowed, still as one executable path.
        assert!(plan_launch(CHECKOUT, "desktop", Some(r"D:\tools\mine.exe"), AUTO_TRUSTED).is_ok());
        // The launchers the managed chain picks itself never need consent.
        assert!(plan_launch(CHECKOUT, "desktop", Some("node.exe"), AUTO).is_ok());
        assert!(plan_launch(CHECKOUT, "desktop", None, AUTO).is_ok());
    }

    #[test]
    fn a_shell_is_never_gated_by_the_launcher_consent_rule() {
        // A shell has no configured launcher at all, so the consent question does
        // not apply to it; only checkouts can carry a `command`.
        assert!(plan_launch(OFFICIAL_ID, "desktop", None, AUTO).is_ok());
    }

        #[test]
    /// Ground truth for the one class this machine can be asked about without side
    /// effects: the official shell that is already running.
    ///
    /// Ignored by default, because it reads a live shell. Run it with
    /// `cargo test --lib -- --ignored --nocapture this_machine_reports_what_raising_the_official_shell_would_do`.
    #[ignore = "reads this machine's live official shell; starts nothing and stops nothing"]
    fn this_machine_reports_what_raising_the_official_shell_would_do() {
        let port = 19387;
        println!("listening on {port}: {}", crate::client_window::endpoint_is_listening(port));
        let owner = crate::client_window::endpoint_process_id(port);
        println!("owner pid: {owner:?}");
        if let Some(pid) = owner {
            println!("owner alive: {}", crate::client_window::process_is_alive(pid));
        }
        println!("resolve window: {:?}", crate::client_window::window_for_endpoint(port));
        // This is the part that changes what is on screen: it asks Windows to show and
        // focus the window it resolved. That is precisely the action under test.
        println!("raise: {:?}", crate::client_window::raise_client_window(port));
        // And the same question `ensure_ui` asks first, with the same subject id the
        // settings store holds for this client.
        println!(
            "plan: {:?}",
            plan_launch("shell:com.deepseek.dsh", "desktop", None, LaunchTrigger::Manual)
        );
        // Then the whole action the button performs, on the shell that is already
        // running: it starts nothing, and the alias request only asks the shell to
        // focus the window it owns.
        let state = crate::ManagedDshState::default();
        let ui = ensure_ui(
            "shell:com.deepseek.dsh",
            port,
            "desktop",
            None,
            &state,
        );
        println!("ensure_ui: {ui:?}");
        println!(
            "port after ensure_ui is still listening: {}",
            crate::client_window::endpoint_is_listening(port)
        );
    }
}
