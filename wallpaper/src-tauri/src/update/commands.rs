//! 更新检查命令与整条流程（计划书 §四、§五、§八）。
//!
//! 流程切成三段，是为了让"不该打网络的时候一次都不打"能被单测钉死：
//!
//!  1. [`run_check`] 先读状态，就地给出两种"不检查"的结论：读不到本机版本（§3.1）、或者距上次
//!     检查不足 6 小时（§五）；
//!  2. 只有走到这里才会调用**注入进来的**取数入口（生产是
//!     [`super::source::fetch_latest_release`]）；
//!  3. 拿到正文之后全是纯判断：解析、比较、选资产、落盘。

use std::future::Future;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::release::{self, AssetKind, ReleaseAsset, ReleaseInfo};
use super::state::{self, UpdateState};
use super::version::{self, InstalledVersion, Version, VersionSource};
use super::{CheckOutcome, FailureReport, SkipReason};

/// 命令被拒的原因码。只有码 —— 文案由界面按语言说。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCommandError {
    pub code: &'static str,
}

impl UpdateCommandError {
    /// 调用者不是更新检测的两个面（壁纸宿主 / 设置中心）。
    fn forbidden() -> Self {
        Self { code: "forbidden" }
    }

    /// 算不出状态文件的位置（本地数据目录不可用）。
    fn state_path_unavailable() -> Self {
        Self {
            code: "statePathUnavailable",
        }
    }

    /// 要记下的版本串不是一个可比较的版本号（见 [`run_dismiss`]）。
    fn invalid_version() -> Self {
        Self {
            code: "invalidVersion",
        }
    }
}

/// 一次检查的结果。
///
/// 只有码、数字、版本号与地址：界面按 `outcome` 分支、按 `skipReason` / `failure.code` 选句子
/// （`wallpaper/src/i18n/`）。原生侧一句中文句子都不回 —— 这是 §四 与仓库 i18n 约定共同的要求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheckReport {
    pub outcome: CheckOutcome,
    /// 没检查的原因；`outcome == skipped` 之外恒为 `null`。
    pub skip_reason: Option<SkipReason>,
    /// 失败原因；`outcome == failed` 之外恒为 `null`。
    pub failure: Option<FailureReport>,
    /// 本机当前版本。读不到时是 `null`，而那时**没有检查**（`skipped`）。
    pub current_version: Option<String>,
    pub current_version_source: Option<VersionSource>,
    /// 与状态文件里的 `latestVersion` 一致：本次解析出的版本；解析不出或没检查时，是上一次
    /// 记下的那个。
    pub latest_version: Option<String>,
    /// release 页面。资产缺席或（第二片里）下载失败时，界面回落到"打开 release 页面"。
    pub release_url: Option<String>,
    /// 选中的可安装资产；没选到时是 `null`，此时 `outcome == noInstallableAsset`。
    pub asset: Option<ReleaseAsset>,
    /// 上一次真正检查的时间（被节流跳过时，就是那一次的时间）。
    pub checked_at_ms: Option<u64>,
    /// 用户按过「忽略」的那个版本，原样回给界面。
    pub dismissed_version: Option<String>,
    /// `latestVersion` 是否已被忽略。**原生侧算好**：等价比较（`0.4.1` 与 `0.4.1.0` 是同一个
    /// 版本）不该让界面再实现一遍。
    pub dismissed: bool,
    /// 本次结果是否已经落盘。跳过时没有要落的东西，恒为 `true`；写不进去时是 `false`
    /// （本次结果照样显示，只是下次启动会再查一次）。
    pub state_persisted: bool,
}

impl UpdateCheckReport {
    /// 没有检查就给出的结果（读不到版本 / 被节流）。
    fn skipped(state: &UpdateState, skip_reason: SkipReason, installed: Option<&InstalledVersion>) -> Self {
        Self {
            outcome: CheckOutcome::Skipped,
            skip_reason: Some(skip_reason),
            failure: None,
            current_version: installed.map(|installed| installed.version.to_string()),
            current_version_source: installed.map(|installed| installed.source),
            latest_version: state.latest_version.clone(),
            release_url: None,
            asset: None,
            checked_at_ms: state.checked_at_ms,
            dismissed_version: state.dismissed_version.clone(),
            dismissed: recorded_version_is_dismissed(state),
            state_persisted: true,
        }
    }

    /// 真正检查过之后的结果（含失败）。
    fn concluded(
        state: &UpdateState,
        conclusion: Conclusion,
        installed: &InstalledVersion,
        state_persisted: bool,
    ) -> Self {
        Self {
            outcome: conclusion.outcome,
            skip_reason: None,
            failure: conclusion.failure,
            current_version: Some(installed.version.to_string()),
            current_version_source: Some(installed.source),
            latest_version: state.latest_version.clone(),
            release_url: conclusion.release_url,
            asset: conclusion.asset,
            checked_at_ms: state.checked_at_ms,
            dismissed_version: state.dismissed_version.clone(),
            dismissed: recorded_version_is_dismissed(state),
            state_persisted,
        }
    }
}

/// 一次「忽略」的结果。
///
/// 与检查报告一样只有码、版本号与布尔：界面自己说文案。`persisted` 是**要给用户看的**那一半
/// —— 写不进去时"这个版本下次启动还会提示"，界面必须能如实说出来，而不是让用户以为按过了。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateDismissReport {
    /// 记下的版本（按**写下来的段数**输出：`0.4.1` 不写成 `0.4.1.0`）。
    pub dismissed_version: Option<String>,
    /// 记录有没有落盘。写不进去时是 `false`（本次仍然生效，重启之后失效）。
    pub persisted: bool,
}

/// 一次检查的结论：解析成功与失败两条路都汇到这里。
struct Conclusion {
    outcome: CheckOutcome,
    failure: Option<FailureReport>,
    release_url: Option<String>,
    asset: Option<ReleaseAsset>,
    /// 本次解析出的版本。`None` 表示"不要动状态里记着的版本"（标签读不懂，或这次失败了）。
    latest: Option<Version>,
}

impl Conclusion {
    fn failed(failure: FailureReport) -> Self {
        Self {
            outcome: CheckOutcome::Failed,
            failure: Some(failure),
            release_url: None,
            asset: None,
            latest: None,
        }
    }
}

/// 检查更新：决定要不要打网络、打完之后出结论并落盘。
///
/// `fetch` 是**注入进来**的取数入口（§九 2 要求网络层可替换）：生产传
/// [`super::source::fetch_latest_release`]，单测传一个只会返回固定正文的闭包 —— 于是
/// "6 小时内不打网络""读不到版本不检查"这些都能靠数调用次数来验。`state_path` 同理，单测指向
/// 临时目录。
///
/// 时间戳的语义有两处是刻意的：
///
///  - **失败也写** `checkedAtMs`。不写的话，断网时每次启动都会再打一次网络，正好违反 §八 8；
///    节流管的是"查了几次"，不是"成功了几次"。
///  - 标签读不懂时**不覆盖** `latestVersion`：一个读不懂的标签不该把上一次的结论抹掉。
pub(crate) async fn run_check<F, Fut>(
    installed: Option<InstalledVersion>,
    state_path: &Path,
    now_ms: u64,
    manual: bool,
    fetch: F,
) -> UpdateCheckReport
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<String, FailureReport>>,
{
    let mut state = state::load(state_path);

    // 第一段：两个不打网络的就地结论。
    let Some(installed) = installed else {
        // 读不到本机版本 ⇒ 不检查，而不是猜一个版本去比（§3.1、§八 9）。
        log::info!("更新检查：读不到本机版本，跳过本次检查");
        return UpdateCheckReport::skipped(&state, SkipReason::VersionUnavailable, None);
    };
    if !manual && !state::auto_check_due(state.checked_at_ms, now_ms) {
        // 6 小时节流（§五）。手动检查不受限（§四），所以这一支只在自动检查时进得来。
        log::info!("更新检查：距上次检查不足 6 小时，跳过本次（手动检查不受限）");
        return UpdateCheckReport::skipped(&state, SkipReason::Throttled, Some(&installed));
    }

    // 第二段：整条流程里唯一的一次网络。
    let fetched = fetch().await;

    // 第三段：纯判断，然后落盘。
    let conclusion = match fetched {
        Ok(json) => match release::parse_release(&json) {
            Ok(info) => conclude(&info, &installed),
            Err(failure) => Conclusion::failed(failure),
        },
        Err(failure) => Conclusion::failed(failure),
    };
    if let Some(failure) = conclusion.failure {
        log::warn!("更新检查失败：{failure:?}");
    }

    state.checked_at_ms = Some(now_ms);
    if let Some(latest) = conclusion.latest {
        state.latest_version = Some(latest.to_string());
    }
    state.last_outcome = Some(conclusion.outcome);
    state.last_failure = conclusion.failure;
    let state_persisted = match state::save(state_path, &state) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("更新状态写不进去（{}）：{error}", state_path.display());
            false
        }
    };

    UpdateCheckReport::concluded(&state, conclusion, &installed, state_persisted)
}

/// 记下「这个版本用户按过忽略」（§四：忽略记录的是**具体版本**，不是"忽略全部"）。
///
/// 只做一件事：读状态、写 `dismissedVersion`、落盘。**不重新检查**，也不动 `latestVersion` ——
/// 用户按下忽略时手上那个版本号由界面给（就是报告里的 `latestVersion`），原生不替他猜：
/// 猜错就是把一个用户没见过的版本永久吞掉。
///
/// 解析不出的版本串**拒绝**，而不是照写。`UpdateState::is_dismissed` 是按版本**等价**比较的
/// （`0.4.1` 与 `0.4.1.0` 是同一个版本），写进一串认不出的形状等于写了一条永远匹配不上的记录：
/// 界面会以为"已经忽略了"，下次启动同一个版本照旧提示，中间没有任何一处报错。
pub(crate) fn run_dismiss(
    version: &str,
    state_path: &Path,
) -> Result<UpdateDismissReport, UpdateCommandError> {
    let Some(version) = Version::parse(version) else {
        log::warn!("更新忽略：版本串不是一个可比较的版本（{version:?}），拒绝记录");
        return Err(UpdateCommandError::invalid_version());
    };
    let recorded = version.to_string();

    let mut state = state::load(state_path);
    state.dismissed_version = Some(recorded.clone());
    let persisted = match state::save(state_path, &state) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("更新忽略写不进去（{}）：{error}", state_path.display());
            false
        }
    };
    log::info!("更新忽略：已记下 {recorded}（落盘：{persisted}）");

    Ok(UpdateDismissReport {
        dismissed_version: Some(recorded),
        persisted,
    })
}

/// 拿到 release 之后的全部判断（纯函数）。
fn conclude(info: &ReleaseInfo, installed: &InstalledVersion) -> Conclusion {
    let Some(latest) = info.tag.as_deref().and_then(Version::parse_tag) else {
        // 标签不能比较：一律按"没有新版本"处理（§三 宁可漏报），也不覆盖上一次记下的版本。
        log::info!(
            "更新检查：release 标签不是一个可比较的版本（{:?}），按没有新版本处理",
            info.tag
        );
        return Conclusion {
            outcome: CheckOutcome::UpToDate,
            failure: None,
            release_url: info.page_url.clone(),
            asset: None,
            latest: None,
        };
    };

    if !latest.is_newer_than(&installed.version) {
        log::info!("更新检查：本机 {} 已是最新（release {latest}）", installed.version);
        return Conclusion {
            outcome: CheckOutcome::UpToDate,
            failure: None,
            release_url: info.page_url.clone(),
            asset: None,
            latest: Some(latest),
        };
    }

    // 资产选择按后缀白名单，并且优先给与当前安装形态同类的那一个（§3.1）。一个都没选到不是
    // 错误：界面回落"打开 release 页面"（§六），所以这里给的是另一个结论码。
    let asset = release::select_asset(&info.assets, Some(preferred_kind(installed.source))).cloned();
    let outcome = match asset {
        Some(_) => CheckOutcome::UpdateAvailable,
        None => CheckOutcome::NoInstallableAsset,
    };
    log::info!(
        "更新检查：本机 {}（{:?}）→ release {latest} ⇒ {outcome:?}",
        installed.version,
        installed.source
    );
    Conclusion {
        outcome,
        failure: None,
        release_url: info.page_url.clone(),
        asset,
        latest: Some(latest),
    }
}

/// 与当前安装形态同类的资产：打包态给 `.msix`，非打包给 NSIS 的 `.exe`。
fn preferred_kind(source: VersionSource) -> AssetKind {
    match source {
        VersionSource::Packaged => AssetKind::Msix,
        VersionSource::UninstallEntry | VersionSource::Executable => AssetKind::Exe,
    }
}

/// 状态里记着的那个版本是否已被忽略。
fn recorded_version_is_dismissed(state: &UpdateState) -> bool {
    match state.latest_version.as_deref().and_then(Version::parse) {
        Some(latest) => state.is_dismissed(&latest),
        None => false,
    }
}

/// 更新检测的两个面：壁纸宿主（立绘气泡）与设置中心（系统页）。
///
/// 与 `capabilities/*.json` 里的 `allow-update-check` 是**两道独立的门**：能力清单管"这个窗口
/// 能不能调"，这里管"这条命令给谁用"。Lite 的两个窗口都不在名单里，与命令本身的
/// `#[cfg(not(feature = "lite"))]` 一致。窗口标签取自 `lib.rs` 的常量，不写第二份字面量。
fn require_update_surface(caller: &tauri::WebviewWindow) -> Result<(), UpdateCommandError> {
    let label = caller.label();
    (label == crate::BACKGROUND_WINDOW_LABEL || label == crate::SETTINGS_WINDOW_LABEL)
        .then_some(())
        .ok_or_else(UpdateCommandError::forbidden)
}

/// 状态文件的位置：`<本地数据>\com.dsh.wallpaper\updates\state.json`（§五）。
///
/// 优先用 Tauri 算的那个本地数据目录（打包态它会被重定向到包容器里，与其它功能同一处），
/// 拿不到时退回 `dirs` 的 `%LOCALAPPDATA%` —— 两条路拼出来的是同一个目录，退回不是"第二个位置"。
fn state_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let local_app_data = app
        .path()
        .app_local_data_dir()
        .ok()
        .or_else(|| dirs::data_local_dir().map(|directory| directory.join(super::APP_IDENTIFIER)))?;
    Some(state::updates_dir(&local_app_data).join(state::STATE_FILE_NAME))
}

/// 检查更新。只检查，不下载不安装 —— 下载与安装是第二片。
///
/// `manual` 是设置中心那枚「检查更新」按钮（不受 6 小时限制）；不传或传 `false` 是自动检查
/// （进入里桌面时那次），6 小时内最多真正打一次网络（§五、§八 8）。返回值只有结构化码与数字，
/// 文案由界面按语言说。
#[tauri::command]
// 整个模块已经在 `lib.rs` 里按 edition 门控；这一行是仓库惯例的第二道（Lite 不含本功能）。
#[cfg(not(feature = "lite"))]
pub(crate) async fn update_check(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    manual: Option<bool>,
) -> Result<UpdateCheckReport, UpdateCommandError> {
    require_update_surface(&caller)?;
    let Some(state_path) = state_path(&app) else {
        return Err(UpdateCommandError::state_path_unavailable());
    };
    // 版本读取是几次本地小读（包身份 / 注册表 / VERSIONINFO），不值得占一个阻塞线程；
    // 网络那一跳本来就是 async。
    let installed = version::installed_version(&version::VersionProbes::system());
    Ok(run_check(
        installed,
        &state_path,
        state::now_ms(),
        manual.unwrap_or(false),
        super::source::fetch_latest_release,
    )
    .await)
}

/// 「忽略」：把某个版本写进 `dismissedVersion`（§四）。
///
/// 两个面都调它：立绘气泡上的「忽略」与设置中心那一行上的「忽略」。同一条状态迁移只有这一个实现，
/// 于是两个界面对"忽略"的理解不会漂移。`version` 由调用者给出（报告里的 `latestVersion`）——
/// 原生不接受"忽略最新那个"这种说法：那会在两次调用之间变成另一个版本。
///
/// 返回值只有版本号与一个布尔（见 [`UpdateDismissReport`]），文案由界面按语言说。
#[tauri::command]
// 整个模块已经在 `lib.rs` 里按 edition 门控；这一行是仓库惯例的第二道（Lite 不含本功能）。
#[cfg(not(feature = "lite"))]
pub(crate) fn update_dismiss(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    version: String,
) -> Result<UpdateDismissReport, UpdateCommandError> {
    require_update_surface(&caller)?;
    let Some(state_path) = state_path(&app) else {
        return Err(UpdateCommandError::state_path_unavailable());
    };
    // 读状态 + 写一个小 JSON，不占阻塞线程也不打网络。
    run_dismiss(&version, &state_path)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const NOW: u64 = 1_700_000_000_000;

    /// 一个只会数调用次数、永远返回同一段正文的取数入口。
    struct Fetch {
        calls: Cell<usize>,
        answer: Result<String, FailureReport>,
    }

    impl Fetch {
        fn ok(json: &str) -> Self {
            Self {
                calls: Cell::new(0),
                answer: Ok(json.to_string()),
            }
        }

        fn failing(failure: FailureReport) -> Self {
            Self {
                calls: Cell::new(0),
                answer: Err(failure),
            }
        }

        fn call(&self) -> Result<String, FailureReport> {
            self.calls.set(self.calls.get() + 1);
            self.answer.clone()
        }

        /// `run_check` 要的是一个返回 future 的取数入口：包一层 async，测试里没有真的等待。
        async fn fetch(&self) -> Result<String, FailureReport> {
            self.call()
        }
    }

    fn release(tag: &str, assets: &[(&str, u64)]) -> String {
        let assets = assets
            .iter()
            .map(|(name, size)| {
                format!(
                    r#"{{"name":"{name}","size":{size},"browser_download_url":"https://example.test/{name}"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://example.test/tag","assets":[{assets}]}}"#
        )
    }

    fn installed(version: &str, source: VersionSource) -> Option<InstalledVersion> {
        Some(InstalledVersion {
            version: Version::parse(version).expect("a parseable version"),
            source,
        })
    }

    fn state_path(directory: &tempfile::TempDir) -> PathBuf {
        state::updates_dir(directory.path()).join(state::STATE_FILE_NAME)
    }

    /// 今天真身的资产名（NSIS 的 `.exe`），外加一枚后缀在白名单外的噪声文件。
    fn published_release() -> String {
        release(
            "v0.4.2",
            &[
                ("dsh-wallpaper_0.4.2_x64-setup.exe", 31_457_280),
                ("dsh-wallpaper_0.4.2_x64-setup.exe.sig", 400),
            ],
        )
    }

    #[tokio::test]
    async fn a_missing_current_version_skips_the_check_without_touching_the_network() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let fetch = Fetch::ok(&published_release());

        // 手动检查也一样：读不到版本就不比，宁可什么都不说（§3.1、§八 9）。
        let report = run_check(None, &path, NOW, true, || fetch.fetch()).await;

        assert_eq!(fetch.calls.get(), 0);
        assert_eq!(report.outcome, CheckOutcome::Skipped);
        assert_eq!(report.skip_reason, Some(SkipReason::VersionUnavailable));
        assert_eq!(report.current_version, None);
        assert_eq!(report.current_version_source, None);
        assert_eq!(report.latest_version, None);
        assert!(!path.exists(), "a skipped check writes nothing");
    }

    #[tokio::test]
    async fn an_auto_check_within_six_hours_is_skipped_and_a_manual_one_is_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        state::save(
            &path,
            &UpdateState {
                checked_at_ms: Some(NOW - 60_000),
                latest_version: Some("0.4.1".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let automatic = Fetch::ok(&published_release());
        let report = run_check(
            installed("0.4.0", VersionSource::UninstallEntry),
            &path,
            NOW,
            false,
            || automatic.fetch(),
        )
        .await;
        assert_eq!(automatic.calls.get(), 0, "6 小时内不打网络（§八 8）");
        assert_eq!(report.outcome, CheckOutcome::Skipped);
        assert_eq!(report.skip_reason, Some(SkipReason::Throttled));
        assert_eq!(report.current_version.as_deref(), Some("0.4.0"));
        assert_eq!(report.latest_version.as_deref(), Some("0.4.1"));
        assert_eq!(report.checked_at_ms, Some(NOW - 60_000));

        let manual = Fetch::ok(&published_release());
        let report = run_check(
            installed("0.4.0", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || manual.fetch(),
        )
        .await;
        assert_eq!(manual.calls.get(), 1, "手动检查不受限（§四）");
        assert_eq!(report.outcome, CheckOutcome::UpdateAvailable);
        assert_eq!(report.skip_reason, None);
        assert_eq!(report.current_version.as_deref(), Some("0.4.0"));
        assert_eq!(report.current_version_source, Some(VersionSource::UninstallEntry));
        assert_eq!(report.latest_version.as_deref(), Some("0.4.2"));
        assert_eq!(
            report.release_url.as_deref(),
            Some("https://example.test/tag")
        );
        let asset = report.asset.expect("the setup executable");
        assert_eq!(asset.name, "dsh-wallpaper_0.4.2_x64-setup.exe");
        assert_eq!(asset.size, 31_457_280);
        assert_eq!(report.checked_at_ms, Some(NOW));
        assert!(report.state_persisted);
    }

    #[tokio::test]
    async fn a_packaged_install_is_offered_the_msix_when_the_release_has_both() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let json = release(
            "v0.4.2",
            &[
                ("dsh-wallpaper_0.4.2_x64-setup.exe", 10),
                ("dsh-wallpaper_0.4.2.msix", 20),
            ],
        );
        let fetch = Fetch::ok(&json);
        let report = run_check(
            installed("0.2.0.202", VersionSource::Packaged),
            &path,
            NOW,
            true,
            || fetch.fetch(),
        )
        .await;
        assert_eq!(report.outcome, CheckOutcome::UpdateAvailable);
        assert_eq!(report.asset.unwrap().name, "dsh-wallpaper_0.4.2.msix");
    }

    #[tokio::test]
    async fn a_tag_that_cannot_be_compared_is_not_an_update_and_not_a_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let fetch = Fetch::ok(&release(
            "checkpoint-2026-09-28",
            &[("dsh-wallpaper_x64-setup.exe", 10)],
        ));

        let report = run_check(
            installed("0.4.1", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || fetch.fetch(),
        )
        .await;

        assert_eq!(report.outcome, CheckOutcome::UpToDate);
        assert_eq!(report.failure, None);
        assert_eq!(report.latest_version, None);
        assert_eq!(report.asset, None);
        // 读不懂的标签不覆盖上一次记下的版本（状态里还是原来的结论）。
        let stored = state::load(&path);
        assert_eq!(stored.latest_version, None);
        assert_eq!(stored.last_outcome, Some(CheckOutcome::UpToDate));
    }

    #[tokio::test]
    async fn a_release_without_an_installable_asset_is_its_own_outcome() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let fetch = Fetch::ok(&release(
            "v0.4.2",
            &[("dsh-wallpaper_0.4.2.nsis.zip", 10), ("SHA256SUMS.txt", 1)],
        ));

        let report = run_check(
            installed("0.4.1", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || fetch.fetch(),
        )
        .await;

        // 不是错误：界面拿到 release 页面地址，回落到"打开 release 页面"（§六）。
        assert_eq!(report.outcome, CheckOutcome::NoInstallableAsset);
        assert_eq!(report.failure, None);
        assert_eq!(report.asset, None);
        assert_eq!(report.release_url.as_deref(), Some("https://example.test/tag"));
        assert_eq!(report.latest_version.as_deref(), Some("0.4.2"));
    }

    #[tokio::test]
    async fn a_failed_check_keeps_the_last_version_and_still_records_the_timestamp() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        state::save(
            &path,
            &UpdateState {
                checked_at_ms: Some(NOW - 7 * 60 * 60 * 1000),
                latest_version: Some("0.4.1".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let fetch = Fetch::failing(FailureReport::http_status(403));
        let report = run_check(
            installed("0.4.0", VersionSource::UninstallEntry),
            &path,
            NOW,
            false,
            || fetch.fetch(),
        )
        .await;

        assert_eq!(report.outcome, CheckOutcome::Failed);
        assert_eq!(report.failure, Some(FailureReport::http_status(403)));
        assert_eq!(report.latest_version.as_deref(), Some("0.4.1"));
        assert!(report.state_persisted);

        let stored = state::load(&path);
        assert_eq!(stored.checked_at_ms, Some(NOW), "失败也算查过一次");
        assert_eq!(stored.last_outcome, Some(CheckOutcome::Failed));
        assert_eq!(stored.last_failure, Some(FailureReport::http_status(403)));
        assert_eq!(stored.latest_version.as_deref(), Some("0.4.1"));

        // 断网时不会每次启动都打网络：紧接着的那次自动检查被节流住（§八 8）。
        let next = Fetch::ok(&published_release());
        let report = run_check(
            installed("0.4.0", VersionSource::UninstallEntry),
            &path,
            NOW + 60_000,
            false,
            || next.fetch(),
        )
        .await;
        assert_eq!(next.calls.get(), 0);
        assert_eq!(report.skip_reason, Some(SkipReason::Throttled));
    }

    #[tokio::test]
    async fn a_dismissed_version_comes_back_marked_as_dismissed() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        // 用户按「忽略」时记下的是三段写法；release 标签这次写成四段，是同一个版本。
        state::save(
            &path,
            &UpdateState {
                dismissed_version: Some("0.4.2".into()),
                latest_version: Some("0.4.2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let fetch = Fetch::ok(&release(
            "v0.4.2.0",
            &[("dsh-wallpaper_0.4.2_x64-setup.exe", 10)],
        ));

        let report = run_check(
            installed("0.4.1", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || fetch.fetch(),
        )
        .await;

        assert_eq!(report.outcome, CheckOutcome::UpdateAvailable);
        assert_eq!(report.dismissed_version.as_deref(), Some("0.4.2"));
        assert!(report.dismissed, "0.4.2 与 0.4.2.0 是同一个版本");
    }

    /// 界面按这些字段名与码分支，所以它们是契约（与状态文件那条同理）。
    #[tokio::test]
    async fn the_report_serializes_to_the_fields_the_interface_reads() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let fetch = Fetch::ok(&published_release());
        let report = run_check(
            installed("0.4.1", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || fetch.fetch(),
        )
        .await;

        let value = serde_json::to_value(&report).unwrap();
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
                "asset",
                "checkedAtMs",
                "currentVersion",
                "currentVersionSource",
                "dismissed",
                "dismissedVersion",
                "failure",
                "latestVersion",
                "outcome",
                "releaseUrl",
                "skipReason",
                "statePersisted",
            ]
        );
        assert_eq!(value["outcome"], serde_json::json!("updateAvailable"));
        assert_eq!(value["skipReason"], serde_json::json!(null));
        assert_eq!(value["failure"], serde_json::json!(null));
        assert_eq!(value["dismissed"], serde_json::json!(false));
        assert_eq!(value["asset"]["name"], serde_json::json!("dsh-wallpaper_0.4.2_x64-setup.exe"));
        assert_eq!(
            value["asset"]["downloadUrl"],
            serde_json::json!("https://example.test/dsh-wallpaper_0.4.2_x64-setup.exe")
        );
    }

    /// 「忽略」记下的是**具体版本**：同一个版本不再提示，更晚的版本仍然提示（§四、§八 3）。
    #[tokio::test]
    async fn a_dismissal_silences_that_version_and_leaves_later_ones_alone() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);

        let report = run_dismiss("0.4.2", &path).expect("a parseable version is accepted");
        assert_eq!(report.dismissed_version.as_deref(), Some("0.4.2"));
        assert!(report.persisted);
        // 写的是状态文件里那一个字段，而不是别处：下一次检查就是从这儿读出来的。
        assert_eq!(state::load(&path).dismissed_version.as_deref(), Some("0.4.2"));

        // 同一个版本（哪怕 release 这次写成四段）不再提示。
        let same = Fetch::ok(&release(
            "v0.4.2.0",
            &[("dsh-wallpaper_0.4.2_x64-setup.exe", 10)],
        ));
        let report = run_check(
            installed("0.4.1", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || same.fetch(),
        )
        .await;
        assert!(report.dismissed);
        assert_eq!(report.outcome, CheckOutcome::UpdateAvailable, "忽略不改结论，只是让界面别提示");

        // 更晚的版本照旧提示。
        let later = Fetch::ok(&release(
            "v0.4.3",
            &[("dsh-wallpaper_0.4.3_x64-setup.exe", 10)],
        ));
        let report = run_check(
            installed("0.4.1", VersionSource::UninstallEntry),
            &path,
            NOW,
            true,
            || later.fetch(),
        )
        .await;
        assert!(!report.dismissed, "0.4.3 不该被 0.4.2 的那条记录吞掉");
    }

    /// 记录写不下来时**照样回报**，只是 `persisted` 是 `false`：界面要能把"下次启动还会提示"
    /// 说出来，而不是让用户以为按过了。
    #[test]
    fn an_unwritable_state_file_is_reported_instead_of_swallowed() {
        let directory = tempfile::tempdir().unwrap();
        // 把状态文件的路径指到一个**目录**上：读是"没有历史"，写必然失败。
        let blocked = directory.path().join("state.json");
        std::fs::create_dir_all(&blocked).unwrap();

        let report = run_dismiss("0.4.2", &blocked).expect("the version itself is fine");
        assert_eq!(report.dismissed_version.as_deref(), Some("0.4.2"));
        assert!(!report.persisted);
    }

    /// 认不出版本的串**拒绝**，不写一条永远匹配不上的记录（见 `run_dismiss`）。
    #[test]
    fn a_version_that_cannot_be_compared_is_refused_rather_than_recorded() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);

        for version in ["", "   ", "latest", "0.4.2 (build 7)", "0.4.2.3.4"] {
            let error = run_dismiss(version, &path).expect_err("must be refused");
            assert_eq!(error.code, "invalidVersion", "{version:?}");
        }
        assert!(
            !path.exists(),
            "拒绝之后一个字节都不该落盘：写下去等于记了一条永远匹配不上的忽略"
        );
        // 带 `v` 前缀或首尾空白是允许的（`Version::parse` 的规则），记下来的是规范写法。
        assert_eq!(
            run_dismiss(" v0.4.2 ", &path).unwrap().dismissed_version.as_deref(),
            Some("0.4.2")
        );
    }

    /// 「忽略」的结果也是契约：界面按这两个字段名读（`dismissedVersion` / `persisted`）。
    #[test]
    fn the_dismiss_report_serializes_to_the_fields_the_interface_reads() {
        let report = UpdateDismissReport {
            dismissed_version: Some("0.4.2".into()),
            persisted: false,
        };
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            r#"{"dismissedVersion":"0.4.2","persisted":false}"#
        );
    }

    /// 上一版就是在这儿栽的：命令写好了、`build.rs` 也登记了，但**没给窗口授权**，于是界面每
    /// 一次调用都被能力系统拒掉（报错原文：`Command ... not allowed by ACL`）。这道测试把"四处
    /// 必须同时存在"钉住：permission 文件（allow 与 deny 两条）、两个窗口的 capability、
    /// `build.rs` 的命令清单、以及 `generate_handler!`。
    ///
    /// **表驱动**：加一条更新命令就在 `COMMANDS` 里加一行 —— 漏任何一处，界面上的按钮就是
    /// "点了没反应"，而这类故障在构建期一声不响。
    #[test]
    fn every_update_command_is_registered_in_every_acl_place_it_is_called_through() {
        /// (命令名, 授权标识符)。`update_check` 是"查"，`update_dismiss` 是「忽略」。
        const COMMANDS: [(&str, &str); 2] =
            [("update_check", "allow-update-check"), ("update_dismiss", "allow-update-dismiss")];

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let build = std::fs::read_to_string(root.join("build.rs")).expect("build.rs must exist");
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib.rs must exist");

        for (command, permission) in COMMANDS {
            // 1. permission 文件（照 `permissions/autogenerated/` 的既有格式：allow + deny 两条）。
            let file = root.join(format!("permissions/autogenerated/{command}.toml"));
            let text = std::fs::read_to_string(&file)
                .unwrap_or_else(|error| panic!("{} 必须存在：{error}", file.display()));
            assert!(text.contains(&format!(r#"identifier = "{permission}""#)), "{text}");
            assert!(text.contains(&format!(r#"commands.allow = ["{command}"]"#)), "{text}");
            let denied = permission.replace("allow-", "deny-");
            assert!(text.contains(&format!(r#"identifier = "{denied}""#)), "{text}");
            assert!(text.contains(&format!(r#"commands.deny = ["{command}"]"#)), "{text}");

            // 2. 两个窗口的 capability：气泡在壁纸窗口里、那一行在设置窗口里，两个都要授权。
            for capability in ["background", "settings"] {
                let text =
                    std::fs::read_to_string(root.join(format!("capabilities/{capability}.json")))
                        .expect("the capability file must exist");
                assert!(
                    text.contains(&format!(r#""{permission}""#)),
                    "{capability} must grant {command}"
                );
            }
            // 3. Lite 两个窗口都不给（命令本身也不在 Lite 里，§七）。
            for capability in ["lite-background", "lite-settings"] {
                let text =
                    std::fs::read_to_string(root.join(format!("capabilities/{capability}.json")))
                        .expect("the capability file must exist");
                assert!(
                    !text.contains(permission),
                    "{capability} must not grant {command}"
                );
            }

            // 4. 没在 `build.rs` 的清单里声明，命令根本不会有 ACL 条目；没在
            //    `generate_handler!` 里登记，命令根本不存在。
            assert!(
                build.contains(&format!(r#""{command}""#)),
                "build.rs must declare {command}"
            );
            assert!(
                lib.contains(&format!("update::commands::{command}")),
                "lib.rs must register {command} in generate_handler!"
            );
        }
    }
}
