//! 取数入口：整个更新检查里**唯一**碰网络的地方（计划书 §二、§九 2）。
//!
//! 它只做一件事：把 `releases/latest` 的正文取回来 —— 成功给字符串，失败给码。所有判断都在
//! [`super::commands::run_check`] 里，那些是纯函数，所以单测可以完全不碰网络（连打桩都不用：
//! 取数入口是**注入**给流程的，测试传自己的闭包）。
//!
//! 换一次取数（比如将来加镜像）就是换这一个函数，判断逻辑一行都不用动。

use std::sync::OnceLock;
use std::time::Duration;

use super::{FailureReport, RELEASE_REPOSITORY};

/// 更新检查必须自报家门：GitHub 的匿名调用没有 `User-Agent` 会直接 403（§二）。
const USER_AGENT: &str = concat!(
    "dsh-wallpaper/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/kyorakuyk/dsh-wallpaper)"
);

/// `releases/latest` 的地址。
///
/// 匿名可用（每小时 60 次，够用），更新源就是这个仓库（§二）。
pub(crate) fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{RELEASE_REPOSITORY}/releases/latest")
}

/// 一次进程一份客户端（与其它探针同一写法）。
fn release_client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(USER_AGENT)
                // 检查更新不该让谁等下去：连不上就算了，界面上留一行结果（§五：不打扰是设计目标）。
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|error| log::warn!("更新检查：HTTP 客户端无法初始化：{error}"))
                .ok()
        })
        .as_ref()
}

/// 取 `releases/latest` 的正文。
///
/// 失败只回一个码加一个数字（HTTP 状态），细节进日志 —— 界面自己按语言说文案。
pub(crate) async fn fetch_latest_release() -> Result<String, FailureReport> {
    let Some(client) = release_client() else {
        return Err(FailureReport::network());
    };
    let response = client
        .get(latest_release_url())
        .send()
        .await
        .map_err(|error| {
            log::warn!("更新检查：请求失败：{error}");
            FailureReport::network()
        })?;
    let status = response.status();
    if !status.is_success() {
        log::warn!("更新检查：release 端点返回 HTTP {status}");
        return Err(FailureReport::http_status(status.as_u16()));
    }
    response.text().await.map_err(|error| {
        log::warn!("更新检查：读取响应正文失败：{error}");
        FailureReport::network()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_the_repositorys_latest_release() {
        assert_eq!(
            latest_release_url(),
            "https://api.github.com/repos/kyorakuyk/dsh-wallpaper/releases/latest"
        );
    }

    #[test]
    fn the_user_agent_names_this_build() {
        // 不带 User-Agent 会被 403（§二），所以这一串的存在本身是被要求的；带上版本号是为了
        // 在 GitHub 的限流日志里分得清哪一版在查。
        assert!(USER_AGENT.starts_with("dsh-wallpaper/"));
        assert!(USER_AGENT.contains(env!("CARGO_PKG_VERSION")));
    }
}
