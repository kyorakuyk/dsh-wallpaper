#[cfg(not(feature = "lite"))]
mod api_persistence;
mod app_core;
#[cfg(not(feature = "lite"))]
mod appearance;
#[cfg(not(feature = "lite"))]
mod chat;
mod client_window;
pub mod desktop_repair;
#[cfg(not(feature = "lite"))]
mod deepseek_web;
// 打开转写里的外部链接：白名单在这个模块里，见其头部说明。
mod external_link;
#[cfg(not(feature = "lite"))]
mod deepseek_web_config;
#[cfg(not(feature = "lite"))]
mod harness_targets;
#[cfg(not(feature = "lite"))]
mod harness_launch;
#[cfg(not(feature = "lite"))]
mod harness_catalog;
// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定，见 docs/plans/release-scope-cleanup-plan.md）。恢复办法：取消注释。
// mod desktop_fallback;
// 悬浮球是「折叠态胶囊」的新家，而胶囊只属于完整版；它的创建/监控也只挂在完整版
// 的 setup 块里。像上面 api_persistence / chat 那样按 edition 收窄，否则整个模块
// 在 Lite 构建里全部不可达，会给 Lite 门禁凭空加一串 dead_code 警告。
#[cfg(not(feature = "lite"))]
mod floating_ball;
mod lock_screen_backup;
mod native_bootstrap;
mod native_handoff;
// 更新检测（第一片：原生侧）：版本读取、GitHub 检查、状态文件与 6 小时节流。完整版专属 ——
// 取数用的是 `reqwest`，那是 `full` 特性后面的依赖，Lite 只给提示（计划书 §七）。
#[cfg(not(feature = "lite"))]
mod update;
mod windows_integration;

#[cfg(not(feature = "lite"))]
use app_core::BackendMode;
use app_core::{Activity, AppAction, AppCore, AppSnapshot, HarnessAvailability};
#[cfg(not(feature = "lite"))]
use std::collections::{HashSet, VecDeque};
#[cfg(not(feature = "lite"))]
use std::path::{Path, PathBuf};
#[cfg(not(feature = "lite"))]
use std::process::{Child, Stdio};
use std::sync::OnceLock;
#[cfg(not(feature = "lite"))]
use std::sync::{Mutex, RwLock};
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, EventTarget, Manager, WebviewUrl, WebviewWindowBuilder};

/// Paint the immutable first frame before Tauri/WebView2 starts. This is a
/// best-effort user-mode guard; it uses a bounded WorkerW discovery window and
/// keeps a valid Progman fallback until the Explorer host can be recovered.
pub fn prepare_native_bootstrap() {
    native_bootstrap::prepare();
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(feature = "lite"))]
struct DshPathCandidate {
    root_path: String,
    source: String,
}

#[cfg(not(feature = "lite"))]
const DSH_SCAN_MAX_DIRECTORIES: usize = 4096;
#[cfg(not(feature = "lite"))]
const DSH_SCAN_MAX_DEPTH: u8 = 5;

#[cfg(not(feature = "lite"))]
fn is_dsh_root(path: &Path) -> bool {
    path.join("package.json").is_file() && path.join("apps").join("cli").is_dir()
}

#[cfg(not(feature = "lite"))]
fn enqueue_dsh_scan_root(queue: &mut VecDeque<(PathBuf, u8, String)>, path: PathBuf, source: &str) {
    if !path.as_os_str().is_empty() && path.is_dir() {
        queue.push_back((path, 0, source.to_string()));
    }
}

#[cfg(not(feature = "lite"))]
fn should_skip_dsh_scan_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return true;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "$recycle.bin"
            | "appdata"
            | "build"
            | "dist"
            | ".git"
            | "node_modules"
            | "program files"
            | "program files (x86)"
            | "programdata"
            | "system volume information"
            | "target"
            | "windows"
    )
}

#[cfg(not(feature = "lite"))]
fn scan_dsh_paths_blocking(hint_path: Option<String>, deep_scan: bool) -> Vec<DshPathCandidate> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut seen_candidates = HashSet::new();
    let mut candidates = Vec::new();

    if let Some(hint) = hint_path
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        enqueue_dsh_scan_root(&mut queue, PathBuf::from(hint), "当前设置路径");
    }
    if let Ok(current) = std::env::current_dir() {
        enqueue_dsh_scan_root(&mut queue, current, "当前目录");
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        let home = PathBuf::from(home);
        for relative in [
            "source/deepseek-harness",
            "Documents/deepseek-harness",
            "Documents/DeepSeekHarness/deepseek-harness",
            "Desktop/deepseek-harness",
            "Downloads/deepseek-harness",
        ] {
            enqueue_dsh_scan_root(&mut queue, home.join(relative), "常见项目目录");
        }
        enqueue_dsh_scan_root(&mut queue, home, "用户目录扫描");
    }
    if let Ok(dsh_root) = std::env::var("DSH_ROOT") {
        enqueue_dsh_scan_root(&mut queue, PathBuf::from(dsh_root), "DSH_ROOT 环境变量");
    }
    if deep_scan {
        for drive in b'A'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", drive as char));
            enqueue_dsh_scan_root(&mut queue, root, "磁盘扫描");
        }
    }

    while let Some((path, depth, source)) = queue.pop_front() {
        if visited.len() >= DSH_SCAN_MAX_DIRECTORIES {
            break;
        }
        let key = path.to_string_lossy().to_ascii_lowercase();
        if !visited.insert(key) {
            continue;
        }
        if is_dsh_root(&path) {
            let root = std::fs::canonicalize(&path).unwrap_or(path.clone());
            let candidate_key = root.to_string_lossy().to_ascii_lowercase();
            if seen_candidates.insert(candidate_key) {
                candidates.push(DshPathCandidate {
                    root_path: root.to_string_lossy().into_owned(),
                    source: source.clone(),
                });
            }
        }
        if depth >= DSH_SCAN_MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if entry
                .file_type()
                .map(|value| value.is_dir())
                .unwrap_or(false)
                && !should_skip_dsh_scan_directory(&child)
            {
                queue.push_back((child, depth.saturating_add(1), source.clone()));
            }
        }
    }

    candidates
}

#[cfg(not(feature = "lite"))]
fn resolve_dsh_launcher(value: &str) -> Option<PathBuf> {
    let requested = Path::new(value);
    if requested.components().count() > 1 || value.contains('/') || value.contains('\\') {
        return requested.is_file().then(|| requested.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(value);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    #[cfg(windows)]
    {
        for candidate in [
            std::env::var_os("ProgramFiles")
                .map(|root| PathBuf::from(root).join("nodejs").join(value)),
            std::env::var_os("LOCALAPPDATA").map(|root| {
                PathBuf::from(root)
                    .join("Programs")
                    .join("nodejs")
                    .join(value)
            }),
            std::env::var_os("APPDATA").map(|root| PathBuf::from(root).join("npm").join(value)),
        ]
        .into_iter()
        .flatten()
        {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 这个端口上有没有人在听。
///
/// 端口是**参数**而不是常量 3080：同一个主体可以在 3080 与 3081 上各起一个实例，而"能不能起"
/// 只取决于**这一次要用的那个端口**。写死 3080 会让第二个实例永远起不来 —— 那不是并行实例，
/// 那是"并行实例被自己的第一个实例挡住"。
#[cfg(not(feature = "lite"))]
fn dsh_port_is_occupied(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(250),
    )
    .is_ok()
}

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
struct ManagedDshState(Mutex<std::collections::BTreeMap<String, ManagedDshProcess>>);

/// Records the one automatic launch attempt this process is allowed to make.
///
/// The single-flight guarantee has to live here rather than in the renderer: a
/// React remount, a `settings-changed` broadcast, an unlock, HMR, or a second
/// WebView would each be a fresh caller, and only the process-wide state can
/// enforce "start DSH at most once per wallpaper launch".
#[cfg(not(feature = "lite"))]
#[derive(Default)]
struct ManagedDshAutostartState(Mutex<Option<ManagedDshAutostart>>);

#[cfg(not(feature = "lite"))]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedDshAutostart {
    /// Stable, non-sensitive outcome code; never an exception body.
    outcome: String,
    /// Present only when this process actually spawned the child.
    pid: Option<u32>,
    /// True when an external DSH already owned 3080 and was left alone.
    external: bool,
}

#[cfg(not(feature = "lite"))]
impl ManagedDshAutostart {
    fn new(outcome: &str) -> Self {
        Self { outcome: outcome.into(), pid: None, external: false }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(feature = "lite"))]
struct ManagedDshInstance {
    /// 停止与下拉都用它定位：`主体 id` + 启动参数（见 `harness_launch::instance_key`）。
    instance_key: String,
    subject_id: String,
    /// 这个实例在哪服务，读不到时为 `None`（渲染层会把它显示成「端口未确认」，而不是 0）。
    port: Option<u16>,
    pid: u32,
    /// 只在**本进程启动**的实例上有：落盘记录里的那一格只有 pid 与端口。
    root_path: Option<String>,
    profile: Option<String>,
    args: Vec<String>,
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
struct ManagedDshStatus {
    instances: Vec<ManagedDshInstance>,
    managed: bool,
    running: bool,
}

/// Only the settings surface is allowed to request the API-key prompt. This
/// is a defense in depth check: it prevents the desktop wallpaper WebView (or
/// a future WebView) from opening a native credential capture dialog.
const SETTINGS_WINDOW_LABEL: &str = "settings";

/// The WorkerW-backed wallpaper WebView is the only surface that may send,
/// cancel, or read chat conversations. Capabilities provide the primary ACL;
/// this guard makes that boundary survive an accidental future capability
/// change or a newly added WebView.
const BACKGROUND_WINDOW_LABEL: &str = "background";

/// The binary entry point fixes the product edition before Tauri starts. Keep
/// it in a process-local cell so native commands can fail closed if an old or
/// compromised Lite renderer tries to invoke a full-edition action.
static LITE_EDITION: OnceLock<bool> = OnceLock::new();

fn is_lite_edition() -> bool {
    *LITE_EDITION.get_or_init(|| false)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn scan_dsh_paths(
    caller: tauri::WebviewWindow,
    hint_path: Option<String>,
    deep_scan: Option<bool>,
) -> Result<Vec<DshPathCandidate>, String> {
    require_wallpaper_surface(&caller)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_dsh_paths_blocking(hint_path, deep_scan.unwrap_or(false))
    })
    .await
    .map_err(|error| format!("扫描 DSH 未完成：{error}"))
}

/// 确保某个主体的档案里有我们钉住的那一版桥。
///
/// 界面在"选中主体"时调用它（"每次启动"那条在 harness_launch 里、由原生自己做）。
/// 版本相符时它只读一下档案里的版本就收场：不跑包管理，也不碰用户的档案。
///
/// **只注册进完整版**：Lite 的能力边界明文禁止连接 harness（`verify-lite-bundle.ps1` 会检查），
/// 这条命令不该出现在那边。
// Lite 构建里连 harness_launch 模块都不存在（见文件顶部那一串 cfg），所以这条命令也要同门。
#[cfg(not(feature = "lite"))]
#[tauri::command]
async fn ensure_profile_bridge(
    caller: tauri::WebviewWindow,
    subject_id: String,
    profile: String,
) -> Result<Vec<harness_launch::BridgeInstallOutcome>, String> {
    // 两个应用窗口都会调它：设置中心负责"换主体时对齐并报告"，壁纸宿主负责"每次启动静默对齐"。
    // 所以这里不能只认设置窗口 —— 早先那样写，启动那一路被拒绝、又被前端的 catch 吞掉，
    // 实测表现为"删掉桥、重启应用后桥没有回来"（2026-09-30 06:30）。
    let label = caller.label();
    // 先记请求本身：三次"什么都没发生"的实测（2026-09-30）都是因为没有这条记录 —— 拒绝、
    // spawn 失败、CLI 报错在前端都被同一句话吞掉了，日志里什么都看不到。
    log::info!("装桥请求：caller={label} subject={subject_id} profile={profile}");
    if label != SETTINGS_WINDOW_LABEL && label != BACKGROUND_WINDOW_LABEL {
        log::warn!("装桥被拒：caller={label} 不是应用自己的窗口");
        return Err("该命令只允许壁纸宿主与设置中心调用。".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let kind = harness_launch::subject_kind_from_id(&subject_id);
        let outcomes = harness_launch::install_bridge_for_subject(&subject_id, &profile, kind);
        for outcome in &outcomes {
            log::info!(
                "装桥结果：profile={} status={} command={} detail={}",
                outcome.profile,
                outcome.status,
                outcome.command,
                outcome.detail
            );
        }
        outcomes
    })
    .await
    .map_err(|error| format!("装桥任务未完成：{error}"))
}

/// List the harness execution subjects this machine offers.
///
/// This is the shim's "find" half: it answers "which subjects exist" for the two
/// frozen classes (a shell that carries its own checkout, and a source tree),
/// and nothing else. It never starts, stops, or takes over anything, and it
/// reads no credential — a shell is recognised from its shell registration and a
/// checkout from its directory fingerprint, so no value it returns can be
/// invalidated by a client update.
///
/// Discovery is deliberately separate from launch: the process forms differ
/// enough that only discovery can be shared (`harness_targets`).
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn scan_harness_targets(
    caller: tauri::WebviewWindow,
    hint_path: Option<String>,
    deep_scan: Option<bool>,
) -> Result<harness_targets::HarnessTargetScan, String> {
    require_wallpaper_surface(&caller)?;
    let scan = tauri::async_runtime::spawn_blocking(move || {
        harness_targets::scan_harness_targets_blocking(hint_path, deep_scan.unwrap_or(false))
    })
    .await
    .map_err(|error| format!("扫描 Harness 执行主体未完成：{error}"))?;
    // A completed scan is a verification, so it refreshes the catalogue the settings
    // surface reads on open. Best effort by design: see `harness_catalog`.
    harness_catalog::persist_scan(&scan);
    Ok(scan)
}

/// The subjects the last scan confirmed, with the time it confirmed them.
///
/// Read-only and cheap, so the settings surface can show what it already knows
/// instead of walking the disk every time it opens. `null` means nothing has been
/// scanned yet, which the UI says plainly rather than showing an empty list as if it
/// were a result.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn harness_target_catalog(
    caller: tauri::WebviewWindow,
) -> Result<Option<harness_catalog::HarnessTargetCatalog>, String> {
    require_wallpaper_surface(&caller)?;
    Ok(harness_catalog::load_catalog())
}

/// Start the chosen execution subject — a shell that carries its own checkout, or
/// a source tree — and report what happened.
///
/// This is the shim's "start" half, and the only manual launch entry point once a
/// subject is chosen: the class decides the mechanism, so the caller passes an id
/// and gets a closed outcome code back instead of branching on client shape
/// itself. Checkouts still start through `spawn_managed_dsh`, which is what keeps
/// ownership tracking and the port-occupancy rule in one place.
///
/// 渲染层只有一处调它：岛上的「启动」滑槽。那处要的是"把这个主体供起来"（界面在壁纸这边），
/// 不是"把它的窗口给我看"，所以触发者是 `LaunchTrigger::Slider` —— 与开机自启一样把壳的窗口
/// 留在屏幕外。要显示窗口的那两个动作（设置里的「打开」、岛上的图标）走的是
/// `ensure_harness_ui`，它们不带隐藏。
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn launch_harness_target(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    target_id: String,
    profile: Option<String>,
    args: Option<Vec<String>>,
) -> Result<harness_launch::HarnessLaunchOutcome, String> {
    require_wallpaper_surface(&caller)?;
    let args = harness_launch::normalize_launch_args(args)?;
    let profile = profile.unwrap_or_default();
    // 决定"跑什么"要看扫描记录、必要时还要重扫一次（施工文档 §7.4 第 5 条），所以它与启动一起
    // 留在阻塞线程上：那一步最坏要几秒，而它绝不该占住界面线程。
    tauri::async_runtime::spawn_blocking(move || {
        let host_cli = harness_launch::shell_host_cli(&target_id);
        let plan = harness_launch::plan_background_launch(
            &target_id,
            &profile,
            &args,
            harness_launch::LaunchTrigger::Slider,
            host_cli.as_deref(),
        )
        .map_err(str::to_string)?;
        let managed = app.state::<ManagedDshState>();
        Ok::<_, String>(harness_launch::run_launch(&plan, managed.inner(), harness_launch::LaunchTrigger::Slider))
    })
    .await
    .map_err(|error| format!("启动 Harness 执行主体未完成：{error}"))?
}

/// The one automatic-launch record, read without consuming it.
///
/// Read and write are separate helpers on purpose: a live `tauri::State` borrow of
/// the app handle cannot cross an `await`, and the start below has to be offloaded
/// to a blocking task.
#[cfg(not(feature = "lite"))]
fn autostart_attempt(app: &tauri::AppHandle) -> Result<Option<ManagedDshAutostart>, String> {
    let state = app.state::<ManagedDshAutostartState>();
    let record = state
        .0
        .lock()
        .map_err(|_| "DSH 自动启动状态不可用".to_string())?;
    Ok(record.clone())
}

#[cfg(not(feature = "lite"))]
fn record_autostart_attempt(
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

/// Make a chosen execution subject's interface available and foreground.
///
/// 「拉起 UI」, and idempotent on purpose: the subject may not be running, may be
/// running with the window the wallpaper hid at startup, or may simply be behind
/// something else, and the caller should not have to know which. Starting a subject
/// is part of this action (§5.2), which is why it lives beside the launch chain
/// rather than in the raise-only path.
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn ensure_harness_ui(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    target_id: Option<String>,
    port: u16,
    profile: Option<String>,
    args: Option<Vec<String>>,
) -> Result<harness_launch::HarnessUiOutcome, String> {
    require_wallpaper_surface(&caller)?;
    let target_id = target_id.unwrap_or_default();
    let profile = profile.unwrap_or_default();
    let args = harness_launch::normalize_launch_args(args)?;
    let worker = app.clone();
    // 闭包要拿走一份用于记录，外部保留一份用于"这次是不是我启动的、pid 是多少"。
    let subject_for_record = target_id.clone();
    let profile_for_refresh = profile.clone();
    let args_for_record = args.clone();
    let args_for_refresh = args.clone();
    let record_from = app.clone();
    // **在动手之前**看一次：这个端口上有属主吗？
    //
    // 这是"孩子是不是我启动的"唯一可靠的判据。试过两个标志都不行：`started` 属于壳/受管链那套
    // 口径（实测已安装 CLI 被真正启动时它仍是 false），`start_outcome` 在这条路上也没带值 ⇒
    // 记录代码一次都没执行。所以不再问"哪条路发生了什么"，只观察事实本身：
    //   之前没人听、之后有人听 ⇒ 这次是我们启动的；
    //   之前就有人听 ⇒ 本来就在跑 ⇒ **不记**（正确的"不是我的"）。
    let owner_before = crate::client_window::endpoint_process_id(port);
    tauri::async_runtime::spawn_blocking(move || {
        let managed = worker.state::<ManagedDshState>();
        harness_launch::ensure_ui(
            &target_id,
            port,
            &profile,
            &args,
            managed.inner(),
        )
    })
    .await
    .map_err(|error| format!("拉起 Harness 界面未完成：{error}"))
    .map(|outcome| {
        // 记录"这个孩子是壁纸启动的"——**记的是端口的属主**，不是刚 spawn 出来的外壳。
        //
        // 为什么必须是属主：Windows 上启动 `.cmd` 会多出一层 `cmd.exe`（`dsh.cmd` 与 `pnpm.cmd`
        // 都是批处理），外壳与真正的 DSH 宿主是两个 pid。记外壳会让状态与停止按钮指错对象：
        // "还活着吗""是不是同一个进程""要停哪一个"全部会答错。属主才是那个在服务、也是我们真正
        // 想要停止的东西；`ensure_ui` 返回时端口已经在应答（它自己会等），所以这里读到的一定是它。
        //
        // 已安装的 CLI 与源码目录都走这一处：两条路的"孩子"定义本就该一致。
        if owner_before.is_none() {
            // 端口**不会**在 `ensure_ui` 返回前就绪：只有壳会等窗口，非壳主体不等（实测：刚 spawn
            // 完就去问属主，得到的是"还没人在听"，两秒后端口才起来）。所以这里不抢答，交给一个
            // 有限的轮询：端口一起来就把**那时**的属主记为孩子；等不到就留一条警告，让"没有记录"
            // 这件事有据可查，而不是静悄悄。
            let subject_for_record = subject_for_record.trim().to_string();
            std::thread::spawn(move || {
                for _ in 0..60 {
                    if let Some(pid) = crate::client_window::endpoint_process_id(port) {
                        harness_launch::remember_child(
                            &subject_for_record,
                            &args_for_record,
                            pid,
                            Some(port),
                            harness_launch::known_web_handoff(port),
                        );
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                log::warn!("harness managed-child unknown: nothing listening on {port} after waiting");
            });
        }
        // 门票只属于**那一次启动**：`dsh web` 每次起来现生成一个，打印一次。如果宿主已经在跑、
        // 而壁纸手里没有它这一代的门票（装机/重启之后最常见），浏览器会被自己的围栏挡在门外。
        // 处理分两种，界线是"是不是我启动的"：
        //   * 是我的 ⇒ 重启它一次去取票（这是本应用自己的孩子，动它不越界）；
        //   * 不是我的 ⇒ 什么都不做，由调用方如实说明拿不到门票 —— 绝不接管别人的实例。
        let subject = subject_for_record.trim();
        let needs_ticket = profile_for_refresh.trim() == "web"
            && !outcome.started
            && harness_launch::known_web_handoff(port).is_none();
        if needs_ticket {
            // 按**实例**找（主体 + 参数）：同一个主体的另一个端口有它自己的记录，不能被这里认领，
            // 否则"刷新 3080 的门票"会把 3081 那个实例重启掉。
            let key = harness_launch::instance_key(subject, &args_for_refresh);
            if let Some(child) = harness_launch::owned_instance(&key) {
                log::info!(
                    "harness handoff refresh: restarting our own host on {port} (pid {}) to capture its browser ticket",
                    child.pid
                );
                crate::client_window::stop_process_tree(child.pid);
                harness_launch::forget_instance(&key);
                let state = record_from.state::<ManagedDshState>();
                let _ = harness_launch::ensure_ui(
                    subject,
                    port,
                    &profile_for_refresh,
                    &args_for_refresh,
                    state.inner(),
                );
            } else {
                log::warn!(
                    "harness handoff unavailable for {port}: the host is not one this app started; leaving it alone"
                );
            }
        }
        outcome
    })
}

/// Start the chosen execution subject at most once per wallpaper process.
///
/// The automatic counterpart of `launch_harness_target`, for the
/// `autoStartWithWallpaper` setting. The trigger is the only difference in the
/// plan, and it is a real one: only this path may keep a window out of sight
/// (§5.1) and only this path refuses a custom launcher the user has not confirmed.
///
/// The single-flight record is shared with `autostart_managed_dsh`, so the two
/// entry points cannot each start something — the guarantee has to be native,
/// because a remount, a broadcast, an unlock, HMR or a second WebView would each
/// arrive as a fresh caller.
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn autostart_harness_target(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    target_id: Option<String>,
    profile: String,
    args: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    if let Some(previous) = autostart_attempt(&app)? {
        // Already attempted in this process: report the same outcome rather than
        // trying again.
        return Ok(serde_json::to_value(&previous).unwrap_or_else(|_| {
            serde_json::json!({ "outcome": "already-attempted" })
        }));
    }

    // Nothing chosen yet is the same situation the checkout-only version reported
    // as `root-path-missing`, and the settings view already words that outcome.
    let Some(target_id) = target_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        let outcome = ManagedDshAutostart::new("root-path-missing");
        record_autostart_attempt(&app, &outcome)?;
        return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
    };
    let args = harness_launch::normalize_launch_args(args)?;
    // The handle is cloned into the blocking task so this one keeps working for
    // the record below: the task owns the clone, this frame owns the original.
    let worker = app.clone();
    // 计划连同启动一起放进阻塞任务：决定"跑什么"可能要重扫一次扫描记录（§7.4 第 5 条），
    // 而那条路最坏几秒。拿不到计划时把拒绝码原样带回来记进那一次尝试的结果里。
    let subject_for_plan = target_id.clone();
    let planned = tauri::async_runtime::spawn_blocking(move || {
        let host_cli = harness_launch::shell_host_cli(&subject_for_plan);
        let plan = harness_launch::plan_background_launch(
            &subject_for_plan,
            &profile,
            &args,
            harness_launch::LaunchTrigger::Automatic,
            host_cli.as_deref(),
        )?;
        let managed = worker.state::<ManagedDshState>();
        Ok::<_, &'static str>(harness_launch::run_launch(
            &plan,
            managed.inner(),
            harness_launch::LaunchTrigger::Automatic,
        ))
    })
    .await
    .map_err(|error| format!("启动 Harness 执行主体未完成：{error}"))?;
    let outcome = match planned {
        Ok(outcome) => outcome,
        Err(code) => {
            let outcome = ManagedDshAutostart::new(code);
            record_autostart_attempt(&app, &outcome)?;
            return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
        }
    };

    let record = ManagedDshAutostart {
        // An `already-running` subject is someone else's live client, left
        // completely alone: the same meaning `external` carries on the checkout
        // path, where an external DSH already owned the port.
        external: outcome.outcome == "already-running",
        pid: outcome.pid,
        outcome: outcome.outcome,
    };
    record_autostart_attempt(&app, &record)?;
    Ok(serde_json::to_value(record).unwrap_or(serde_json::Value::Null))
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn launch_dsh(
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
fn autostart_managed_dsh(
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
fn managed_dsh_autostart_status(
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
fn classify_dsh_launch_failure(error: &str) -> String {
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
fn managed_dsh_status(
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
fn merge_managed_instances(
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
async fn stop_managed_dsh(
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
fn stop_targets(
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

/// Settings are always authored by the dedicated settings surface and then
/// delivered to the wallpaper by Rust.  This prevents a renderer from
/// selecting an arbitrary event target through `core:event:emit_to`.
const SETTINGS_CHANGED_EVENT: &str = "settings-changed";
#[cfg(not(feature = "lite"))]
const APPEARANCE_CHANGED_EVENT: &str = "appearance-changed";

fn require_background(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == BACKGROUND_WINDOW_LABEL {
        Ok(())
    } else {
        Err("该命令只允许壁纸宿主调用。".into())
    }
}

fn require_settings(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == SETTINGS_WINDOW_LABEL {
        Ok(())
    } else {
        Err("该命令只允许设置中心调用。".into())
    }
}

/// 悬浮球是独立顶层窗口，只允许它请求「进里桌面」。
///
/// 命令表面尽量小：球没有别的能力，也不需要别的能力。
#[cfg(not(feature = "lite"))]
fn require_ball(caller: &tauri::WebviewWindow) -> Result<(), String> {
    if caller.label() == floating_ball::BALL_LABEL {
        Ok(())
    } else {
        Err("该命令只允许悬浮球调用。".into())
    }
}

/// These commands expose no conversation body or system-setting write:
/// the settings center needs a current runtime snapshot, and the wallpaper's
/// DeepSeek entry needs to create/show one fixed, domain-restricted WebView.
/// Keep the shared surface restricted to the two declared WebViews.
fn require_wallpaper_surface(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        BACKGROUND_WINDOW_LABEL | SETTINGS_WINDOW_LABEL => Ok(()),
        _ => Err("该命令只允许壁纸或设置中心调用。".into()),
    }
}

/// Owner of the durable API transcript archive. The wallpaper composer reads
/// it (`api_history`) and the settings center lists and prunes it, so both
/// declared WebViews are allowed and nothing else is.
///
/// Listing returns only conversation metadata — id, message count, bytes and
/// last activity — never a message body, and deletion is the only mutation. A
/// caller that is allowed to destroy the archive is by construction allowed to
/// see its size.
#[cfg(not(feature = "lite"))]
fn require_api_history_owner(caller: &tauri::WebviewWindow) -> Result<(), String> {
    match caller.label() {
        BACKGROUND_WINDOW_LABEL | SETTINGS_WINDOW_LABEL => Ok(()),
        _ => Err("该命令只允许壁纸或设置中心调用。".into()),
    }
}

/// `keyring`'s Windows backend stores string secrets in a Generic Credential
/// as a UTF-16 little-endian byte blob. CredUI returns UTF-16 code units, so
/// encode them explicitly rather than relying on the host representation when
/// passing a raw `CredentialBlob` to `CredWriteW`.
#[cfg(all(windows, not(feature = "lite")))]
fn credential_blob_from_prompt_password(password: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(password.len().saturating_mul(std::mem::size_of::<u16>()));
    for code_unit in password {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

/// Do not use ordinary slice assignment for a secret: the optimizer is
/// permitted to remove a write whose result is never read. Volatile writes
/// make the cleanup observable to the machine, without adding a plaintext
/// dependency or copying the key through the WebView.
#[cfg(all(windows, not(feature = "lite")))]
fn secure_zero_u16(secret: &mut [u16]) {
    for value in secret {
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(all(windows, not(feature = "lite")))]
fn secure_zero_bytes(secret: &mut [u8]) {
    for value in secret {
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

fn emit_app_snapshot(app: &tauri::AppHandle, snapshot: &AppSnapshot) {
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        "app-snapshot",
        snapshot,
    );
}

fn dispatch_ui_action(app: &tauri::AppHandle, action: AppAction, _request_focus: bool) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    emit_app_snapshot(app, &snapshot);
}

fn dispatch_tray_ui_action(app: &tauri::AppHandle, action: AppAction) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    // Opening the tray menu temporarily makes Shell_TrayWnd the foreground
    // window. Treat an explicit tray command as user intent to reveal the
    // interaction surface; once focused, the normal foreground monitor takes over.
    core.dispatch(AppAction::DesktopForegroundChanged(true));
    let snapshot = core.dispatch(action);
    log::info!(
        "tray interaction request: phase={:?} visible={} settings={} desktop={}",
        snapshot.phase,
        snapshot.interaction.visible,
        snapshot.interaction.settings_open,
        snapshot.interaction.desktop_foreground
    );
    emit_app_snapshot(app, &snapshot);
}

fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        window
    } else {
        // Keep the resident wallpaper to one WebView at startup. Both the
        // full and Lite editions create the settings surface only on demand;
        // `hide_settings_window` destroys it for Lite and hides it for full.
        let title = if is_lite_edition() {
            "DSH Wallpaper Lite Settings"
        } else {
            "DSH Wallpaper Settings"
        };
        WebviewWindowBuilder::new(
            app,
            SETTINGS_WINDOW_LABEL,
            WebviewUrl::App("index.html?surface=settings".into()),
        )
        .title(title)
        .inner_size(920.0, 680.0)
        .min_inner_size(760.0, 560.0)
        .center()
        .decorations(false)
        .resizable(true)
        .transparent(true)
        // WebView 的**背衬底色**必须是面板深色，不能留默认的白。
        //
        // 起因（用户实测）：快速拖动窗口时会出现白色虚影。原因是窗口被做成透明之后，
        // CSS 根节点也是透明的，于是 WebView2 自己的默认底色（白）成了"还没画上内容
        // 的地方"的底色——拖动/重绘跟不上时那片区域就闪白。
        // 现在背衬是 #0d1625：来不及重绘时露出来的是深色，与面板几乎无差别；
        // 圆角处 CSS 曲线之外的那几像素也由 DWM 的系统圆角裁掉，不会露出方形。
        .background_color(tauri::window::Color(0x0d, 0x16, 0x25, 0xff))
        .focusable(true)
        .skip_taskbar(false)
        .visible(false)
        .build()
        .map_err(|error| format!("无法创建设置中心：{error}"))?
    };
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    // Tao/Windows can restore the active-window accent border as part of
    // SetForegroundWindow. Apply our DWM policy after activation so the CSS
    // outline remains the only visible settings frame.
    windows_integration::configure_settings_window(&window)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_settings_window(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_background(&caller)?;
    if let Some(core) = app.try_state::<AppCore>() {
        let snapshot = core.dispatch(AppAction::OpenSettings);
        emit_app_snapshot(&app, &snapshot);
    }
    show_settings_window(&app)
}

#[tauri::command]
fn native_bootstrap_generation(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    let generation = native_bootstrap::generation();
    native_bootstrap::record_startup_diagnostic(&format!(
        "event=handoff-generation-query generation={generation}"
    ));
    Ok(generation)
}

#[tauri::command]
fn release_native_bootstrap(caller: tauri::WebviewWindow, generation: u64) -> Result<bool, String> {
    require_background(&caller)?;
    let raw = caller.hwnd().map_err(|error| error.to_string())?;
    native_bootstrap::release(generation, raw.0 as isize)
}

#[tauri::command]
fn get_app_snapshot(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, AppCore>,
) -> Result<AppSnapshot, String> {
    require_wallpaper_surface(&caller)?;
    Ok(state.snapshot())
}

/// Enumerate the current physical monitor topology for the full desktop
/// edition. Settings and the background surface share this read-only command;
/// no display or wallpaper setting is changed here.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn desktop_displays(
    caller: tauri::WebviewWindow,
) -> Result<Vec<windows_integration::DesktopDisplayInfo>, String> {
    require_wallpaper_surface(&caller)?;
    windows_integration::desktop_displays()
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn set_interaction_enabled(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    require_settings(&caller)?;
    let snapshot = state.dispatch(AppAction::SetInteractionEnabled(enabled));
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn select_backend(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    backend: String,
) -> Result<AppSnapshot, String> {
    // 设置中心也要能切**正在运行的那个壁纸**的 chat 模式（网页桥接 / API / Harness）——
    // 用户实测提出的正是这条：运行中的壁纸没有 web↔api 的开关，输入岛那个开关只做
    // Harness↔网页。这里的白名单就是那条既有边界（壁纸宿主 + 设置中心），球与网页窗口仍然不行。
    // 切换成后照常 `emit_app_snapshot`：背景端就是靠这份快照改 `runtime.backend` 的（托盘菜单
    // 走的也是同一条路），所以这里不需要第二条通路。
    require_wallpaper_surface(&caller)?;
    if is_lite_edition() {
        return Err("Lite 版不提供后端或模型切换。".into());
    }
    let backend = match backend.as_str() {
        "deepseek-web" => BackendMode::DeepseekWeb,
        "deepseek-api" => BackendMode::DeepseekApi,
        "harness" => BackendMode::Harness,
        _ => return Err(format!("unknown backend: {backend}")),
    };
    let snapshot = state.dispatch(AppAction::SelectBackend(backend));
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn dispatch_app_action(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppCore>,
    action: String,
    play_wake: Option<bool>,
    value: Option<String>,
) -> Result<AppSnapshot, String> {
    require_background(&caller)?;
    if is_lite_edition()
        && !matches!(
            action.as_str(),
            "boot-ready" | "lock" | "unlock" | "wake-done" | "recover"
        )
    {
        return Err("Lite 版不提供会话或桌面交互动作。".into());
    }
    let action = match action.as_str() {
        "boot-ready" => AppAction::BootReady {
            play_wake: play_wake.unwrap_or(true),
        },
        "lock" => AppAction::Lock,
        "unlock" => AppAction::Unlock {
            play_wake: play_wake.unwrap_or(true),
        },
        "wake-done" => AppAction::WakeDone,
        "open-chat" => AppAction::OpenChat,
        "close-chat" => AppAction::CloseChat,
        "open-settings" => AppAction::OpenSettings,
        "close-settings" => AppAction::CloseSettings,
        "toggle-history" => AppAction::ToggleHistory,
        "auth-required" => AppAction::AuthRequired,
        "auth-ready" => AppAction::AuthReady,
        "recover" => AppAction::Recover,
        "fail" => AppAction::Fail(value.unwrap_or_else(|| "未知错误".into())),
        "set-island-pinned" => AppAction::SetIslandPinned(match value.as_deref() {
            Some("true") => true,
            Some("false") => false,
            _ => return Err("invalid island pinned flag".into()),
        }),
        "set-activity" => AppAction::SetActivity(match value.as_deref() {
            Some("idle") => Activity::Idle,
            Some("sending") => Activity::Sending,
            Some("thinking") => Activity::Thinking,
            Some("streaming") => Activity::Streaming,
            Some("tool") => Activity::Tool,
            Some("done") => Activity::Done,
            _ => return Err("invalid activity".into()),
        }),
        "set-harness" => AppAction::SetHarnessAvailability(match value.as_deref() {
            Some("offline") => HarnessAvailability::Offline,
            Some("web-only") => HarnessAvailability::WebOnly,
            Some("bridge-loading") => HarnessAvailability::BridgeLoading,
            Some("bridge-auth-unavailable") => HarnessAvailability::BridgeAuthUnavailable,
            Some("bridge-incompatible") => HarnessAvailability::BridgeIncompatible,
            Some("bridge-ready") => HarnessAvailability::BridgeReady,
            _ => return Err("invalid harness availability".into()),
        }),
        _ => return Err(format!("unknown app action: {action}")),
    };
    // Let the native hand-off layer get ahead of the renderer on both a cold
    // start and a later session unlock.  The call is idempotent when the WTS
    // callback already displayed the frame, and the WebView hides it only
    // after painting its matching wake frame.
    match &action {
        AppAction::BootReady { play_wake: true } | AppAction::Unlock { play_wake: true } => {
            let _ = native_bootstrap::start_wake();
        }
        AppAction::Lock => {
            let _ = native_bootstrap::show_sleep();
        }
        _ => {}
    }
    let snapshot = state.dispatch(action);
    emit_app_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn publish_settings(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    settings: serde_json::Value,
) -> Result<(), String> {
    require_settings(&caller)?;
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        SETTINGS_CHANGED_EVENT,
        settings,
    );
    Ok(())
}

/// Lite keeps its small, non-sensitive settings in a native file shared by
/// the background and settings WebViews.  Tauri gives each WebView an
/// isolated browser storage partition, so using localStorage here would make
/// a setting appear to reset whenever the wallpaper surface restarts.
#[cfg(feature = "lite")]
fn lite_settings_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法定位 Lite 设置目录：{error}"))?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建 Lite 设置目录：{error}"))?;
    Ok(directory.join("lite-settings.json"))
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_settings_get(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_wallpaper_surface(&caller)?;
    let path = lite_settings_file(&app)?;
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(serde_json::json!({}));
    };
    if bytes.len() > 128 * 1024 {
        return Ok(serde_json::json!({}));
    }
    let value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|_| {
        log::warn!("Lite 设置文件无法解析，将在前端恢复默认值");
        serde_json::json!({})
    });
    Ok(if value.is_object() {
        value
    } else {
        serde_json::json!({})
    })
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_settings_save(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    settings: serde_json::Value,
) -> Result<(), String> {
    require_settings(&caller)?;
    if !settings.is_object() {
        return Err("Lite 设置必须是 JSON 对象。".into());
    }
    // Whitelist the release surface at the native boundary as well. This
    // prevents a stale/full renderer or a malformed caller from persisting
    // connection, conversation, or credential fields into Lite storage.
    let allowed = [
        "version",
        "background",
        "portrait",
        "animationsEnabled",
        "animationSpeed",
        "playWakeOnEveryUnlock",
        "skipWakeAnimation",
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//         "lockScreenEnabled",
        "desktopWallpaperFallback",
        "autostart",
    ];
    let object = settings.as_object().expect("object checked above");
    let sanitized = object
        .iter()
        .filter(|(key, _)| allowed.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    let settings = serde_json::Value::Object(sanitized);
    let bytes = serde_json::to_vec(&settings).map_err(|error| error.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("Lite 设置内容过大。".into());
    }
    let path = lite_settings_file(&app)?;
    std::fs::write(&path, bytes).map_err(|error| format!("无法保存 Lite 设置：{error}"))?;
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        SETTINGS_CHANGED_EVENT,
        settings,
    );
    Ok(())
}

#[cfg(feature = "lite")]
fn lite_image_directory(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法定位 Lite 素材目录：{error}"))?
        .join("assets");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建 Lite 素材目录：{error}"))?;
    Ok(directory)
}

#[cfg(feature = "lite")]
fn lite_image_extension(path: &std::path::Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| "自定义素材必须是 PNG、JPEG 或 WebP 图片。".to_string())?;
    match extension.as_str() {
        "png" => Ok("png"),
        "jpg" | "jpeg" => Ok("jpg"),
        "webp" => Ok("webp"),
        _ => Err("自定义素材必须是 PNG、JPEG 或 WebP 图片。".into()),
    }
}

#[cfg(feature = "lite")]
fn lite_image_slot_name(slot: &str) -> Result<&'static str, String> {
    match slot {
        "background" => Ok("background"),
        "portrait" => Ok("portrait"),
        _ => Err("Lite 素材位置无效。".into()),
    }
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_image_import(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    slot: String,
    path: String,
) -> Result<(), String> {
    require_settings(&caller)?;
    let slot = lite_image_slot_name(slot.trim())?;
    let source =
        std::fs::canonicalize(path.trim()).map_err(|error| format!("无法读取所选图片：{error}"))?;
    if !source.is_file() {
        return Err("所选路径不是图片文件。".into());
    }
    let metadata =
        std::fs::metadata(&source).map_err(|error| format!("无法读取图片大小：{error}"))?;
    if metadata.len() > 32 * 1024 * 1024 {
        return Err("图片不能超过 32 MB。".into());
    }
    let extension = lite_image_extension(&source)?;
    let directory = lite_image_directory(&app)?;
    let temporary = directory.join(format!(".{slot}-{}.tmp", std::process::id()));
    let destination = directory.join(format!("{slot}.{extension}"));
    let backup = directory.join(format!(".{slot}-{}.bak", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let _ = std::fs::remove_file(&backup);
    std::fs::copy(&source, &temporary).map_err(|error| format!("无法导入图片：{error}"))?;
    // Remove only the old, known slot files after the new copy has completed;
    // a failed copy therefore leaves the previous valid asset untouched.
    for old_extension in ["png", "jpg", "webp"] {
        let old = directory.join(format!("{slot}.{old_extension}"));
        if old != destination {
            let _ = std::fs::remove_file(old);
        }
    }
    let had_previous = if destination.is_file() {
        std::fs::rename(&destination, &backup).is_ok()
    } else {
        false
    };
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        let _ = std::fs::remove_file(&temporary);
        if had_previous {
            let _ = std::fs::rename(&backup, &destination);
        }
        return Err(format!("无法保存 Lite 图片：{error}"));
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

#[tauri::command]
#[cfg(feature = "lite")]
fn lite_image_resolve(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    slot: String,
) -> Result<Option<serde_json::Value>, String> {
    require_wallpaper_surface(&caller)?;
    let slot = lite_image_slot_name(slot.trim())?;
    let directory = lite_image_directory(&app)?;
    let candidate = ["png", "jpg", "webp"]
        .into_iter()
        .map(|extension| directory.join(format!("{slot}.{extension}")))
        .find(|path| path.is_file());
    let Some(path) = candidate else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|error| format!("无法读取 Lite 图片：{error}"))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Lite 图片超过 32 MB。".into());
    }
    use base64::Engine;
    let extension = lite_image_extension(&path)?;
    let mime_type = match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => unreachable!(),
    };
    Ok(Some(serde_json::json!({
        "mimeType": mime_type,
        "bytesBase64": base64::engine::general_purpose::STANDARD.encode(bytes),
    })))
}

/// The Lite release can optionally align Explorer's ordinary desktop
/// wallpaper with the packaged sleep artwork. This masks the short interval
/// before WorkerW/WebView2 paints after login. The source is resolved inside
/// Rust and the renderer can only choose the boolean setting.
// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定，见 docs/plans/release-scope-cleanup-plan.md）。恢复办法：取消注释。
// #[tauri::command]
// #[cfg(feature = "lite")]
// fn set_desktop_wallpaper_fallback(
//     caller: tauri::WebviewWindow,
//     enabled: bool,
// ) -> Result<String, String> {
//     require_settings(&caller)?;
//     if enabled {
//         let source = desktop_fallback::bundled_sleep_source()?;
//         desktop_fallback::set_fallback(Some(&source), true)
//     } else {
//         desktop_fallback::set_fallback(None, false)
//     }
// }
// 
// #[tauri::command]
// #[cfg(feature = "lite")]
// fn desktop_wallpaper_fallback_status(
//     caller: tauri::WebviewWindow,
// ) -> Result<desktop_fallback::DesktopWallpaperFallbackStatus, String> {
//     require_settings(&caller)?;
//     Ok(desktop_fallback::status())
// }

#[cfg(all(test, feature = "lite"))]
mod lite_asset_tests {
    use super::{lite_image_extension, lite_image_slot_name};

    #[test]
    fn accepts_only_supported_lite_image_extensions() {
        assert_eq!(
            lite_image_extension(std::path::Path::new("wallpaper.PNG")),
            Ok("png")
        );
        assert_eq!(
            lite_image_extension(std::path::Path::new("portrait.jpeg")),
            Ok("jpg")
        );
        assert!(lite_image_extension(std::path::Path::new("payload.exe")).is_err());
    }

    #[test]
    fn image_slots_are_closed_to_the_two_public_components() {
        assert_eq!(lite_image_slot_name("background"), Ok("background"));
        assert_eq!(lite_image_slot_name("portrait"), Ok("portrait"));
        assert!(lite_image_slot_name("sleep").is_err());
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn notify_appearance_changed(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_settings(&caller)?;
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        APPEARANCE_CHANGED_EVENT,
        (),
    );
    Ok(())
}

// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
// #[tauri::command]
// async fn set_lock_screen_enabled(
//     caller: tauri::WebviewWindow,
//     app: tauri::AppHandle,
//     enabled: bool,
// ) -> Result<String, String> {
//     require_settings(&caller)?;
//     windows_integration::set_lock_screen(&app, enabled).await
// }
// 
// #[tauri::command]
// async fn clear_stale_lock_screen_backup(
//     caller: tauri::WebviewWindow,
//     app: tauri::AppHandle,
//     confirmed: bool,
// ) -> Result<String, String> {
//     require_settings(&caller)?;
//     windows_integration::clear_stale_lock_screen_backup(&app, confirmed).await
// }
// 
// #[tauri::command]
// async fn get_lock_screen_diagnostics(
//     caller: tauri::WebviewWindow,
//     app: tauri::AppHandle,
// ) -> Result<windows_integration::LockScreenDiagnostics, String> {
//     require_settings(&caller)?;
//     tauri::async_runtime::spawn_blocking(move || windows_integration::lock_screen_diagnostics(&app))
//         .await
//         .map_err(|error| format!("读取锁屏诊断未完成：{error}"))?
// }

fn set_autostart_blocking(enabled: bool) -> Result<windows_integration::AutostartStatus, String> {
    // The whole operation lives next to the Windows calls it makes: which path
    // is authoritative, what is written, and — new — the read-back that decides
    // whether the settings page may call the change applied. This wrapper only
    // keeps it off the UI thread.
    windows_integration::set_autostart(enabled)
}

#[tauri::command]
async fn set_autostart(
    caller: tauri::WebviewWindow,
    enabled: bool,
) -> Result<windows_integration::AutostartStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || set_autostart_blocking(enabled))
        .await
        .map_err(|error| format!("开机自启操作未完成：{error}"))?
}

#[tauri::command]
async fn autostart_status(
    caller: tauri::WebviewWindow,
) -> Result<windows_integration::AutostartStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(windows_integration::autostart_status)
        .await
        .map_err(|error| format!("读取开机自启状态未完成：{error}"))?
}

// FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定，见 docs/plans/release-scope-cleanup-plan.md）。恢复办法：取消注释。
// #[derive(serde::Serialize)]
// #[serde(rename_all = "camelCase")]
// struct TranslucentTbStatus {
//     installed: bool,
//     running: bool,
//     source: Option<String>,
// }
// 
// #[cfg(windows)]
// fn hide_child_console(command: &mut std::process::Command) {
//     std::os::windows::process::CommandExt::creation_flags(command, 0x08000000);
// }
// 
// fn translucent_tb_status_blocking() -> Result<TranslucentTbStatus, String> {
//     #[cfg(windows)]
//     {
//         let mut tasklist = std::process::Command::new("tasklist");
//         tasklist.args(["/FI", "IMAGENAME eq TranslucentTB.exe", "/FO", "CSV", "/NH"]);
//         hide_child_console(&mut tasklist);
//         let running = tasklist.output().ok().is_some_and(|output| {
//             String::from_utf8_lossy(&output.stdout)
//                 .to_ascii_lowercase()
//                 .contains("translucenttb.exe")
//         });
// 
//         let mut where_command = std::process::Command::new("where.exe");
//         where_command.arg("ttb.exe");
//         hide_child_console(&mut where_command);
//         let alias = where_command
//             .output()
//             .ok()
//             .is_some_and(|output| output.status.success());
// 
//         // AppX discovery is occasionally slow on a busy Windows session, so
//         // this whole probe runs on a blocking worker and the child console is
//         // explicitly suppressed. The settings WebView remains responsive.
//         let mut packaged_command = std::process::Command::new("powershell.exe");
//         packaged_command.args([
//             "-NoLogo",
//             "-NoProfile",
//             "-NonInteractive",
//             "-Command",
//             "if (Get-AppxPackage -Name TranslucentTB -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }",
//         ]);
//         hide_child_console(&mut packaged_command);
//         let packaged = packaged_command
//             .status()
//             .ok()
//             .is_some_and(|status| status.success());
// 
//         return Ok(TranslucentTbStatus {
//             installed: alias || packaged,
//             running,
//             source: if alias {
//                 Some("execution-alias".into())
//             } else if packaged {
//                 Some("msix".into())
//             } else {
//                 None
//             },
//         });
//     }
// 
//     #[cfg(not(windows))]
//     Ok(TranslucentTbStatus {
//         installed: false,
//         running: false,
//         source: None,
//     })
// }
// 
// #[tauri::command]
// async fn translucent_tb_status(
//     caller: tauri::WebviewWindow,
// ) -> Result<TranslucentTbStatus, String> {
//     require_settings(&caller)?;
//     tauri::async_runtime::spawn_blocking(translucent_tb_status_blocking)
//         .await
//         .map_err(|error| format!("读取透明任务栏状态未完成：{error}"))?
// }
// 
// #[tauri::command]
// fn launch_translucent_tb(caller: tauri::WebviewWindow) -> Result<(), String> {
//     require_settings(&caller)?;
//     std::process::Command::new("ttb.exe")
//         .spawn()
//         .map(|_| ())
//         .map_err(|_| {
//             "未找到 TranslucentTB。请先从 Microsoft Store 安装并启用 ttb.exe 执行别名。".into()
//         })
// }
// 
// #[tauri::command]
// fn open_translucent_tb_install(caller: tauri::WebviewWindow) -> Result<(), String> {
//     require_settings(&caller)?;
//     // `explorer.exe <uri>` may treat the Store URI as a filesystem path and
//     // open Documents instead. Ask ShellExecute to resolve the URI protocol.
//     #[cfg(windows)]
//     {
//         let store_uri = "ms-windows-store://pdp/?ProductId=9PF4KZ2VN4W9";
//         let status = std::process::Command::new("powershell.exe")
//             .args([
//                 "-NoProfile",
//                 "-NonInteractive",
//                 "-Command",
//                 "Start-Process",
//                 store_uri,
//             ])
//             .status()
//             .map_err(|error| format!("无法启动 Microsoft Store：{error}"))?;
//         if status.success() {
//             return Ok(());
//         }
// 
//         // A Store-disabled Windows installation still gets a useful route.
//         std::process::Command::new("rundll32.exe")
//             .args([
//                 "url.dll,FileProtocolHandler",
//                 "https://apps.microsoft.com/detail/9PF4KZ2VN4W9",
//             ])
//             .spawn()
//             .map(|_| ())
//             .map_err(|error| format!("无法打开 TranslucentTB 下载页：{error}"))
//     }
//     #[cfg(not(windows))]
//     Err("TranslucentTB 仅支持 Windows。".into())
// }

// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
// /// Opens the system-owned lock-screen settings page. During the current
// /// MSIX-only test phase Windows accepts the package's bundled sleep image but
// /// may reject a user-image restore snapshot; delegating the choice to Windows
// /// is clearer and safer than pretending a restore has completed.
// #[tauri::command]
// fn open_windows_lock_screen_settings(caller: tauri::WebviewWindow) -> Result<(), String> {
//     require_settings(&caller)?;
//     #[cfg(windows)]
//     {
//         std::process::Command::new("powershell.exe")
//             .args([
//                 "-NoProfile",
//                 "-NonInteractive",
//                 "-Command",
//                 "Start-Process",
//                 "ms-settings:lockscreen",
//             ])
//             .spawn()
//             .map(|_| ())
//             .map_err(|error| format!("无法打开 Windows 锁屏设置：{error}"))
//     }
//     #[cfg(not(windows))]
//     Err("锁屏设置仅支持 Windows。".into())
// }

/// 「桌面会话」目录名。与桥那边（`bridge/src/index.ts`）保持一致：**同一个名字**既是工作区标题，
/// 也是壁纸数据目录下的那个子目录名。
#[cfg(not(feature = "lite"))]
const DESKTOP_WORKSPACE_DIRECTORY_NAME: &str = "桌面会话";

/// 桌面会话工作区落点的自检信息（只读）。
///
/// 位置规则与桥一致：**壁纸自己的数据目录**（`%LOCALAPPDATA%\com.dsh.wallpaper`，也就是打包标识）
/// 下的「桌面会话」。安装目录不能用——MSIX 每次升级会把整个安装目录替换掉，写在那里的东西必丢；
/// 数据目录则运行时可写、升级保留、卸载也留得下（用户明确要求"卸载之后也保留桌面会话的数据"）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn desktop_workspace_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
    Ok(desktop_workspace_status_from(&data_dir))
}

/// 项目记忆的文件名。**必须与桥里的 `PROJECT_MEMORY_FILE_NAME` 一致** —— 桥按它读/注入，
/// 设置中心按它打开；两处不同的话，用户在设置里看到的和助手实际用的会是两个文件。
#[cfg(not(feature = "lite"))]
const PROJECT_MEMORY_FILE_NAME: &str = "项目记忆.md";

/// 在资源管理器里打开「项目记忆」：文件在就选中它，不在就打开工作区目录（顺带建出来）。
///
/// 为什么要有这个按钮：记忆文件的**绝对路径不该出现在桌面会话里**（那是聊天面，不是文件管理器），
/// 但用户确实需要一个地方去改它 —— 那个地方就是设置，路径也只在这里出现。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_project_memory(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
    let workspace = data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME);
    std::fs::create_dir_all(&workspace).map_err(|error| format!("无法创建桌面会话目录：{error}"))?;
    let memory = workspace.join(PROJECT_MEMORY_FILE_NAME);
    // 有文件就选中它（用户一眼看到要改的东西）；没有就把目录打开，别替他造一个空文件。
    let argument = if memory.is_file() {
        format!("/select,{}", memory.display())
    } else {
        workspace.display().to_string()
    };
    std::process::Command::new("explorer.exe")
        .arg(&argument)
        .spawn()
        .map_err(|error| format!("无法打开资源管理器：{error}"))?;
    Ok(serde_json::json!({
        "opened": argument,
        "memoryFile": memory.display().to_string(),
        "memoryExists": memory.is_file(),
    }))
}

/// 「打开 TUI」：把本机的 TUI（`dst`）在一个**新的终端窗口**里拉起来。
///
/// 为什么这件事必须由原生做：能不能开终端窗口、以及**本机到底有没有装 TUI**，都只有原生能回答。
/// 而这条命令的契约是「缺什么就说什么」：找不到 `dst` 时返回 `opened: false` 加一句怎么办，
/// **绝不静默改成打开浏览器** —— 那等于替用户换了一条他没选的路。
///
/// 与 `open_project_memory` 同样的门：**只有设置中心能调**（`require_settings`）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_subject_tui(
    caller: tauri::WebviewWindow,
    args: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let args = harness_launch::normalize_launch_args(args)?;
    let launchers = harness_targets::tui_launcher_paths(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("PATH").ok().as_deref(),
    );
    let Some(launcher) = launchers.first() else {
        return Ok(serde_json::json!({
            "opened": false,
            "reason": "not-installed",
            "message": "本机没有找到 TUI（dst）。安装：npm i -g @deepseek-harness-tui/dsh-tui",
        }));
    };
    let (program, command_args) = harness_launch::tui_launch_command(launcher, &args);
    std::process::Command::new(&program)
        .args(&command_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("无法拉起 TUI：{error}"))?;
    Ok(serde_json::json!({
        "opened": true,
        "launcher": launcher.display().to_string(),
    }))
}

/// 「清除全部用户数据」里，**能由本应用安全删掉**的那几处。
///
/// 刻意不含 WebView2 配置目录（设置与网页登录态都住在里面）：那个目录正被运行中的进程占用，
/// 在这里删它只会删到一半；那条路径由命令自己回报给界面，让用户"退出应用后删掉"。
#[cfg(not(feature = "lite"))]
fn clearable_user_data_targets(data_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME)]
}

/// 清除本应用自己的用户数据：桌面会话工作区 + 凭据管理器里那条 API Key。
///
/// 只做这两件**能确定删干净**的事，并把其余（设置 / 网页登录态）连同路径回报出去 —— 卸载时
/// MSIX 没有自定义卸载界面可以问，所以这个显式入口就是"想清干净的时候能清干净"的答案。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn clear_user_data(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;

    let mut removed: Vec<String> = Vec::new();
    for target in clearable_user_data_targets(&data_dir) {
        if !target.exists() {
            continue;
        }
        std::fs::remove_dir_all(&target).map_err(|error| format!("无法删除 {}：{error}", target.display()))?;
        removed.push(target.display().to_string());
    }

    let credential_removed = remove_api_key_credential()?;

    Ok(serde_json::json!({
        "removed": removed,
        "credentialRemoved": credential_removed,
        // 这两处不在本进程的删除范围内，如实回报路径，让用户自己决定。
        "manual": [
            { "what": "设置与网页登录态", "path": data_dir.display().to_string() },
            { "what": "桥接凭据", "path": "~/.dsh/wallpaper" },
        ],
    }))
}

/// 删除凭据管理器里那条 API Key；本来就不存在也算成功（返回 `false`）。
#[cfg(all(windows, not(feature = "lite")))]
fn remove_api_key_credential() -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = HSTRING::from("deepseek-api.dsh-wallpaper");
    match unsafe { CredDeleteW(&target, CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(all(not(windows), not(feature = "lite")))]
fn remove_api_key_credential() -> Result<bool, String> {
    Err("仅 Windows 支持凭据管理器。".into())
}

#[cfg(all(test, not(feature = "lite")))]
mod desktop_workspace_tests {
    use super::{desktop_workspace_status_from, DESKTOP_WORKSPACE_DIRECTORY_NAME, PROJECT_MEMORY_FILE_NAME};

    /// 位置规则只有一条：数据目录下的「桌面会话」。
    ///
    /// 这条断言的价值在于它同时否掉了两个诱人的位置：安装目录（MSIX 升级时整体替换，写那里的必丢）
    /// 和 DSH 自己的目录（不是"壁纸的目录"，清理时说不清该不该留）。
    #[test]
    fn the_workspace_lives_beside_the_apps_own_data() {
        let status = desktop_workspace_status_from(std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper"));
        assert_eq!(
            status["workspaceDirectory"].as_str().unwrap(),
            format!(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\{DESKTOP_WORKSPACE_DIRECTORY_NAME}")
        );
        assert_eq!(status["dataDirectory"].as_str().unwrap(), r"C:\Users\me\AppData\Local\com.dsh.wallpaper");
        // 「清除全部用户数据」要删的那条凭据在凭据管理器里，不在文件系统上——把名字一并报出来。
        assert_eq!(status["credentialTarget"].as_str().unwrap(), "deepseek-api.dsh-wallpaper");
        // 打包前目录还不存在时也必须给出路径（桥会按同一条规则重建）。
        assert_eq!(status["workspaceExists"].as_bool(), Some(false));
        // 「项目记忆」的路径也要报出来（设置里那个「打开项目记忆」按钮用它），而且**必须与桥读的
        // 是同一个名字**：两处不同的话，用户在设置里打开的和助手实际用的是两个文件。
        assert_eq!(
            status["memoryFile"].as_str().unwrap(),
            format!(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\{DESKTOP_WORKSPACE_DIRECTORY_NAME}\{PROJECT_MEMORY_FILE_NAME}")
        );
        assert_eq!(status["memoryExists"].as_bool(), Some(false));
    }
}

/// 纯函数：只负责"路径怎么算"，好让测试直接钉住它（真实的数据目录是运行时才知道的）。
#[cfg(not(feature = "lite"))]
fn desktop_workspace_status_from(data_dir: &std::path::Path) -> serde_json::Value {
    let workspace = data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME);
    let memory = workspace.join(PROJECT_MEMORY_FILE_NAME);
    serde_json::json!({
        "dataDirectory": data_dir.display().to_string(),
        "workspaceDirectory": workspace.display().to_string(),
        "workspaceExists": workspace.is_dir(),
        // 助手维护的「项目记忆」（说话人格等长期要求就落在这里）；设置里给它一个打开入口，
        // 桌面会话里不贴路径。
        "memoryFile": memory.display().to_string(),
        "memoryExists": memory.is_file(),
        // 清除全部用户数据时要一并删掉的那条凭据（在凭据管理器里，不在文件系统上）。
        "credentialTarget": "deepseek-api.dsh-wallpaper",
    })
}

/// Set the API-key credential in Windows Credential Manager.
/// 把 API Key 写进当前用户的**通用凭据**（target / 用户名与 `keyring` 的 Windows 映射一致，
/// 所以 API 客户端读到的是同一条；keyring 的 `set_password` 把 UTF-16 小端字节当成 blob，
/// 这里必须写成同一个形状）。
#[cfg(all(windows, not(feature = "lite")))]
fn write_api_key_credential(key: &str) -> Result<(), String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::FILETIME,
            Security::Credentials::{
                CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC,
            },
        },
    };

    const CREDENTIAL_TARGET: &str = "deepseek-api.dsh-wallpaper";
    const CREDENTIAL_USERNAME: &str = "deepseek-api";

    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("API Key 不能为空。".into());
    }
    // CredUI 那条路的上限是 256 个 UTF-16 码元；这里直接输入也守同一个界，
    // 免得把一条明显不是 Key 的长文本写进凭据管理器。
    let mut units: Vec<u16> = trimmed.encode_utf16().collect();
    if units.len() > 256 {
        secure_zero_u16(&mut units);
        return Err("API Key 过长（最多 256 个字符）。".into());
    }
    let mut target = CREDENTIAL_TARGET.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut username = CREDENTIAL_USERNAME.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut blob = credential_blob_from_prompt_password(&units);
    let byte_len = u32::try_from(blob.len()).map_err(|_| "API Key 长度无效。".to_string())?;
    let mut credential = CREDENTIALW {
        Flags: CRED_FLAGS::default(),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        Comment: PWSTR::null(),
        LastWritten: FILETIME::default(),
        CredentialBlobSize: byte_len,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_ENTERPRISE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(username.as_mut_ptr()),
    };
    let result = unsafe { CredWriteW(&mut credential, 0) }
        .map_err(|error| format!("无法保存 API Key 到 Windows 凭据管理器：{error}"));
    secure_zero_bytes(&mut blob);
    secure_zero_u16(&mut units);
    secure_zero_u16(&mut target);
    secure_zero_u16(&mut username);
    result
}

/// 展示用的脱敏形式：只留头 3 与尾 4，中间一律替换。
///
/// 这是**唯一**被允许离开原生进程的 Key 形态——渲染端没有任何命令能把明文读回来，
/// 设置中心显示的也只是这个字符串。短 Key 不做"留一半"的漂亮处理：本来就短，全遮。
#[cfg(all(windows, not(feature = "lite")))]
fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    let len = chars.len();
    let tail = |count: usize| -> String { chars[len - count..].iter().collect() };
    match len {
        0 => String::new(),
        // 短到没什么可藏的：整串遮掉，别为了好看泄漏一半。
        1..=6 => "•".repeat(len),
        7..=12 => format!("{}{}", "•".repeat(len - 2), tail(2)),
        _ => format!("{}••••••••{}", chars[..3].iter().collect::<String>(), tail(4)),
    }
}

/// Stores the key the user typed in Settings. This is the one place a plaintext API key
/// crosses the Tauri IPC boundary, and it does so **once, on the way in**: the renderer
/// never receives a key back (`api_key_status` answers with `mask_api_key` only), and the
/// chat client reads the credential natively.
///
/// 这条路径取代了原来那个系统凭据对话框（`prompt_for_api_key`）：用户实测明确要求"在我们
/// 的设置窗内输入"，宁可放弃"明文从不经过渲染端"这条性质，换一个正常的输入体验。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn save_api_key(caller: tauri::WebviewWindow, key: String) -> Result<(), String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以更新 API Key。".into());
    }

    #[cfg(windows)]
    {
        write_api_key_credential(&key)
    }

    #[cfg(not(windows))]
    {
        let _ = key;
        Err("API Key 仅支持 Windows 凭据管理器。".into())
    }
}

/// Whether a key is stored, and the masked form to show in Settings.
///
/// 回答里**永远没有明文**：脱敏在原生侧做完，渲染端拿到的最多是一串点和首尾几个字符。
/// 没有 Key 时返回 `present: false`，而不是报错——"还没配"是正常状态。
#[cfg(all(windows, not(feature = "lite")))]
fn api_key_status_from_store() -> Result<serde_json::Value, String> {
    let entry = keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|_| "无法访问 Windows 凭据管理器。请检查系统凭据服务后重试。".to_string())?;
    match entry.get_password() {
        Ok(key) => Ok(serde_json::json!({ "present": true, "masked": mask_api_key(&key) })),
        Err(_) => Ok(serde_json::json!({ "present": false })),
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn api_key_status(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以读取 API Key 状态。".into());
    }

    #[cfg(windows)]
    {
        api_key_status_from_store()
    }

    #[cfg(not(windows))]
    {
        Ok(serde_json::json!({ "present": false }))
    }
}

#[cfg(all(test, windows))]
#[cfg(not(feature = "lite"))]
mod api_credential_tests {
    use super::{credential_blob_from_prompt_password, mask_api_key};

    #[test]
    fn serializes_the_same_utf16_little_endian_shape_as_keyring_windows() {
        assert_eq!(
            credential_blob_from_prompt_password(&[0x0073, 0x006B, 0x4F60]),
            vec![0x73, 0x00, 0x6B, 0x00, 0x60, 0x4F]
        );
    }

    /// 设置中心显示的是这个字符串——它必须"认得出是哪一条"，同时**不能**泄漏中间那段，
    /// 因为这是渲染端唯一能拿到的 Key 形态（没有任何命令能读回明文）。
    #[test]
    fn never_reveals_the_middle_of_a_stored_key() {
        assert_eq!(mask_api_key("sk-1234567890abcdef"), "sk-••••••••cdef");
        // 首尾照旧看得见，中间那段原样字符一个都不许出现。
        let masked = mask_api_key("sk-1234567890abcdef");
        assert!(!masked.contains("1234567890"));
        assert!(masked.ends_with("cdef"));
        // 前后空白不是 Key 的一部分。
        assert_eq!(mask_api_key("  sk-1234567890abcdef  "), "sk-••••••••cdef");
    }

    /// 短 Key 不玩"留一半"的漂亮处理：全遮，宁可少显示也不多泄漏。
    #[test]
    fn hides_short_keys_entirely_rather_than_half_of_them() {
        assert_eq!(mask_api_key(""), "");
        assert_eq!(mask_api_key("abc"), "•••");
        assert_eq!(mask_api_key("abcdef"), "••••••");
        // 再长一点只留最后两个字符。
        assert_eq!(mask_api_key("abcdefghi"), "•••••••hi");
    }

    /// 真机探针：设置中心那个「测试」按钮背后就是这一次调用（`/models` 要密钥，200 即密钥可用，
    /// 返回体就是可用模型列表）。忽略是刻意的：它要联网，而且依赖这台机器真的存了 Key。
    ///
    /// `cargo test --lib api_credential -- --ignored --nocapture`
    #[test]
    #[ignore = "calls the real DeepSeek API with this machine's stored key"]
    fn this_machine_reports_what_testing_the_stored_api_key_would_answer() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let status = crate::api_key_status_from_store();
        match &status {
            Ok(value) => eprintln!("stored key: {value}"),
            Err(error) => eprintln!("stored key unreadable: {error}"),
        }
        let answer = rt.block_on(crate::chat::api_models("https://api.deepseek.com".into()));
        match &answer {
            Ok(payload) => {
                let models = payload.get("models").and_then(serde_json::Value::as_array);
                eprintln!(
                    "models -> supported={} count={}",
                    payload.get("supported").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    models.map(Vec::len).unwrap_or(0)
                );
                for model in models.unwrap_or(&vec![]).iter().take(12) {
                    eprintln!("  {} / {}", model.get("id").and_then(serde_json::Value::as_str).unwrap_or("?"), model.get("name").and_then(serde_json::Value::as_str).unwrap_or("?"));
                }
            }
            Err(error) => eprintln!("models rejected -> {error}"),
        }
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn show_deepseek_login(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web::show_login(&app))
        .await
        .map_err(|error| format!("DeepSeek 应用内页面启动未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_ensure(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
) -> Result<(), String> {
    require_background(&caller)?;
    tauri::async_runtime::spawn_blocking(move || {
        deepseek_web::ensure(&app, conversation_id, new_conversation.unwrap_or(false))
    })
    .await
    .map_err(|error| format!("DeepSeek 网页预热未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web::WebStatus, String> {
    require_background(&caller)?;
    deepseek_web::status(&app).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_history(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
) -> Result<deepseek_web::WebHistory, String> {
    require_background(&caller)?;
    deepseek_web::history(&app, conversation_id, new_conversation.unwrap_or(false)).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn deepseek_web_adapter_config_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web_config::status(&app))
        .await
        .map_err(|error| format!("读取 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn open_deepseek_web_adapter_config(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web_config::open(&app))
        .await
        .map_err(|error| format!("打开 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn reset_deepseek_web_adapter_config(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || deepseek_web_config::reset(&app))
        .await
        .map_err(|error| format!("恢复 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
fn start_settings_drag(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_settings(&caller)?;
    app.get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_settings_window(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    require_settings(&caller)?;
    let window = app
        .get_webview_window(SETTINGS_WINDOW_LABEL)
        .ok_or_else(|| "settings window missing".to_string())?;
    if is_lite_edition() {
        window.destroy().map_err(|error| error.to_string())
    } else {
        window.hide().map_err(|error| error.to_string())
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn begin_interaction_region_session(caller: tauri::WebviewWindow) -> Result<u64, String> {
    require_background(&caller)?;
    windows_integration::begin_interaction_region_session()
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn update_interaction_regions(
    caller: tauri::WebviewWindow,
    regions: Vec<windows_integration::InteractionRegionInput>,
    scale_factor: f64,
    session: u64,
    revision: u64,
) -> Result<windows_integration::InteractionRegionUpdateResult, String> {
    require_background(&caller)?;
    windows_integration::update_interaction_regions(regions, scale_factor, session, revision)
}

/// 单击悬浮球：进入里桌面（并把球收回）。
///
/// 这是 §1.1 拍板的「表桌面单击胶囊 = 进入里桌面 + 弹出输入岛」的原生那一半：
/// 隐藏 Explorer 的图标层并发出 `desktop-workspace-toggle` 的 `"enter"`，
/// `App.tsx` 收到后会展开输入岛并打开对话。**与桌面空白双击共用同一个实现**
/// （`windows_integration::enter_inner_workspace`），两条入场路径不会分叉。
///
/// 球的收回走请求位，由监控线程在下一拍执行：窗口只由一个线程移动。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn enter_inner_workspace_from_ball(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_ball(&caller)?;
    windows_integration::enter_inner_workspace(&app)?;
    floating_ball::request_ball_retract();
    Ok(())
}

/// 离开里桌面：输入岛右上角的「X」走这条命令。
///
/// **必须落回原生**，因为"现在在不在里桌面"是原生的事实（`INNER_WORKSPACE_ACTIVE`）：
/// 前端自己把界面搬回表桌面，原生那个事实会原地不动——实测后果很实在，点 X 之后
/// 悬浮球再也弹不出来、点球也唤不起输入岛（`enter_inner_workspace` 看到"已经在里桌面"
/// 就直接返回，`desktop-workspace-toggle` 的 `enter` 事件根本不会发出去）。
///
/// 与桌面空白双击那条路**共用同一个实现**，状态迁移也就只有一份。
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn leave_inner_workspace(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_background(&caller)?;
    windows_integration::leave_inner_workspace(&app)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn desktop_layout_metrics(
    caller: tauri::WebviewWindow,
    display_id: Option<String>,
) -> Result<windows_integration::DesktopLayoutMetrics, String> {
    require_background(&caller)?;
    Ok(windows_integration::desktop_layout_metrics(
        &caller,
        display_id.as_deref(),
    ))
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn send_chat(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    web_state: tauri::State<'_, deepseek_web::DeepSeekWebState>,
    mode: String,
    text: String,
    conversation_id: Option<String>,
    request_id: Option<String>,
    new_conversation: Option<bool>,
    base_url: Option<String>,
    model: Option<String>,
    price_input_per_million: Option<f64>,
    price_output_per_million: Option<f64>,
) -> Result<Option<String>, String> {
    require_background(&caller)?;
    match mode.as_str() {
        "deepseek-web" => deepseek_web::send(
            app,
            web_state.inner(),
            text,
            conversation_id,
            request_id,
            new_conversation.unwrap_or(false),
        )
        .await
        .map(Some),
        "deepseek-api" => chat::send_api(
            app,
            state,
            text,
            base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            model.unwrap_or_else(|| "deepseek-chat".into()),
            conversation_id,
            request_id,
            price_input_per_million,
            price_output_per_million,
        )
        .await
        .map(Some),
        "harness" => chat::harness_send(state, text).await.map(|_| None),
        _ => Err("DeepSeek 网页桥接需要登录 WebView 和 DOM adapter".into()),
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn api_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    conversation_id: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::api_history(state.inner(), &conversation_id, limit)
}

/// List the durable API transcripts so the settings center can offer a delete
/// control. Returns metadata only; message bodies stay behind `api_history`.
///
/// The settings WebView is a separate process with an isolated storage
/// partition, so its in-memory archive is a snapshot from its own start. The
/// listing therefore reads the archive through the store rather than reporting
/// that snapshot.
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn list_api_conversations(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_api_history_owner(&caller)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || chat::list_api_conversations(&state))
        .await
        .map_err(|_| "读取 API 会话列表失败".to_string())?
}

/// Deleting durable API history touches the DPAPI archive, which is a file
/// write plus a cross-process mutex wait. Keep it off the WebView command
/// thread so a slow disk cannot freeze the calling surface.
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn delete_api_conversation(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    conversation_id: String,
) -> Result<bool, String> {
    require_api_history_owner(&caller)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        chat::delete_api_conversation(&state, &conversation_id)
    })
    .await
    .map_err(|_| "删除 API 会话记录失败".to_string())?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn clear_api_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<usize, String> {
    require_api_history_owner(&caller)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || chat::clear_api_history(&state))
        .await
        .map_err(|_| "清空 API 会话记录失败".to_string())?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn cancel_chat(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    web_state: tauri::State<'_, deepseek_web::DeepSeekWebState>,
    mode: String,
) -> Result<(), String> {
    require_background(&caller)?;
    if mode == "deepseek-web" {
        deepseek_web::cancel(&app, web_state.inner()).await
    } else if mode == "harness" {
        chat::harness_cancel(state).await
    } else {
        chat::cancel_api(&state);
        Ok(())
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn connect_harness(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    resume_session_id: Option<String>,
    connection_id: String,
    model: Option<String>,
    preset: Option<String>,
) -> Result<String, String> {
    require_background(&caller)?;
    // The session follows the endpoint selected in settings, resolved here rather
    // than trusted from the caller: the renderer cannot name an arbitrary port,
    // and the monitor reads the same value, so status and sessions cannot
    // disagree about which client is in use.
    let endpoint_port = Some(harness_endpoint_port());
    chat::harness_connect(app, state, resume_session_id, connection_id, model, endpoint_port, preset).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_history(state).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_presets(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_presets(state).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_set_preset(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    preset: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_set_preset(state, preset).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_controls(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_controls(state).await
}

/// 枚举当前 Harness 主体（检出 / 桌面 / 官壳）可用的模型。
///
/// 名单由宿主提供、经桥接转发；宿主没有该能力时桥接报 `supported: false`，
/// 前端据此退化成"只显示当前模型"，而不是摆一串会被拒绝的 id。
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_models(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_models(state).await
}

/// 把壁纸选定的模型推给 Harness 宿主，使宿主默认模型与壁纸一致。
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_set_model(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    model: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_set_model(state, model).await
}

/// 枚举 DeepSeek API 端点自己列出的模型（兼容接口的 `/models`）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn api_models(caller: tauri::WebviewWindow, base_url: String) -> Result<serde_json::Value, String> {
    // 两个界面都要用这一份目录：壁纸端的模型选择器（Harness 兼容端点）与设置中心的
    // 「测试／刷新」（用户要在那里看到"现在有哪些模型"）。**不是**放宽给球或网页窗口——
    // 它仍然只认这两个表面，`require_wallpaper_surface` 就是这条边界。
    require_wallpaper_surface(&caller)?;
    chat::api_models(base_url).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn harness_set_permission(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    permission: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_set_permission(state, permission).await
}

#[cfg(not(feature = "lite"))]
static HARNESS_STATUS_CACHE: OnceLock<RwLock<serde_json::Value>> = OnceLock::new();
#[cfg(not(feature = "lite"))]
static HARNESS_PROBE_CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();

/// The wallpaper bridge exposes a small, versioned protocol of its own.  A
/// listening service on port 3080 is not sufficient proof that it is our
/// bridge: it may be DSH's regular web UI, an older bridge, or another local
/// service altogether.
#[cfg(not(feature = "lite"))]
const HARNESS_BRIDGE_PROTOCOL_VERSION: u64 = 1;
#[cfg(not(feature = "lite"))]
const REQUIRED_HARNESS_BRIDGE_CAPABILITIES: &[&str] =
    &["sessions", "history", "sse", "cancel", "approval-handoff"];

/// The DSH client shapes listen on different ports, and only some of them are
/// configurable, so probing one hardcoded port means "only ever connect to the CLI
/// shape". The official desktop shell compiles 19387 into its asar; the CLI and the
/// official web app both default to 3080 (`ctx.webStartup.port ?? 3080`).
///
/// Order is the shipped priority: official desktop, then plain web/CLI. The renderer
/// applies the same order, and `wallpaper/src/connect/endpoints.ts` documents it.
///
/// （第三方的 43120 2026-09-27 按用户要求移除：它把本地接口锁在自己的授权后面，壁纸一律 403 ✓。）
#[cfg(not(feature = "lite"))]
const HARNESS_ENDPOINT_PORTS: &[(u16, &str)] =
    &[(19387, "official-desktop"), (3080, "official-web")];

/// Port used when a caller does not name one. Matches DSH's own web default, so a
/// CLI-started Host is found without configuration.
#[cfg(not(feature = "lite"))]
const HARNESS_DEFAULT_PORT: u16 = 3080;

/// Which endpoints the wallpaper may talk to, and which one it is using.
///
/// Three facts that used to be two, and the missing one was the cause of a real
/// bug: `pinned` is the user's explicit choice, `subject` is the execution subject
/// the settings configure, and `active` is whatever auto mode last found. Without
/// `subject` the monitor probed the shipped priority order and would connect to
/// whichever client happened to answer — i.e. silently answer a session for a
/// subject the user never chose (the frozen rule: 「无论A是怎么死的，都不允许静默用B
/// 来替换A」).
///
/// `subject` is a whole record rather than a port list because an **empty** list is
/// meaningful: the subject is configured, this build just cannot say where it
/// answers. Treating that as "nothing configured" would put the wallpaper back on
/// the priority order, which is precisely the substitution being prevented.
#[cfg(not(feature = "lite"))]
#[derive(Default)]
struct HarnessEndpointState {
    /// The user's choice from settings, kept until they change it.
    pinned: Mutex<Option<u16>>,
    /// The configured execution subject and the ports it may answer on.
    subject: Mutex<Option<HarnessEndpointSubject>>,
    /// The endpoint auto mode last confirmed, used while nothing is configured.
    active: Mutex<Option<u16>>,
}

#[cfg(not(feature = "lite"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct HarnessEndpointSubject {
    /// The id the settings store (`shell:<aumid>` or a checkout root path).
    id: String,
    /// Every port it may be reached on, in the subject's own order.
    ports: Vec<u16>,
}

#[cfg(not(feature = "lite"))]
static HARNESS_ENDPOINT_STATE: OnceLock<HarnessEndpointState> = OnceLock::new();

#[cfg(not(feature = "lite"))]
fn harness_endpoint_state() -> &'static HarnessEndpointState {
    HARNESS_ENDPOINT_STATE.get_or_init(HarnessEndpointState::default)
}

#[cfg(not(feature = "lite"))]
fn locked<T>(value: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
    value.lock().ok()
}

/// 主体范围的稳定标识：钉住的端点 + "哪个主体、允许哪些端口"。
///
/// 探测器靠它回答一个问题：**这一轮探的还是同一个对象吗？**
///
/// 缺了它，`connected_once` 会跨主体保留。实测症状正是如此：在 CLI（3080）上连过一次之后切到
/// 官壳，官壳那边没有人在听，可"曾经连上过"这条记忆还在、又拿不到"属主已死"的证据 ⇒ 只把灯
/// 变成呼吸态、`available` 仍是 BridgeReady ⇒ 界面一直写着"DSH Bridge 已连接"，而一发消息
/// 就被告知会话没有建立。换了主体，"连接"这个词说的就是另一个对象了。
#[cfg(not(feature = "lite"))]
fn harness_scope_key() -> String {
    let state = harness_endpoint_state();
    let pinned = locked(&state.pinned).and_then(|guard| *guard);
    match locked(&state.subject).and_then(|guard| guard.clone()) {
        Some(subject) => format!("{pinned:?}|{}|{:?}", subject.id, subject.ports),
        None => format!("{pinned:?}|"),
    }
}

/// 主体范围变化时的叫醒通道。
///
/// 不加这个，探测器只在自己排定的 tick 上醒来 —— 日志实测：切换发生在 01:10:13，`connecting`
/// 到 01:10:15 才发布（界面这两秒仍写着"已连接"），结论又等到 01:10:23 才发出（灰灯迟了 8 秒）。
/// 进程退出那条路早就有一条一模一样的叫醒通道（`exits`），主体变化凭什么要等。
#[cfg(not(feature = "lite"))]
fn harness_scope_notify() -> &'static tokio::sync::Notify {
    static NOTIFY: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    NOTIFY.get_or_init(tokio::sync::Notify::new)
}

/// The ports the wallpaper may probe, in order — never widened by what answers.
///
/// The order carries the "connected ⇒ sticky" rule as well: the endpoint already
/// in use leads the list, so a later probe keeps talking to the same instance
/// instead of re-deciding on every tick.
#[cfg(not(feature = "lite"))]
fn harness_endpoint_candidates() -> Vec<u16> {
    let state = harness_endpoint_state();
    if let Some(port) = locked(&state.pinned).and_then(|guard| *guard) {
        // An explicit pin is the user's own statement about where their DSH is, so
        // it is the whole candidate list — honoured even while nothing answers.
        return vec![port];
    }
    if let Some(subject) = locked(&state.subject).and_then(|guard| guard.clone()) {
        let sticky = locked(&state.active).and_then(|guard| *guard);
        let mut ordered: Vec<u16> = Vec::with_capacity(subject.ports.len());
        if let Some(port) = sticky {
            if subject.ports.contains(&port) {
                ordered.push(port);
            }
        }
        for port in &subject.ports {
            if !ordered.contains(port) {
                ordered.push(*port);
            }
        }
        return ordered;
    }
    // Nothing configured yet: the shipped priority order, with whatever auto mode
    // is already using first.
    let mut ordered: Vec<u16> = locked(&state.active).and_then(|guard| *guard).into_iter().collect();
    for (port, _kind) in HARNESS_ENDPOINT_PORTS {
        if !ordered.contains(port) {
            ordered.push(*port);
        }
    }
    ordered
}

/// Whether the settings name where the wallpaper may connect at all.
///
/// The candidate list cannot answer this on its own: an *empty* list means "the
/// subject is configured but this build cannot place it", while nothing configured
/// yields the shipped order. Only this call separates the two, and it is what
/// decides whether the reason the user reads is `subject-offline` or `no-endpoint`.
#[cfg(not(feature = "lite"))]
fn harness_endpoint_configured() -> bool {
    let state = harness_endpoint_state();
    if locked(&state.pinned).map(|guard| guard.is_some()).unwrap_or(true) {
        return true;
    }
    locked(&state.subject)
        .map(|guard| guard.is_some())
        .unwrap_or(true)
}

/// The port the wallpaper should talk to.
///
/// Among the permitted endpoints, one that is actually listening wins, then the
/// first permitted one. Never a port outside the permitted set, so this cannot
/// hand a session to another subject; within one subject's own ports it prefers the
/// live instance, which is what keeps a CLI-started Host on 3080 working with no
/// configuration at all.
#[cfg(not(feature = "lite"))]
fn harness_endpoint_port() -> u16 {
    let candidates = harness_endpoint_candidates();
    candidates
        .iter()
        .copied()
        .find(|port| client_window::endpoint_is_listening(*port))
        .or_else(|| candidates.first().copied())
        .unwrap_or(HARNESS_DEFAULT_PORT)
}

/// Record the endpoint a successful probe used.
///
/// Sticky, not "discovered": it never widens the permitted set, it only remembers
/// which permitted port answered. In the subject case that is the whole of
/// "connected ⇒ sticky"; in auto mode it is the same preference applied to the
/// shipped order, and a pin is a choice rather than an observation, so nothing is
/// recorded for it.
#[cfg(not(feature = "lite"))]
fn note_endpoint_in_use(port: u16) {
    let state = harness_endpoint_state();
    if locked(&state.pinned).map(|guard| guard.is_some()).unwrap_or(true) {
        return;
    }
    if let Ok(mut guard) = state.active.lock() {
        *guard = Some(port);
    }
}

/// Publish the endpoint scope the settings configure.
///
/// One function for the whole decision, because the three facts are one decision: a
/// partial update would leave the monitor probing a port from a subject the user has
/// already left. Changing the subject clears the recorded endpoint, because that is
/// a real change of subject rather than a substitution — the one moment the
/// wallpaper is allowed to move.
#[cfg(not(feature = "lite"))]
fn apply_endpoint_scope(
    state: &HarnessEndpointState,
    port: Option<u16>,
    subject_id: Option<&str>,
    extra_ports: &[u16],
    args: Option<&[String]>,
) -> Result<serde_json::Value, String> {
    {
        let mut guard = state.pinned.lock().map_err(|_| "接入端点状态不可用".to_string())?;
        *guard = port;
    }
    let subject_id = subject_id.unwrap_or_default().trim().to_string();
    // 「启动参数」里点名的端口属于**这个主体**（我们就是这样启动它的），所以它进的是主体的端口
    // 表；并行实例的第二个因此会被探针看见，而不是永远显示成离线。
    let declared = args.and_then(harness_launch::port_from_args);
    let ports = harness_targets::subject_endpoint_ports(&subject_id, extra_ports, declared);
    let previous = {
        let mut guard = state
            .subject
            .lock()
            .map_err(|_| "接入端点状态不可用".to_string())?;
        let previous = guard.clone();
        *guard = if subject_id.is_empty() {
            None
        } else {
            Some(HarnessEndpointSubject {
                id: subject_id,
                ports,
            })
        };
        previous
    };
    let next = state
        .subject
        .lock()
        .map_err(|_| "接入端点状态不可用".to_string())?
        .clone();
    if previous != next {
        if let Ok(mut guard) = state.active.lock() {
            *guard = None;
        }
        // 立刻叫醒探测器：切换主体这一步是用户动作，界面必须在**这一拍**进入"连接中"，
        // 而不是等下一次排定的探测（实测迟 2–5 秒，用户看到的是切换后仍写着"已连接"）。
        harness_scope_notify().notify_one();
    }
    Ok(serde_json::json!({
        "port": port,
        "subjectId": next.as_ref().map(|subject| subject.id.clone()),
        "subjectPorts": next.map(|subject| subject.ports).unwrap_or_default(),
    }))
}

#[cfg(not(feature = "lite"))]
fn harness_status_cache() -> &'static RwLock<serde_json::Value> {
    HARNESS_STATUS_CACHE
        .get_or_init(|| RwLock::new(serde_json::json!({ "availability": "offline" })))
}

#[cfg(not(feature = "lite"))]
fn harness_probe_client() -> Option<&'static reqwest::Client> {
    HARNESS_PROBE_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                // Status probing decides whether the UI exposes Harness
                // mode. Keep it on loopback even when the user has a system
                // proxy configured for the DeepSeek web route.
                .no_proxy()
                .timeout(std::time::Duration::from_millis(1200))
                .build()
                .ok()
        })
        .as_ref()
}

/// Convert a bridge status document into the small status shape exposed to
/// the WebView.  This is intentionally fail-closed: only a bridge that speaks
/// the fresh-session protocol we need can enable Harness mode. `resume` is an
/// optional DSH persistence feature and cannot be required for a new session.
#[cfg(not(feature = "lite"))]
fn compatible_harness_bridge_status(data: &serde_json::Value) -> Option<serde_json::Value> {
    let status = diagnose_harness_bridge_status(data)?;
    if status
        .get("availability")
        .and_then(serde_json::Value::as_str)
        == Some("bridge-ready")
    {
        Some(status)
    } else {
        None
    }
}

/// Interpret one Bridge `/status` document into a diagnostic state.
///
/// This is the native mirror of the renderer's `interpretHarnessBridgeStatus`
/// and must agree with it: both sides exist (the renderer needs a browser
/// preview fallback) and a divergence would show the user two different
/// explanations of one failure.
///
/// Returns `None` only when the document is not a Bridge status at all, which
/// is what lets the caller fall back to the root-page probe. A Bridge that
/// answers but is unusable yields a *state*, never `None`: "the Bridge is still
/// loading", "its token is unavailable" and "it is not the Bridge we expect"
/// need three different user actions, and the previous boolean answer collapsed
/// all of them into `web-only`.
#[cfg(not(feature = "lite"))]
fn diagnose_harness_bridge_status(data: &serde_json::Value) -> Option<serde_json::Value> {
    let protocol_version = data
        .get("protocolVersion")
        .and_then(serde_json::Value::as_u64)?;
    if data.get("dsh").and_then(serde_json::Value::as_str) != Some("online") {
        return None;
    }

    let optional_string = |field: &str| -> Option<String> {
        data.get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    let mut status = serde_json::Map::new();
    // These are informational only.  Do not let malformed optional metadata
    // make a diagnostic unusable, or expose non-string JSON to the UI.
    for field in [
        "bridgeVersion",
        "bridgeBuild",
        "model",
        "provider",
        "reasoningEffort",
    ] {
        if let Some(value) = optional_string(field) {
            status.insert(field.into(), serde_json::Value::String(value));
        }
    }
    status.insert("protocolVersion".into(), serde_json::json!(protocol_version));

    let set_state = |status: &mut serde_json::Map<String, serde_json::Value>,
                     availability: &str,
                     reason_code: &str| {
        status.insert(
            "availability".into(),
            serde_json::Value::String(availability.into()),
        );
        status.insert(
            "reasonCode".into(),
            serde_json::Value::String(reason_code.into()),
        );
    };

    if protocol_version != HARNESS_BRIDGE_PROTOCOL_VERSION {
        set_state(&mut status, "bridge-incompatible", "protocol-version-mismatch");
        return Some(serde_json::Value::Object(status));
    }
    if data
        .get("authentication")
        .and_then(serde_json::Value::as_str)
        != Some("ready")
    {
        // The reason vocabulary belongs to the consumer, not to the payload: a
        // Bridge reporting `state: bridge-ready` while its token is missing must
        // not have that stale `reasonCode` describe the failure.
        set_state(&mut status, "bridge-auth-unavailable", "token-unavailable");
        return Some(serde_json::Value::Object(status));
    }

    // A malformed or absent capability list is unsafe to read as "no
    // capabilities". The document identified itself as a Bridge, so the honest
    // answer is that it is incompatible, not that it does not exist.
    let capability_list: Option<Vec<&str>> = data
        .get("capabilities")
        .and_then(serde_json::Value::as_array)
        .filter(|capabilities| capabilities.iter().all(serde_json::Value::is_string))
        .map(|capabilities| capabilities.iter().filter_map(|v| v.as_str()).collect());
    let Some(capability_list) = capability_list else {
        set_state(&mut status, "bridge-incompatible", "capabilities-missing");
        return Some(serde_json::Value::Object(status));
    };
    let has_every_capability = REQUIRED_HARNESS_BRIDGE_CAPABILITIES
        .iter()
        .all(|required| capability_list.contains(required));
    if has_every_capability {
        status.insert(
            "availability".into(),
            serde_json::Value::String("bridge-ready".into()),
        );
        return Some(serde_json::Value::Object(status));
    }

    // A Bridge that names itself as still composing its service set is loading;
    // waiting is the right action. Anything else that lacks a required
    // capability cannot be fixed by waiting, so it is incompatible.
    if optional_string("state").as_deref() == Some("bridge-loading") {
        // The loading reason vocabulary is closed and owned by the consumer, so
        // a stale payload reason (for example `ready` from an earlier status)
        // can never be rendered as the explanation for a wait.
        let reason = optional_string("reasonCode")
            .filter(|value| value == "services-pending" || value.starts_with("waiting:"))
            .unwrap_or_else(|| "services-pending".into());
        set_state(&mut status, "bridge-loading", &reason);
    } else {
        // A host whose *shape* did not match carries the member name after the
        // colon, and it is worth forwarding: it tells the user which DSH service
        // to report, and it is a compile-time identifier rather than host data.
        // Any other reason is not forwarded, because the rest of the vocabulary
        // is about waiting and would be misleading here.
        let reason = optional_string("reasonCode")
            .filter(|value| value.starts_with("host-shape-mismatch:"))
            .unwrap_or_else(|| "capabilities-missing".into());
        set_state(&mut status, "bridge-incompatible", &reason);
    }
    Some(serde_json::Value::Object(status))
}

#[cfg(not(feature = "lite"))]
async fn fetch_harness_status_at(port: u16) -> serde_json::Value {
    let Some(client) = harness_probe_client() else {
        return serde_json::json!({ "availability": "offline" });
    };
    if let Ok(response) = client
        .get(format!(
            "http://127.0.0.1:{port}/api/wallpaper/v1/status"
        ))
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(data) = response.json::<serde_json::Value>().await {
                // Keep the Bridge's own diagnosis: it is strictly more
                // specific than a root-page guess, so never downgrade it.
                if let Some(status) = diagnose_harness_bridge_status(&data) {
                    return status;
                }
            }
        }
    }
    let root_status = client
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .ok()
        .map(|response| response.status());
    match root_probe_availability(root_status) {
        // A root endpoint proves only that an HTTP service accepted a
        // successful request. Authentication failures and error pages must
        // not turn an unrelated process on this port into a misleading “DSH
        // online” state.
        HarnessAvailability::WebOnly => serde_json::json!({
            "availability": "web-only",
            "reasonCode": "bridge-status-missing",
        }),
        _ => serde_json::json!({ "availability": "offline" }),
    }
}

/// Probe every known endpoint and report which ones host a Bridge.
///
/// The native side scans independently of the renderer because the probe cache
/// the wallpaper actually renders from is native: without this, a Bridge on
/// 19387 would still be invisible to the status the desktop shows.
#[cfg(not(feature = "lite"))]
async fn scan_harness_endpoints(extra_ports: Vec<u16>) -> serde_json::Value {
    let mut plan: Vec<(u16, &str, &str)> = HARNESS_ENDPOINT_PORTS
        .iter()
        .map(|(port, kind)| (*port, *kind, "default"))
        .collect();
    for port in extra_ports {
        if port == 0 || plan.iter().any(|(known, _, _)| *known == port) {
            continue;
        }
        plan.push((port, "official-web", "user"));
    }

    let mut found = Vec::new();
    for (port, kind, source) in plan {
        let status = fetch_harness_status_at(port).await;
        let availability = status
            .get("availability")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("offline");
        // `web-only`/`offline` mean no Bridge answered, so they are not offered
        // as endpoints. They are still reported so the UI can say why a port was
        // skipped.
        let bridge_found = availability != "offline" && availability != "web-only";
        found.push(serde_json::json!({
            "port": port,
            "kind": kind,
            "source": source,
            "bridgeFound": bridge_found,
            "status": status,
        }));
    }
    serde_json::Value::Array(found)
}

#[cfg(not(feature = "lite"))]
async fn fetch_harness_status() -> serde_json::Value {
    fetch_harness_status_at(harness_endpoint_port()).await
}

/// Verify that the renderer's island `pointerdown` really is a user click.
///
/// The native click route is closed - a real click reaches neither `WM_MOUSEACTIVATE` nor
/// `WM_LBUTTONDOWN` nor a hit test, because the WebView2 child owns the mouse messages from
/// another process (see `docs/evidence/input-island-focus-native-route-closed.md`). The island event
/// therefore has to be reported by the renderer, and plan 3.A requires the native side not to
/// trust that report: it re-checks the physical left button and the window under the cursor.
///
/// Restricted to the wallpaper surface, so neither the Lite surface nor a remote page can ask
/// for it. It performs no activation yet - this is the diagnostic step of plan 3.A.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn verify_island_click(app: tauri::AppHandle, caller: tauri::WebviewWindow) -> Result<String, String> {
    require_wallpaper_surface(&caller)?;
    // Log both outcomes. Logging only the success made a rejected report
    // indistinguishable from a report that was never sent, which is exactly the
    // ambiguity the repair plan forbids swallowing.
    let verdict = match windows_integration::verify_island_click() {
        Ok(verdict) => verdict,
        Err(error) => {
            log::warn!("island click report rejected: {error}");
            return Err(error);
        }
    };
    log::info!("{verdict}");
    // A verified click is the authorisation: a genuine input event lifts the foreground
    // lock, which is what makes the handover permissible, and why it is bound to a click
    // and never called on its own.
    windows_integration::hand_over_keyboard_for_app(&app);
    Ok(verdict)
}

/// Publish which endpoints the wallpaper may talk to.
///
/// The settings are the authority, and this is how they reach the monitor that
/// actually probes: the subject id, the user's explicit pin, and the ports they
/// added by hand. All three arrive together because they are one decision, and a
/// partial update would leave the monitor on an endpoint belonging to a subject the
/// user has already left.
///
/// Without the subject the monitor only knew ports, so it probed the shipped
/// priority order and connected to whichever client answered — the silent
/// substitution of one subject for another that the frozen rule forbids. Validation
/// stays strict: a port outside the TCP range is rejected rather than silently
/// dropped, so a bad value is visible instead of looking like "no Bridge".
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn set_harness_endpoint(
    caller: tauri::WebviewWindow,
    port: Option<u16>,
    subject_id: Option<String>,
    extra_ports: Option<Vec<u16>>,
    args: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    require_wallpaper_surface(&caller)?;
    if port == Some(0) {
        return Err("接入端点端口必须在 1-65535 之间".into());
    }
    let args = harness_launch::normalize_launch_args(args)?;
    let state = harness_endpoint_state();
    apply_endpoint_scope(
        state,
        port,
        subject_id.as_deref(),
        &extra_ports.unwrap_or_default(),
        Some(&args),
    )
}

/// A root-page response is diagnostic only. It is deliberately not part of
/// the compatible Bridge validation above: any non-2xx response (including a
/// gateway, auth challenge, or unrelated service error) must be treated as
/// offline rather than an apparently usable local DSH instance.
#[cfg(not(feature = "lite"))]
fn root_probe_availability(status: Option<reqwest::StatusCode>) -> HarnessAvailability {
    match status {
        Some(status) if status.is_success() => HarnessAvailability::WebOnly,
        _ => HarnessAvailability::Offline,
    }
}

#[cfg(test)]
#[cfg(not(feature = "lite"))]
mod dsh_autostart_tests {
    use super::{
        classify_dsh_launch_failure, merge_managed_instances, resolve_dsh_launcher, stop_targets,
        ManagedDshInstance,
    };
    use crate::harness_launch::{self, ManagedChild};

    fn instance(instance_key: &str, subject_id: &str, pid: u32, port: Option<u16>) -> ManagedDshInstance {
        ManagedDshInstance {
            instance_key: instance_key.into(),
            subject_id: subject_id.into(),
            port,
            pid,
            root_path: Some(subject_id.into()),
            profile: Some("desktop".into()),
            args: Vec::new(),
        }
    }

    fn recorded(instance_key: &str, subject_id: &str, pid: u32, port: Option<u16>) -> ManagedChild {
        ManagedChild {
            instance_key: instance_key.into(),
            subject_id: subject_id.into(),
            args: Vec::new(),
            pid,
            started_at: Some(1),
            port,
            handoff: None,
        }
    }

    /// Every launch failure must map to the code whose message tells the user
    /// what to fix. The substrings are the ones the error strings actually carry.
    #[test]
    fn launch_failures_map_to_their_stable_codes() {
        assert_eq!(classify_dsh_launch_failure("DSH 根目录不存在或不可访问"), "root-path-invalid");
        assert_eq!(
            classify_dsh_launch_failure("选择的目录不是可识别的 DSH 项目根目录"),
            "root-path-invalid"
        );
        assert_eq!(classify_dsh_launch_failure("未找到 Node.js。请确认 node.exe 已加入系统 PATH"), "launcher-missing");
        assert_eq!(classify_dsh_launch_failure("未找到 pnpm。请确认 pnpm.cmd 已加入系统 PATH"), "launcher-missing");
        assert_eq!(classify_dsh_launch_failure("DSH profile 只能包含字母、数字、连字符或下划线"), "profile-invalid");
        // 端口占用读的是"这件事"，不是某一个数字：并行实例换到 3081 之后，同一个失败仍要报成
        // "端口被占"，而不是最没法照做的"进程启动失败"。
        assert_eq!(classify_dsh_launch_failure("本机 3080 端口已被其他进程占用"), "port-occupied-external");
        assert_eq!(classify_dsh_launch_failure("本机 3081 端口已被其他进程占用"), "port-occupied-external");
        assert_eq!(classify_dsh_launch_failure("启动参数最多 32 个"), "launch-args-invalid");
        assert_eq!(classify_dsh_launch_failure("单个启动参数不能超过 512 个字符"), "launch-args-invalid");
        // Anything unrecognised still yields a code rather than leaking the text.
        let other = classify_dsh_launch_failure("something unexpected: 0x80070005");
        assert_eq!(other, "spawn-failed");
        assert!(!other.contains("0x80070005"));
    }

    /// Launcher resolution decides what actually gets executed, so the two input
    /// shapes are pinned: a path is taken as a path, and a bare name is only
    /// accepted when it exists on `PATH` or in a well-known install location.
    #[test]
    fn launcher_resolution_handles_paths_and_bare_names() {
        // A path-shaped value that does not exist must not resolve.
        assert!(resolve_dsh_launcher(r"C:\definitely\missing\node.exe").is_none());
        assert!(resolve_dsh_launcher("definitely-not-a-real-launcher-xyz.exe").is_none());
        // A path-shaped value that does exist must resolve to exactly it.
        let self_exe = std::env::current_exe().expect("current exe");
        let resolved = resolve_dsh_launcher(&self_exe.to_string_lossy()).expect("resolves");
        assert_eq!(resolved, self_exe);
        // A bare name is not treated as a relative path.
        assert!(!resolve_dsh_launcher("node.exe")
            .map(|path| path == std::path::PathBuf::from("node.exe"))
            .unwrap_or(false));
    }

    /// 停止的粒度：点名停一个、不带名字停全部，而且官壳在任何一条路上都停不了。
    #[test]
    fn stopping_names_one_instance_or_all_of_them_and_never_the_shell() {
        let tree = r"D:\Family\DeepSeekHarness\deepseek-harness";
        let second = harness_launch::instance_key(tree, &["--port".to_string(), "3081".to_string()]);
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        let owned = vec![
            recorded(tree, tree, 111, Some(3080)),
            recorded(&second, tree, 222, Some(3081)),
            recorded(cli, cli, 333, Some(3080)),
            // 官壳那条记录是给门票用的，它混在"我们记着的东西"里是**正常的** —— 而它不该出现在
            // 任何一个停止名单里。
            recorded("shell:com.deepseek.dsh", "shell:com.deepseek.dsh", 444, Some(19387)),
        ];
        // 点名一个：就只有那一个。3080 上那个还在服务时，停 3081 不该顺手把 3080 也带走。
        assert_eq!(stop_targets(Some(second.clone()), owned.clone()).unwrap(), vec![second.clone()]);
        assert_eq!(
            stop_targets(Some(tree.to_string()), owned.clone()).unwrap(),
            vec![tree.to_string()]
        );
        // 不点名：本应用启动的每一个（两个实例 + CLI），官壳不在里面。
        let all = stop_targets(None, owned.clone()).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all.contains(&second));
        assert!(all.contains(&tree.to_string()));
        assert!(all.contains(&cli.to_string()));
        assert!(!all.iter().any(|key| key.starts_with("shell:")), "the official shell is never ours to stop");
        // 点名官壳 ⇒ **明确拒绝**，而不是静默成功：静默成功会让界面刷新出一个"已经停了"的假象，
        // 而真相是用户自己的客户端还开着。
        let refusal = stop_targets(Some("shell:com.deepseek.dsh".to_string()), owned.clone()).unwrap_err();
        assert!(refusal.contains("不由本应用管理"), "{refusal}");
        // 只有官壳可停时，"全部"是空名单 —— 一个都不动，也不报错。
        assert!(stop_targets(None, vec![recorded("shell:com.deepseek.dsh", "shell:com.deepseek.dsh", 1, None)])
            .unwrap()
            .is_empty());
    }

    /// 用户要看的那份清单：**同一个主体的两个实例都在**，而且官壳永远不在。
    #[test]
    fn the_status_list_reports_every_instance_this_app_started() {
        let tree = r"D:\Family\DeepSeekHarness\deepseek-harness";
        let second_key = harness_launch::instance_key(tree, &["--port".to_string(), "3081".to_string()]);
        let merged = merge_managed_instances(
            vec![
                instance(tree, tree, 111, Some(3080)),
                instance(&second_key, tree, 222, Some(3081)),
            ],
            Vec::new(),
        );
        // 两个都在，一行一个：这就是"并行实例"在状态列表这一侧的验收条件。
        assert_eq!(merged.len(), 2);
        assert_eq!(merged.iter().map(|item| item.pid).collect::<Vec<_>>(), vec![111, 222]);
        // 官壳即便从两条来源都混进来，也一律不出现在清单里。
        let merged = merge_managed_instances(
            vec![instance("shell:com.deepseek.dsh", "shell:com.deepseek.dsh", 9, Some(19387))],
            vec![recorded("shell:com.deepseek.dsh", "shell:com.deepseek.dsh", 9, Some(19387))],
        );
        assert!(merged.is_empty(), "the official shell is not ours to list or stop");
    }

    /// 壁纸重启过之后，清单靠落盘记录补齐；两处都有时用内存那一份（它才知道 root_path/profile）。
    #[test]
    fn the_status_list_merges_the_record_with_what_this_process_spawned() {
        let tree = r"D:\Family\DeepSeekHarness\deepseek-harness";
        let cli = r"cli:C:\Users\u\AppData\Roaming\npm\dsh.cmd";
        let merged = merge_managed_instances(
            vec![instance(tree, tree, 111, Some(3080))],
            vec![
                // 同一个实例在落盘记录里也有（pid 与端口都旧了）⇒ 不重复一行，且内存那份赢。
                recorded(tree, tree, 999, Some(9999)),
                // 只有落盘记录有的那个（壁纸重启前启动、现在还在跑）⇒ 补上。
                recorded(cli, cli, 222, Some(3080)),
                // 落盘记录里连端口都没有 ⇒ 用参数/默认端口兜底，而不是让那一行空着。
                recorded(r"D:\other", r"D:\other", 333, None),
            ],
        );
        assert_eq!(merged.len(), 3);
        let tree_row = merged.iter().find(|item| item.subject_id == tree).expect("the tree");
        assert_eq!(tree_row.pid, 111, "the in-memory instance wins over the stale record");
        assert!(merged.iter().any(|item| item.subject_id == cli && item.pid == 222));
        let bare = merged.iter().find(|item| item.subject_id == r"D:\other").expect("the other");
        // 没有参数 ⇒ DSH 自己的默认端口：显示 3080 是"我们要求它听在哪儿"，比留空更有用。
        assert_eq!(bare.port, Some(3080));
        assert!(bare.root_path.is_none(), "a record alone cannot know the root path");
    }
}

#[cfg(test)]
#[cfg(not(feature = "lite"))]
mod harness_status_tests {
    use super::{
        advance_harness_monitor, apply_endpoint_scope, compatible_harness_bridge_status,
        diagnose_harness_bridge_status, fetch_harness_status_at, harness_endpoint_port,
        harness_probe_interval, probe_current_endpoint, root_probe_availability,
        HarnessEndpointState, HarnessMonitorState, HARNESS_DEFAULT_PORT, HARNESS_ENDPOINT_PORTS,
        HARNESS_TRANSITION_INTERVAL, HARNESS_UNPROVEN_EXIT_MS,
    };
    use crate::app_core::HarnessAvailability;
    use serde_json::json;

    fn valid_status() -> serde_json::Value {
        json!({
            "bridgeVersion": "0.1.1",
            "bridgeBuild": "dev",
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-ready",
            "reasonCode": "ready",
            "capabilities": [
                "status",
                "control",
                "sessions",
                "resume",
                "history",
                "sse",
                "cancel",
                "approval-handoff",
                "future-capability"
            ],
            "authentication": "ready",
            "provider": "deepseek",
            "model": "deepseek-chat",
            "reasoningEffort": "high"
        })
    }

    #[test]
    fn accepts_only_a_ready_compatible_bridge() {
        let status = compatible_harness_bridge_status(&valid_status()).expect("compatible status");
        assert_eq!(status["availability"], "bridge-ready");
        assert_eq!(status["bridgeVersion"], "0.1.1");
        assert_eq!(status["bridgeBuild"], "dev");
        assert_eq!(status["provider"], "deepseek");
        assert_eq!(status["model"], "deepseek-chat");
        assert_eq!(status["reasoningEffort"], "high");
    }

    #[test]
    fn accepts_a_bridge_without_optional_session_persistence() {
        let mut document = valid_status();
        let capabilities = document["capabilities"]
            .as_array_mut()
            .expect("capabilities");
        capabilities.retain(|capability| capability.as_str() != Some("resume"));
        assert!(compatible_harness_bridge_status(&document).is_some());
    }

    #[test]
    fn a_document_without_a_bridge_identity_is_not_a_bridge() {
        // These must stay `None` so the caller falls back to the root probe.
        // Reporting a Bridge state for an unrelated service on 3080 would be a
        // wrong diagnosis, not a conservative one.
        for document in [
            json!({}),
            json!({ "status": "unrelated-service" }),
            json!({ "protocolVersion": 1 }),
            json!({ "dsh": "online" }),
            json!({ "protocolVersion": "1", "dsh": "online", "capabilities": [], "authentication": "ready" }),
            json!({ "protocolVersion": 1, "dsh": "offline", "capabilities": [], "authentication": "ready" }),
        ] {
            assert!(
                diagnose_harness_bridge_status(&document).is_none(),
                "unexpected diagnosis: {document}"
            );
            assert!(compatible_harness_bridge_status(&document).is_none());
        }
    }

    /// Auto means *discovered*, not *3080*.
    ///
    /// The regression this pins: the monitor used to probe one hardcoded port, so
    /// it reported `offline` while a ready Bridge served the official desktop shell
    /// on 19387, and choosing that shell in settings did not help because nothing
    /// ever looked there.
    #[test]
    fn a_discovered_endpoint_is_used_while_nothing_is_configured() {
        let state = HarnessEndpointState::default();
        // The shipped priority order is the candidate list while nothing is
        // configured, and DSH's own default is still among the candidates, so a
        // CLI-started Host on 3080 keeps working with no configuration at all.
        assert!(candidates_of(&state).contains(&HARNESS_DEFAULT_PORT));
        assert_eq!(candidates_of(&state)[0], 19387);
        // Whatever auto mode found leads the list, so the next probe keeps using it.
        assert_eq!(note_in_use_on(&state, 3080)[0], 3080);
    }

    /// An explicit choice outranks everything, in both directions.
    #[test]
    fn a_pinned_endpoint_is_never_overridden() {
        let state = HarnessEndpointState::default();
        // A recorded endpoint is used while nothing is pinned...
        assert_eq!(note_in_use_on(&state, 19387)[0], 19387);
        // ...and stops mattering the moment the user pins something else.
        *state.pinned.lock().expect("pin lock") = Some(6000);
        assert_eq!(candidates_of(&state), vec![6000]);
        // Clearing the pin hands control back to the configured subject, and a pin
        // never recorded anything for itself to revive.
        *state.pinned.lock().expect("pin lock") = None;
        assert_eq!(state.active.lock().expect("active lock").clone(), Some(19387));
        assert_eq!(candidates_of(&state)[0], 19387);
    }

    /// The frozen rule, native half: only the configured subject's own ports may be
    /// probed, and its being down is not a reason to use another client.
    #[test]
    fn a_configured_subject_narrows_the_candidates_to_its_own_ports() {
        let state = HarnessEndpointState::default();
        apply_endpoint_scope(&state, None, Some("shell:com.deepseek.dsh"), &[], None).expect("scope");
        // The official shell owns 19387, and the wallpaper's own host port follows it: the
        // client is asked first, and only when nothing answers there does the host we run
        // ourselves come into play. 3080 belongs to neither — a CLI host may be on it.
        assert_eq!(candidates_of(&state), vec![19387, crate::harness_targets::WALLPAPER_HOST_PORT]);

        // A checkout owns DSH's default plus the ports the user added for it.
        apply_endpoint_scope(&state, None, Some(r"D:\tree"), &[3081], None).expect("scope");
        assert_eq!(candidates_of(&state), vec![HARNESS_DEFAULT_PORT, 3081]);

        // An unknown shell is *configured* with nowhere to look, which is reported
        // as unreachable rather than as a reason to fall back to another client.
        apply_endpoint_scope(&state, None, Some("shell:com.unknown.client"), &[], None).expect("scope");
        assert!(candidates_of(&state).is_empty());
        assert!(harness_endpoint_configured_on(&state));
    }

    /// 「启动参数」里点名的端口必须进到探针的候选表里，否则并行实例是看不见的。
    ///
    /// 这一条是端到端的：设置里写 `--port 3081` ⇒ 这个主体的候选表以 3081 开头 ⇒ 监视器去敲
    /// 3081 ⇒ 桥在那个端口上应答。少了任何一环，用户看到的是"第二个实例起来了但一直离线"。
    #[test]
    fn launch_args_move_the_probe_to_the_port_the_instance_serves() {
        let state = HarnessEndpointState::default();
        let args = vec!["--port".to_string(), "3081".to_string()];
        apply_endpoint_scope(&state, None, Some(r"D:\tree"), &[], Some(&args)).expect("scope");
        assert_eq!(candidates_of(&state)[0], 3081);
        // DSH 自己的默认端口仍然在表里：同一个主体的另一个实例可能就在那儿。
        assert!(candidates_of(&state).contains(&HARNESS_DEFAULT_PORT));
        // 换回没有参数 ⇒ 回到默认端口（一个不再生效的设置不该留下痕迹）。
        apply_endpoint_scope(&state, None, Some(r"D:\tree"), &[], Some(&[])).expect("scope");
        assert_eq!(candidates_of(&state), vec![HARNESS_DEFAULT_PORT]);
        // 官壳的端口编译在它自己的包里，参数改不了它 —— 壁纸自己宿主的那个端口也不受参数影响。
        apply_endpoint_scope(
            &state,
            None,
            Some("shell:com.deepseek.dsh"),
            &[],
            Some(&args),
        )
        .expect("scope");
        assert_eq!(
            candidates_of(&state),
            vec![19387, crate::harness_targets::WALLPAPER_HOST_PORT]
        );
    }

    /// Changing the subject is the one moment the wallpaper may move, and it does
    /// not carry the previous endpoint across.
    #[test]
    fn changing_the_subject_clears_the_endpoint_in_use() {
        let state = HarnessEndpointState::default();
        apply_endpoint_scope(&state, None, Some("shell:com.deepseek.dsh"), &[], None).expect("scope");
        assert_eq!(note_in_use_on(&state, 19387)[0], 19387);
        // Same subject again: the endpoint it is already using stays first.
        apply_endpoint_scope(&state, None, Some("shell:com.deepseek.dsh"), &[], None).expect("scope");
        assert_eq!(candidates_of(&state)[0], 19387);
        // A different subject starts from its own order.
        apply_endpoint_scope(&state, None, Some(r"D:\tree"), &[], None).expect("scope");
        assert_eq!(candidates_of(&state), vec![HARNESS_DEFAULT_PORT]);
        assert_eq!(*state.active.lock().expect("active lock"), None);
    }

    /// A pin never records an observation, so clearing it cannot revive one.
    #[test]
    fn nothing_is_recorded_while_pinned() {
        let state = HarnessEndpointState::default();
        *state.pinned.lock().expect("pin lock") = Some(6000);
        note_in_use_on(&state, 19387);
        assert_eq!(*state.active.lock().expect("active lock"), None);
    }

    /// No configuration at all is a different fact from a configuration that cannot
    /// be placed, and the two read differently to the user.
    #[test]
    fn nothing_configured_is_not_the_same_as_configured_but_unreachable() {
        let state = HarnessEndpointState::default();
        assert!(!harness_endpoint_configured_on(&state));
        apply_endpoint_scope(&state, None, Some("   "), &[], None).expect("scope");
        assert!(!harness_endpoint_configured_on(&state));
        // A pin alone is a configuration too.
        apply_endpoint_scope(&state, Some(3080), Some(""), &[], None).expect("scope");
        assert!(harness_endpoint_configured_on(&state));
    }

    /// The same resolution the monitor uses, against a caller-owned state.
    fn candidates_of(state: &HarnessEndpointState) -> Vec<u16> {
        if let Some(port) = state.pinned.lock().ok().and_then(|guard| *guard) {
            return vec![port];
        }
        if let Some(subject) = state.subject.lock().ok().and_then(|guard| guard.clone()) {
            let sticky = state.active.lock().ok().and_then(|guard| *guard);
            let mut ordered: Vec<u16> = sticky
                .filter(|port| subject.ports.contains(port))
                .into_iter()
                .collect();
            for port in &subject.ports {
                if !ordered.contains(port) {
                    ordered.push(*port);
                }
            }
            return ordered;
        }
        let mut ordered: Vec<u16> = state.active.lock().ok().and_then(|guard| *guard).into_iter().collect();
        for (port, _kind) in HARNESS_ENDPOINT_PORTS {
            if !ordered.contains(port) {
                ordered.push(*port);
            }
        }
        ordered
    }

    fn harness_endpoint_configured_on(state: &HarnessEndpointState) -> bool {
        state.pinned.lock().map(|guard| guard.is_some()).unwrap_or(true)
            || state.subject.lock().map(|guard| guard.is_some()).unwrap_or(true)
    }

    /// `note_endpoint_in_use`, against a caller-owned state, returning the candidate
    /// list the monitor would then probe.
    fn note_in_use_on(state: &HarnessEndpointState, port: u16) -> Vec<u16> {
        if state.pinned.lock().map(|guard| guard.is_some()).unwrap_or(true) {
            return candidates_of(state);
        }
        if let Ok(mut guard) = state.active.lock() {
            *guard = Some(port);
        }
        candidates_of(state)
    }

    /// The user's four phases, as transitions: connect, suspend, confirm, fall back.
    ///
    /// `advance_harness_monitor` is pure precisely so this can be checked without a
    /// live client — it is the function that decides when somebody's session is
    /// allowed to be taken away from them.
    #[test]
    fn a_connected_subject_is_suspended_before_it_is_declared_dead() {
        let mut state = HarnessMonitorState::default();
        // Two agreeing probes: one is a timing artefact.
        let (next, publish) = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 0);
        assert!(!publish, "a single ready probe must not publish");
        state = next;
        let (state, publish) = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 0);
        assert!(publish);
        assert_eq!(state.available, HarnessAvailability::BridgeReady);

        // One failure is not a verdict: the light goes amber, the state does not move,
        // so the switch, the model list and the session are untouched.
        let (state, publish) = advance_harness_monitor(state, HarnessAvailability::Offline, Some(true), 1_000);
        assert!(publish);
        assert!(state.probing);
        assert_eq!(
            state.available,
            HarnessAvailability::BridgeReady,
            "a suspended subject must stay usable"
        );

        // It stays suspended for as long as anything proves the process is alive.
        let (state, _) = advance_harness_monitor(state, HarnessAvailability::Offline, Some(true), 1_000 + HARNESS_UNPROVEN_EXIT_MS * 10);
        assert!(state.probing);
        assert_eq!(state.available, HarnessAvailability::BridgeReady);
    }

    #[test]
    fn a_proven_exit_is_the_verdict_that_does_not_wait() {
        let mut state = HarnessMonitorState::default();
        for _ in 0..2 {
            state = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 0).0;
        }
        assert_eq!(state.available, HarnessAvailability::BridgeReady);

        // The first unproven failure only suspends...
        let (state, _) = advance_harness_monitor(state, HarnessAvailability::Offline, Some(false), 0);
        assert!(state.probing);
        // ...and the second one, with the owner's process gone, is death — no window.
        let (state, publish) = advance_harness_monitor(state, HarnessAvailability::Offline, Some(false), 5_000);
        assert!(publish);
        assert!(!state.probing);
        assert_eq!(state.available, HarnessAvailability::Offline);
    }

    #[test]
    fn without_evidence_the_window_decides_as_it_always_did() {
        let mut state = HarnessMonitorState::default();
        for _ in 0..2 {
            state = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 0).0;
        }
        // No owner to watch: three failures and the whole window before red, which is
        // the behaviour a subject we could never identify keeps.
        let mut now = 0;
        for _ in 0..3 {
            let (next, _) = advance_harness_monitor(state, HarnessAvailability::Offline, None, now);
            state = next;
            now += 1_000;
        }
        assert!(state.probing, "the window has not elapsed yet");
        let (state, publish) = advance_harness_monitor(state, HarnessAvailability::Offline, None, HARNESS_UNPROVEN_EXIT_MS + 1);
        assert!(publish);
        assert!(!state.probing);
        assert_eq!(state.available, HarnessAvailability::Offline);
    }

    #[test]
    fn a_new_subject_cannot_inherit_the_previous_connection() {
        // 实测的边界情况：在 CLI（3080）上连过一次之后切到官壳（那边没有人在听），旧实现把
        // `connected_once` 留着 ⇒ 只把灯变成呼吸态、`available` 仍是 BridgeReady ⇒ 界面一直写着
        // "DSH Bridge 已连接"，而一发消息就被告知会话尚未建立。
        let mut monitor = HarnessMonitorState::default();
        monitor = advance_harness_monitor(monitor, HarnessAvailability::BridgeReady, None, 0).0;
        let (connected, _) =
            advance_harness_monitor(monitor, HarnessAvailability::BridgeReady, None, 0);
        assert_eq!(connected.available, HarnessAvailability::BridgeReady);

        // 带着"曾经连上过"的记忆时，新主体没人应答只够让它**可疑**：灯呼吸、标签照旧。
        // （`publish` 在这里为真是对的：呼吸态本身就是要发布给界面的状态；错的是 `available`
        // 还停在 BridgeReady —— 于是"呼吸着"和"已连接"同时出现在屏幕上。）
        let (suspended, publish) =
            advance_harness_monitor(connected, HarnessAvailability::Offline, Some(true), 1_000);
        assert!(suspended.probing, "这是呼吸态");
        assert!(publish);
        assert_eq!(suspended.available, HarnessAvailability::BridgeReady);

        // 忘掉上一条连接之后（monitor 循环在主体变化时做的正是这件事），新主体的结论就是结论：
        // 灯熄灭、标签跟着说离线。默认态本来就是 Offline，所以这里不发布也算对 —— 关键是它**没有**
        // 继承上一条连接。
        let (fresh, _) = advance_harness_monitor(
            HarnessMonitorState::default(),
            HarnessAvailability::Offline,
            Some(true),
            1_000,
        );
        assert_eq!(fresh.available, HarnessAvailability::Offline);
        assert!(!fresh.probing);
    }

    #[test]
    fn a_bridge_that_was_never_connected_reports_what_it_sees() {
        // The diagnostic states have to reach the UI: with nothing connected there is
        // no session to protect, so the observed state is published as it comes.
        for observed in [
            HarnessAvailability::WebOnly,
            HarnessAvailability::BridgeLoading,
            HarnessAvailability::BridgeAuthUnavailable,
            HarnessAvailability::BridgeIncompatible,
        ] {
            let (state, publish) = advance_harness_monitor(
                HarnessMonitorState::default(),
                observed,
                Some(false),
                0,
            );
            assert!(publish, "{observed:?} must be published");
            assert!(!state.probing);
            assert_eq!(state.available, observed);
        }
        // The one case that publishes nothing: it was already offline.
        let (state, publish) = advance_harness_monitor(
            HarnessMonitorState::default(),
            HarnessAvailability::Offline,
            Some(false),
            0,
        );
        assert!(!publish);
        assert!(!state.probing);
        assert_eq!(state.available, HarnessAvailability::Offline);
    }

    #[test]
    fn a_recovery_needs_two_answers_after_a_suspension() {
        let mut state = HarnessMonitorState::default();
        for _ in 0..2 {
            state = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 0).0;
        }
        let (state, _) = advance_harness_monitor(state, HarnessAvailability::Offline, Some(true), 500);
        let (state, publish) = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 1_000);
        assert!(!publish, "the amber light must not clear on one lucky probe");
        assert!(state.probing);
        let (state, publish) = advance_harness_monitor(state, HarnessAvailability::BridgeReady, None, 5_000);
        assert!(publish);
        assert!(!state.probing);
        assert_eq!(state.available, HarnessAvailability::BridgeReady);
    }

    #[test]
    fn the_transition_phase_polls_faster_than_keep_alive() {
        // 用户实测"杀掉官壳之后留着的那段防瞬发缓冲太长"。结论落在两个数上：确认所需次数（见
        // 上一条测试）与**确认期间多久探一次**。这里钉住后者：挂起时必须是快档。
        assert!(harness_probe_interval(true) < harness_probe_interval(false));
        assert_eq!(harness_probe_interval(true), HARNESS_TRANSITION_INTERVAL);
        // 而且合起来要足够短：两次确认 + 挂起判定，用户应该在一两秒内看到结果，而不是十几秒。
        assert!(
            HARNESS_TRANSITION_INTERVAL * 2 <= std::time::Duration::from_secs(3),
            "两次确认的总时长必须让用户觉得是「立刻」"
        );
    }

    #[test]
    #[ignore = "requires a live client on 19387 or 3080"]
    fn auto_mode_discovers_a_live_endpoint_on_this_machine() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let (port, status) = rt.block_on(async {
            let port = harness_endpoint_port();
            (port, fetch_harness_status_at(port).await)
        });
        eprintln!("current endpoint {port} -> {status}");
        // Crucially this must NOT be the default when something else is live, so
        // assert discovery by running the real probe and reporting what it found.
        let (answered, discovered) = rt.block_on(probe_current_endpoint());
        eprintln!("after discovery -> port {answered} {discovered} (candidates {})", harness_endpoint_port());
        assert!(
            discovered.get("availability").and_then(serde_json::Value::as_str) != Some("offline")
                || port != HARNESS_DEFAULT_PORT,
            "auto mode left the wallpaper offline on the default port while a client may be live"
        );
    }

    #[test]
    fn names_the_layer_that_is_unusable() {        // A protocol this wallpaper cannot speak.
        let incompatible = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 2,
            "dsh": "online",
            "capabilities": ["sessions", "history", "sse", "cancel", "approval-handoff"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(incompatible["availability"], "bridge-incompatible");
        assert_eq!(incompatible["reasonCode"], "protocol-version-mismatch");

        // Mounted, but its token is unavailable. The payload's stale
        // `reasonCode` must not describe this failure.
        let auth = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-ready",
            "reasonCode": "ready",
            "capabilities": ["sessions", "history", "sse", "cancel", "approval-handoff"],
            "authentication": "unavailable"
        }))
        .expect("diagnosis");
        assert_eq!(auth["availability"], "bridge-auth-unavailable");
        assert_eq!(auth["reasonCode"], "token-unavailable");

        // Still composing: waiting is the right action.
        let loading = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-loading",
            "reasonCode": "services-pending",
            "capabilities": ["status"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(loading["availability"], "bridge-loading");
        assert_eq!(loading["reasonCode"], "services-pending");

        // A capability set that will never satisfy the wallpaper, and no
        // promise that it is still growing.
        let missing = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-ready",
            "capabilities": ["sessions", "history", "sse", "cancel"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(missing["availability"], "bridge-incompatible");
        assert_eq!(missing["reasonCode"], "capabilities-missing");
    }

    #[test]
    fn a_loading_reason_is_only_accepted_from_the_closed_vocabulary() {
        // A stale `ready` left over from an earlier status must not be rendered
        // as the explanation for a wait.
        let stale = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-loading",
            "reasonCode": "ready",
            "capabilities": ["status"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(stale["reasonCode"], "services-pending");

        // A specific waiting reason from the same vocabulary is preserved.
        let specific = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-loading",
            "reasonCode": "waiting:workspaceRegistry",
            "capabilities": ["status"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(specific["reasonCode"], "waiting:workspaceRegistry");
    }

    #[test]
    fn reads_a_legacy_bridge_that_predates_the_state_field() {
        // An older installed copy announces neither `state` nor `reasonCode`.
        let mut legacy = valid_status();
        legacy.as_object_mut().expect("object").remove("state");
        legacy.as_object_mut().expect("object").remove("reasonCode");
        legacy.as_object_mut().expect("object").remove("bridgeBuild");
        let status = compatible_harness_bridge_status(&legacy).expect("legacy bridge is usable");
        assert_eq!(status["availability"], "bridge-ready");
        assert!(status.get("bridgeBuild").is_none());

        // An incomplete legacy capability set is incompatible rather than
        // "loading", because nothing in the document says it intends to finish.
        let mut legacy_partial = legacy.clone();
        legacy_partial["capabilities"] = json!(["sessions"]);
        let partial = diagnose_harness_bridge_status(&legacy_partial).expect("diagnosis");
        assert_eq!(partial["availability"], "bridge-incompatible");
        assert_eq!(partial["reasonCode"], "capabilities-missing");
    }

    #[test]
    fn a_malformed_capability_list_is_incompatible_not_absent() {
        // The document identified itself as a Bridge, so the honest answer is
        // that it is unusable rather than that it does not exist.
        for capabilities in [json!(["sessions", 3]), json!("sessions"), json!(null)] {
            let mut document = valid_status();
            document["capabilities"] = capabilities.clone();
            let diagnosis = diagnose_harness_bridge_status(&document)
                .unwrap_or_else(|| panic!("expected a diagnosis for {capabilities}"));
            assert_eq!(diagnosis["availability"], "bridge-incompatible");
            assert_eq!(diagnosis["reasonCode"], "capabilities-missing");
            assert!(compatible_harness_bridge_status(&document).is_none());
        }
    }

    #[test]
    fn ignores_malformed_optional_metadata() {
        let mut document = valid_status();
        document["model"] = json!({ "unexpected": true });
        document["provider"] = json!(42);
        document["reasoningEffort"] = json!("");
        let status = compatible_harness_bridge_status(&document).expect("compatible status");
        assert!(status.get("model").is_none());
        assert!(status.get("provider").is_none());
        assert!(status.get("reasoningEffort").is_none());
    }

    #[test]
    fn forwards_the_specific_host_member_but_not_a_misleading_reason() {
        // A shape mismatch names the member after the colon; forwarding it is
        // what makes the diagnostic actionable.
        let named = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-incompatible",
            "reasonCode": "host-shape-mismatch:agentPresets.recompose",
            "capabilities": ["status"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(named["availability"], "bridge-incompatible");
        assert_eq!(named["reasonCode"], "host-shape-mismatch:agentPresets.recompose");

        // A `waiting:` reason describes a wait, so it must not be rendered as the
        // explanation for an incompatibility.
        let misleading = diagnose_harness_bridge_status(&json!({
            "protocolVersion": 1,
            "dsh": "online",
            "state": "bridge-incompatible",
            "reasonCode": "waiting:workspaceRegistry",
            "capabilities": ["status"],
            "authentication": "ready"
        }))
        .expect("diagnosis");
        assert_eq!(misleading["reasonCode"], "capabilities-missing");
    }

    #[test]
    fn root_probe_requires_an_inspectable_success_status() {
        assert_eq!(
            root_probe_availability(Some(reqwest::StatusCode::OK)),
            HarnessAvailability::WebOnly
        );
        assert_eq!(
            root_probe_availability(Some(reqwest::StatusCode::NO_CONTENT)),
            HarnessAvailability::WebOnly
        );
        for status in [
            None,
            Some(reqwest::StatusCode::UNAUTHORIZED),
            Some(reqwest::StatusCode::NOT_FOUND),
            Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
        ] {
            assert_eq!(
                root_probe_availability(status),
                HarnessAvailability::Offline
            );
        }
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn probe_harness(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    Ok(harness_status_cache()
        .read()
        .map(|status| status.clone())
        .unwrap_or_else(|_| serde_json::json!({ "availability": "offline" })))
}

/// Scan every known DSH endpoint and report what answered.
///
/// Both surfaces may ask: the settings window renders the picker, and the
/// background surface shows the currently selected client. The scan only reads
/// each endpoint's public `/status` and root page, so it never touches a
/// credential and never mutates another client's state.
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn scan_harness_endpoints_command(
    caller: tauri::WebviewWindow,
    extra_ports: Option<Vec<u16>>,
) -> Result<serde_json::Value, String> {
    require_wallpaper_surface(&caller)?;
    Ok(scan_harness_endpoints(extra_ports.unwrap_or_default()).await)
}

/// Bring the running DSH client's own Windows window forward.
///
/// The rule splits by client shape: a windowless client (CLI / webui on 3080) is
/// opened in the default browser by the renderer, while the two desktop clients
/// are raised here. Doing the desktop half natively means the window raised is
/// resolved from the port the wallpaper is actually connected to, so it is the
/// same client whose session the user is talking to — and no stored executable
/// path can go stale.
///
/// This never launches anything. Starting a client is a separate, heavier action
/// with its own single-flight and trust decisions; raising must not become a back
/// door that spawns processes.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn raise_client_window(
    caller: tauri::WebviewWindow,
    port: u16,
) -> Result<client_window::RaiseOutcome, String> {
    require_wallpaper_surface(&caller)?;
    if port == 0 {
        return Err("接入端点端口无效".into());
    }
    Ok(client_window::raise_client_window(port))
}

/// Open the windowless client's web UI in the user's default browser.
///
/// The rule splits by client shape: the CLI / webui shape has no Windows window to
/// raise, so its interface is the browser at the endpoint it listens on (3080 by
/// default). The two desktop clients never take this path.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn open_client_in_browser(
    caller: tauri::WebviewWindow,
    port: u16,
    path: Option<String>,
) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    // 没给路径、或给的就是裸根 `/` 时，用**这次启动打印出来的门票**：`dsh web` 的浏览器围栏要求
    // URL 上带 token，裸端口只会得到那句 "dsh web authentication required"。门票由启动时捕获
    // （见 `harness_launch::known_web_handoff`），前端不需要知道它存在。
    //
    // 把 `/` 也当作"没指定"是刻意的：渲染层传参默认值就是 `'/'`（实测），而对一个带围栏的宿主
    // 来说，裸根本来就不是任何人想要的结果 —— 它只会换来一句道歉。
    let requested = path.as_deref().unwrap_or("/");
    let target = if requested == "/" {
        harness_launch::known_web_handoff(port).unwrap_or_else(|| requested.to_string())
    } else {
        requested.to_string()
    };
    client_window::open_loopback_url(port, &target)
}

/// Open a link from the transcript in the user's default browser.
///
/// The address is model output, so it never reaches the shell unvalidated: see
/// `external_link::validate` for what counts as openable. This command only adds the
/// surface check — the link lives on the wallpaper's chat surface, not in settings.
#[tauri::command]
fn open_external_link(caller: tauri::WebviewWindow, url: String) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    external_link::open(&url).map(|_| ())
}

/// Report whether anything is listening on an endpoint, without raising it.
///
/// Lets the UI tell "client not running" from "client running but has no window",
/// which need different wording and different recovery.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn harness_endpoint_listening(caller: tauri::WebviewWindow, port: u16) -> Result<bool, String> {
    require_wallpaper_surface(&caller)?;
    Ok(client_window::endpoint_is_listening(port))
}

/// Whether an endpoint has a window that *could* be raised.
///
/// Separate from raising so the UI can label the action honestly before the user
/// presses it, and so this machine's split-process shape can be checked without
/// disturbing whatever window is on screen. Measured here: the process that
/// listens is not always the process that owns the window.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn harness_endpoint_window(caller: tauri::WebviewWindow, port: u16) -> Result<bool, String> {
    require_wallpaper_surface(&caller)?;
    Ok(client_window::window_for_endpoint(port).is_some())
}


/// Probe the permitted endpoints and report what the wallpaper is talking to.
///
/// Tiered on purpose, but only *within* the permitted set: the endpoint in use is
/// tried first (the common case is one loopback request), and the rest of the
/// subject's own ports are tried after it, so a tree that was started on a port the
/// user added is still found. What never happens is looking outside that set — a
/// ready Bridge on another port is a different subject, and adopting it is the
/// silent substitution the frozen rule forbids for any reason. When nothing is
/// configured the permitted set *is* the shipped priority order, which is the one
/// case where "whichever client answers" is what the user asked for.
///
/// Returns the port that answered along with its status, because the caller needs to
/// know *whose* process to watch: the owner of that port is the subject, and its
/// liveness is what separates "exited" from "hung".
#[cfg(not(feature = "lite"))]
async fn probe_current_endpoint() -> (u16, serde_json::Value) {
    let candidates = harness_endpoint_candidates();
    let configured = harness_endpoint_configured();
    let mut first: Option<(u16, serde_json::Value)> = None;
    let mut present: Option<(u16, serde_json::Value)> = None;
    for port in candidates {
        let status = fetch_harness_status_at(port).await;
        if harness_status_is_ready(&status) {
            // Keep talking to the instance that answered: "connected ⇒ sticky".
            note_endpoint_in_use(port);
            return (port, status);
        }
        if harness_status_is_present(&status) && present.is_none() {
            present = Some((port, status.clone()));
        }
        if first.is_none() {
            first = Some((port, status));
        }
    }
    // Nothing permitted is usable. A Bridge that answered but is not ready is the
    // more specific diagnosis, so it wins over a bare "nothing there"; otherwise the
    // first permitted port's own status is reported, and the reason code says which
    // of the two situations this is.
    let (port, mut status) = present
        .or(first)
        .unwrap_or((harness_endpoint_port(), serde_json::json!({ "availability": "offline" })));
    if status.get("availability").and_then(serde_json::Value::as_str) == Some("offline") {
        status["reasonCode"] = serde_json::Value::String(
            if configured { "subject-offline" } else { "no-endpoint" }.to_string(),
        );
    }
    (port, status)
}

/// Whether a status document proves a Bridge is there, even if it is not usable.
#[cfg(not(feature = "lite"))]
fn harness_status_is_present(status: &serde_json::Value) -> bool {
    match status.get("availability").and_then(serde_json::Value::as_str) {
        Some("offline") | Some("web-only") | None => false,
        Some(_) => true,
    }
}

#[cfg(not(feature = "lite"))]
fn harness_status_is_ready(status: &serde_json::Value) -> bool {
    status.get("availability").and_then(serde_json::Value::as_str) == Some("bridge-ready")
}

/// How often the monitor probes while nothing is in transition.
///
/// Low on purpose ("连上后低频保活"), because the event that matters — the subject exiting
/// — no longer has to be discovered by a poll: a watcher on the process handle wakes
/// this loop the moment it happens.
#[cfg(not(feature = "lite"))]
const HARNESS_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// How often the monitor probes *while a ready Bridge is failing*.
///
/// The one phase where speed is the whole point: this is the "suspended, not dead"
/// stretch, the light is breathing amber and the user is waiting to learn which of the
/// two it is. Probing that at the keep-alive rate makes the verdict — and therefore the
/// reset — take several times longer than the evidence does.
#[cfg(not(feature = "lite"))]
const HARNESS_TRANSITION_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1_200);

/// 切换主体之后，"连接中"至少要占住这段时间，多快拿到结论都不改口。
///
/// 与渲染端的最短停留（`HARNESS_SWITCH_BUFFER_MS`）是同一个数字的两半：**这一半是权威**（它决定
/// 发布什么状态），那一半只管界面上的灯。实测过的坑：新主体的第一份结论会被"从没连过就发布所见"
/// 那条老规则立刻发出去，于是黄灯一步都没出现、直接变灰 —— 用户看到的就是"没有尝试连接的阶段"。
#[cfg(not(feature = "lite"))]
const HARNESS_SWITCH_HOLD: std::time::Duration = std::time::Duration::from_millis(1_200);

/// 切换缓冲期内发布的原因码：界面上它对应"连接中"，与其它任何原因都不同（诊断里一眼能看出
/// "这是刚换了主体、还在缓冲"，而不是"主体不在"或"桥不兼容"）。
#[cfg(not(feature = "lite"))]
const HARNESS_CONNECTING_REASON: &str = "connecting";

/// How long "not answering" may last before the subject is called dead *when there is
/// nothing to prove it with*.
///
/// The renderer has its own copy of this number (`HARNESS_SUSPECT_GRACE_MS`) because the
/// browser preview runs its own loop; that copy is the display window. This one is the
/// verdict, and only this one decides when the switch is allowed to reset. It is the
/// fallback for a subject whose process could not be identified at all — a *proven* exit
/// does not wait for it.
#[cfg(not(feature = "lite"))]
const HARNESS_UNPROVEN_EXIT_MS: u64 = 20_000;

/// How long to wait before the next probe, given what the last one found.
#[cfg(not(feature = "lite"))]
fn harness_probe_interval(probing: bool) -> std::time::Duration {
    if probing {
        HARNESS_TRANSITION_INTERVAL
    } else {
        HARNESS_PROBE_INTERVAL
    }
}

/// What one probe means for the published Harness state.
///
/// Pulled out of the loop and kept pure because this is where the user's four phases
/// live, and getting a transition wrong is invisible until it has already reset
/// somebody's session:
///
/// * a Bridge that answers twice in a row is connected, and a single slow probe can
///   never flicker the switch off;
/// * a Bridge that *was* connected and stops answering is **suspended**, not lost:
///   `available` is left exactly as it was, so the mode switch, the model list and the
///   session keep treating the subject as present, and only the light changes;
/// * the suspension ends in one of two ways — the process that answered is **proven
///   gone** (the only immediate proof of death), or, when there is no proof to be had,
///   the window above expires.
#[cfg(not(feature = "lite"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HarnessMonitorState {
    /// A compatible Bridge has been seen at least once, so a failure now means "lost",
    /// not "never there".
    connected_once: bool,
    successes: u8,
    failures: u8,
    /// The published availability. While `probing` is set it is deliberately the
    /// *previous* value.
    available: HarnessAvailability,
    probing: bool,
    /// When the current suspension began, in the caller's clock.
    suspect_since_ms: Option<u64>,
}

#[cfg(not(feature = "lite"))]
fn advance_harness_monitor(
    mut state: HarnessMonitorState,
    observed: HarnessAvailability,
    owner_alive: Option<bool>,
    now_ms: u64,
) -> (HarnessMonitorState, bool) {
    let before = (state.available, state.probing);
    if observed == HarnessAvailability::BridgeReady {
        state.successes = state.successes.saturating_add(1);
        state.failures = 0;
        state.suspect_since_ms = None;
        // Two consecutive ready answers, for the same reason a failure needs more than
        // one: a single probe is a timing artefact, not a state.
        if state.successes >= 2 {
            state.connected_once = true;
            state.probing = false;
            state.available = HarnessAvailability::BridgeReady;
        }
    } else {
        state.failures = state.failures.saturating_add(1);
        state.successes = 0;
        let exited = owner_alive == Some(false);
        if !state.connected_once {
            // Nothing was ever connected here, so this is simply what the endpoint
            // says: "no Bridge", "web-only", or a Bridge still loading. Publishing it
            // is what makes those diagnostics reachable at all.
            state.probing = false;
            state.available = observed;
        } else if exited && state.failures >= 2 {
            // Proof of death: the process that owned the port is gone. Two readings
            // first, so a probe that simply raced the process's exit cannot decide it.
            state.probing = false;
            state.available = HarnessAvailability::Offline;
        } else {
            // Unproven: suspend. The light goes amber, nothing else moves.
            state.probing = true;
            let since = *state.suspect_since_ms.get_or_insert(now_ms);
            if owner_alive.is_none()
                && state.failures >= 3
                && now_ms.saturating_sub(since) >= HARNESS_UNPROVEN_EXIT_MS
            {
                // Nothing can prove the subject is alive, and it has been silent for the
                // whole window: fall back to the verdict that the evidence-free case has
                // always used.
                state.probing = false;
                state.available = HarnessAvailability::Offline;
            }
        }
    }
    let publish = (state.available, state.probing) != before;
    (state, publish)
}

#[cfg(not(feature = "lite"))]
fn start_harness_monitor(app: tauri::AppHandle) {
    use std::sync::Arc;

    tauri::async_runtime::spawn(async move {
        let mut monitor = HarnessMonitorState::default();
        // Who owned the port the last time it answered: the only process whose exit
        // proves the subject itself is gone.
        let mut owner_pid: Option<u32> = None;
        // The process id a watcher thread is already waiting on, so a restart spawns a
        // watcher for the new process instead of piling one up per probe.
        let mut watched: Option<u32> = None;
        // The subject's exit is an event, not something to discover by polling: the
        // watcher below signals this and the loop probes immediately. Without it the
        // light could only change at the next scheduled probe, which is exactly the
        // "防瞬发的缓冲时间太长" the user reported after killing the client.
        let exits = Arc::new(tokio::sync::Notify::new());
        let started = std::time::Instant::now();
        let mut last_reason: Option<String> = None;
        // 这一轮探的是哪个主体。变了就把上一条连接的历史作废（见 `harness_scope_key`）。
        let mut scope = harness_scope_key();
        // 换了主体必须**发布一次**：界面上那条状态属于上一个对象，不发布就会一直停在它上面。
        let mut force_publish = false;
        // 切换主体后**先只发布"连接中"**，多快拿到新主体的结论都不改变这一点。
        //
        // 实测：新主体的第一份结论会被"从没连过就发布所见"那条老规则立刻发出去（日志：
        // `scope changed → Offline (probing false)`），于是界面上根本没有黄灯，直接变成灰 ——
        // 而用户要的是"切换后固定 1–2s 的黄灯缓冲"。缓冲期内发布的状态是"还没定"，这正是黄灯的
        // 唯一含义；结论晚 1.2 秒再说，不会晚过任何人的耐心，却让中间态真的看得见。
        let mut scope_changed_at: Option<std::time::Instant> = None;
        loop {
            let current_scope = harness_scope_key();
            if current_scope != scope {
                log::info!(
                    "harness monitor: endpoint scope changed ({scope} -> {current_scope}); forgetting the previous connection"
                );
                scope = current_scope;
                // 忘了"曾经连上过"、忘了属主、忘了理由：它们说的都是**上一个**主体。
                monitor = HarnessMonitorState {
                    probing: true,
                    ..HarnessMonitorState::default()
                };
                owner_pid = None;
                watched = None;
                last_reason = None;
                force_publish = true;
                scope_changed_at = Some(std::time::Instant::now());
            }
            let holding = scope_changed_at
                .is_some_and(|at| at.elapsed() < HARNESS_SWITCH_HOLD);
            let (port, status) = probe_current_endpoint().await;
            if let Ok(mut cache) = harness_status_cache().write() {
                *cache = status.clone();
            }
            // Keep the reason code with the state: a diagnostic without its
            // cause is not actionable, and publishing them separately would let
            // a stale reason describe a newer state.
            let reason = status
                .get("reasonCode")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let observed = match status
                .get("availability")
                .and_then(serde_json::Value::as_str)
            {
                Some("bridge-ready") => HarnessAvailability::BridgeReady,
                Some("bridge-loading") => HarnessAvailability::BridgeLoading,
                Some("bridge-auth-unavailable") => HarnessAvailability::BridgeAuthUnavailable,
                Some("bridge-incompatible") => HarnessAvailability::BridgeIncompatible,
                Some("web-only") => HarnessAvailability::WebOnly,
                _ => HarnessAvailability::Offline,
            };
            if observed == HarnessAvailability::BridgeReady {
                // Remember whose port answers, so a later silence can be told apart
                // from a later *exit*. Kept when a probe fails: that is exactly when it
                // is needed.
                owner_pid = client_window::endpoint_process_id(port).or(owner_pid);
                if let Some(pid) = owner_pid {
                    if watched != Some(pid) {
                        watched = Some(pid);
                        let exits = exits.clone();
                        std::thread::spawn(move || {
                            if client_window::wait_for_process_exit(pid) {
                                log::info!(
                                    "harness monitor: the process answering {port} (pid {pid}) exited"
                                );
                                exits.notify_one();
                            }
                        });
                    }
                }
            }
            let owner_alive = owner_pid.map(client_window::process_is_alive);
            let now_ms = started.elapsed().as_millis() as u64;
            let (next, publish) = advance_harness_monitor(monitor, observed, owner_alive, now_ms);
            monitor = next;
            // The reason belongs to the *reported* state: a suspension shows the
            // endpoint's own last reason, and a confirmed exit says so, because "the
            // process is gone" and "it is not answering" need different user actions.
            let reason_for_state = match (monitor.available, owner_alive) {
                (HarnessAvailability::Offline, Some(false)) if monitor.failures >= 2 => {
                    Some("subject-exited".to_string())
                }
                _ => reason.clone(),
            };
            // 缓冲期结束的那一 tick：缓冲期内压住的结论现在要发出来。
            if scope_changed_at.is_some() && !holding {
                scope_changed_at = None;
                force_publish = true;
            }
            if holding {
                // 缓冲期内**只**发布"连接中"：不发布新主体的结论，也不发布上一个主体的残留。
                // 状态用 Offline + probing（界面上就是黄灯 + "连接中"），理由是这两件事都是真的：
                // 此刻确实没有连上，也确实还在连。
                if last_reason.as_deref() != Some(HARNESS_CONNECTING_REASON) {
                    last_reason = Some(HARNESS_CONNECTING_REASON.to_string());
                    log::info!("harness availability: connecting (held for the switch buffer)");
                    if let Some(core) = app.try_state::<AppCore>() {
                        let snapshot = core.dispatch(AppAction::SetHarnessDiagnostic {
                            availability: HarnessAvailability::Offline,
                            reason_code: Some(HARNESS_CONNECTING_REASON.to_string()),
                            probing: true,
                        });
                        emit_app_snapshot(&app, &snapshot);
                    }
                }
            } else if publish || force_publish || reason_for_state != last_reason {
                force_publish = false;
                log::info!(
                    "harness availability: {:?} (probing {}, owner {:?} alive {:?}, reason {:?})",
                    monitor.available,
                    monitor.probing,
                    owner_pid,
                    owner_alive,
                    reason_for_state
                );
                last_reason = reason_for_state.clone();
                if let Some(core) = app.try_state::<AppCore>() {
                    let snapshot = core.dispatch(AppAction::SetHarnessDiagnostic {
                        availability: monitor.available,
                        reason_code: reason_for_state,
                        probing: monitor.probing,
                    });
                    emit_app_snapshot(&app, &snapshot);
                }
            }
            // A resident wallpaper must not hammer a dead loopback port — except in the
            // one phase where the user is waiting on the answer, which polls fast (see
            // `HARNESS_TRANSITION_INTERVAL`). Either way the wait is interruptible: the
            // subject exiting is a fact worth reacting to now, not at the next tick.
            tokio::select! {
                _ = tokio::time::sleep(harness_probe_interval(monitor.probing)) => {}
                _ = exits.notified() => {}
                // 切换主体也叫醒这一轮：见 `harness_scope_notify`。
                _ = harness_scope_notify().notified() => {}
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = LITE_EDITION.set(false);
    run_with_edition(false);
}

/// Entry point used by the first public Lite package. The two editions share
/// the audited Windows host and lock-screen implementation, but Lite does
/// not start chat, DeepSeek WebView, Harness probing, or desktop-workspace
/// monitors.
pub fn run_lite() {
    let _ = LITE_EDITION.set(true);
    run_with_edition(true);
}

#[cfg(feature = "lite")]
macro_rules! register_edition_commands {
    ($builder:expr) => {
        $builder.invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            dispatch_app_action,
            lite_settings_get,
            lite_settings_save,
            lite_image_import,
            lite_image_resolve,
            // set_desktop_wallpaper_fallback,
            // desktop_wallpaper_fallback_status,
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//             set_lock_screen_enabled,
//             clear_stale_lock_screen_backup,
//             get_lock_screen_diagnostics,
            set_autostart,
            autostart_status,
            // translucent_tb_status,
            // launch_translucent_tb,
            // open_translucent_tb_install,
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//             open_windows_lock_screen_settings,
            native_bootstrap_generation,
            release_native_bootstrap,
            start_settings_drag,
            hide_settings_window
        ])
    };
}

#[cfg(not(feature = "lite"))]
macro_rules! register_edition_commands {
    ($builder:expr) => {
        $builder.invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            desktop_displays,
            set_interaction_enabled,
            select_backend,
            dispatch_app_action,
            publish_settings,
            ensure_profile_bridge,
            notify_appearance_changed,
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//             set_lock_screen_enabled,
//             clear_stale_lock_screen_backup,
//             get_lock_screen_diagnostics,
            set_autostart,
            autostart_status,
            scan_dsh_paths,
            scan_harness_targets,
            harness_target_catalog,
            launch_harness_target,
            ensure_harness_ui,
            autostart_harness_target,
            launch_dsh,
            autostart_managed_dsh,
            managed_dsh_autostart_status,
            managed_dsh_status,
            stop_managed_dsh,
            // translucent_tb_status,
            // launch_translucent_tb,
            // open_translucent_tb_install,
// FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30 决定，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
//             open_windows_lock_screen_settings,
            save_api_key,
            api_key_status,
            desktop_workspace_status,
            open_project_memory,
            open_subject_tui,
            clear_user_data,
            show_deepseek_login,
            native_bootstrap_generation,
            release_native_bootstrap,
            deepseek_web_ensure,
            deepseek_web_status,
            deepseek_web_history,
            deepseek_web_adapter_config_status,
            open_deepseek_web_adapter_config,
            reset_deepseek_web_adapter_config,
            open_settings_window,
            start_settings_drag,
            hide_settings_window,
            begin_interaction_region_session,
            update_interaction_regions,
            desktop_layout_metrics,
            enter_inner_workspace_from_ball,
            leave_inner_workspace,
            send_chat,
            cancel_chat,
            connect_harness,
            harness_history,
            harness_presets,
            harness_set_preset,
            harness_controls,
            harness_models,
            harness_set_model,
            api_models,
            harness_set_permission,
            api_history,
            list_api_conversations,
            delete_api_conversation,
            clear_api_history,
            probe_harness,
            scan_harness_endpoints_command,
            set_harness_endpoint,
            verify_island_click,
            raise_client_window,
            open_client_in_browser,
            open_external_link,
            harness_endpoint_listening,
            harness_endpoint_window,
            appearance::commands::appearance_get_state,
            appearance::commands::appearance_list_themes,
            appearance::commands::appearance_list_assets,
            appearance::commands::appearance_activate_theme,
            appearance::commands::appearance_set_override,
            appearance::commands::appearance_clear_override,
            appearance::commands::appearance_import_paths,
            appearance::commands::appearance_classify_asset,
            appearance::commands::appearance_export_current_theme,
            appearance::commands::appearance_resolve_asset,
            appearance::commands::appearance_resolve_library_asset,
            // 更新检测：壁纸宿主（立绘气泡）与设置中心（系统页）各调一次；返回值只有结构化码与
            // 数字，文案由界面按语言说。`update_dismiss` 是气泡与设置页共用的那一条「忽略」；
            // `update_download` / `update_install` 是第三片的「下载」与「点击安装」（进度走
            // `update-download` 那条全局事件，见 `update/download.rs`）。
            update::commands::update_check,
            update::commands::update_dismiss,
            update::commands::update_download,
            update::commands::update_install
        ])
    };
}

fn run_with_edition(lite: bool) {
    // DPI 感知已在 main.rs 进程入口设置（Per-Monitor DPI Aware）。
    if !windows_integration::acquire_shared_wallpaper_host() {
        return;
    }
    let mut builder = tauri::Builder::default()
        // This must be registered before plugins that start resident services
        // or create native windows. A second launch then exits before it can
        // create another WorkerW host or duplicate tray icon.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            log::info!("second dsh-wallpaper launch redirected to existing instance");
        }))
        // The log plugin defaults to Trace. The resident Harness probe and
        // WebView2 internals would otherwise append a DEBUG connection line
        // every few seconds, adding needless I/O and obscuring real startup
        // failures in the small rotating log.
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .manage(AppCore::default());

    // The Lite process has no chat or theme-library surface. Avoid opening
    // SQLite, allocating the encrypted API store, or creating a managed DSH
    // child state at startup; those services are registered only by the full
    // edition. Commands remain compiled for the full build, but the Lite
    // capability files do not grant them to either Lite WebView.
    #[cfg(not(feature = "lite"))]
    {
        let appearance_paths =
            appearance::AppearancePaths::from_local_app_data().expect("local app data unavailable");
        appearance_paths
            .create()
            .expect("failed to create appearance directories");
        let appearance_repository =
            appearance::AppearanceRepository::open(&appearance_paths.catalog)
                .expect("failed to open appearance catalog");
        let appearance_importer = appearance::AppearanceImporter::new(appearance_paths.clone())
            .expect("failed to initialize appearance importer");
        let appearance_exporter = appearance::AppearanceExporter::new(appearance_paths.clone())
            .expect("failed to initialize appearance exporter");
        builder = builder
            .manage(appearance::AppearanceState::with_runtime_io(
                appearance_repository,
                appearance_importer,
                appearance_exporter,
                appearance_paths,
            ))
            .manage(chat::ChatState::default())
            .manage(deepseek_web::DeepSeekWebState::default())
            .manage(ManagedDshState::default())
            .manage(ManagedDshAutostartState::default());
    }

    register_edition_commands!(builder)
        .setup(move |app| {
            native_bootstrap::report_tauri_ready();
            // "这个孩子是不是我启动的"要跨壁纸重启成立，就得把记录落在本地数据目录里 ——
            // 这个路径只有 Tauri 算得准（打包应用会被重定向），不能靠环境变量硬拼。
            // `harness_launch` 是 `#[cfg(not(feature = "lite"))]` 的模块，所以这里必须同样受门控：
            // Lite 目标里它根本不存在（CI 抓到的就是这个 E0433 —— 我本地只跑默认特性，看不见）。
            #[cfg(not(feature = "lite"))]
            {
                match app.path().app_local_data_dir() {
                    Ok(dir) => harness_launch::set_records_path(dir.join("managed-dsh.json")),
                    Err(error) => log::warn!("managed-child record path unavailable: {error}"),
                }
            }
            if let Err(error) = windows_integration::start_wallpaper_host(app.handle().clone()) {
                log::error!("WorkerW wallpaper host failed: {error}");
            }
            if let Some(settings) = app.get_webview_window("settings") {
                if let Err(error) = windows_integration::configure_settings_window(&settings) {
                    log::warn!("settings window frame configuration failed: {error}");
                }
            }
            if let Err(error) = windows_integration::register_session_events(app.handle()) {
                log::warn!("session notification failed: {error}");
            }
            #[cfg(not(feature = "lite"))]
            {
                windows_integration::start_foreground_monitor(app.handle().clone());
                windows_integration::start_desktop_workspace_monitor(app.handle().clone());
                start_harness_monitor(app.handle().clone());
                // 悬浮球（interaction-handover §3.1 的增量 1）：独立顶层窗口，
                // 平时停在屏幕之外，鼠标靠近底边才滑入。它是人工入口而不是常驻
                // 服务，因此创建失败只记日志，绝不打断启动链（也绝不 `?`）。
                match floating_ball::ensure_ball(app.handle()) {
                    Ok(_) => floating_ball::start_ball_monitor(app.handle().clone()),
                    Err(error) => log::warn!("floating ball unavailable: {error}"),
                }
            }
            // Keep an enabled pre-StartupTask installation running across a
            // package update. The migration is best-effort and leaves the
            // legacy Run entry intact if Windows asks for user approval or
            // denies the new task.
            tauri::async_runtime::spawn_blocking(|| {
                match windows_integration::migrate_legacy_autostart() {
                    Ok(Some(status)) => log::info!(
                        "autostart migration check complete: source={}, enabled={}",
                        status.source,
                        status.enabled
                    ),
                    Ok(None) => {}
                    Err(error) => log::warn!("autostart migration deferred: {error}"),
                }
            });
            let menu = if lite {
                MenuBuilder::new(app)
                    .text("settings", "设置")
                    .separator()
                    .text("lock", "锁定 Windows")
                    .separator()
                    .text("quit", "退出")
                    .build()?
            } else {
                MenuBuilder::new(app)
                    .text("show", "显示中央会话窗")
                    .text("hide", "隐藏中央会话窗")
                    .separator()
                    .text("deepseek-web", "DeepSeek 网页模式")
                    .text("deepseek-api", "DeepSeek API 模式")
                    .text("harness", "Harness 模式")
                    .separator()
                    .text("lock", "锁定 Windows")
                    .text("settings", "设置")
                    .separator()
                    .text("quit", "退出")
                    .build()?
            };
            let mut tray = TrayIconBuilder::with_id("dsh-wallpaper")
                .menu(&menu)
                .tooltip(if lite {
                    "DSH Wallpaper Lite"
                } else {
                    "DSH Wallpaper"
                })
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(core) = app.try_state::<AppCore>() {
                            core.dispatch(AppAction::SetInteractionEnabled(true));
                        }
                        dispatch_tray_ui_action(app, AppAction::OpenChat);
                    }
                    "hide" => {
                        dispatch_ui_action(app, AppAction::SetInteractionEnabled(false), false)
                    }
                    "deepseek-web" | "deepseek-api" | "harness" => {
                        let _ = app.emit_to(
                            EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
                            "tray-backend",
                            event.id().as_ref(),
                        );
                    }
                    "settings" => {
                        dispatch_tray_ui_action(app, AppAction::OpenSettings);
                        if let Err(error) = show_settings_window(app) {
                            log::error!("failed to show settings window: {error}");
                        }
                    }
                    "lock" => {
                        #[cfg(windows)]
                        {
                            let _ = std::process::Command::new("rundll32.exe")
                                .arg("user32.dll,LockWorkStation")
                                .spawn();
                        }
                    }
                    "quit" => {
                        #[cfg(windows)]
                        windows_integration::restore_desktop_icons();
                        app.exit(0)
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    ) {
                        let app = tray.app_handle();
                        dispatch_tray_ui_action(app, AppAction::OpenSettings);
                        if let Err(error) = show_settings_window(app) {
                            log::error!(
                                "failed to show settings window from tray double-click: {error}"
                            );
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let _ = tray.build(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build dsh-wallpaper")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                shutdown_native_state(app);
            }
        });
}

/// Restore everything this process changed on the user's desktop.
///
/// Hiding the settings window or minimizing the wallpaper is not an exit and
/// must not run any of this: the tray quit item and a real window close both
/// reach `RunEvent::ExitRequested`, while tray "hide" only dispatches an
/// action. Every step is idempotent, because `ExitRequested` can be followed by
/// `Exit`.
fn shutdown_native_state(app: &tauri::AppHandle) {
    static SHUTDOWN: std::sync::Once = std::sync::Once::new();
    SHUTDOWN.call_once(|| {
        // 1. The desktop icon layer is deliberately hidden while the wallpaper
        //    is resident; leaving it hidden after exit would strand the user
        //    with an iconless desktop.
        #[cfg(windows)]
        windows_integration::restore_desktop_icons();

        // 2. Destroy the native first-frame hand-off window. Hiding it is not
        //    enough at exit: its GDI bitmaps and the boosted process priority
        //    would outlive the WebView.
        if let Err(error) = native_bootstrap::destroy() {
            log::warn!("native bootstrap teardown failed: {error}");
        }

        // 3. Stop only the DSH instances this process launched. An external DSH —
        //    or someone else's client on our port — belongs to the user and is
        //    never touched. 现在是**每一个**我们自己启动的实例：并行实例意味着出口这里也可能
        //    不止一个，漏掉一个就会在壁纸退出后留下一台没人认领的宿主。
        #[cfg(not(feature = "lite"))]
        if let Some(state) = app.try_state::<ManagedDshState>() {
            if let Ok(mut managed) = state.0.lock() {
                for (_, mut process) in std::mem::take(&mut *managed) {
                    crate::client_window::stop_process_tree(process.child.id());
                    #[cfg(not(windows))]
                    {
                        let _ = process.child.kill();
                    }
                    let _ = process.child.wait();
                }
            }
        }

        // 4. Release native session/notification resources owned by this
        //    process so a restart does not inherit a stale registration.
        windows_integration::unregister_session_events(app);

        // 5. Last, record that the desktop was released. The repair helper reads
        //    this marker after this process is gone: its presence means the
        //    mutations above were undone, its absence means this process died
        //    early and the desktop still needs repairing. Writing it any earlier
        //    would let the helper skip a repair for work that had not happened yet.
        desktop_repair::mark_clean_exit();

        log::info!("dsh-wallpaper native state restored on exit");
    });
}
