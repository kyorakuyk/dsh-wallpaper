use crate::*;
use super::{require_background, require_wallpaper_surface};
use std::sync::{Mutex, RwLock};

#[cfg(not(feature = "lite"))]
static HARNESS_STATUS_CACHE: std::sync::OnceLock<RwLock<serde_json::Value>> = OnceLock::new();
#[cfg(not(feature = "lite"))]
static HARNESS_PROBE_CLIENT: std::sync::OnceLock<Option<reqwest::Client>> = OnceLock::new();

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
pub(crate) const HARNESS_DEFAULT_PORT: u16 = 3080;

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
static HARNESS_ENDPOINT_STATE: std::sync::OnceLock<HarnessEndpointState> = OnceLock::new();

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
pub(crate) fn harness_endpoint_configured() -> bool {
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
pub(crate) fn harness_endpoint_port() -> u16 {
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
pub(crate) fn set_harness_endpoint(
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
#[path = "harness_status_tests.rs"]
mod harness_status_tests;

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn probe_harness(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
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
pub(crate) async fn scan_harness_endpoints_command(
    caller: tauri::WebviewWindow,
    extra_ports: Option<Vec<u16>>,
) -> Result<serde_json::Value, String> {
    require_wallpaper_surface(&caller)?;
    Ok(scan_harness_endpoints(extra_ports.unwrap_or_default()).await)
}

/// Report whether anything is listening on an endpoint, without raising it.
///
/// Lets the UI tell "client not running" from "client running but has no window",
/// which need different wording and different recovery.
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn harness_endpoint_listening(caller: tauri::WebviewWindow, port: u16) -> Result<bool, String> {
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
pub(crate) fn harness_endpoint_window(caller: tauri::WebviewWindow, port: u16) -> Result<bool, String> {
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
pub(crate) fn start_harness_monitor(app: tauri::AppHandle) {
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
                        crate::commands::desktop::emit_app_snapshot(&app, &snapshot);
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
                    crate::commands::desktop::emit_app_snapshot(&app, &snapshot);
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
