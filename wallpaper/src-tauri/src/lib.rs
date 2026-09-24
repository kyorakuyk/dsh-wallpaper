#[cfg(not(feature = "lite"))]
mod api_persistence;
mod app_core;
#[cfg(not(feature = "lite"))]
mod appearance;
#[cfg(not(feature = "lite"))]
mod chat;
#[cfg(not(feature = "lite"))]
mod deepseek_web;
#[cfg(not(feature = "lite"))]
mod deepseek_web_config;
mod desktop_fallback;
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
use std::process::Child;
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

/// Shared, validated launch path for the manual button and the autostart
/// setting. Both callers get identical validation, ownership tracking and
/// single-instance behaviour because there is only one implementation.
#[cfg(not(feature = "lite"))]
fn spawn_managed_dsh(
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
    }    launch.current_dir(&root);
    // DSH is a resident background service. `pnpm.cmd` otherwise inherits a
    // new visible console from the desktop process, leaving a stray CMD
    // window beside the wallpaper. Keep the child hidden while preserving its
    // stdout/stderr for the process lifetime and managed PID tracking.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut launch, 0x08000000);
    let child = launch
        .spawn()
        .map_err(|error| format!("无法启动 DSH：{error}"))?;
    let pid = child.id();
    *managed = Some(ManagedDshProcess {
        child,
        root_path: root.to_string_lossy().into_owned(),
        profile: profile.into(),
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
        Ok(Some(_)) | Err(_) => {
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
        use std::process::Command;
        if windows_integration::set_startup_task(enabled)?.is_some() {
            // A package StartupTask is the authoritative autostart path. Drop
            // any legacy Run value left by an older build so the single-instance
            // guard does not needlessly process a second launch attempt.
            let _ = windows_integration::remove_legacy_run_entry();
            return windows_integration::autostart_status();
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // 注册表 Run 键：开机自启 dsh-wallpaper
        //  add:    reg add HKCU\...\Run /V dsh-wallpaper /D "<exe>" /F
        //  delete: reg delete HKCU\...\Run /V dsh-wallpaper /F
        let mut cmd = Command::new("reg");
        // `reg.exe` is only a compatibility fallback. Keep it out of the
        // user's desktop even when the host is a GUI-subsystem process.
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x08000000);
        if enabled {
            cmd.args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "dsh-wallpaper",
                "/D",
                &exe.to_string_lossy(),
                "/F",
            ]);
        } else {
            cmd.args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/V",
                "dsh-wallpaper",
                "/F",
            ]);
        }
        let status = cmd.status().map_err(|e| e.to_string())?;
        if !status.success() && (enabled || windows_integration::legacy_run_entry_present()?) {
            return Err("更新当前用户开机自启失败".into());
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
    chat::harness_connect(app, state, resume_session_id, connection_id, model).await
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
async fn harness_presets(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_presets().await
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
async fn fetch_harness_status() -> serde_json::Value {
    let Some(client) = harness_probe_client() else {
        return serde_json::json!({ "availability": "offline" });
    };
    if let Ok(response) = client
        .get("http://127.0.0.1:3080/api/wallpaper/v1/status")
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
        .get("http://127.0.0.1:3080/")
        .send()
        .await
        .ok()
        .map(|response| response.status());
    match root_probe_availability(root_status) {
        // A root endpoint proves only that an HTTP service accepted a
        // successful request. Authentication failures and error pages must
        // not turn an unrelated process on 3080 into a misleading “DSH
        // online” state.
        HarnessAvailability::WebOnly => serde_json::json!({
            "availability": "web-only",
            "reasonCode": "bridge-status-missing",
        }),
        _ => serde_json::json!({ "availability": "offline" }),
    }
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
mod harness_status_tests {
    use super::{compatible_harness_bridge_status, diagnose_harness_bridge_status, root_probe_availability};
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

    #[test]
    fn names_the_layer_that_is_unusable() {
        // A protocol this wallpaper cannot speak.
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

#[cfg(not(feature = "lite"))]
fn start_harness_monitor(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut consecutive_successes = 0u8;
        let mut consecutive_failures = 0u8;
        let mut last = HarnessAvailability::Offline;
        let mut last_reason: Option<String> = None;
        loop {
            let status = fetch_harness_status().await;
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
            // A compatible Bridge is the only successful probe. Every other
            // state, including `web-only` and `bridge-loading`, remains useful
            // diagnostic information but must settle through the same failure
            // path so a stale ready state cannot keep Harness selectable after
            // the bridge disappears.
            let bridge_ready = observed == HarnessAvailability::BridgeReady;
            let changed = last != observed || last_reason != reason;
            let settled = if bridge_ready {
                consecutive_successes = consecutive_successes.saturating_add(1);
                consecutive_failures = 0;
                consecutive_successes >= 2
            } else {
                consecutive_failures = consecutive_failures.saturating_add(1);
                consecutive_successes = 0;
                consecutive_failures >= 3
            };
            if changed && settled {
                log::info!(
                    "harness availability changed: {:?} -> {:?} (reason {:?})",
                    last,
                    observed,
                    reason
                );
                last = observed;
                last_reason = reason.clone();
                if let Some(core) = app.try_state::<AppCore>() {
                    let snapshot = core.dispatch(AppAction::SetHarnessDiagnostic {
                        availability: observed,
                        reason_code: reason,
                    });
                    emit_app_snapshot(&app, &snapshot);
                }
            }
            // A resident wallpaper must not hammer a dead loopback port. A
            // single cadence also keeps the two stabilisation thresholds
            // comparable in wall-clock time.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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
            send_chat,
            cancel_chat,
            connect_harness,
            harness_history,
            harness_presets,
            harness_set_preset,
            harness_controls,
            harness_set_permission,
            api_history,
            list_api_conversations,
            delete_api_conversation,
            clear_api_history,
            probe_harness,
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

        log::info!("dsh-wallpaper native state restored on exit");
    });
}
