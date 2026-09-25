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

use crate::harness_targets::{known_shell, HarnessTargetKind, SHELL_ID_PREFIX};

/// How long a shell is given to answer before its start is reported as
/// unconfirmed. Generous on purpose: an Electron client's first start after a
/// login is slow, and a false "failed" would be worse than a slow "started".
const SHELL_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const SHELL_START_POLL: std::time::Duration = std::time::Duration::from_millis(250);

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
/// class a stored id resolves to, whether a profile is even meaningful, and
/// whether an unattended start is allowed to run the configured launcher.
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
        command: Option<String>,
    },
}

/// Who is asking for the start, which is what decides the two rules that differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchTrigger {
    /// A control the user just pressed. The configured launcher is used as given,
    /// and a window is shown — they asked for the thing on screen.
    Manual,
    /// The wallpaper's own unattended start. Only an allowlisted launcher runs
    /// without explicit consent, and a shell that supports it keeps its window out
    /// of sight (§5.1).
    Automatic { trusted_command: bool },
}

impl LaunchTrigger {
    fn is_automatic(self) -> bool {
        matches!(self, Self::Automatic { .. })
    }

    fn trusted_command(self) -> bool {
        matches!(self, Self::Automatic { trusted_command: true })
    }
}

/// Decide how to start `id`, or refuse with a closed code.
///
/// The trigger is the one input that changes behaviour, and it changes it in two
/// places on purpose: only an unattended start hides a window (§5.1 defines that
/// as the startup behaviour), and only an unattended start needs consent for a
/// custom launcher, because running an arbitrary configured program at every login
/// is a different trust decision from a button press.
pub(crate) fn plan_launch(
    id: &str,
    profile: &str,
    command: Option<&str>,
    trigger: LaunchTrigger,
) -> Result<LaunchPlan, &'static str> {
    let command = command.map(str::trim).filter(|value| !value.is_empty());
    if let Some(aumid) = id.trim().strip_prefix(SHELL_ID_PREFIX) {
        let Some(shell) = known_shell(aumid) else {
            return Err("unknown-target");
        };
        return Ok(LaunchPlan::Shell {
            aumid: shell.aumid.to_string(),
            alias: shell.alias,
            port: shell.default_port,
            hide_window: trigger.is_automatic() && shell.can_start_hidden,
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
    if trigger.is_automatic()
        && !crate::is_allowlisted_auto_start_launcher(command)
        && !trigger.trusted_command()
    {
        return Err("command-not-confirmed");
    }
    Ok(LaunchPlan::Checkout {
        root_path: root_path.to_string(),
        profile: profile.trim().to_string(),
        command: command.map(str::to_string),
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
        LaunchPlan::Checkout {
            root_path,
            profile,
            command,
        } => match crate::spawn_managed_dsh(managed, root_path, profile, command.as_deref()) {
            // `spawn_managed_dsh` returns the existing pid when this process
            // already owns a running child, so "started" also covers "already
            // managed"; ownership is the same either way.
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

    let mut launch = std::process::Command::new("explorer.exe");
    launch.arg(alias);
    // `explorer.exe` is a GUI process: it has no use for our standard streams, and
    // inheriting the wallpaper's would keep a handle family alive past this call.
    launch
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    log::info!("harness shell launch: program=explorer.exe arg={alias} aumid={aumid}");
    if let Err(error) = launch.spawn() {
        log::warn!("harness shell launch failed: {error}");
        return HarnessLaunchOutcome::new("spawn-failed", HarnessTargetKind::EmbeddedShell);
    }

    let (confirmed, hidden) = match port {
        Some(port) => wait_for_shell(port, hide_window),
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
fn wait_for_shell(port: u16, hide_window: bool) -> (bool, bool) {
    let deadline = std::time::Instant::now() + SHELL_START_TIMEOUT;
    let mut confirmed = false;
    let mut hidden = false;
    while std::time::Instant::now() < deadline {
        if !confirmed {
            confirmed = crate::client_window::endpoint_is_listening(port);
        }
        if hide_window && !hidden {
            hidden = crate::client_window::hide_client_window(port).hidden;
        }
        if confirmed && (!hide_window || hidden) {
            break;
        }
        std::thread::sleep(SHELL_START_POLL);
    }
    if hide_window && !hidden {
        log::warn!("harness shell window was never hideable: port={port} confirmed={confirmed}");
    }
    (confirmed, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFICIAL_ID: &str = "shell:com.deepseek.dsh";
    const DESKTOP_ID: &str = "shell:ai.deepseek.dsh.desktop";
    const CHECKOUT: &str = r"D:\Family\DeepSeekHarness\deepseek-harness";

    const MANUAL: LaunchTrigger = LaunchTrigger::Manual;
    const AUTO: LaunchTrigger = LaunchTrigger::Automatic { trusted_command: false };
    const AUTO_TRUSTED: LaunchTrigger = LaunchTrigger::Automatic { trusted_command: true };

    #[test]
    fn a_shell_plan_carries_the_alias_this_build_knows() {
        let plan = plan_launch(OFFICIAL_ID, "desktop", None, MANUAL).expect("shell plan");
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
    fn only_an_unattended_start_keeps_the_window_out_of_sight() {
        // §5.1: the startup path starts the official shell without showing it...
        let automatic = plan_launch(OFFICIAL_ID, "desktop", None, AUTO).expect("shell plan");
        assert!(matches!(automatic, LaunchPlan::Shell { hide_window: true, .. }));
        // ...while a button the user just pressed shows them what they asked for.
        let manual = plan_launch(OFFICIAL_ID, "desktop", None, MANUAL).expect("shell plan");
        assert!(matches!(manual, LaunchPlan::Shell { hide_window: false, .. }));
        // §4.10: the third-party desktop client is never hidden, either way.
        let third_party = plan_launch(DESKTOP_ID, "desktop", None, AUTO).expect("plan");
        assert!(matches!(third_party, LaunchPlan::Shell { hide_window: false, .. }));
    }

    #[test]
    fn an_unknown_shell_id_is_refused_instead_of_launched() {
        // The alias string is a shell launch request, so anything outside this
        // build's table must not reach the shell.
        assert_eq!(plan_launch("shell:Notepad", "desktop", None, MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("shell:", "desktop", None, MANUAL), Err("unknown-target"));
        assert_eq!(
            plan_launch("shell:Microsoft.Windows.Explorer", "desktop", None, AUTO),
            Err("unknown-target")
        );
    }

    #[test]
    fn an_empty_subject_is_refused() {
        assert_eq!(plan_launch("   ", "desktop", None, MANUAL), Err("unknown-target"));
        assert_eq!(plan_launch("", "desktop", None, AUTO), Err("unknown-target"));
    }

    #[test]
    fn a_shell_ignores_the_profile_setting_which_belongs_to_a_checkout_only() {
        // §4.7: a shell uses its own data, so a leftover profile value must not
        // change — or block — its launch.
        let plan = plan_launch(OFFICIAL_ID, "not a profile at all", None, MANUAL)
            .expect("shell plan");
        assert!(matches!(plan, LaunchPlan::Shell { .. }));
    }

    #[test]
    fn a_checkout_plan_is_its_root_path_with_its_profile() {
        let plan =
            plan_launch(CHECKOUT, " desktop ", Some("node.exe"), MANUAL).expect("checkout plan");
        assert_eq!(
            plan,
            LaunchPlan::Checkout {
                root_path: CHECKOUT.into(),
                profile: "desktop".into(),
                command: Some("node.exe".into()),
            }
        );
    }

    #[test]
    fn an_untrusted_launcher_needs_consent_only_on_the_automatic_path() {
        // Manual: the user just asked for it, so the configured launcher is used.
        assert!(plan_launch(CHECKOUT, "desktop", Some(r"D:\tools\mine.exe"), MANUAL).is_ok());
        // Automatic: refused until the user explicitly agreed.
        assert_eq!(
            plan_launch(CHECKOUT, "desktop", Some(r"D:\tools\mine.exe"), AUTO),
            Err("command-not-confirmed")
        );
        // Agreed → allowed, still as one executable path.
        assert!(plan_launch(CHECKOUT, "desktop", Some(r"D:\tools\mine.exe"), AUTO_TRUSTED).is_ok());
        // The launchers the managed chain picks itself never need consent.
        assert!(plan_launch(CHECKOUT, "desktop", Some("node.exe"), AUTO).is_ok());
        assert!(plan_launch(CHECKOUT, "desktop", None, AUTO).is_ok());
    }

    #[test]
    fn a_shell_is_never_gated_by_the_launcher_consent_rule() {
        // A shell has no configured launcher at all, so the consent question does
        // not apply to it; only checkouts can carry a `command`.
        assert!(plan_launch(DESKTOP_ID, "desktop", None, AUTO).is_ok());
    }

    /// Ground truth for the one class this machine can be asked about.
    ///
    /// Ignored by default: it starts the third-party desktop client and stops it
    /// again. The official shell is never touched here — it hosts the session that
    /// would be running this test — so the official shell's launch stays a
    /// user-verified step, which the design records as such (§9.2).
    ///
    /// What this proves that a unit test cannot: the alias the wallpaper stores is
    /// one the Windows shell actually resolves, the start is confirmed by the
    /// port, and the process that answered can be identified and stopped again
    /// without touching an instance the user already had running.
    #[test]
    #[ignore = "starts and stops this machine's third-party desktop client"]
    fn this_machine_starts_the_third_party_desktop_from_its_alias() {
        let port = 43120;
        if crate::client_window::endpoint_is_listening(port) {
            // The user already has it running: leave it exactly as found.
            println!("desktop client already listening on {port}; nothing started");
            return;
        }

        let plan = plan_launch(DESKTOP_ID, "desktop", None, AUTO).expect("shell plan");
        // §4.10: the third-party client is started with its window, never hidden.
        assert!(matches!(plan, LaunchPlan::Shell { hide_window: false, .. }));

        let state = crate::ManagedDshState::default();
        let outcome = run_launch(&plan, &state);
        println!("launch outcome: {outcome:?}");
        assert_eq!(outcome.kind, HarnessTargetKind::EmbeddedShell);
        // The alias was accepted and the client answered within the timeout.
        assert_eq!(outcome.outcome, "started");
        assert!(crate::client_window::endpoint_is_listening(port));

        // Starting again must not open a second client: it reports the running one
        // and leaves it alone.
        let again = run_launch(&plan, &state);
        assert_eq!(again.outcome, "already-running");

        let pid = crate::client_window::endpoint_process_id(port)
            .expect("the answering process is identifiable");
        println!("stopping the client this test started: pid={pid}");
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while crate::client_window::endpoint_is_listening(port)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(SHELL_START_POLL);
        }
        assert!(
            !crate::client_window::endpoint_is_listening(port),
            "the client this test started is still listening on {port}"
        );
    }
}
