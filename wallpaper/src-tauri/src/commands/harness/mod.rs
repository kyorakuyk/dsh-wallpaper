#[cfg(test)]
#[cfg(not(feature = "lite"))]
mod dsh_autostart_tests {
    use super::discovery::resolve_dsh_launcher;
    use super::managed::{
        classify_dsh_launch_failure, merge_managed_instances, stop_targets, ManagedDshInstance,
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
pub(crate) mod managed;
pub(crate) mod session;
pub(crate) mod ui;

// D 组（discovery）：外部仍按 commands::harness::<名字> 引用扫描与候选类型。
#[allow(unused_imports)]
pub(crate) use discovery::{harness_target_catalog, scan_dsh_paths, scan_harness_targets};
pub(crate) use discovery::{scan_dsh_paths_blocking, DshPathCandidate};

// M 组（managed）：类型与纯函数被 harness_launch / harness_targets / lib.rs 直接调用。
pub(crate) use managed::{
    classify_dsh_launch_failure, shutdown_managed_dsh, spawn_managed_dsh, ManagedDshAutostartState,
    ManagedDshState,
};
#[allow(unused_imports)]
pub(crate) use managed::{
    autostart_managed_dsh, launch_dsh, managed_dsh_autostart_status, managed_dsh_status,
    stop_managed_dsh, ManagedDshStatus,
};

// U 组（ui）：注册走子模块路径；重导出只为保持 commands::harness::<命令名> 这个名字。
#[allow(unused_imports)]
pub(crate) use ui::{
    autostart_harness_target, ensure_harness_ui, ensure_profile_bridge, launch_harness_target,
};

// 重导出保持外部看到的是 commands::harness::<命令名>（决定 4）。
// handler 无法穿过重导出找到 #[tauri::command] 生成的宏，故注册改用子模块路径（决定 4 允许的例外）。
#[allow(unused_imports)]
pub(crate) use session::{
    connect_harness, harness_controls, harness_history, harness_models, harness_presets,
    harness_set_model, harness_set_permission, harness_set_preset,
};
