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

/// 更新目录：`<本地数据>\com.dsh.wallpaper\updates`（§五）。
///
/// 优先用 Tauri 算的那个本地数据目录（打包态它会被重定向到包容器里，与其它功能同一处），
/// 拿不到时退回 `dirs` 的 `%LOCALAPPDATA%` —— 两条路拼出来的是同一个目录，退回不是"第二个位置"。
///
/// 状态文件与下载下来的安装包都在这个目录下（`state.json` 与 `<版本>\<资产名>`，§六）。
fn updates_directory(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let local_app_data = app
        .path()
        .app_local_data_dir()
        .ok()
        .or_else(|| dirs::data_local_dir().map(|directory| directory.join(super::APP_IDENTIFIER)))?;
    Some(state::updates_dir(&local_app_data))
}

/// 状态文件的位置：`<本地数据>\com.dsh.wallpaper\updates\state.json`（§五）。
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

/// 把已经下载好的安装包交给 Windows（§六）。
///
/// 路径**不由界面给**：它读状态文件里那一条（`downloadedPath`）。界面因此无法让原生去打开任意一个
/// 文件 —— 那会是一条"打开任何可执行文件"的命令，比 `open_external_link` 更宽。
pub(crate) fn run_install(state_path: &Path) -> Result<UpdateInstallReport, UpdateCommandError> {
    let recorded = state::load(state_path).downloaded_path;
    let Some(recorded) = recorded.filter(|path| !path.trim().is_empty()) else {
        log::info!("更新安装：状态文件里没有下过的安装包");
        return Err(UpdateCommandError::nothing_downloaded());
    };
    let path = PathBuf::from(recorded);
    let kind = install_kind(&path)?;
    shell_open(&path)?;
    Ok(UpdateInstallReport {
        path: path.display().to_string(),
        kind,
    })
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

/// 「安装」：把已经下载好的安装包按后缀交给 Windows 的默认处理程序（§六）。
///
/// `.exe`（NSIS setup）⇒ 安装向导；`.msix` ⇒ App Installer。两条分支都留着 —— MSIX 是将来上商店
/// 的路，不许因为今天只发 `.exe` 就删掉。两条都只是 `ShellExecuteW("open", …)`，确认由用户在
/// 系统界面上点。
///
/// 返回值只有路径与按后缀分派的结果（见 [`UpdateInstallReport`]）；开不起来给的是码
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
    // 读状态 + 一次 `ShellExecuteW`，都不打网络也不占阻塞线程。
    run_install(&state_path)
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

        // 还没下过任何东西。
        assert_eq!(run_install(&path).unwrap_err().code, "nothingDownloaded");

        // 记过一条，但文件已经不在了。
        state::save(
            &path,
            &UpdateState {
                downloaded_path: Some(directory.path().join("gone.exe").display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(run_install(&path).unwrap_err().code, "installerMissing");

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
        assert_eq!(run_install(&path).unwrap_err().code, "unsupportedAsset");

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
            assert_eq!(run_install(&path).unwrap_err().code, "nothingDownloaded", "{empty:?}");
        }
    }

    /// 两个报告也是契约：界面按这些字段名读（`started` / `kind` 是新增的两个）。
    #[test]
    fn the_download_and_install_reports_serialize_to_the_fields_the_interface_reads() {
        let download = UpdateDownloadReport {
            started: true,
            version: "0.4.2".into(),
            destination: "C:\\updates\\0.4.2\\setup.exe".into(),
        };
        assert_eq!(
            serde_json::to_string(&download).unwrap(),
            r#"{"started":true,"version":"0.4.2","destination":"C:\\updates\\0.4.2\\setup.exe"}"#
        );

        // 按后缀分派的结果就是这两个词（界面据此说"交给安装向导"还是"交给 App Installer"）。
        for (kind, expected) in [(AssetKind::Exe, "exe"), (AssetKind::Msix, "msix")] {
            let report = UpdateInstallReport {
                path: "C:\\updates\\0.4.2\\a.bin".into(),
                kind,
            };
            let value = serde_json::to_value(&report).unwrap();
            assert_eq!(value["kind"], serde_json::json!(expected));
            assert_eq!(value["path"], serde_json::json!("C:\\updates\\0.4.2\\a.bin"));
        }
    }
}
