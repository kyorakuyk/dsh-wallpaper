use crate::*;
use crate::commands::{require_background, require_wallpaper_surface};
use super::managed::{autostart_attempt, record_autostart_attempt, ManagedDshAutostart, ManagedDshState};

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
