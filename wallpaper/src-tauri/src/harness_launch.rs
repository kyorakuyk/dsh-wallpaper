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
/// How often the tight tick runs while a show is imminent.
///
/// 这一档就是"一帧能被看见多久"的上界：每次醒来只做一次 `IsWindowVisible`（真机实测这台机器上
/// 是亚微秒级），所以它可以密到 1 毫秒而不花什么 CPU。1 毫秒也是 `Sleep` 的粒度下限，为此竞速
/// 期间会把系统计时器分辨率抬到 1 毫秒再还回去（`client_window::TimerResolution`）；实测发现
/// 延迟中位 1.5 毫秒。
const SHELL_HIDE_TICK: std::time::Duration = std::time::Duration::from_millis(1);
/// 全量枚举最密的那一档：枚举**就是**检测器的时候用（还没认出"壳会显示的那个窗口"）。
///
/// 枚举是这条路上唯一贵的一步：真机实测这台机器有 538 个顶层窗口，一次家族枚举约 30 毫秒
/// （其中 Toolhelp 进程快照约 8 毫秒），而 tick 的一次检查是亚微秒。所以只有"没有比它更快的
/// 检测器"时才用这一档，并且有 [`SHELL_HIDE_RACE_WINDOW`] 兜底。
const SHELL_HIDE_SWEEP_RACE: std::time::Duration = std::time::Duration::from_millis(50);
/// 有 tick 盯着那个窗口时的枚举间隔：枚举这时只负责"顺带看看"（托盘菜单、家族里别的窗口）。
const SHELL_HIDE_SWEEP_GUARD: std::time::Duration = std::time::Duration::from_millis(150);
/// 还没到"显示随时可能发生"之前，全量枚举的间隔。
///
/// 这段时间里循环的活是**找到**那个"已经建好、还没显示"的窗口（拿到句柄以后，竞速期就只剩一次
/// `IsWindowVisible`），所以它不必密：壳在自己的 `show()` 之前还有一整套宿主启动要走。
const SHELL_HIDE_SWEEP_BOOT: std::time::Duration = std::time::Duration::from_millis(250);
/// 密集枚举档最多持续多久（从"显示随时可能发生"起算）。
///
/// 壳显示窗口的时刻就在宿主就绪之后不久（`ready` 与 `show()` 是同一段代码里的相邻两步），所以
/// 密集档只需要覆盖那几秒。超过它还没有任何一次成功，说明这台机器上"还没显示的那个窗口"认不
/// 出来（例如壳换成了不是电子的实现）—— 那时枚举退到守护档：仍然会藏，只是不再以整机 CPU 为
/// 代价去抢那一帧。
const SHELL_HIDE_RACE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
/// Bridge 的时钟每多久问一次：宿主是不是已经就绪。
///
/// 单飞且有界：一次 `GET /api/wallpaper/v1/status` 在这台机器上实测中位 0.8 毫秒（见
/// `docs/evidence/...` 里那次测量），所以 5 毫秒一档的代价约为这块时间里六分之一颗核；它只在
/// "宿主还没就绪"这个阶段跑，Bridge 一报告就绪就停，不到放弃时刻也停。
const SHELL_HOST_POLL: std::time::Duration = std::time::Duration::from_millis(5);
/// Bridge 始终不报告就绪时，这台时钟最多走多久。
const SHELL_HOST_POLL_GIVE_UP: std::time::Duration = std::time::Duration::from_secs(15);
/// 家族进程第一次出现是每多久探一次（一次 Toolhelp 快照 + 路径确认，约 1 毫秒）。
const SHELL_FAMILY_PROBE: std::time::Duration = std::time::Duration::from_millis(100);
/// 一次后台启动的隐藏**至多**持续多久 —— 这一场竞速的绝对上限。
///
/// 它不是"要藏多久"：正常的启动在第一次藏住之后 [`SHELL_HIDE_SETTLE`] 就结束了。取 45 秒是因为
/// 用户报告"开机第一次启动远不止十几秒"，而热态实测约 6 秒 —— 3 倍于最坏报告，够覆盖一次冷启动，
/// 又不会让一场竞速无限延长。它**不**延长任何一次对抗：第一次藏住之后再现的窗口一律归用户
/// （见 [`wait_for_shell`] 的规则 2）。
const SHELL_HIDE_EPISODE: std::time::Duration = std::time::Duration::from_secs(45);
/// How long a shell's windows must stay off screen before a background start is done.
///
/// Not a guess about boot time: the client's own `show()` is measured to win against an
/// external hide (540 ms after the request, on this machine), so "hidden once" is not an
/// answer — "hidden and nothing brought it back" is. Two seconds of quiet after the
/// first hide is what separates the two.
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
    /// 本应用为**某个主体**跑起来的宿主：它的 CLI，**绑壁纸自己的端口**（`WALLPAPER_HOST_PORT`）。
    ///
    /// 与 `InstalledCli` 的差别有两处，两处都关键：记录用的主体 id 是 `host:<主体>`
    /// （所以它归我们管、可以被我们收回；而主体自己的 `shell:<aumid>` 仍然永不归我们管），
    /// 而且它**没有窗口**（`web` 档案、`--no-open`）——那正是这条路存在的理由。
    ///
    /// 端口**不是**主体自己的那个（施工文档 §7.6 修正版）：19387 是壳自己要绑的，我们占着它，
    /// 用户直接打开壳就会撞上 `EADDRINUSE` 并拿到一个错误框（实测）。所以两个字段分开：
    /// `subject_port` 只用来判断"客户端是不是已经在自己服务了"，`host_port` 才是我们绑的。
    SubjectHost {
        /// `host:<主体 id>`：记录与停止都用它。
        host_id: String,
        /// 主体自己的 id，只用于日志与类别。
        subject_id: String,
        kind: HarnessTargetKind,
        launcher: String,
        profile: String,
        args: Vec<String>,
        /// 主体自己（客户端）的端口；它已经有人应答时我们什么都不起。
        subject_port: Option<u16>,
        /// 我们的宿主绑哪个端口。
        host_port: u16,
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

/// 从一份记录里挑出"本应用启动的、官壳除外"的那些，且**此刻还确实是同一个进程**。
///
/// 独立成纯函数是为了能被钉住：这里曾经把判据写反过（`!is_managed_by_us`），而两个消费者
/// （状态清单、"停止全部"）各自都会再滤一遍官壳，于是它**一声不响**——落盘记录那一半永远是空的：
/// 壁纸重启之后既列不出、也停不掉自己启动过的宿主，而"停止全部"还会静默报成功。
fn our_live_instances(
    cylinder: &ManagedChildren,
    alive: &dyn Fn(&str) -> bool,
) -> Vec<ManagedChild> {
    cylinder
        .children
        .values()
        .filter(|child| is_managed_by_us(&child.subject_id))
        .filter(|child| alive(&child.instance_key))
        .cloned()
        .collect()
}

/// 本应用启动的**每一个**仍然活着的实例，官壳除外。
///
/// 官壳那一类**必须**被排除，它不在本应用的管辖范围内：它是用户自己的客户端，退出方式是它
/// 自己的托盘菜单。`ensure_harness_ui` 会给它写下一条记录（那条记录是给"门票"用的），所以
/// 这里不能只靠"壳没有孩子"这个假设，而是明确按 id 前缀过滤。
pub(crate) fn owned_instances_all() -> Vec<ManagedChild> {
    let Some(path) = RECORDS_PATH.get() else { return Vec::new() };
    let cylinder = read_managed_children(path);
    our_live_instances(&cylinder, &|key| owned_instance(key).is_some())
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
/// 跑一个主体的 **CLI 宿主**：`.cmd`/`.exe` 的差别、`web` 档案才有的门票、以及"这个孩子是
/// 我们起的"那条记录，全在这一处。
///
/// `subject_id` 是**记录用的 id**，不一定是主体的 id：壳的后台宿主记在 `host:<主体>` 下
/// （见 `LaunchPlan::SubjectHost`），而那正是"它归我们管、主体自己的进程不归我们管"的分界。
fn launch_cli_host(
    subject_id: &str,
    kind: HarnessTargetKind,
    launcher: &str,
    profile: &str,
    args: &[String],
    hidden: bool,
) -> HarnessLaunchOutcome {
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
        "harness cli host launch: subject={subject_id} program={} args={command_args:?}",
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
                    let subject = subject_id.to_string();
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
                kind,
                pid: Some(child.id()),
                hidden,
            }
        }
        Err(error) => {
            log::warn!("harness cli host launch failed: subject={subject_id} error={error}");
            HarnessLaunchOutcome::new("spawn-failed", kind)
        }
    }
}

/// 已安装 CLI 那一类：记录 id 就是它自己，界面在浏览器里，所以不算"隐藏"。
fn launch_installed_cli(launcher: &str, profile: &str, args: &[String]) -> HarnessLaunchOutcome {
    launch_cli_host(
        &format!("{CLI_ID_PREFIX}{launcher}"),
        HarnessTargetKind::InstalledCli,
        launcher,
        profile,
        args,
        false,
    )
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

/// A shell's own CLI, relative to the executable its windows belong to.
///
/// Measured on the official client: `DeepSeek Harness.exe` sits beside
/// `resources\runtime\cli\bin\dsh.cmd`, and that batch file starts the same executable
/// again in `ELECTRON_RUN_AS_NODE` mode — which is why this route needs no separate
/// Node and why its version can never drift from the client's.
const BUNDLED_CLI_RELATIVE: [&str; 5] = ["resources", "runtime", "cli", "bin", "dsh.cmd"];

/// How a subject's **CLI** is reached, which is what every subject kind collapses onto
/// (施工文档 §7：三种主体都退化为"跑它的 CLI"，所以后台宿主只需要这一条解析).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BackgroundCli {
    /// Start this launcher. `installed_cli_command` owns the `cmd /c` difference and
    /// the `--profile`/`--no-open` prefix; this only answers *which* file.
    Launch(PathBuf),
    /// A source tree already *is* its CLI: the managed chain runs `apps/cli` under the
    /// tree, so there is no separate launcher to resolve and nothing to substitute.
    TreeChain,
}

/// Which CLI can serve as this subject's background host.
///
/// `shell_executable` is the path the scan recorded for the client's own windows
/// (`HarnessTarget::executable`) — never a path derived from the AUMID. That is not
/// pedantry: the id is deliberately location-independent, so the AUMID alone cannot
/// answer where the client is installed, and a caller that "figured it out" from the
/// name would start a path nobody measured.
///
/// `exists` is injected rather than read, so the resolver stays pure: the happy path and
/// both refusals can be pinned without a machine, and the *caller* decides what a
/// refusal means. A shell that refuses falls back to AUMID activation, which is exactly
/// why the two refusals are different codes instead of one.
pub(crate) fn plan_background_cli(
    subject_id: &str,
    shell_executable: Option<&str>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<BackgroundCli, &'static str> {
    let id = subject_id.trim();
    if let Some(launcher) = id.strip_prefix(CLI_ID_PREFIX) {
        let launcher = launcher.trim();
        if launcher.is_empty() {
            return Err("unknown-target");
        }
        let path = PathBuf::from(launcher);
        return if exists(&path) {
            Ok(BackgroundCli::Launch(path))
        } else {
            Err("missing-launcher")
        };
    }
    if id.starts_with(SHELL_ID_PREFIX) {
        let Some(executable) = shell_executable.map(str::trim).filter(|value| !value.is_empty()) else {
            return Err("missing-executable");
        };
        // A bare file name has no directory to hang the install on, and an empty parent
        // would silently turn the relative path into one against the current directory.
        let Some(install) = Path::new(executable)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        else {
            return Err("missing-executable");
        };
        let mut path = install.to_path_buf();
        for segment in BUNDLED_CLI_RELATIVE {
            path.push(segment);
        }
        return if exists(&path) {
            Ok(BackgroundCli::Launch(path))
        } else {
            Err("missing-launcher")
        };
    }
    Ok(BackgroundCli::TreeChain)
}

/// 主体后台宿主该用哪个 CLI 启动器，含"记录可能比这一版构建旧"的处理。
///
/// 政策（施工文档 §7.4 第 5 条）：记录里放不出可执行文件时**先重扫一次**再判。理由是实测的：
/// 本机那份记录是 2026-09-29 写的、没有 `executable` 字段，而同一次重扫就能读到，于是从安装
/// 位置推出的 `resources\runtime\cli\bin\dsh.cmd` 确实存在。记录只在设置窗口扫描时才写，
/// 所以"启动一次"完全可能发生在"记录还没被这一版构建重写过"之前。
///
/// `rescan` 由调用方给：它做完扫描并把新记录落盘后，返回新的可执行文件。**至多重扫一次**，
/// 而且重扫没有带来不同答案时直接沿用第一次的拒绝 —— 同一个输入问两遍只是多花两秒。
pub(crate) fn resolve_background_cli(
    subject_id: &str,
    recorded: Option<String>,
    rescan: &dyn Fn() -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<BackgroundCli, &'static str> {
    let first = plan_background_cli(subject_id, recorded.as_deref(), exists);
    if first.is_ok() {
        return first;
    }
    let refreshed = rescan();
    if refreshed == recorded {
        return first;
    }
    plan_background_cli(subject_id, refreshed.as_deref(), exists)
}

/// 一个壳主体的后台宿主该跑哪个 CLI；`None` 表示退回 AUMID 激活。
///
/// 三处 I/O 都收在这里（读记录、必要时重扫一次、查路径），调用方拿到的就是一个答案 ——
/// 启动路径上不该再出现第二处"这个路径从哪来"的判断。
pub(crate) fn shell_host_cli(subject_id: &str) -> Option<PathBuf> {
    let aumid = subject_id.trim().strip_prefix(SHELL_ID_PREFIX)?;
    known_shell(aumid)?;
    let resolved = resolve_background_cli(
        subject_id,
        recorded_shell_executable(aumid),
        &|| {
            // 重扫只读用户数据，落盘只是为了让下一次不必再扫。它慢（本机约 2.8 秒），
            // 所以只在记录放不出可执行文件时才走，而且只走一次。
            let scan = crate::harness_targets::scan_harness_targets_blocking(None, false);
            crate::harness_catalog::persist_scan(&scan);
            crate::harness_targets::recorded_shell_executable(&scan.targets, aumid)
        },
        &|path| path.exists(),
    );
    match resolved {
        Ok(BackgroundCli::Launch(path)) => Some(path),
        _ => None,
    }
}

/// 后台启动（滑槽、随壁纸自启）该怎么跑 —— §7.6 走法 A 之后，壳与别的类走的路不同了。
///
/// * 壳：能解析出它自带的 CLI 就跑那个（`web` 档案、**绑壁纸自己的端口**、没有窗口），
///   记录用的 id 是 `host:<主体>`；解析不出来就退回原来的 AUMID 激活（背景启动照旧把窗口
///   留在屏幕外）；
/// * 已安装 CLI 与源码树：原样交给 `plan_launch` —— 它们本来就是"跑自己的 CLI"。
pub(crate) fn plan_background_launch(
    id: &str,
    profile: &str,
    args: &[String],
    trigger: LaunchTrigger,
    subject_host_cli: Option<&Path>,
) -> Result<LaunchPlan, &'static str> {
    let subject = id.trim();
    if let Some(aumid) = subject.strip_prefix(SHELL_ID_PREFIX) {
        if let (Some(launcher), Some(shell)) = (subject_host_cli, known_shell(aumid)) {
            // **端口是我们的，不是用户说的那个**：许可端口表里壳自己的端口是 19387、我们的宿主
            // 在 `WALLPAPER_HOST_PORT`，而参数里的 `--port` 在这里一律丢掉 —— 让用户改掉我们绑在
            // 哪个端口上，就等于让探测表与实际对不上（表是按主体算的，不读参数）。
            // 「启动参数」的其余部分照常送达。
            let mut host_args = without_port_flag(args);
            host_args.push("--port".to_string());
            host_args.push(crate::harness_targets::WALLPAPER_HOST_PORT.to_string());
            return Ok(LaunchPlan::SubjectHost {
                host_id: crate::harness_targets::host_subject_id(subject),
                subject_id: subject.to_string(),
                kind: HarnessTargetKind::EmbeddedShell,
                launcher: launcher.to_string_lossy().into_owned(),
                // 只有 `web` 能提供 HTTP 且能装桥；`desktop` 档案两边的 CLI 都会拒绝。
                profile: "web".to_string(),
                args: host_args,
                subject_port: shell.default_port,
                host_port: crate::harness_targets::WALLPAPER_HOST_PORT,
            });
        }
    }
    plan_launch(subject, profile, args, trigger)
}

/// 「启动参数」去掉 `--port` 那一段（`--port N` 与 `--port=N` 两种写法都算）。
///
/// 壳那条路上端口是**契约**不是偏好：探测表按主体算端口，用户改不掉它，所以也不该能改我们绑在
/// 哪儿。其余参数一个字都不动 —— 用户写的别的词仍然原样送到启动器后面。
fn without_port_flag(args: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    let mut iterator = args.iter();
    while let Some(value) = iterator.next() {
        if value == "--port" {
            let _ = iterator.next();
            continue;
        }
        if value.starts_with("--port=") {
            continue;
        }
        kept.push(value.clone());
    }
    kept
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
    trigger: LaunchTrigger,
) -> HarnessLaunchOutcome {
    match plan {
        LaunchPlan::Shell {
            aumid,
            alias,
            port,
            hide_window,
        } => launch_shell(aumid, alias, *port, *hide_window, trigger),
        LaunchPlan::SubjectHost {
            host_id,
            subject_id,
            kind,
            launcher,
            profile,
            args,
            subject_port,
            host_port,
        } => {
            // 两个端口各有一句话要说。
            //
            // **主体自己的端口有人应答**（用户开着客户端，或者另一个 DSH 宿主在那儿）⇒ 什么都别起：
            // 那个进程服务的是同一份数据、同一套桥，壁纸连上它就是对的答案。这与 `launch_shell`
            // 开头那条判断是同一条规矩。
            if let Some(port) = subject_port {
                if crate::client_window::endpoint_is_listening(*port) {
                    log::info!(
                        "harness subject host not needed: subject={subject_id} port={port} already answers"
                    );
                    return HarnessLaunchOutcome::new("already-running", *kind);
                }
            }
            // **我们自己的端口有人应答** ⇒ 上一次起的宿主还在，重复按「启动」应当是幂等的一次，
            // 而不是第二个进程白撞一次 `EADDRINUSE`。
            if crate::client_window::endpoint_is_listening(*host_port) {
                log::info!(
                    "harness subject host already serving: subject={subject_id} port={host_port}"
                );
                return HarnessLaunchOutcome::new("already-running", *kind);
            }
            log::info!(
                "harness subject host: subject={subject_id} record={host_id} profile={profile} port={host_port}"
            );
            launch_cli_host(host_id, *kind, launcher, profile, args, true)
        }
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
    trigger: LaunchTrigger,
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

    // 新的一次后台启动有自己的诉求：上一次"用户要它出现"的落款到此为止。**只有后台启动会清它**
    // —— 而它也不在这里被设上（设它的是揭示路径与托盘菜单，见 [`RevealIntent`]）。
    if hide_window {
        clear_user_wants_shell();
    }

    // 时刻从这里算起：这一行下面第一件事就是向 Windows shell 提出启动请求。
    let timeline = LaunchTimeline::start(trigger);
    if !spawn_alias(alias, aumid) {
        return HarnessLaunchOutcome::new("spawn-failed", HarnessTargetKind::EmbeddedShell);
    }

    let (confirmed, hidden, visible_frame) = match port {
        Some(port) => wait_for_shell(port, hide_window, executable.as_deref(), timeline),
        None => {
            log::warn!("harness shell has no port to wait for: aumid={aumid}");
            (false, false, None)
        }
    };
    log::info!(
        "harness shell launched: aumid={aumid} trigger={trigger:?} confirmed={confirmed} hidden={hidden} visible_frame_ms={}",
        visible_frame.map_or_else(|| "none".to_string(), |ms| ms.to_string())
    );
    HarnessLaunchOutcome {
        outcome: if confirmed { "started" } else { "started-unconfirmed" }.into(),
        kind: HarnessTargetKind::EmbeddedShell,
        pid: None,
        hidden,
    }
}

/// 一次后台启动的时间线。
///
/// 存在的理由是一个必须用测量回答的问题：**Bridge 在壳显示窗口之前还是之后挂载**。壁纸日志的
/// 时间戳只到秒（`%LOCALAPPDATA%\com.dsh.wallpaper\logs\dsh-wallpaper.log` 里是
/// `[2026-09-28][19:42:48]`），所以时间必须由这里自己带上：每一行都写 `+Nms`，相对"壁纸提出
/// 启动请求"那一刻。一次启动十几行，便宜，而且答案会随着壳每一次更新重新变得值得核对，所以它
/// 不是临时代码 —— 它是这台竞速唯一的仪表盘。
struct LaunchTimeline {
    /// 壁纸向 Windows shell 提出启动请求的那一刻。
    requested_at: std::time::Instant,
    /// 已经报过的里程碑：同一个里程碑只报一次，因为它回答的是"第一次"。
    reported: std::collections::BTreeSet<&'static str>,
}

impl LaunchTimeline {
    fn start(trigger: LaunchTrigger) -> Self {
        let requested_at = std::time::Instant::now();
        let timeline = Self {
            requested_at,
            reported: std::collections::BTreeSet::new(),
        };
        log::info!("[launch-timing] requested +0ms trigger={trigger:?}");
        timeline
    }

    fn mark(&mut self, label: &'static str, detail: impl std::fmt::Display) {
        if !self.reported.insert(label) {
            return;
        }
        log::info!(
            "[launch-timing] {label} +{}ms {detail}",
            self.requested_at.elapsed().as_millis()
        );
    }
}

/// 「用户已经要了这个窗口」：这一场后台启动的隐藏从此不再插手。
///
/// 为什么需要一个显式状态：壳自己那条显示窗口的路（托盘、`second-instance`、启动完成后的
/// `show()`）实测 540ms 就能把窗口拿回来，而重藏是毫秒级的 —— 继续重藏就变成"壁纸把用户按死
/// 在屏幕上"，比闪一帧坏得多。用户在设置里按「打开」、在岛上按桌面会话图标，落的就是这一款。
///
/// 三个写入者，**没有一个是这个循环自己的动作**：
/// 1. [`note_user_wants_shell`] —— 揭示路径（`ensure_ui`）在动手之前落款；
/// 2. [`RevealIntent::observe_human_interaction`] —— 循环**看见**壳的托盘菜单（`#32768`）出现在
///    屏幕上。那只能是人点出来的：循环自己既不弹菜单，也不显示任何窗口；
/// 3. [`clear_user_wants_shell`] —— 下一次**后台**启动把它清掉（新的那次启动有自己的诉求）。
///
/// 循环只读它（[`RevealIntent::wants`]），而且每一次藏之前都读。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RevealIntent {
    wanted: Option<String>,
}

impl RevealIntent {
    /// 用户要的是不是这个可执行文件的窗口。
    pub(crate) fn wants(&self, executable: &str) -> bool {
        let wanted = crate::client_window::normalized_executable_path(executable);
        !wanted.is_empty() && self.wanted.as_deref() == Some(wanted.as_str())
    }

    /// 落款：用户明确要这个窗口出现。
    pub(crate) fn note_user_request(&mut self, executable: &str) {
        let wanted = crate::client_window::normalized_executable_path(executable);
        if !wanted.is_empty() {
            self.wanted = Some(wanted);
        }
    }

    /// 看见人手：壳的托盘菜单出现了。
    ///
    /// 与 [`Self::note_user_request`] 是同一款，分开写是因为**来路**不同：一个是明确的命令，
    /// 一个是观察到的人手。将来若要收紧口径，先收紧哪一个应当是清楚的选择，而不是一处含糊的
    /// 合并。
    pub(crate) fn observe_human_interaction(&mut self, executable: &str) {
        self.note_user_request(executable);
    }

    /// 忘掉它（下一次后台启动）。
    pub(crate) fn clear(&mut self) {
        self.wanted = None;
    }
}

static REVEAL_INTENT: std::sync::OnceLock<std::sync::Mutex<RevealIntent>> =
    std::sync::OnceLock::new();

fn reveal_intent() -> &'static std::sync::Mutex<RevealIntent> {
    REVEAL_INTENT.get_or_init(|| std::sync::Mutex::new(RevealIntent::default()))
}

/// 落款：用户要这个窗口出现（设置里的「打开」、岛上的桌面会话图标）。
pub(crate) fn note_user_wants_shell(executable: &str) {
    if let Ok(mut intent) = reveal_intent().lock() {
        if !intent.wants(executable) {
            log::info!("harness shell window is wanted: executable={executable}");
        }
        intent.note_user_request(executable);
    }
}

/// 下一次后台启动：这一款到此为止。
pub(crate) fn clear_user_wants_shell() {
    if let Ok(mut intent) = reveal_intent().lock() {
        intent.clear();
    }
}

/// 用户要过这个窗口吗（循环每一次藏之前都问）。
fn user_wants_shell(executable: &str) -> bool {
    reveal_intent()
        .lock()
        .map(|intent| intent.wants(executable))
        .unwrap_or(false)
}

/// 循环看见人手：壳的托盘菜单在屏幕上。
fn note_human_interaction(executable: &str) {
    if let Ok(mut intent) = reveal_intent().lock() {
        if !intent.wants(executable) {
            log::info!("harness shell asked for by hand: a menu of {executable} is on screen");
        }
        intent.observe_human_interaction(executable);
    }
}

/// Bridge 的公开状态路由。
///
/// 与 `lib.rs` 里那个探针用的是同一条地址，两者都必须与 `bridge/src/protocol.ts` 的
/// `API_PREFIX` 一致 —— 这条注释就是那份"两处必须一致"的提醒。
const HARNESS_STATUS_PATH: &str = "/api/wallpaper/v1/status";
/// 状态响应最多读这么多字节。它是几百字节的文档，这个上限只是"绝不无界读"。
const MAX_STATUS_BYTES: usize = 8 * 1024;

/// 问一次 Bridge 的公开状态路由：**宿主已经就绪了吗**。
///
/// 这是启动竞速里最好的一台外部时钟，理由是时序：壳的窗口只在宿主子进程发出 `ready` 之后才显示
/// （`app.asar!/lib/main.js:11569-11576`，而 `ready` 由 `dsh-desktop-host` 在应用装配完成之后
/// 发出），而 Bridge 就是那个 profile 里的一个插件 —— 它的 `state: "bridge-ready"` 因此**晚于**
/// 宿主内部就绪、**早于** `show()`。"端口开始应答"更早也更糊（webserver 先监听、插件后装配），
/// 所以两个信号都留着：谁先到用谁。
///
/// 这个路由是**公开**的（`bridge/src/index.ts` 里由 `['webServer']` 作用域挂载、不带令牌），所以
/// 这里不需要读令牌：一次最小 GET、只认一个字段、失败即"还没就绪"。超时就是轮询间隔 —— 一次
/// 问话最多让循环晚一个间隔醒来。
fn host_reported_ready(port: u16) -> bool {
    loopback_get(port, HARNESS_STATUS_PATH, SHELL_HOST_POLL)
        .as_deref()
        .is_some_and(harness_status_reported_ready)
}

/// 对回环端口发一次最小 GET，返回响应文本（头 + 体，最多 [`MAX_STATUS_BYTES`]）。
///
/// 手写 HTTP 而不是拿一个 HTTP 客户端，有两个理由：这条路在 lite 版里也要能编译（那里没有
/// reqwest），而这里要的只是"一次 GET、看一个字段、绝不拖住循环"。两个超时都由调用方给的预算
/// 决定，所以最坏情况也只是这一次问话白问 —— 下一次间隔会再来一次。
fn loopback_get(port: u16, path: &str, timeout: std::time::Duration) -> Option<String> {
    use std::io::{Read, Write};

    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = Vec::with_capacity(MAX_STATUS_BYTES);
    let mut chunk = [0u8; 512];
    while response.len() < MAX_STATUS_BYTES {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            // 超时或对端提前关闭：手上这一段通常已经够了，交给解析去判断。
            Err(_) => break,
        }
    }
    Some(String::from_utf8_lossy(&response).into_owned())
}

/// 这份响应说"宿主已经就绪"了吗。
///
/// 纯函数，所以线格式可以被测试钉住 —— 而它值得被钉住：这条响应在这台机器上是
/// `Transfer-Encoding: chunked`（实测），整段并不是合法 JSON，只看 `{` 到 `}` 之间那一段才是
/// Bridge 自己写的那份文档。
fn harness_status_reported_ready(text: &str) -> bool {
    let Some(status_line) = text.lines().next() else {
        return false;
    };
    // 状态行必须是 200：别的状态码意味着这个端口上答话的不是我们要找的那个 Bridge。
    if !status_line.starts_with("HTTP/") || !status_line.contains(" 200") {
        return false;
    }
    let Some(headers_end) = text.find("\r\n\r\n") else {
        return false;
    };
    let body = &text[headers_end + 4..];
    let (Some(start), Some(end)) = (body.find('{'), body.rfind('}')) else {
        return false;
    };
    if end <= start {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(&body[start..=end])
        .ok()
        .and_then(|document| {
            document
                .get("state")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .is_some_and(|state| state == "bridge-ready")
}

/// 一次全量枚举的结果。
struct FamilySweep {
    /// 家族此刻有多少个顶层窗口（0 表示进程还没起来）。
    windows: usize,
    /// 这些窗口里此刻在屏幕上的有几个。**含弹出物**：这个问题问的是"有没有人想让它可见"，
    /// 而不是"能不能藏"。
    on_screen: usize,
    /// 这一次藏掉了几个。
    hidden: usize,
    /// 屏幕上有没有壳的弹出菜单（= 有人正在跟它打交道）。
    menu_visible: bool,
    /// 那个"已经建好、还没显示"的界面窗口，连同它的类名（记日志用）。
    unshown: Option<(crate::client_window::WindowHandle, String)>,
}

/// 全量枚举一次：藏掉家族此刻在屏幕上的界面窗口（`hide` 为假时只看不藏），并顺便认两件只有枚举
/// 能认出来的事 —— 用户刚点过的托盘菜单，以及那个"已经建好、还没显示"的窗口。
///
/// 身份用**可执行文件**，不用端口：按端口找到的是监听那个套接字的进程，而窗口属于同一个可执行
/// 文件的另一个进程（电子客户端的运行时在子进程里）。按路径找一次就覆盖整个进程家族，而且不只
/// 一个窗口 —— 实测这台机器上那个可执行文件有 10 个顶层窗口。
///
/// 藏的动作**逐个重新判定**（`client_window::hide_window`），不拿枚举时看到的状态直接用：枚举
/// 与隐藏之间隔着几次系统调用，中间窗口可能已经换了主人。
fn sweep_family(port: u16, executable: Option<&str>, hide: bool) -> FamilySweep {
    let Some(executable) = executable else {
        // 记录里没有可执行文件路径（老记录、或还没扫过）⇒ 退回按端口那条路。它一次只能回答一个
        // 窗口，也没有"哪个窗口还没显示"这种事实可认。这是降级，不是错误：按端口那条路一直是
        // 这套动作修复之前的行为。
        let hidden = if hide && crate::client_window::hide_client_window(port).hidden {
            1
        } else {
            0
        };
        return FamilySweep {
            windows: 0,
            on_screen: usize::from(crate::client_window::endpoint_window_on_screen(port)),
            hidden,
            menu_visible: false,
            unshown: None,
        };
    };
    let windows = crate::client_window::family_windows(executable);
    let mut sweep = FamilySweep {
        windows: windows.len(),
        on_screen: windows.iter().filter(|window| window.visible).count(),
        hidden: 0,
        menu_visible: windows.iter().any(|window| window.is_menu()),
        unshown: None,
    };
    for window in windows.iter().filter(|window| window.is_unshown_interface_window()) {
        sweep.unshown = Some((window.handle, window.class.clone()));
        break;
    }
    if hide {
        for window in windows.iter().filter(|window| window.is_hideable()) {
            if crate::client_window::hide_window(window.handle) {
                sweep.hidden += 1;
            }
        }
    }
    sweep
}

/// 循环在等什么，以及下一次醒来之前该睡多久。
///
/// 抽成纯函数是因为它就是"一帧能被看见多久"的上界：tick 越密，帧越短。三个档位对应三种处境，
/// 各自的代价都算得出来 —— 一次 tick 是一次 `IsWindowVisible`（约 1 微秒），一次时钟是一次回环
/// GET（这台机器上实测中位 0.8 毫秒），一次全量枚举是一张进程快照 + 一次窗口枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HidePoll {
    /// 显示随时可能发生：宿主已就绪、或已认出那个还没显示的窗口、或已经藏过一次。
    pub show_imminent: bool,
    /// 这一场竞速还没过期。
    pub within_episode: bool,
    /// Bridge 的时钟还在走（还在等宿主就绪，且没到放弃时刻）。
    pub clock_active: bool,
}

pub(crate) fn hide_poll_interval(state: HidePoll) -> std::time::Duration {
    if state.show_imminent && state.within_episode {
        SHELL_HIDE_TICK
    } else if state.clock_active {
        SHELL_HOST_POLL
    } else {
        SHELL_START_POLL
    }
}

/// 全量枚举该多密 —— 三种处境，三档，理由只有一个：**枚举是这条路上最贵的一步**。
///
/// 真机实测（`this_machine_measures_the_hide_race`）：这台机器有 538 个顶层窗口，一次家族枚举
/// 约 30 毫秒，其中 Toolhelp 进程快照约 8 毫秒；而 tick 的一次检查是亚微秒。所以密集档只留给
/// "没有比枚举更快的检测器"这一种处境，而且还有 [`SHELL_HIDE_RACE_WINDOW`] 兜底。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SweepPoll {
    /// 显示随时可能发生（宿主就绪、端口应答、或已经藏过一次）。
    pub show_imminent: bool,
    /// 枚举**就是**检测器：还没认出"壳会显示的那个窗口"。
    pub sweep_is_the_detector: bool,
    /// 密集档还没过期。
    pub within_race_window: bool,
}

pub(crate) fn hide_sweep_interval(state: SweepPoll) -> std::time::Duration {
    if state.sweep_is_the_detector && state.within_race_window {
        SHELL_HIDE_SWEEP_RACE
    } else if state.show_imminent {
        SHELL_HIDE_SWEEP_GUARD
    } else {
        SHELL_HIDE_SWEEP_BOOT
    }
}

/// 这场循环还要不要继续 —— 两条钟各回答一个问题。
///
/// * `SHELL_START_TIMEOUT`（30 秒）回答"启动被确认了吗"：到点还没人应答就该如实报告
///   `started-unconfirmed`，而不是把"起不来"这件事再拖 15 秒。
/// * `SHELL_HIDE_EPISODE`（45 秒）回答"隐藏最多做多久"：它只在**端口已经应答**之后才延长，
///   因为那时才确实有一个客户端正在启动、确实有一个窗口可能冒出来。
pub(crate) fn hide_episode_continues(
    hide_window: bool,
    confirmed: bool,
    start_horizon_passed: bool,
    episode_horizon_passed: bool,
) -> bool {
    if episode_horizon_passed {
        return false;
    }
    if !start_horizon_passed {
        return true;
    }
    hide_window && confirmed
}

/// 从"第一次看到它在屏幕上"到"藏住"之间过了多久 —— 也就是这一次竞速剩下的那一帧。
///
/// 它是**上界**：看见它的那一刻本身有一个 tick（或一次枚举）的延迟，所以真实帧只会更短。
fn visible_frame_ms(
    visible_at: Option<std::time::Instant>,
    hidden_at: Option<std::time::Instant>,
) -> Option<u128> {
    Some(hidden_at?.saturating_duration_since(visible_at?).as_millis())
}

/// Wait for a freshly started shell to answer, hiding its window on the way.
///
/// 这一场竞速的规则（用户报告之后重写，2026-09-29）：
///
/// 1. **一次启动只有一场隐藏，而且它有绝对上限。** 这一场在"第一次藏住 + 家族连续
///    [`SHELL_HIDE_SETTLE`] 不在屏幕上"或 [`SHELL_HIDE_EPISODE`] 到点时结束；之后这次启动不再
///    藏任何窗口。上限取得宽（45 秒）是因为冷启动"远不止十几秒"，而**防止对抗**靠的是规则 2，
///    不是这个上限。
/// 2. **第一次藏住之后窗口再出现，这一场立刻结束。** 那是"有人要它"的证据（壳自己的托盘、
///    `second-instance`、用户点了它），用户的显示赢：一次启动最多一次对抗。这条规则替代了老做法
///    ——老做法是"只要它露头就再藏一次，直到连续 2 秒不在屏幕上"，而用户每次把窗口拿出来都会
///    重置那 2 秒，于是隐藏越藏越久，用户看到的是"窗口刚出来就被按回去，而且是十几秒"。
/// 3. 用户明确要过的窗口（设置里的「打开」、岛上的桌面会话图标、壳自己的托盘菜单）**连这一次
///    对抗都不会发生**：落款在先，循环每一次藏之前都读它（见 [`RevealIntent`]）。
/// 4. 隐藏永远不让启动失败：藏不到就是"窗口还在屏幕上"，如实报告。
///
/// 时长只剩下两处：壳自己显示出来的那一帧最多被看一个 [`SHELL_HIDE_TICK`]（1 毫秒，前提是那个
/// "还没显示"的窗口已经认出来），以及在竞速退化成按端口枚举时最多一个
/// [`SHELL_HIDE_SWEEP_RACE`]。零帧做不到：外部没有"别显示"的开关，能做的只是比对手快 ——
/// 真正的零帧只能靠一个盖在它上面的不透明面，那是另一件事，本项目不做（见
/// `docs/evidence/dsh-shell-background-launch-timing.md`）。
fn wait_for_shell(
    port: u16,
    hide_window: bool,
    executable: Option<&str>,
    mut timeline: LaunchTimeline,
) -> (bool, bool, Option<u128>) {
    let started_at = std::time::Instant::now();
    let start_horizon = started_at + SHELL_START_TIMEOUT;
    let episode_horizon = started_at + SHELL_HIDE_EPISODE;
    let clock_horizon = started_at + SHELL_HOST_POLL_GIVE_UP;
    // 家族的可执行文件：记录里那条路径（如果有），否则等端口应答后问正在服务的那台客户端。
    let mut family: Option<String> = executable.map(str::to_string);
    // When the client started answering. It is the moment after which its window is
    // expected, which is what makes the fast poll below worth paying for.
    let mut confirmed_at: Option<std::time::Instant> = None;
    // 宿主就绪（Bridge 自己报的）。这是"显示即将发生"最贴近的那台时钟。
    let mut bridge_ready_at: Option<std::time::Instant> = None;
    // 已经建好、还没显示的界面窗口：壳自己的 show() 会显示的那一个。
    let mut watched: Option<crate::client_window::WindowHandle> = None;
    // 第一次真正藏住是什么时候。它有值之后，这一场就只等结算或等用户要回去（规则 2）。
    let mut hidden_at: Option<std::time::Instant> = None;
    // 第一次看到家族在屏幕上（壳自己把它显示出来的那一刻，我们这一侧看到的）。
    let mut visible_at: Option<std::time::Instant> = None;
    let mut visible_frame: Option<u128> = None;
    let mut family_probe_at: Option<std::time::Instant> = None;
    let mut sweep_at: Option<std::time::Instant> = None;
    // 第一个"显示随时可能发生"的信号：密集枚举档的起算点。
    let mut race_started_at: Option<std::time::Instant> = None;
    // 最近一次枚举里"屏幕上有没有这个家族的东西"。规则 2 用它回答"它是不是又回来了"。
    let mut on_screen = false;
    let mut sweeps = 0u32;
    // 铺在壳窗口前面的那层冻结画面（只在后台启动里用；见 `client_window::LaunchCover`）。
    // 拿它换的不是"更快"，而是"没有可见的一帧"：壳的 show() 会画在这层底下。
    let mut cover: Option<crate::client_window::LaunchCover> = None;
    // "窗口要显示"的现场通知（`SetWinEventHook`）。它连同 `pump_show_events` 一起工作。
    let mut show_watch: Option<crate::client_window::ShowEventWatch> = None;
    let mut hidden_windows = 0u32;
    // 抬到 1 毫秒只在这一场里需要（[`SHELL_HIDE_TICK`] 的兑现条件），出作用域就还回去 ——
    // 它最多活 [`SHELL_HIDE_EPISODE`]，而且只出现在一次后台启动里。
    let _resolution = hide_window.then(crate::client_window::TimerResolution::new);

    loop {
        let now = std::time::Instant::now();
        if !hide_episode_continues(
            hide_window,
            confirmed_at.is_some(),
            now >= start_horizon,
            now >= episode_horizon,
        ) {
            break;
        }

        // 端口应答：既是"启动被确认"的判据，也是家族路径最后的来源。
        if confirmed_at.is_none() && crate::client_window::endpoint_is_listening(port) {
            confirmed_at = Some(now);
            timeline.mark("port-answered", format!("port={port}"));
            if family.is_none() {
                family = crate::client_window::endpoint_executable(port);
                if let Some(path) = &family {
                    timeline.mark("family-executable", format!("executable={path}"));
                }
            }
        }
        // 这一档（手动触发）要的是"窗口给我看"，所以它只等端口应答：没有窗口要藏，
        // 也就没有竞速。
        if !hide_window && confirmed_at.is_some() {
            return (true, false, None);
        }

        // 规则 3：用户已经要它了 —— 这一场到此为止，而且连一次对抗都不发生。
        if hide_window && family.as_deref().is_some_and(user_wants_shell) {
            timeline.mark(
                "user-wants",
                "the user asked for this window: no hide, the episode is over",
            );
            return (confirmed_at.is_some(), false, visible_frame);
        }

        if hide_window {
            // Bridge 的时钟：宿主就绪 = 显示即将发生。**只在第一次藏住之前走**，之后不再轮询
            // —— 已经不藏了，再问也没有用。
            let clock_active = hidden_at.is_none() && bridge_ready_at.is_none() && now < clock_horizon;
            if clock_active && host_reported_ready(port) {
                bridge_ready_at = Some(std::time::Instant::now());
                timeline.mark(
                    "bridge-ready",
                    format!("port={port} poll={}ms", SHELL_HOST_POLL.as_millis()),
                );
            }
            // 家族进程第一次出现。回答的是时间而不是身份，所以哪一个进程不重要。
            if hidden_at.is_none() && family_probe_at.map_or(true, |at| now.duration_since(at) >= SHELL_FAMILY_PROBE) {
                family_probe_at = Some(now);
                if let Some(path) = family.as_deref() {
                    if let Some(pid) = crate::client_window::first_process_of_executable(path) {
                        timeline.mark(
                            "family-process",
                            format!("pid={pid} probe={}ms", SHELL_FAMILY_PROBE.as_millis()),
                        );
                    }
                }
            }

            let show_imminent =
                hidden_at.is_some() || bridge_ready_at.is_some() || confirmed_at.is_some() || watched.is_some();
            // 密集枚举的起点：第一个"显示随时可能发生"的信号。它只用来给密集档设一个寿命。
            if show_imminent && race_started_at.is_none() {
                race_started_at = Some(std::time::Instant::now());
            }
            let sweep_interval = hide_sweep_interval(SweepPoll {
                show_imminent,
                // 已经认出那个窗口（或已经藏到过）⇒ 1 毫秒的 tick 才是检测器，枚举不必再抢。
                sweep_is_the_detector: watched.is_none() && hidden_at.is_none(),
                within_race_window: race_started_at
                    .map_or(false, |at| now.duration_since(at) < SHELL_HIDE_RACE_WINDOW),
            });
            // 正在盯一个已知句柄、而且还没藏到过：**这一档不枚举**。枚举一次约 30ms，而且它阻塞
            // 这个线程 —— 壳的 show() 只要落在那一瞬，可见帧就等于一次枚举（实测 31ms 正是这么来的）。
            // 句柄就是检测器；只有"还没认出句柄"和"已经藏住"（那时枚举只负责看它有没有再回来）才枚举。
            if (hidden_at.is_some() || watched.is_none())
                && sweep_at.map_or(true, |at| now.duration_since(at) >= sweep_interval)
            {
                sweep_at = Some(now);
                sweeps += 1;
                // 第一次藏住之后只看不藏（规则 1 + 2）。
                let sweep = sweep_family(port, family.as_deref(), hidden_at.is_none());
                on_screen = sweep.on_screen > 0;
                if sweep.windows > 0 {
                    timeline.mark(
                        "family-windows",
                        format!("count={} on_screen={}", sweep.windows, sweep.on_screen),
                    );
                }
                if sweep.menu_visible {
                    if let Some(path) = family.as_deref() {
                        note_human_interaction(path);
                    }
                    timeline.mark(
                        "tray-menu",
                        "a menu of the shell is on screen: the user is asking for it",
                    );
                }
                if sweep.hidden > 0 {
                    hidden_windows += sweep.hidden as u32;
                    visible_at = visible_at.or(Some(now));
                    hidden_at = Some(std::time::Instant::now());
                    visible_frame = visible_frame_ms(visible_at, hidden_at).or(visible_frame);
                    // 位置挪回去：屏幕外那个位置只在"它第一次显示"的那一瞬间有用，此刻窗口已经隐藏，
                    // 所以这次位移谁也看不见；而此后无论谁把它叫出来（壳自己的托盘、我们的展示路径、AUMID
                    // 激活）都会在正确的位置。只靠"展示路径挪回"是不够的 —— 实测用户正是从托盘把它叫回来，
                    // 那条路是壳的代码，我们拦不到。
                    if let Some(executable) = family.as_deref() {
                        if crate::client_window::restore_from_offscreen(executable) {
                            timeline.mark("position-restored", "the window is back where it belongs");
                        }
                    }
                    // 窗口已经不在屏上了：那层冻结画面留着只会挡住用户。
                    if cover.take().is_some() {
                        timeline.mark("cover-removed", "the window is off screen: the frozen frame is no longer needed");
                    }
                    log::info!(
                        "harness shell window hidden: windows={} source=sweep sweep={}ms",
                        sweep.hidden,
                        sweep_interval.as_millis()
                    );
                    timeline.mark(
                        "hide-landed",
                        format!("windows={} sweeps={sweeps} via=sweep", sweep.hidden),
                    );
                    timeline.mark(
                        "visible-frame",
                        format!(
                            "{}ms (upper bound: one sweep = {}ms)",
                            visible_frame.unwrap_or(0),
                            sweep_interval.as_millis()
                        ),
                    );
                }
                if watched.is_none() {
                    if let Some((handle, class)) = sweep.unshown.as_ref() {
                        watched = Some(*handle);
                        timeline.mark(
                            "window-exists-hidden",
                            format!("pid={} class={class}", handle.pid()),
                        );
                        // 句柄已知、它还没显示：现在装上"窗口要显示"的现场通知。它比轮询早一个阶段 ——
                        // 轮询问的是"可见了吗"（那时 DWM 可能已经合成上屏），而事件是壳自己那次
                        // ShowWindow 里发出来的。装了必须抽消息，见循环里的 `pump_show_events`。
                        show_watch = crate::client_window::watch_show_events(*handle);
                        if show_watch.is_some() {
                            timeline.mark("show-hook-installed", format!("pid={}", handle.pid()));
                        }
                        // 挪到屏幕之外：位置与可见性是**两个独立属性** —— 壳随后随便 show()，那块
                        // 地方都不在任何显示器的像素里，所以这里没有赛跑（隐藏会被它的 show 撤销，
                        // 盖一层要抢在它前面铺好，只有挪走一直有效）。展示路径会先挪回再显示。
                        // 一族一起挪：家族里不止一个界面窗口，只挪我们盯着的那个，用户仍会在
                        // 原处看到另一个的轮廓闪出来（实测就是这样）。
                        let moved = family
                            .as_deref()
                            .map(crate::client_window::move_family_offscreen)
                            .unwrap_or(0);
                        if moved > 0 {
                            timeline.mark(
                                "moved-offscreen",
                                format!("{moved} window(s) outside every display until we bring them back"),
                            );
                        }
                        // FREEZE（按用户 2026-09-30 的指示冻结这一方向，不是删除）：这里原来会铺一层
                        // "先截屏、再盖上去"的冻结画面，让壳出生在它底下。实测两次都仍然可见 —— 第一次
                        // 是被上面那条守卫自己挡掉了，第二次守卫收窄之后依旧没能阻止，用户判断这个方向
                        // 不再值得投入，改为试 SetWinEventHook 把观测点提前一个阶段。
                        // 恢复办法：把这十行取消注释、并去掉下面那行 `let _ = &mut cover;`。
                        // 代码本体（`client_window::raise_launch_cover` 与 `LaunchCover`）原样保留。
                        let _ = &mut cover;
                    }
                }
            }

            match (hidden_at, watched) {
                // 还没藏到过：盯着那个已经建好的句柄，壳的 show() 一落地就按下去。
                (None, Some(handle)) => {
                    // 先问"这个句柄还是不是我们能碰的东西"：句柄值会被回收，1 毫秒前记下的数值现在
                    // 可能属于别人刚建出来的窗口。丢失就放手、让下一次枚举重新认一个 —— 只凭
                    // `window_is_visible` 去按，等于赌那个数值没被接手。
                    if matches!(
                        crate::client_window::watch_window_state(handle),
                        crate::client_window::WatchWindowState::Lost
                    ) {
                        watched = None;
                        cover = None;
                        timeline.mark(
                            "watched-window-lost",
                            "the watched handle is no longer a hideable interface window",
                        );
                    } else if crate::client_window::window_is_visible(handle) {
                        // 它第一次可见的那一刻**在哪**：这一问决定"我们挪走的位置是不是被谁又摆回去了"。
                        // 我们挪到屏幕之外，若此刻读回来的矩形已经是原处，那就是壳自己把它请回去的。
                        if visible_at.is_none() {
                            if let Some(rect) = crate::client_window::window_rect(handle) {
                                timeline.mark(
                                    "window-visible-at",
                                    format!("rect=({},{})-({},{})", rect.0, rect.1, rect.2, rect.3),
                                );
                            }
                        }
                        visible_at = visible_at.or(Some(std::time::Instant::now()));
                        if crate::client_window::hide_window(handle) {
                            hidden_windows += 1;
                            hidden_at = Some(std::time::Instant::now());
                            visible_frame = visible_frame_ms(visible_at, hidden_at).or(visible_frame);
                            // 位置挪回去：屏幕外那个位置只在"它第一次显示"的那一瞬间有用，此刻窗口已经隐藏，
                            // 所以这次位移谁也看不见；而此后无论谁把它叫出来（壳自己的托盘、我们的展示路径、AUMID
                            // 激活）都会在正确的位置。只靠"展示路径挪回"是不够的 —— 实测用户正是从托盘把它叫回来，
                            // 那条路是壳的代码，我们拦不到。
                            if let Some(executable) = family.as_deref() {
                                if crate::client_window::restore_from_offscreen(executable) {
                                    timeline.mark("position-restored", "the window is back where it belongs");
                                }
                            }
                            // 窗口已经不在屏上了：那层冻结画面留着只会挡住用户。
                            if cover.take().is_some() {
                                timeline.mark("cover-removed", "the window is off screen: the frozen frame is no longer needed");
                            }
                            log::info!(
                                "harness shell window hidden: windows=1 source=tick tick={}ms",
                                SHELL_HIDE_TICK.as_millis()
                            );
                            timeline.mark(
                                "hide-landed",
                                format!("windows=1 sweeps={sweeps} via=tick"),
                            );
                            timeline.mark(
                                "visible-frame",
                                format!(
                                    "{}ms (== one tick = {}ms)",
                                    visible_frame.unwrap_or(0),
                                    SHELL_HIDE_TICK.as_millis()
                                ),
                            );
                        } else {
                            // 这个句柄不再是"可以藏的那个界面窗口"（换了属主、有了属主、或类名是
                            // 弹出物）。丢掉它，让下一次枚举重新认一个 —— 否则会在一个我们永远
                            // 不该碰的窗口上每秒空转一千次。
                            watched = None;
                            timeline.mark(
                                "watched-window-refused",
                                "the watched handle is no longer a hideable interface window",
                            );
                        }
                    }
                }
                // 已经藏到过：只等结算，或者等它再出现（那就归用户，规则 2）。
                (Some(hidden_since), _) => {
                    if hide_settled(Some(hidden_since), now) {
                        timeline.mark(
                            "settled",
                            format!("hidden_for={}ms", now.duration_since(hidden_since).as_millis()),
                        );
                        return (confirmed_at.is_some(), true, visible_frame);
                    }
                    let reappeared = watched.is_some_and(crate::client_window::window_is_visible) || on_screen;
                    if reappeared {
                        timeline.mark(
                            "reappeared",
                            "the window came back after the first hide: the user's show wins",
                        );
                        return (confirmed_at.is_some(), false, visible_frame);
                    }
                }
                (None, None) => {}
            }
        }

        // 出上下文的事件钩子靠这个消息队列投递：不抽消息，回调永远不会来（而日志里看上去会像
        // "钩子装了却没反应"）。它必须在这个线程做 —— 钩子就是在这一步注册的。
        if hide_window && show_watch.is_some() {
            crate::client_window::pump_show_events();
            if hidden_at.is_none() {
                if let Some((seen_at, hidden_by_event)) = crate::client_window::take_show_event() {
                    timeline.mark(
                        "show-event",
                        format!("the shell asked to show its window, {}ms into the watch",
                            seen_at.duration_since(started_at).as_millis()),
                    );
                    if hidden_by_event {
                        if let Some(handle) = watched {
                            if let Some(rect) = crate::client_window::window_rect(handle) {
                                timeline.mark(
                                    "window-visible-at",
                                    format!("rect=({},{})-({},{}) via=show-event", rect.0, rect.1, rect.2, rect.3),
                                );
                            }
                        }
                        visible_at = Some(seen_at);
                        hidden_at = Some(std::time::Instant::now());
                        visible_frame = visible_frame_ms(visible_at, hidden_at).or(visible_frame);
                        hidden_windows += 1;
                        // 位置挪回去：屏幕外那个位置只在"它第一次显示"的那一瞬间有用，此刻窗口已经隐藏，
                        // 所以这次位移谁也看不见；而此后无论谁把它叫出来（壳自己的托盘、我们的展示路径、AUMID
                        // 激活）都会在正确的位置。只靠"展示路径挪回"是不够的 —— 实测用户正是从托盘把它叫回来，
                        // 那条路是壳的代码，我们拦不到。
                        if let Some(executable) = family.as_deref() {
                            if crate::client_window::restore_from_offscreen(executable) {
                                timeline.mark("position-restored", "the window is back where it belongs");
                            }
                        }
                        log::info!("harness shell window hidden: windows=1 source=show-event");
                        timeline.mark("hide-landed", format!("windows=1 sweeps={sweeps} via=show-event"));
                        timeline.mark(
                            "visible-frame",
                            format!("{}ms (measured from the show event)", visible_frame.unwrap_or(0)),
                        );
                    } else {
                        timeline.mark(
                            "show-event-refused",
                            "the window that asked to show is no longer a hideable interface window",
                        );
                    }
                }
            }
        }

        let clock_active = hide_window && hidden_at.is_none() && bridge_ready_at.is_none() && now < clock_horizon;
        std::thread::sleep(hide_poll_interval(HidePoll {
            show_imminent: hide_window
                && (hidden_at.is_some()
                    || bridge_ready_at.is_some()
                    || confirmed_at.is_some()
                    || watched.is_some()),
            within_episode: hide_window && now < episode_horizon,
            clock_active,
        }));
    }

    let hidden = hidden_at.is_some();
    if hide_window && !hidden {
        log::warn!(
            "harness shell window was never hideable: port={port} listening={} family={:?}",
            confirmed_at.is_some(),
            family
        );
    }
    timeline.mark(
        "episode-over",
        format!("hidden={hidden} sweeps={sweeps} hidden_windows={hidden_windows}"),
    );
    (confirmed_at.is_some(), hidden, visible_frame)
}

/// Whether the hide has been *holding*: something was put out of sight, and nothing of
/// that executable has been on screen since.
///
/// 这是"这次隐藏做完了"的判据，所以它单独成了一个纯函数。判据必须是这样而不是"藏成功过一次"：
/// 壳自己那条显示窗口的路实测 540ms 就能把窗口拿回来，所以"曾经藏住"不构成答案，只有"藏住之后
/// 没人再把它显示出来"才是。`None` 表示一次都没藏到过（窗口还没画出来，或它自己就是以隐藏状态
/// 启动的），那时无论过了多久都不算完成。
///
/// 静默期**只测一次**（第一次藏住之后），因为这一场竞速只有一次对抗：再现的窗口一律归用户，见
/// [`wait_for_shell`] 的规则 2 —— 老做法是每次再现都重新计时，于是用户每次把窗口拿出来都让循环
/// 多活一会儿，那正是用户报告的那个故障。
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
    // 「用户要这个窗口」这件事必须在**动手之前**落款，而且要在启动之前 —— 这次调用很可能是在
    // 后台启动正把窗口按着的时候来的（开机自启刚起来、用户按了「打开」）。那场隐藏是毫秒级的，
    // 晚一步就是一次多余的对抗；落款之后它在毫秒内停产，此后这一场也不会再藏（见
    // [`wait_for_shell`] 的规则 3）。
    if let Some(path) = executable.as_deref() {
        note_user_wants_shell(path);
    }
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
            let launch = run_launch(plan, managed, LaunchTrigger::Manual);
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

    /// 施工文档 §7 把三种主体都收成"跑它的 CLI"，所以这条解析是所有主体共用的一步。
    ///
    /// 三件事必须钉住，否则错法都是安静的：壳的路径**从安装位置推**（不是存的，也不是从
    /// AUMID 猜的）、两种拒绝**分得开**（缺可执行文件 vs 缺启动器，调用方的退路不一样）、
    /// 源码树**不查启动器**（它自己就是 CLI）。
    #[test]
    fn a_shells_cli_is_derived_from_its_install_and_never_stored() {
        let executable = r"D:\Family\dsh-official\DeepSeek Harness.exe";
        let bundled = r"D:\Family\dsh-official\resources\runtime\cli\bin\dsh.cmd";
        let found = |path: &Path| path == Path::new(bundled);
        assert_eq!(
            plan_background_cli(OFFICIAL_ID, Some(executable), &found),
            Ok(BackgroundCli::Launch(PathBuf::from(bundled)))
        );

        // 换一个安装目录，答案跟着走：这正是"不存路径"要保住的性质（客户端重装到别处时，
        // 存储里那份 id 一个字都不该改）。
        let elsewhere = r"E:\Apps\DSH\DeepSeek Harness.exe";
        let bundled_elsewhere = r"E:\Apps\DSH\resources\runtime\cli\bin\dsh.cmd";
        let found_elsewhere = |path: &Path| path == Path::new(bundled_elsewhere);
        assert_eq!(
            plan_background_cli(OFFICIAL_ID, Some(elsewhere), &found_elsewhere),
            Ok(BackgroundCli::Launch(PathBuf::from(bundled_elsewhere)))
        );

        // 反过来也要成立：问的是"这份安装里那个启动器在不在"，不是"某个固定路径在不在"。
        assert_eq!(
            plan_background_cli(OFFICIAL_ID, Some(elsewhere), &found),
            Err("missing-launcher")
        );
    }

    #[test]
    fn a_shell_without_a_measured_executable_refuses_instead_of_guessing() {
        // AUMID 换不出路径。没有扫描到的可执行文件时只能拒绝，让调用方退回 AUMID 激活；
        // 拼一个"看起来对"的目录，会在用户机器上开出一个没人验证过的路径。
        assert_eq!(plan_background_cli(OFFICIAL_ID, None, &|_| true), Err("missing-executable"));
        assert_eq!(plan_background_cli(OFFICIAL_ID, Some("   "), &|_| true), Err("missing-executable"));
        // 只有文件名、没有目录：没有安装位置可挂，同样拒绝（否则相对路径会落到当前目录）。
        assert_eq!(
            plan_background_cli(OFFICIAL_ID, Some("DeepSeek Harness.exe"), &|_| true),
            Err("missing-executable")
        );
    }

    #[test]
    fn a_missing_launcher_is_a_different_refusal_than_a_missing_executable() {
        // 这两个错误码不能合成一个：缺可执行文件是"这个主体没有可用的安装位置"，缺启动器是
        // "安装位置有，但那份 CLI 不在"。客户端的退路（AUMID 激活）只对后者有意义。
        assert_eq!(
            plan_background_cli(OFFICIAL_ID, Some(r"D:\gone\DeepSeek Harness.exe"), &|_| false),
            Err("missing-launcher")
        );
    }

    #[test]
    fn an_installed_cli_is_its_own_launcher() {
        let launcher = r"C:\Users\someone\AppData\Roaming\npm\dsh.cmd";
        let id = format!("{CLI_ID_PREFIX}{launcher}");
        let found = |path: &Path| path == Path::new(launcher);
        assert_eq!(
            plan_background_cli(&id, None, &found),
            Ok(BackgroundCli::Launch(PathBuf::from(launcher)))
        );
        // 两端空白是存储格式的噪音，不是路径的一部分。
        let padded = format!("  {CLI_ID_PREFIX}  {launcher}  ");
        assert_eq!(
            plan_background_cli(&padded, None, &found),
            Ok(BackgroundCli::Launch(PathBuf::from(launcher)))
        );
        // 空启动器与已卸载的 CLI 是两件事：前者是坏 id，后者是"这个主体现在起不来"。
        assert_eq!(plan_background_cli("cli:", None, &|_| true), Err("unknown-target"));
        assert_eq!(plan_background_cli("cli:   ", None, &|_| true), Err("unknown-target"));
        assert_eq!(plan_background_cli(&id, None, &|_| false), Err("missing-launcher"));
    }

    /// 这台机器上的真话：官方客户端自带的 CLI 到底在不在那个相对位置上。
    ///
    /// 默认忽略（要读本机安装目录）。跑法：
    /// `cargo test --lib -- --ignored --nocapture this_machine_resolves_the_bundled_cli`。
    /// 它只读路径、不启动任何东西 —— 与上面那条 `this_machine_*` 同一条规矩。
    #[test]
    #[ignore = "reads this machine's installed client; starts nothing"]
    fn this_machine_resolves_the_bundled_cli() {
        let aumid = "com.deepseek.dsh";
        let id = format!("{SHELL_ID_PREFIX}{aumid}");
        // 两条来源分开打：记录里那份可能比这一版构建旧（本机实测就是），而"重扫一次能不能
        // 解析成功"才是这条路可不可用的真话。差在这两者之间，是**重扫**要补的，不是推导错。
        let recorded = recorded_shell_executable(aumid);
        println!("recorded executable: {recorded:?}");
        println!(
            "plan from the record: {:?}",
            plan_background_cli(&id, recorded.as_deref(), &|path| path.exists())
        );

        let scan = crate::harness_targets::scan_harness_targets_blocking(None, false);
        let live = scan
            .targets
            .iter()
            .find(|target| target.id.eq_ignore_ascii_case(&id))
            .and_then(|target| target.executable.clone());
        println!("scanned executable: {live:?}");
        println!(
            "plan from a live scan: {:?}",
            plan_background_cli(&id, live.as_deref(), &|path| path.exists())
        );
    }

    #[test]
    fn a_checkout_is_already_its_own_cli() {
        // 源码树的 CLI 就在树里（托管链跑的是它的 `apps/cli`），所以这里没有"启动器"要解析，
        // 也就**不查**任何路径：树本身是不是成立，由托管链按根的形状自己去验。
        assert_eq!(plan_background_cli(CHECKOUT, None, &|_| false), Ok(BackgroundCli::TreeChain));
        assert_eq!(
            plan_background_cli(CHECKOUT, Some(r"D:\ignored\DeepSeek Harness.exe"), &|_| false),
            Ok(BackgroundCli::TreeChain)
        );
        assert_eq!(plan_background_cli("   ", None, &|_| false), Ok(BackgroundCli::TreeChain));
    }

    /// 后台启动的分岔（施工文档 §7.6 修正版）：壳走它自带的 CLI，但**绑壁纸自己的端口**。
    #[test]
    fn a_shells_background_host_binds_the_wallpapers_own_port() {
        let launcher = Path::new(r"D:\Family\dsh-official\resources\runtime\cli\bin\dsh.cmd");
        let plan = plan_background_launch(OFFICIAL_ID, "web", &[], SLIDER, Some(launcher)).expect("host plan");
        match plan {
            LaunchPlan::SubjectHost {
                host_id,
                subject_id,
                kind,
                launcher: planned,
                profile,
                args,
                subject_port,
                host_port,
            } => {
                // 记录用的是 **host:<主体>**：它归我们管；而 `shell:<aumid>` 永远不归我们管，
                // 这两件事必须分得开，否则"收掉自己的宿主"会变成"杀掉用户的客户端"。
                assert_eq!(host_id, "host:shell:com.deepseek.dsh");
                assert_eq!(subject_id, OFFICIAL_ID);
                assert_eq!(kind, HarnessTargetKind::EmbeddedShell);
                assert_eq!(planned, launcher.to_string_lossy());
                // 只有 `web` 能提供 HTTP 且能装桥；`desktop` 档案两边的 CLI 都会拒绝。
                assert_eq!(profile, "web");
                // 客户端自己的端口只用来问"它在不在跑"。
                assert_eq!(subject_port, Some(19387));
                // 我们绑的是**壁纸自己的**端口。占着 19387 会让用户从开始菜单直接打开壳时
                // 引导失败并弹错误框（2026-09-30 实测的 `EADDRINUSE 127.0.0.1:19387`）。
                assert_eq!(host_port, crate::harness_targets::WALLPAPER_HOST_PORT);
                assert_eq!(args, argv(&["--port", "3099"]));
            }
            other => panic!("expected a subject host plan, got {other:?}"),
        }
    }

    /// 壳那条路上端口是**契约**，不是用户的偏好。
    #[test]
    fn a_launch_arg_port_cannot_move_the_shell_host() {
        let launcher = Path::new(r"D:\Family\dsh-official\resources\runtime\cli\bin\dsh.cmd");
        let declared = argv(&["--port", "4000", "--verbose"]);
        let plan = plan_background_launch(OFFICIAL_ID, "web", &declared, AUTO, Some(launcher)).expect("host plan");
        match plan {
            LaunchPlan::SubjectHost { args, host_port, .. } => {
                assert_eq!(host_port, crate::harness_targets::WALLPAPER_HOST_PORT);
                // 用户写的端口被丢掉、别的词照常送达：探测表按主体算端口，读不到用户的参数，
                // 所以让参数改掉我们绑在哪儿，等于让表与实际对不上。
                assert_eq!(args, argv(&["--verbose", "--port", "3099"]));
            }
            other => panic!("expected a subject host plan, got {other:?}"),
        }
        // 两种写法都要认，而且 `--port` 后面那个值必须一起走 —— 留下孤零零的 `4000` 会被
        // 启动器当成一个它不认识的词。
        assert_eq!(without_port_flag(&argv(&["--port=4000", "a"])), argv(&["a"]));
        assert_eq!(without_port_flag(&argv(&["--port"])), Vec::<String>::new());
        assert_eq!(without_port_flag(&argv(&["a", "--port", "4000", "b"])), argv(&["a", "b"]));
    }

    #[test]
    fn a_shell_without_a_resolvable_cli_falls_back_to_activating_the_client() {
        // 解析不出来（记录旧、CLI 被删、客户端换过地方）时退回原来的路：背景启动照旧把窗口
        // 留在屏幕外，而不是什么都不起。
        let automatic = plan_background_launch(OFFICIAL_ID, "desktop", &[], AUTO, None).expect("shell plan");
        assert!(matches!(automatic, LaunchPlan::Shell { hide_window: true, .. }));
        let slider = plan_background_launch(OFFICIAL_ID, "desktop", &[], SLIDER, None).expect("shell plan");
        assert_eq!(slider, automatic);
    }

    #[test]
    fn other_subject_kinds_are_untouched_by_the_host_route() {
        // 已安装 CLI 与源码树本来就是"跑自己的 CLI"，这条岔路与它们无关：即便递进来一个启动器，
        // 计划也不该变成"用壳的方式跑它"。
        let launcher = Path::new(r"D:\Family\dsh-official\resources\runtime\cli\bin\dsh.cmd");
        let cli = format!("{CLI_ID_PREFIX}C:\\Users\\u\\AppData\\Roaming\\npm\\dsh.cmd");
        assert!(matches!(
            plan_background_launch(&cli, "web", &[], SLIDER, Some(launcher)),
            Ok(LaunchPlan::InstalledCli { .. })
        ));
        assert!(matches!(
            plan_background_launch(CHECKOUT, " web ", &[], SLIDER, Some(launcher)),
            Ok(LaunchPlan::Checkout { .. })
        ));
    }

    /// "记录可能比这一版构建旧"的处理：先重扫一次，而且**只扫一次**。
    #[test]
    fn a_stale_record_is_rescanned_once_before_refusing() {
        let bundled = r"D:\Family\dsh-official\resources\runtime\cli\bin\dsh.cmd";
        let found = |path: &Path| path == Path::new(bundled);
        let exe = r"D:\Family\dsh-official\DeepSeek Harness.exe";

        // 记录里就有可执行文件 ⇒ 一次都不扫（重扫本机实测约 2.8 秒，不该花在能直接回答的时候）。
        let scans = std::cell::Cell::new(0);
        let resolved = resolve_background_cli(
            OFFICIAL_ID,
            Some(exe.to_string()),
            &|| { scans.set(scans.get() + 1); None },
            &found,
        );
        assert!(matches!(resolved, Ok(BackgroundCli::Launch(_))));
        assert_eq!(scans.get(), 0, "记录够用时不该扫描");

        // 记录里没有 ⇒ 扫一次，并用新答案。
        let scans = std::cell::Cell::new(0);
        let resolved = resolve_background_cli(
            OFFICIAL_ID,
            None,
            &|| { scans.set(scans.get() + 1); Some(exe.to_string()) },
            &found,
        );
        assert!(matches!(resolved, Ok(BackgroundCli::Launch(_))));
        assert_eq!(scans.get(), 1);

        // 重扫没有带来不同答案 ⇒ 沿用第一次的拒绝，而不是拿同一个输入再问一遍。
        let scans = std::cell::Cell::new(0);
        assert_eq!(
            resolve_background_cli(OFFICIAL_ID, None, &|| { scans.set(scans.get() + 1); None }, &found),
            Err("missing-executable")
        );
        assert_eq!(scans.get(), 1);

        // 客户端换过地方是同一个形状：旧记录指向一条不存在的路径，新记录给出新安装位置。
        let moved = |path: &Path| path == Path::new(r"E:\Apps\DSH\resources\runtime\cli\bin\dsh.cmd");
        assert_eq!(
            resolve_background_cli(
                OFFICIAL_ID,
                Some(r"D:\old\DeepSeek Harness.exe".to_string()),
                &|| Some(r"E:\Apps\DSH\DeepSeek Harness.exe".to_string()),
                &moved,
            ),
            Ok(BackgroundCli::Launch(PathBuf::from(r"E:\Apps\DSH\resources\runtime\cli\bin\dsh.cmd")))
        );
    }

    /// 落盘记录那一半挑出来的必须是**我们自己的**实例，官壳除外。
    ///
    /// 这条判据曾经写反（`!is_managed_by_us`），而两个消费者各自又滤了一遍官壳，于是它一声不响：
    /// 重启过的壁纸既列不出、也停不掉自己启动的宿主，而"停止全部"还会静默报成功。
    #[test]
    fn the_record_only_half_lists_ours_and_never_the_client() {
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        let host = "host:shell:com.deepseek.dsh";
        let mut cylinder = ManagedChildren::default();
        cylinder.remember(child(CHECKOUT, CHECKOUT, 111, Some(1)));
        cylinder.remember(child(cli, cli, 222, Some(1)));
        cylinder.remember(child(host, host, 333, Some(1)));
        cylinder.remember(child(OFFICIAL_ID, OFFICIAL_ID, 444, Some(1)));

        let listed = our_live_instances(&cylinder, &|_| true);
        let ours: Vec<&str> = listed.iter().map(|entry| entry.subject_id.as_str()).collect();
        assert!(ours.contains(&CHECKOUT) && ours.contains(&cli) && ours.contains(&host));
        // 官壳那条记录混在这份文件里是正常的（它是给"门票"用的），但它一次都不该出现在"我们的"里。
        assert!(!ours.contains(&OFFICIAL_ID), "the client is never ours: {ours:?}");
        assert_eq!(ours.len(), 3);

        // 存活判定同样要生效：报"已经不是同一个进程"的一条都不留。
        assert!(our_live_instances(&cylinder, &|_| false).is_empty());
    }

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
    ///
    /// 静默期**只测一次**：这一场竞速只有一次对抗，再现的窗口一律归用户（见 [`wait_for_shell`]
    /// 的规则 2）。老做法每次再现都重新计时，于是用户每次把窗口拿出来都让循环多活一会儿 ——
    /// 那正是用户报告"窗口刚出来就被按回去，而且是十几秒"的成因。
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
        // 静默期比这一场的上限短：否则"结算"永远不会先于"到点"发生。
        assert!(SHELL_HIDE_SETTLE < SHELL_HIDE_EPISODE);
    }

    /// 三档轮询各自的处境，以及它们之间的顺序 —— 这些常数就是"一帧能被看见多久"。
    ///
    /// 顺序不是凑出来的：tick 最密（一次 `IsWindowVisible`），时钟次之（一次回环 GET，实测中位
    /// 0.8 毫秒），枚举最贵（一张进程快照 + 一次 `EnumWindows`），而"什么都没有"的那一档可以慢
    /// 到 250 毫秒。写反任何一对，要么白烧 CPU，要么把帧拉长。
    #[test]
    fn the_poll_cadence_is_ordered_by_what_each_wake_up_costs() {
        let racing = HidePoll {
            show_imminent: true,
            within_episode: true,
            clock_active: true,
        };
        assert_eq!(hide_poll_interval(racing), SHELL_HIDE_TICK);
        // 时钟走着的阶段（宿主还没就绪）：单飞地问 /status，密到个位数毫秒。
        let clocking = HidePoll {
            show_imminent: false,
            within_episode: true,
            clock_active: true,
        };
        assert_eq!(hide_poll_interval(clocking), SHELL_HOST_POLL);
        // 既没有"显示即将发生"也没有时钟：慢档，250 毫秒。
        let idle = HidePoll {
            show_imminent: false,
            within_episode: true,
            clock_active: false,
        };
        assert_eq!(hide_poll_interval(idle), SHELL_START_POLL);
        // 竞速过期（这一场到点了）：即使显示信号还挂着，也退回慢档 —— 上限就是上限。
        let expired = HidePoll {
            show_imminent: true,
            within_episode: false,
            clock_active: true,
        };
        assert_eq!(hide_poll_interval(expired), SHELL_HOST_POLL);
        // 数字上的顺序：tick < 时钟 ≤ 枚举 ≤ 慢档，而且 tick 是个位数毫秒（用户要求的那一档）。
        assert!(SHELL_HIDE_TICK < SHELL_HOST_POLL);
        assert!(SHELL_HOST_POLL < SHELL_START_POLL);
        assert!(SHELL_HIDE_SWEEP_RACE < SHELL_HIDE_SWEEP_GUARD);
        assert!(SHELL_HIDE_SWEEP_GUARD < SHELL_HIDE_SWEEP_BOOT);
        assert!(SHELL_HIDE_SWEEP_BOOT <= SHELL_START_POLL);
        assert!(SHELL_HIDE_TICK.as_millis() < 10);
        assert!(SHELL_HOST_POLL.as_millis() < 10);
        // 密集枚举的寿命短于这一场本身，也短于"启动被确认"那条钟：它只覆盖"宿主就绪到显示"
        // 那几秒。
        assert!(SHELL_HIDE_RACE_WINDOW < SHELL_HIDE_EPISODE);
        assert!(SHELL_HIDE_RACE_WINDOW < SHELL_START_TIMEOUT);
        // 枚举三档：枚举是检测器时最密（有寿命），有 tick 盯着时守护档，什么都没有时最稀。
        assert_eq!(
            hide_sweep_interval(SweepPoll {
                show_imminent: true,
                sweep_is_the_detector: true,
                within_race_window: true
            }),
            SHELL_HIDE_SWEEP_RACE
        );
        assert_eq!(
            hide_sweep_interval(SweepPoll {
                show_imminent: true,
                sweep_is_the_detector: true,
                within_race_window: false
            }),
            SHELL_HIDE_SWEEP_GUARD
        );
        assert_eq!(
            hide_sweep_interval(SweepPoll {
                show_imminent: true,
                sweep_is_the_detector: false,
                within_race_window: true
            }),
            SHELL_HIDE_SWEEP_GUARD
        );
        assert_eq!(
            hide_sweep_interval(SweepPoll {
                show_imminent: false,
                sweep_is_the_detector: false,
                within_race_window: false
            }),
            SHELL_HIDE_SWEEP_BOOT
        );
    }

    /// 这一场什么时候结束 —— 两条钟，各自服务一个问题。
    #[test]
    fn the_episode_ends_at_the_first_of_two_horizons() {
        // 还没到任何一条钟：继续。
        assert!(hide_episode_continues(true, false, false, false));
        assert!(hide_episode_continues(true, true, false, false));
        // 启动确认那条钟到了、而端口应答过：隐藏还能继续（上限是另一条钟）。
        assert!(hide_episode_continues(true, true, true, false));
        // 启动那条钟到了、而端口**没**应答过：不再等 —— "起不来"这件事该如实报告了，
        // 不该再拖 15 秒。
        assert!(!hide_episode_continues(true, false, true, false));
        // 手动触发（不藏窗口）到点就结束。
        assert!(!hide_episode_continues(false, true, true, false));
        // 上限到点：无论别的条件如何都结束。
        assert!(!hide_episode_continues(true, true, false, true));
        assert!(!hide_episode_continues(true, true, true, true));
        // 上限比启动确认那条钟长 —— 否则"冷启动远不止十几秒"就永远盖不到。
        assert!(SHELL_START_TIMEOUT < SHELL_HIDE_EPISODE);
        // 那一台时钟自己也有界。
        assert!(SHELL_HOST_POLL_GIVE_UP < SHELL_HIDE_EPISODE);
    }

    /// 可见帧 = 从"看见它在屏幕上"到"藏住"。
    ///
    /// 它是**上界**：看见的那一刻本身有一次轮询的延迟，所以真实帧只会更短。缺任何一头都不算数
    /// —— 没看见过就说不出帧有多长，没藏住就没有帧的终点。
    #[test]
    fn the_visible_frame_is_measured_between_seeing_it_and_hiding_it() {
        let seen = std::time::Instant::now();
        let hidden = seen + std::time::Duration::from_millis(3);
        assert_eq!(visible_frame_ms(Some(seen), Some(hidden)), Some(3));
        assert_eq!(
            visible_frame_ms(Some(seen), Some(seen + std::time::Duration::from_millis(1))),
            Some(1)
        );
        // 一头缺了就没有数字可说，绝不猜一个。
        assert_eq!(visible_frame_ms(None, Some(hidden)), None);
        assert_eq!(visible_frame_ms(Some(seen), None), None);
        assert_eq!(visible_frame_ms(None, None), None);
    }

    /// 「用户要这个窗口」这一款：谁来落、谁来清、以及循环只读它。
    ///
    /// 循环自己**不会**落这一款（它只有 `wants` 这个读法），落款的两条路都是人的意图：揭示路径
    /// 的命令，和"看见托盘菜单"这一个人手的证据。清的只有一条路：下一次后台启动。
    #[test]
    fn the_user_intent_is_set_by_people_and_cleared_by_the_next_background_launch() {
        const EXE: &str = r"D:\Family\dsh-official\DeepSeek Harness.exe";
        let mut intent = RevealIntent::default();
        // 谁都没要过：不拦。
        assert!(!intent.wants(EXE));
        // 揭示路径落款。
        intent.note_user_request(EXE);
        assert!(intent.wants(EXE));
        // 拼法不同但同一个文件：款照样认（快捷方式、进程镜像、命令行的三种拼法）。
        assert!(intent.wants(r"\\?\D:\Family\dsh-official\DeepSeek Harness.exe"));
        assert!(intent.wants(r"d:/family/DSH-OFFICIAL/DeepSeek Harness.exe"));
        // 别的程序不认：这一款只对落款的那个壳有效。
        assert!(!intent.wants(r"C:\other\DeepSeek Harness.exe"));
        assert!(!intent.wants(""));
        // 下一次后台启动清掉它 —— 新的那次启动有自己的诉求。
        intent.clear();
        assert!(!intent.wants(EXE));
        // 看见人手（托盘菜单）落的是同一款，来路不同而已。
        intent.observe_human_interaction(EXE);
        assert!(intent.wants(EXE));
        // 空路径不落款：一个空字符串会让"任何人都匹配"成为可能。
        let mut empty = RevealIntent::default();
        empty.note_user_request("   ");
        assert!(!empty.wants(EXE));
        assert!(!empty.wants("   "));
    }

    /// 全世界的线格式：Bridge 的 `/status` 在这台机器上实测就是这一段（`Transfer-Encoding: chunked`）。
    ///
    /// 只认一个字段，而且**必须**是 `bridge-ready`：`bridge-loading` 是"还在装配"，把它当成
    /// 就绪等于把竞速的时钟拨早了 —— 那时壳的窗口还早着呢。
    #[test]
    fn the_host_is_only_ready_when_the_bridge_says_bridge_ready() {
        // 实测原文（去掉令牌与无关头，`128` 是分块长度）。
        const READY: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/json; charset=utf-8\r\ncache-control: no-store\r\nVary: Accept-Encoding\r\nDate: Mon, 28 Sep 2026 19:51:36 GMT\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n128\r\n{\"bridgeVersion\":\"0.1.3\",\"bridgeBuild\":\"dev\",\"protocolVersion\":1,\"dsh\":\"online\",\"authoredAgainst\":\"^0.1.0-rc.5 || ^0.2.0-rc.1\",\"state\":\"bridge-ready\",\"reasonCode\":\"ready\",\"capabilities\":[\"status\",\"control\",\"sessions\",\"history\",\"sse\",\"cancel\",\"approval-handoff\",\"resume\"],\"authentication\":\"ready\"}\r\n0\r\n\r\n";
        assert!(harness_status_reported_ready(READY));
        // 还在装配：不是就绪。壳的窗口要等的是 `ready`，不是"路由挂上了"。
        let loading = READY.replace("bridge-ready", "bridge-loading");
        assert!(!harness_status_reported_ready(&loading));
        // 令牌还没就绪：也不是。
        assert!(!harness_status_reported_ready(
            &READY.replace("bridge-ready", "bridge-auth-unavailable")
        ));
        // 别的状态码、别的服务、空响应、截断的响应：全都不是"宿主已就绪"。
        assert!(!harness_status_reported_ready(""));
        assert!(!harness_status_reported_ready("HTTP/1.1 404 Not Found\r\n\r\n"));
        assert!(!harness_status_reported_ready(&READY.replace("200 OK", "500 Internal Server Error")));
        assert!(!harness_status_reported_ready("HTTP/1.1 200 OK\r\n\r\n<htm"));
        // 端口上答话的是别的 HTTP 服务：它答 200，但没有那个字段。
        assert!(!harness_status_reported_ready("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok"));
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

    /// 真机测量：这一场竞速里我们这一侧的两个数字 —— 发现延迟与按下延迟。
    ///
    /// 窗口由**本测试自己**创建：离屏坐标（`-4000,-4000`）、`WS_EX_TOOLWINDOW`（所以屏幕上、
    /// 任务栏和 Alt+Tab 里都没有它），而且显示与隐藏都由它**自己的线程**执行并各自打时间戳 ——
    /// 这样量到的是纯粹的"发现延迟"，没有跨线程 `ShowWindow` 的排队效应掺进来。认窗口的方式与
    /// 产品代码完全相同（`family_windows(本测试程序自己的路径)`），拍板隐藏用的也是产品那个
    /// `hide_window`。
    ///
    /// 打印三个数：一次全量枚举要多久、tick 认出"它被显示出来了"要多久、以及藏掉它要多久。
    /// 第一个数决定竞速前那段"找到那个还没显示的窗口"能不能便宜地反复做；第二个数是可见帧里
    /// 我们这一侧的那一半。
    ///
    /// `cargo test --lib -- --ignored --nocapture this_machine_measures_the_hide_race`
    #[test]
    #[ignore = "creates its own off-screen window to time the hide tick"]
    fn this_machine_measures_the_hide_race() {
        #[cfg(windows)]
        {
            use std::sync::mpsc;
            use std::time::{Duration, Instant};
            use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
                RegisterClassW, ShowWindow, TranslateMessage, MSG, PM_REMOVE, SW_HIDE, SW_SHOW,
                WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
            };

            /// 让窗口那条线程做时间敏感的那两件事，并各自把时刻带回来。
            enum Command {
                Show(mpsc::Sender<Instant>),
                Hide,
                Quit,
            }

            unsafe extern "system" fn wnd_proc(
                hwnd: HWND,
                message: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }

            fn report(label: &str, mut samples: Vec<Duration>) {
                samples.sort();
                let count = samples.len();
                println!(
                    "{label}: n={count} min={:?} median={:?} max={:?}",
                    samples[0],
                    samples[count / 2],
                    samples[count - 1]
                );
            }

            let (ready_tx, ready_rx) = mpsc::channel::<crate::client_window::WindowHandle>();
            let (command_tx, command_rx) = mpsc::channel::<Command>();
            let class_name: Vec<u16> = "dsh-wallpaper-hide-race-probe\0".encode_utf16().collect();
            let title: Vec<u16> = "DSH wallpaper hide race probe\0".encode_utf16().collect();
            let title_for_thread = title.clone();
            let class_for_thread = class_name.clone();
            std::thread::spawn(move || {
                let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.expect("module").0);
                let class = WNDCLASSW {
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: instance,
                    lpszClassName: windows::core::PCWSTR(class_for_thread.as_ptr()),
                    ..Default::default()
                };
                assert_ne!(unsafe { RegisterClassW(&class) }, 0, "注册窗口类");
                let hwnd = unsafe {
                    CreateWindowExW(
                        WS_EX_TOOLWINDOW,
                        windows::core::PCWSTR(class_for_thread.as_ptr()),
                        windows::core::PCWSTR(title_for_thread.as_ptr()),
                        WS_OVERLAPPEDWINDOW,
                        // 离屏：可见位是真的（`IsWindowVisible` 会回答"是"），但屏幕上什么都看不到。
                        -4000,
                        -4000,
                        320,
                        200,
                        None,
                        None,
                        Some(instance),
                        None,
                    )
                }
                .expect("探针窗口");
                let _ = ready_tx.send(crate::client_window::WindowHandle::from(hwnd, std::process::id()));
                loop {
                    // 泵消息：一个不泵消息的窗口线程会让跨线程的显示/隐藏变成异步排队，那正是
                    // 这次测量要避开的。
                    let mut message = MSG::default();
                    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                        unsafe {
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    match command_rx.recv_timeout(Duration::from_millis(2)) {
                        Ok(Command::Show(ack)) => {
                            let at = Instant::now();
                            unsafe {
                                let _ = ShowWindow(hwnd, SW_SHOW);
                            }
                            let _ = ack.send(at);
                        }
                        Ok(Command::Hide) => unsafe {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        },
                        Ok(Command::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            });
            let handle = ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("探针窗口就绪");

            let executable = std::env::current_exe()
                .expect("测试程序自己的路径")
                .to_string_lossy()
                .into_owned();
            // 这台机器上有多少个顶层窗口，以及光枚举一遍要多久：家族枚举的成本主要是它。
            {
                use windows::Win32::Foundation::LPARAM as WindowLparam;
                use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
                use windows::core::BOOL;
                static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                unsafe extern "system" fn count(_: HWND, _: WindowLparam) -> BOOL {
                    COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    BOOL(1)
                }
                let mut enumerations = Vec::new();
                for _ in 0..10 {
                    COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
                    let at = Instant::now();
                    unsafe {
                        let _ = EnumWindows(Some(count), LPARAM(0));
                    }
                    enumerations.push(at.elapsed());
                }
                println!(
                    "all top-level windows: n={}，raw EnumWindows median={:?}",
                    COUNT.load(std::sync::atomic::Ordering::Relaxed),
                    {
                        enumerations.sort();
                        enumerations[enumerations.len() / 2]
                    }
                );
            }
            // 这个循环每 1 毫秒做的那一件事：两次只读系统调用。它的代价决定 tick 能不能这么密。
            let mut checks = Vec::new();
            for _ in 0..200 {
                let at = Instant::now();
                let _ = crate::client_window::window_is_visible(handle);
                checks.push(at.elapsed());
            }
            report("tick check (一次 IsWindowVisible)", checks);
            // 家族枚举的成本在哪一半：进程快照，还是窗口枚举。
            let mut pid_scans = Vec::new();
            for _ in 0..10 {
                let at = Instant::now();
                let pids = crate::client_window::family_process_ids(&executable);
                pid_scans.push(at.elapsed());
                assert!(!pids.is_empty(), "本进程必须在这张表里");
            }
            report("family pid scan (Toolhelp + 路径确认)", pid_scans);
            let mut sweeps = Vec::new();
            for _ in 0..10 {
                let at = Instant::now();
                let windows = crate::client_window::family_windows(&executable);
                sweeps.push(at.elapsed());
                assert!(
                    windows.iter().any(|window| window.handle == handle),
                    "枚举必须认出本进程的这个窗口"
                );
            }
            report("family sweep (一次全量枚举)", sweeps);

            // 竞速：让它自己显示，看 tick 多久认出、`hide_window` 多久按下去。计时器分辨率抬到
            // 1 毫秒，与产品循环里那一段完全一样（否则 1 毫秒的 tick 在默认 15.6 毫秒的分辨率下
            // 根本兑现不了）。
            let _resolution = crate::client_window::TimerResolution::new();
            let mut reactions = Vec::new();
            let mut hides = Vec::new();
            for _ in 0..8 {
                command_tx.send(Command::Hide).expect("隐藏探针");
                std::thread::sleep(Duration::from_millis(30));
                assert!(
                    !crate::client_window::window_is_visible(handle),
                    "前置条件：它现在是隐藏的"
                );
                let (ack_tx, ack_rx) = mpsc::channel();
                command_tx.send(Command::Show(ack_tx)).expect("显示探针");
                let noticed = {
                    let deadline = Instant::now() + Duration::from_secs(2);
                    let mut at = None;
                    while at.is_none() && Instant::now() < deadline {
                        if crate::client_window::window_is_visible(handle) {
                            at = Some(Instant::now());
                        } else {
                            std::thread::sleep(SHELL_HIDE_TICK);
                        }
                    }
                    at.expect("tick 必须认出在屏幕上的窗口")
                };
                let shown_at = ack_rx.recv_timeout(Duration::from_secs(1)).expect("显示时刻");
                reactions.push(noticed.saturating_duration_since(shown_at));
                let at = Instant::now();
                assert!(
                    crate::client_window::hide_window(handle),
                    "hide_window 必须接受这个界面窗口"
                );
                hides.push(at.elapsed());
                assert!(
                    !crate::client_window::window_is_visible(handle),
                    "藏完必须不可见"
                );
            }
            let _ = command_tx.send(Command::Quit);
            report("tick notice (发现延迟)", reactions);
            report("hide (按下延迟)", hides);
        }
        #[cfg(not(windows))]
        println!("这一个测量只在 Windows 上有意义");
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
