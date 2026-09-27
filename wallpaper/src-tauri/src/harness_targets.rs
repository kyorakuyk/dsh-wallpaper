//! The shim: which harness actually executes on this machine, and how each one
//! is recognised.
//!
//! The wallpaper used to model exactly one harness shape — a source checkout it
//! starts with `node`/`pnpm`. The two Electron shells (the official client and
//! the third-party desktop client) bundle their *own* checkout, so they were
//! invisible: the wallpaper could neither see them nor raise them. This module
//! is that missing half, and it answers one question: *which execution subjects
//! are available here?* The design it implements is frozen in
//! `docs/design/harness-subject-and-ui-design.md` (§3–§4).
//!
//! Two classes, and only two:
//!
//! | class | subject | identity | launch |
//! | --- | --- | --- | --- |
//! | 自带检出 [`HarnessTargetKind::EmbeddedShell`] | official / third-party desktop client | AUMID | `shell:AppsFolder\<AUMID>` |
//! | 非自带检出 [`HarnessTargetKind::Checkout`] | a source tree | directory fingerprint | the managed launch chain |
//!
//! Three rules from the design shape every function here:
//!
//! * **No baked-in path.** A shell is addressed by its AUMID, so re-installing
//!   it cannot invalidate anything the renderer stored (§4.4). For a shell, `id`
//!   and the launch alias are location-independent; a checkout's path *is* its
//!   identity, which is why it is the one target kind that carries a path.
//! * **One fingerprint, one place.** The checkout fingerprint is
//!   [`crate::scan_dsh_paths_blocking`] — not a second copy that could drift
//!   away from the one the settings picker already shows.
//! * **Facts here, wording there.** This module returns labels only where a
//!   label is an identity (the two known shells reuse the strings
//!   `wallpaper/src/connect/endpoints.ts` already shows); every sentence the
//!   user reads is composed by the renderer.
//!
//! Deliberately *not* here yet (next slices, see §9 of the design): the protocol
//! fingerprint (which subject is answering right now), persistence of the scan
//! result, and the launch/raise actions. `identity.default_ports` is carried as
//! a probe *hint* only — ports are defaults, never a contract (§4.5).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Which of the two frozen classes an execution subject belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessTargetKind {
    /// A shell that ships its own checkout. Choosing it fixes the service *and*
    /// the window at once; the two cannot be split (§3).
    EmbeddedShell,
    /// A source tree. The service is this tree; the window is chosen separately,
    /// because a checkout has none of its own.
    Checkout,
    /// A DSH CLI installed on this machine rather than a tree the user owns — the
    /// `npm i -g @deepseek-ai/dsh` shape. It has no AUMID (it is not a shell) and no
    /// source tree (it ships packed), so it is neither of the two classes above; what
    /// it *does* have is a launcher on `PATH`, and that launcher is its identity.
    ///
    /// The service it starts is the same web/app shape a checkout starts — same
    /// profile, same default port — which is why its `client` is `OfficialWeb`.
    InstalledCli,
}

/// The client shapes the endpoint scanner already knows.
///
/// The three strings match `HarnessClientKind` in
/// `wallpaper/src/connect/endpoints.ts` on purpose: the settings UI already has
/// one vocabulary for "official desktop / official web", and a second one here
/// would eventually disagree with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessClientKind {
    OfficialDesktop,
    OfficialWeb,
}

/// How the wallpaper starts a subject.
///
/// One enum value per class, because launch genuinely cannot be unified (§10):
/// the shells are resolved by the Windows shell through a name that survives
/// their updates, while a checkout is a process the wallpaper has to spawn and
/// own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchRecipeKind {
    /// Hand the AUMID alias to the shell. This is the same indirection the
    /// autostart entry uses, and the reason a client update cannot break it.
    AppsFolder,
    /// Spawn the checkout's own launcher through the managed launch chain.
    ManagedCommand,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecipe {
    pub kind: LaunchRecipeKind,
    /// The location-independent token `AppsFolder` resolves at launch time
    /// (`shell:AppsFolder\<AUMID>`), or `None` for a checkout, whose launch
    /// input is its root path.
    pub alias: Option<String>,
}

/// What the scanner matched this target on.
///
/// Exactly one of the two identity fields is set, and which one is set *is* the
/// class: a shell is known by name, a checkout by location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetIdentity {
    /// The shell's `System.AppUserModel.ID`. Both a fingerprint — it is how the
    /// shell registration is recognised — and the launch token.
    pub aumid: Option<String>,
    /// The checkout's canonical root path. One of the few places a path belongs:
    /// a source tree has no other identity.
    pub root_path: Option<String>,
    /// Default ports for this shape, as a hint for the endpoint probe.
    ///
    /// Never a contract: a user may move any of them, which is why discovery
    /// also has to scan arbitrary loopback ports (§4.5).
    pub default_ports: Vec<u16>,
}

/// What this target can be asked to do, as measured rather than assumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCapabilities {
    /// Launching it again reuses the running shell instead of opening a second
    /// window.
    ///
    /// Measured for the official client (`requestSingleInstanceLock`), and
    /// measured *absent* in the third-party client, which is therefore never
    /// re-launched to recover a window it lost (§6.1).
    pub single_instance: bool,
    /// It owns a Windows window, so 「拉起 UI」 can raise it. A checkout has no
    /// window of its own; its interface is the browser.
    pub owns_window: bool,
    /// It can be started with its window kept out of sight, so "开机只供能" is
    /// possible for it (§5.1).
    pub can_start_hidden: bool,
    /// It needs the `profile` setting, which belongs to the checkout path only
    /// (§4.7).
    pub needs_profile: bool,
}

/// One execution subject: what it is, how it was found, and how to start it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessTarget {
    /// The stable key the renderer stores and later resolves back to a target.
    /// Location-independent for a shell.
    pub id: String,
    pub kind: HarnessTargetKind,
    pub client: HarnessClientKind,
    pub label: String,
    /// Where the scan found it: a checkout's scan origin, exactly as
    /// `scan_dsh_paths` reports it, or the shortcut directory a shell was
    /// registered from.
    pub source: String,
    pub identity: TargetIdentity,
    pub launch: LaunchRecipe,
    pub capabilities: TargetCapabilities,
}

/// The outcome of one scan, in the shape the settings surface needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessTargetScan {
    /// Shells first (in priority order), then the checkouts the scan found in
    /// its own order — so the user's configured root still leads.
    pub targets: Vec<HarnessTarget>,
    /// True when more than one source tree exists. The wallpaper must not pick
    /// one for the user; the settings surface asks instead (§4.3).
    pub requires_subject_choice: bool,
}

/// A shortcut whose `System.AppUserModel.ID` the Windows shell reported.
///
/// A separate type so the matching rules can be tested without a Start Menu:
/// reading the property needs COM, deciding what it means does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannedShortcut {
    pub aumid: String,
    /// The directory the shortcut was found in, for the settings row.
    pub directory: String,
}

/// A shell this build knows how to start.
///
/// The list is a *matching* table, not a path table: nothing here names an
/// install location, so moving or reinstalling a client changes only the AUMID
/// the shell registration carries (*it does not change at all*).
struct ShellAppSpec {
    aumid: &'static str,
    /// Reuses the wording `endpoints.ts` already shows for this shape.
    label: &'static str,
    client: HarnessClientKind,
    default_ports: &'static [u16],
    single_instance: bool,
    can_start_hidden: bool,
}

/// 官方客户端 —— 本 build 唯一按 AUMID 寻址的壳。
///
/// 第三方桌面客户端（`ai.deepseek.dsh.desktop`，默认 43120）2026-09-27 按用户要求移除：
/// 实测它把整台本地 HTTP 服务放在自己的授权之后，用 bridge token 打过去一律 403（连 `/` 都进不去），
/// 所以它永远点不亮；留在"可选择的主体"里只是误导。
const SHELL_APPS: &[ShellAppSpec] = &[
    ShellAppSpec {
        aumid: "com.deepseek.dsh",
        label: "官方桌面客户端",
        client: HarnessClientKind::OfficialDesktop,
        default_ports: &[19387],
        // The official shell calls `requestSingleInstanceLock`, so a second
        // launch focuses the running window instead of adding one, and it
        // supports starting without showing the window.
        single_instance: true,
        can_start_hidden: true,
    },
];

/// Prefix of every shell target id. One definition, because the launcher parses
/// this namespace back out of a stored id and the two spellings must not drift.
pub(crate) const SHELL_ID_PREFIX: &str = "shell:";

/// The alias the Windows shell resolves through the current registration.
fn apps_folder_alias(aumid: &str) -> String {
    format!(r"shell:AppsFolder\{aumid}")
}

/// The launch-relevant facts about a shell this build knows how to start.
///
/// Returned by [`known_shell`] so the launcher never has to re-read the table or
/// trust a caller-supplied alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShellApp {
    /// Canonical spelling, as this build knows the client.
    pub aumid: &'static str,
    /// The canonical launch alias.
    pub alias: String,
    /// Default port, as a probe hint for waiting on a start.
    pub default_port: Option<u16>,
    /// Whether an unattended start may keep the window out of sight.
    pub can_start_hidden: bool,
    /// Whether launching it again focuses the instance already running.
    ///
    /// This is the one reliable way to reach a single-instance shell's window when
    /// the window cannot be resolved directly (§6.1): the shell itself brings the
    /// window it already owns to the front.
    pub single_instance: bool,
}

/// Look up a shell by AUMID, case-insensitively.
///
/// This lookup *is* the allowlist. It keeps a renderer from asking the Windows
/// shell to start an arbitrary Start Menu item (the alias string is a launch
/// request, so an unvalidated one would be a general-purpose "run this app"
/// primitive), and it is why the alias is taken from this table rather than from
/// whatever the caller passed.
pub(crate) fn known_shell(aumid: &str) -> Option<ShellApp> {
    SHELL_APPS
        .iter()
        .find(|spec| spec.aumid.eq_ignore_ascii_case(aumid.trim()))
        .map(|spec| ShellApp {
            aumid: spec.aumid,
            alias: apps_folder_alias(spec.aumid),
            default_port: spec.default_ports.first().copied(),
            can_start_hidden: spec.can_start_hidden,
            single_instance: spec.single_instance,
        })
}

/// Every default port a known shell carries.
///
/// A slice rather than the single [`ShellApp::default_port`] probe hint, because a
/// caller deciding *which endpoints a subject may be reached on* needs the whole
/// set: the hint answers "where to wait after a start", this answers "where this
/// client can be talked to at all", and conflating them is how one port silently
/// became the contract §4.5 forbids.
pub(crate) fn known_shell_ports(aumid: &str) -> Option<Vec<u16>> {
    SHELL_APPS
        .iter()
        .find(|spec| spec.aumid.eq_ignore_ascii_case(aumid.trim()))
        .map(|spec| spec.default_ports.to_vec())
}

/// The ports a configured execution subject may be reached on, in order.
///
/// This is the native half of the frozen "no substitution" rule, and its renderer
/// mirror is `subjectEndpointPorts` in `wallpaper/src/connect/endpoints.ts`. The
/// set is derived from the subject alone, never from what a probe finds:
///
/// * a shell owns the port compiled into it — the port is part of the client, so a
///   second one would mean a second client, which is also why a checkout's added
///   ports deliberately do not apply to it;
/// * a source tree owns DSH's own web default, plus the ports the user added for
///   it by hand (adding one *is* the user saying which subject answers there);
/// * an unknown shell AUMID yields an empty set, which is not the same as "nothing
///   configured" — see below.
///
/// The empty return is loaded with meaning, and there are two of them, so callers
/// must pass the subject id rather than only this list: an **empty id** means
/// "nothing chosen yet", the one case where the shipped priority order applies,
/// while an **empty set for a non-empty id** means the subject is configured but
/// this build cannot say where it answers, which must be reported as unreachable
/// rather than as a reason to fall back to another client.
pub(crate) fn subject_endpoint_ports(subject_id: &str, extra_ports: &[u16]) -> Vec<u16> {
    let subject = subject_id.trim();
    if subject.is_empty() {
        return Vec::new();
    }
    if let Some(aumid) = subject.strip_prefix(SHELL_ID_PREFIX) {
        return known_shell_ports(aumid).unwrap_or_default();
    }
    let mut ports = vec![crate::HARNESS_DEFAULT_PORT];
    for port in extra_ports {
        if *port != 0 && !ports.contains(port) {
            ports.push(*port);
        }
    }
    ports
}

/// Build the target for one known shell.
fn shell_target(spec: &ShellAppSpec, source: String) -> HarnessTarget {
    HarnessTarget {
        // Location-independent by construction: no version, no directory, and
        // therefore nothing for an update to invalidate.
        id: format!("{SHELL_ID_PREFIX}{}", spec.aumid.to_ascii_lowercase()),
        kind: HarnessTargetKind::EmbeddedShell,
        client: spec.client,
        label: spec.label.into(),
        source,
        identity: TargetIdentity {
            aumid: Some(spec.aumid.into()),
            root_path: None,
            default_ports: spec.default_ports.to_vec(),
        },
        launch: LaunchRecipe {
            kind: LaunchRecipeKind::AppsFolder,
            alias: Some(apps_folder_alias(spec.aumid)),
        },
        capabilities: TargetCapabilities {
            single_instance: spec.single_instance,
            owns_window: true,
            can_start_hidden: spec.can_start_hidden,
            needs_profile: false,
        },
    }
}

/// Build the target for one source checkout.
///
/// `source` is the scan's own origin string ("当前设置路径", "常见项目目录", …),
/// kept verbatim so this list and the settings picker explain a candidate the
/// same way.
fn checkout_target(root_path: &str, source: &str) -> HarnessTarget {
    let label = Path::new(root_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| root_path.to_string());
    HarnessTarget {
        // For a checkout the path *is* the identity: there is no name that
        // survives the tree being moved, rebuilt, or re-cloned.
        id: root_path.to_string(),
        kind: HarnessTargetKind::Checkout,
        // A source tree started as `dsh web` is the shape the scanner calls
        // official web: no window of its own, interface in the browser.
        client: HarnessClientKind::OfficialWeb,
        label,
        source: source.to_string(),
        identity: TargetIdentity {
            aumid: None,
            root_path: Some(root_path.to_string()),
            default_ports: vec![crate::HARNESS_DEFAULT_PORT],
        },
        launch: LaunchRecipe {
            kind: LaunchRecipeKind::ManagedCommand,
            alias: None,
        },
        capabilities: TargetCapabilities {
            // "Single instance" for a checkout is not a property of a client but
            // of the port it listens on, and that is handled by the managed
            // launch chain (an external DSH on the port is left alone, §4.6).
            single_instance: false,
            owns_window: false,
            can_start_hidden: true,
            needs_profile: true,
        },
    }
}

/// Prefix of a globally installed CLI's target id.
///
/// Like `SHELL_ID_PREFIX`, the launcher parses this namespace back out of a stored
/// id, so the two spellings must not drift.
pub(crate) const CLI_ID_PREFIX: &str = "cli:";

/// One globally installed DSH CLI, as a subject.
///
/// Mirrors `checkout_target` deliberately — same service shape, same default port,
/// and the same "what you must name is its identity" rule. The only difference is
/// what gets named: a launcher on `PATH` instead of a tree on disk.
fn installed_cli_target(launcher: &Path) -> HarnessTarget {
    let path = launcher.to_string_lossy().into_owned();
    HarnessTarget {
        // A globally installed CLI has no tree to point at, so its launcher is what
        // survives: reinstalling the package rewrites that file in place.
        id: format!("{CLI_ID_PREFIX}{path}"),
        kind: HarnessTargetKind::InstalledCli,
        client: HarnessClientKind::OfficialWeb,
        label: "已安装的 DSH CLI".to_string(),
        // The settings row shows where a subject came from; for this class that is
        // the launcher itself, which is also its identity.
        source: path,
        identity: TargetIdentity {
            aumid: None,
            root_path: None,
            default_ports: vec![crate::HARNESS_DEFAULT_PORT],
        },
        launch: LaunchRecipe {
            kind: LaunchRecipeKind::ManagedCommand,
            alias: None,
        },
        capabilities: TargetCapabilities {
            // Single instance is a property of the port, not of this shape, and the
            // managed chain already leaves an external DSH on that port alone.
            single_instance: false,
            owns_window: false,
            can_start_hidden: true,
            needs_profile: true,
        },
    }
}

/// Match the scanned shortcuts against the shells this build can start.
///
/// Unknown AUMIDs are ignored: they belong to applications that have nothing to
/// do with the harness, and reporting them would turn a settings list into an
/// inventory of the Start Menu.
fn shell_targets_from(shortcuts: &[ScannedShortcut]) -> Vec<HarnessTarget> {
    let mut targets = Vec::new();
    for spec in SHELL_APPS {
        // The same application can be registered more than once (a Start Menu
        // entry and a desktop shortcut); the first registration is still one
        // subject, which is why this looks a match up instead of collecting.
        let Some(shortcut) = shortcuts
            .iter()
            .find(|shortcut| shortcut.aumid.eq_ignore_ascii_case(spec.aumid))
        else {
            continue;
        };
        targets.push(shell_target(spec, shortcut.directory.clone()));
    }
    targets
}

/// Assemble one scan result from the two fingerprint sources.
///
/// Visible to the crate so the catalogue's tests can record a scan built by the
/// model's own matching rules instead of hand-made targets.
pub(crate) fn build_scan_for_tests(shortcuts: &[ScannedShortcut]) -> HarnessTargetScan {
    build_scan(shortcuts, &[], &[])
}

fn build_scan(
    shortcuts: &[ScannedShortcut],
    checkouts: &[crate::DshPathCandidate],
    installed_clis: &[PathBuf],
) -> HarnessTargetScan {
    let mut targets = shell_targets_from(shortcuts);
    let mut seen = HashSet::new();
    for checkout in checkouts {
        // Defence in depth: the directory scan already deduplicates
        // case-insensitively, and a second key here keeps a future caller from
        // handing this list the same tree twice.
        if !seen.insert(checkout.root_path.to_ascii_lowercase()) {
            continue;
        }
        targets.push(checkout_target(&checkout.root_path, &checkout.source));
    }
    for launcher in installed_clis {
        // Same defence, same key shape: the discovery already deduplicates, and this
        // keeps a future caller from listing one launcher twice.
        if !seen.insert(launcher.to_string_lossy().to_ascii_lowercase()) {
            continue;
        }
        targets.push(installed_cli_target(launcher));
    }
    let requires_subject_choice = targets
        .iter()
        .filter(|target| target.kind == HarnessTargetKind::Checkout)
        .count()
        > 1;
    HarnessTargetScan {
        targets,
        requires_subject_choice,
    }
}

/// Scan this machine for harness execution subjects.
///
/// Blocking by design: it walks the filesystem and reads shell properties, so
/// the command wrapper runs it on a blocking thread.
pub fn scan_harness_targets_blocking(hint_path: Option<String>, deep_scan: bool) -> HarnessTargetScan {
    let shortcuts = scan_shell_shortcuts();
    let checkouts = crate::scan_dsh_paths_blocking(hint_path, deep_scan);
    // npm's global prefix on Windows is `%APPDATA%\npm`, and it also puts that
    // directory on `PATH`; both are given to the pure matcher, so this is the only
    // place the environment is read.
    let installed_clis = installed_cli_launchers_from(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("PATH").ok().as_deref(),
    );
    let scan = build_scan(&shortcuts, &checkouts, &installed_clis);
    log::info!(
        "harness target scan: {} 个快捷方式，{} 个可选项（{} 个壳，{} 份源码检出，{} 个已安装 CLI）",
        shortcuts.len(),
        scan.targets.len(),
        scan.targets
            .iter()
            .filter(|target| target.kind == HarnessTargetKind::EmbeddedShell)
            .count(),
        scan.targets
            .iter()
            .filter(|target| target.kind == HarnessTargetKind::Checkout)
            .count(),
        scan.targets
            .iter()
            .filter(|target| target.kind == HarnessTargetKind::InstalledCli)
            .count()
    );
    scan
}

/// The launchers of a globally installed DSH CLI, as `npm` leaves them.
///
/// `npm i -g @deepseek-ai/dsh` writes `dsh`/`dsh.cmd`/`dsh.ps1` into the global
/// prefix's bin directory (on Windows that is `%APPDATA%\npm`) and puts that
/// directory on `PATH`. Both halves are checked, prefix first, and the `PATH` walk
/// looks only for the Windows launchers `.cmd`/`.exe`, because that is what a spawn
/// from Rust can execute directly.
///
/// Pure on purpose: the environment values are parameters, so the layout can be
/// tested without reading this machine.
fn installed_cli_launchers_from(npm_prefix: Option<&str>, path: Option<&str>) -> Vec<PathBuf> {
    npm_launcher_paths(npm_prefix, path, &["dsh"])
}

/// The same search for any npm-installed launcher, by base name.
///
/// Kept general because the TUI is a second such command (`dst`, installed globally
/// from `@deepseek-harness-tui/dsh-tui`) and it has to be found the same way, in the
/// same places, with the same rules — a second implementation would drift from this
/// one about which directory counts.
fn npm_launcher_paths(
    npm_prefix: Option<&str>,
    path: Option<&str>,
    names: &[&str],
) -> Vec<PathBuf> {
    let mut launchers = Vec::new();
    let mut push = |candidate: PathBuf| {
        if candidate.is_file() && !launchers.iter().any(|existing| existing == &candidate) {
            launchers.push(candidate);
        }
    };
    if let Some(prefix) = npm_prefix {
        for name in names {
            for extension in ["cmd", "exe"] {
                push(PathBuf::from(prefix).join(format!("{name}.{extension}")));
            }
        }
    }
    if let Some(path) = path {
        for directory in std::env::split_paths(path) {
            for name in names {
                for extension in ["cmd", "exe"] {
                    push(directory.join(format!("{name}.{extension}")));
                }
            }
        }
    }
    launchers
}

/// The directories a shell registration can live in.
///
/// Start Menu entries are what `Get-StartApps` reports; the desktop directory is
/// included because an installer may create only that shortcut.
fn shortcut_directories_from(
    appdata: Option<&str>,
    programdata: Option<&str>,
    userprofile: Option<&str>,
) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut push = |path: PathBuf| {
        if path.is_dir() && !directories.iter().any(|existing| existing == &path) {
            directories.push(path);
        }
    };
    if let Some(appdata) = appdata {
        push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(programdata) = programdata {
        push(PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(userprofile) = userprofile {
        push(PathBuf::from(userprofile).join("Desktop"));
    }
    directories
}

#[cfg(windows)]
fn shortcut_directories() -> Vec<PathBuf> {
    shortcut_directories_from(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("ProgramData").ok().as_deref(),
        std::env::var("USERPROFILE").ok().as_deref(),
    )
}

/// Read the `System.AppUserModel.ID` the shell registered for one shortcut.
///
/// This is why a shell needs no path: the same string that proves the
/// application is installed is the string `shell:AppsFolder\<AUMID>` resolves at
/// launch time. A shortcut without the property (most of them) reports no error
/// worth surfacing — it simply is not a candidate.
#[cfg(windows)]
fn shortcut_aumid(path: &Path) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};

    let item: IShellItem2 = unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()? };
    let raw = unsafe { item.GetString(&PKEY_AppUserModel_ID).ok()? };
    if raw.is_null() {
        return None;
    }
    let value = unsafe { raw.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void)) };
    let value = value?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Bound the walk: a Start Menu is user data, and a pathological one must not
/// turn one settings scan into thousands of COM property reads.
#[cfg(windows)]
const SHORTCUT_SCAN_MAX_FILES: usize = 512;
#[cfg(windows)]
const SHORTCUT_SCAN_MAX_DEPTH: u8 = 5;

#[cfg(windows)]
fn collect_shortcut_aumids(
    directory: &Path,
    depth: u8,
    budget: &mut usize,
    found: &mut Vec<ScannedShortcut>,
) {
    if *budget == 0 || depth > SHORTCUT_SCAN_MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    // Sorted so a duplicated registration resolves to the same one on every
    // scan, instead of depending on directory order.
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if *budget == 0 {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_shortcut_aumids(&path, depth.saturating_add(1), budget, found);
            continue;
        }
        if path
            .extension()
            .map(|extension| !extension.eq_ignore_ascii_case("lnk"))
            .unwrap_or(true)
        {
            continue;
        }
        *budget -= 1;
        if let Some(aumid) = shortcut_aumid(&path) {
            found.push(ScannedShortcut {
                aumid,
                directory: directory.to_string_lossy().into_owned(),
            });
        }
    }
}

/// Every shell registration this machine exposes to the settings list.
#[cfg(windows)]
fn scan_shell_shortcuts() -> Vec<ScannedShortcut> {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    // Property reads on a pooled blocking thread: mirror the app's own apartment
    // (MTA). A thread that already has an apartment keeps it — the call is then a
    // no-op — and for the same reason this deliberately does not uninitialize.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut budget = SHORTCUT_SCAN_MAX_FILES;
    let mut found = Vec::new();
    for directory in shortcut_directories() {
        collect_shortcut_aumids(&directory, 0, &mut budget, &mut found);
    }
    found
}

#[cfg(not(windows))]
fn scan_shell_shortcuts() -> Vec<ScannedShortcut> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试里的扫描大多不关心"已安装的 CLI"这一类，所以在这里补一个两参数包装：调用点读起来
    /// 仍然是"快捷方式 + 检出"两件事，而需要覆盖第三类的测试直接调 `super::build_scan`。
    fn build_scan(
        shortcuts: &[ScannedShortcut],
        checkouts: &[crate::DshPathCandidate],
    ) -> HarnessTargetScan {
        super::build_scan(shortcuts, checkouts, &[])
    }

    #[test]
    fn the_same_search_finds_the_tui_launcher() {
        // ③ 要用它找 `dst`（全局安装的 TUI）：同一个搜索、同一批位置、同一套规则，
        // 只是换一个基名。这里用真实存在的文件验证"按名字找"确实生效。
        let dir = std::env::temp_dir().join("dsh-wallpaper-launcher-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let dst = dir.join("dst.cmd");
        std::fs::write(&dst, b"@echo off\r\n").expect("write");
        let found = npm_launcher_paths(None, Some(dir.to_string_lossy().as_ref()), &["dst"]);
        assert_eq!(found, vec![dst.clone()]);
        // 同一个目录里找 `dsh` 不会把 `dst` 也算进去 —— 名字是精确的。
        assert!(npm_launcher_paths(None, Some(dir.to_string_lossy().as_ref()), &["dsh"]).is_empty());
        // 前缀优先，且同一个启动器不会被列两次。
        let both = npm_launcher_paths(
            Some(dir.to_string_lossy().as_ref()),
            Some(dir.to_string_lossy().as_ref()),
            &["dst"],
        );
        assert_eq!(both, vec![dst]);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn shortcut(aumid: &str, directory: &str) -> ScannedShortcut {
        ScannedShortcut {
            aumid: aumid.into(),
            directory: directory.into(),
        }
    }

    fn checkout(root: &str, source: &str) -> crate::DshPathCandidate {
        crate::DshPathCandidate {
            root_path: root.into(),
            source: source.into(),
        }
    }

    const OFFICIAL: &str = "com.deepseek.dsh";
    /// 一个**本 build 已不再支持**的 AUMID（第三方桌面客户端，2026-09-27 移除）：
    /// 用它验证"陌生/不受支持的 AUMID 不会被当成主体"。
    const UNSUPPORTED_AUMID: &str = "ai.deepseek.dsh.desktop";

    #[test]
    fn a_shell_is_matched_by_aumid_and_never_by_path() {
        let scan = build_scan(&[shortcut(OFFICIAL, r"C:\Start Menu")], &[]);
        assert_eq!(scan.targets.len(), 1);
        let target = &scan.targets[0];
        assert_eq!(target.kind, HarnessTargetKind::EmbeddedShell);
        assert_eq!(target.client, HarnessClientKind::OfficialDesktop);
        assert_eq!(target.label, "官方桌面客户端");
        assert_eq!(target.id, "shell:com.deepseek.dsh");
        assert_eq!(target.identity.aumid.as_deref(), Some(OFFICIAL));
        assert_eq!(target.identity.root_path, None);
        assert_eq!(target.identity.default_ports, vec![19387]);
        assert_eq!(target.launch.kind, LaunchRecipeKind::AppsFolder);
        assert_eq!(
            target.launch.alias.as_deref(),
            Some(r"shell:AppsFolder\com.deepseek.dsh")
        );
    }

    #[test]
    fn the_shell_registration_is_matched_regardless_of_letter_case() {
        let scan = build_scan(&[shortcut("COM.DeepSeek.DSH", r"C:\Start Menu")], &[]);
        let target = &scan.targets[0];
        // The canonical spelling is this build's, not whatever the registration
        // happened to record.
        assert_eq!(target.identity.aumid.as_deref(), Some(OFFICIAL));
        assert_eq!(
            target.launch.alias.as_deref(),
            Some(r"shell:AppsFolder\com.deepseek.dsh")
        );
    }

    #[test]
    fn unrelated_shortcut_aumids_are_not_offered() {
        let scan = build_scan(
            &[
                shortcut("Microsoft.Windows.Explorer", r"C:\Start Menu"),
                shortcut("Notepad", r"C:\Start Menu"),
            ],
            &[],
        );
        assert!(scan.targets.is_empty());
    }

    #[test]
    fn a_shell_seen_in_two_places_is_still_one_subject() {
        let scan = build_scan(
            &[
                shortcut(OFFICIAL, r"C:\Start Menu"),
                shortcut(OFFICIAL, r"C:\Desktop"),
            ],
            &[],
        );
        assert_eq!(scan.targets.len(), 1);
        assert_eq!(scan.targets[0].source, r"C:\Start Menu");
    }

    #[test]
    fn an_installed_cli_is_named_by_its_launcher() {
        let launcher = r"C:\Users\someone\AppData\Roaming\npm\dsh.cmd";
        let target = installed_cli_target(Path::new(launcher));
        assert_eq!(target.kind, HarnessTargetKind::InstalledCli);
        assert_eq!(target.client, HarnessClientKind::OfficialWeb);
        // 它的身份就是那个启动器，id 把它带进存储；两者必须是同一份字符串。
        assert_eq!(target.id, format!("{CLI_ID_PREFIX}{launcher}"));
        assert_eq!(target.source, launcher);
        // 没有 AUMID、没有源码树 —— 这正是它必须单列一类的原因。
        assert!(target.identity.aumid.is_none());
        assert!(target.identity.root_path.is_none());
        assert_eq!(
            target.identity.default_ports,
            vec![crate::HARNESS_DEFAULT_PORT]
        );
        // 它会像检出那样 boot 一个 profile。
        assert!(target.capabilities.needs_profile);
        assert!(!target.capabilities.owns_window);
    }

    #[test]
    fn a_shell_this_build_no_longer_supports_is_not_a_target() {
        // 第三方桌面客户端 2026-09-27 移除：它的快捷方式即使就摆在那儿，也必须**扫不出任何主体** ——
        // 否则设置里又会多出一个永远点不亮的选项（实测它对自己的本地接口一律 403）。
        let scan = build_scan(&[shortcut(UNSUPPORTED_AUMID, r"C:\Desktop")], &[]);
        assert!(scan.targets.is_empty(), "unsupported AUMID must not become a subject");

        let official = build_scan(&[shortcut(OFFICIAL, r"C:\Start Menu")], &[]);
        let clients: Vec<_> = official.targets.iter().map(|target| target.client).collect();
        assert_eq!(clients, vec![HarnessClientKind::OfficialDesktop]);
        assert!(official.targets[0].capabilities.single_instance);
        assert!(official.targets[0].capabilities.can_start_hidden);
        assert!(official.targets[0].capabilities.owns_window);
    }

    #[test]
    fn a_checkout_is_identified_by_its_root_and_needs_a_profile() {
        let scan = build_scan(&[], &[checkout(r"D:\Family\DeepSeekHarness\deepseek-harness", "常见项目目录")]);
        let target = &scan.targets[0];
        assert_eq!(target.kind, HarnessTargetKind::Checkout);
        assert_eq!(target.client, HarnessClientKind::OfficialWeb);
        assert_eq!(target.label, "deepseek-harness");
        assert_eq!(target.source, "常见项目目录");
        assert_eq!(target.id, r"D:\Family\DeepSeekHarness\deepseek-harness");
        assert_eq!(
            target.identity.root_path.as_deref(),
            Some(r"D:\Family\DeepSeekHarness\deepseek-harness")
        );
        assert_eq!(target.identity.aumid, None);
        assert_eq!(target.identity.default_ports, vec![3080]);
        assert_eq!(target.launch.kind, LaunchRecipeKind::ManagedCommand);
        assert_eq!(target.launch.alias, None);
        // A checkout has no window of its own and no single instance: what
        // answers on the port decides, and the managed chain leaves another
        // instance alone.
        assert!(!target.capabilities.owns_window);
        assert!(!target.capabilities.single_instance);
        assert!(target.capabilities.needs_profile);
    }

    #[test]
    fn a_duplicated_checkout_is_reported_once() {
        let scan = build_scan(
            &[],
            &[
                checkout(r"D:\Family\Harness", "当前设置路径"),
                checkout(r"d:\family\harness", "常见项目目录"),
            ],
        );
        assert_eq!(scan.targets.len(), 1);
        assert_eq!(scan.targets[0].id, r"D:\Family\Harness");
    }

    #[test]
    fn shells_lead_the_list_and_checkouts_keep_the_scan_order() {
        let scan = build_scan(
            &[shortcut(OFFICIAL, r"C:\Start Menu")],
            &[
                checkout(r"D:\Family\DeepSeekHarness\deepseek-harness", "当前设置路径"),
                checkout(r"D:\Family\DeepSeekHarness\plugins\dsh-wallpaper", "常见项目目录"),
            ],
        );
        let ids: Vec<_> = scan.targets.iter().map(|target| target.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "shell:com.deepseek.dsh",
                r"D:\Family\DeepSeekHarness\deepseek-harness",
                r"D:\Family\DeepSeekHarness\plugins\dsh-wallpaper",
            ]
        );
    }

    #[test]
    fn several_source_trees_ask_the_user_to_choose_a_subject() {
        let one = build_scan(&[], &[checkout(r"D:\Family\Harness", "当前目录")]);
        assert!(!one.requires_subject_choice);
        // A shell plus one tree is unambiguous: the shell is not a "subject
        // choice" the user has to resolve.
        let with_shell = build_scan(
            &[shortcut(OFFICIAL, r"C:\Start Menu")],
            &[checkout(r"D:\Family\Harness", "当前目录")],
        );
        assert!(!with_shell.requires_subject_choice);
        let two = build_scan(
            &[],
            &[
                checkout(r"D:\Family\Harness", "当前目录"),
                checkout(r"D:\Family\Harness-rebuild", "常见项目目录"),
            ],
        );
        assert!(two.requires_subject_choice);
    }

    #[test]
    fn shortcut_directories_skip_roots_that_do_not_exist() {
        let root = tempfile::tempdir().expect("temp dir");
        let user_start_menu = root.path().join(r"appdata\Microsoft\Windows\Start Menu\Programs");
        let machine_start_menu = root
            .path()
            .join(r"programdata\Microsoft\Windows\Start Menu\Programs");
        std::fs::create_dir_all(&user_start_menu).expect("user start menu");
        std::fs::create_dir_all(&machine_start_menu).expect("machine start menu");

        let directories = shortcut_directories_from(
            Some(&root.path().join("appdata").to_string_lossy()),
            Some(&root.path().join("programdata").to_string_lossy()),
            // A user profile without a Desktop contributes nothing, rather than
            // a path that cannot exist on this machine.
            Some(&root.path().join("home").to_string_lossy()),
        );

        assert_eq!(directories, vec![user_start_menu, machine_start_menu]);
    }

    #[test]
    fn the_shell_allowlist_answers_with_this_build_s_canonical_alias() {
        let shell = known_shell("COM.DeepSeek.DSH").expect("known shell");
        assert_eq!(shell.aumid, OFFICIAL);
        assert_eq!(shell.alias, r"shell:AppsFolder\com.deepseek.dsh");
        assert_eq!(shell.default_port, Some(19387));
        assert!(shell.can_start_hidden);
        // Anything this build does not know how to start is refused here rather
        // than handed to the Windows shell as a launch request.
        assert!(known_shell("Notepad").is_none());
        assert!(known_shell("Microsoft.Windows.Explorer").is_none());
        assert!(known_shell("   ").is_none());
    }

    #[test]
    fn a_shell_target_id_resolves_back_to_the_same_shell() {
        let scan = build_scan(&[shortcut(OFFICIAL, r"C:\Start Menu")], &[]);
        let target = &scan.targets[0];
        let aumid = target
            .id
            .strip_prefix(SHELL_ID_PREFIX)
            .expect("shell id namespace");
        let shell = known_shell(aumid).expect("resolvable");
        // What the renderer stores, and what the launcher will hand to Windows,
        // are the same alias: no path and no version exists in between.
        assert_eq!(Some(shell.alias), target.launch.alias);
    }

    /// The native half of the frozen "no substitution" rule.
    ///
    /// Its renderer mirror is `subjectEndpointPorts` in
    /// `wallpaper/src/connect/endpoints.ts`; the two must agree, because one side
    /// decides what is scanned and the other what is shown.
    #[test]
    fn a_subject_decides_which_ports_may_be_probed() {
        // A shell owns the port compiled into it, and nothing else: 3080 is
        // another client's.
        assert_eq!(
            subject_endpoint_ports(&format!("{SHELL_ID_PREFIX}{OFFICIAL}"), &[]),
            vec![19387]
        );
        // 第三方客户端已移除 ⇒ 它的 AUMID 现在与陌生 AUMID 一样：**一个端口都不给**（而不是
        // "给 43120 然后探测失败"）。这正是与 `endpoints.ts` 必须一致的那条规则。
        assert_eq!(
            subject_endpoint_ports(&format!("{SHELL_ID_PREFIX}{UNSUPPORTED_AUMID}"), &[]),
            Vec::<u16>::new()
        );
        assert_eq!(
            subject_endpoint_ports(&format!("{SHELL_ID_PREFIX}com.unknown.client"), &[]),
            Vec::<u16>::new()
        );
        // The id is matched the same way the launcher matches it.
        assert_eq!(
            subject_endpoint_ports("shell:COM.DeepSeek.DSH", &[]),
            vec![19387]
        );
        // A source tree owns DSH's own default plus the ports the user added for it.
        assert_eq!(subject_endpoint_ports(r"D:\tree", &[]), vec![3080]);
        assert_eq!(subject_endpoint_ports(r"D:\tree", &[3081, 3081, 0]), vec![3080, 3081]);
    }

    /// The two empty answers mean opposite things, and conflating them is how the
    /// wallpaper would end up on the shipped priority order with a subject chosen.
    #[test]
    fn an_empty_subject_and_an_unplaceable_one_are_different() {
        assert!(subject_endpoint_ports("", &[3081]).is_empty());
        assert!(subject_endpoint_ports("   ", &[3081]).is_empty());
        // Configured, but this build cannot say where it answers: no port, and the
        // caller must say so rather than scanning elsewhere.
        assert!(subject_endpoint_ports("shell:com.unknown.client", &[]).is_empty());
        // A checkout with added ports is never empty, even before any scan.
        assert!(!subject_endpoint_ports(r"D:\tree", &[]).is_empty());
    }

    /// Ground truth for this machine's shell registrations.
    ///
    /// Ignored by default: it reads the real Start Menu and a real filesystem,
    /// so it belongs in a manual run (`cargo test -- --ignored --nocapture`).
    /// It asserts only structural invariants — never that a particular client is
    /// installed, which is the user's machine's business.
    #[test]
    #[ignore = "reads this machine's shell registrations and filesystem"]
    fn this_machine_reports_its_installed_subjects() {
        let scan = scan_harness_targets_blocking(None, false);
        println!("{}", serde_json::to_string_pretty(&scan).expect("scan json"));
        let mut ids = HashSet::new();
        for target in &scan.targets {
            assert!(ids.insert(target.id.clone()), "duplicate id {}", target.id);
            match target.kind {
                HarnessTargetKind::EmbeddedShell => {
                    assert!(target.identity.aumid.is_some());
                    assert!(target.identity.root_path.is_none());
                }
                HarnessTargetKind::Checkout => {
                    assert!(target.identity.root_path.is_some());
                    assert!(target.identity.aumid.is_none());
                }
                HarnessTargetKind::InstalledCli => {
                    // 已安装的 CLI 既不是壳也不是源码树：它没有 AUMID，也没有根目录。
                    assert!(target.identity.aumid.is_none());
                    assert!(target.identity.root_path.is_none());
                    assert_eq!(target.client, HarnessClientKind::OfficialWeb);
                }
            }
        }
    }
}
