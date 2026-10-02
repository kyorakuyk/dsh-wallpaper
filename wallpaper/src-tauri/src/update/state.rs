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

/// 状态文件所在目录：`<本地数据>\updates`，即 `%LOCALAPPDATA%\com.dsh.wallpaper\updates`。
///
/// 参数是**应用自己的**本地数据目录，也就是 Tauri 的 `app_local_data_dir()` —— 它**已经带上包
/// 标识符**（`%LOCALAPPDATA%\com.dsh.wallpaper`），所以这里只补一层 `updates`。
///
/// 0.4.6 及以前的这里又拼了一次 [`APP_IDENTIFIER`]，于是状态与安装包落在
/// `%LOCALAPPDATA%\com.dsh.wallpaper\com.dsh.wallpaper\updates` —— 标识符拼了两遍，那个目录
/// 谁也不会去看一眼（真机上攒了 237 MB 的旧安装包）。形状断言见
/// `the_state_file_sits_directly_under_the_local_data_directory`，旧位置的清理见
/// [`migrate_legacy_updates`]。
pub(crate) fn updates_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join(UPDATES_DIR_NAME)
}

/// 0.4.6 及以前写坏的位置：`<本地数据>\<标识符>\updates`。
///
/// 只给迁移用（[`migrate_legacy_updates`]）：新代码不许再往这里写任何东西。名字留在这一层的理由
/// 与 [`APP_IDENTIFIER`] 相同 —— 两处各拼一份的话，"要搬走的东西"迟早不是同一个目录。
pub(crate) fn legacy_updates_dir(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join(APP_IDENTIFIER)
        .join(UPDATES_DIR_NAME)
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

/// 一次迁移做了多少事。只用来记日志与给测试看 —— 迁移本身**不返回错误**（理由见
/// [`migrate_legacy_updates`]）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MigrationOutcome {
    /// 搬到正确位置的条目数。
    pub moved: usize,
    /// 位置已经有一个同名的、或者搬不动，于是删掉的条目数。
    pub removed: usize,
    /// 搬也搬不动、删也删不掉的条目数（被别的进程占着多半是这种）。
    pub failed: usize,
}

impl MigrationOutcome {
    /// 一件都没做：没有旧目录，或者旧目录是空的。
    ///
    /// 有了它，调用方才能把"这次真的搬了东西"与"本来就没事"分开说 —— 每次启动都喊一句
    /// "迁移完成"的日志，看日志的人就再也分不出哪一次真做了事。
    pub(crate) fn did_nothing(&self) -> bool {
        self.moved == 0 && self.removed == 0 && self.failed == 0
    }
}

/// 把旧（多套了一层标识符）的更新目录搬进正确的位置：**能搬就搬，搬不动就删，删不掉就留着**。
///
/// 三处刻意的选择：
///
///  - **不返回 `Result`**：这是"顺手清理"，不是一条功能。旧目录不存在、目标目录建不出来、
///    某个文件被占着 —— 都不该让更新检查或启动失败，所以失败只记账、只记日志；
///  - **先整套 `rename`**：同一卷上的改名不搬字节，"4 个 59 MB 的安装包"是常数时间；`rename`
///    失败（跨卷、被占用）才退化成删除 —— 旧位置上的安装包没有任何代码会再去读，占地才是问题；
///  - **目标位置已有同名条目时删旧的**：两份一样的东西里，正确位置上的那一份才是"现在用的"；
///    反过来覆盖会把可能正被读的那一份换掉。
///
/// 两层空目录（`…\标识符\updates` 与 `…\标识符`）顺手删掉：`remove_dir` 只删空目录，
/// 所以那里要是还有别的东西，一步都不会动。
pub(crate) fn migrate_legacy_updates(legacy: &Path, target: &Path) -> MigrationOutcome {
    let mut outcome = MigrationOutcome::default();
    // `legacy == target` 是"调用方给错了参数"（比如哪天常量改了）：宁可不做，也不要在同一个
    // 目录里删自己的东西。
    if legacy == target || !legacy.is_dir() {
        return outcome;
    }
    let entries = match std::fs::read_dir(legacy) {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!("更新目录迁移：旧目录读不了（{}）：{error}", legacy.display());
            outcome.failed += 1;
            return outcome;
        }
    };
    if let Err(error) = std::fs::create_dir_all(target) {
        log::warn!(
            "更新目录迁移：目标目录建不出来（{}）：{error}",
            target.display()
        );
        outcome.failed += 1;
        return outcome;
    }
    for entry in entries {
        let Ok(entry) = entry else {
            outcome.failed += 1;
            continue;
        };
        let from = entry.path();
        let to = target.join(entry.file_name());
        if !to.exists() && std::fs::rename(&from, &to).is_ok() {
            outcome.moved += 1;
            continue;
        }
        // 正确位置上已经有了，或者搬不动：旧的这一份不再有用。
        match remove_file_or_directory(&from) {
            Ok(()) => outcome.removed += 1,
            Err(error) => {
                log::warn!("更新目录迁移：{} 搬不动也删不掉：{error}", from.display());
                outcome.failed += 1;
            }
        }
    }
    let _ = std::fs::remove_dir(legacy);
    if let Some(identifier_directory) = legacy.parent() {
        let _ = std::fs::remove_dir(identifier_directory);
    }
    if !outcome.did_nothing() {
        log::info!(
            "更新目录迁移：{} → {}（搬走 {}，清掉 {}，没动成 {}）",
            legacy.display(),
            target.display(),
            outcome.moved,
            outcome.removed,
            outcome.failed
        );
    }
    outcome
}

/// 删一个文件，或者一整棵目录（迁移里"搬不动的就删"那一步）。
fn remove_file_or_directory(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// 状态里记着的那条安装包路径**原本落在旧目录**时，把它指到新位置（纯函数）。
///
/// 迁移把文件搬走了，可状态文件里记的还是旧绝对路径。不改写的话，设置中心照样显示"已下载"，
/// 而「点击安装」会说"记录里的文件已经不在了" —— 文件其实就躺在旁边。
///
/// 返回值：`None` = 这条记录与旧目录无关（不用动）；`Some(state)` = 记着的那条要改写。
/// 文件在新位置也不在了（真被删了、或者当初就没搬成）时，如实把这条记录清掉：界面因此说
/// "还没下过"，而不是把一句用户无法理解的位置当成原因。
pub(crate) fn repoint_downloaded_path(
    state: &UpdateState,
    legacy: &Path,
    target: &Path,
) -> Option<UpdateState> {
    let recorded = state.downloaded_path.as_deref()?;
    let relative = Path::new(recorded).strip_prefix(legacy).ok()?.to_path_buf();
    let mut updated = state.clone();
    let moved = target.join(relative);
    if moved.is_file() {
        updated.downloaded_path = Some(moved.display().to_string());
    } else {
        updated.downloaded_path = None;
        updated.downloaded_sha256 = None;
    }
    Some(updated)
}

/// 该删掉的"已下载安装包"目录（纯函数）：`updates\<版本>` 里版本不在 `keep` 里那些。
///
/// 保守到只认形状：名字里的版本解析不出来（`state.json`、`tmp`、随手起的名）一律不碰 ——
/// 这是个删目录的动作，认错一个名字就是删掉别人的东西。
///
/// `keep` 由调用方给（见 `commands::versions_to_keep`：**正在跑的这一版**，加上状态里记着
/// 要装的那一枚）。`keep` 一个都没有时返回空名单：读不到当前版本就说不清"哪些是旧的"，
/// 那时候宁可一个都不删 —— 一个删目录的功能，猜错一次就是删掉正在用的东西。
///
/// 判断的是**版本等价**，不是字符串相等：`0.4.2` 与 `0.4.2.0` 是同一个版本，不会因为写法不同
/// 就被当成"别人的版本"删掉。
pub(crate) fn stale_installer_dirs(entries: &[PathBuf], keep: &[Version]) -> Vec<PathBuf> {
    if keep.is_empty() {
        return Vec::new();
    }
    entries
        .iter()
        .filter(|entry| {
            let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            let Some(version) = Version::parse(name) else {
                return false;
            };
            !keep.contains(&version)
        })
        .cloned()
        .collect()
}

/// 删掉不该留着的旧版本安装包（每个版本约 59 MB，留着就是永久占用）。返回删掉了几个目录。
///
/// 失败只记日志：清理是顺手做的事，不是任何一条命令的前置条件 —— 删不掉一个被占着的目录，
/// 更新检查照跑。
pub(crate) fn prune_downloaded_installers(updates_dir: &Path, keep: &[Version]) -> usize {
    let Ok(entries) = std::fs::read_dir(updates_dir) else {
        return 0;
    };
    // 只对**目录**动手：`state.json` 与别人放的散文件不在这个函数的射程里。
    let candidates = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<PathBuf>>();
    let mut removed = 0;
    for stale in stale_installer_dirs(&candidates, keep) {
        match std::fs::remove_dir_all(&stale) {
            Ok(()) => {
                removed += 1;
                log::info!("更新目录清理：删掉旧版本安装包目录 {}", stale.display());
            }
            Err(error) => log::warn!("更新目录清理：{} 删不掉：{error}", stale.display()),
        }
    }
    removed
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

    /// 状态路径的形状：**`<应用本地数据目录>\updates\state.json`**，标识符只出现一次。
    ///
    /// 这是 0.4.6 那个 bug 的回归钉子：`app_local_data_dir()` 自己已经带上包标识符，而那时候这里
    /// 又拼了一次，于是状态与安装包落在 `…\com.dsh.wallpaper\com.dsh.wallpaper\updates`（真机上
    /// 攒了 237 MB 的旧安装包）。断言取"拼出来的完整路径里标识符只出现一次"：少拼一层会让它红，
    /// 多拼一层同样会。
    #[test]
    fn the_state_file_sits_directly_under_the_local_data_directory() {
        // 打包态：Tauri 给 `app_local_data_dir()` 的返回值末尾**已经是**标识符。
        let local_app_data = Path::new("C:\\Users\\u\\AppData\\Local\\com.dsh.wallpaper");
        let directory = updates_dir(local_app_data);
        assert_eq!(directory, local_app_data.join("updates"));
        assert_eq!(
            directory.join(STATE_FILE_NAME),
            Path::new("C:\\Users\\u\\AppData\\Local\\com.dsh.wallpaper\\updates\\state.json")
        );
        let text = directory.display().to_string();
        assert_eq!(
            text.matches(APP_IDENTIFIER).count(),
            1,
            "标识符被拼了第二次：{text}"
        );

        // 旧位置仍然算得出来（迁移要按它去找东西），而且它与正确位置是两个不同的目录。
        assert_eq!(
            legacy_updates_dir(local_app_data),
            local_app_data.join(APP_IDENTIFIER).join("updates")
        );
        assert_ne!(legacy_updates_dir(local_app_data), directory);
    }

    /// 迁移的三种形状：没有旧目录（什么都不做，也**不**顺手建出目标目录）、有旧目录（逐条搬走）、
    /// 旧目录里既有子目录又有散文件。
    #[test]
    fn the_legacy_directory_is_moved_entry_by_entry() {
        let directory = tempfile::tempdir().unwrap();
        let local_app_data = directory.path().join(APP_IDENTIFIER);
        let target = updates_dir(&local_app_data);
        let legacy = legacy_updates_dir(&local_app_data);

        // 旧目录不存在：一件都不做。
        assert!(migrate_legacy_updates(&legacy, &target).did_nothing());
        assert!(!target.exists(), "没有东西要搬时不该顺手建出目标目录");

        // 旧目录里：一个状态文件、一个版本目录（里面一个安装包）、一个散文件。
        std::fs::create_dir_all(legacy.join("0.4.6")).unwrap();
        std::fs::write(legacy.join(STATE_FILE_NAME), "{}").unwrap();
        std::fs::write(legacy.join("0.4.6").join("setup.exe"), b"installer").unwrap();
        std::fs::write(legacy.join("stray.txt"), b"x").unwrap();

        let outcome = migrate_legacy_updates(&legacy, &target);
        assert_eq!(
            outcome,
            MigrationOutcome {
                moved: 3,
                removed: 0,
                failed: 0
            }
        );
        assert!(!legacy.exists(), "搬空之后旧目录本身也该走");
        assert!(
            !legacy.parent().unwrap().exists(),
            "上面那层标识符目录也空了，一并删掉"
        );
        assert_eq!(
            std::fs::read_to_string(target.join(STATE_FILE_NAME)).unwrap(),
            "{}"
        );
        assert!(target.join("0.4.6").join("setup.exe").is_file());
        assert!(target.join("stray.txt").is_file());
    }

    /// 目标位置已经有同名条目时**删旧的、不覆盖新的**：正确位置上那一份才是"现在用的"，
    /// 覆盖会把可能正被读的那一份换掉。
    #[test]
    fn a_duplicate_in_the_right_place_wins_over_the_legacy_one() {
        let directory = tempfile::tempdir().unwrap();
        let local_app_data = directory.path().join(APP_IDENTIFIER);
        let target = updates_dir(&local_app_data);
        let legacy = legacy_updates_dir(&local_app_data);
        std::fs::create_dir_all(target.join("0.4.6")).unwrap();
        std::fs::write(target.join(STATE_FILE_NAME), "new").unwrap();
        std::fs::create_dir_all(legacy.join("0.4.3")).unwrap();
        std::fs::write(legacy.join(STATE_FILE_NAME), "old").unwrap();
        std::fs::write(legacy.join("0.4.3").join("setup.exe"), b"installer").unwrap();

        let outcome = migrate_legacy_updates(&legacy, &target);

        assert_eq!(
            outcome,
            MigrationOutcome {
                moved: 1,
                removed: 1,
                failed: 0
            }
        );
        assert_eq!(
            std::fs::read_to_string(target.join(STATE_FILE_NAME)).unwrap(),
            "new"
        );
        assert!(target.join("0.4.3").join("setup.exe").is_file());
    }

    /// 目标位置建不出来（那儿被一个同名文件占着，或者磁盘不让写）：只记账、只记日志，
    /// 旧内容一个字节都不动 —— 下一次启动还会再试一遍，而启动与检查都不受影响。
    #[test]
    fn a_target_that_cannot_be_created_is_only_counted() {
        let directory = tempfile::tempdir().unwrap();
        let local_app_data = directory.path().join(APP_IDENTIFIER);
        let legacy = legacy_updates_dir(&local_app_data);
        std::fs::create_dir_all(legacy.join("0.4.3")).unwrap();
        std::fs::write(legacy.join("0.4.3").join("setup.exe"), b"installer").unwrap();
        let target = updates_dir(&local_app_data);
        std::fs::write(&target, b"not a directory").unwrap();

        let outcome = migrate_legacy_updates(&legacy, &target);

        assert_eq!(
            outcome,
            MigrationOutcome {
                moved: 0,
                removed: 0,
                failed: 1
            }
        );
        assert!(legacy.join("0.4.3").join("setup.exe").is_file());
    }

    /// 状态里那条路径跟着迁移走：旧前缀换成新前缀；新位置也没有那个文件时，如实把这条记录
    /// （含摘要）清掉 —— 界面因此说"还没下过"，而不是说"记录里的文件已经不在了"。
    #[test]
    fn the_recorded_installer_path_follows_the_migration() {
        let local_app_data = Path::new("C:\\Users\\u\\AppData\\Local\\com.dsh.wallpaper");
        let legacy = legacy_updates_dir(local_app_data);
        let target = updates_dir(local_app_data);

        // 没有记录：不用动。记录本来就在正确位置上：也不用动。
        assert_eq!(repoint_downloaded_path(&UpdateState::default(), &legacy, &target), None);
        let already_right = UpdateState {
            downloaded_path: Some(target.join("0.4.6").join("setup.exe").display().to_string()),
            ..Default::default()
        };
        assert_eq!(repoint_downloaded_path(&already_right, &legacy, &target), None);

        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join(APP_IDENTIFIER).join("updates");
        let new = directory.path().join("updates");
        std::fs::create_dir_all(new.join("0.4.6")).unwrap();
        std::fs::write(new.join("0.4.6").join("setup.exe"), b"installer").unwrap();

        let moved = UpdateState {
            downloaded_path: Some(old.join("0.4.6").join("setup.exe").display().to_string()),
            downloaded_sha256: Some("sha256:ab".into()),
            ..Default::default()
        };
        let repointed = repoint_downloaded_path(&moved, &old, &new).unwrap();
        assert_eq!(
            repointed.downloaded_path,
            Some(new.join("0.4.6").join("setup.exe").display().to_string())
        );
        assert_eq!(repointed.downloaded_sha256.as_deref(), Some("sha256:ab"));

        let gone = UpdateState {
            downloaded_path: Some(old.join("0.4.5").join("setup.exe").display().to_string()),
            downloaded_sha256: Some("sha256:ab".into()),
            ..Default::default()
        };
        let cleared = repoint_downloaded_path(&gone, &old, &new).unwrap();
        assert_eq!(cleared.downloaded_path, None);
        assert_eq!(cleared.downloaded_sha256, None);
    }

    /// 清理名单只装"版本对不上"的那几个目录名：`state.json`、随手起的名、`0.4.x` 这种解析不出的
    /// 一律不碰；当前版本与状态里记着要装的那一版也不碰。
    #[test]
    fn only_directories_of_other_versions_are_selected_for_deletion() {
        let entries = |names: &[&str]| {
            names
                .iter()
                .map(|name| PathBuf::from("updates").join(name))
                .collect::<Vec<PathBuf>>()
        };
        let keep = [
            Version::parse("0.4.6").unwrap(),
            Version::parse("0.4.7").unwrap(),
        ];
        let candidates = entries(&[
            "0.4.3",
            "0.4.6",
            "0.4.6.0", // 同一个版本的另一种写法：仍算当前版本
            "0.4.7",
            "0.5.0",
            "state.json",
            "tmp",
            "0.4.x",
        ]);

        assert_eq!(
            stale_installer_dirs(&candidates, &keep),
            entries(&["0.4.3", "0.5.0"]),
            "只该删「不是这几版」的版本目录"
        );
        assert_eq!(
            stale_installer_dirs(&candidates, &[]),
            Vec::<PathBuf>::new(),
            "一个要留的版本都给不出来（当前版本都读不到）时，什么都不删"
        );
        assert_eq!(stale_installer_dirs(&[], &keep), Vec::<PathBuf>::new());
    }

    /// 真删的那一层：只删目录、不碰 `state.json`，被占着的删不掉也只是记一条日志。
    #[test]
    fn pruning_removes_only_the_other_versions_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let updates = updates_dir(&directory.path().join(APP_IDENTIFIER));
        for name in ["0.4.3", "0.4.6"] {
            std::fs::create_dir_all(updates.join(name)).unwrap();
            std::fs::write(updates.join(name).join("setup.exe"), b"installer").unwrap();
        }
        std::fs::write(updates.join(STATE_FILE_NAME), "{}").unwrap();

        let removed = prune_downloaded_installers(
            &updates,
            &[Version::parse("0.4.6").unwrap()],
        );

        assert_eq!(removed, 1);
        assert!(updates.join("0.4.6").join("setup.exe").is_file());
        assert!(!updates.join("0.4.3").exists());
        assert!(updates.join(STATE_FILE_NAME).is_file());
        // 目录都不在时是 0，不是 panic。
        assert_eq!(
            prune_downloaded_installers(&directory.path().join("missing"), &[]),
            0
        );
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
