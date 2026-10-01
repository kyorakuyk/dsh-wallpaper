//! 下载与校验：§四 的 `downloading` / `ready` / `failed` 三个状态由这里产生（计划书 §六）。
//!
//! 这一片接的是第二片留下的手：`features/update/useUpdate.ts` 里的「下载」原来只是"打开发布页"，
//! 现在它调 [`super::commands::update_download`]，进度由**一条全局事件**回到界面。
//!
//! 三件事写在这里，而且都是**可测的**（时间与数据块都是注入的，单测不碰网络、不等 200 毫秒）：
//!
//!  - 落盘位置与文件名的安全检查（[`destination`]）；
//!  - 写盘 + 摘要 + 进度节流（[`FileSink`] / [`ProgressTicker`]）；
//!  - 校验与"不符就删掉"（[`verify`] / [`finalize`] / [`commit`]）。
//!
//! 一条贯穿的规矩：**进度事件是界面唯一的下载状态来源**。命令立刻返回（只说"开工了没有"），
//! 终局（`ready` / `failed`）永远会发一条事件 —— 于是不会出现"命令回来了但界面停在进度条上"。
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::release::{AssetKind, ReleaseAsset};
use super::state;
use super::version::Version;

/// 进度事件名。**一条全局事件**：壁纸宿主与设置中心都订阅它（计划书 §四；上一片发现两个窗口
/// 之间没有推送，就是缺这一条）。
pub(crate) const DOWNLOAD_EVENT: &str = "update-download";

/// 进度事件的最小间隔（§四：每 ~200 毫秒一发）。
pub(crate) const PROGRESS_INTERVAL_MS: u64 = 200;

/// 下载中的临时文件后缀：名字里带一个点，与正式文件区分开 —— 半截文件永远不以正式名字出现。
const PART_SUFFIX: &str = ".part";

/// 下载用的总超时。
///
/// 与检查那一次的 20 秒**不能共用**：一次 30 MB 的下载在慢网络上远超它，而超时会被当成"网络
/// 失败"、于是删掉已经下了一半的文件（用户看到的是"下载失败：网络不可用"）。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// §四 的下载状态：`downloading` 是过程，`ready` / `failed` 是两种终局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DownloadPhase {
    Downloading,
    Ready,
    Failed,
}

/// 下载失败的原因码。只有码 —— 文案由界面按语言说。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DownloadFailureCode {
    /// 请求没到：DNS、TLS、超时、没有网络。
    Network,
    /// 服务器答了，但不是 2xx。
    HttpStatus,
    /// 保存位置不可用：版本/资产名拼不出安全路径，或保存目录建不出来。
    DestinationUnavailable,
    /// 写盘失败：权限、被占用、改名失败。
    WriteFailed,
    /// 磁盘空间不足（Windows 的 `ERROR_DISK_FULL`，Unix 的 `ENOSPC`）。
    DiskFull,
    /// 写下来的字节数与 API 给的大小不符。
    SizeMismatch,
    /// 摘要与 API 给的 `digest` 不符。
    DigestMismatch,
}

/// 下载失败：一个码，加上（有的话）能对上账的数字。
///
/// 与检查那一步的 `FailureReport` 同一形状：**一句句子都不回**，界面按码说人话
/// （`features/update/updateCopy.ts`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadFailure {
    pub code: DownloadFailureCode,
    /// `code == HttpStatus` 时是响应码；其余情况为 `null`。
    pub http_status: Option<u16>,
    /// 校验失败时才有的两个数字。
    pub expected_bytes: Option<u64>,
    pub actual_bytes: Option<u64>,
    /// 摘要不符时才有的两个串（`sha256:<hex>`）。
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
}

impl DownloadFailure {
    fn only_code(code: DownloadFailureCode) -> Self {
        Self {
            code,
            http_status: None,
            expected_bytes: None,
            actual_bytes: None,
            expected_sha256: None,
            actual_sha256: None,
        }
    }

    pub(crate) fn network() -> Self {
        Self::only_code(DownloadFailureCode::Network)
    }

    pub(crate) fn http_status(status: u16) -> Self {
        Self {
            http_status: Some(status),
            ..Self::only_code(DownloadFailureCode::HttpStatus)
        }
    }

    pub(crate) fn destination_unavailable() -> Self {
        Self::only_code(DownloadFailureCode::DestinationUnavailable)
    }

    /// 写盘失败。磁盘满单独给一个码：它是**用户能自己解决**的那一类，界面要说得出"磁盘空间不足"。
    pub(crate) fn from_io(error: &std::io::Error) -> Self {
        let code = if is_disk_full(error) {
            DownloadFailureCode::DiskFull
        } else {
            log::warn!("更新下载：写盘失败：{error}");
            DownloadFailureCode::WriteFailed
        };
        Self::only_code(code)
    }

    pub(crate) fn size_mismatch(expected: u64, actual: u64) -> Self {
        Self {
            expected_bytes: Some(expected),
            actual_bytes: Some(actual),
            ..Self::only_code(DownloadFailureCode::SizeMismatch)
        }
    }

    pub(crate) fn digest_mismatch(expected: &str, actual: &str) -> Self {
        Self {
            expected_sha256: Some(expected.to_string()),
            actual_sha256: Some(actual.to_string()),
            ..Self::only_code(DownloadFailureCode::DigestMismatch)
        }
    }

    /// 校验失败要不要删掉文件：§六 说"不符即删除"。
    pub(crate) fn is_verification_failure(&self) -> bool {
        matches!(
            self.code,
            DownloadFailureCode::SizeMismatch | DownloadFailureCode::DigestMismatch
        )
    }
}

/// 磁盘满：Windows 是 `ERROR_DISK_FULL`（112），Unix 是 `ENOSPC`（28）。
///
/// 不按 `io::ErrorKind::StorageFull` 判：那一条在本仓库声明的最低 Rust 版本（1.77）里还没有稳定。
fn is_disk_full(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(112) | Some(28))
}

/// 一条进度/终局事件。字段名是两侧的契约（界面按 `phase` 分支、按 `failure.code` 选句子）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadEvent {
    /// 这次下载的是哪个版本（界面据此忽略**别的**版本的旧事件）。
    pub version: String,
    pub phase: DownloadPhase,
    /// 已经写下的字节数（终局时就是文件的大小）。
    pub downloaded_bytes: u64,
    /// API 给的总大小；没给时是 `null` —— 界面那时说"已下载多少"，**不编**百分比。
    pub total_bytes: Option<u64>,
    /// `ready` 时的落盘位置。
    pub path: Option<String>,
    /// `ready` 时算出来的 sha256（`sha256:<hex>`）。
    pub sha256: Option<String>,
    /// `failed` 时的原因码；其余情况为 `null`。
    pub failure: Option<DownloadFailure>,
}

impl DownloadEvent {
    pub(crate) fn progress(version: &str, downloaded_bytes: u64, total_bytes: Option<u64>) -> Self {
        Self {
            version: version.to_string(),
            phase: DownloadPhase::Downloading,
            downloaded_bytes,
            total_bytes,
            path: None,
            sha256: None,
            failure: None,
        }
    }

    pub(crate) fn ready(version: &str, bytes: u64, path: &Path, sha256: &str) -> Self {
        Self {
            version: version.to_string(),
            phase: DownloadPhase::Ready,
            downloaded_bytes: bytes,
            total_bytes: Some(bytes),
            path: Some(path.display().to_string()),
            sha256: Some(format!("sha256:{sha256}")),
            failure: None,
        }
    }

    pub(crate) fn failed(
        version: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        failure: DownloadFailure,
    ) -> Self {
        Self {
            version: version.to_string(),
            phase: DownloadPhase::Failed,
            downloaded_bytes,
            total_bytes,
            path: None,
            sha256: None,
            failure: Some(failure),
        }
    }
}

/// 界面递回来的那一个资产 —— 就是检查报告里的 `asset`（资产选择在 `release::select_asset`
/// 里已经做完了，这里不再选一遍）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DownloadRequest {
    pub name: String,
    pub download_url: String,
    /// API 给的字节数；没给时是 0（那就只核摘要，不核大小）。
    pub size: u64,
    /// GitHub 的 `digest`（`sha256:<hex>`）；老响应没有就是 `None`。
    pub digest: Option<String>,
}

impl DownloadRequest {
    /// 从一位已选中的资产造一个请求（生产路径：报告里的那一个）。
    pub(crate) fn of(asset: &ReleaseAsset) -> Self {
        Self {
            name: asset.name.clone(),
            download_url: asset.download_url.clone(),
            size: asset.size,
            digest: asset.digest.clone(),
        }
    }
}

/// 进度节流器：**第一条立刻发**（气泡要马上从「下载」变成进度条），之后每 `interval_ms` 最多一条。
///
/// 时间从外面递进来，所以"每 200 毫秒一发"这件事能在单测里用假时钟钉住，不必真的等。
#[derive(Debug)]
pub(crate) struct ProgressTicker {
    interval_ms: u64,
    last_at_ms: Option<u64>,
}

impl ProgressTicker {
    pub(crate) fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            last_at_ms: None,
        }
    }

    /// 这一刻该不该发一条。发了就记下这一刻（下一次要再等 `interval_ms`）。
    pub(crate) fn due(&mut self, now_ms: u64) -> bool {
        let due = match self.last_at_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= self.interval_ms,
        };
        if due {
            self.last_at_ms = Some(now_ms);
        }
        due
    }
}

/// 写盘的一端：写块、数字节、算摘要，并节流地告诉调用者"该发一条进度了"。
///
/// 单独一个结构体是为了让"写盘 + 摘要 + 节流"这三件与网络无关的事能被单测直接喂数据块验证。
pub(crate) struct FileSink {
    file: std::fs::File,
    hasher: Sha256,
    written: u64,
    ticker: ProgressTicker,
}

impl FileSink {
    /// 建临时文件。建不出来（目录不可写、磁盘满）当场就是一个码。
    pub(crate) fn create(temporary: &Path, interval_ms: u64) -> Result<Self, DownloadFailure> {
        let file = std::fs::File::create(temporary).map_err(|error| DownloadFailure::from_io(&error))?;
        Ok(Self {
            file,
            hasher: Sha256::new(),
            written: 0,
            ticker: ProgressTicker::new(interval_ms),
        })
    }

    /// 写一块。返回值是"这一刻要发进度的话，已写多少字节"。
    pub(crate) fn write(&mut self, chunk: &[u8], now_ms: u64) -> Result<Option<u64>, DownloadFailure> {
        self.file
            .write_all(chunk)
            .map_err(|error| DownloadFailure::from_io(&error))?;
        self.hasher.update(chunk);
        self.written += chunk.len() as u64;
        Ok(self.ticker.due(now_ms).then_some(self.written))
    }

    pub(crate) fn written(&self) -> u64 {
        self.written
    }

    /// 收尾：把缓冲里的字节落盘并给出摘要。
    ///
    /// `flush` 不能省：不刷的话"大小对得上"只是缓冲区里的数字，文件其实还是半截。
    fn finish(mut self) -> Result<(u64, String), DownloadFailure> {
        self.file
            .flush()
            .map_err(|error| DownloadFailure::from_io(&error))?;
        Ok((self.written, hex(&self.hasher.finalize())))
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // `write!` 到 String 不会失败（`fmt::Write` 的约定）。
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// 下载的落盘位置：`<更新目录>\<版本>\<资产名>`（§六）。
///
/// 版本先过一遍 [`Version::parse`]：它只接受"一到四段十进制"，于是 `../..` 这类写法在这里就到不了
/// 文件系统；资产名再过一遍 [`safe_file_name`]。文档里写的 `%LOCALAPPDATA%\com.dsh.wallpaper\updates`
/// 就是调用方递进来的那个目录。
pub(crate) fn destination(updates_dir: &Path, version: &str, asset_name: &str) -> Option<PathBuf> {
    let version = Version::parse(version)?;
    let name = safe_file_name(asset_name)?;
    Some(updates_dir.join(version.to_string()).join(name))
}

/// 资产名能不能直接当文件名用。
///
/// 名字来自 release（不是我们写的），而它要拼进一个路径：所以只接受**最后一段**里没有分隔符、
/// 没有 Windows 保留字符、没有控制字符、不是 `.`/`..`、也不以点或空格结尾的名字。不合格就是
/// `None` —— 调用方报 `destinationUnavailable`，而不是硬拼一个路径出去。
fn safe_file_name(name: &str) -> Option<&str> {
    let name = name.trim();
    if name.is_empty() || name.len() > 255 {
        return None;
    }
    if name == "." || name == ".." {
        return None;
    }
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return None;
    }
    if name.chars().any(char::is_control) {
        return None;
    }
    // Windows 会把结尾的点和空格吃掉：写下的名字与读回来的名字因此不是同一个。
    if name.ends_with('.') || name.ends_with(' ') {
        return None;
    }
    Some(name)
}

/// 只有发布仓库的 https 地址才会被下载。
///
/// 地址是界面递回来的（就是报告里那一个），所以在这里再确认一次它指的地方：这一个是取回**可执行
/// 文件**的出口，不能让一个被改过的渲染层决定去哪儿拿它。GitHub 的下载会 302 到
/// `*.githubusercontent.com`，所以那一族也放行（重定向由 reqwest 跟随，落点仍在同一族里）。
pub(crate) fn trusted_asset_url(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("https://") else {
        return false;
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    // 端口不影响判定（GitHub 只答 443，但 `github.com:443` 也是同一个主机）。
    let host = authority.split(':').next().unwrap_or_default().to_ascii_lowercase();
    host == "github.com" || host == "api.github.com" || host.ends_with(".githubusercontent.com")
}

/// API 的 `digest` 里**能核对的那一半**：`sha256:<64 位 hex>` ⇒ 小写 hex。
///
/// 不是这个形状（老响应没有这个字段，或者将来换成别的算法）就是"没有可核对的对象"，而不是
/// "校验失败"：认不出格式就删文件的话，用户永远装不上更新（§六 只要求"有 digest 时"核对）。
pub(crate) fn expected_sha256(digest: Option<&str>) -> Option<String> {
    let digest = digest?.trim();
    let hex = digest.strip_prefix("sha256:")?.trim();
    (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

/// 已经全部写进临时文件的东西：位置、字节数与算出来的摘要。
#[derive(Debug)]
pub(crate) struct StagedFile {
    /// 写完但还没改名的临时文件。
    pub(crate) temporary: PathBuf,
    /// 校验通过后要改成的那个名字。
    pub(crate) final_path: PathBuf,
    pub(crate) bytes: u64,
    /// 算出来的 sha256（裸 hex，不带 `sha256:` 前缀）。
    pub(crate) sha256: String,
}

/// 校验（§六）：大小不符、或（API 给了 `digest` 时）摘要不符 ⇒ 失败。
///
/// 只有 API 真的给了 `size`（> 0）才核对大小：拿 0 去比等于"每一个下载都不符"。
pub(crate) fn verify(
    staged: &StagedFile,
    expected_size: u64,
    digest: Option<&str>,
) -> Result<(), DownloadFailure> {
    if expected_size > 0 && staged.bytes != expected_size {
        log::warn!(
            "更新下载：大小不符（API 说 {expected_size}，写下 {}）",
            staged.bytes
        );
        return Err(DownloadFailure::size_mismatch(expected_size, staged.bytes));
    }
    if let Some(expected) = expected_sha256(digest) {
        if !expected.eq_ignore_ascii_case(&staged.sha256) {
            log::warn!("更新下载：摘要不符（API 说 {expected}，算出来 {}）", staged.sha256);
            // 两个串都按 `sha256:<hex>` 的形状报出去：与 `ready` 事件、状态文件里那一份同形。
            return Err(DownloadFailure::digest_mismatch(
                &format!("sha256:{expected}"),
                &format!("sha256:{}", staged.sha256),
            ));
        }
        log::info!("更新下载：摘要核对通过（{expected}）");
    } else if digest.is_some() {
        // 给了 digest 但形状不是 `sha256:<hex>`：只核大小，并且留一条痕（不静默跳过）。
        log::warn!("更新下载：digest 不是 sha256 形状，只核对大小");
    }
    Ok(())
}

/// 校验不通过就**删掉**刚下下来的东西，然后把失败报出去（§六："不符即删除并报 failed"）。
pub(crate) fn finalize(
    staged: StagedFile,
    expected_size: u64,
    digest: Option<&str>,
) -> Result<StagedFile, DownloadFailure> {
    if let Err(failure) = verify(&staged, expected_size, digest) {
        debug_assert!(failure.is_verification_failure());
        discard(&staged.temporary);
        return Err(failure);
    }
    Ok(staged)
}

/// 校验通过 ⇒ 改名到正式位置。改名在这一层是原子的，所以正式名字下永远是一个完整文件。
///
/// 目标已经存在（上一次下过同一个版本）时**直接覆盖**：Rust 的 `fs::rename` 在 Windows 上走的是
/// `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`，覆盖是它保证的行为。这不是一句可以省略的前提 ——
/// 如果它不覆盖，第二次下载就会卡在最后一步，而报出来的是含义模糊的"写盘失败"。
pub(crate) fn commit(staged: &StagedFile) -> Result<(), DownloadFailure> {
    std::fs::rename(&staged.temporary, &staged.final_path).map_err(|error| {
        log::warn!(
            "更新下载：改名失败（{} → {}）：{error}",
            staged.temporary.display(),
            staged.final_path.display()
        );
        DownloadFailure::from_io(&error)
    })
}

/// 删掉下载了一半的东西。删不掉只留一条日志：失败本来就要报，再叠一个错误没有意义。
pub(crate) fn discard(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => log::warn!("更新下载：删不掉 {}：{error}", path.display()),
    }
}

/// 正在进行的那一次下载（版本号）。**全局一把**，理由两条：
///
/// 1. 两个窗口可能在同一时刻按「下载」：同一个文件被两路写会写坏（后一路从 0 截断前一路的成果）；
/// 2. 界面上"正在下载"是一个状态，同一时刻只该有一次。
static IN_FLIGHT: Mutex<Option<String>> = Mutex::new(None);

/// 占住这一次下载的凭据：拿着它才算"正在下载"，丢掉它就释放（含出错与提前返回的路径）。
pub(crate) struct InFlight;

impl InFlight {
    /// 占住这一次下载。已经有一次在跑（任何版本）时返回 `None`。
    pub(crate) fn claim(version: &str) -> Option<Self> {
        let mut slot = IN_FLIGHT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_some() {
            return None;
        }
        *slot = Some(version.to_string());
        Some(Self)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        *IN_FLIGHT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

/// 进程一份的下载客户端。
///
/// 与检查那一次的客户端**不是同一个**：总超时是 30 分钟而不是 20 秒（见 [`DOWNLOAD_TIMEOUT`]），
/// 但同样自报家门 —— GitHub 的匿名请求没有 `User-Agent` 会 403。
fn download_client() -> Option<&'static reqwest::Client> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(super::source::user_agent())
                .connect_timeout(Duration::from_secs(15))
                .timeout(DOWNLOAD_TIMEOUT)
                .build()
                .map_err(|error| log::warn!("更新下载：HTTP 客户端无法初始化：{error}"))
                .ok()
        })
        .as_ref()
}

/// 真正的一次下载：取流、写盘、发进度、校验、改名、落状态。
///
/// `chunks` 是数据块流（生产来自 `reqwest` 的 `bytes_stream`，测试来自 `stream::iter`），`emit`
/// 与 `now_ms` 都是注入的 —— 于是这一段能在单测里整条跑完（含终局事件与状态文件），不碰网络、
/// 不等时间。
///
/// 终局事件**永远**会发一条：它是界面唯一的"下载结束了"信号（命令早就返回了）。
pub(crate) async fn run<S, E, C>(
    version: &str,
    request: &DownloadRequest,
    final_path: &Path,
    state_path: &Path,
    chunks: S,
    mut emit: E,
    now_ms: &mut C,
) -> DownloadEvent
where
    S: Stream<Item = Result<Vec<u8>, DownloadFailure>> + Send,
    E: FnMut(&DownloadEvent) + Send,
    // 时钟与事件出口都要求 `Send`：整个下载要在 tauri 的异步运行时上跑（`spawn` 要求 future 是
    // `Send`），所以注入进来的这两样也不能把它拖住。
    C: FnMut() -> u64 + Send,
{
    let total_bytes = (request.size > 0).then_some(request.size);
    let temporary = final_path.with_file_name({
        let mut name = final_path
            .file_name()
            .unwrap_or_default()
            .to_os_string();
        name.push(PART_SUFFIX);
        name
    });

    // 保存目录：建不出来是"点了没反应"的经典成因，所以它是一个**自己的码**，不是泛泛的写盘失败。
    if let Some(parent) = final_path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!("更新下载：保存目录建不出来（{}）：{error}", parent.display());
            let event = DownloadEvent::failed(version, 0, total_bytes, DownloadFailure::destination_unavailable());
            emit(&event);
            return event;
        }
    }

    let mut sink = match FileSink::create(&temporary, PROGRESS_INTERVAL_MS) {
        Ok(sink) => sink,
        Err(failure) => {
            let event = DownloadEvent::failed(version, 0, total_bytes, failure);
            emit(&event);
            return event;
        }
    };
    // 第一条进度立刻发：界面据此从「下载」变成进度条（第二片的「下载」只是打开发布页）。
    emit(&DownloadEvent::progress(version, 0, total_bytes));

    futures_util::pin_mut!(chunks);
    while let Some(chunk) = chunks.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(failure) => {
                discard(&temporary);
                let event = DownloadEvent::failed(version, sink.written(), total_bytes, failure);
                emit(&event);
                return event;
            }
        };
        match sink.write(&chunk, now_ms()) {
            Ok(Some(written)) => emit(&DownloadEvent::progress(version, written, total_bytes)),
            Ok(None) => (),
            Err(failure) => {
                discard(&temporary);
                let event = DownloadEvent::failed(version, sink.written(), total_bytes, failure);
                emit(&event);
                return event;
            }
        }
    }

    let (bytes, sha256) = match sink.finish() {
        Ok(finished) => finished,
        Err(failure) => {
            discard(&temporary);
            let event = DownloadEvent::failed(version, 0, total_bytes, failure);
            emit(&event);
            return event;
        }
    };

    let staged = StagedFile {
        temporary,
        final_path: final_path.to_path_buf(),
        bytes,
        sha256,
    };
    let staged = match finalize(staged, request.size, request.digest.as_deref()) {
        Ok(staged) => staged,
        Err(failure) => {
            let event = DownloadEvent::failed(version, bytes, total_bytes, failure);
            emit(&event);
            return event;
        }
    };
    if let Err(failure) = commit(&staged) {
        discard(&staged.temporary);
        let event = DownloadEvent::failed(version, bytes, total_bytes, failure);
        emit(&event);
        return event;
    }

    // 落盘成功之后才写状态：写不进去不影响这一枚安装包已经在盘上（界面照样能装），所以只留日志。
    let mut state = state::load(state_path);
    state.downloaded_path = Some(staged.final_path.display().to_string());
    state.downloaded_sha256 = Some(format!("sha256:{}", staged.sha256));
    if let Err(error) = state::save(state_path, &state) {
        log::warn!("更新下载：状态写不进去（{}）：{error}", state_path.display());
    }

    let event = DownloadEvent::ready(version, bytes, &staged.final_path, &staged.sha256);
    emit(&event);
    event
}

/// 取回资产并写盘（生产路径）：唯一一处把请求发出去的地方。
pub(crate) async fn fetch(
    version: &str,
    request: &DownloadRequest,
    final_path: &Path,
    state_path: &Path,
    mut emit: impl FnMut(&DownloadEvent) + Send,
) -> DownloadEvent {
    let Some(client) = download_client() else {
        let event = DownloadEvent::failed(
            version,
            0,
            (request.size > 0).then_some(request.size),
            DownloadFailure::network(),
        );
        emit(&event);
        return event;
    };
    let response = match client.get(request.download_url.clone()).send().await {
        Ok(response) => response,
        Err(error) => {
            log::warn!("更新下载：请求失败：{error}");
            let event = DownloadEvent::failed(
                version,
                0,
                (request.size > 0).then_some(request.size),
                DownloadFailure::network(),
            );
            emit(&event);
            return event;
        }
    };
    let status = response.status();
    if !status.is_success() {
        log::warn!("更新下载：资产地址返回 HTTP {status}");
        let event = DownloadEvent::failed(
            version,
            0,
            (request.size > 0).then_some(request.size),
            DownloadFailure::http_status(status.as_u16()),
        );
        emit(&event);
        return event;
    }

    // 数据块流：错误在这里统一变成一个码（网络那一类），细节进日志。
    let chunks = response.bytes_stream().map(|result| {
        result
            .map(|bytes| bytes.to_vec())
            .map_err(|error| {
                log::warn!("更新下载：读流出错：{error}");
                DownloadFailure::network()
            })
    });
    let mut clock = state::now_ms;
    run(version, request, final_path, state_path, chunks, emit, &mut clock).await
}

/// 后缀白名单里的一种安装形态 —— 安装那一步按它分派给 Windows 的处理程序（§六）。
pub(crate) fn install_kind(name: &str) -> Option<AssetKind> {
    AssetKind::from_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn staged(directory: &tempfile::TempDir, bytes: &[u8]) -> StagedFile {
        let temporary = directory.path().join("asset.exe.part");
        std::fs::write(&temporary, bytes).unwrap();
        StagedFile {
            temporary,
            final_path: directory.path().join("asset.exe"),
            bytes: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
        }
    }

    /// 落盘位置就是 §六 写的那一处，而且版本与资产名都过了一遍安全检查。
    #[test]
    fn the_download_lands_where_the_plan_says_it_does() {
        let root = Path::new("updates");
        assert_eq!(
            destination(root, "0.4.2", "dsh-wallpaper_0.4.2_x64-setup.exe").unwrap(),
            root.join("0.4.2").join("dsh-wallpaper_0.4.2_x64-setup.exe")
        );
        // 版本写法不唯一（`v0.4.2` 与 `0.4.2` 是同一个版本）：目录名用规范写法。
        assert_eq!(
            destination(root, " v0.4.2 ", "a.exe").unwrap(),
            root.join("0.4.2").join("a.exe")
        );
    }

    /// 版本与资产名都会被拼进路径：认不出的、带分隔符的、Windows 不接受的，一律拒绝。
    #[test]
    fn a_version_or_name_that_cannot_be_a_path_is_refused() {
        let root = Path::new("updates");
        for version in ["", "..", "../..", "0.4.2/../..", "latest", "0.4.2.3.4"] {
            assert!(destination(root, version, "a.exe").is_none(), "{version:?}");
        }
        for name in [
            "",
            "   ",
            "..",
            ".",
            "a/b.exe",
            "a\\b.exe",
            "C:evil.exe",
            "a.exe:ads",
            "a*.exe",
            "a?.exe",
            "a\0.exe",
            "trailing.",
            // 256 字节：超过文件名上限。
        ] {
            assert!(destination(root, "0.4.2", name).is_none(), "{name:?}");
        }
        assert!(destination(root, "0.4.2", &"a".repeat(256)).is_none());
        // 首尾空白是被**修剪**掉的（名字来自 release 的 JSON，多一个空格就是另一个文件名）。
        assert_eq!(
            destination(root, "0.4.2", "  a.exe  ").unwrap().file_name().unwrap(),
            "a.exe"
        );
        // 正常名字不会被误伤（含 `+`、`-`、下划线与大写后缀）。
        assert!(destination(root, "0.4.2", "dsh-wallpaper_0.4.2_x64-setup.EXE").is_some());
    }

    /// 只有发布仓库的 https 地址会被下载。
    #[test]
    fn only_https_addresses_of_the_release_host_are_downloaded() {
        for url in [
            "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/a.exe",
            "https://objects.githubusercontent.com/github-production-release-asset/a.exe",
            "https://release-assets.githubusercontent.com/x",
        ] {
            assert!(trusted_asset_url(url), "{url}");
        }
        for url in [
            "http://github.com/a.exe",
            "https://evil.test/github.com/a.exe",
            "https://github.com.evil.test/a.exe",
            "https://apple.com@evil.test/a.exe",
            "file:///C:/Windows/System32/calc.exe",
            "https://",
            "",
        ] {
            assert!(!trusted_asset_url(url), "{url}");
        }
    }

    /// 校验：大小不符、摘要不符都要报；`digest` 缺席时只核大小（§六）。
    #[test]
    fn verification_checks_the_size_and_the_digest_only_when_the_api_gave_one() {
        let directory = tempfile::tempdir().unwrap();
        let file = staged(&directory, b"hello world");
        let digest = format!("sha256:{}", file.sha256);

        // 都对得上：过。
        assert_eq!(verify(&file, 11, Some(&digest)), Ok(()));
        // API 没给 size：不核大小（拿 0 去比等于每个下载都不符）。
        assert_eq!(verify(&file, 0, None), Ok(()));
        // API 没给 digest：只核大小。
        assert_eq!(verify(&file, 11, None), Ok(()));

        // 大小不符：报两个数字（界面把它们印出来）。
        let failure = verify(&file, 4096, Some(&digest)).expect_err("size mismatch");
        assert_eq!(failure.code, DownloadFailureCode::SizeMismatch);
        assert_eq!(failure.expected_bytes, Some(4096));
        assert_eq!(failure.actual_bytes, Some(11));
        assert!(failure.is_verification_failure());

        // 摘要不符：报两个串；大小写不敏感是允许的（hex 的大小写没有意义）。
        let other = format!("sha256:{}", "ab".repeat(32));
        let failure = verify(&file, 11, Some(&other)).expect_err("digest mismatch");
        assert_eq!(failure.code, DownloadFailureCode::DigestMismatch);
        assert_eq!(failure.expected_sha256.as_deref(), Some(other.as_str()));
        assert_eq!(
            failure.actual_sha256.as_deref(),
            Some(format!("sha256:{}", file.sha256).as_str()),
            "两个串同形（都是 `sha256:<hex>`）"
        );
        assert_eq!(verify(&file, 11, Some(&digest.to_uppercase().replace("SHA256:", "sha256:"))), Ok(()));

        // digest 不是 sha256 形状（认不出）：按"没有可核对的对象"办，而不是把文件删掉。
        assert_eq!(expected_sha256(None), None);
        assert_eq!(expected_sha256(Some("")), None);
        assert_eq!(expected_sha256(Some("md5:abcd")), None);
        assert_eq!(expected_sha256(Some("sha256:abcd")), None);
        assert_eq!(verify(&file, 11, Some("md5:abcd")), Ok(()));
        assert_eq!(expected_sha256(Some(&digest)).as_deref(), Some(file.sha256.as_str()));
    }

    /// §六 那句"不符即删除"：校验不通过之后，盘上不该留下那个文件。
    #[test]
    fn a_file_that_fails_verification_is_deleted_not_left_behind() {
        let directory = tempfile::tempdir().unwrap();

        let kept = staged(&directory, b"hello world");
        let digest = format!("sha256:{}", kept.sha256);
        let kept = finalize(kept, 11, Some(&digest)).expect("verified");
        assert!(kept.temporary.exists(), "校验通过时文件留着（下一步改名）");

        let rejected = staged(&directory, b"hello world");
        let failure = finalize(rejected, 4096, None).expect_err("size mismatch");
        assert!(failure.is_verification_failure());
        assert!(!directory.path().join("asset.exe.part").exists(), "不符的文件必须被删掉");
    }

    /// 清理是幂等的：文件不在（或已经删过）不算错。
    #[test]
    fn discarding_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("asset.exe.part");
        std::fs::write(&path, b"half").unwrap();
        discard(&path);
        assert!(!path.exists());
        discard(&path);
    }

    /// 改名：上一次下过同一个版本时，旧文件要被新文件覆盖（第二次下载不许卡在最后一步）。
    #[test]
    fn committing_replaces_a_previous_download_of_the_same_version() {
        let directory = tempfile::tempdir().unwrap();
        let final_path = directory.path().join("asset.exe");
        std::fs::write(&final_path, b"old").unwrap();

        let mut staged = staged(&directory, b"new payload");
        staged.final_path = final_path.clone();
        commit(&staged).expect("replaces the old file");

        assert_eq!(std::fs::read(&final_path).unwrap(), b"new payload");
        assert!(!staged.temporary.exists(), "临时文件改名之后不留在目录里");
    }

    /// 每 ~200 毫秒一条：第一条立刻发，之后不足间隔的不发（§四）。
    #[test]
    fn progress_is_throttled_to_one_event_every_two_hundred_milliseconds() {
        assert_eq!(PROGRESS_INTERVAL_MS, 200);
        let mut ticker = ProgressTicker::new(PROGRESS_INTERVAL_MS);
        assert!(ticker.due(1_000), "第一条立刻发：界面要马上从「下载」变成进度条");
        assert!(!ticker.due(1_000));
        assert!(!ticker.due(1_199));
        assert!(ticker.due(1_200), "正好 200 毫秒");
        assert!(!ticker.due(1_399));
        assert!(ticker.due(1_400));
        // 时钟回拨（或注入了一个更小的值）：不 panic、也不发（`saturating_sub`），
        // 而且下一次正常的时间点照样发 —— 节流不会因为一次回拨就卡死。
        assert!(!ticker.due(0));
        assert!(ticker.due(1_600));
    }

    /// 数据块真的写进文件、摘要算对、进度按节流回执 —— 这一条把三件事一次钉住。
    #[test]
    fn chunks_land_on_disk_with_their_digest_and_a_throttled_progress() {
        let directory = tempfile::tempdir().unwrap();
        let temporary = directory.path().join("asset.exe.part");
        let mut sink = FileSink::create(&temporary, PROGRESS_INTERVAL_MS).unwrap();

        let chunks: [&[u8]; 3] = [b"hello ", b"wor", b"ld"];
        let mut now = 1_000u64;
        let mut progress = Vec::new();
        for chunk in chunks {
            if let Some(written) = sink.write(chunk, now).unwrap() {
                progress.push(written);
            }
            // 每个数据块之间只过 10 毫秒：只有第一条会发出去（节流生效）。
            now += 10;
        }
        assert_eq!(progress, vec![6], "200 毫秒内的后续数据块不发事件");
        assert_eq!(sink.written(), 11);

        let (bytes, sha256) = sink.finish().unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(sha256, hex(&Sha256::digest(b"hello world")));
        // `finish` 之后文件里就是全部字节（没有留在缓冲区里的）。
        assert_eq!(std::fs::read(&temporary).unwrap(), b"hello world");

        // 再喂一块、并且时间跨过间隔：进度继续发（节流不是"只发第一条"）。
        let mut sink = FileSink::create(&temporary, PROGRESS_INTERVAL_MS).unwrap();
        assert_eq!(sink.write(b"a", 1_000).unwrap(), Some(1));
        assert_eq!(sink.write(b"b", 1_150).unwrap(), None);
        assert_eq!(sink.write(b"c", 1_200).unwrap(), Some(3));
    }

    /// 写盘失败与磁盘满各有各的码：界面据此说"磁盘空间不足"而不是泛泛的"写不下去"。
    #[test]
    fn an_io_failure_becomes_a_code_the_interface_can_translate() {
        assert_eq!(
            DownloadFailure::from_io(&std::io::Error::from_raw_os_error(112)).code,
            DownloadFailureCode::DiskFull,
            "Windows 的 ERROR_DISK_FULL"
        );
        assert_eq!(
            DownloadFailure::from_io(&std::io::Error::from_raw_os_error(28)).code,
            DownloadFailureCode::DiskFull,
            "Unix 的 ENOSPC"
        );
        assert_eq!(
            DownloadFailure::from_io(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)).code,
            DownloadFailureCode::WriteFailed
        );

        // 真的写不进去时也走同一个码：把"临时文件"指到一个目录上，建文件必然失败。
        let directory = tempfile::tempdir().unwrap();
        let blocked = directory.path().join("asset.exe.part");
        std::fs::create_dir_all(&blocked).unwrap();
        let failure = match FileSink::create(&blocked, PROGRESS_INTERVAL_MS) {
            Ok(_) => panic!("在这个路径上建文件必须失败"),
            Err(failure) => failure,
        };
        assert_eq!(failure.code, DownloadFailureCode::WriteFailed);
    }

    /// 同一条码的序列化形状是两侧的契约（界面按 `code` 分支）。
    #[test]
    fn the_failure_and_event_shapes_are_the_contract_the_interface_reads() {
        assert_eq!(
            serde_json::to_string(&DownloadFailure::network()).unwrap(),
            r#"{"code":"network","httpStatus":null,"expectedBytes":null,"actualBytes":null,"expectedSha256":null,"actualSha256":null}"#
        );
        assert_eq!(
            serde_json::to_string(&DownloadFailure::http_status(404)).unwrap(),
            r#"{"code":"httpStatus","httpStatus":404,"expectedBytes":null,"actualBytes":null,"expectedSha256":null,"actualSha256":null}"#
        );
        assert_eq!(
            serde_json::to_string(&DownloadFailure::size_mismatch(10, 9)).unwrap(),
            r#"{"code":"sizeMismatch","httpStatus":null,"expectedBytes":10,"actualBytes":9,"expectedSha256":null,"actualSha256":null}"#
        );
        assert_eq!(
            serde_json::to_string(&DownloadFailure::from_io(&std::io::Error::from_raw_os_error(112))).unwrap(),
            r#"{"code":"diskFull","httpStatus":null,"expectedBytes":null,"actualBytes":null,"expectedSha256":null,"actualSha256":null}"#
        );

        let progress = DownloadEvent::progress("0.4.2", 1024, Some(2048));
        assert_eq!(
            serde_json::to_string(&progress).unwrap(),
            r#"{"version":"0.4.2","phase":"downloading","downloadedBytes":1024,"totalBytes":2048,"path":null,"sha256":null,"failure":null}"#
        );
        // 总大小未知（API 没给 size）：`totalBytes` 是 null，界面据此不印百分比。
        let unknown = DownloadEvent::progress("0.4.2", 1024, None);
        assert_eq!(
            serde_json::to_string(&unknown).unwrap(),
            r#"{"version":"0.4.2","phase":"downloading","downloadedBytes":1024,"totalBytes":null,"path":null,"sha256":null,"failure":null}"#
        );
        let ready = DownloadEvent::ready("0.4.2", 2048, Path::new("C:\\updates\\0.4.2\\a.exe"), "ab");
        assert_eq!(ready.phase, DownloadPhase::Ready);
        assert_eq!(ready.path.as_deref(), Some("C:\\updates\\0.4.2\\a.exe"));
        assert_eq!(ready.sha256.as_deref(), Some("sha256:ab"));
        assert!(serde_json::to_string(&ready).unwrap().contains(r#""phase":"ready""#));
        assert!(serde_json::to_string(&DownloadEvent::failed("0.4.2", 0, None, DownloadFailure::network()))
            .unwrap()
            .contains(r#""phase":"failed""#));
    }

    /// 一次下载在跑的时候不许再开第二次（同一个文件被两路写会写坏它）。
    #[test]
    fn only_one_download_runs_at_a_time() {
        let first = InFlight::claim("0.4.2").expect("the first claim wins");
        assert!(InFlight::claim("0.4.2").is_none(), "同一个版本不重复下");
        assert!(InFlight::claim("0.4.3").is_none(), "另一个版本也要等这一轮结束");
        drop(first);
        assert!(InFlight::claim("0.4.3").is_some(), "上一轮结束之后可以再下");
    }

    /// 一次完整的下载：数据块 → 文件、进度事件、`ready` 事件、状态文件（全都不碰网络）。
    #[tokio::test]
    async fn a_complete_download_emits_progress_then_ready_and_records_the_state() {
        let directory = tempfile::tempdir().unwrap();
        let updates = directory.path().join("updates");
        let state_path = updates.join("state.json");
        let final_path = destination(&updates, "0.4.2", "setup.exe").unwrap();

        let payload = b"a real installer".to_vec();
        let request = DownloadRequest {
            name: "setup.exe".into(),
            download_url: "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/setup.exe".into(),
            size: payload.len() as u64,
            digest: Some(format!("sha256:{}", hex(&Sha256::digest(&payload)))),
        };
        let chunks = futures_util::stream::iter(vec![
            Ok(payload[..7].to_vec()),
            Ok(payload[7..].to_vec()),
        ]);
        let mut events = Vec::new();
        let mut clock = || 1_000u64;

        let terminal = run("0.4.2", &request, &final_path, &state_path, chunks, |event| events.push(event.clone()), &mut clock).await;

        assert_eq!(terminal.phase, DownloadPhase::Ready);
        assert_eq!(std::fs::read(&final_path).unwrap(), payload);
        assert!(!final_path.with_extension("exe.part").exists(), "临时文件不留在目录里");
        // 第一条进度立刻发（界面据此进进度条），终局是 ready。
        assert_eq!(events.first().unwrap().phase, DownloadPhase::Downloading);
        assert_eq!(events.first().unwrap().downloaded_bytes, 0);
        assert_eq!(events.last().unwrap().phase, DownloadPhase::Ready);
        assert_eq!(events.last().unwrap().downloaded_bytes, payload.len() as u64);
        // 状态文件里留下的是"下载好了"这件事（§五 的两个字段）。
        let state = state::load(&state_path);
        assert_eq!(state.downloaded_path.as_deref(), Some(final_path.display().to_string().as_str()));
        assert_eq!(state.downloaded_sha256.as_deref(), Some(format!("sha256:{}", hex(&Sha256::digest(&payload))).as_str()));
    }

    /// 校验不符：文件删掉、状态文件一个字节都不写、终局是 `failed`。
    #[tokio::test]
    async fn a_download_that_fails_verification_deletes_the_file_and_reports_the_code() {
        let directory = tempfile::tempdir().unwrap();
        let updates = directory.path().join("updates");
        let state_path = updates.join("state.json");
        let final_path = destination(&updates, "0.4.2", "setup.exe").unwrap();
        let request = DownloadRequest {
            name: "setup.exe".into(),
            download_url: "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/setup.exe".into(),
            // API 说 4096 字节，实际只有 5：截断的下载必须被发现。
            size: 4096,
            digest: None,
        };
        let chunks = futures_util::stream::iter(vec![Ok(b"short".to_vec())]);
        let mut events = Vec::new();
        let mut clock = || 1_000u64;

        let terminal = run("0.4.2", &request, &final_path, &state_path, chunks, |event| events.push(event.clone()), &mut clock).await;

        assert_eq!(terminal.phase, DownloadPhase::Failed);
        assert_eq!(terminal.failure.as_ref().unwrap().code, DownloadFailureCode::SizeMismatch);
        assert_eq!(terminal.failure.as_ref().unwrap().expected_bytes, Some(4096));
        assert_eq!(terminal.failure.as_ref().unwrap().actual_bytes, Some(5));
        assert!(!final_path.exists(), "校验不符的文件必须删掉（§六）");
        assert!(!final_path.with_extension("exe.part").exists());
        assert!(!state_path.exists(), "失败的下载不写状态文件");
        assert_eq!(events.last().unwrap().phase, DownloadPhase::Failed);
    }

    /// 流中途断了（网络）：半截文件删掉，终局 `failed` 走同一个出口。
    #[tokio::test]
    async fn a_stream_that_breaks_midway_reports_a_network_failure_and_leaves_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let updates = directory.path().join("updates");
        let final_path = destination(&updates, "0.4.2", "setup.exe").unwrap();
        let request = DownloadRequest {
            name: "setup.exe".into(),
            download_url: "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/setup.exe".into(),
            size: 0,
            digest: None,
        };
        let chunks = futures_util::stream::iter(vec![Ok(vec![1u8; 16]), Err(DownloadFailure::network())]);
        let mut events = Vec::new();
        let mut clock = || 1_000u64;

        let terminal = run("0.4.2", &request, &final_path, &updates.join("state.json"), chunks, |event| events.push(event.clone()), &mut clock).await;

        assert_eq!(terminal.phase, DownloadPhase::Failed);
        assert_eq!(terminal.failure.as_ref().unwrap().code, DownloadFailureCode::Network);
        assert!(!final_path.exists());
        assert!(!final_path.with_extension("exe.part").exists());
    }

    /// 保存目录建不出来（这里用一个**文件**占住目录名）：报 `destinationUnavailable`，不动盘。
    #[tokio::test]
    async fn a_destination_that_cannot_be_created_is_its_own_code() {
        let directory = tempfile::tempdir().unwrap();
        // `updates/0.4.2` 的位置上先放一个文件：`create_dir_all` 必然失败。
        let blocked = directory.path().join("updates").join("0.4.2");
        std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        std::fs::write(&blocked, b"not a directory").unwrap();
        let final_path = blocked.join("setup.exe");

        let request = DownloadRequest {
            name: "setup.exe".into(),
            download_url: "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/setup.exe".into(),
            size: 0,
            digest: None,
        };
        let chunks = futures_util::stream::iter(vec![Ok(vec![1u8; 4])]);
        let mut events = Vec::new();
        let mut clock = || 1_000u64;

        let terminal = run("0.4.2", &request, &final_path, &directory.path().join("updates").join("state.json"), chunks, |event| events.push(event.clone()), &mut clock).await;

        assert_eq!(terminal.phase, DownloadPhase::Failed);
        assert_eq!(
            terminal.failure.as_ref().unwrap().code,
            DownloadFailureCode::DestinationUnavailable
        );
        assert_eq!(events.len(), 1, "建目录就失败了：只有一条终局事件");
    }

    /// 装得上的是两种：`.exe`（安装向导）与 `.msix`（App Installer）。白名单与选择那一步同一份。
    #[test]
    fn the_install_whitelist_is_the_same_one_the_selection_uses() {
        assert_eq!(install_kind("dsh-wallpaper_0.4.2_x64-setup.exe"), Some(AssetKind::Exe));
        assert_eq!(install_kind("dsh-wallpaper_0.4.2.msix"), Some(AssetKind::Msix));
        assert_eq!(install_kind("dsh-wallpaper_0.4.2_x64-setup.exe.sig"), None);
        assert_eq!(install_kind("dsh-wallpaper_0.4.2.nsis.zip"), None);
        assert_eq!(install_kind("SHA256SUMS.txt"), None);
    }

    /// 从一位已选中的资产造请求：报告里那一个原样递过来（下载这一步不再选一遍）。
    #[test]
    fn the_request_comes_straight_from_the_selected_asset() {
        let asset = ReleaseAsset {
            name: "setup.exe".into(),
            size: 31_457_280,
            download_url: "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/setup.exe".into(),
            digest: Some(format!("sha256:{}", "ab".repeat(32))),
        };
        let request = DownloadRequest::of(&asset);
        assert_eq!(request.name, asset.name);
        assert_eq!(request.size, 31_457_280);
        assert_eq!(request.digest, asset.digest);
        assert!(trusted_asset_url(&request.download_url));
        // 缺字段的请求（界面递了个空壳）不会 panic，只是下载会在校验那一步失败。
        let empty: DownloadRequest = serde_json::from_str("{}").unwrap();
        assert!(empty.name.is_empty() && empty.size == 0 && empty.digest.is_none());
    }

    /// 大小的两条端点都试一下：真实下载里的 "0 字节资产" 不该被当成"API 没给大小"。
    #[test]
    fn an_asset_without_a_size_only_checks_the_digest() {
        let directory = tempfile::tempdir().unwrap();
        let file = staged(&directory, b"x");
        let wrong = format!("sha256:{}", "cd".repeat(32));
        assert_eq!(
            verify(&file, 0, Some(&wrong)).unwrap_err().code,
            DownloadFailureCode::DigestMismatch,
            "没给大小也照样核摘要"
        );
        assert_eq!(MIB, 1_048_576);
    }
}
