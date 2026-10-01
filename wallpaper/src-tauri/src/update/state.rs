//! 更新状态文件与自动检查的节流（计划书 §五、§八 8、§十）。
//!
//! 状态放在原生侧而不是 WebView 的 `localStorage`：它要在壁纸重启后仍然有效，而 `localStorage`
//! 属于那个 WebView 实例（§五）。位置是 `%LOCALAPPDATA%\com.dsh.wallpaper\updates\state.json`。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::version::Version;
use super::{CheckOutcome, FailureReport, SkipReason, APP_IDENTIFIER};

/// 自动检查的最小间隔：6 小时（§五 与 §十）。
pub(crate) const CHECK_INTERVAL_MS: u64 = 6 * 60 * 60 * 1000;

const UPDATES_DIR_NAME: &str = "updates";
pub(crate) const STATE_FILE_NAME: &str = "state.json";

/// 状态文件所在目录：`<本地数据>\<标识符>\updates`。
///
/// 参数是"本地数据根"（打包态由 Tauri 给的是被重定向过的那个目录），拼出来的形状与 §五 写的
/// `%LOCALAPPDATA%\com.dsh.wallpaper\updates` 一致。
pub(crate) fn updates_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join(APP_IDENTIFIER).join(UPDATES_DIR_NAME)
}

/// 现在：Unix 纪元的毫秒。节流的唯一时间来源（注入给纯函数的也是它）。
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// 状态文件的内容（§五 要求的字段，加上"上次结果"）。
///
/// 字段名就是文件里的键（camelCase）。读的时候多出来的键忽略、缺的键取默认值，所以以后加字段
/// 不需要迁移，旧版本读到新文件也不会崩。`lastOutcome`/`lastFailure` 是 §五 那句话里的"至少"：
/// 设置中心要显示"上次结果（已是最新 / 有新版本 x / 检查失败的原因）"（§四），而失败原因无法从
/// 别的字段推出来。
///
/// `lastSkipReason` 是被真机坑出来的一个字段：跳过分支原先只留一行日志，于是"检查从来没被触发过"
/// 与"跑了、命中了跳过分支"留下的**唯一**区别就是那条日志，而日志会被截断 ⇒ 事后无法定性。
/// 现在两条跳过分支（`commands::run_check`）也落盘，理由是"先让失败/状态可见"。
/// 注意它**不代表**一次检查：`checkedAtMs` 的语义（真正查过的时刻）不受影响。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct UpdateState {
    /// 上一次**解析成功**的 release 版本（`v` 前缀已去掉，形状按标签原样）。
    pub latest_version: Option<String>,
    /// 上一次真正检查的时间，毫秒。失败也算一次检查（理由见 `commands::run_check`）。
    pub checked_at_ms: Option<u64>,
    /// 用户按过「忽略」的那**一个**版本（§四：忽略记录的是具体版本，不是"忽略全部"）。
    pub dismissed_version: Option<String>,
    /// 已下载资产的落盘位置（`update_download` 成功时写；§六 的 `<版本>\<资产名>`）。
    pub downloaded_path: Option<String>,
    /// 已下载资产算出来的 sha256（`sha256:<hex>`）。
    ///
    /// API 给了 `digest` 时它就是**核对过**的那个值；没给时仍然写下来（文件在盘上的实际摘要），
    /// 只是那一次没有可核对的对象（§六 只要求"有 digest 时"核对）。
    pub downloaded_sha256: Option<String>,
    /// 上一次检查的结论。
    pub last_outcome: Option<CheckOutcome>,
    /// 上一次失败的原因（§十：保留最近一次原因，供设置页显示）。
    ///
    /// 跳过时**不动它**（跳过会写 `lastOutcome`，但失败原因是"上一次真正检查"的事）；报告里的
    /// `failure` 在跳过时照旧是 `null`，界面不会因此显示一句过期的报错。
    pub last_failure: Option<FailureReport>,
    /// 上一次**跳过**的原因；`lastOutcome` 不是 `Skipped` 时是 `null`。
    ///
    /// 没有它的话，"这台机器为什么不检查"在状态文件里读不出来：`Skipped` 只说明"没查"，区分
    /// 不了"读不到本机版本"（那台机器会一直跳过）与"6 小时节流"（下一次自然会查）。
    pub last_skip_reason: Option<SkipReason>,
}

impl UpdateState {
    /// 这个版本是不是用户按过「忽略」的那个。
    ///
    /// **按版本等价比较，不按字符串**：`0.4.1` 与 `0.4.1.0` 是同一个版本（§三 的补齐规则），
    /// 将来标签从三段变四段时不该把已经忽略过的版本再提示一次。记录本身形状认不出时算"没忽略"
    /// —— 宁可多提示一次，也不能把提示永久吞掉。
    pub(crate) fn is_dismissed(&self, version: &Version) -> bool {
        self.dismissed_version
            .as_deref()
            .and_then(Version::parse)
            == Some(*version)
    }
}

/// 读状态。
///
/// 文件不在、读不了、JSON 坏了，三者都当"没有历史"并留一条日志：更新状态丢了只是"这次从头
/// 开始"，绝不能让检查本身失败，更不能因此拒绝检查。
pub(crate) fn load(path: &Path) -> UpdateState {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            log::warn!("更新状态文件无法解析（{}）：{error}", path.display());
            UpdateState::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => UpdateState::default(),
        Err(error) => {
            log::warn!("更新状态文件无法读取（{}）：{error}", path.display());
            UpdateState::default()
        }
    }
}

/// 写状态。
///
/// 先写同目录的临时文件再改名：半截 JSON 被下一次读到会让"已忽略的版本"这类记录悄悄消失，
/// 而 Windows 上的改名会覆盖已有文件，所以这一次替换是原子的。
pub(crate) fn save(path: &Path, state: &UpdateState) -> std::io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "更新状态文件没有父目录",
        )
    })?;
    std::fs::create_dir_all(directory)?;
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// 自动检查是不是该跑了（§五：启动后首次进入里桌面查一次，之后每 6 小时最多一次；
/// 手动检查不看这个，§四）。
pub(crate) fn auto_check_due(last_checked_at_ms: Option<u64>, now_ms: u64) -> bool {
    match last_checked_at_ms {
        None => true,
        // 时钟回拨（`checkedAt` 落在未来）：允许查这一次，并把正确时间写回去。不这样做的话，
        // 一台时钟曾被设到未来的机器会**永远**不再自动检查；多查一次是自愈的，永久卡住不是。
        Some(last) if last > now_ms => true,
        Some(last) => now_ms - last >= CHECK_INTERVAL_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_file_lives_under_the_app_identifier() {
        let directory = updates_dir(Path::new("data"));
        assert_eq!(directory.file_name().unwrap(), "updates");
        assert_eq!(directory.parent().unwrap().file_name().unwrap(), APP_IDENTIFIER);
        assert_eq!(directory.join(STATE_FILE_NAME).file_name().unwrap(), "state.json");
    }

    #[test]
    fn the_state_file_carries_the_documented_fields() {
        let state = UpdateState {
            latest_version: Some("0.4.2".into()),
            checked_at_ms: Some(1_700_000_000_000),
            dismissed_version: Some("0.4.0".into()),
            downloaded_path: Some("data/0.4.2/setup.exe".into()),
            downloaded_sha256: Some("ab".repeat(32)),
            last_outcome: Some(CheckOutcome::UpdateAvailable),
            last_failure: None,
            last_skip_reason: None,
        };
        let value = serde_json::to_value(&state).unwrap();
        let mut keys = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<String>>();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "checkedAtMs",
                "dismissedVersion",
                "downloadedPath",
                "downloadedSha256",
                "lastFailure",
                "lastOutcome",
                "lastSkipReason",
                "latestVersion",
            ]
        );
        // 结论也要按码落盘（界面按码分支），不是句子。
        assert_eq!(value["lastOutcome"], serde_json::json!("updateAvailable"));
    }

    #[test]
    fn the_state_round_trips_through_its_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = updates_dir(directory.path()).join(STATE_FILE_NAME);
        let state = UpdateState {
            latest_version: Some("0.4.2".into()),
            checked_at_ms: Some(1_700_000_000_000),
            dismissed_version: Some("0.4.0".into()),
            downloaded_path: None,
            downloaded_sha256: None,
            last_outcome: Some(CheckOutcome::UpdateAvailable),
            last_failure: Some(FailureReport::http_status(403)),
            last_skip_reason: Some(SkipReason::VersionUnavailable),
        };
        save(&path, &state).expect("the state file is writable");
        assert_eq!(load(&path), state);
        // 临时文件不留在目录里（原子替换的另一半）。
        assert!(!path.with_extension("json.tmp").exists());

        // 再存一次：Windows 上的改名要能覆盖已有文件，否则"忽略"这类记录第二次就写不进去。
        let updated = UpdateState {
            checked_at_ms: Some(1_800_000_000_000),
            ..state.clone()
        };
        save(&path, &updated).expect("the state file is replaceable");
        assert_eq!(load(&path), updated);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn a_missing_or_corrupt_state_file_reads_as_no_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = updates_dir(directory.path()).join(STATE_FILE_NAME);
        assert_eq!(load(&path), UpdateState::default());

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(load(&path), UpdateState::default());
    }

    #[test]
    fn unknown_fields_do_not_invalidate_the_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = updates_dir(directory.path()).join(STATE_FILE_NAME);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"latestVersion":"0.4.2","somethingNewer":true}"#).unwrap();
        assert_eq!(load(&path).latest_version.as_deref(), Some("0.4.2"));
    }

    /// 旧版本写下的状态文件**没有** `lastSkipReason` 这个键：读它必须取默认（`None`），而不是报错
    /// 把整份历史丢掉。方向也是反的：新字段不可能让旧文件失效，否则升级一次就等于清空状态。
    #[test]
    fn a_state_file_without_the_new_skip_field_still_reads() {
        let directory = tempfile::tempdir().unwrap();
        let path = updates_dir(directory.path()).join(STATE_FILE_NAME);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"latestVersion":"0.4.2","checkedAtMs":1700000000000,"lastOutcome":"upToDate"}"#,
        )
        .unwrap();

        let state = load(&path);
        assert_eq!(state.last_skip_reason, None);
        // 别的字段一个不少（"缺字段取默认"不该顺手把整份状态换成默认值）。
        assert_eq!(state.latest_version.as_deref(), Some("0.4.2"));
        assert_eq!(state.checked_at_ms, Some(1_700_000_000_000));
        assert_eq!(state.last_outcome, Some(CheckOutcome::UpToDate));
    }

    #[test]
    fn the_auto_check_runs_at_most_once_every_six_hours() {
        assert_eq!(CHECK_INTERVAL_MS, 6 * 60 * 60 * 1000);
        let now = 1_700_000_000_000u64;
        // 从没查过：查（启动后首次进入里桌面那一次）。
        assert!(auto_check_due(None, now));
        // 正好 6 小时：查；差 1 毫秒：不查。
        assert!(auto_check_due(Some(now - CHECK_INTERVAL_MS), now));
        assert!(!auto_check_due(Some(now - CHECK_INTERVAL_MS + 1), now));
        assert!(!auto_check_due(Some(now), now));
        // 时钟回拨：允许一次，写回正确时间就自愈，而不是永久不再检查。
        assert!(auto_check_due(Some(now + CHECK_INTERVAL_MS), now));
    }

    #[test]
    fn a_dismissal_matches_the_same_version_written_differently() {
        let dismissed = UpdateState {
            dismissed_version: Some("0.4.1".into()),
            ..Default::default()
        };
        assert!(dismissed.is_dismissed(&Version::parse_tag("v0.4.1").unwrap()));
        assert!(dismissed.is_dismissed(&Version::parse_tag("v0.4.1.0").unwrap()));
        assert!(!dismissed.is_dismissed(&Version::parse_tag("v0.4.2").unwrap()));
        assert!(!UpdateState::default().is_dismissed(&Version::parse_tag("v0.4.1").unwrap()));
        // 记录本身形状认不出：算"没忽略"（宁可多提示一次）。
        let broken = UpdateState {
            dismissed_version: Some("0.4.1 (build 7)".into()),
            ..Default::default()
        };
        assert!(!broken.is_dismissed(&Version::parse_tag("v0.4.1").unwrap()));
    }
}
