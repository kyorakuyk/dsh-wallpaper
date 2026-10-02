//! 更新检查与下载/安装命令、以及整条流程（计划书 §四、§五、§六）。
//!
//! 流程切成三段，是为了让"不该打网络的时候一次都不打"能被单测钉死：
//!
//!  1. [`run_check`] 先读状态，就地给出两种"不检查"的结论：读不到本机版本（§3.1）、或者距上次
//!     检查不足 6 小时（§五）。**两条跳过都落盘**（[`persist_skip`]）：跳过不留痕的话，"没触发过"
//!     与"跑了但跳过"就只能靠日志区分，而日志会被截断；
//!  2. 只有走到这里才会调用**注入进来的**取数入口（生产是
//!     [`super::source::fetch_latest_release`]）；
//!  3. 拿到正文之后全是纯判断：解析、比较、选资产、落盘。
//!
//! 下载与安装是**另外两条命令**（[`update_download`] / [`update_install`]），只由界面按下按钮那
//! 一次发起：`run_check` 这条路上没有任何下载调用（§六 "绝不在检查时自动下载"），单测里由
//! `checking_never_downloads_anything` 钉着。

use std::future::Future;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::download::{self, DownloadEvent, DownloadRequest};
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

    /// 版本或资产名拼不出一个安全的落盘路径（见 [`super::download::destination`]）。
    fn destination_unavailable() -> Self {
        Self {
            code: "destinationUnavailable",
        }
    }

    /// 下载地址不在发布仓库那一族里（见 [`super::download::trusted_asset_url`]）。
    fn untrusted_asset_url() -> Self {
        Self {
            code: "untrustedAssetUrl",
        }
    }

    /// 状态文件里没有"下过什么"这条记录 ⇒ 没有可装的东西。
    fn nothing_downloaded() -> Self {
        Self {
            code: "nothingDownloaded",
        }
    }

    /// 记录里那个文件已经不在了（用户删了、或清理工具扫走了）。
    fn installer_missing() -> Self {
        Self {
            code: "installerMissing",
        }
    }

    /// 后缀不在白名单里（只认 `.exe` 与 `.msix`，§六）。
    fn unsupported_asset() -> Self {
        Self {
            code: "unsupportedAsset",
        }
    }

    /// `ShellExecuteW` 没打开它（没有默认处理程序、被策略拦住等）。
    fn open_failed() -> Self {
        Self {
            code: "openFailed",
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
    /// 本次结果是否已经落盘。**跳过也算落盘**（见 [`persist_skip`]）：跳过会写
    /// `lastOutcome` + `lastSkipReason`，正是为了事后能区分"没跑过"与"跑了但跳过"；写不进去时是
    /// `false`（本次结果照样显示，只是下次启动会再查一次）。
    pub state_persisted: bool,
}

impl UpdateCheckReport {
    /// 没有检查就给出的结果（读不到版本 / 被节流）。
    ///
    /// `checked_at_ms` 原样照抄状态里的值，**只有 [`persist_skip`] 写过状态之后**才是这次的状态
    /// —— 时间戳的语义（真正查过的时刻）不由跳过分支改变。
    fn skipped(
        state: &UpdateState,
        skip_reason: SkipReason,
        installed: Option<&InstalledVersion>,
        state_persisted: bool,
    ) -> Self {
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
            state_persisted,
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

/// 一次「下载」调用的回执。
///
/// **不含终局**：下载在后台跑，`downloading` / `ready` / `failed` 三个状态由
/// [`super::download::DOWNLOAD_EVENT`] 那条全局事件回给界面（§四）。这里的 `started` 只回答
/// "这一次真的开工了没有" —— `false` 表示同一时刻已经有一次下载在跑（不重复下同一个文件）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateDownloadReport {
    pub started: bool,
    pub version: String,
    /// 落盘位置（开工之前就能算出来，也是事件里 `path` 会指的地方）。
    pub destination: String,
}

/// 一次「安装」的结果：把哪一个文件交给了 Windows 的哪一种处理程序（§六）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateInstallReport {
    /// 交给 Windows 的那个文件（`.exe` 或 `.msix`）。
    pub path: String,
    /// 按后缀分派的结果：`exe` ⇒ 安装向导，`msix` ⇒ App Installer。
    pub kind: AssetKind,
    /// 接下来的那一步（[`InstallLaunch`]）：已经安排了"退出后再安装"，还是回落成了"现在就打开"。
    /// 界面据此说最后那句话 —— 应用在这一条命令里可能**不会再回话了**（它正在退出），所以这句话
    /// 必须由这一个字段决定，而不能靠界面猜。
    pub next_step: InstallLaunch,
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

/// 让"没有检查"这件事留下持久痕迹，并回给界面一条跳过原因。
///
/// 真机上踩过一次：跳过分支原先只写一行日志就返回，于是"检查从来没被触发过"与"跑了、命中了跳过
/// 分支"在证据上**完全一样**，而唯一能区分的那行日志会被截断 ⇒ 事后无法定性。现在两条跳过分支都
/// 落盘 `lastOutcome = Skipped` + `lastSkipReason = <原因>`。
///
/// **不动 `checkedAtMs`**：它代表"真正查过的时刻"，节流靠它。写它会让两种情况都坏掉 ——
/// "读不到本机版本"的那台机器会把这次跳过当成一次检查、从此永不重查；"被节流"那条则是把时间戳挪
/// 到不该挪的地方（它本来就有值，且正是把这次挡在门外的那个值）。失败原因也同理不动。
fn persist_skip(state: &mut UpdateState, state_path: &Path, skip_reason: SkipReason) -> bool {
    state.last_outcome = Some(CheckOutcome::Skipped);
    state.last_skip_reason = Some(skip_reason);
    match state::save(state_path, state) {
        Ok(()) => {
            log::info!(
                "更新检查：跳过（{skip_reason:?}），状态已落盘（{}）",
                state_path.display()
            );
            true
        }
        Err(error) => {
            // 写不进去要**说出来**：这条日志背后正是"这条跳过没有留下任何持久痕迹"。
            log::warn!(
                "更新状态写不进去（{}）：{error} —— 本次跳过（{skip_reason:?}）没有留在状态文件里",
                state_path.display()
            );
            false
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
///
/// 还有一处同样刻意：**跳过时不写** `checkedAtMs`（见 [`persist_skip`]），只写"为什么跳过"。
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

    // 第一段：两个不打网络的就地结论。两条都落盘（`persist_skip`）。
    let Some(installed) = installed else {
        // 读不到本机版本 ⇒ 不检查，而不是猜一个版本去比（§3.1、§八 9）。
        log::info!("更新检查：读不到本机版本，跳过本次检查");
        let persisted = persist_skip(&mut state, state_path, SkipReason::VersionUnavailable);
        return UpdateCheckReport::skipped(
            &state,
            SkipReason::VersionUnavailable,
            None,
            persisted,
        );
    };
    if !manual && !state::auto_check_due(state.checked_at_ms, now_ms) {
        // 6 小时节流（§五）。手动检查不受限（§四），所以这一支只在自动检查时进得来。
        log::info!("更新检查：距上次检查不足 6 小时，跳过本次（手动检查不受限）");
        let persisted = persist_skip(&mut state, state_path, SkipReason::Throttled);
        return UpdateCheckReport::skipped(
            &state,
            SkipReason::Throttled,
            Some(&installed),
            persisted,
        );
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
    // 真正查过之后就没有"为什么跳过"这回事了：留着它会让 `lastSkipReason` 与 `lastOutcome` 对不上
    // （后者已经不是 `Skipped`），下次看状态文件的人会把上一次跳过当成这次的解释。
    state.last_skip_reason = None;
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

/// 应用的本地数据目录（`%LOCALAPPDATA%\com.dsh.wallpaper`）：**Tauri 算的那个优先**。
///
/// 打包态它会被重定向到包容器里，与其它功能（桌面会话、桥的记录文件）同一处；拿不到时退回
/// `dirs` 的 `%LOCALAPPDATA%` 再补上标识符 —— 两条路拼出来的是同一个目录，退回不是"第二个位置"。
fn local_app_data_root(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .ok()
        .or_else(|| dirs::data_local_dir().map(|directory| directory.join(super::APP_IDENTIFIER)))
}

/// 更新目录：`<本地数据>\updates`，即 `%LOCALAPPDATA%\com.dsh.wallpaper\updates`（§五）。
///
/// **不再自己拼标识符**：Tauri 的 `app_local_data_dir()` 返回值末尾已经是标识符，0.4.6 及以前
/// 这里又拼了一次，于是目录变成 `…\com.dsh.wallpaper\com.dsh.wallpaper\updates`（真机上攒了
/// 4 个安装包、237 MB）。老位置的残留在第一次用到这个目录时由
/// [`reconcile_updates_directory`] 搬走。
///
/// 状态文件与下载下来的安装包都在这个目录下（`state.json` 与 `<版本>\<资产名>`，§六）。
fn updates_directory(app: &tauri::AppHandle) -> Option<PathBuf> {
    let local_app_data = local_app_data_root(app)?;
    let directory = state::updates_dir(&local_app_data);
    reconcile_at_most_once(&local_app_data, &directory, &running_version(app));
    Some(directory)
}

/// 启动时就把更新目录理顺（不等到第一次「检查更新」）。
///
/// 与 [`updates_directory`] 走同一个"只做一次"的闸门：谁先到谁做，另一处是空转。这样"用户
/// 什么都没按"的一次启动也会把旧目录收干净。失败只记日志（见 [`reconcile_updates_directory`]）。
pub(crate) fn reconcile_updates_at_startup(app: &tauri::AppHandle) {
    let Some(local_app_data) = local_app_data_root(app) else {
        log::warn!("更新目录：拿不到本地数据目录，这一次不做迁移与清理");
        return;
    };
    let directory = state::updates_dir(&local_app_data);
    reconcile_at_most_once(&local_app_data, &directory, &running_version(app));
}

/// 正在跑的这一版的版本号（原样给字符串；解析不出来时清理那一步会自己保守起来）。
fn running_version(app: &tauri::AppHandle) -> String {
    use tauri::Manager;
    app.package_info().version.to_string()
}

/// "顺手清理"每个进程只做一次：迁移旧目录 + 清掉旧版本的安装包。重复做没有意义，
/// 而 `Once` 让两处入口（启动、第一次用到更新目录）不会互相踩。
fn reconcile_at_most_once(local_app_data: &Path, target: &Path, running: &str) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| reconcile_updates_directory(local_app_data, target, running));
}

/// 更新目录的一次性整理：把 0.4.6 写坏的旧目录搬回来、把状态里那条路径跟着改、再清掉旧版本的
/// 安装包（每版约 59 MB）。
///
/// **不返回 `Result`**：这是"顺手清理"而不是一条功能。旧目录不存在、某一个文件被占着、状态文件
/// 写不回去 —— 都只记日志。用户按下「检查更新」或「安装」时，卡在一个删不掉的文件上比什么都不做
/// 更没道理。
fn reconcile_updates_directory(local_app_data: &Path, target: &Path, running: &str) {
    let legacy = state::legacy_updates_dir(local_app_data);
    if legacy.is_dir() {
        log::info!("更新目录：发现 0.4.6 及以前写坏的嵌套目录（{}）", legacy.display());
    }
    state::migrate_legacy_updates(&legacy, target);

    // 迁移把文件搬到了新位置，可状态里记的还是旧绝对路径 —— 先把它改对，再决定删什么：
    // "状态里记着的那一枚"必须进保留名单，否则清理会当场删掉用户刚下好的安装包。
    let state_path = target.join(state::STATE_FILE_NAME);
    let recorded = state::load(&state_path);
    let current = match state::repoint_downloaded_path(&recorded, &legacy, target) {
        Some(updated) => {
            if let Err(error) = state::save(&state_path, &updated) {
                log::warn!("更新目录：状态里那条安装包路径改不回去（{error}）");
            }
            updated
        }
        None => recorded,
    };

    let removed = state::prune_downloaded_installers(target, &versions_to_keep(running, &current));
    if removed > 0 {
        log::info!("更新目录：清掉 {removed} 个旧版本的安装包目录");
    }
}

/// 清理时要留下的版本（纯函数）：**正在跑的这一版**，加上状态里记着的那一枚安装包所属的版本。
///
/// 两枚都留的理由不一样：当前版本那一枚是同一版本的安装包（"再装一次"用得上）；记着的那一枚
/// 是下一次要装的（删了就等于让用户白下 59 MB）。
///
/// 读不到当前版本（`package_info` 给了个解析不出的串）时返回的名单里只有记着的那一枚；
/// 一枚都没有时 [`state::stale_installer_dirs`] 会自己停下来什么也不删。
fn versions_to_keep(running: &str, state: &UpdateState) -> Vec<Version> {
    let mut keep = Vec::new();
    if let Some(version) = Version::parse(running) {
        keep.push(version);
    }
    let recorded = state.downloaded_path.as_deref().and_then(|path| {
        Path::new(path)
            .parent()?
            .file_name()?
            .to_str()
            .and_then(Version::parse)
    });
    if let Some(version) = recorded {
        if !keep.contains(&version) {
            keep.push(version);
        }
    }
    keep
}

/// 状态文件的位置：`<本地数据>\updates\state.json`（§五）。
fn state_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    Some(updates_directory(app)?.join(state::STATE_FILE_NAME))
}

/// 下载开工前的**纯**准备：版本可比较、资产名能当文件名、目录拼得出来、地址在发布仓库那一族里。
///
/// 这四件事都在发起下载之前做完，于是"点到就报错"的那些情况（版本串被改过、地址不是 GitHub）
/// 走的是命令的返回值，而不是一条 `failed` 事件 —— 界面那时还没进 `downloading`。
fn prepare_download(
    updates_dir: &Path,
    version: &str,
    request: &DownloadRequest,
) -> Result<PathBuf, UpdateCommandError> {
    let Some(destination) = download::destination(updates_dir, version, &request.name) else {
        log::warn!("更新下载：版本或资产名拼不出安全路径（版本 {version:?}，资产 {:?}）", request.name);
        return Err(UpdateCommandError::destination_unavailable());
    };
    if !download::trusted_asset_url(&request.download_url) {
        log::warn!("更新下载：地址不在发布仓库那一族里（{}）", request.download_url);
        return Err(UpdateCommandError::untrusted_asset_url());
    }
    Ok(destination)
}

/// 「安装」的两道判断（纯）：文件还在不在、后缀在不在白名单里。
///
/// 白名单用的就是资产选择那一个（[`release::AssetKind::from_name`]）：两处各写一份的话，"选中的"
/// 与"能装的"迟早会不是同一批文件。
fn install_kind(path: &Path) -> Result<AssetKind, UpdateCommandError> {
    if !path.is_file() {
        log::warn!("更新安装：记录里的文件已经不在了（{}）", path.display());
        return Err(UpdateCommandError::installer_missing());
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(UpdateCommandError::unsupported_asset());
    };
    download::install_kind(name).ok_or_else(|| {
        log::warn!("更新安装：后缀不在白名单里（{name}）");
        UpdateCommandError::unsupported_asset()
    })
}

/// 把文件交给 Windows 的默认处理程序（`ShellExecuteW("open", …)`，§六）。
///
/// 与 `external_link::open` 是同一个动作的两个出处：那一条开的是地址（因此要校验协议），这一条开
/// 的是一个**已经由我们自己算出来的路径**（`state.json` 里的 `downloadedPath`，后缀也过过白名单）。
#[cfg(windows)]
fn shell_open(path: &Path) -> Result<(), UpdateCommandError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = "open".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    // 路径走宽字符而不是 `to_string_lossy()`：非 UTF-8 的用户名目录不该被问号替换掉。
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a value <= 32 on failure; only > 32 is success.
    if result.0 as usize <= 32 {
        log::warn!("更新安装：Windows 没有打开 {}（ShellExecuteW 返回 {}）", path.display(), result.0 as usize);
        return Err(UpdateCommandError::open_failed());
    }
    log::info!("更新安装：已交给 Windows（{}）", path.display());
    Ok(())
}

#[cfg(not(windows))]
fn shell_open(path: &Path) -> Result<(), UpdateCommandError> {
    log::warn!("更新安装：当前平台不支持把 {} 交给系统处理程序", path.display());
    Err(UpdateCommandError::open_failed())
}

/// 装好之前会短暂用一次的助手执行文件（两种写法都试，见 [`installer_launcher_argv`]）。
///
/// 用 `%SystemRoot%` 拼**绝对路径**是首选：`powershell.exe` 在 `PATH` 被收紧的进程里照样在。
/// 读不到那个变量时回落到名字本身，让系统按 `PATH` 找 —— 找不到就当"助手起不来"，走回落分支。
///
/// 这里不按平台门控：这是一段纯拼路径的计算，真正的"起不起来"由 [`spawn_install_launcher`] 那条
/// 分支决定（非 Windows 上它直接报 `openFailed`）。不门控的好处是命令行那两个纯函数在**任何平台**
/// 上都编得过、测得上 —— 它们的单测正是转义与编码那几件事的证据。
fn windows_powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| {
            PathBuf::from(root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        })
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// 助手要跑的脚本（纯函数，单测钉着它的每一处转义）。
///
/// 语义只有一条：**等本进程结束，再把安装包（带上 [`INSTALLER_ARGUMENTS`]）交给 Windows**。
/// 这里没有也不许有固定延时 ——"等一会儿"与"等它退出"是两回事，前者只是在赌。
///
/// 写法上的两处讲究（都在本机实测过）：
///
///  - **先取进程对象、再 `WaitForExit()`**：`Wait-Process -Id … -ErrorAction SilentlyContinue` 在
///    目标**已经不在**时会退化成"立刻返回"（实测：目标还活着也只花 13 ms），于是等待会静默消失；
///    `$p.WaitForExit()` 拿到的是那个进程对象本身，实测会等满目标的整个生命周期（5 s 的目标等了
///    5189 ms），而进程已经不在了时 `Get-Process` 给 `$null`，`if ($p)` 直接跳过（实测 213 ms）；
///  - **`Get-Process` 不重定向 `-ErrorAction`**：目标进程**权限比助手高**时它会给 `$null` 而不是抛错
///    （"已经不在"与"看不见"在这里是同一件事：都只能往下走），脚本照旧启动安装包。
///
/// 参数与路径都不过命令行解析这一关：路径走 `-FilePath` 的单引号字面量（`'` 翻倍），参数走
/// `-ArgumentList` 的数组（见 [`installer_arguments`]），整段脚本再按 UTF-16LE 走 `-EncodedCommand`。
///
/// 路径来源见 [`super::state::UpdateState::downloaded_path`]：是我们自己写下的记录，但仍按不可信输入
/// 处理 —— 这里只把 `'` 翻倍（PowerShell 单引号字符串里唯一的转义），因此**不存在**命令行引号、
/// 转义或编码问题。
fn installer_waiter_script(process_id: u32, installer: &Path) -> String {
    let quoted = installer.to_string_lossy().replace('\'', "''");
    format!(
        "$p = Get-Process -Id {process_id} -ErrorAction SilentlyContinue; \
         if ($p) {{ $p.WaitForExit() }}; \
         Start-Process -FilePath '{quoted}' -ArgumentList {}",
        installer_arguments()
    )
}

/// 安装器要带的参数。一枚一枚都对着模板里的一处行为（`src-tauri/nsis/installer.nsi`）：
///
///  - **`/P`（passive）**：欢迎页、目录页、开始菜单页、完成页都挂了 `SkipIfPassive`，而"已安装"
///    维护页在 `$PassiveMode = 1` 时**根本不建那个对话框**（它只跑自己的离开逻辑）—— 于是这一枚
///    把"维护页 + 选路径 + 向导"整片去掉，只剩一条进度。实测（探针安装器 + 每页留一个文件的
///    记录）：`/P` 下欢迎页 `Abort`、维护页进了函数但没有对话框、安装段执行、完成页 `Abort`、
///    进程自己退出（退出码 0）；
///  - **`/UPDATE`**：告诉安装器"这是升级"。两处作用：同版本重装时不再去跑旧卸载器
///    （`Function PageLeaveReinstall` 开头那一条 `$UpdateMode = 1` 分支），以及**不会顺手建
///    快捷方式**（完成页被跳过时，模板会给 passive/silent 安装补一个桌面快捷方式 ——
///    升级不该夹带这个）。升级时的装法不变：passive 下维护页取的是"不卸载"那条分支，
///    也就是原地覆盖，与今天界面上的默认一致；
///  - **`/R`**：装完自动把新版本起回来。passive 跳过了完成页，而"装完运行"那枚开关就长在完成页上
///    （`MUI_FINISHPAGE_RUN` → `RunMainBinary`），所以只能由 `/R` 补（`Function .onInstSuccess`）。
///    实测：只有 `/P` 时完成页的 run 函数一次都没被调用；`/P /R` 时它起来了。
///
/// 不做 `/S`（完全静默）：那一枚会把进度窗口也去掉 —— 用户按下「点击安装」之后，应用消失、
/// 60 MB 的安装与旧卸载都发生在什么都没有的屏幕后面，出事时（例如安装器弹出错误框）也少一层
/// 可读的过程；`/P` 同样没有要点的页，却保留进度与细节。
const INSTALLER_ARGUMENTS: [&str; 3] = ["/P", "/UPDATE", "/R"];

/// `-ArgumentList` 的那一截（纯函数）：每枚参数各自是一个 PowerShell 单引号字面量，逗号分隔。
///
/// 写成**数组**（而不是一整个字符串）是这一处唯一容易写错的地方：`Start-Process -ArgumentList`
/// 收数组时由 PowerShell 按 Windows 的规则拼命令行，参数里的空格、引号都不用自己操心 ——
/// 而漏掉 `-ArgumentList` 的话参数根本不会跟着 exe 走（这一条正是被单测钉住的）。
fn installer_arguments() -> String {
    INSTALLER_ARGUMENTS
        .iter()
        .map(|flag| format!("'{flag}'"))
        .collect::<Vec<String>>()
        .join(",")
}

/// 把一个脚本包成 `-EncodedCommand` 认的那段 Base64（UTF-16LE）。
fn encode_powershell(script: &str) -> String {
    use base64::Engine;

    let mut utf16 = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// 起助手的完整命令行：要跑哪个执行文件、按哪几个参数（纯函数）。
///
/// 三个"别这么写"，每一个都在本机实测过：
///
///  - **不带 `-WindowStyle Hidden`**：它和 `-EncodedCommand` 一起用时 PowerShell 会在**十几毫秒**内
///    直接退出（实测 14 ms），脚本一行都不跑 —— 而且没有任何输出，看起来就像"助手起来了"。
///    不留黑框靠的是 `CREATE_NO_WINDOW`（见 [`spawn_install_launcher`]），那是**进程级**的事，
///    不该交给 PowerShell 的窗口样式参数；
///  - **`-NoProfile` 要留着**：用户的 profile 可能改掉 `Get-Process` / `Start-Process` 的行为，
///    也可能弹提示、写输出、拖慢启动；
///  - **`-EncodedCommand` 与它那段 Base64 分成两项**：写成一项（`"-EncodedCommand AAAA…"`）时
///    PowerShell 会把它们当成一个整体去匹配开关名，同样是不跑脚本。
///
/// 参数分开给也意味着 `Command::args` 会按 Windows 的 `CommandLineToArgvW` 规则自己加引号：
/// 路径里的空格、中文、括号都不会漏出去。
fn installer_launcher_argv(process_id: u32, installer: &Path) -> (PathBuf, [String; 3]) {
    let encoded = encode_powershell(&installer_waiter_script(process_id, installer));
    (
        windows_powershell(),
        [
            "-NoProfile".to_string(),
            "-EncodedCommand".to_string(),
            // 必须是最后一项：它后面没有别的开关，PowerShell 把这一段当脚本正文。
            encoded,
        ],
    )
}

/// 起一个**比我们活得久**的助手：它等本进程结束，再把安装包交给 Windows（[`InstallLaunch`]）。
///
/// 这一层是注入点（`Box<dyn Fn>`）：单测里换成"只数调用次数"的假助手，绝不在单测里真的起进程。
pub(crate) type InstallLauncher = Box<dyn Fn(u32, &Path) -> Result<(), UpdateCommandError>>;

/// 回落那一步（`ShellExecuteW("open", …)`）同样是注入点：单测里换成假的，不真的打开安装包。
pub(crate) type InstallOpener = Box<dyn Fn(&Path) -> Result<(), UpdateCommandError>>;

/// 默认助手：`powershell.exe` 分离启动（`CREATE_NO_WINDOW`，见下）。
///
/// 为什么是 PowerShell 而不是 `cmd.exe /c`：
///
///  - `$p.WaitForExit()` 等的是**进程结束**这个语义本身，不是一段猜出来的延时；
///  - 等待与启动各是一句话，不需要在 `cmd` 里再拼一层引号（`start "" "…"` 那套转义又长又容易错）；
///  - 整段脚本走 `-EncodedCommand`（UTF-16LE + Base64），路径里的空格、中文、引号都不过命令行
///    解析这一关，`SHIFT-JIS`/代码页一类的编码坑也不存在。
///
/// `CREATE_NO_WINDOW` 是"不留黑框"的那一半：助手是控制台程序，不拦的话用户会看到一个黑色的
/// 控制台窗口。**只剩这一半**是对的：`-WindowStyle Hidden` 那条路会把脚本本身弄没（见
/// [`installer_launcher_argv`] 的第一条）。启动它的进程**不阻塞我们**，也不回收它 ——
/// 它比我们活得久正是这件事的全部意义。
#[cfg(all(not(feature = "lite"), windows))]
fn spawn_install_launcher(process_id: u32, installer: &Path) -> Result<(), UpdateCommandError> {
    use std::os::windows::process::CommandExt;

    /// `CREATE_NO_WINDOW`（winbase.h）：给控制台程序用，不建控制台窗口。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let (program, arguments) = installer_launcher_argv(process_id, installer);
    let mut command = std::process::Command::new(&program);
    command.args(arguments).creation_flags(CREATE_NO_WINDOW);
    match command.spawn() {
        Ok(_child) => {
            // `_child` 被丢在这里是**故意的**：`std::process::Child` 的 Drop 不杀进程，句柄一关
            // 助手就独立了。等它、杀它都会让"等我们退出"这件事变成我们自己的负担。
            log::info!(
                "更新安装：已安排退出后安装（助手 {}，本进程 {}，安装包 {}）",
                program.display(),
                process_id,
                installer.display()
            );
            Ok(())
        }
        Err(error) => {
            log::warn!(
                "更新安装：{} 起不来（{error}），改回直接打开安装包",
                program.display()
            );
            Err(UpdateCommandError::open_failed())
        }
    }
}

/// 非 Windows：没有"等本进程退出再启动"的这套东西（`.exe`/`.msix` 本来就只属于 Windows）。
#[cfg(not(windows))]
fn spawn_install_launcher(process_id: u32, installer: &Path) -> Result<(), UpdateCommandError> {
    log::warn!(
        "更新安装：当前平台没有先退出再安装这条路（助手未启动，进程 {process_id}，安装包 {}）",
        installer.display()
    );
    Err(UpdateCommandError::open_failed())
}

/// 这一台机器上**真实**的两个外部动作（生产用这一份；单测注入假的，绝不真的起进程）。
struct InstallRunner {
    /// 起助手：它等本进程结束后再启动安装包。
    launcher: InstallLauncher,
    /// 回落那一步：现在就 `ShellExecuteW("open", …)`。
    opener: InstallOpener,
}

fn system_install_runner() -> InstallRunner {
    InstallRunner {
        launcher: Box::new(spawn_install_launcher),
        opener: Box::new(shell_open),
    }
}

/// 安装这一步的两种走法（见 [`UpdateInstallReport::next_step`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum InstallLaunch {
    /// 助手已经起来：它在等本进程退出，退出之后由它启动安装包 ⇒ 应用**接着就退出**。
    Exiting,
    /// 助手起不来，已经回落到"现在就交给 Windows"：应用**继续运行**。
    Opened,
}

/// 走完"安装"这一步：先起助手，起不来才回落。
///
/// 两件事分开，是因为"起没起来"决定的是**用户接下来看到什么**（应用退出 / 应用留着），
/// 而不是成败：回落分支照样把安装包交出去了，只是没有"先退出"那一步。回落**不静默** ——
/// 报告里的 `next_step` 就是界面要说的那句话的依据（`update.notice.install-fallback-opened`）。
fn launch_installer(
    process_id: u32,
    path: &Path,
    kind: AssetKind,
    runner: &InstallRunner,
) -> Result<UpdateInstallReport, UpdateCommandError> {
    let next_step = match (runner.launcher)(process_id, path) {
        Ok(()) => InstallLaunch::Exiting,
        Err(error) => {
            // 只有 **openFailed** 是"助手起不来"（见 `spawn_install_launcher`）。别的码说明这一步
            // 之前就已经坏了（路径守卫那几条），那时候再打开一次安装包只会把原因盖掉。
            if error.code != "openFailed" {
                return Err(error);
            }
            log::warn!("更新安装：助手起不来，回落到直接打开安装包（应用不退出）");
            (runner.opener)(path)?;
            InstallLaunch::Opened
        }
    };
    Ok(UpdateInstallReport {
        path: path.display().to_string(),
        kind,
        next_step,
    })
}

/// 把已经下载好的安装包交给 Windows（§六）。
///
/// 路径**不由界面给**：它读状态文件里那一条（`downloadedPath`）。界面因此无法让原生去打开任意一个
/// 文件 —— 那会是一条"打开任何可执行文件"的命令，比 `open_external_link` 更宽。
///
/// 顺序是"先安排、后退出"：助手起不来时**不退出**（那时候安装包走的是"现在就打开"那条回落），
/// 所以 [`InstallLaunch::Opened`] 这个结论本身就带着"应用不许退出"的意思。
pub(crate) fn run_install(state_path: &Path) -> Result<UpdateInstallReport, UpdateCommandError> {
    run_install_with(state_path, std::process::id(), &system_install_runner())
}

/// [`run_install`] 的注入版：进程号与两个外部动作都由调用者给（单测里从不真的起进程）。
fn run_install_with(
    state_path: &Path,
    process_id: u32,
    runner: &InstallRunner,
) -> Result<UpdateInstallReport, UpdateCommandError> {
    let recorded = state::load(state_path).downloaded_path;
    let Some(recorded) = recorded.filter(|path| !path.trim().is_empty()) else {
        log::info!("更新安装：状态文件里没有下过的安装包");
        return Err(UpdateCommandError::nothing_downloaded());
    };
    let path = PathBuf::from(recorded);
    let kind = install_kind(&path)?;
    launch_installer(process_id, &path, kind, runner)
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

/// 「下载」：流式下载选中的那一个资产，进度与终局都走 [`super::download::DOWNLOAD_EVENT`]。
///
/// **只在这条命令被调用时才下载**（§六）：界面按下「下载」那一次。检查（`update_check`）这条路
/// 上没有任何下载调用，所以"打开壁纸就自动下 30 MB"这件事在代码里不存在。
///
/// `version` 与 `asset` 由界面递回来 —— 就是检查报告里的 `latestVersion` 与 `asset`（资产选择在
/// `run_check` 里已经做完了，这里不再选一遍）。两个都当**不可信输入**处理：版本要能解析、资产名要
/// 能当文件名、地址要在发布仓库那一族里，不合格当场拒绝，而不是硬着头皮去下。
///
/// 下载在后台跑（`tauri::async_runtime::spawn`），所以命令很快返回：`started` 说"这一次真的开工了
/// 没有"，终局由事件回。同一时刻只允许一次下载（见 [`super::download::InFlight`]）。
#[tauri::command]
// 整个模块已经在 `lib.rs` 里按 edition 门控；这一行是仓库惯例的第二道（Lite 不含本功能）。
#[cfg(not(feature = "lite"))]
pub(crate) async fn update_download(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    version: String,
    asset: DownloadRequest,
) -> Result<UpdateDownloadReport, UpdateCommandError> {
    use tauri::Emitter;

    require_update_surface(&caller)?;
    let Some(updates_dir) = updates_directory(&app) else {
        return Err(UpdateCommandError::state_path_unavailable());
    };
    let destination = prepare_download(&updates_dir, &version, &asset)?;
    let reported_version = Version::parse(&version)
        .map(|version| version.to_string())
        .unwrap_or_else(|| version.clone());
    let report = UpdateDownloadReport {
        started: false,
        version: reported_version.clone(),
        destination: destination.display().to_string(),
    };
    let Some(claim) = download::InFlight::claim(&reported_version) else {
        log::info!("更新下载：已经有一次下载在跑，这一次不重复下（{reported_version}）");
        return Ok(report);
    };

    let state_path = updates_dir.join(state::STATE_FILE_NAME);
    let version_for_task = reported_version.clone();
    tauri::async_runtime::spawn(async move {
        // 凭据跟着任务走：任务结束（含失败）就释放，下一次下载才能开工。
        let _claim = claim;
        let terminal = download::fetch(
            &version_for_task,
            &asset,
            &destination,
            &state_path,
            |event: &DownloadEvent| {
                // 一条全局事件：两个窗口都收得到（壁纸气泡与设置卡片看的是同一份进度）。
                if let Err(error) = app.emit(download::DOWNLOAD_EVENT, event) {
                    log::warn!("更新下载：进度事件发不出去：{error}");
                }
            },
        )
        .await;
        log::info!(
            "更新下载：{version_for_task} 结束（{:?}，{} 字节）",
            terminal.phase,
            terminal.downloaded_bytes
        );
    });

    Ok(UpdateDownloadReport {
        started: true,
        ..report
    })
}

/// 「安装」：**先安排"我们退出之后再启动安装包"，然后退出**；安排不了才回落到直接打开（§六）。
///
/// `.exe`（NSIS setup）⇒ 安装器；`.msix` ⇒ App Installer。两条分支都留着 —— MSIX 是将来上商店
/// 的路，不许因为今天只发 `.exe` 就删掉。两条走的是同一个助手。
///
/// **为什么必须先退出**：Tauri 的 NSIS 安装包在检测到已安装时先跑旧版卸载器（带 `/S`，本意是静默），
/// 而安装器换不掉一个还在运行的 `dsh-wallpaper.exe`；那时它会把卸载器弹成可见窗口 —— 用户看到的
/// "先一个 uninstaller、再一个 installer"就是这么来的。应用自己先退出，这一步就干净了。
///
/// **为什么是升级而不是一次向导**：`/UPDATE` 意味着原地覆盖（维护页在 passive 下取的是"不卸载"
/// 那条分支），所以"旧版本还跑着"这件事是这条路的前提，而不是可以省掉的一步。
///
/// 退出走的是 [`tauri::AppHandle::exit`]（`0`）：它走 `RunEvent::ExitRequested` → `Exit`，所以
/// `lib.rs` 里那个 `shutdown_native_state` 照常把桌面图标、原生首帧窗口、DSH 实例与清理标记都收好
/// —— 与托盘「退出」是同一条路，不是硬杀。**回落分支不退出**：那时候安装包是"现在就打开"的，
/// 我们退出只会让用户看到安装器去抢一个刚被占用的文件（退回原来的两步）。
///
/// 返回值只有路径、按后缀分派的结果与接下来的那一步（见 [`UpdateInstallReport`]）；开不起来给的是码
/// （`nothingDownloaded` / `installerMissing` / `unsupportedAsset` / `openFailed`）。
#[tauri::command]
// 整个模块已经在 `lib.rs` 里按 edition 门控；这一行是仓库惯例的第二道（Lite 不含本功能）。
#[cfg(not(feature = "lite"))]
pub(crate) fn update_install(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<UpdateInstallReport, UpdateCommandError> {
    require_update_surface(&caller)?;
    let Some(state_path) = state_path(&app) else {
        return Err(UpdateCommandError::state_path_unavailable());
    };
    // 读状态 + 起一个助手，都不打网络也不占阻塞线程。
    let report = run_install(&state_path)?;
    if report.next_step == InstallLaunch::Exiting {
        // 顺序是"先起助手、后退出"：反过来的话助手可能来不及起来，安装包就没人启动了。
        log::info!("更新安装：助手已就位，本进程退出（{}）", report.path);
        app.exit(0);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

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

    /// 假的一条注入链：`launcher` 记 `(进程号, 安装包路径)`，`opener` 记回落那一步收到的路径。
    ///
    /// 记账本用 `Rc<Cell<..>>` 而不是 `&Cell<..>`：装进 [`InstallRunner`] 的是一个 `'static` 闭包，
    /// 拿着引用的闭包过不了这道门槛。`Box<dyn Fn>` 也让"助手报什么错"能被注入进来 —— 回落分支
    /// 就是这样被覆盖到的，而不是真去起一个起不来的进程。
    fn counting_runner(
        launcher: Result<(), UpdateCommandError>,
    ) -> (InstallRunner, Rc<LauncherCalls>, Rc<OpenerCalls>) {
        counting_runner_with(launcher, Ok(()))
    }

    fn counting_runner_with(
        launcher: Result<(), UpdateCommandError>,
        opener: Result<(), UpdateCommandError>,
    ) -> (InstallRunner, Rc<LauncherCalls>, Rc<OpenerCalls>) {
        let launches = Rc::new(LauncherCalls::default());
        let opens = Rc::new(OpenerCalls::default());
        let runner = InstallRunner {
            launcher: fake_launcher(launcher, Rc::clone(&launches)),
            opener: fake_opener(opener, Rc::clone(&opens)),
        };
        (runner, launches, opens)
    }

    /// 一次"起助手"的调用记录。
    #[derive(Default)]
    struct LauncherCalls {
        calls: Cell<usize>,
        arguments: RefCell<(u32, String)>,
    }

    /// 一次"打开安装包"（回落）的调用记录。
    #[derive(Default)]
    struct OpenerCalls {
        calls: Cell<usize>,
        arguments: RefCell<String>,
    }

    /// 假的"起助手"：只数调用次数、记下收到的那两样东西，**绝不真的起进程**。
    fn fake_launcher(
        answer: Result<(), UpdateCommandError>,
        calls: Rc<LauncherCalls>,
    ) -> InstallLauncher {
        let answer = Rc::new(answer);
        Box::new(move |process_id, installer| {
            calls.calls.set(calls.calls.get() + 1);
            *calls.arguments.borrow_mut() = (process_id, installer.display().to_string());
            match &*answer {
                Ok(()) => Ok(()),
                Err(error) => Err(error.clone()),
            }
        })
    }

    /// 假的"打开安装包"（回落那一步）：同样是记账，不打开任何东西。
    fn fake_opener(
        answer: Result<(), UpdateCommandError>,
        calls: Rc<OpenerCalls>,
    ) -> InstallOpener {
        let answer = Rc::new(answer);
        Box::new(move |installer| {
            calls.calls.set(calls.calls.get() + 1);
            *calls.arguments.borrow_mut() = installer.display().to_string();
            match &*answer {
                Ok(()) => Ok(()),
                Err(error) => Err(error.clone()),
            }
        })
    }

    /// 状态文件里记上这一个安装包 —— 「安装」那一步读的**只有**这里。
    fn install_runner(directory: &tempfile::TempDir, installer: &Path) -> PathBuf {
        let path = state_path(directory);
        state::save(
            &path,
            &UpdateState {
                downloaded_path: Some(installer.display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        path
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
        // 别的字段一个都没被改动过（节流与"忽略"都读这份状态），只多了"这次为什么跳过"。
        assert_eq!(
            state::load(&path),
            UpdateState {
                last_outcome: Some(CheckOutcome::Skipped),
                last_skip_reason: Some(SkipReason::VersionUnavailable),
                ..Default::default()
            }
        );
    }

    /// 跳过也要留痕：真机上"没被触发过"与"跑了但跳过"曾经不可区分，因为唯一的区别是会被截断的
    /// 日志。这次跳过必须能从状态文件里读出来，**同时**不许写 `checkedAtMs` —— 写了它，这台读不到
    /// 版本的机器会把跳过当成一次检查，从此永远不再重查（§3.1 的自愈前提）。
    #[tokio::test]
    async fn a_skipped_check_still_leaves_its_reason_in_the_state_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let fetch = Fetch::ok(&published_release());

        let report = run_check(None, &path, NOW, false, || fetch.fetch()).await;

        assert_eq!(report.outcome, CheckOutcome::Skipped);
        assert!(report.state_persisted, "跳过也要落盘");
        assert!(path.exists(), "跳过分支必须写出状态文件");
        assert!(!path.with_extension("json.tmp").exists(), "半截文件不留盘");

        let stored = state::load(&path);
        assert_eq!(stored.last_outcome, Some(CheckOutcome::Skipped));
        assert_eq!(
            stored.last_skip_reason,
            Some(SkipReason::VersionUnavailable)
        );
        assert_eq!(stored.checked_at_ms, None, "没查过就是没查过");
    }

    /// 状态文件里的形状也是契约：键名 camelCase，值是指出的那几个码。
    #[tokio::test]
    async fn a_skipped_state_file_records_the_reason_by_code() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        state::save(
            &path,
            &UpdateState {
                checked_at_ms: Some(NOW - 60_000),
                ..Default::default()
            },
        )
        .unwrap();
        let fetch = Fetch::ok(&published_release());

        let report = run_check(
            installed("0.4.0", VersionSource::UninstallEntry),
            &path,
            NOW,
            false,
            || fetch.fetch(),
        )
        .await;

        assert_eq!(report.skip_reason, Some(SkipReason::Throttled));
        assert_eq!(fetch.calls.get(), 0, "被节流的那次一个网络都不打");
        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["lastOutcome"], serde_json::json!("skipped"));
        assert_eq!(value["lastSkipReason"], serde_json::json!("throttled"));
        // 跳过不是失败：报告里没有 `failure`，状态里也不许冒出上一次的失败原因。
        assert_eq!(value["lastFailure"], serde_json::json!(null));
        // 节流靠这个字段，跳过不许动它：还是原来那个"上次真正查过"的时刻。
        assert_eq!(value["checkedAtMs"], serde_json::json!(NOW - 60_000));
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
        // 这次跳过也留了痕（见 `a_skipped_check_still_leaves_its_reason_in_the_state_file`）。
        assert_eq!(state::load(&path).last_skip_reason, Some(SkipReason::Throttled));

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
        // 真正查过之后，"为什么跳过"就被清掉了：留下的 `Skipped` 解释不该跟着一次成功的检查。
        assert_eq!(state::load(&path).last_skip_reason, None);
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
    /// 真机：把**系统的版本探针**与**真实的网络**接在一起跑一次完整判定 —— 生产路径上
    /// `update_check` 做的就是这件事，这条测试只是把输入换成真机、输出写到临时文件里。
    /// 默认 ignore（要网络、要读本机安装）；手动跑：
    /// `cargo test --lib -- --ignored this_machine_checking --nocapture`
    #[tokio::test]
    #[ignore = "hits the network and reads the local install"]
    async fn this_machine_checking_against_the_real_release_concludes_something() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        let installed = crate::update::version::installed_version(
            &crate::update::version::VersionProbes::system(),
        );
        println!("  本机版本: {installed:?}");
        let report = run_check(
            installed,
            &path,
            1_700_000_000_000,
            true,
            crate::update::source::fetch_latest_release,
        )
        .await;
        println!("  结论: {:?}", report.outcome);
        println!("  最新: {:?}", report.latest_version);
        println!("  失败: {:?}", report.failure);
        println!("  状态文件: {}", std::fs::read_to_string(&path).unwrap_or_default());
        assert_ne!(report.outcome, CheckOutcome::Failed, "真机 + 真实网络不该失败");
        assert!(report.latest_version.is_some(), "应该读到最新版本号");
    }

    /// 真机：真的向 GitHub 发一次请求，验证"请求 + 解析"这段胶水（单测里它被注入的取数入口替掉了）。
    /// 默认 ignore（CI 不该依赖网络）；手动跑：
    /// `cargo test --lib -- --ignored a_real_fetch_from_github --nocapture`
    #[tokio::test]
    #[ignore = "hits the network; run on purpose"]
    async fn a_real_fetch_from_github_parses_into_a_release() {
        let json = super::super::source::fetch_latest_release()
            .await
            .expect("要能连上 GitHub 并拿到正文");
        let info = super::super::release::parse_release(&json).expect("真实的 release 要被解析出来");
        println!("  latest tag: {:?}", info.tag);
        println!("  page: {:?}", info.page_url);
        println!(
            "  assets: {:?}",
            info.assets.iter().map(|a| (&a.name, a.size)).collect::<Vec<_>>()
        );
        assert!(info.tag.is_some(), "真实的 release 一定有标签");
        assert!(!info.assets.is_empty(), "真实的 release 一定有资产");
    }

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
        /// (命令名, 授权标识符)。`update_check` 是"查"，`update_dismiss` 是「忽略」，
        /// `update_download` 是「下载」，`update_install` 是「点击安装」—— 后两个是第三片加的。
        const COMMANDS: [(&str, &str); 4] = [
            ("update_check", "allow-update-check"),
            ("update_dismiss", "allow-update-dismiss"),
            ("update_download", "allow-update-download"),
            ("update_install", "allow-update-install"),
        ];

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

    /// §六 那条"绝不在检查时自动下载"：检查这条路（`run_check` 与 `update_check`）里不许出现任何
    /// 下载调用。
    ///
    /// 这条靠读源码来钉（和上面那条 ACL 表一样）：它是"这条路上没有这段代码"的声明，而单测没法
    /// 用行为证明"某件事没有发生"。按函数切段检查，而不是全文搜索 —— 全文里当然有 `download`。
    #[test]
    fn checking_never_downloads_anything() {
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/update/commands.rs"),
        )
        .expect("this file must be readable");

        for function in ["run_check", "update_check"] {
            let body = function_body(&source, function)
                .unwrap_or_else(|| panic!("{function} 必须还能被找到（这道测试靠名字切段）"));
            // 四个标记分别对应下载那条路上的四样东西：模块、命令、请求体、状态字段。
            for forbidden in ["download::", "update_download", "DownloadRequest", "downloaded_path"] {
                assert!(
                    !body.contains(forbidden),
                    "{function} 里不许出现 {forbidden}：下载只由按下「下载」那一次发起（§六）"
                );
            }
        }
    }

    /// 从源码里切出一个函数的正文（从 `fn <名字>` 到下一个顶格 `}`）。
    ///
    /// 够用就行：这个文件里的函数都是顶格写的，注释也在同一个缩进层内。
    fn function_body(source: &str, name: &str) -> Option<String> {
        let start = source.find(&format!("fn {name}"))?;
        let rest = &source[start..];
        // 函数体结束的标志：一行只有 `}` 的行（`insert` 之后剩下的部分从那里开始）。
        let mut offset = 0usize;
        let mut end = None;
        for line in rest.split_inclusive('\n') {
            offset += line.len();
            if line.trim_end().ends_with('}') && !line.starts_with(' ') && offset > 1 {
                end = Some(offset);
                break;
            }
        }
        Some(rest[..end.unwrap_or(rest.len())].to_string())
    }

    /// 下载开工前的那四道纯判断（版本、文件名、地址）。
    #[test]
    fn a_download_that_cannot_be_trusted_is_refused_before_it_starts() {
        let directory = tempfile::tempdir().unwrap();
        let updates = directory.path();
        let good = DownloadRequest {
            name: "dsh-wallpaper_0.4.2_x64-setup.exe".into(),
            download_url: "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/setup.exe".into(),
            size: 10,
            digest: None,
        };
        assert_eq!(
            prepare_download(updates, "0.4.2", &good).unwrap(),
            updates.join("0.4.2").join("dsh-wallpaper_0.4.2_x64-setup.exe")
        );
        // 版本写法不唯一：目录名用规范写法（`v0.4.2` 与 `0.4.2` 是同一个版本）。
        assert_eq!(
            prepare_download(updates, "v0.4.2", &good).unwrap(),
            updates.join("0.4.2").join("dsh-wallpaper_0.4.2_x64-setup.exe")
        );

        // 认不出的版本：拒绝，而不是把版本串拼进路径。
        for version in ["", "latest", "0.4.2/../../evil"] {
            let error = prepare_download(updates, version, &good).expect_err("must be refused");
            assert_eq!(error.code, "destinationUnavailable", "{version:?}");
        }
        // 认不出的资产名（连文件名都当不了）同理。
        let bad_name = DownloadRequest {
            name: "../evil.exe".into(),
            ..good.clone()
        };
        assert_eq!(
            prepare_download(updates, "0.4.2", &bad_name).unwrap_err().code,
            "destinationUnavailable"
        );
        // 地址不在发布仓库那一族里：拒绝（这一步取回的是**可执行文件**）。
        for url in ["http://github.com/a.exe", "https://evil.test/a.exe", "https://github.com.evil.test/a.exe"] {
            let bad_url = DownloadRequest {
                download_url: url.into(),
                ..good.clone()
            };
            assert_eq!(
                prepare_download(updates, "0.4.2", &bad_url).unwrap_err().code,
                "untrustedAssetUrl",
                "{url}"
            );
        }
    }

    /// 安装的两道判断：文件在不在、后缀在不在白名单里（白名单与资产选择是同一份）。
    #[test]
    fn installing_checks_the_file_and_its_suffix_before_handing_it_over() {
        let directory = tempfile::tempdir().unwrap();

        // 不存在的文件：说"它已经不在了"，而不是去试一次必然失败的打开。
        assert_eq!(
            install_kind(&directory.path().join("gone.exe")).unwrap_err().code,
            "installerMissing"
        );

        // 在的文件：按后缀分派（`.exe` ⇒ 安装向导，`.msix` ⇒ App Installer）。
        let exe = directory.path().join("dsh-wallpaper_0.4.2_x64-setup.exe");
        std::fs::write(&exe, b"installer").unwrap();
        assert_eq!(install_kind(&exe).unwrap(), AssetKind::Exe);
        let msix = directory.path().join("dsh-wallpaper_0.4.2.msix");
        std::fs::write(&msix, b"package").unwrap();
        assert_eq!(install_kind(&msix).unwrap(), AssetKind::Msix);
        // 大小写不敏感（Windows 上文件名就是不区分大小写）。
        let shouted = directory.path().join("setup.EXE");
        std::fs::write(&shouted, b"installer").unwrap();
        assert_eq!(install_kind(&shouted).unwrap(), AssetKind::Exe);

        // 后缀在白名单外：拒绝，绝不硬着头皮交给 shell。
        for name in ["SHA256SUMS.txt", "setup.exe.sig", "package.msixbundle", "notes.zip"] {
            let path = directory.path().join(name);
            std::fs::write(&path, b"x").unwrap();
            assert_eq!(install_kind(&path).unwrap_err().code, "unsupportedAsset", "{name}");
        }
    }

    /// 「安装」读的是**状态文件**里的那一条：没下过、或记录里的文件被删了，各有各的码。
    #[test]
    fn installing_uses_the_recorded_download_and_says_what_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let path = state_path(&directory);
        // 这一条只走"路径守卫"那几道判断：两道守卫都过不去时**一个助手都不该起**。
        let (runner, launches, _opens) = counting_runner(Ok(()));

        // 还没下过任何东西。
        assert_eq!(
            run_install_with(&path, 4242, &runner).unwrap_err().code,
            "nothingDownloaded"
        );

        // 记过一条，但文件已经不在了。
        state::save(
            &path,
            &UpdateState {
                downloaded_path: Some(directory.path().join("gone.exe").display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            run_install_with(&path, 4242, &runner).unwrap_err().code,
            "installerMissing"
        );

        // 记录里的东西不是可安装资产。
        let text = directory.path().join("readme.txt");
        std::fs::write(&text, b"x").unwrap();
        state::save(
            &path,
            &UpdateState {
                downloaded_path: Some(text.display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            run_install_with(&path, 4242, &runner).unwrap_err().code,
            "unsupportedAsset"
        );

        // 空串与空白按"没有记录"处理（状态文件被手改过时也不该去开一个空路径）。
        for empty in ["", "   "] {
            state::save(
                &path,
                &UpdateState {
                    downloaded_path: Some(empty.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                run_install_with(&path, 4242, &runner).unwrap_err().code,
                "nothingDownloaded",
                "{empty:?}"
            );
        }

        // 四条拒绝路径上都不许起助手：那一步只在**确定要装这个文件**之后才发生。
        assert_eq!(launches.calls.get(), 0);
    }

    /// 「安装」的顺序：**先起助手、立刻回 `exiting`**。应用接着退出，所以这里不能是 `opened`。
    #[test]
    fn installing_schedules_the_installer_for_after_our_exit() {
        let directory = tempfile::tempdir().unwrap();
        let installer = directory.path().join("dsh-wallpaper_0.4.3_x64-setup.exe");
        std::fs::write(&installer, b"installer").unwrap();
        let (runner, launches, opens) = counting_runner(Ok(()));

        let report = run_install_with(&install_runner(&directory, &installer), 4242, &runner).unwrap();

        assert_eq!(report.next_step, InstallLaunch::Exiting);
        assert_eq!(report.kind, AssetKind::Exe);
        assert_eq!(report.path, installer.display().to_string());
        // 起助手那一步拿到的必须是**本进程的号**与**记录里那个安装包**：换一个号就会去等一个
        // 不存在的进程（等不到，退出后没人启动安装器）；换一条路径就是打开另一个文件。
        assert_eq!(launches.calls.get(), 1);
        let (pid, launched) = &*launches.arguments.borrow();
        assert_eq!(*pid, 4242);
        assert_eq!(launched, &installer.display().to_string());
        // 助手起来了就**不**回落：回落那条路意味着"应用不退出"，那是另一种用户可见的行为。
        assert_eq!(opens.calls.get(), 0);
    }

    /// `.msix` 与 `.exe` 走同一套（MSIX 是将来上商店的路，不许因为今天只发 `.exe` 就漏掉一条）。
    #[test]
    fn installing_hands_a_msix_to_the_same_launcher() {
        let directory = tempfile::tempdir().unwrap();
        let package = directory.path().join("dsh-wallpaper_0.4.3_x64.msix");
        std::fs::write(&package, b"package").unwrap();
        let (runner, launches, _opens) = counting_runner(Ok(()));

        let report = run_install_with(&install_runner(&directory, &package), 7, &runner).unwrap();

        assert_eq!(report.kind, AssetKind::Msix);
        assert_eq!(report.next_step, InstallLaunch::Exiting);
        assert_eq!(launches.calls.get(), 1);
    }

    /// 助手起不来：**回落成现在就打开**，而且如实报告 `opened`（应用不退出）。
    #[test]
    fn a_launcher_that_cannot_start_falls_back_to_opening_it_now() {
        let directory = tempfile::tempdir().unwrap();
        let installer = directory.path().join("setup.exe");
        std::fs::write(&installer, b"installer").unwrap();
        let (runner, launches, opens) = counting_runner(Err(UpdateCommandError::open_failed()));

        let report = run_install_with(&install_runner(&directory, &installer), 4242, &runner).unwrap();

        // 回落不是失败：安装包**已经交出去了**，只是没有"先退出"这一步。界面按 `opened` 说那句
        // "已交给 Windows，应用不会退出"，而不是显示一个错误。
        assert_eq!(report.next_step, InstallLaunch::Opened);
        assert_eq!(report.kind, AssetKind::Exe);
        assert_eq!(launches.calls.get(), 1);
        assert_eq!(opens.calls.get(), 1);
        assert_eq!(*opens.arguments.borrow(), installer.display().to_string());
    }

    /// 回落那一步也失败：这才有错误码，而且是既有的 `openFailed`（新码会让界面无话可说）。
    #[test]
    fn a_fallback_that_also_fails_keeps_the_open_failed_code() {
        let directory = tempfile::tempdir().unwrap();
        let installer = directory.path().join("setup.exe");
        std::fs::write(&installer, b"installer").unwrap();
        let (runner, launches, opens) = counting_runner_with(
            Err(UpdateCommandError::open_failed()),
            Err(UpdateCommandError::open_failed()),
        );

        let error = run_install_with(&install_runner(&directory, &installer), 4242, &runner)
            .expect_err("both routes failed");

        assert_eq!(error.code, "openFailed");
        assert_eq!(launches.calls.get(), 1);
        assert_eq!(opens.calls.get(), 1);
    }

    /// 助手起不来时报的**不是** openFailed（别的码）：原样往外抛，不去打开安装包盖掉原因。
    #[test]
    fn a_launcher_failure_that_is_not_open_failed_is_reported_as_is() {
        let directory = tempfile::tempdir().unwrap();
        let installer = directory.path().join("setup.exe");
        std::fs::write(&installer, b"installer").unwrap();
        let (runner, launches, opens) =
            counting_runner(Err(UpdateCommandError::unsupported_asset()));

        let error = run_install_with(&install_runner(&directory, &installer), 4242, &runner)
            .expect_err("the launcher refused");

        assert_eq!(error.code, "unsupportedAsset");
        assert_eq!(launches.calls.get(), 1);
        assert_eq!(opens.calls.get(), 0);
    }

    /// 助手要跑的脚本（纯函数）：等的是**我们的进程号**，启动的是**那个安装包**、而且**带上那几枚
    /// 参数**，可被 PowerShell 逐字还原 —— 路径里的空格、中文、单引号都不许把脚本带跑偏。
    #[test]
    fn the_waiter_script_waits_for_us_and_then_starts_the_installer() {        let installer = Path::new("C:\\Users\\我 的 用户\\AppData\\Local\\com.dsh.wallpaper\\updates\\0.4.3\\dsh-wallpaper_0.4.3_x64-setup.exe");
        let script = installer_waiter_script(4242, installer);
        assert!(script.contains("Get-Process -Id 4242"));
        // 等的是"进程结束"这件事本身：`$p.WaitForExit()` 拿的是那个进程对象。
        // （`Wait-Process -Id … -ErrorAction SilentlyContinue` 实测会退化成"立刻返回"，不许用。）
        assert!(script.contains("$p.WaitForExit()"));
        assert!(!script.contains("Wait-Process"));
        // 路径与参数在同一句里：参数漏掉的话安装器会以普通向导起来（用户看到的维护页就是它）。
        assert!(script.contains(&format!(
            "Start-Process -FilePath '{}' -ArgumentList '/P','/UPDATE','/R'",
            installer.display()
        )));
        // 固定延时的痕迹：这件事的语义是"等进程结束"，不是"等一会儿"。
        assert!(!script.contains("Start-Sleep"));
        assert!(!script.contains("timeout"));

        // PowerShell 单引号字符串里唯一的转义就是把 `'` 写成 `''`；别的字符（含 `$`、反引号、
        // 空格、中文）在单引号里都是字面量，所以这里不该出现别的转义。参数也不能因为路径难写
        // 就掉队（这两半在同一条命令行上）。
        let awkward = Path::new("C:\\Users\\O'Brien\\更新 包\\setup.exe");
        let escaped = installer_waiter_script(4242, awkward);
        assert!(escaped.contains("'C:\\Users\\O''Brien\\更新 包\\setup.exe' -ArgumentList '/P','/UPDATE','/R'"));
        assert!(!escaped.contains("`"));
    }

    /// 安装器参数的清单与拼法（纯函数）：`/P`（没有向导页）、`/UPDATE`（原地覆盖、不夹带快捷方式）、
    /// `/R`（装完自动起新版）。**没有 `/S`**：那一枚会把进度窗口也去掉（理由写在常量上）。
    ///
    /// 单测钉的是"参数真的跟着 exe 走"这件事 —— 漏掉 `-ArgumentList` 时安装器会以普通向导起来，
    /// 而症状是用户看到一个维护页，不是这里红。
    #[test]
    fn the_installer_arguments_ride_along_as_an_array() {
        assert_eq!(INSTALLER_ARGUMENTS, ["/P", "/UPDATE", "/R"]);
        assert_eq!(installer_arguments(), "'/P','/UPDATE','/R'");

        let script = installer_waiter_script(7, Path::new("C:\\updates\\0.4.7\\setup.exe"));
        assert!(script.contains(" -ArgumentList '/P','/UPDATE','/R'"));
        // 数组形态：每枚参数各自一对单引号（写成 `'/P /UPDATE /R'` 也传得过去，但那样引号就得
        // 自己管；分开写让 PowerShell 按 Windows 的规则拼）。
        assert_eq!(script.matches("'/P'").count(), 1);
        assert!(!script.contains("'/P /UPDATE /R'"));
    }

    /// 清理时留下的版本（纯函数）：**正在跑的这一版**，加上状态里记着的那一枚安装包所属的版本。
    ///
    /// 记着的那一枚不能漏：它是用户已经下好、准备装的那一个，删了就等于让用户白下 59 MB。
    /// 两枚都读不出来时给的是空名单 —— 清理那一步会因此什么都不删（见 `stale_installer_dirs`）。
    #[test]
    fn the_versions_to_keep_cover_the_running_one_and_the_recorded_download() {
        let recorded = UpdateState {
            downloaded_path: Some(
                "C:\\Users\\u\\AppData\\Local\\com.dsh.wallpaper\\updates\\0.4.7\\dsh-wallpaper_0.4.7_x64-setup.exe"
                    .into(),
            ),
            ..Default::default()
        };
        let keep = versions_to_keep("0.4.6", &recorded);
        assert_eq!(keep.len(), 2);
        assert!(keep.contains(&Version::parse("0.4.6").unwrap()));
        assert!(keep.contains(&Version::parse("0.4.7").unwrap()));

        // 状态里记着的就是当前版本那一枚（真机上今天的形状）：只留一份，不重复。
        let same_version = UpdateState {
            downloaded_path: Some("C:\\updates\\0.4.6\\setup.exe".into()),
            ..Default::default()
        };
        assert_eq!(
            versions_to_keep("0.4.6", &same_version),
            vec![Version::parse("0.4.6").unwrap()]
        );

        // 没有记录：只留当前版本。当前版本也读不出来：空名单（清理会因此停下，什么都不删）。
        assert_eq!(
            versions_to_keep("0.4.6", &UpdateState::default()),
            vec![Version::parse("0.4.6").unwrap()]
        );
        assert!(versions_to_keep("说不清的版本", &UpdateState::default()).is_empty());
        // 记录的父目录名不是版本（用户自己把安装包挪成了一枚散文件）：不参与保留。
        let stranger = UpdateState {
            downloaded_path: Some("C:\\updates\\setup.exe".into()),
            ..Default::default()
        };
        assert_eq!(
            versions_to_keep("0.4.6", &stranger),
            vec![Version::parse("0.4.6").unwrap()]
        );
    }

    /// 整个"顺手清理"跑一次：旧目录搬走、状态里那条路径跟着改、旧版本的安装包被删、
    /// 状态里记着的那一枚留着。现场按真机上的形状摆（4 个安装包 + 一条指向当前版本的记录）。
    #[test]
    fn the_reconcile_moves_the_legacy_directory_and_prunes_other_versions() {
        let directory = tempfile::tempdir().unwrap();
        let local_app_data = directory.path().join(super::super::APP_IDENTIFIER);
        let legacy = state::legacy_updates_dir(&local_app_data);
        for version in ["0.4.3", "0.4.4", "0.4.5", "0.4.6"] {
            std::fs::create_dir_all(legacy.join(version)).unwrap();
            std::fs::write(
                legacy
                    .join(version)
                    .join(format!("dsh-wallpaper_{version}_x64-setup.exe")),
                b"installer",
            )
            .unwrap();
        }
        let recorded = legacy
            .join("0.4.6")
            .join("dsh-wallpaper_0.4.6_x64-setup.exe")
            .display()
            .to_string();
        state::save(
            &legacy.join(state::STATE_FILE_NAME),
            &UpdateState {
                latest_version: Some("0.4.6".into()),
                downloaded_path: Some(recorded),
                ..Default::default()
            },
        )
        .unwrap();

        let updates = state::updates_dir(&local_app_data);
        reconcile_updates_directory(&local_app_data, &updates, "0.4.6");

        // 旧的嵌套目录连上面那层标识符目录一起收掉。
        assert!(!legacy.exists());
        assert!(!legacy.parent().unwrap().exists());
        // 当前版本那一枚留着（状态里记着它），三个旧版本没了。
        assert!(updates
            .join("0.4.6")
            .join("dsh-wallpaper_0.4.6_x64-setup.exe")
            .is_file());
        for gone in ["0.4.3", "0.4.4", "0.4.5"] {
            assert!(!updates.join(gone).exists(), "{gone} 是旧版本，该删");
        }
        // 状态里的那条路径跟着搬到了新位置（不然后面「点击安装」会说文件已经不在了），
        // 别的字段一个都没动。
        let moved = state::load(&updates.join(state::STATE_FILE_NAME));
        assert_eq!(
            moved.downloaded_path,
            Some(
                updates
                    .join("0.4.6")
                    .join("dsh-wallpaper_0.4.6_x64-setup.exe")
                    .display()
                    .to_string()
            )
        );
        assert_eq!(moved.latest_version.as_deref(), Some("0.4.6"));
    }

    /// 状态里记着的那一枚**还在旧目录**、又要被清理时：先改路径、再按新路径决定留谁 ——
    /// 顺序颠倒了就会把用户刚下好的那一枚当场删掉。
    #[test]
    fn the_reconcile_keeps_the_downloaded_version_the_state_file_points_at() {
        let directory = tempfile::tempdir().unwrap();
        let local_app_data = directory.path().join(super::super::APP_IDENTIFIER);
        let legacy = state::legacy_updates_dir(&local_app_data);
        // 正在跑 0.4.6，而下好的那一枚是 0.4.7（升级中）。
        for version in ["0.4.6", "0.4.7"] {
            std::fs::create_dir_all(legacy.join(version)).unwrap();
            std::fs::write(legacy.join(version).join("setup.exe"), b"installer").unwrap();
        }
        state::save(
            &legacy.join(state::STATE_FILE_NAME),
            &UpdateState {
                downloaded_path: Some(legacy.join("0.4.7").join("setup.exe").display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let updates = state::updates_dir(&local_app_data);
        reconcile_updates_directory(&local_app_data, &updates, "0.4.6");

        assert!(updates.join("0.4.7").join("setup.exe").is_file());
        assert!(updates.join("0.4.6").join("setup.exe").is_file());
        assert_eq!(
            state::load(&updates.join(state::STATE_FILE_NAME)).downloaded_path,
            Some(updates.join("0.4.7").join("setup.exe").display().to_string())
        );
    }

    /// **手动的一次真机验证**（`#[ignore]`：动的是这台机器上真实的
    /// `%LOCALAPPDATA%\com.dsh.wallpaper`：把 0.4.6 写坏的嵌套目录搬走，并删掉旧版本的安装包）。
    ///
    /// 这是唯一能证明"路径真的修好了"的一条：真机上那份现场（4 个安装包、237 MB）造不出来 ——
    /// `app_local_data_dir()` 得由 Tauri 在打包态给出。跑法：
    ///
    /// ```text
    /// cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --lib -- --ignored --nocapture \
    ///   migrate_the_real_machine_updates_directory
    /// ```
    #[test]
    #[ignore = "动本机 %LOCALAPPDATA% 下真实的更新目录，只在手动验证时跑"]
    fn migrate_the_real_machine_updates_directory() {
        let Some(local_app_data) =
            dirs::data_local_dir().map(|directory| directory.join(super::super::APP_IDENTIFIER))
        else {
            println!("拿不到 %LOCALAPPDATA%，跳过");
            return;
        };
        let updates = state::updates_dir(&local_app_data);
        let legacy = state::legacy_updates_dir(&local_app_data);

        println!("本地数据目录：{}", local_app_data.display());
        println!("== 正确位置（之前）==\n{}", print_tree(&updates));
        println!("== 旧位置（之前）==\n{}", print_tree(&legacy));

        reconcile_updates_directory(&local_app_data, &updates, env!("CARGO_PKG_VERSION"));

        println!("== 正确位置（之后）==\n{}", print_tree(&updates));
        println!("== 旧位置（之后）==\n{}", print_tree(&legacy));
        assert!(
            !legacy.is_dir(),
            "旧目录还在：\n{}（迁移应当把它清干净）",
            print_tree(&legacy)
        );
    }

    /// 一棵目录里的文件清单（真机验证那条测试的取证用）：路径、每个文件的字节数、合计。
    fn print_tree(root: &Path) -> String {
        let Ok(entries) = std::fs::read_dir(root) else {
            return format!("（不存在：{}）", root.display());
        };
        let mut lines = vec![root.display().to_string()];
        let mut total = 0u64;
        let mut paths = entries
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<PathBuf>>();
        paths.sort();
        for path in paths {
            if path.is_file() {
                let size = path.metadata().map(|meta| meta.len()).unwrap_or_default();
                total += size;
                lines.push(format!("  {size:>12}  {}", path.display()));
                continue;
            }
            let mut inner = std::fs::read_dir(&path)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .collect::<Vec<PathBuf>>()
                })
                .unwrap_or_default();
            inner.sort();
            for file in inner {
                let size = file.metadata().map(|meta| meta.len()).unwrap_or_default();
                total += size;
                lines.push(format!("  {size:>12}  {}", file.display()));
            }
        }
        format!("合计 {total} 字节\n{}", lines.join("\n"))
    }

    /// 助手命令行：整段脚本按 **UTF-16LE + Base64** 走 `-EncodedCommand`，所以路径里的空格与中文
    /// 不经过命令行解析。这一条把 Base64 解回来逐字比对 —— 编码方式写错（比如用 UTF-8）时，
    /// PowerShell 会解码出乱码路径，而那时症状是"安装器起不来"，不是这里有测试失败。
    ///
    /// 同时钉住那三个**实测出来的**"别这么写"（每一个都让助手静默地什么都不做）：
    /// `-WindowStyle Hidden` 会让 PowerShell 在 14 ms 内直接退出；`-EncodedCommand` 与那段
    /// Base64 必须分成两项；`-NoProfile` 必须留着。
    #[test]
    fn the_launcher_argv_encodes_the_script_as_utf16le_base64() {
        use base64::Engine;

        let installer = Path::new("C:\\Users\\我 的 用户\\setup.exe");
        let (program, arguments) = installer_launcher_argv(4242, installer);

        // 执行文件是一个绝对路径（找得到 `powershell.exe` 的进程里，`PATH` 是否完整都不影响）。
        assert_eq!(
            program.file_name().and_then(|name| name.to_str()),
            Some("powershell.exe")
        );
        assert_eq!(
            arguments[..2],
            ["-NoProfile".to_string(), "-EncodedCommand".to_string()],
            "助手必须无配置地起来，而且那一段 Base64 要单独作为一项"
        );
        assert!(
            !arguments.iter().any(|argument| argument.contains("WindowStyle")),
            "`-WindowStyle Hidden` 会让脚本一行都不跑（实测 14 ms 就退出），不许加回来"
        );

        let encoded = &arguments[2];
        // Base64 字母表里没有空格、引号这些会让命令行解析犯迷糊的字符，所以整项按原样传即可。
        assert!(
            encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
            "Base64 以外的东西出现在这里：{encoded}"
        );
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        assert_eq!(bytes.len() % 2, 0, "UTF-16LE 的字节数一定是偶数");
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<u16>>();
        let decoded = String::from_utf16(&units).unwrap();
        assert_eq!(decoded, installer_waiter_script(4242, installer));
    }

    /// 两个报告也是契约：界面按这些字段名读（`started` / `kind` / `nextStep` 是新增的那几个）。
    #[test]
    fn the_download_and_install_reports_serialize_to_the_fields_the_interface_reads() {        let download = UpdateDownloadReport {
            started: true,
            version: "0.4.2".into(),
            destination: "C:\\updates\\0.4.2\\setup.exe".into(),
        };
        assert_eq!(
            serde_json::to_string(&download).unwrap(),
            r#"{"started":true,"version":"0.4.2","destination":"C:\\updates\\0.4.2\\setup.exe"}"#
        );

        // 按后缀分派的结果就是这两个词（界面据此说"交给安装向导"还是"交给 App Installer"）；
        // `nextStep` 是"应用接着退出"还是"应用留着"，界面按它说最后那句话。
        for (kind, expected) in [(AssetKind::Exe, "exe"), (AssetKind::Msix, "msix")] {
            for (step, step_name) in [
                (InstallLaunch::Exiting, "exiting"),
                (InstallLaunch::Opened, "opened"),
            ] {
                let report = UpdateInstallReport {
                    path: "C:\\updates\\0.4.2\\a.bin".into(),
                    kind,
                    next_step: step,
                };
                let value = serde_json::to_value(&report).unwrap();
                assert_eq!(value["kind"], serde_json::json!(expected));
                assert_eq!(value["path"], serde_json::json!("C:\\updates\\0.4.2\\a.bin"));
                assert_eq!(value["nextStep"], serde_json::json!(step_name));
            }
        }
    }

    /// **手动的一次真机验证**（`#[ignore]`：需要真的起进程、真的开一个记事本，跑完自动杀掉）。
    ///
    /// 说的是我们唯一没法在单测里证明的那一段：`installer_waiter_script` 生成的脚本喂给
    /// `installer_launcher_argv` 生成的命令行之后，助手**真的**会等到目标进程结束才启动目标程序。
    ///
    /// 目标用 `notepad.exe`（不跑真安装器），闸门用"临时起的一个 5 秒进程"而不是本测试进程
    /// （后者要等测试进程结束，那就在测试里等不到结果）。
    ///
    /// 跑法：`cargo test --lib -- --ignored real_launcher_waits_for_the_gate_process --nocapture`
    #[test]
    #[ignore = "真的起进程（PowerShell + notepad），只在手动验证时跑"]
    fn real_launcher_waits_for_the_gate_process() {
        use std::time::{Duration, Instant};

        let gate = std::process::Command::new(crate::update::commands::windows_powershell())
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"])
            .spawn()
            .expect("a gate process");
        let gate_id = gate.id();

        let (program, arguments) =
            installer_launcher_argv(gate_id, Path::new("C:\\Windows\\System32\\notepad.exe"));
        let started = Instant::now();
        let _helper = std::process::Command::new(&program)
            .args(arguments)
            .spawn()
            .expect("the helper");

        std::thread::sleep(Duration::from_millis(1500));
        let early = wait_for_notepad(0);
        let awaited = started.elapsed();
        let notepad = wait_for_notepad(15);

        // 收尾：别给这台机器留下一个记事本窗口。
        let _ = std::process::Command::new("taskkill").args(["/IM", "notepad.exe", "/F"]).output();
        let _ = std::process::Command::new("taskkill").args(["/PID", &gate_id.to_string(), "/F"]).output();

        assert!(early == 0, "闸门还活着（已 {awaited:?}），记事本不该已经起来 —— 助手没在等");
        assert!(notepad > 0, "助手退出之后安装器该起来了（等了 {awaited:?}）");
    }

    /// **手动的一次真机验证**（`#[ignore]`：真的起进程、真的跑一遍那段 PowerShell）。
    ///
    /// 说的是单测证明不了的那一半：脚本里的 `-ArgumentList '/P','/UPDATE','/R'` **真的**会跟着
    /// 可执行文件走。目标不是安装器，是一个两行的 `.cmd`：把它收到的参数写进旁边的 `args.txt`
    /// （`Start-Process` 收数组时由 PowerShell 按 Windows 的规则拼命令行，这一条正是"拼得对不对"
    /// 的证据）。闸门用"临时起的一个 2 秒进程"，与上面那条一样。
    ///
    /// 跑法：`cargo test --lib -- --ignored real_launcher_passes_the_arguments --nocapture`
    #[test]
    #[ignore = "真的起进程（PowerShell + 一个临时 .cmd），只在手动验证时跑"]
    fn real_launcher_passes_the_arguments() {
        use std::time::Duration;

        let directory = tempfile::tempdir().unwrap();
        let recorder = directory.path().join("record args.cmd");
        let recorded = directory.path().join("args.txt");
        std::fs::write(
            &recorder,
            format!(
                "@echo off\r\necho %* > \"{}\"\r\n",
                recorded.display()
            ),
        )
        .unwrap();

        let gate = std::process::Command::new(crate::update::commands::windows_powershell())
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 2"])
            .spawn()
            .expect("a gate process");
        let gate_id = gate.id();

        let (program, arguments) = installer_launcher_argv(gate_id, &recorder);
        let _helper = std::process::Command::new(&program)
            .args(arguments)
            .spawn()
            .expect("the helper");

        // 等闸门结束 + 记录文件出现（`.cmd` 会在自己的控制台里跑完就退出）。
        let mut text = String::new();
        for _ in 0..60 {
            if let Ok(recorded_text) = std::fs::read_to_string(&recorded) {
                text = recorded_text;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &gate_id.to_string(), "/F"])
            .output();

        // 路径里带空格（`record args.cmd`）：它得原样当**一个**参数传过去，参数还得排在它后面。
        println!("脚本：{}", installer_waiter_script(gate_id, &recorder));
        println!("recorder 收到的参数：{text:?}");
        assert!(text.contains("/P"), "参数没跟着可执行文件走：{text:?}");
        assert!(text.contains("/UPDATE"), "少了 /UPDATE：{text:?}");
        assert!(text.contains("/R"), "少了 /R：{text:?}");
        assert!(
            text.contains("/P /UPDATE /R"),
            "三枚参数该按顺序连着给：{text:?}"
        );
    }

    /// 等一个 `notepad.exe` 出现（最多 `seconds` 秒），返回看到几个。
    fn wait_for_notepad(seconds: u64) -> usize {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
        loop {
            let output = std::process::Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq notepad.exe", "/NH"])
                .output()
                .expect("tasklist");
            let text = String::from_utf8_lossy(&output.stdout);
            if text.to_lowercase().contains("notepad.exe") {
                return 1;
            }
            if std::time::Instant::now() >= deadline {
                return 0;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}
