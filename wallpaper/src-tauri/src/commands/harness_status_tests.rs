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
