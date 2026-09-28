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

// ---------------------------------------------------------------------------
// 「启动参数」：分词结果、实例键、以及参数里声明的端口
// ---------------------------------------------------------------------------

/// 参数条数上限。够写端口、host、路径，又不足以拼出一条完整的命令行。
pub(crate) const MAX_LAUNCH_ARGS: usize = 32;
/// 单个参数长度上限。超过这个长度更像误粘贴，而不是一个参数。
pub(crate) const MAX_LAUNCH_ARG_LENGTH: usize = 512;
/// DSH 自己的 web 默认端口，也是"参数里没写 `--port`"时实例所在的端口。
const DEFAULT_LAUNCH_PORT: u16 = 3080;

/// 实例键里主体 id 与每个参数之间的分隔符。
///
/// 选 `U+001F`（单元分隔符）是因为它**不可能出现在 Windows 路径里**，也不会出现在合法的参数里
/// （`normalize_launch_args` 明确拒绝控制字符）。用途只有一个：让"同一个主体的两个不同实例"
/// 拿到两个不同的键。
const INSTANCE_KEY_SEPARATOR: char = '\u{1f}';

/// 一份「这个孩子是哪个实例」的键：主体 id，加上它被启动时用的参数。
///
/// 这就是"并行实例"的全部机制。上一版只有主体 id 一格，于是"同一个源码目录、两个不同端口"
/// 必然互相覆盖：启动第二个会顶掉第一个的记录，状态列表只剩一行，而停止按钮会停错人。
///
/// 参数为空时键**恰好等于主体 id**：默认实例沿用旧记录的形状，所以升级之前写下的弹仓不需要
/// 任何迁移就能继续对齐（这是刻意留的一条向后兼容，不是巧合）。
pub(crate) fn instance_key(subject_id: &str, args: &[String]) -> String {
    let subject = subject_id.trim();
    if args.is_empty() {
        return subject.to_string();
    }
    let mut key = String::from(subject);
    for arg in args {
        key.push(INSTANCE_KEY_SEPARATOR);
        key.push_str(arg);
    }
    key
}

/// 参数里声明的监听端口，或者 `None`。
///
/// 与渲染层的 `connect/launchArgs.ts::launchPortFromArgs` 是**同一组规则的两份实现**，因为
/// 两边各有一个问题只有自己答得了：渲染层要给「打开界面」算出浏览器地址，原生要在启动前检查
/// "这个端口是不是已经被别人占了"。两份由各自的测试用同一组例子钉住（`--port 3081`、
/// `--port=3081`、重复旗标只认第一个、非数字不认）—— 和 `SHELL_APPS` 与 `SHELL_SUBJECTS` 那对
/// 必须一致的常量是同一个理由：一致性靠测试，不靠"记得两边都改"。
///
/// 读不到就说读不到：绝不猜一个端口。猜错的代价是去敲一扇没人应门的窗，或者更坏 —— 把
/// "端口被占"报成一个不存在的冲突。
pub(crate) fn port_from_args(args: &[String]) -> Option<u16> {
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        let value = match argument.strip_prefix("--port=") {
            Some(inline) => Some(inline),
            None if argument == "--port" => args.get(index + 1).map(String::as_str),
            None => None,
        };
        if let Some(value) = value {
            return value
                .parse::<u32>()
                .ok()
                .filter(|port| (1..=65535).contains(port))
                .map(|port| port as u16);
        }
        index += 1;
    }
    None
}

/// 这个实例会在哪个端口上服务：参数里点名的，或者 DSH 自己的默认端口。
pub(crate) fn instance_port(args: &[String]) -> u16 {
    port_from_args(args).unwrap_or(DEFAULT_LAUNCH_PORT)
}

/// 把渲染层交来的参数洗一遍，或者说出为什么不能用。
///
/// 原生是**信任边界**，即使渲染层已经分好词：这里的检查不是"再分一次词"（那是注入的成因），而是
/// 确认这份数组的形状是一个启动器能接受的 argv —— 条数、长度、以及**没有控制字符**。最后一条是
/// 功能需要而非洁癖：实例键用 `U+001F` 分隔，参数里再出现同一个字符就会让两个不同的实例撞成
/// 一个键，而那正是本功能要解决的问题。
pub(crate) fn normalize_launch_args(args: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let Some(args) = args else { return Ok(Vec::new()) };
    if args.len() > MAX_LAUNCH_ARGS {
        return Err(format!("启动参数最多 {MAX_LAUNCH_ARGS} 个"));
    }
    let mut cleaned = Vec::with_capacity(args.len());
    for arg in args {
        let value = arg.trim();
        // 空参数没有任何用处，却会变成启动器上一个莫名其妙的空词；静默丢掉比报错好，
        // 因为用户看见的是"我多打了一个空格"。
        if value.is_empty() {
            continue;
        }
        if value.chars().count() > MAX_LAUNCH_ARG_LENGTH {
            return Err(format!("单个启动参数不能超过 {MAX_LAUNCH_ARG_LENGTH} 个字符"));
        }
        if value.chars().any(|character| character.is_control()) {
            return Err("启动参数里不能包含控制字符".to_string());
        }
        cleaned.push(value.to_string());
    }
    Ok(cleaned)
}

/// How long a shell is given to answer before its start is reported as
/// unconfirmed. Generous on purpose: an Electron client's first start after a
/// login is slow, and a false "failed" would be worse than a slow "started".
const SHELL_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const SHELL_START_POLL: std::time::Duration = std::time::Duration::from_millis(250);
/// How often the hide is re-applied while a just-started shell is up but not yet settled.
///
/// Shorter than [`SHELL_START_POLL`] because this loop is a *race* with the client's own
/// `show()`: whatever it puts on screen stays until the next attempt, so this interval is
/// literally how long the user could see a frame the wallpaper meant to keep out of
/// sight. Measured on this machine, one attempt costs ~10 ms, so the loop is cheap.
const SHELL_HIDE_POLL: std::time::Duration = std::time::Duration::from_millis(80);
/// How long after the client starts answering the fast hide poll keeps running.
///
/// The race is real but bounded: an Electron client creates its window before it shows
/// it, and this machine's shell shows it once the host behind it reports ready — seconds,
/// not minutes. Past this window the slow poll takes over, so a client that never paints
/// costs nothing and a client that paints very late is still hidden, just later.
const SHELL_HIDE_POLL_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
/// How long a shell's windows must stay off screen before a background start is done.
///
/// Not a guess about boot time: the client's own `show()` is measured to win against an
/// external hide (540 ms after the request, on this machine), so "hidden once" is not an
/// answer — "hidden and nothing brought it back" is. Two seconds of quiet after the last
/// attempt is what separates the two.
const SHELL_HIDE_SETTLE: std::time::Duration = std::time::Duration::from_secs(2);
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
/// class a stored id resolves to, whether a profile is even meaningful, and what
/// exactly the launcher will be told.
///
/// 两个变体都带 `args`，而不是带一个"启动命令"：跑的是谁由**这个 build** 决定（源码树走受管链、
/// 已安装 CLI 走它自己），用户能加的只有后面的词。这是这次改动里唯一真正的能力边界移动。
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
        /// 「启动参数」，按原样追加在启动器与档案之后。
        args: Vec<String>,
    },
    /// Spawn a globally installed DSH CLI (`npm i -g @deepseek-ai/dsh`).
    ///
    /// It differs from a checkout in the one way that matters here: there is no tree to
    /// run a launcher *from*, only the launcher itself. Everything else — the profile it
    /// boots, the port it answers on, the extra arguments it is given — is the same,
    /// which is why the caller cannot tell the two apart once the command line is decided.
    InstalledCli {
        launcher: String,
        profile: String,
        args: Vec<String>,
    },
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

/// 记住"这个主体的这个实例当前的孩子是 pid"，并落盘。
///
/// 只动**这一个实例**那一格：别的格子原样保留，这样"CLI → 客户端 → 切回 CLI"时壁纸仍然认得
/// CLI 那个孩子，而"同一个源码目录起了两个端口"两个实例也都各自留着记录。写失败只记日志、
/// 不影响启动本身（这份记录是参考，不是启动的前提）。
///
/// `port` 与 `handoff` 是**为门票**留的：门票每次启动现生成，但它在那个宿主活着期间一直有效，
/// 所以存下来是对的 —— 壁纸重启（例如每次装机）之后，"宿主还在跑、票却随着上一个进程丢掉"这种
/// 情况就不必靠重启宿主来解决。前提是它必须**随进程一起作废**，那由读取时的 pid + 创建时间
/// 校验保证（见 `handoff_in`）。
///
/// `port` 现在**还多了一个用途**：实例下拉里的 `别名 · 端口` 那一行就是从它来的。它仍然不是
/// 身份（身份永远是 pid + 创建时间），只是这个实例最有用的一条描述。
pub(crate) fn remember_child(
    subject_id: &str,
    args: &[String],
    pid: u32,
    port: Option<u16>,
    handoff: Option<String>,
) {
    let Some(path) = RECORDS_PATH.get() else { return };
    let key = instance_key(subject_id, args);
    let mut cylinder = read_managed_children(path);
    // 同一个孩子被记两次很常见（先记下"它是我的"，稍后才解析到门票）。没有票的那次不能把
    // 已有票抹掉；而**换了 pid** 就是另一个进程，票必须作废，不能继承。
    let (port, handoff) = match cylinder.children.get(&key) {
        Some(previous) if previous.pid == pid => {
            (port.or(previous.port), handoff.or_else(|| previous.handoff.clone()))
        }
        _ => (port, handoff),
    };
    cylinder.remember(ManagedChild {
        instance_key: key,
        subject_id: subject_id.to_string(),
        args: args.to_vec(),
        pid,
        started_at: crate::client_window::process_started_at(pid),
        port,
        handoff,
    });
    if let Err(error) = write_managed_children(path, &cylinder) {
        log::warn!("harness managed-child record not written: {error}");
    } else {
        log::info!("harness managed-child remembered: subject={subject_id} pid={pid}");
    }
}

/// 由本应用停掉之后清掉**这一个实例**那一格；别的格子不受影响。
pub(crate) fn forget_instance(instance_key: &str) {
    let Some(path) = RECORDS_PATH.get() else { return };
    let mut cylinder = read_managed_children(path);
    cylinder.forget(instance_key);
    if let Err(error) = write_managed_children(path, &cylinder) {
        log::warn!("harness managed-child record not written: {error}");
    }
}

/// 这个主体**所有**记着的实例（不做存活校验，校验由 [`owned_instances`] 负责）。
pub(crate) fn recorded_instances(subject_id: &str) -> Vec<ManagedChild> {
    let Some(path) = RECORDS_PATH.get() else { return Vec::new() };
    read_managed_children(path).for_subject(subject_id)
}

/// 这个实例记着的端口（**观察到**的那一个：那是端口属主，比"我们要求它听在哪儿"更硬）。
pub(crate) fn recorded_port(instance_key: &str) -> Option<u16> {
    let path = RECORDS_PATH.get()?;
    read_managed_children(path)
        .children
        .get(instance_key)
        .and_then(|child| child.port)
}

/// 这个实例记着的孩子**此刻是否真的还是同一个进程**。
///
/// 任一不确定（没有记录、读不到记录、进程已退出、创建时间对不上）都返回 `None`：调用方据此
/// 认为"不是我启动的"，于是既不会去停它，也不会声称拥有它。
pub(crate) fn owned_instance(instance_key: &str) -> Option<ManagedChild> {
    let path = RECORDS_PATH.get()?;
    let child = read_managed_children(path).children.get(instance_key).cloned()?;
    let live_pid = Some(child.pid).filter(|pid| crate::client_window::process_is_alive(*pid));
    let live_start = live_pid.and_then(crate::client_window::process_started_at);
    owns_live_process(Some(&child), live_pid, live_start).then_some(child)
}

/// 这个主体此刻仍然活着、且确实是本应用启动的那些实例。
pub(crate) fn owned_instances(subject_id: &str) -> Vec<ManagedChild> {
    recorded_instances(subject_id)
        .into_iter()
        .filter(|child| owned_instance(&child.instance_key).is_some())
        .collect()
}

/// 本应用启动的**每一个**仍然活着的实例，官壳除外。
///
/// 官壳那一类**必须**被排除，它不在本应用的管辖范围内：它是用户自己的客户端，退出方式是它
/// 自己的托盘菜单。`ensure_harness_ui` 会给它写下一条记录（那条记录是给"门票"用的），所以
/// 这里不能只靠"壳没有孩子"这个假设，而是明确按 id 前缀过滤。
pub(crate) fn owned_instances_all() -> Vec<ManagedChild> {
    let Some(path) = RECORDS_PATH.get() else { return Vec::new() };
    read_managed_children(path)
        .children
        .values()
        .filter(|child| !is_managed_by_us(&child.subject_id))
        .filter(|child| owned_instance(&child.instance_key).is_some())
        .cloned()
        .collect()
}

/// 这一类主体是不是"本应用可以启动、也可以停止"的那一类。
///
/// 唯一的否定答案就是官壳：它不归本应用管（`docs/design/harness-subject-and-ui-design.md` 里
/// 那条"绝不接管他人实例"的底线，对用户自己的客户端同样成立）。名字里说的是"我们"而不是
/// "别人"，因为这条规则的另一半是：源码目录与已安装 CLI 是我们启动的，就可以由我们停止。
pub(crate) fn is_managed_by_us(subject_id: &str) -> bool {
    !subject_id.trim().starts_with(SHELL_ID_PREFIX)
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

/// 每个**实例**一条记录 —— 左轮弹仓：一格一个实例。
///
/// **别的格子一律保留**，只动被点名的那个实例。理由是一个真实场景：用户先用 CLI 对话、
/// 然后切到客户端、再切回 CLI —— 那时壁纸必须还能认出 CLI 那个孩子是自己的，否则它要么重复
/// 启动一个，要么不敢停自己启动的那个。第二个场景是并行实例：同一个源码目录在 3080 与 3081 上
/// 各起一个，两个都要留着自己的记录，否则"起第二个"就会把第一个的归属抹掉。
///
/// **键是实例键**（`主体 id` + 参数，见 [`instance_key`]），不是主体 id、也不是端口：
/// 端口不是契约（同一个端口可以被不同主体先后使用），而参数是"我们到底启动了什么"的一部分。
/// 主体 id 另存为字段，所以"这个主体的所有实例"是一次字段比较，不需要从键里解析前缀。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ManagedChildren {
    /// 用 `BTreeMap`：落盘后顺序稳定，改动一眼可见。
    #[serde(default)]
    pub children: std::collections::BTreeMap<String, ManagedChild>,
}

impl ManagedChildren {
    /// 这个主体记着的所有实例。其它主体一概不看。
    pub(crate) fn for_subject(&self, subject_id: &str) -> Vec<ManagedChild> {
        let subject = subject_id.trim();
        self.children
            .values()
            .filter(|child| child.subject_id.trim() == subject)
            .cloned()
            .collect()
    }

    /// 成功启动后覆盖**这一格**；别的格子原样保留。
    pub(crate) fn remember(&mut self, child: ManagedChild) {
        self.children.insert(child.instance_key.clone(), child);
    }

    /// 由本应用停掉之后清掉**这一格**；别的格子不受影响。
    pub(crate) fn forget(&mut self, instance_key: &str) {
        self.children.remove(instance_key);
    }
}

/// 一份"这个孩子是壁纸启动的"记录，落在盘上，跨壁纸重启有效。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ManagedChild {
    /// 实例键（`主体 id` + 参数）。**身份不是它**：身份是 pid + 创建时间；这个键只回答
    /// "这是同一个主体的哪一个实例"，也就是"停止"要停哪一个、下拉里哪一行。
    #[serde(default)]
    pub instance_key: String,
    /// 主体 id（`shell:<aumid>` / 目录路径 / `cli:<启动器>`）—— 不是端口：端口不是契约，
    /// 而且不同主体可以先后用同一个端口。
    pub subject_id: String,
    /// 启动这个实例时用的「启动参数」。存下来是为了让下拉与日志能说清"这一行是哪一个"，
    /// 也为了让旧记录（没有这个字段）读成"没有参数"而不是整份记录作废。
    #[serde(default)]
    pub args: Vec<String>,
    pub pid: u32,
    /// 进程创建时间（内核给的）。**必须和 pid 一起比**：pid 回收得很快，只比 pid 会让壁纸
    /// 把别人的进程认成自己的孩子，然后去停它 —— 那就破了"绝不接管他人实例"这条底线。
    /// `None` 表示取不到（平台不提供）⇒ 判定一律为"不是我启动的"，宁可少一个按钮可用。
    pub started_at: Option<u64>,
    /// 这个实例的孩子当时在哪个端口上服务。**身份依旧是 pid + 创建时间**：端口不是契约，
    /// 不能拿它当"这是我的"的证据。它的两个用途是找回门票，以及在下拉里显示 `别名 · 端口`。
    #[serde(default)]
    pub port: Option<u16>,
    /// 那次启动打印出来的门票（`/?token=…`）。它在那个宿主活着期间一直有效，所以值得存下来；
    /// 但**只要那一格不再对应同一个进程就作废**（`handoff_in` 负责校验）。
    #[serde(default)]
    pub handoff: Option<String>,
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
    if let Some(path) = web_handoffs().lock().ok()?.get(&port).cloned() {
        return Some(path);
    }
    // 内存里没有 ⇒ 这一代壁纸还没做过那次启动。落盘记录里可能还留着那一代宿主的票，而票在
    // 宿主活着期间一直有效 —— 但**只认仍然对应同一个进程的那一格**（pid + 创建时间）。
    recorded_web_handoff(port)
}

/// 从落盘记录里找回某一端口所属宿主的门票。
///
/// 找不到 RECORDS_PATH（测试或非打包运行）就当作没有：这张票只是省一次重启，不是必需品。
fn recorded_web_handoff(port: u16) -> Option<String> {
    let path = RECORDS_PATH.get()?;
    let cylinder = read_managed_children(path);
    handoff_in(&cylinder, port, |child| {
        // 必须是**此刻正占着这个端口**的那一格：不同主体可以先后用同一个端口，所以"记过这个端口"
        // 不等于"就是它"。再加上 pid + 创建时间的校验，两张票同时存在时也不会张冠李戴。
        if crate::client_window::endpoint_process_id(port) != Some(child.pid) {
            return false;
        }
        let live_start = crate::client_window::process_started_at(child.pid);
        owns_live_process(Some(child), Some(child.pid), live_start)
    })
}

/// 从弹仓里取某一端口的门票，**只认通过校验的那一格**。
///
/// 把"这一格是不是此刻该端口的属主、且仍然是同一个进程"作为参数传进来，是为了让这条规则可以被
/// 纯函数测住：进程换了 pid、创建时间对不上、或者端口已经被别的主体接手，票都必须作废 ——
/// 一张过期门票比没有门票更坏，它会让浏览器停在同一句道歉页上，而我们却以为自己有票。
pub(crate) fn handoff_in(
    cylinder: &ManagedChildren,
    port: u16,
    owns_the_port_now: impl Fn(&ManagedChild) -> bool,
) -> Option<String> {
    cylinder
        .children
        .values()
        .find_map(|child| {
            (child.port == Some(port) && owns_the_port_now(child))
                .then(|| child.handoff.clone())
                .flatten()
        })
}

/// 日志里的门票要打码：它虽然只是本机回环的凭据，但"凭据不进日志"是条不该破的规矩。
pub(crate) fn redact_handoff(path: &str) -> String {
    match path.find("token=") {
        Some(at) => format!("{}{}", &path[..at], "token=••••"),
        None => path.to_string(),
    }
}

/// Start a globally installed CLI: `<launcher> --profile <profile> <启动参数>`.
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
fn launch_installed_cli(launcher: &str, profile: &str, args: &[String]) -> HarnessLaunchOutcome {
    let (program, command_args) = installed_cli_command(Path::new(launcher), profile, args);
    let wants_handoff = profile.trim() == "web";
    let mut command = std::process::Command::new(&program);
    command
        .args(&command_args)
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
        "harness installed-cli launch: program={} args={command_args:?}",
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
                    let record_args = args.to_vec();
                    std::thread::spawn(move || {
                        use std::io::BufRead;
                        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                            if let Some((port, path)) = web_handoff_path(&line) {
                                // 记录里可以存票（它在这个宿主活着期间一直有效），但**日志里不行**。
                                log::info!(
                                    "harness web handoff: port={port} path={}",
                                    redact_handoff(&path)
                                );
                                if let Ok(mut map) = web_handoffs().lock() {
                                    map.insert(port, path.clone());
                                }
                                // 端口可能比门票晚一点点才被登记到内核表里；短暂轮询，不空等。
                                for _ in 0..40 {
                                    if let Some(pid) = crate::client_window::endpoint_process_id(port) {
                                        remember_child(
                                            &subject,
                                            &record_args,
                                            pid,
                                            Some(port),
                                            Some(path.clone()),
                                        );
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
///
/// 「启动参数」**照常追加**：它加的是"启动器后面的话"，而这条路的启动器是 TUI 自己。实测这一侧
/// **没有** `--port`（`@deepseek-harness-tui/dsh-tui` 的 bin 里没有任何端口旗标，它是个 ink 终端
/// 程序，不监听 HTTP），所以 `--port 3081` 只对 web 那条路有意义 —— 这一点写在这里，是为了让
/// "TUI 为什么不换端口"有一个能读到的答案，而不是看起来像漏了。
pub(crate) fn tui_launch_command(launcher: &Path, args: &[String]) -> (PathBuf, Vec<String>) {
    let mut command_args = vec![
        "/c".to_string(),
        "start".to_string(),
        // The empty title is not decoration: `start` reads its first quoted
        // argument as a window title, and without this an unquoted path would be
        // taken as one.
        String::new(),
        "cmd".to_string(),
        "/k".to_string(),
        launcher.to_string_lossy().into_owned(),
    ];
    command_args.extend(args.iter().cloned());
    (PathBuf::from("cmd.exe"), command_args)
}

/// The program and arguments that start a **globally installed** DSH CLI.
///
/// npm's Windows launcher is a `.cmd` batch file, and `CreateProcess` cannot run one
/// directly — it needs `cmd /c` in front of it. A `.exe` (a different packaging, or a
/// future npm) is spawned as-is, so this decides by extension instead of assuming.
///
/// `args` are appended **last**, after the launcher's own flags. That order is not
/// cosmetic: the DSH launcher parses only the flags it owns and hands everything after
/// the first unrecognised token to the booted profile's app (see the installed
/// `lib/types/args.d.ts`), so `--port 3081` only reaches the web app when it follows
/// `--profile web`. Appending is exactly what 「启动参数」 promises.
///
/// Pure, and that is the point: the spawn site stays boring, and the tests can read
/// the exact command line a stored subject would produce on this machine.
pub(crate) fn installed_cli_command(
    launcher: &Path,
    profile: &str,
    args: &[String],
) -> (PathBuf, Vec<String>) {
    let mut profile_args = vec!["--profile".to_string(), profile.to_string()];
    // `dsh web` 的默认行为是"起服务**并且打开默认浏览器**"。这个决定该由壁纸来做：设置里选的
    // 是浏览器还是终端里的 TUI，而且开机自启时更不该自己弹窗。`--no-open` 是 **web 应用自己的**
    // 旗标，所以只在 `web` 这个档案上带 —— 别的档案的 app 未必认这个参数。
    if profile.trim() == "web" {
        profile_args.push("--no-open".to_string());
    }
    profile_args.extend(args.iter().cloned());
    let extension = launcher
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("cmd") | Some("bat") => {
            let mut command_args = vec!["/c".to_string(), launcher.to_string_lossy().into_owned()];
            command_args.extend(profile_args);
            (PathBuf::from("cmd.exe"), command_args)
        }
        _ => (launcher.to_path_buf(), profile_args),
    }
}

/// Who is asking for the start, which is what decides the one rule that still differs.
///
/// 「显示还是不显示那个窗口」由**谁在问**决定，而不是由"是不是用户按的"决定：用户按下的
/// 两处控制想要的东西本来就不一样 —— 滑槽要的是"把这个主体供起来"（界面在壁纸这边），
/// 「打开」要的是"把它的窗口给我看"。所以这里三个变体，两条隐藏、一条显示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchTrigger {
    /// The island's launch slider: the user asked for the subject to *serve*, not for
    /// its window (§5.3 — 点滑槽与开机静默启动做的是同一件事).
    ///
    /// 这条路上窗口必须留在屏幕外：用户要的是"切到 harness 用壁纸聊天"，一个自己弹出来的
    /// 客户端窗口是他没要的第二个界面。要显示它有专门的那一个动作（岛上的图标、设置里的
    /// 「打开」），两者都走 [`LaunchTrigger::Manual`]。
    Slider,
    /// The wallpaper's own unattended start, which keeps a shell's window out of sight
    /// (§5.1).
    ///
    /// 它**不再**带"自定义启动命令是否已获授权"这一位：那个设置已经不在了（现在只有「启动参数」，
    /// 而参数加不了参数以外的任何东西 —— 启动器本身永远是扫描决定的、原生自己选的）。原来那一问
    /// 的前提是"无人值守时要不要执行用户随手填的一个程序"，前提消失，问题也随之消失。
    Automatic,
    /// A control whose whole point is to put the interface on screen: 设置里的「打开」and
    /// the island's own raise affordance. A window is shown — they asked for the thing
    /// on screen.
    Manual,
}

impl LaunchTrigger {
    /// Whether this trigger's start may put the window out of sight.
    fn starts_in_background(self) -> bool {
        match self {
            Self::Slider | Self::Automatic => true,
            Self::Manual => false,
        }
    }
}

/// Whether a start at this trigger hides the subject's window — the whole decision, in
/// one place.
///
/// `can_start_hidden` stays **authoritative**: a subject whose capability says it cannot
/// be started in the background is never hidden, whatever asked for the start. That is
/// the direction that must not be reversed — a hidden window nobody can raise is an
/// application the user cannot reach, while a visible window is merely an annoyance.
/// Pure, so both directions of the rule are pinned without a machine.
pub(crate) fn keeps_window_hidden(trigger: LaunchTrigger, can_start_hidden: bool) -> bool {
    trigger.starts_in_background() && can_start_hidden
}

/// Decide how to start `id`, or refuse with a closed code.
///
/// The trigger is the one input that changes behaviour, and it changes it in one
/// place on purpose: the two unattended-shaped starts keep a shell's window out of
/// sight (§5.1), while the action whose point is to show the interface does not. It
/// does **not** change what runs — the launcher is always one this build picked, for
/// both paths, and `args` is therefore honoured identically whether the user pressed a
/// button or the wallpaper started it.
pub(crate) fn plan_launch(
    id: &str,
    profile: &str,
    args: &[String],
    trigger: LaunchTrigger,
) -> Result<LaunchPlan, &'static str> {
    if let Some(launcher) = id.trim().strip_prefix(CLI_ID_PREFIX) {
        let launcher = launcher.trim();
        if launcher.is_empty() {
            return Err("unknown-target");
        }
        return Ok(LaunchPlan::InstalledCli {
            launcher: launcher.to_string(),
            profile: profile.trim().to_string(),
            args: args.to_vec(),
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
            hide_window: keeps_window_hidden(trigger, shell.can_start_hidden),
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
    Ok(LaunchPlan::Checkout {
        root_path: root_path.to_string(),
        profile: profile.trim().to_string(),
        args: args.to_vec(),
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
        LaunchPlan::InstalledCli {
            launcher,
            profile,
            args,
        } => launch_installed_cli(launcher, profile, args),
        LaunchPlan::Checkout {
            root_path,
            profile,
            args,
        } => match crate::spawn_managed_dsh(managed, root_path, root_path, profile, args) {
            // `spawn_managed_dsh` returns the existing pid when this process
            // already owns a running child for this instance, so "started" also
            // covers "already managed"; ownership is the same either way.
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

    // Which windows belong to this subject is asked **once**, before the start, from
    // the record the last scan wrote: that path does not depend on the client being up
    // yet, and asking it here keeps the waiting loop below about one thing.
    let executable = recorded_shell_executable(aumid);

    if !spawn_alias(alias, aumid) {
        return HarnessLaunchOutcome::new("spawn-failed", HarnessTargetKind::EmbeddedShell);
    }

    let (confirmed, hidden) = match port {
        Some(port) => wait_for_shell(port, hide_window, executable.as_deref()),
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
/// 隐藏是**反复施加**的，不是一次性的，原因是实测：
///
/// * 壳自己有"把我的窗口拿到前台"的一条路（托盘、`second-instance`、启动完成后的
///   `window.show()`），它会把我们藏起来的窗口重新显示出来 —— 实测：外部隐藏之后请求壳自己
///   聚焦，窗口 540ms 后回到屏幕上；
/// * 而"趁窗口还不可见时先藏起来"并不成立：属于这个可执行文件的 10 个顶层窗口里有 9 个本来
///   就是 `IsWindowVisible == false`（电子客户端建窗口时就是这个样子），对它们施加隐藏是无操作，
///   什么也锁不住。
///
/// 所以这里做的是"只要它露头就再藏一次"，直到**连续 [`SHELL_HIDE_SETTLE`] 屏幕上都没有它的
/// 窗口**为止 —— 那个静默期才是"这次启动结束了"的判据，壳晚一点才显示的那一帧因此也会被盖掉。
/// 代价是一帧：壳自己显示出来的那一帧最多被看到一个 [`SHELL_HIDE_POLL`]。阻止一个进程显示自己
/// 的窗口，在进程外没有别的办法（不碰透明/分层那一类会让 Chromium 渲染出问题的招）。
fn wait_for_shell(port: u16, hide_window: bool, executable: Option<&str>) -> (bool, bool) {
    let deadline = std::time::Instant::now() + SHELL_START_TIMEOUT;
    // When the client started answering. It is the moment after which its window is
    // expected, which is what makes the fast poll below worth paying for.
    let mut confirmed_at: Option<std::time::Instant> = None;
    // When the family was last seen on screen (and therefore last put out of sight
    // again). The settle period is measured from here, so a late `show()` from the
    // client restarts it instead of being missed.
    let mut hidden_since: Option<std::time::Instant> = None;
    while std::time::Instant::now() < deadline {
        if confirmed_at.is_none() && crate::client_window::endpoint_is_listening(port) {
            confirmed_at = Some(std::time::Instant::now());
        }
        let confirmed = confirmed_at.is_some();
        if hide_window && hide_started_shell(port, confirmed, executable) {
            // Something of that executable was on screen and is now out of sight: the
            // first paint, or the client asking for its own window again.
            hidden_since = Some(std::time::Instant::now());
        }
        let settled = hide_settled(hidden_since, std::time::Instant::now());
        if confirmed && (!hide_window || settled) {
            break;
        }
        // The fast poll is a race with the client's first paint, and the race has a
        // bounded useful life: once the client has been up for a while without painting,
        // the slow poll takes over and the hide still happens — later, not never.
        let racing = hidden_since.is_some()
            || confirmed_at.is_some_and(|at| at.elapsed() < SHELL_HIDE_POLL_WINDOW);
        std::thread::sleep(if hide_window && confirmed && !settled && racing {
            SHELL_HIDE_POLL
        } else {
            SHELL_START_POLL
        });
    }
    let hidden = hidden_since.is_some();
    if hide_window && !hidden {
        log::warn!(
            "harness shell window was never hideable: port={port} listening={}",
            confirmed_at.is_some()
        );
    }
    (confirmed_at.is_some(), hidden)
}

/// Whether the hide has been *holding*: something was put out of sight, and nothing of
/// that executable has been on screen since.
///
/// 这是"这次启动的隐藏做完了"的判据，所以它单独成了一个纯函数。判据必须是这样而不是"藏成功过
/// 一次"：壳自己那条显示窗口的路实测 540ms 就能把窗口拿回来，所以"曾经藏住"不构成答案，
/// 只有"藏住之后没人再把它显示出来"才是。`None` 表示一次都没藏到过（窗口还没画出来，或它自己
/// 就是以隐藏状态启动的），那时无论过了多久都不算完成。
fn hide_settled(hidden_since: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    hidden_since.is_some_and(|since| now.saturating_duration_since(since) >= SHELL_HIDE_SETTLE)
}

/// The executable whose windows belong to a shell, as the last scan recorded it.
///
/// Read from the stored subject list rather than carried through the launch plan, for
/// the same reason the launcher re-resolves a subject from its id instead of trusting a
/// stored path: the path is a *report* about an install, while the plan is a decision
/// about what to run. `None` means the record has none (no scan yet, or a record
/// written before the field existed) — the caller then falls back to the client that is
/// actually running, and says so rather than guessing.
fn recorded_shell_executable(aumid: &str) -> Option<String> {
    let catalog = crate::harness_catalog::load_catalog()?;
    crate::harness_targets::recorded_shell_executable(&catalog.targets, aumid)
}

/// Put the windows of the shell that was just started out of sight, and say whether
/// anything was on screen.
///
/// The answer is "did I just hide something", not "is the job finished": the caller
/// applies this repeatedly (see [`wait_for_shell`]), and a `true` is exactly the news
/// that the client had something on screen — the first paint, or its own `show()`.
///
/// 身份用**可执行文件**，不用端口：按端口找到的是监听那个套接字的进程，而窗口属于同一个
/// 可执行文件的另一个进程（电子客户端的运行时在子进程里）。按路径找一次就覆盖整个进程家族，
/// 而且不只一个窗口 —— 实测这台机器上那个可执行文件有 10 个顶层窗口。
///
/// The path sources are tried in the order that needs the least guessing:
///
/// 1. the path the scan recorded for the subject the user chose — the executable their
///    own Start Menu entry starts;
/// 2. the executable of the process answering on `port`, once it answers — the client
///    that is actually running, which is the answer that stays right when the record
///    has no path (this machine's does not, until a re-scan) or when the client was
///    reinstalled somewhere else since that scan;
/// 3. [`crate::client_window::hide_client_window`] — the port-shaped lookup that was the
///    only route before, kept last so that a machine where the executable cannot be read
///    at all behaves as it did then.
fn hide_started_shell(port: u16, confirmed: bool, recorded: Option<&str>) -> bool {
    let live = confirmed
        .then(|| crate::client_window::endpoint_executable(port))
        .flatten();
    for candidate in [recorded.map(str::to_string), live].into_iter().flatten() {
        if crate::client_window::hide_executable_windows(&candidate).hidden {
            log::info!("harness shell window hidden by executable: executable={candidate}");
            return true;
        }
    }
    crate::client_window::hide_client_window(port).hidden
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
    args: &[String],
    managed: &crate::ManagedDshState,
) -> HarnessUiOutcome {
    let kind = subject_kind(subject_id);
    let port = ui_port(subject_id, port);
    // An empty id means "no subject chosen yet": still a legitimate request to
    // reach whatever answers on that endpoint, just nothing to start.
    let plan = if subject_id.trim().is_empty() {
        None
    } else {
        match plan_launch(subject_id, profile, args, LaunchTrigger::Manual) {
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
    // 显示这一半与隐藏那一半认出的是同一个东西：这个主体的可执行文件。设置里的「打开」与岛上
    // 那个图标都走这条命令，所以它们要能把壁纸在后台启动时藏起来的窗口找回来 —— 按端口那条路
    // 找不到它（实测：监听进程不是窗口的属主）。
    let executable = shell
        .as_ref()
        .and_then(|shell| recorded_shell_executable(shell.aumid));
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
            if let Some(raised) = ask_shell_to_focus(shell, port, executable.as_deref()) {
                log::info!(
                    "harness shell focused its own window: aumid={} outcome={}",
                    shell.aumid,
                    raised.outcome
                );
                raised
            } else {
                // Its focus path is not installed yet (it is still booting) or it owns
                // no window after all: fall back to resolving the window ourselves.
                let resolved = reveal(port, kind, executable.as_deref());
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
        _ => reveal(port, kind, executable.as_deref()),
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
    executable: Option<&str>,
) -> Option<crate::client_window::RaiseOutcome> {
    if !spawn_alias(&shell.alias, shell.aumid) {
        return None;
    }
    let deadline = std::time::Instant::now() + SHELL_FOCUS_TIMEOUT;
    loop {
        let raise = raise_shell_window(port, executable);
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
fn reveal(
    port: u16,
    kind: HarnessTargetKind,
    executable: Option<&str>,
) -> crate::client_window::RaiseOutcome {
    // 只有官壳拥有自己的窗口；源码目录与已安装 CLI 的界面是**浏览器**。
    //
    // 原来这里对它们也调一次 `raise_client_window`，而那个函数的做法是"找占用该端口的进程，再找
    // 属于这个进程的任意可见窗口"。对 CLI 来说那是个 Node 宿主：它可能没有窗口，也可能沿着进程
    // 祖先走到**启动它的终端或编辑器**。实测过：用户选了浏览器，被拉到前台的是别人的窗口。
    // 所以这里直接如实回答"没有窗口"，让调用方走用户选的那条路（浏览器），而不是拿一个碰巧
    // 存在的窗口当作主体的界面。等待窗口的宽限只留给官壳，那也是它唯一有意义的地方。
    //
    // 非壳主体的 `executable` 一定是 `None`（只有壳的目标带这个路径），所以这条分类既决定了
    // 要不要等窗口，也决定了去哪里找窗口 —— 两件事本来就该由同一个分类回答。
    if kind != HarnessTargetKind::EmbeddedShell {
        return crate::client_window::RaiseOutcome {
            outcome: "no-window",
            raised: false,
        };
    }
    let deadline = std::time::Instant::now() + UI_WINDOW_TIMEOUT;
    loop {
        let raise = raise_shell_window(port, executable);
        if raise.outcome == "no-window" && std::time::Instant::now() < deadline {
            std::thread::sleep(SHELL_START_POLL);
            continue;
        }
        return raise;
    }
}

/// Bring a shell's window forward, by its executable when this machine knows it.
///
/// The path route is what actually finds the window of the client this wallpaper
/// started in the background: the listener is not the window's owner (measured), and
/// the window is invisible — two things the path-shaped search handles and the
/// port-shaped one only approximates. `raise_client_window(port)` stays as the answer
/// for a machine whose record carries no path at all, so "no scan yet" degrades to the
/// behaviour this build had before rather than to "no window".
fn raise_shell_window(port: u16, executable: Option<&str>) -> crate::client_window::RaiseOutcome {
    if let Some(executable) = executable {
        let shown = crate::client_window::show_executable_windows(executable);
        if shown.outcome != "no-window" {
            return shown;
        }
    }
    crate::client_window::raise_client_window(port)
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

    /// 一条最小的实例记录（测试用）。字段多的结构体，用构造函数比到处写全字段更不容易看漏。
    fn child(instance_key: &str, subject_id: &str, pid: u32, started_at: Option<u64>) -> ManagedChild {
        ManagedChild {
            instance_key: instance_key.into(),
            subject_id: subject_id.into(),
            args: Vec::new(),
            pid,
            started_at,
            port: None,
            handoff: None,
        }
    }

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn an_installed_cli_starts_through_cmd_because_npm_ships_a_batch_file() {
        let (program, command_args) = installed_cli_command(
            Path::new(r"C:\Users\u\AppData\Roaming\npm\dsh.cmd"),
            "web",
            &[],
        );
        // `CreateProcess` cannot execute a `.cmd`; without `cmd /c` this would fail
        // with "not a valid application" on every machine that has the CLI.
        assert_eq!(program, PathBuf::from("cmd.exe"));
        assert_eq!(
            command_args,
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
        let (_, desktop) = installed_cli_command(Path::new(r"C:\tools\dsh.exe"), "desktop", &[]);
        assert_eq!(desktop, vec!["--profile", "desktop"]);
    }

    #[test]
    fn launch_args_are_appended_after_the_launcher_s_own_flags() {
        // 顺序不是装饰：DSH 的启动器只解析自己那几个旗标，**第一个不认识的词之后**整段原样交给
        // 被 boot 的档案（实测 `lib/types/args.d.ts` 就是这么写的）。`--port` 是 web 应用自己的
        // 旗标，所以它必须在 `--profile web` 之后才到得了那个应用。
        let (program, command_args) = installed_cli_command(
            Path::new(r"C:\Users\u\AppData\Roaming\npm\dsh.cmd"),
            "web",
            &argv(&["--port", "3081"]),
        );
        assert_eq!(program, PathBuf::from("cmd.exe"));
        assert_eq!(
            command_args,
            vec![
                "/c",
                r"C:\Users\u\AppData\Roaming\npm\dsh.cmd",
                "--profile",
                "web",
                "--no-open",
                "--port",
                "3081"
            ]
        );
        // 原生启动器（.exe）同样追加在最后。
        let (program, command_args) =
            installed_cli_command(Path::new(r"C:\tools\dsh.exe"), "web", &argv(&["--port", "3082"]));
        assert_eq!(program, PathBuf::from(r"C:\tools\dsh.exe"));
        assert_eq!(command_args, vec!["--profile", "web", "--no-open", "--port", "3082"]);
    }

    #[test]
    fn a_native_launcher_is_started_directly() {
        let (program, command_args) =
            installed_cli_command(Path::new(r"C:\tools\dsh.exe"), "web", &[]);
        assert_eq!(program, PathBuf::from(r"C:\tools\dsh.exe"));
        assert_eq!(command_args, vec!["--profile", "web", "--no-open"]);
    }

    #[test]
    fn the_tui_is_raised_in_a_console_that_stays_until_it_is_read() {
        let (program, command_args) =
            tui_launch_command(Path::new(r"C:\Users\u\AppData\Roaming\npm\dst.cmd"), &[]);
        assert_eq!(program, PathBuf::from("cmd.exe"));
        assert_eq!(
            command_args,
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
        assert!(!command_args.iter().any(|arg| arg == "--profile"));
        // 「启动参数」照常追加：它加的是启动器后面的话，而这条路的启动器就是 TUI 自己。
        let (_, with_args) = tui_launch_command(
            Path::new(r"C:\Users\u\AppData\Roaming\npm\dst.cmd"),
            &argv(&["--model", "flash"]),
        );
        assert_eq!(
            &with_args[with_args.len() - 2..],
            &["--model".to_string(), "flash".to_string()]
        );
    }

    #[test]
    fn the_port_a_launch_declares_is_read_the_way_dsh_declares_it() {
        // 实测 `dsh web --help`：`--port <port>  listen port; pass 0 to let the OS pick a free one`。
        assert_eq!(port_from_args(&argv(&["--port", "3081"])), Some(3081));
        assert_eq!(port_from_args(&argv(&["--port=3082"])), Some(3082));
        assert_eq!(port_from_args(&argv(&["--no-open", "--port", "8080", "--x"])), Some(8080));
        // 没有参数 ⇒ DSH 自己的默认端口（web 与 CLI 都在 3080 上服务）。
        assert_eq!(port_from_args(&[]), None);
        assert_eq!(instance_port(&[]), 3080);
        assert_eq!(instance_port(&argv(&["--port", "3081"])), 3081);
        // 读不出来就说读不出来：绝不猜。
        assert_eq!(port_from_args(&argv(&["--port"])), None);
        assert_eq!(port_from_args(&argv(&["--port", "abc"])), None);
        // `--port 0` 是 web 应用自己的合法用法（让系统挑），但它不是一个可以拿去检查占用的端口。
        assert_eq!(port_from_args(&argv(&["--port", "0"])), None);
        assert_eq!(port_from_args(&argv(&["--port", "70000"])), None);
        // 重复旗标只认**第一个**：后一个不该被当成答案（那正是"解析器替你猜"）。
        assert_eq!(port_from_args(&argv(&["--port", "3081", "--port", "3082"])), Some(3081));
    }

    #[test]
    fn launch_args_are_shape_checked_but_never_re_split() {
        // 空词被丢掉：用户多打一个空格不该变成启动器上一个空参数。
        assert_eq!(normalize_launch_args(Some(argv(&["  ", "--port", " 3081 "]))).unwrap(), argv(&["--port", "3081"]));
        assert_eq!(normalize_launch_args(None).unwrap(), Vec::<String>::new());
        // 控制字符要拒绝，而且理由是**功能需要**：实例键用 U+001F 分隔，参数里再出现同一个字符
        // 就会让两个不同的实例撞成一个键。
        assert_eq!(
            normalize_launch_args(Some(argv(&["a\u{1f}b"]))),
            Err("启动参数里不能包含控制字符".to_string())
        );
        assert!(normalize_launch_args(Some(argv(&["--port\n3081"]))).is_err());
        assert!(normalize_launch_args(Some(vec!["x".repeat(MAX_LAUNCH_ARG_LENGTH + 1)])).is_err());
        assert!(normalize_launch_args(Some(vec!["x".into(); MAX_LAUNCH_ARGS + 1])).is_err());
        // 参数里的 `;` 与 `|` 是普通字符 —— 这里没有命令行解释器，原样传下去才是对的。
        assert_eq!(normalize_launch_args(Some(argv(&["a;b", "c|d"]))).unwrap(), argv(&["a;b", "c|d"]));
    }

    #[test]
    fn two_instances_of_one_subject_are_two_keys() {
        let subject = r"D:\Family\DeepSeekHarness\deepseek-harness";
        // 不同的参数 ⇒ 两个键：这正是"同一个主体并行起两个端口"能共存的原因。
        assert_ne!(
            instance_key(subject, &argv(&["--port", "3081"])),
            instance_key(subject, &argv(&["--port", "3082"]))
        );
        // 同样的参数 ⇒ 同一个键：那是**同一个实例**，重复按「启动」不该多出一个。
        assert_eq!(
            instance_key(subject, &argv(&["--port", "3081"])),
            instance_key(subject, &argv(&["--port", "3081"]))
        );
        // 没有参数时键恰好等于主体 id：升级前写下的记录不需要迁移就能继续对齐。
        assert_eq!(instance_key(subject, &[]), subject);
        assert_eq!(instance_key(&format!("  {subject}  "), &[]), subject);
        // 键里带得出参数，所以两条记录不会长得一模一样。
        assert!(instance_key(subject, &argv(&["--port", "3081"])).contains("3081"));
    }

    #[test]
    fn the_cylinder_keeps_every_instance_of_every_subject() {
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        let tree = r"D:\Family\DeepSeekHarness\deepseek-harness";
        let mut cylinder = ManagedChildren::default();
        // 同一个源码目录的两个实例：一个默认端口、一个 3081。
        cylinder.remember(child(tree, tree, 111, Some(1)));
        cylinder.remember(child(
            &instance_key(tree, &argv(&["--port", "3081"])),
            tree,
            222,
            Some(2),
        ));
        // 另一个主体：CLI。
        cylinder.remember(child(cli, cli, 333, Some(3)));
        // 一个主体问"我的实例有哪些" ⇒ 两个，第三个不属于它。
        let tree_instances = cylinder.for_subject(tree);
        assert_eq!(tree_instances.len(), 2);
        assert_eq!(
            tree_instances.iter().map(|c| c.pid).collect::<Vec<_>>(),
            vec![111, 222]
        );
        assert_eq!(cylinder.for_subject(cli).len(), 1);
        // 停掉 3081 那一个：另一格与另一个主体都不受影响 —— 这就是"只停一个实例"。
        cylinder.forget(&instance_key(tree, &argv(&["--port", "3081"])));
        assert_eq!(cylinder.for_subject(tree).len(), 1);
        assert_eq!(cylinder.for_subject(tree)[0].pid, 111);
        assert_eq!(cylinder.for_subject(cli).len(), 1);
        // 路径里带 `#` 也不会让两个主体撞在一起：查找按**字段**比，不从键里解析前缀。
        let odd = r"D:\a#b";
        cylinder.remember(child(&instance_key(odd, &argv(&["--port", "1"])), odd, 444, Some(4)));
        assert_eq!(cylinder.for_subject(odd).len(), 1);
        assert_eq!(cylinder.for_subject(tree).len(), 1);
    }

    #[test]
    fn the_official_shell_is_never_one_of_our_instances() {
        // 官壳那条记录是给"门票"用的（`ensure_harness_ui` 会写下它），但停止列表里绝不能有它：
        // 它不是本应用的孩子，退出方式是它自己的托盘菜单。
        assert!(!is_managed_by_us("shell:com.deepseek.dsh"));
        assert!(is_managed_by_us(r"D:\Family\DeepSeekHarness\deepseek-harness"));
        assert!(is_managed_by_us(r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd"));
        // 前后空白不该改变判定（存下来的 id 一路都被 trim 过）。
        assert!(!is_managed_by_us("  shell:com.deepseek.dsh  "));
    }

    #[test]
    fn a_ticket_is_only_used_while_the_process_that_earned_it_is_the_same_one() {
        let mut cylinder = ManagedChildren::default();
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        cylinder.remember(ManagedChild {
            handoff: Some("/?token=SECRET".into()),
            port: Some(3080),
            ..child(cli, cli, 4242, Some(1_700_000_000))
        });
        // 那一格正占着这个端口、也仍是同一个进程 ⇒ 票可用（这正是"壁纸重启、宿主还在跑"能省下一次重启的原因）。
        assert_eq!(
            handoff_in(&cylinder, 3080, |child| child.pid == 4242).as_deref(),
            Some("/?token=SECRET")
        );
        // 进程换了（pid 回收、或宿主退出后重启）⇒ 票必须作废：过期门票比没有门票更坏，
        // 它会让浏览器停在同一句道歉页上，而我们以为自己有票。
        assert_eq!(handoff_in(&cylinder, 3080, |_| false), None);
        // 别的端口不认这张票。
        assert_eq!(handoff_in(&cylinder, 4000, |_| true), None);
        // 两个格子都记得这个端口，但**此刻占着端口的不是持票那一格**（不同主体可以先后用同一个
        // 端口）⇒ 不能把它的票拿出来用。这一条是写这个测试时才发现的漏洞：原先只问"是不是我们的
        // 进程"，于是会退回到另一格上。
        cylinder.remember(ManagedChild {
            port: Some(3080),
            ..child(r"D:\checkout", r"D:\checkout", 77, Some(1))
        });
        assert_eq!(handoff_in(&cylinder, 3080, |child| child.pid == 77), None);
        assert_eq!(
            handoff_in(&cylinder, 3080, |child| child.pid == 4242).as_deref(),
            Some("/?token=SECRET")
        );
        // 并行实例也要能各自领到自己的票：同一主体的 3080 与 3081 两张票互不串门。
        cylinder.remember(ManagedChild {
            port: Some(3081),
            handoff: Some("/?token=SECOND".into()),
            ..child(
                &instance_key("D:\\checkout", &argv(&["--port", "3081"])),
                "D:\\checkout",
                78,
                Some(2),
            )
        });
        assert_eq!(
            handoff_in(&cylinder, 3081, |child| child.pid == 78).as_deref(),
            Some("/?token=SECOND")
        );
        assert_eq!(handoff_in(&cylinder, 3080, |child| child.pid == 78), None);
    }

    #[test]
    fn a_ticket_never_reaches_the_log_in_clear() {
        assert_eq!(redact_handoff("/?token=SECRET"), "/?token=••••");
        assert_eq!(redact_handoff("/"), "/");
        // 打码后不能还剩下原文的任何一段。
        let redacted = redact_handoff("/index.html?token=abc123&x=1");
        assert!(!redacted.contains("abc123"));
        assert!(redacted.starts_with("/index.html"));
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
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        cylinder.remember(child(cli, cli, 4242, Some(1_700_000_000)));
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
    fn a_record_written_before_instances_existed_still_reads() {
        // 升级路径：旧记录没有 `instance_key`，也没有 `args`。它必须读成"这个主体的默认实例"，
        // 而不是整份弹仓作废 —— 否则装机重启之后，用户会发现自己启动的 DSH 不认了。
        let legacy = br#"{"children":{"cli:C:\\dsh.cmd":{"subject_id":"cli:C:\\dsh.cmd","pid":9,"started_at":5,"port":3080,"handoff":null}}}"#;
        let cylinder: ManagedChildren = serde_json::from_slice(legacy).expect("legacy record");
        let recorded = cylinder.for_subject(r"cli:C:\dsh.cmd");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].pid, 9);
        assert!(recorded[0].args.is_empty());
        // 空的实例键仍然能按主体 id 找到（默认实例的键就是主体 id）。
        assert_eq!(instance_key(r"cli:C:\dsh.cmd", &[]), r"cli:C:\dsh.cmd");
    }

    #[test]
    fn ownership_needs_the_pid_and_the_start_time_to_agree() {
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        let recorded = child(cli, cli, 4242, Some(1_700_000_000));
        // 两项都对上：是自己的孩子。
        assert!(owns_live_process(Some(&recorded), Some(4242), Some(1_700_000_000)));
        // pid 相同但创建时间不同 ⇒ 那是回收后的**另一个**进程：绝不能认领，更不能去停它。
        assert!(!owns_live_process(Some(&recorded), Some(4242), Some(1_700_000_999)));
        // pid 不同 / 进程已不在 / 没有记录。
        assert!(!owns_live_process(Some(&recorded), Some(4243), Some(1_700_000_000)));
        assert!(!owns_live_process(Some(&recorded), None, None));
        assert!(!owns_live_process(None, Some(4242), Some(1_700_000_000)));
        // 创建时间任一侧取不到 ⇒ 判定为"不是我启动的"（安全方向，绝不误杀）。
        let unknown = ManagedChild { started_at: None, ..recorded.clone() };
        assert!(!owns_live_process(Some(&unknown), Some(4242), Some(1_700_000_000)));
        assert!(!owns_live_process(Some(&recorded), Some(4242), None));
    }

    #[test]
    fn the_browser_handoff_is_read_from_the_line_the_cli_prints() {
        // 实测原文（`dsh web` 启动时打印的那一行）。
        let printed = "dsh web: http://127.0.0.1:3080/?token=w03-O64JxomJCRw9Any3kg9Nap09jNg56tJbeU6pE4s";
        let (port, path) = web_handoff_path(printed).expect("handoff");
        assert_eq!(port, 3080);
        assert_eq!(path, "/?token=w03-O64JxomJCRw9Any3kg9Nap09jNg56tJbeU6pE4s");
        // 换端口也认（端口不是契约）——并行实例的第二张票就是这样被读出来的。
        assert_eq!(web_handoff_path("dsh web: http://127.0.0.1:8080/?token=abc").map(|(p, _)| p), Some(8080));
        // 没有门票的普通行、或别的地址，都不该被误认成门票。
        assert!(web_handoff_path("dsh web: opening the default browser").is_none());
        assert!(web_handoff_path("http://127.0.0.1:3080/").is_none());
        assert!(web_handoff_path("https://example.com/?token=abc").is_none());
    }

    #[test]
    fn an_installed_cli_plans_the_launcher_itself_with_its_profile() {
        let id = format!("{CLI_ID_PREFIX}C:\\Users\\u\\AppData\\Roaming\\npm\\dsh.cmd");
        let plan = plan_launch(&id, " web ", &[], MANUAL).expect("cli plan");
        match plan {
            LaunchPlan::InstalledCli {
                launcher,
                profile,
                args: planned,
            } => {
                assert_eq!(launcher, r"C:\Users\u\AppData\Roaming\npm\dsh.cmd");
                assert_eq!(profile, "web", "the profile is trimmed like a checkout's");
                assert!(planned.is_empty());
            }
            _ => panic!("expected an installed-cli plan"),
        }
        // 参数随计划一路带到命令行：计划里没有它，`run_launch` 就只能靠再来一次全局设置。
        let plan = plan_launch(&id, "web", &argv(&["--port", "3081"]), MANUAL).expect("cli plan");
        match plan {
            LaunchPlan::InstalledCli { args: planned, .. } => assert_eq!(planned, argv(&["--port", "3081"])),
            _ => panic!("expected an installed-cli plan"),
        }
        // 空启动器要拒绝，否则会变成"启动当前目录"那种意外。
        assert_eq!(plan_launch("cli:", "web", &[], MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("cli:   ", "web", &[], AUTO), Err("unknown-target"));
    }

    const OFFICIAL_ID: &str = "shell:com.deepseek.dsh";
    const CHECKOUT: &str = r"D:\Family\DeepSeekHarness\deepseek-harness";

    const MANUAL: LaunchTrigger = LaunchTrigger::Manual;
    const AUTO: LaunchTrigger = LaunchTrigger::Automatic;
    const SLIDER: LaunchTrigger = LaunchTrigger::Slider;

    #[test]
    fn a_shell_plan_carries_the_alias_this_build_knows() {
        let plan = plan_launch(OFFICIAL_ID, "desktop", &[], MANUAL).expect("shell plan");
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
    fn both_background_starts_keep_the_window_out_of_sight_and_the_open_action_does_not() {
        // §5.1: the startup path starts the official shell without showing it...
        let automatic = plan_launch(OFFICIAL_ID, "desktop", &[], AUTO).expect("shell plan");
        assert!(matches!(automatic, LaunchPlan::Shell { hide_window: true, .. }));
        // ...and the island's slider asks for the same thing: the subject is brought up
        // to serve, its interface is the wallpaper's own chat surface.
        let slider = plan_launch(OFFICIAL_ID, "desktop", &[], SLIDER).expect("shell plan");
        assert!(matches!(slider, LaunchPlan::Shell { hide_window: true, .. }));
        // ...while 「打开」 shows the thing the user asked to see.
        let manual = plan_launch(OFFICIAL_ID, "desktop", &[], MANUAL).expect("shell plan");
        assert!(matches!(manual, LaunchPlan::Shell { hide_window: false, .. }));
        // 三者跑的东西完全相同：这次改的只有"窗口显示不显示"，不是"跑什么"。
        assert_eq!(plan_launch(OFFICIAL_ID, "desktop", &[], AUTO), plan_launch(OFFICIAL_ID, "desktop", &[], SLIDER));
    }

    /// 「能不能后台启动」是主体自己说了算的，触发者说了不算。
    ///
    /// 方向不能反：藏起来的窗口如果没人能再显示，用户面对的是一个够不着的程序；而多一个可见的
    /// 窗口只是碍眼。表里唯一的官壳恰好是"可以"，所以这条用纯函数把两个方向都钉住。
    #[test]
    fn a_subject_that_cannot_start_hidden_is_never_hidden() {
        assert!(keeps_window_hidden(AUTO, true));
        assert!(keeps_window_hidden(SLIDER, true));
        assert!(!keeps_window_hidden(MANUAL, true));
        // 能力说不行 ⇒ 无论谁在问，都不藏。
        assert!(!keeps_window_hidden(AUTO, false));
        assert!(!keeps_window_hidden(SLIDER, false));
        assert!(!keeps_window_hidden(MANUAL, false));
    }

    /// "藏住了"= 藏成功过 **且** 之后没人再把它显示出来。
    ///
    /// 三个方向都钉在这里：一次没藏到过（窗口还没画出来）不算完成；刚藏完还在静默期里不算完成；
    /// 静默期过了才算。壳自己那条 show 路实测 540ms 就能把窗口拿回来，所以第二档不是形式主义：
    /// 它正是"别把壳刚显示出来的窗口当成已经藏好了"。
    #[test]
    fn hiding_is_only_settled_once_nothing_brought_the_window_back() {
        let now = std::time::Instant::now();
        // 从没藏到过：不算完成，等多久都不算。
        assert!(!hide_settled(None, now));
        // 刚藏完（0ms、1s、静默期前一瞬间）：还在静默期里。
        assert!(!hide_settled(Some(now), now));
        assert!(!hide_settled(Some(now - std::time::Duration::from_secs(1)), now));
        assert!(!hide_settled(
            Some(now - (SHELL_HIDE_SETTLE - std::time::Duration::from_millis(1))),
            now
        ));
        // 静默期满：这次隐藏成立。
        assert!(hide_settled(Some(now - SHELL_HIDE_SETTLE), now));
        assert!(hide_settled(Some(now - SHELL_HIDE_SETTLE * 3), now));
        // 快速轮询只服务于"第一次绘制"这场竞速，且它自己也有寿命：静默期比它短，
        // 否则一个始终不露头的客户端会让高频率的枚举一直跑下去。
        assert!(SHELL_HIDE_SETTLE < SHELL_HIDE_POLL_WINDOW);
        assert!(SHELL_HIDE_POLL < SHELL_START_POLL);
    }

    /// 藏起来的那个窗口，要由**这个主体自己那条记录**告诉我们是哪个可执行文件的。
    ///
    /// 记录里没有（老记录、没扫过）就是 `None`：那时调用方去问正在应答的那台客户端，而不是拿一个
    /// 猜出来的路径去藏别人的窗口。
    #[test]
    fn the_window_executable_is_selected_from_the_recorded_subject() {
        use crate::harness_targets::recorded_shell_executable;
        let scan = crate::harness_targets::build_scan_for_tests(&[crate::harness_targets::ScannedShortcut {
            aumid: "com.deepseek.dsh".into(),
            directory: r"C:\Users\u\Start Menu".into(),
            target: Some(r"D:\Family\dsh-official\DeepSeek Harness.exe".into()),
        }]);
        let targets = &scan.targets;
        // AUMID 的大小写不参与判定：它来自快捷方式属性，与我们表里的拼法未必逐字相同。
        assert_eq!(
            recorded_shell_executable(targets, "com.deepseek.dsh").as_deref(),
            Some(r"D:\Family\dsh-official\DeepSeek Harness.exe")
        );
        assert_eq!(
            recorded_shell_executable(targets, "COM.DeepSeek.DSH").as_deref(),
            Some(r"D:\Family\dsh-official\DeepSeek Harness.exe")
        );
        // 别的 AUMID、空 AUMID：记录里没有它，答案是"没有路径"。
        assert_eq!(recorded_shell_executable(targets, "ai.deepseek.dsh.desktop"), None);
        assert_eq!(recorded_shell_executable(targets, "   "), None);
        // 路径读不到时也是 `None`，而不是空字符串 —— 空串会让"按路径找窗口"去找一个空路径。
        let unread = crate::harness_targets::build_scan_for_tests(&[crate::harness_targets::ScannedShortcut {
            aumid: "com.deepseek.dsh".into(),
            directory: r"C:\Users\u\Start Menu".into(),
            target: None,
        }]);
        assert_eq!(recorded_shell_executable(&unread.targets, "com.deepseek.dsh"), None);
    }

    #[test]
    fn an_unknown_shell_id_is_refused_instead_of_launched() {
        // The alias string is a shell launch request, so anything outside this
        // build's table must not reach the shell.
        assert_eq!(plan_launch("shell:Notepad", "desktop", &[], MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("shell:", "desktop", &[], MANUAL), Err("unknown-target"));
        assert_eq!(
            plan_launch("shell:Microsoft.Windows.Explorer", "desktop", &[], AUTO),
            Err("unknown-target")
        );
    }

    #[test]
    fn an_empty_subject_is_refused() {
        assert_eq!(plan_launch("   ", "desktop", &[], MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("", "desktop", &[], AUTO), Err("unknown-target"));
    }

    #[test]
    fn a_shell_ignores_the_profile_setting_which_belongs_to_a_checkout_only() {
        // §4.7: a shell uses its own data, so a leftover profile value must not
        // change — or block — its launch.
        let plan = plan_launch(OFFICIAL_ID, "not a profile at all", &[], MANUAL)
            .expect("shell plan");
        assert!(matches!(plan, LaunchPlan::Shell { .. }));
    }

    #[test]
    fn a_checkout_plan_is_its_root_path_with_its_profile_and_args() {
        let plan = plan_launch(CHECKOUT, " desktop ", &argv(&["--port", "3081"]), MANUAL)
            .expect("checkout plan");
        assert_eq!(
            plan,
            LaunchPlan::Checkout {
                root_path: CHECKOUT.into(),
                profile: "desktop".into(),
                args: argv(&["--port", "3081"]),
            }
        );
        // 没有参数时是空表，不是 `None`：空表就是"什么都不加"，与"没设置"没有区别。
        let bare = plan_launch(CHECKOUT, "desktop", &[], MANUAL).expect("checkout plan");
        assert!(matches!(bare, LaunchPlan::Checkout { args, .. } if args.is_empty()));
    }

    #[test]
    fn args_change_nothing_about_who_runs_on_either_trigger() {
        // 这一条钉的正是本次改动的**能力边界**：手动与自动两条路都只跑本 build 自己选的启动器，
        // 用户能加的只有后面的词。所以「启动参数」在两条路上完全一致，而"任意程序"这件事
        // 在两条路上都做不到（它没有入口了：`LaunchPlan` 里没有任何"程序路径"字段）。
        let manual = plan_launch(CHECKOUT, "desktop", &argv(&["--port", "3081"]), MANUAL).expect("manual");
        let automatic = plan_launch(CHECKOUT, "desktop", &argv(&["--port", "3081"]), AUTO).expect("auto");
        assert_eq!(manual, automatic);
        // 计划里唯一与"跑什么"有关的字段是主体 id 与档案；参数只在其后。
        match &manual {
            LaunchPlan::Checkout { root_path, profile, args } => {
                assert_eq!(root_path, CHECKOUT);
                assert_eq!(profile, "desktop");
                assert_eq!(args, &argv(&["--port", "3081"]));
            }
            _ => panic!("expected a checkout plan"),
        }
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
            plan_launch("shell:com.deepseek.dsh", "desktop", &[], LaunchTrigger::Manual)
        );
        // Then the whole action the button performs, on the shell that is already
        // running: it starts nothing, and the alias request only asks the shell to
        // focus the window it owns.
        let state = crate::ManagedDshState::default();
        let ui = ensure_ui(
            "shell:com.deepseek.dsh",
            port,
            "desktop",
            &[],
            &state,
        );
        println!("ensure_ui: {ui:?}");
        println!(
            "port after ensure_ui is still listening: {}",
            crate::client_window::endpoint_is_listening(port)
        );
    }
}
