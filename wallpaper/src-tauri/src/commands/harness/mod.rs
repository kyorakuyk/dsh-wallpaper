use crate::*;
use super::{require_background, require_settings, require_wallpaper_surface};
use discovery::{dsh_port_is_occupied, resolve_dsh_launcher};
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
pub(crate) struct ManagedDshStatus {
    instances: Vec<ManagedDshInstance>,
    managed: bool,
    running: bool,
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
pub(crate) async fn ensure_profile_bridge(
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
pub(crate) async fn launch_harness_target(
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
pub(crate) async fn ensure_harness_ui(
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
pub(crate) async fn autostart_harness_target(
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

pub(crate) mod discovery;
pub(crate) mod session;

// D 组（discovery）：外部仍按 commands::harness::<名字> 引用扫描与候选类型。
#[allow(unused_imports)]
pub(crate) use discovery::{harness_target_catalog, scan_dsh_paths, scan_harness_targets};
pub(crate) use discovery::{scan_dsh_paths_blocking, DshPathCandidate};

// 重导出保持外部看到的是 commands::harness::<命令名>（决定 4）。
// handler 无法穿过重导出找到 #[tauri::command] 生成的宏，故注册改用子模块路径（决定 4 允许的例外）。
#[allow(unused_imports)]
pub(crate) use session::{
    connect_harness, harness_controls, harness_history, harness_models, harness_presets,
    harness_set_model, harness_set_permission, harness_set_preset,
};
