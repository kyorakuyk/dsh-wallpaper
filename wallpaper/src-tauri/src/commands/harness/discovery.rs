use crate::*;
use crate::commands::require_wallpaper_surface;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(not(feature = "lite"))]
pub(crate) struct DshPathCandidate {
    pub(crate) root_path: String,
    pub(crate) source: String,
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
pub(crate) fn scan_dsh_paths_blocking(hint_path: Option<String>, deep_scan: bool) -> Vec<DshPathCandidate> {
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
pub(super) fn resolve_dsh_launcher(value: &str) -> Option<PathBuf> {
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
pub(super) fn dsh_port_is_occupied(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(250),
    )
    .is_ok()
}

/// Only the settings surface is allowed to request the API-key prompt. This
/// is a defense in depth check: it prevents the desktop wallpaper WebView (or
/// a future WebView) from opening a native credential capture dialog.

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn scan_dsh_paths(
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
pub(crate) async fn scan_harness_targets(
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
pub(crate) fn harness_target_catalog(
    caller: tauri::WebviewWindow,
) -> Result<Option<harness_catalog::HarnessTargetCatalog>, String> {
    require_wallpaper_surface(&caller)?;
    Ok(harness_catalog::load_catalog())
}
