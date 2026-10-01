//! 更新检测：原生侧（第一片）＋ 下载与安装（第三片）。
//!
//! 依据 `docs/plans/wallpaper-update-detection-plan.md`。第一片只做"查"：读本机版本（按安装
//! 形态分派）、解析 release 标签并比较、打一次 GitHub API、把状态落盘；第三片接上"下"与"装"：
//! 流式下载 + 进度事件 + 校验 + 把文件交给 Windows 的处理程序。界面在 `wallpaper/src/features/update/`。
//!
//! 四条贯穿整片的规矩：
//!
//!  - **宁可漏报，不可误报**（计划书 §三）：标签解析不出、版本读不到、资产选不到，一律按
//!    "没有新版本"或"不检查"处理，绝不猜一个数字出来；
//!  - **与安装形态无关**（§3.1）：MSIX 与 NSIS 各有一条版本读取器，安装那一步按资产后缀分派，
//!    所以将来换打包方式只改 release、不改代码；
//!  - **界面自己说文案**（§四、`wallpaper/src/i18n/`）：命令只回码、数字、版本号与地址，
//!    一句中文句子都不回；
//!  - **不在检查时下载**（§六）：下载只由界面按下「下载」那一次发起（`update_download`），
//!    `run_check` 这条路上没有任何下载调用。
//!
//! 模块划分：`version`（版本与安装形态）、`release`（release 文档与资产选择）、`source`（唯一
//! 碰网络的地方）、`download`（流式下载、校验与进度事件）、`state`（状态文件与 6 小时节流）、
//! `commands`（命令与整条流程）。
mod release;
mod source;
mod state;
mod version;

// 命令所在的那个模块必须按名字可达：`generate_handler![update::commands::update_check]` 会去
// `update::commands` 里找 `#[tauri::command]` 生成的辅助项（与 `appearance::commands` 同理）。
// 整块只在完整版里存在（`mod update` 在 `lib.rs` 里本身就按 edition 门控），所以这里不再逐项加
// `#[cfg(not(feature = "lite"))]`：那只会让人以为存在"Lite 也能编译这个模块"的情形。
pub(crate) mod commands;
pub(crate) mod download;

use serde::{Deserialize, Serialize};

/// 本应用在 `tauri.conf.json` 里的包标识符。
///
/// 状态文件的目录名就是它（§五 的 `%LOCALAPPDATA%\com.dsh.wallpaper\updates\state.json`），
/// 所以改清单里的 `identifier` 就必须改这一行：两处不一致会让状态写进一个没人读的目录。
pub(crate) const APP_IDENTIFIER: &str = "com.dsh.wallpaper";

/// 更新源：GitHub Releases 上的这个仓库（§一 规矩 1：更新源就是它，不自建）。
pub(crate) const RELEASE_REPOSITORY: &str = "kyorakuyk/dsh-wallpaper";

/// 一次检查的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CheckOutcome {
    /// 有更新的版本，并且选到了一个可安装的资产。
    UpdateAvailable,
    /// 已是最新。**也包括**标签解析不出的情况：那是"没有新版本"，不是错误（§三）。
    UpToDate,
    /// 有更新的版本，但 release 里没有可安装的资产 ⇒ 界面回落到"打开 release 页面"（§3.1）。
    NoInstallableAsset,
    /// 这次没有真正检查，原因见 `skipReason`。
    Skipped,
    /// 检查失败，原因见 `failure`。
    Failed,
}

/// 没检查的原因码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SkipReason {
    /// 距上次检查不足 6 小时（§五）。手动检查不会走到这里。
    Throttled,
    /// 读不到本机版本 ⇒ 不检查，而不是猜（§3.1、§八 9）。
    VersionUnavailable,
}

/// 检查失败的原因码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FailureCode {
    /// 请求没到：DNS、TLS、超时、没有网络。
    Network,
    /// 服务器答了，但不是 2xx。403 常见于缺少 User-Agent（§二），匿名限流也是它。
    HttpStatus,
    /// 答了 2xx，但正文不是我们能读的 release 文档。
    MalformedResponse,
}

/// 失败原因：一个码加一个数字。
///
/// 只有码和数字，没有句子 —— 文案由界面按语言说。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureReport {
    pub code: FailureCode,
    /// `code == HttpStatus` 时是响应码；其余情况为 `null`。
    pub http_status: Option<u16>,
}

impl FailureReport {
    pub(crate) fn network() -> Self {
        Self {
            code: FailureCode::Network,
            http_status: None,
        }
    }

    pub(crate) fn http_status(status: u16) -> Self {
        Self {
            code: FailureCode::HttpStatus,
            http_status: Some(status),
        }
    }

    pub(crate) fn malformed() -> Self {
        Self {
            code: FailureCode::MalformedResponse,
            http_status: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FailureReport, APP_IDENTIFIER};

    /// 界面按 `code` 分支、按 `httpStatus` 拼句子，所以这两个词的拼法是一份契约。
    #[test]
    fn the_report_serializes_to_the_codes_the_interface_branches_on() {
        assert_eq!(
            serde_json::to_string(&FailureReport::network()).unwrap(),
            r#"{"code":"network","httpStatus":null}"#
        );
        assert_eq!(
            serde_json::to_string(&FailureReport::http_status(403)).unwrap(),
            r#"{"code":"httpStatus","httpStatus":403}"#
        );
        assert_eq!(
            serde_json::to_string(&FailureReport::malformed()).unwrap(),
            r#"{"code":"malformedResponse","httpStatus":null}"#
        );
    }

    /// 状态文件的目录名来自这个常量，而它必须与清单里的 `identifier` 逐字一致：不一致只会让状态
    /// 写进一个没人读的目录，什么错都不报。
    #[test]
    fn the_app_identifier_matches_the_manifest() {
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
        )
        .expect("tauri.conf.json must exist");
        let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("valid JSON");
        assert_eq!(
            manifest
                .get("identifier")
                .and_then(serde_json::Value::as_str),
            Some(APP_IDENTIFIER)
        );
    }
}
