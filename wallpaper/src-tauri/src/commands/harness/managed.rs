use crate::*;
use crate::commands::{require_background, require_settings, require_wallpaper_surface};
use super::discovery::{dsh_port_is_occupied, resolve_dsh_launcher};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::Mutex;

/// A DSH process is only "managed" when this instance spawned it and still
/// owns its `Child` handle. A listening port alone never proves ownership.
///
/// 一个实例一条：`instance_key`（主体 id + 启动参数）是键，所以"同一个源码目录的两个端口"
/// 是两条互不覆盖的记录。键由 `harness_launch::instance_key` 生成，与落盘记录用的是同一个函数，
/// 因此内存与磁盘永远指着同一个实例。
#[cfg(not(feature = "lite"))]
struct ManagedDshProcess {
    child: Child,
    instance_key: String,
    subject_id: String,
    root_path: String,
    profile: String,
    /// 启动这个实例时用的「启动参数」，原样留档：状态列表要能说出"这一行是哪一个"。
    args: Vec<String>,
    /// Where this child's own output was captured, when it could be.
    log_path: Option<PathBuf>,
}

#[cfg(not(feature = "lite"))]
#[derive(Default)]
pub(crate) struct ManagedDshState(Mutex<std::collections::BTreeMap<String, ManagedDshProcess>>);

/// Records the one automatic launch attempt this process is allowed to make.
///
/// The single-flight guarantee has to live here rather than in the renderer: a
/// React remount, a `settings-changed` broadcast, an unlock, HMR, or a second
/// WebView would each be a fresh caller, and only the process-wide state can
/// enforce "start DSH at most once per wallpaper launch".
#[cfg(not(feature = "lite"))]
#[derive(Default)]
pub(crate) struct ManagedDshAutostartState(Mutex<Option<ManagedDshAutostart>>);

#[cfg(not(feature = "lite"))]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedDshAutostart {
    /// Stable, non-sensitive outcome code; never an exception body.
    pub(super) outcome: String,
    /// Present only when this process actually spawned the child.
    pub(super) pid: Option<u32>,
    /// True when an external DSH already owned 3080 and was left alone.
    pub(super) external: bool,
}

#[cfg(not(feature = "lite"))]
impl ManagedDshAutostart {
    pub(super) fn new(outcome: &str) -> Self {
        Self { outcome: outcome.into(), pid: None, external: false }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(feature = "lite"))]
pub(super) struct ManagedDshInstance {
    /// 停止与下拉都用它定位：`主体 id` + 启动参数（见 `harness_launch::instance_key`）。
    pub(super) instance_key: String,
    pub(super) subject_id: String,
    /// 这个实例在哪服务，读不到时为 `None`（渲染层会把它显示成「端口未确认」，而不是 0）。
    pub(super) port: Option<u16>,
    pub(super) pid: u32,
    /// 只在**本进程启动**的实例上有：落盘记录里的那一格只有 pid 与端口。
    pub(super) root_path: Option<String>,
    pub(super) profile: Option<String>,
    pub(super) args: Vec<String>,
}

/// 本应用启动的每一个 DSH 实例，外加两个给"启动监督"用的汇总字段。
///
/// `instances` 是权威答案（用户要的是"本应用启动的全部实例"：并行实例功能的前提）。
/// `managed` / `running` 留着，是因为启动监督那条路（`harnessLaunchOutcome`）问的是另一个问题
/// ——"我这次启动的那个孩子还在不在"——它的判据在传了 `subject_id` 时就是那个主体的实例集合。
/// 两个字段都由同一份列表派生，所以不可能出现"列表有它、汇总说没有"这种自相矛盾。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(feature = "lite"))]
pub(crate) struct ManagedDshStatus {
    instances: Vec<ManagedDshInstance>,
    managed: bool,
    running: bool,
}

/// The one automatic-launch record, read without consuming it.
///
/// Read and write are separate helpers on purpose: a live `tauri::State` borrow of
/// the app handle cannot cross an `await`, and the start below has to be offloaded
/// to a blocking task.
#[cfg(not(feature = "lite"))]
pub(super) fn autostart_attempt(app: &tauri::AppHandle) -> Result<Option<ManagedDshAutostart>, String> {
    let state = app.state::<ManagedDshAutostartState>();
    let record = state
        .0
        .lock()
        .map_err(|_| "DSH 自动启动状态不可用".to_string())?;
    Ok(record.clone())
}

#[cfg(not(feature = "lite"))]
pub(super) fn record_autostart_attempt(
    app: &tauri::AppHandle,
    outcome: &ManagedDshAutostart,
) -> Result<(), String> {
    let state = app.state::<ManagedDshAutostartState>();
    let mut record = state
        .0
        .lock()
        .map_err(|_| "DSH 自动启动状态不可用".to_string())?;
    *record = Some(outcome.clone());
    Ok(())
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn launch_dsh(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, ManagedDshState>,
    root_path: String,
    profile: String,
    args: Option<Vec<String>>,
) -> Result<u32, String> {
    // The settings center configures this launch target, while the visible
    // route switch in the WorkerW wallpaper is the user-facing "start DSH"
    // action. Both declared surfaces may request a launch; process ownership
    // and all executable/profile validation remain native below.
    require_wallpaper_surface(&caller)?;
    let args = harness_launch::normalize_launch_args(args)?;
    spawn_managed_dsh(state.inner(), &root_path, &root_path, &profile, &args)
}

/// Launch DSH at most once per wallpaper process, for the
/// `autoStartWithWallpaper` setting.
///
/// The outcome is remembered even when it is a failure, so a bad path or a
/// missing Node install is reported once instead of being retried on every
/// remount, broadcast, or unlock. DSH is a resident service: the user's next
/// action is to fix the configuration in settings, not to watch the wallpaper
/// spawn processes in a loop.
///
/// 这里**不再**有"自定义启动命令需要用户确认"那一步：那个设置已经不在了。「启动参数」加的是
/// 参数，启动器永远是本 build 自己选的那个（`spawn_managed_dsh` 里决定的 node/pnpm），
/// 所以"无人值守时会不会执行用户随手填的一个程序"这个问题不再成立 —— 而它不成立，是因为
/// 那个入口没有了，不是因为这里的检查被放宽了。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn autostart_managed_dsh(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, ManagedDshState>,
    autostart: tauri::State<'_, ManagedDshAutostartState>,
    root_path: Option<String>,
    profile: String,
    args: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    let mut record = autostart
        .0
        .lock()
        .map_err(|_| "DSH 自动启动状态不可用".to_string())?;
    if let Some(previous) = record.as_ref() {
        // Already attempted in this process. Report the same outcome rather
        // than trying again.
        return Ok(serde_json::to_value(previous).unwrap_or_else(|_| serde_json::json!({ "outcome": "already-attempted" })));
    }

    let args = match harness_launch::normalize_launch_args(args) {
        Ok(args) => args,
        Err(error) => {
            let outcome = ManagedDshAutostart::new(&classify_dsh_launch_failure(&error));
            *record = Some(outcome.clone());
            return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
        }
    };

    let Some(root_path) = root_path.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) else {
        let outcome = ManagedDshAutostart::new("root-path-missing");
        *record = Some(outcome.clone());
        return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
    };

    // An external DSH on the instance's port belongs to the user. Do not start a
    // second one, do not take it over, and do not stop it: fall through to Bridge
    // probing. 端口从「启动参数」读，因为并行实例的第二、三个各在自己的端口上，而"占用检查"
    // 问的是**这一次要用的那个端口**。
    let port = harness_launch::instance_port(&args);
    if dsh_port_is_occupied(port) {
        let outcome = ManagedDshAutostart { outcome: "port-occupied-external".into(), pid: None, external: true };
        *record = Some(outcome.clone());
        return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
    }

    let outcome = match spawn_managed_dsh(state.inner(), &root_path, &root_path, &profile, &args) {
        Ok(pid) => ManagedDshAutostart { outcome: "started".into(), pid: Some(pid), external: false },
        Err(error) => {
            log::warn!("wallpaper DSH autostart failed: {error}");
            ManagedDshAutostart::new(&classify_dsh_launch_failure(&error))
        }
    };
    *record = Some(outcome.clone());
    Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null))
}

/// The only automatic-launch state a caller may observe later without
/// triggering another attempt.
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn managed_dsh_autostart_status(
    caller: tauri::WebviewWindow,
    autostart: tauri::State<'_, ManagedDshAutostartState>,
) -> Result<Option<serde_json::Value>, String> {
    require_wallpaper_surface(&caller)?;
    let record = autostart
        .0
        .lock()
        .map_err(|_| "DSH 自动启动状态不可用".to_string())?;
    Ok(record
        .as_ref()
        .and_then(|outcome| serde_json::to_value(outcome).ok()))
}

/// Map a launch failure to a stable code. The message may name a missing
/// executable or an invalid directory, which the settings view needs in order
/// to be actionable, but it must never carry a token or exception body.
#[cfg(not(feature = "lite"))]
pub(crate) fn classify_dsh_launch_failure(error: &str) -> String {
    if error.contains("根目录") {
        "root-path-invalid".into()
    } else if error.contains("Node") || error.contains("pnpm") {
        "launcher-missing".into()
    } else if error.contains("profile") {
        "profile-invalid".into()
    } else if error.contains("端口已被") {
        // 端口被占用的**文案**里带着那一个端口号（3080 只是一个可能值），所以判定读的是"占用"
        // 这件事本身，而不是某一个数字 —— 否则第二个实例换到 3081 之后，同一个失败会被报成
        // "进程启动失败"，而那是最没法照做的一句话。
        "port-occupied-external".into()
    } else if error.contains("启动参数") {
        "launch-args-invalid".into()
    } else {
        "spawn-failed".into()
    }
}

/// Where a DSH started by the wallpaper keeps its own output.
///
/// DSH reports every startup failure on stderr, and a GUI-subsystem parent has no usable standard
/// streams. Inheriting those handles threw the only evidence away, which left the wallpaper able to
/// say no more than "DSH 启动后很快退出" and to point at a log that was never written. Keep the child
/// hidden, but put its own words on disk beside the wallpaper's log.
///
/// **一个实例一个文件**。并行实例功能之前这里只有一个 `managed-dsh.log`：两个实例同时写会把
/// 各自的输出交织在一起，而"孩子退出后它的最后几句话是什么"正是这个文件唯一的用途 —— 混起来的
/// 输出会让那段诊断指向**另一个实例**的原因。
///
/// 文件名由实例键的哈希得出，不是把键直接拼进去：键里有绝对路径与参数（含 `\` `:` `\u{1f}`），
/// 直接当文件名不合法，而"清洗一下"很容易留下两个键撞成同一个名字的余地。
#[cfg(not(feature = "lite"))]
fn managed_dsh_log_path(instance_key: &str) -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| {
        root.join("com.dsh.wallpaper")
            .join("logs")
            .join(format!("managed-dsh-{:016x}.log", stable_hash(instance_key)))
    })
}

/// FNV-1a，64 位。**只用来给日志文件起名**：它不是身份，身份是记录里的 pid + 创建时间。
///
/// 用它而不是 `DefaultHasher`，因为后者的输出在不同 Rust 版本之间没有稳定保证，而这个文件名会
/// 跨版本被写、被读（升级后同一个实例应当落在同一个文件里）。
#[cfg(not(feature = "lite"))]
fn stable_hash(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The last `lines` lines of a managed DSH's captured output, or why there are
/// none to show. Never panics on a missing or unreadable log.
#[cfg(not(feature = "lite"))]
fn managed_dsh_log_tail(path: Option<&PathBuf>, lines: usize) -> String {
    let Some(path) = path else {
        return "（本次未捕获 DSH 输出）".into();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return format!("（无法读取 {}）", path.display());
    };
    let collected = text.lines().collect::<Vec<&str>>();
    let start = collected.len().saturating_sub(lines);
    collected[start..].join("\n")
}

/// Shared, validated launch path for the manual button and the autostart
/// setting. Both callers get identical validation, ownership tracking and
/// per-instance single-instance behaviour because there is only one
/// implementation.
///
/// Reachable from `harness_launch`, which is how a chosen execution subject ends
/// up here: the shim decides *which* class to start, and every checkout still
/// starts through this one chain.
///
/// **`subject_id` 与 `root_path` 分开**，虽然源码目录的 id 就是它的路径、两者一字不差：它们回答
/// 的是两个不同的问题（"这是谁"与"从哪跑"），而实例键用的是前者。已安装的 CLI 走另一条链，
/// 那里的 id 与路径本来就不一样，这个签名让两条链的记法保持一致。
#[cfg(not(feature = "lite"))]
pub(crate) fn spawn_managed_dsh(
    state: &ManagedDshState,
    subject_id: &str,
    root_path: &str,
    profile: &str,
    args: &[String],
) -> Result<u32, String> {
    let root = std::fs::canonicalize(root_path.trim())
        .map_err(|_| "DSH 根目录不存在或不可访问".to_string())?;
    if !root.join("package.json").is_file() || !root.join("apps").join("cli").is_dir() {
        return Err("选择的目录不是可识别的 DSH 项目根目录".into());
    }
    let profile = profile.trim();
    if profile.is_empty()
        || !profile
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
    {
        return Err("DSH profile 只能包含字母、数字、连字符或下划线".into());
    }
    // 参数在入口再洗一遍：`spawn_managed_dsh` 是公开的入口，不该假设每个调用方都洗过。
    let args = harness_launch::normalize_launch_args(Some(args.to_vec()))?;
    let key = harness_launch::instance_key(subject_id, &args);
    let bundled_cli = root.join("apps").join("cli").join("lib").join("bin.js");
    let use_bundled_cli = bundled_cli.is_file();
    // 启动器只有这一个来源：本 build 自己选的 node/pnpm。用户能改的只有后面的参数 —— 这是本次
    // 改动有意收缩的那一处能力（原来这里可以是一个任意程序）。
    let launcher = if use_bundled_cli { "node.exe" } else { "pnpm.cmd" };
    let launcher_path = resolve_dsh_launcher(launcher).ok_or_else(|| {
        if use_bundled_cli {
            "未找到 Node.js。请确认 node.exe 已加入系统 PATH 后再试。".to_string()
        } else {
            "未找到 pnpm。请确认 pnpm.cmd 已加入系统 PATH 后再试。".to_string()
        }
    })?;
    let mut managed = state
        .0
        .lock()
        .map_err(|_| "DSH 进程状态不可用".to_string())?;
    // **按实例**查重，而不是按"有没有孩子"：同一个主体的另一个端口已经在跑时，本次仍然要起。
    // 而同一个实例（同主体、同参数）重复按「启动」只是幂等的一次，不该多出一个进程。
    if let Some(existing) = managed.get_mut(&key) {
        match existing.child.try_wait() {
            Ok(None) => return Ok(existing.child.id()),
            Ok(Some(_)) | Err(_) => {
                managed.remove(&key);
            }
        }
    }
    let port = harness_launch::instance_port(&args);
    // 这个端口上跑的**是本应用启动的另一个实例**（同主体、不同参数，例如只差一个 `--host`）
    // ⇒ 那不是冲突，而是"已经有了"：如实回答它的 pid，而不是报一句"被别的程序占用"。
    // 那句话在这里是假的，而且它会指向错误的下一步 —— 用户会去找一个不存在的程序。
    if let Some(existing) = harness_launch::owned_instances(subject_id)
        .into_iter()
        .find(|child| child.port == Some(port))
    {
        log::info!(
            "managed DSH already serving {port} as a sibling instance: pid={}",
            existing.pid
        );
        return Ok(existing.pid);
    }
    if dsh_port_is_occupied(port) {
        return Err(format!(
            "本机 {port} 端口已被其他进程占用；请先关闭它，或在「启动参数」里为这个实例换一个端口。"
        ));
    }
    let mut launch = std::process::Command::new(launcher_path);
    if use_bundled_cli {
        // `pnpm dsh` intentionally loads the TypeScript source through
        // `tsx/esm`, which is convenient for development but needlessly adds
        // a loader and project graph walk every time the desktop asks for a
        // resident Harness. A built CLI is self-contained and remains
        // compatible with the same profile directory.
        launch.arg(bundled_cli).args(["--profile", profile]);
    } else {
        launch.args(["dsh", "--profile", profile]);
    }
    // 「启动参数」追加在**最后**：DSH 的启动器只解析自己那几个旗标，第一个不认识的词之后整段
    // 原样交给被 boot 的档案，所以 `--port 3081` 必须跟在 `--profile` 后面才到得了 web 应用。
    launch.args(&args);
    launch.current_dir(&root);
    // DSH is a resident background service. `pnpm.cmd` otherwise inherits a new
    // visible console from the desktop process, leaving a stray CMD window
    // beside the wallpaper.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut launch, 0x08000000);
    let managed_log = managed_dsh_log_path(&key);
    launch.stdin(Stdio::null());
    let log_file = managed_log.as_ref().and_then(|path| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    });
    match log_file {
        Some(file) => match file.try_clone() {
            Ok(duplicate) => {
                launch.stdout(Stdio::from(file));
                launch.stderr(Stdio::from(duplicate));
            }
            Err(_) => {
                launch.stdout(Stdio::from(file));
                launch.stderr(Stdio::null());
            }
        },
        None => {
            launch.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    // Record the exact command before it runs. A launch that fails inside DSH
    // cannot be told apart from a wrong profile or a wrong project root without
    // knowing what was actually executed.
    log::info!(
        "managed DSH launch: program={} args={:?} cwd={} log={}",
        launch.get_program().to_string_lossy(),
        launch
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<String>>(),
        root.display(),
        managed_log
            .as_ref()
            .map_or_else(|| "none".to_string(), |path| path.display().to_string())
    );
    let child = launch
        .spawn()
        .map_err(|error| format!("无法启动 DSH：{error}"))?;
    let pid = child.id();
    managed.insert(
        key.clone(),
        ManagedDshProcess {
            child,
            instance_key: key,
            subject_id: subject_id.trim().to_string(),
            root_path: root.to_string_lossy().into_owned(),
            profile: profile.into(),
            args,
            log_path: managed_log,
        },
    );
    Ok(pid)
}

/// 本应用启动的**全部** DSH 实例。
///
/// 三件事必须一起成立，否则并行实例这个功能就是半成品：
///
/// 1. **每一格都报**，不是"当前对齐的那一格"。用户要的是"本应用启动的全部实例"，因为同一个
///    源码目录可以有两个端口各起一个，而漏掉任何一个都会让那一行没有停止入口。
/// 2. **官壳永不出现**。它不是本应用的实例（它是用户自己的客户端，退出方式是它自己的托盘菜单），
///    所以列表按 id 前缀过滤掉壳 —— 这条规则原来靠"壳不写记录"这个假设成立，而那个假设并不真
///    （`ensure_harness_ui` 会给它写一条门票记录），所以现在它是**明写**的过滤。
/// 3. **两种来源都要看**。内存里是本进程启动的（有 `Child` 句柄，能 `wait`）；落盘记录是跨壁纸
///    重启的那一半（例如每次装机）。任何不确定（没有记录、进程已退出、创建时间对不上）都按
///    "不是我启动的"处理 —— 安全方向：宁可少一行，绝不多停一个别人的进程。
///
/// `subject_id` 只影响 `managed` / `running` 两个汇总字段（启动监督问的是"我这次启动的那个
/// 孩子还在不在"）；`instances` 永远是全部。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn managed_dsh_status(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, ManagedDshState>,
    subject_id: Option<String>,
) -> Result<ManagedDshStatus, String> {
    require_wallpaper_surface(&caller)?;
    let mut managed = state
        .0
        .lock()
        .map_err(|_| "DSH 进程状态不可用".to_string())?;
    let mut instances: Vec<ManagedDshInstance> = Vec::new();
    // 先扫内存：活着的一条留下，已经退出的那条**顺手清掉**，并把它的最后几句话记进日志 ——
    // 那是唯一能解释"为什么没了"的证据。
    let keys: Vec<String> = managed.keys().cloned().collect();
    for key in keys {
        let Some(process) = managed.get_mut(&key) else { continue };
        match process.child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                log::warn!(
                    "managed DSH exited: instance={} pid={} code={:?}; last output:\n{}",
                    process.instance_key,
                    process.child.id(),
                    status.code(),
                    managed_dsh_log_tail(process.log_path.as_ref(), 20)
                );
                managed.remove(&key);
                continue;
            }
            Err(error) => {
                log::warn!("managed DSH state query failed: {error}");
                managed.remove(&key);
                continue;
            }
        }
        let process = managed.get(&key).expect("just checked");
        instances.push(ManagedDshInstance {
            instance_key: process.instance_key.clone(),
            subject_id: process.subject_id.clone(),
            // 端口：先用**观察到**的那一个（落盘记录里由端口属主写下的），再用"我们要求它听在
            // 哪儿"（参数里的 `--port`，没有就是 DSH 自己的默认 3080）。两个都不是身份 —— 身份
            // 永远是 pid + 创建时间 —— 但它们合起来总能让那一行不是空的：一个连端口都显示不出来的
            // 停止入口，用户没法确认自己要停的是哪一个。
            port: harness_launch::recorded_port(&process.instance_key)
                .or_else(|| Some(harness_launch::instance_port(&process.args))),
            pid: process.child.id(),
            root_path: Some(process.root_path.clone()),
            profile: Some(process.profile.clone()),
            args: process.args.clone(),
        });
    }
    let in_memory: Vec<ManagedDshInstance> = instances;
    let instances = merge_managed_instances(in_memory, harness_launch::owned_instances_all());
    let asked = subject_id
        .as_deref()
        .map(str::trim)
        .filter(|subject| !subject.is_empty());
    let owned = match asked {
        Some(subject) => instances
            .iter()
            .filter(|item| item.subject_id.trim() == subject)
            .count(),
        None => instances.len(),
    };
    Ok(ManagedDshStatus {
        managed: owned > 0,
        running: owned > 0,
        instances,
    })
}

/// 把两种来源合成**该报给界面的那一份**实例列表。纯函数，所以它的规则可以被测试钉住。
///
/// 规则只有三条，而每一条都对应一个具体的坏结果：
///
/// 1. **内存里的赢**。同一个实例键在两处都有时用内存那一份，因为只有它知道 `root_path` / `profile`
///    与真正的 `Child` 句柄；落盘那一份的 pid 与端口可能已经旧了。
/// 2. **官壳一律丢掉，两条来源都丢**。它是用户自己的客户端，不属于本应用可停止的实例。
/// 3. **端口至少有一样**：观察到的那一个（落盘记录里由端口属主写下的）优先，否则用"我们要求它
///    听在哪儿"（参数里的 `--port`，没有就是 DSH 自己的默认）。都不是身份，但一个连端口都显示
///    不出来的停止入口，用户没法确认自己要停的是哪一个。
#[cfg(not(feature = "lite"))]
pub(super) fn merge_managed_instances(
    in_memory: Vec<ManagedDshInstance>,
    recorded: Vec<harness_launch::ManagedChild>,
) -> Vec<ManagedDshInstance> {
    let mut merged: Vec<ManagedDshInstance> = Vec::with_capacity(in_memory.len() + recorded.len());
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for instance in in_memory {
        if !harness_launch::is_managed_by_us(&instance.subject_id) {
            continue;
        }
        seen.insert(instance.instance_key.clone());
        merged.push(instance);
    }
    for child in recorded {
        if seen.contains(&child.instance_key) {
            continue;
        }
        merged.push(ManagedDshInstance {
            port: child
                .port
                .or_else(|| Some(harness_launch::instance_port(&child.args))),
            instance_key: child.instance_key,
            subject_id: child.subject_id,
            pid: child.pid,
            root_path: None,
            profile: None,
            args: child.args,
        });
    }
    merged.retain(|item| harness_launch::is_managed_by_us(&item.subject_id));
    // 顺序稳定（落盘记录本来就是 BTreeMap）：界面上的每一行不该每次刷新都换个位置。
    merged.sort_by(|left, right| left.instance_key.cmp(&right.instance_key));
    merged
}

/// 停止本应用启动的 DSH：点名一个实例，或者不带名字就停**全部**。
///
/// 两个入口共用这一条命令，因为它们是同一个动作：「实例下拉里某一行的 ×」点的是哪一个由
/// `instance_key` 指名，而「全部停止」不带名字 —— 后者的语义正是原来那个
/// 「停止本应用启动的 DSH」按钮的语义（它当时只能停"唯一的那个"，现在有了并行实例，"全部"
/// 才是它本来的意思）。两个控件做同一件事的问题因此只有一个动作、一个实现。
///
/// **官壳停不了，而且不是"刚好没找到"**：`harness_launch::is_managed_by_us` 明确拒绝它。
/// 那不是本应用的实例 —— 停它会当场关掉用户自己的客户端，并弹一条"宿主意外退出"的报错框。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn stop_managed_dsh(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    instance_key: Option<String>,
) -> Result<(), String> {
    require_settings(&caller)?;
    // 这一步会起 taskkill 并等它结束（正常几百毫秒，遇到卡住的进程更久），所以**不能**在界面
    // 线程上做 —— 那正是"设置窗口先卡死"的成因。状态也在闭包里重新取，避免借用外部的 State。
    tauri::async_runtime::spawn_blocking(move || {
        let named = instance_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        // 清单与 `managed_dsh_status` 用的是同一处（官壳已经滤掉、并且每一条都通过了
        // pid + 创建时间的校验），所以"界面显示什么"与"这里能停什么"不可能对不上。
        let targets = stop_targets(named, harness_launch::owned_instances_all())?;
        let mut failures: Vec<String> = Vec::new();
        for key in targets {
            if let Err(error) = stop_one_instance(&app, &key) {
                failures.push(error);
            }
        }
        match failures.first() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    })
    .await
    .map_err(|error| format!("停止 DSH 未完成：{error}"))?
}

#[cfg(not(feature = "lite"))]
pub(crate) fn shutdown_managed_dsh(app: &tauri::AppHandle) {
    let mut in_memory_keys = Vec::new();
    if let Some(state) = app.try_state::<ManagedDshState>() {
        if let Ok(mut managed) = state.0.lock() {
            let mut in_memory = std::mem::take(&mut *managed);
            in_memory_keys = in_memory.keys().cloned().collect();
            for key in shutdown_targets(in_memory_keys.clone(), Vec::new()) {
                let Some(mut process) = in_memory.remove(&key) else {
                    continue;
                };
                crate::client_window::stop_process_tree(process.child.id());
                #[cfg(not(windows))]
                {
                    let _ = process.child.kill();
                }
                let _ = process.child.wait();
                harness_launch::forget_instance(&key);
            }
        }
    }

    // 先停内存孩子并忘掉对应记录，再读落盘记录；即使 forget 写入失败，owned_instances_all
    // 也会按 PID + 创建时间复核，已停进程自然不会再进第二轮。
    let recorded = harness_launch::owned_instances_all();
    let target_keys = shutdown_targets(in_memory_keys, recorded.clone())
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    for child in recorded {
        if !target_keys.contains(&child.instance_key) {
            continue;
        }
        if crate::client_window::stop_process_tree(child.pid) {
            harness_launch::forget_instance(&child.instance_key);
        } else {
            log::warn!("managed DSH stop failed for pid {}", child.pid);
        }
    }
}

/// The process keys this normal exit is authorized to stop: in-memory children
/// plus already-validated persisted children, deduplicated and excluding the shell.
#[cfg(not(feature = "lite"))]
pub(super) fn shutdown_targets(
    in_memory_keys: Vec<String>,
    recorded: Vec<harness_launch::ManagedChild>,
) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut targets = Vec::new();
    for key in in_memory_keys {
        if harness_launch::is_managed_by_us(&key) && seen.insert(key.clone()) {
            targets.push(key);
        }
    }
    for child in recorded {
        if harness_launch::is_managed_by_us(&child.subject_id)
            && seen.insert(child.instance_key.clone())
        {
            targets.push(child.instance_key);
        }
    }
    targets
}

/// 这一次要停哪些实例。纯函数，因为它是"停哪一个"与"绝不碰官壳"两条规则的落点。
///
/// * 点名了 `instance_key` ⇒ 就是它，**一个**。这正是并行实例需要的粒度：3080 上那个还在
///   服务时，停掉 3081 上那个不该顺手把 3080 也带走。
/// * 没点名 ⇒ 全部（本应用启动的每一个）。原来那个「停止本应用启动的 DSH」按钮在只有一个孩子时
///   就是这个意思，现在有了并行实例，"全部"才是它本来的语义。
/// * **官壳在任何一条路上都被拒绝**：点名它得到一句明确的拒绝（而不是静默成功），不带名字时它
///   根本不在候选里。两条都必要 —— 前者防止一个被改错键的调用方停掉用户自己的客户端，后者是
///   正常路径。
#[cfg(not(feature = "lite"))]
pub(super) fn stop_targets(
    named: Option<String>,
    owned: Vec<harness_launch::ManagedChild>,
) -> Result<Vec<String>, String> {
    match named {
        Some(key) => {
            if !harness_launch::is_managed_by_us(&key) {
                log::warn!("refusing to stop a subject this app does not manage: {key}");
                return Err("这个主体不由本应用管理，因此没有停止它。".to_string());
            }
            Ok(vec![key])
        }
        None => Ok(owned
            .into_iter()
            .filter(|child| harness_launch::is_managed_by_us(&child.subject_id))
            .map(|child| child.instance_key)
            .collect()),
    }
}

/// 停掉一个实例：内存里的 `Child`（能等它退出）优先，否则走落盘记录那条路。
#[cfg(not(feature = "lite"))]
fn stop_one_instance(app: &tauri::AppHandle, instance_key: &str) -> Result<(), String> {
    // 官壳在动任何进程之前就被拒绝：这是"绝不接管他人实例"那条底线在**命令**这一侧的落点，
    // 而不是依赖界面不显示它。
    if !harness_launch::is_managed_by_us(instance_key) {
        log::warn!("refusing to stop a subject this app does not manage: {instance_key}");
        return Err("这个主体不由本应用管理，因此没有停止它。".to_string());
    }
    let state = app.state::<ManagedDshState>();
    let taken = {
        let mut managed = state
            .0
            .lock()
            .map_err(|_| "DSH 进程状态不可用".to_string())?;
        managed.remove(instance_key)
    };
    if let Some(mut process) = taken {
        let pid = process.child.id();
        #[cfg(windows)]
        let stopped = crate::client_window::stop_process_tree(pid);
        #[cfg(not(windows))]
        let stopped = process.child.kill().is_ok();
        if !stopped {
            log::warn!("managed DSH stop failed for pid {pid}");
        }
        let _ = process.child.wait();
        harness_launch::forget_instance(instance_key);
        return if stopped {
            Ok(())
        } else {
            Err("无法停止该 DSH 进程；它可能已经退出，或被别的程序接管了。".to_string())
        };
    }
    // 内存里没有这个孩子（壁纸重启过）⇒ 看落盘记录，而且**只有判定为真的那一格**才动手：
    // pid 与创建时间都对上，才承认它是本应用启动的那个。对不上就什么都不做。
    match harness_launch::owned_instance(instance_key) {
        Some(child) => {
            if crate::client_window::stop_process_tree(child.pid) {
                harness_launch::forget_instance(instance_key);
                Ok(())
            } else {
                Err("无法停止该 DSH 进程；它可能已经退出，或被别的程序接管了。".to_string())
            }
        }
        // 记录里没有它，或者已经不是同一个进程：什么都不做，并且如实说"没有可停的"。
        // 静默成功会更坏 —— 界面会刷新出一个"已经停了"的假象。
        None => Err("没有找到这个实例：它可能已经退出，或从来不是本应用启动的。".to_string()),
    }
}
