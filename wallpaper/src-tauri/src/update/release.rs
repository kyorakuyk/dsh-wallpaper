//! release 文档与资产选择（计划书 §三、§3.1、§六）。
//!
//! 两个纯函数：把 GitHub 的回答读成我们能用的形状，以及按**后缀白名单**挑一个可安装资产。
//! 两处都不碰网络、不碰磁盘，单测直接喂字符串。

use serde::{Deserialize, Serialize};

use super::FailureReport;

/// 后缀白名单，以及每个后缀属于哪种安装形态。
///
/// 按**后缀**判定而不是写死文件名：换打包方式（`.exe` 的 NSIS、`.msix` 的打包形态）只需要改
/// release，不需要改代码（§3.1）。`.msixbundle`、`.sig`、`.blockmap`、`.zip`、`.cer` 都不在里面，
/// 因此都选不上 —— 更新场景下证书早就被信任了（§三）。
const INSTALLABLE_SUFFIXES: [(&str, AssetKind); 2] = [(".msix", AssetKind::Msix), (".exe", AssetKind::Exe)];

/// 资产属于哪一种安装形态。
///
/// 这一层只是"名字像什么"；真正决定交给 Windows 哪一个处理程序的是安装那一步（`update_install`）：
/// `.msix` 走 App Installer，`.exe` 走安装向导（§六）。序列化成 `"exe"` / `"msix"`：安装报告里那
/// 个 `kind` 字段就是它，界面据此说"交给哪个处理程序"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AssetKind {
    Msix,
    Exe,
}

impl AssetKind {
    /// 名字决定形态；大小写不敏感（`.EXE` 也算，Windows 上文件名不区分大小写）。
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        INSTALLABLE_SUFFIXES
            .iter()
            .find(|(suffix, _)| lower.ends_with(suffix))
            .map(|(_, kind)| *kind)
    }
}

/// release 里的一个资产。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseAsset {
    pub name: String,
    /// 字节数。API 没给这个字段就是 0；下载那一步据此决定"要不要核对大小"（§六）。
    pub size: u64,
    pub download_url: String,
    /// GitHub 的 `digest`（形如 `sha256:<hex>`）；老响应没有这个字段就是 `null`。
    pub digest: Option<String>,
}

/// 一次 `releases/latest` 的回答里我们关心的部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseInfo {
    /// 标签原文（`v0.4.1`）。它是不是一个能比较的版本，由
    /// [`super::version::Version::parse_tag`] 说。
    pub tag: Option<String>,
    /// release 页面。资产缺席或下载失败时界面回落到"打开 release 页面"（§3.1、§六）。
    pub page_url: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

/// GitHub 的响应形状：只列我们用的字段。
///
/// 缺字段一律按"没有"处理（`#[serde(default)]`），多出来的字段不看 —— 将来 GitHub 加字段不会
/// 让更新检测变成"解析失败"。
#[derive(Deserialize)]
struct ReleaseDocument {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<AssetDocument>,
}

#[derive(Deserialize)]
struct AssetDocument {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    browser_download_url: Option<String>,
    #[serde(default)]
    digest: Option<String>,
}

/// 解析 `releases/latest` 的正文。
///
/// 只有"正文根本不是 JSON 对象 / 字段类型不对"算失败（`MalformedResponse`）；字段缺了不算失败
/// —— 缺 `tag_name` 的 release 会在比较那一步变成"没有新版本"（§三：宁可漏报，不可误报）。
pub(crate) fn parse_release(json: &str) -> Result<ReleaseInfo, FailureReport> {
    // 先要求"是一个对象"：serde 的结构体反序列化对空数组是**成功**的（所有字段都取默认值），
    // 而一个代理或登录页回一段 `[]` 时，那不该被读成"已是最新"（§八 7：失败要看得见）。
    let value: serde_json::Value = serde_json::from_str(json).map_err(|error| {
        log::warn!("更新检查：release 文档无法解析：{error}");
        FailureReport::malformed()
    })?;
    if !value.is_object() {
        log::warn!("更新检查：release 文档不是一个对象");
        return Err(FailureReport::malformed());
    }
    let document: ReleaseDocument = serde_json::from_value(value).map_err(|error| {
        log::warn!("更新检查：release 文档的字段形状不对：{error}");
        FailureReport::malformed()
    })?;

    let assets = document
        .assets
        .into_iter()
        .filter_map(|asset| {
            // 没有名字或没有下载地址的资产是装不上的，直接不列进来（也就不可能被选中）。
            let name = asset.name?.trim().to_string();
            let download_url = asset.browser_download_url?.trim().to_string();
            (!name.is_empty() && !download_url.is_empty()).then(|| ReleaseAsset {
                name,
                size: asset.size.unwrap_or(0),
                download_url,
                digest: asset.digest.filter(|digest| !digest.trim().is_empty()),
            })
        })
        .collect();

    Ok(ReleaseInfo {
        tag: document.tag_name,
        page_url: document.html_url,
        assets,
    })
}

/// 挑一个可安装资产（§3.1 的"按后缀分派"在选择这一步的影子）。
///
/// 先挑与**当前安装形态**同类的（打包态给 `.msix`，非打包给 `.exe`），同类没有才给另一种：
/// 两种包都挂在同一枚 release 上时，用户拿到的是他能直接装的那一个。同类里取 API 给出的第一个，
/// 代码不去猜"哪个更好" —— 资产顺序是发布者定的。
pub(crate) fn select_asset(
    assets: &[ReleaseAsset],
    preferred: Option<AssetKind>,
) -> Option<&ReleaseAsset> {
    let order = match preferred {
        Some(AssetKind::Exe) => [AssetKind::Exe, AssetKind::Msix],
        // 形态未知时先给打包形态：`MSIX` 那条路（App Installer 或商店）是签名与分发都最稳的
        // 一条，这也是 §六 原本的写法。
        _ => [AssetKind::Msix, AssetKind::Exe],
    };
    order
        .into_iter()
        .find_map(|kind| assets.iter().find(|asset| AssetKind::from_name(&asset.name) == Some(kind)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, size: u64, digest: Option<&str>) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            size,
            download_url: format!("https://example.test/{name}"),
            digest: digest.map(str::to_string),
        }
    }

    /// 一次真实形状的回答（字段名与 GitHub 一致，资产名是本仓库 CI 的产物名）。
    const RELEASE_JSON: &str = r#"{
        "tag_name": "v0.4.2",
        "html_url": "https://github.com/kyorakuyk/dsh-wallpaper/releases/tag/v0.4.2",
        "assets": [
            {
                "name": "dsh-wallpaper_0.4.2_x64-setup.exe",
                "size": 31457280,
                "browser_download_url": "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/dsh-wallpaper_0.4.2_x64-setup.exe",
                "digest": "sha256:0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"
            },
            {
                "name": "dsh-wallpaper_0.4.2_x64-setup.nsis.zip",
                "size": 30000000,
                "browser_download_url": "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/dsh-wallpaper_0.4.2_x64-setup.nsis.zip"
            },
            {
                "name": "dsh-wallpaper_0.4.2_x64-setup.exe.sig",
                "size": 400,
                "browser_download_url": "https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/dsh-wallpaper_0.4.2_x64-setup.exe.sig"
            }
        ]
    }"#;

    #[test]
    fn reads_the_release_fields_the_check_needs() {
        let release = parse_release(RELEASE_JSON).expect("a well-formed release");
        assert_eq!(release.tag.as_deref(), Some("v0.4.2"));
        assert_eq!(
            release.page_url.as_deref(),
            Some("https://github.com/kyorakuyk/dsh-wallpaper/releases/tag/v0.4.2")
        );
        assert_eq!(release.assets.len(), 3);
        assert_eq!(release.assets[0].name, "dsh-wallpaper_0.4.2_x64-setup.exe");
        assert_eq!(release.assets[0].size, 31_457_280);
        assert!(release.assets[0].digest.as_deref().unwrap().starts_with("sha256:"));
        // 老响应没有 `digest`：那就是没有，不是空串。
        assert!(release.assets[1].digest.is_none());
    }

    #[test]
    fn a_body_that_is_not_a_release_document_is_a_failure() {
        // 空数组是这里唯一需要明说的一条：serde 对结构体的空序列是"成功但全默认"，而一段
        // `[]`（代理、登录页）不该被读成"已是最新" —— 那会让"检查失败"从界面上消失。
        for body in ["", "not json", "[]", "null", r#"{"tag_name": 42}"#] {
            let failure = parse_release(body).expect_err("must not read as a release");
            assert_eq!(failure, FailureReport::malformed(), "{body:?}");
        }
    }

    #[test]
    fn missing_fields_are_not_failures() {
        // 缺 tag_name：交给比较那一步变成"没有新版本"，而不是在这里报错。
        let release = parse_release(r#"{"assets": []}"#).expect("an empty release is still readable");
        assert!(release.tag.is_none());
        assert!(release.page_url.is_none());
        assert!(release.assets.is_empty());
        // 资产缺 size/digest/url：缺 url 的没有可下载的东西，不列进来。
        let release = parse_release(
            r#"{"tag_name":"v0.4.2","assets":[{"name":"dsh-wallpaper_0.4.2_x64-setup.exe"}]}"#,
        )
        .expect("an asset without a url is simply unusable");
        assert!(release.assets.is_empty());
    }

    #[test]
    fn selects_the_setup_executable_by_suffix() {
        let release = parse_release(RELEASE_JSON).unwrap();
        let selected = select_asset(&release.assets, Some(AssetKind::Exe)).expect("the .exe");
        assert_eq!(selected.name, "dsh-wallpaper_0.4.2_x64-setup.exe");
        // `.sig` 与 `.nsis.zip` 都在白名单外：它们后缀不符，不是"差不多"。
        assert!(AssetKind::from_name("dsh-wallpaper_0.4.2_x64-setup.exe.sig").is_none());
        assert!(AssetKind::from_name("dsh-wallpaper_0.4.2_x64-setup.nsis.zip").is_none());
        assert!(AssetKind::from_name("dsh-wallpaper_0.4.2.msixbundle").is_none());
        assert!(AssetKind::from_name("dsh-wallpaper_0.4.2.cer").is_none());
    }

    #[test]
    fn keeps_the_msix_branch_even_though_only_exes_are_published_today() {
        // 将来上商店时 release 里会出现 `.msix`：选择逻辑不许因为"今天只有 exe"就删掉这一支。
        let assets = vec![asset("dsh-wallpaper_0.4.2.msix", 10, None)];
        assert_eq!(select_asset(&assets, None).unwrap().name, "dsh-wallpaper_0.4.2.msix");
        assert_eq!(select_asset(&assets, Some(AssetKind::Exe)).unwrap().name, "dsh-wallpaper_0.4.2.msix");
    }

    #[test]
    fn prefers_the_asset_that_matches_the_installed_form() {
        let assets = vec![
            asset("dsh-wallpaper_0.4.2_x64-setup.exe", 10, None),
            asset("dsh-wallpaper_0.4.2.msix", 20, None),
        ];
        // 打包态拿 `.msix`，非打包态拿 `.exe`：两种形态的包都在时，用户拿到他能直接装的那个。
        assert_eq!(select_asset(&assets, Some(AssetKind::Msix)).unwrap().name, "dsh-wallpaper_0.4.2.msix");
        assert_eq!(select_asset(&assets, Some(AssetKind::Exe)).unwrap().name, "dsh-wallpaper_0.4.2_x64-setup.exe");
        // 形态未知时先给打包形态。
        assert_eq!(select_asset(&assets, None).unwrap().name, "dsh-wallpaper_0.4.2.msix");
    }

    #[test]
    fn a_release_without_an_installable_asset_selects_nothing() {
        let release = parse_release(
            r#"{"tag_name":"v0.4.2","assets":[
                {"name":"dsh-wallpaper_0.4.2.nsis.zip","size":1,"browser_download_url":"https://example.test/a"},
                {"name":"SHA256SUMS.txt","size":1,"browser_download_url":"https://example.test/b"}
            ]}"#,
        )
        .unwrap();
        assert!(select_asset(&release.assets, None).is_none());
    }
}
