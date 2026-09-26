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
#[cfg(not(feature = "lite"))]
mod deepseek_web_config;
#[cfg(not(feature = "lite"))]
mod harness_targets;
#[cfg(not(feature = "lite"))]
mod harness_launch;
#[cfg(not(feature = "lite"))]
mod harness_catalog;
mod desktop_fallback;
// 悬浮球是「折叠态胶囊」的新家，而胶囊只属于完整版；它的创建/监控也只挂在完整版
// 的 setup 块里。像上面 api_persistence / chat 那样按 edition 收窄，否则整个模块
// 在 Lite 构建里全部不可达，会给 Lite 门禁凭空加一串 dead_code 警告。
#[cfg(not(feature = "lite"))]
mod floating_ball;
mod lock_screen_backup;
mod native_bootstrap;
mod native_handoff;
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

#[cfg(not(feature = "lite"))]
fn dsh_port_is_occupied() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 3080)),
        std::time::Duration::from_millis(250),
    )
    .is_ok()
}

/// A DSH process is only "managed" when this instance spawned it and still
/// owns its `Child` handle. Port 3080 alone never proves ownership.
#[cfg(not(feature = "lite"))]
struct ManagedDshProcess {
    child: Child,
    root_path: String,
    profile: String,
    /// Where this child's own output was captured, when it could be.
    log_path: Option<PathBuf>,
}

#[derive(Default)]
#[cfg(not(feature = "lite"))]
struct ManagedDshState(Mutex<Option<ManagedDshProcess>>);

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
struct ManagedDshStatus {
    managed: bool,
    running: bool,
    pid: Option<u32>,
    root_path: Option<String>,
    profile: Option<String>,
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
#[tauri::command]
#[cfg(not(feature = "lite"))]
async fn launch_harness_target(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    target_id: String,
    profile: Option<String>,
    command: Option<String>,
) -> Result<harness_launch::HarnessLaunchOutcome, String> {
    require_wallpaper_surface(&caller)?;
    let plan = harness_launch::plan_launch(
        &target_id,
        profile.as_deref().unwrap_or_default(),
        command.as_deref(),
        harness_launch::LaunchTrigger::Manual,
    )
    .map_err(str::to_string)?;
    // Starting a shell waits for the client to answer (up to the launch timeout),
    // so it must not run on the caller's own thread. The managed state is taken
    // inside the blocking task, where acquiring it cannot block the UI.
    tauri::async_runtime::spawn_blocking(move || {
        let managed = app.state::<ManagedDshState>();
        harness_launch::run_launch(&plan, managed.inner())
    })
    .await
    .map_err(|error| format!("启动 Harness 执行主体未完成：{error}"))
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
    command: Option<String>,
) -> Result<harness_launch::HarnessUiOutcome, String> {
    require_wallpaper_surface(&caller)?;
    let target_id = target_id.unwrap_or_default();
    let profile = profile.unwrap_or_default();
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let managed = worker.state::<ManagedDshState>();
        harness_launch::ensure_ui(
            &target_id,
            port,
            &profile,
            command.as_deref(),
            managed.inner(),
        )
    })
    .await
    .map_err(|error| format!("拉起 Harness 界面未完成：{error}"))
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
    command: Option<String>,
    trusted_command: Option<bool>,
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
    let plan = match harness_launch::plan_launch(
        &target_id,
        &profile,
        command.as_deref(),
        harness_launch::LaunchTrigger::Automatic {
            trusted_command: trusted_command == Some(true),
        },
    ) {
        Ok(plan) => plan,
        Err(code) => {
            let outcome = ManagedDshAutostart::new(code);
            record_autostart_attempt(&app, &outcome)?;
            return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
        }
    };

    // The handle is cloned into the blocking task so this one keeps working for
    // the record below: the task owns the clone, this frame owns the original.
    let worker = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let managed = worker.state::<ManagedDshState>();
        harness_launch::run_launch(&plan, managed.inner())
    })
    .await
    .map_err(|error| format!("启动 Harness 执行主体未完成：{error}"))?;

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
    command: Option<String>,
) -> Result<u32, String> {
    // The settings center configures this launch target, while the visible
    // route switch in the WorkerW wallpaper is the user-facing "start DSH"
    // action. Both declared surfaces may request a launch; process ownership
    // and all executable/profile validation remain native below.
    require_wallpaper_surface(&caller)?;
    spawn_managed_dsh(state.inner(), &root_path, &profile, command.as_deref())
}

/**
 * Launchers the *automatic* path may use without an explicit user confirmation.
 *
 * A custom `command` is honoured as a single executable path (the Bridge never
 * passes it through a shell, and splits no arguments out of it), but running an
 * arbitrary configured program unattended at every wallpaper start is a
 * different trust decision from a button the user just pressed. The check is
 * stated here, at the automatic entry point, rather than left implicit in
 * `resolve_dsh_launcher` failing to find a file.
 */
#[cfg(not(feature = "lite"))]
const AUTO_START_LAUNCHER_ALLOWLIST: [&str; 4] =
    ["node.exe", "node", "pnpm.cmd", "pnpm"];

#[cfg(not(feature = "lite"))]
fn is_allowlisted_auto_start_launcher(command: Option<&str>) -> bool {
    let Some(value) = command.map(str::trim).filter(|value| !value.is_empty()) else {
        // No command configured: the built-in Node/pnpm launcher is chosen and
        // validated by `spawn_managed_dsh`.
        return true;
    };
    let file_name = std::path::Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    AUTO_START_LAUNCHER_ALLOWLIST
        .iter()
        .any(|allowed| file_name.eq_ignore_ascii_case(allowed))
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
/// The automatic path deliberately refuses a user-supplied `command` unless the
/// caller confirmed it: silently executing an arbitrary configured program at
/// every login is a different trust decision from a button the user pressed.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn autostart_managed_dsh(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, ManagedDshState>,
    autostart: tauri::State<'_, ManagedDshAutostartState>,
    root_path: Option<String>,
    profile: String,
    command: Option<String>,
    trusted_command: Option<bool>,
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

    let configured = command.as_deref().map(str::trim).filter(|value| !value.is_empty());
    if !is_allowlisted_auto_start_launcher(configured) && trusted_command != Some(true) {
        // Never silently run a custom launcher. The manual "启动" button keeps
        // working; the automatic path needs an explicit confirmation, and even
        // then the value is used as one executable path, never a shell line.
        let outcome = ManagedDshAutostart::new("command-not-confirmed");
        *record = Some(outcome.clone());
        return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
    }

    // An external DSH on 3080 belongs to the user. Do not start a second one,
    // do not take it over, and do not stop it: fall through to Bridge probing.
    if dsh_port_is_occupied() {
        let outcome = ManagedDshAutostart { outcome: "port-occupied-external".into(), pid: None, external: true };
        *record = Some(outcome.clone());
        return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
    }

    let Some(root_path) = root_path.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) else {
        let outcome = ManagedDshAutostart::new("root-path-missing");
        *record = Some(outcome.clone());
        return Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null));
    };

    let outcome = match spawn_managed_dsh(state.inner(), &root_path, &profile, configured) {
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
    } else if error.contains("3080") {
        "port-occupied-external".into()
    } else {
        "spawn-failed".into()
    }
}

/// Where a DSH started by the wallpaper keeps its own output.
///
/// DSH reports every startup failure on stderr, and a GUI-subsystem parent has
/// no usable standard streams. Inheriting those handles threw the only evidence
/// away, which left the wallpaper able to say no more than "DSH 启动后立即退出；
/// 请检查 DSH 配置或启动日志" and to point at a log that was never written. Keep
/// the child hidden, but put its own words on disk beside the wallpaper's log.
#[cfg(not(feature = "lite"))]
fn managed_dsh_log_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| {
        root.join("com.dsh.wallpaper")
            .join("logs")
            .join("managed-dsh.log")
    })
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
/// single-instance behaviour because there is only one implementation.
///
/// Reachable from `harness_launch`, which is how a chosen execution subject ends
/// up here: the shim decides *which* class to start, and every checkout still
/// starts through this one chain.
#[cfg(not(feature = "lite"))]
pub(crate) fn spawn_managed_dsh(
    state: &ManagedDshState,
    root_path: &str,
    profile: &str,
    command: Option<&str>,
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
    let configured_launcher = command.map(str::trim).filter(|value| !value.is_empty());
    let bundled_cli = root.join("apps").join("cli").join("lib").join("bin.js");
    let use_bundled_cli = configured_launcher.is_none() && bundled_cli.is_file();
    let launcher = configured_launcher.unwrap_or(if use_bundled_cli {
        "node.exe"
    } else {
        "pnpm.cmd"
    });
    let launcher_path = resolve_dsh_launcher(launcher).ok_or_else(|| {
        if configured_launcher.is_some() {
            format!("找不到自定义 DSH 启动器：{launcher}")
        } else if use_bundled_cli {
            "未找到 Node.js。请确认 node.exe 已加入系统 PATH，或在启动命令中填写 Node.js 的完整路径。"
                .to_string()
        } else {
            "未找到 pnpm。请确认 pnpm.cmd 已加入系统 PATH，或在启动命令中填写启动器的完整路径。"
                .to_string()
        }
    })?;
    let mut managed = state
        .0
        .lock()
        .map_err(|_| "DSH 进程状态不可用".to_string())?;
    if let Some(existing) = managed.as_mut() {
        match existing.child.try_wait() {
            Ok(None) => return Ok(existing.child.id()),
            Ok(Some(_)) | Err(_) => *managed = None,
        }
    }
    if dsh_port_is_occupied() {
        return Err("本机 3080 端口已被其他进程占用；请先关闭已有 DSH，再启动配置的 DSH。".into());
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
    launch.current_dir(&root);
    // DSH is a resident background service. `pnpm.cmd` otherwise inherits a new
    // visible console from the desktop process, leaving a stray CMD window
    // beside the wallpaper.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut launch, 0x08000000);
    let managed_log = managed_dsh_log_path();
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
    *managed = Some(ManagedDshProcess {
        child,
        root_path: root.to_string_lossy().into_owned(),
        profile: profile.into(),
        log_path: managed_log,
    });
    Ok(pid)
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn managed_dsh_status(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, ManagedDshState>,
) -> Result<ManagedDshStatus, String> {
    require_wallpaper_surface(&caller)?;
    let mut managed = state
        .0
        .lock()
        .map_err(|_| "DSH 进程状态不可用".to_string())?;
    let Some(process) = managed.as_mut() else {
        return Ok(ManagedDshStatus {
            managed: false,
            running: false,
            pid: None,
            root_path: None,
            profile: None,
        });
    };
    match process.child.try_wait() {
        Ok(None) => Ok(ManagedDshStatus {
            managed: true,
            running: true,
            pid: Some(process.child.id()),
            root_path: Some(process.root_path.clone()),
            profile: Some(process.profile.clone()),
        }),
        Ok(Some(status)) => {
            // The child is gone. Its own last words are the only explanation
            // available, so record them with the exit code rather than leaving a
            // bare "not running" that no one can act on.
            log::warn!(
                "managed DSH exited: pid={} code={:?}; last output:\n{}",
                process.child.id(),
                status.code(),
                managed_dsh_log_tail(process.log_path.as_ref(), 20)
            );
            *managed = None;
            Ok(ManagedDshStatus {
                managed: false,
                running: false,
                pid: None,
                root_path: None,
                profile: None,
            })
        }
        Err(error) => {
            log::warn!("managed DSH state query failed: {error}");
            *managed = None;
            Ok(ManagedDshStatus {
                managed: false,
                running: false,
                pid: None,
                root_path: None,
                profile: None,
            })
        }
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
fn stop_managed_dsh(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, ManagedDshState>,
) -> Result<(), String> {
    require_settings(&caller)?;
    let mut managed = state
        .0
        .lock()
        .map_err(|_| "DSH 进程状态不可用".to_string())?;
    let Some(mut process) = managed.take() else {
        return Ok(());
    };
    // Scope termination to the exact process spawned by this application;
    // never infer a target by probing a port or executable name.
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/PID", &process.child.id().to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = process.child.kill();
    }
    let _ = process.child.wait();
    Ok(())
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
    require_background(&caller)?;
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
        "lockScreenEnabled",
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
#[tauri::command]
#[cfg(feature = "lite")]
fn set_desktop_wallpaper_fallback(
    caller: tauri::WebviewWindow,
    enabled: bool,
) -> Result<String, String> {
    require_settings(&caller)?;
    if enabled {
        let source = desktop_fallback::bundled_sleep_source()?;
        desktop_fallback::set_fallback(Some(&source), true)
    } else {
        desktop_fallback::set_fallback(None, false)
    }
}

#[tauri::command]
#[cfg(feature = "lite")]
fn desktop_wallpaper_fallback_status(
    caller: tauri::WebviewWindow,
) -> Result<desktop_fallback::DesktopWallpaperFallbackStatus, String> {
    require_settings(&caller)?;
    Ok(desktop_fallback::status())
}

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

#[tauri::command]
async fn set_lock_screen_enabled(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<String, String> {
    require_settings(&caller)?;
    windows_integration::set_lock_screen(&app, enabled).await
}

#[tauri::command]
async fn clear_stale_lock_screen_backup(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    confirmed: bool,
) -> Result<String, String> {
    require_settings(&caller)?;
    windows_integration::clear_stale_lock_screen_backup(&app, confirmed).await
}

#[tauri::command]
async fn get_lock_screen_diagnostics(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<windows_integration::LockScreenDiagnostics, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || windows_integration::lock_screen_diagnostics(&app))
        .await
        .map_err(|error| format!("读取锁屏诊断未完成：{error}"))?
}

fn set_autostart_blocking(enabled: bool) -> Result<windows_integration::AutostartStatus, String> {
    #[cfg(windows)]
    {
        if windows_integration::set_startup_task(enabled)?.is_some() {
            // A package StartupTask is the authoritative autostart path. Drop
            // any legacy Run value left by an older build so the single-instance
            // guard does not needlessly process a second launch attempt.
            let _ = windows_integration::remove_legacy_run_entry();
            return windows_integration::autostart_status();
        }
        // 注册表 Run 键：开机自启 dsh-wallpaper
        //  开启: 写入当前构建需要的启动命令
        //  关闭: 删除该值
        // An MSIX install must record the shell's version-stable launch alias
        // rather than its own versioned `WindowsApps` path, which the next
        // package update deletes.
        if enabled {
            windows_integration::write_run_entry(
                &windows_integration::current_run_entry_command()?
            )?;
        } else {
            windows_integration::remove_legacy_run_entry()?;
        }
    }
    windows_integration::autostart_status()
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslucentTbStatus {
    installed: bool,
    running: bool,
    source: Option<String>,
}

#[cfg(windows)]
fn hide_child_console(command: &mut std::process::Command) {
    std::os::windows::process::CommandExt::creation_flags(command, 0x08000000);
}

fn translucent_tb_status_blocking() -> Result<TranslucentTbStatus, String> {
    #[cfg(windows)]
    {
        let mut tasklist = std::process::Command::new("tasklist");
        tasklist.args(["/FI", "IMAGENAME eq TranslucentTB.exe", "/FO", "CSV", "/NH"]);
        hide_child_console(&mut tasklist);
        let running = tasklist.output().ok().is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("translucenttb.exe")
        });

        let mut where_command = std::process::Command::new("where.exe");
        where_command.arg("ttb.exe");
        hide_child_console(&mut where_command);
        let alias = where_command
            .output()
            .ok()
            .is_some_and(|output| output.status.success());

        // AppX discovery is occasionally slow on a busy Windows session, so
        // this whole probe runs on a blocking worker and the child console is
        // explicitly suppressed. The settings WebView remains responsive.
        let mut packaged_command = std::process::Command::new("powershell.exe");
        packaged_command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "if (Get-AppxPackage -Name TranslucentTB -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }",
        ]);
        hide_child_console(&mut packaged_command);
        let packaged = packaged_command
            .status()
            .ok()
            .is_some_and(|status| status.success());

        return Ok(TranslucentTbStatus {
            installed: alias || packaged,
            running,
            source: if alias {
                Some("execution-alias".into())
            } else if packaged {
                Some("msix".into())
            } else {
                None
            },
        });
    }

    #[cfg(not(windows))]
    Ok(TranslucentTbStatus {
        installed: false,
        running: false,
        source: None,
    })
}

#[tauri::command]
async fn translucent_tb_status(
    caller: tauri::WebviewWindow,
) -> Result<TranslucentTbStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(translucent_tb_status_blocking)
        .await
        .map_err(|error| format!("读取透明任务栏状态未完成：{error}"))?
}

#[tauri::command]
fn launch_translucent_tb(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_settings(&caller)?;
    std::process::Command::new("ttb.exe")
        .spawn()
        .map(|_| ())
        .map_err(|_| {
            "未找到 TranslucentTB。请先从 Microsoft Store 安装并启用 ttb.exe 执行别名。".into()
        })
}

#[tauri::command]
fn open_translucent_tb_install(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_settings(&caller)?;
    // `explorer.exe <uri>` may treat the Store URI as a filesystem path and
    // open Documents instead. Ask ShellExecute to resolve the URI protocol.
    #[cfg(windows)]
    {
        let store_uri = "ms-windows-store://pdp/?ProductId=9PF4KZ2VN4W9";
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Process",
                store_uri,
            ])
            .status()
            .map_err(|error| format!("无法启动 Microsoft Store：{error}"))?;
        if status.success() {
            return Ok(());
        }

        // A Store-disabled Windows installation still gets a useful route.
        std::process::Command::new("rundll32.exe")
            .args([
                "url.dll,FileProtocolHandler",
                "https://apps.microsoft.com/detail/9PF4KZ2VN4W9",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 TranslucentTB 下载页：{error}"))
    }
    #[cfg(not(windows))]
    Err("TranslucentTB 仅支持 Windows。".into())
}

/// Opens the system-owned lock-screen settings page. During the current
/// MSIX-only test phase Windows accepts the package's bundled sleep image but
/// may reject a user-image restore snapshot; delegating the choice to Windows
/// is clearer and safer than pretending a restore has completed.
#[tauri::command]
fn open_windows_lock_screen_settings(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_settings(&caller)?;
    #[cfg(windows)]
    {
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Process",
                "ms-settings:lockscreen",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开 Windows 锁屏设置：{error}"))
    }
    #[cfg(not(windows))]
    Err("锁屏设置仅支持 Windows。".into())
}

/// Opens the Windows-owned credential prompt and stores the API key without
/// transporting plaintext through Tauri IPC or the renderer. The generic
/// CredUI target deliberately matches the `keyring` crate's default Windows
/// target naming, so the existing API client reads the same credential.
#[tauri::command]
#[cfg(not(feature = "lite"))]
fn prompt_for_api_key(caller: tauri::WebviewWindow) -> Result<bool, String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以更新 API Key。".into());
    }

    #[cfg(windows)]
    {
        use windows::{
            core::{HSTRING, PCWSTR, PWSTR},
            Win32::{
                Foundation::{ERROR_CANCELLED, FILETIME},
                Security::Credentials::{
                    CredUIPromptForCredentialsW, CredWriteW, CREDENTIALW,
                    CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST,
                    CREDUI_FLAGS_GENERIC_CREDENTIALS, CREDUI_FLAGS_PASSWORD_ONLY_OK, CREDUI_INFOW,
                    CREDUI_MAX_USERNAME_LENGTH, CRED_FLAGS, CRED_PERSIST_ENTERPRISE,
                    CRED_TYPE_GENERIC,
                },
            },
        };

        // The Windows SDK caps CredUI password input at 256 UTF-16 code units
        // plus a terminator.  DeepSeek keys are far shorter, while this avoids
        // an unbounded native buffer when a malformed value is supplied.
        const API_KEY_BUFFER_LEN: usize = 257;
        const CREDENTIAL_TARGET: &str = "deepseek-api.dsh-wallpaper";
        const CREDENTIAL_USERNAME: &str = "deepseek-api";

        let caption = HSTRING::from("更新 DeepSeek API Key");
        let message =
            HSTRING::from("请输入 DeepSeek API Key。该密钥仅保存到当前 Windows 用户的凭据管理器。");
        let target = HSTRING::from(CREDENTIAL_TARGET);
        let username = HSTRING::from(CREDENTIAL_USERNAME);
        let mut password = [0u16; API_KEY_BUFFER_LEN];
        // Password-only mode must not surface a username control, but CredUI
        // still requires a writable username buffer. Give it the documented
        // maximum rather than risking ERROR_INSUFFICIENT_BUFFER on a Windows
        // implementation that fills a default account name internally.
        let mut credential_username = [0u16; CREDUI_MAX_USERNAME_LENGTH as usize + 1];
        let parent = caller
            .hwnd()
            .map_err(|error| format!("无法关联 Windows 凭据对话框：{error}"))?;
        let ui_info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            hwndParent: parent,
            pszMessageText: PCWSTR(message.as_ptr()),
            pszCaptionText: PCWSTR(caption.as_ptr()),
            hbmBanner: Default::default(),
        };
        let flags = CREDUI_FLAGS_GENERIC_CREDENTIALS
            | CREDUI_FLAGS_ALWAYS_SHOW_UI
            | CREDUI_FLAGS_PASSWORD_ONLY_OK
            // We deliberately make CredUI return the password to this native
            // function, then write the exact Generic Credential target below.
            // Microsoft documents that this is the supported path for
            // inspecting a returned password; it is wiped immediately after
            // CredWriteW and never enters WebView IPC.
            | CREDUI_FLAGS_DO_NOT_PERSIST;
        let result = unsafe {
            CredUIPromptForCredentialsW(
                Some(&ui_info),
                &target,
                None,
                0,
                &mut credential_username,
                &mut password,
                None,
                flags,
            )
        };

        if result == ERROR_CANCELLED {
            secure_zero_u16(&mut password);
            secure_zero_u16(&mut credential_username);
            return Ok(false);
        }
        if result.0 != 0 {
            secure_zero_u16(&mut password);
            secure_zero_u16(&mut credential_username);
            return Err(format!("Windows 凭据输入失败（错误代码 {}）。", result.0));
        }

        let save_result = (|| -> Result<(), String> {
            let password_len = password
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(password.len());
            if password_len == 0 {
                return Err("API Key 不能为空。".into());
            }
            // Match `keyring`'s Windows Generic Credential representation:
            // its `set_password` serializes password UTF-16 code units as a
            // little-endian blob and `get_password` reverses that encoding.
            let mut password_blob = credential_blob_from_prompt_password(&password[..password_len]);
            let byte_len = u32::try_from(password_blob.len()).map_err(|_| "API Key 长度无效。")?;
            let mut credential = CREDENTIALW {
                Flags: CRED_FLAGS::default(),
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target.as_ptr() as *mut u16),
                Comment: PWSTR::null(),
                LastWritten: FILETIME::default(),
                CredentialBlobSize: byte_len,
                CredentialBlob: password_blob.as_mut_ptr(),
                Persist: CRED_PERSIST_ENTERPRISE,
                AttributeCount: 0,
                Attributes: std::ptr::null_mut(),
                TargetAlias: PWSTR::null(),
                UserName: PWSTR(username.as_ptr() as *mut u16),
            };
            // `keyring::Entry::new("dsh-wallpaper", "deepseek-api")`
            // resolves to this target on Windows. Keep the credential write
            // native so the renderer never sees plaintext.
            let result = unsafe { CredWriteW(&mut credential, 0) }
                .map_err(|error| format!("无法保存 API Key 到 Windows 凭据管理器：{error}"));
            secure_zero_bytes(&mut password_blob);
            result
        })();
        secure_zero_u16(&mut password);
        secure_zero_u16(&mut credential_username);
        save_result?;
        Ok(true)
    }

    #[cfg(not(windows))]
    {
        let _ = caller;
        Err("API Key 原生凭据输入仅支持 Windows。".into())
    }
}

#[cfg(all(test, windows))]
#[cfg(not(feature = "lite"))]
mod api_credential_tests {
    use super::credential_blob_from_prompt_password;

    #[test]
    fn serializes_the_same_utf16_little_endian_shape_as_keyring_windows() {
        assert_eq!(
            credential_blob_from_prompt_password(&[0x0073, 0x006B, 0x4F60]),
            vec![0x73, 0x00, 0x6B, 0x00, 0x60, 0x4F]
        );
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
) -> Result<String, String> {
    require_background(&caller)?;
    // The session follows the endpoint selected in settings, resolved here rather
    // than trusted from the caller: the renderer cannot name an arbitrary port,
    // and the monitor reads the same value, so status and sessions cannot
    // disagree about which client is in use.
    let endpoint_port = Some(harness_endpoint_port());
    chat::harness_connect(app, state, resume_session_id, connection_id, model, endpoint_port).await
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
    require_background(&caller)?;
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

/// The three DSH client shapes listen on different ports, and only some of them
/// are configurable, so probing one hardcoded port means "only ever connect to
/// the CLI shape". The official desktop shell compiles 19387 into its asar; the
/// community desktop defaults to 43120; the CLI and the official web app both
/// default to 3080 (`ctx.webStartup.port ?? 3080`).
///
/// Order is the shipped priority: official desktop, then community desktop, then
/// plain web/CLI. The renderer applies the same order, and
/// `wallpaper/src/connect/endpoints.ts` documents it.
#[cfg(not(feature = "lite"))]
const HARNESS_ENDPOINT_PORTS: &[(u16, &str)] =
    &[(19387, "official-desktop"), (43120, "community-desktop"), (3080, "official-web")];

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
) -> Result<serde_json::Value, String> {
    {
        let mut guard = state.pinned.lock().map_err(|_| "接入端点状态不可用".to_string())?;
        *guard = port;
    }
    let subject_id = subject_id.unwrap_or_default().trim().to_string();
    let ports = harness_targets::subject_endpoint_ports(&subject_id, extra_ports);
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
) -> Result<serde_json::Value, String> {
    require_wallpaper_surface(&caller)?;
    if port == Some(0) {
        return Err("接入端点端口必须在 1-65535 之间".into());
    }
    let state = harness_endpoint_state();
    apply_endpoint_scope(state, port, subject_id.as_deref(), &extra_ports.unwrap_or_default())
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
        classify_dsh_launch_failure, is_allowlisted_auto_start_launcher, resolve_dsh_launcher,
    };

    /// The automatic path may only use a launcher it knows, unless the user has
    /// confirmed a custom one. This is a trust boundary, so each case is pinned:
    /// a bare name is allowlisted, an absolute path to the same binary is
    /// allowlisted, and anything else is not.
    #[test]
    fn only_known_launchers_are_allowed_without_confirmation() {
        // No command configured: the built-in launcher is chosen further down and
        // validated by `spawn_managed_dsh`.
        assert!(is_allowlisted_auto_start_launcher(None));
        assert!(is_allowlisted_auto_start_launcher(Some("   ")));

        for allowed in [
            "node.exe",
            "node",
            "pnpm.cmd",
            "pnpm",
            "NODE.EXE",
            r"C:\Program Files\nodejs\node.exe",
            r"D:\tools\pnpm.cmd",
            "/usr/bin/node",
        ] {
            assert!(is_allowlisted_auto_start_launcher(Some(allowed)), "{allowed}");
        }

        // A different program, or one that merely mentions an allowed name, must
        // not slip through: the comparison is on the file name, not a substring.
        for rejected in [
            "powershell.exe",
            "cmd.exe",
            r"C:\tools\mynode.exe",
            "node-wrapper.exe",
            "python.exe",
            "evil-node.exe.bat",
        ] {
            assert!(!is_allowlisted_auto_start_launcher(Some(rejected)), "{rejected}");
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
        assert_eq!(classify_dsh_launch_failure("本机 3080 端口已被其他进程占用"), "port-occupied-external");
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
        *state.pinned.lock().expect("pin lock") = Some(43120);
        assert_eq!(candidates_of(&state), vec![43120]);
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
        apply_endpoint_scope(&state, None, Some("shell:com.deepseek.dsh"), &[]).expect("scope");
        // The official shell owns 19387 and nothing else, even though the shipped
        // order also holds 43120 and 3080.
        assert_eq!(candidates_of(&state), vec![19387]);

        // A checkout owns DSH's default plus the ports the user added for it.
        apply_endpoint_scope(&state, None, Some(r"D:\tree"), &[3081]).expect("scope");
        assert_eq!(candidates_of(&state), vec![HARNESS_DEFAULT_PORT, 3081]);

        // An unknown shell is *configured* with nowhere to look, which is reported
        // as unreachable rather than as a reason to fall back to another client.
        apply_endpoint_scope(&state, None, Some("shell:com.unknown.client"), &[]).expect("scope");
        assert!(candidates_of(&state).is_empty());
        assert!(harness_endpoint_configured_on(&state));
    }

    /// Changing the subject is the one moment the wallpaper may move, and it does
    /// not carry the previous endpoint across.
    #[test]
    fn changing_the_subject_clears_the_endpoint_in_use() {
        let state = HarnessEndpointState::default();
        apply_endpoint_scope(&state, None, Some("shell:com.deepseek.dsh"), &[]).expect("scope");
        assert_eq!(note_in_use_on(&state, 19387)[0], 19387);
        // Same subject again: the endpoint it is already using stays first.
        apply_endpoint_scope(&state, None, Some("shell:com.deepseek.dsh"), &[]).expect("scope");
        assert_eq!(candidates_of(&state)[0], 19387);
        // A different subject starts from its own order.
        apply_endpoint_scope(&state, None, Some("shell:ai.deepseek.dsh.desktop"), &[]).expect("scope");
        assert_eq!(candidates_of(&state), vec![43120]);
        assert_eq!(*state.active.lock().expect("active lock"), None);
    }

    /// A pin never records an observation, so clearing it cannot revive one.
    #[test]
    fn nothing_is_recorded_while_pinned() {
        let state = HarnessEndpointState::default();
        *state.pinned.lock().expect("pin lock") = Some(43120);
        note_in_use_on(&state, 19387);
        assert_eq!(*state.active.lock().expect("active lock"), None);
    }

    /// No configuration at all is a different fact from a configuration that cannot
    /// be placed, and the two read differently to the user.
    #[test]
    fn nothing_configured_is_not_the_same_as_configured_but_unreachable() {
        let state = HarnessEndpointState::default();
        assert!(!harness_endpoint_configured_on(&state));
        apply_endpoint_scope(&state, None, Some("   "), &[]).expect("scope");
        assert!(!harness_endpoint_configured_on(&state));
        // A pin alone is a configuration too.
        apply_endpoint_scope(&state, Some(3080), Some(""), &[]).expect("scope");
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
    client_window::open_loopback_url(port, path.as_deref().unwrap_or("/"))
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
        loop {
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
            if publish || reason_for_state != last_reason {
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
            set_desktop_wallpaper_fallback,
            desktop_wallpaper_fallback_status,
            set_lock_screen_enabled,
            clear_stale_lock_screen_backup,
            get_lock_screen_diagnostics,
            set_autostart,
            autostart_status,
            translucent_tb_status,
            launch_translucent_tb,
            open_translucent_tb_install,
            open_windows_lock_screen_settings,
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
            notify_appearance_changed,
            set_lock_screen_enabled,
            clear_stale_lock_screen_backup,
            get_lock_screen_diagnostics,
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
            translucent_tb_status,
            launch_translucent_tb,
            open_translucent_tb_install,
            open_windows_lock_screen_settings,
            prompt_for_api_key,
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
            appearance::commands::appearance_resolve_library_asset
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

        // 3. Stop only the DSH child this process launched. An external DSH on
        //    3080 belongs to the user and is never touched.
        #[cfg(not(feature = "lite"))]
        if let Some(state) = app.try_state::<ManagedDshState>() {
            if let Ok(mut managed) = state.0.lock() {
                if let Some(mut process) = managed.take() {
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("taskkill.exe")
                            .args(["/PID", &process.child.id().to_string(), "/T", "/F"])
                            .output();
                    }
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
