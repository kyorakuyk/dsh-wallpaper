//! 版本号，以及"本机当前版本"的三条读取器（计划书 §三、§3.1）。
//!
//! 这一层只有两件事：把一个字符串变成一个**可比较**的版本号，以及按安装形态决定去哪儿读本机
//! 版本。读不到就是读不到 —— 没有任何一条路径会去猜一个数字出来。

use std::cmp::Ordering;
use std::fmt;

use serde::Serialize;

use crate::windows_integration::PackageVersionProbe;

/// 段数上限：§三 的标签正则 `^v\d+(\.\d+){0,3}$` 最多四段。
const MAX_SEGMENTS: usize = 4;

/// 一个可比较的版本号。
///
/// 内部恒为四段（缺的段补 0），`written` 只记住"原样写了几段"给显示用：`0.4.1` 与 `0.4.1.0`
/// 是**同一个版本**，但报给界面的字面量应当各按各的写法。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Version {
    digits: [u32; MAX_SEGMENTS],
    written: u8,
}

impl Version {
    /// 解析 release 标签。**只接受** `^v\d+(\.\d+){0,3}$`：小写 `v`、纯十进制段、最多四段，
    /// 前后不许有空白（§三）。调用方对 `None` 的处置是"没有新版本"，所以这里宽一点不会更聪明，
    /// 只会更错。
    pub(crate) fn parse_tag(tag: &str) -> Option<Self> {
        // 正则里的两个锚点在代码里就是"一个字符都不修剪"。
        Self::parse_digits(tag.strip_prefix('v')?)
    }

    /// 解析不带标签前缀的版本串：卸载项的 `DisplayVersion`、包全名里的版本段、exe 的
    /// VERSIONINFO。
    ///
    /// 与标签的区别有两处，都是因为"这些值不是我们写的"：允许首尾空白，允许带 `v`/`V` 前缀
    /// （有的产品就把 `V1.2.3` 写进 VERSIONINFO）。形状仍然只认"一到四段十进制"——
    /// `0.4.1 (build 5)` 这类返回 `None`，不去截前几位猜一个：读不到就不检查（§3.1）。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        Self::parse_digits(text)
    }

    fn parse_digits(text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let mut digits = [0u32; MAX_SEGMENTS];
        let mut written = 0usize;
        for part in text.split('.') {
            if written == MAX_SEGMENTS {
                return None;
            }
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            // 段值溢出 `u32`（`v99999999999`）同样是"解析不出"：宁可漏报。
            digits[written] = part.parse().ok()?;
            written += 1;
        }
        debug_assert!(written > 0);
        Some(Self {
            digits,
            written: written as u8,
        })
    }

    /// 这个版本是否比 `other` 新（缺的段按 0 补齐后逐段比较）。
    pub(crate) fn is_newer_than(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Greater
    }
}

impl fmt::Display for Version {
    /// 按**写下来的段数**输出（`0.4.1` 不写成 `0.4.1.0`）：这一串要进界面，也要能与
    /// `Get-AppxPackage` 显示的版本逐字对上（§八 10）。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::new();
        for (index, digit) in self.digits[..self.written as usize].iter().enumerate() {
            if index > 0 {
                text.push('.');
            }
            text.push_str(&digit.to_string());
        }
        formatter.write_str(&text)
    }
}

// 相等与排序都**只看四段数字**：`0.4.1` 与 `0.4.1.0` 是同一个版本（§三 的补齐规则），所以这几
// 个实现必须手写 —— derive 会把 `written` 也算进去，于是同一个版本有两种相等性。
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.digits == other.digits
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // 数组比较是逐段的；缺的段已经是 0，所以"补齐"不需要额外一步。
        self.digits.cmp(&other.digits)
    }
}

/// 本机版本是从哪一条读取器读出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum VersionSource {
    /// MSIX（打包态）：`Package.Current.Id.Version` 的等价物。
    Packaged,
    /// NSIS 等非打包安装：卸载项的 `DisplayVersion`。
    UninstallEntry,
    /// 兜底：主程序 exe 自己声明的版本。
    Executable,
}

/// 本机当前版本，以及它是怎么读出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstalledVersion {
    pub version: Version,
    pub source: VersionSource,
}

/// 三条原始读取器，**顺序就是分派顺序**（见 [`installed_version`]）。
///
/// 抽成三个可替换的入口，是为了让"读不到 ⇒ 不检查"这条能被单测钉死，而测试不必去碰真注册表
/// 或真包身份：生产实现见 [`VersionProbes::system`]。
pub(crate) struct VersionProbes {
    /// MSIX 包版本。三态：没有包身份 / 有身份但读不出 / 读到了。
    pub packaged: Box<dyn Fn() -> PackageVersionProbe>,
    /// 卸载项的 `DisplayVersion`。
    pub uninstall_entry: Box<dyn Fn() -> Option<String>>,
    /// 主程序 exe 自己声明的版本。
    pub executable: Box<dyn Fn() -> Option<String>>,
}

impl VersionProbes {
    /// 生产环境的三条读取器。
    pub(crate) fn system() -> Self {
        Self {
            // 打包态：包版本。类型与判断都在 `windows_integration`（包身份那套探针已经在那儿）。
            packaged: Box::new(crate::windows_integration::current_package_version),
            // 非打包态：卸载项（NSIS）。
            uninstall_entry: Box::new(crate::windows_integration::uninstall_display_version),
            // 再退一步：复用**扫描侧**那条 exe 版本读取器（它读的是 VERSIONINFO 里的
            // FileVersion/ProductVersion）。同一条读法不写第二份。
            executable: Box::new(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|path| crate::harness_targets::file_version(&path))
            }),
        }
    }
}

/// 读本机当前版本，按安装形态分派（§3.1）。
///
///  - 打包态：包版本是唯一权威来源。**有包身份却读不出包版本时不检查**，也不退到 exe 的字符串
///    版本 —— 那会让设置里显示的版本与 `Get-AppxPackage` 对不上（§八 10）；
///  - 非打包态：先读卸载项的 `DisplayVersion`（形状不认识就继续往下退），再退到主程序 exe；
///  - 都读不到：`None` ⇒ 不检查，而不是猜一个版本出来（§八 9：`tauri dev` 下不报错、不误判）。
pub(crate) fn installed_version(probes: &VersionProbes) -> Option<InstalledVersion> {
    match (probes.packaged)() {
        PackageVersionProbe::Version(text) => {
            return Version::parse(&text).map(|version| InstalledVersion {
                version,
                source: VersionSource::Packaged,
            });
        }
        PackageVersionProbe::Unavailable => {
            log::warn!("更新检查：有 MSIX 包身份但读不出包版本，本次不检查（不退到 exe 版本）");
            return None;
        }
        PackageVersionProbe::NotPackaged => {}
    }

    match (probes.uninstall_entry)() {
        Some(text) => match Version::parse(&text) {
            Some(version) => {
                return Some(InstalledVersion {
                    version,
                    source: VersionSource::UninstallEntry,
                });
            }
            // 卸载项在、但版本形状不认识：留下一条痕再往下退，而不是在这里放弃。
            None => log::warn!("更新检查：卸载项的 DisplayVersion 形状不认识（{text}），退到主程序 exe"),
        },
        None => {}
    }

    (probes.executable)()
        .and_then(|text| Version::parse(&text))
        .map(|version| InstalledVersion {
            version,
            source: VersionSource::Executable,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的三条读取器：给定什么就返回什么。
    fn probes(
        packaged: PackageVersionProbe,
        uninstall_entry: Option<&'static str>,
        executable: Option<&'static str>,
    ) -> VersionProbes {
        VersionProbes {
            packaged: Box::new(move || packaged.clone()),
            uninstall_entry: Box::new(move || uninstall_entry.map(str::to_string)),
            executable: Box::new(move || executable.map(str::to_string)),
        }
    }

    fn tag(tag: &str) -> Version {
        Version::parse_tag(tag).unwrap_or_else(|| panic!("{tag} should parse"))
    }

    #[test]
    fn parses_the_tags_the_releases_actually_carry() {
        // 三段是今天的真身（2026-10-01 补充），四段是 MSIX 时代的形状：两代都要认。
        assert_eq!(tag("v0.4.1").to_string(), "0.4.1");
        assert_eq!(tag("v0.3.2").to_string(), "0.3.2");
        assert_eq!(tag("v0.2.0.202").to_string(), "0.2.0.202");
        assert_eq!(tag("v1").to_string(), "1");
        assert_eq!(tag("v1.2.3.4").to_string(), "1.2.3.4");
    }

    #[test]
    fn rejects_everything_the_plan_regex_does_not_accept() {
        for tag in [
            "",
            "v",
            "0.4.1",            // 没有前缀
            "V0.4.1",           // 正则只认小写 v
            "v0.4.1 ",          // 锚点：不许有空白
            " v0.4.1",
            "v0.4.1.2.3",       // 五段
            "v1..2",
            "v1.",
            "v.1",
            "v1.2.x",
            "v-1",
            "v+1",
            "version 0.4.1",
            "checkpoint-2026-09-28", // 仓库里现存的那类标签：不可比较就是不可比较
            "v１.2",            // 全角数字不是 ASCII 十进制
            "v99999999999",     // 段值溢出 u32
        ] {
            assert!(
                Version::parse_tag(tag).is_none(),
                "{tag:?} must not be read as a version"
            );
        }
    }

    #[test]
    fn missing_segments_count_as_zero() {
        assert_eq!(Version::parse_tag("v0.4.1"), Version::parse_tag("v0.4.1.0"));
        assert_eq!(Version::parse_tag("v1"), Version::parse_tag("v1.0.0.0"));
        // 补齐的后果也要说清楚：`0.4.1.9` 比 `0.4.2` 旧。
        assert!(Version::parse_tag("v0.4.1.9") < Version::parse_tag("v0.4.2"));
    }

    #[test]
    fn orders_releases_by_segment() {
        assert!(tag("v0.4.2").is_newer_than(&tag("v0.4.1")));
        assert!(tag("v0.4.1").is_newer_than(&tag("v0.3.9")));
        assert!(tag("v1.0").is_newer_than(&tag("v0.99.99")));
        assert!(!tag("v0.4.1").is_newer_than(&tag("v0.4.1")));
        assert!(!tag("v0.4.0").is_newer_than(&tag("v0.4.1")));
    }

    #[test]
    fn keeps_the_written_shape_for_display() {
        // 报给界面的字面量按原样写了几段来：这一串要和用户装的那个包对得上。
        assert_eq!(Version::parse_tag("v0.4.1.0").unwrap().to_string(), "0.4.1.0");
        assert_eq!(Version::parse("0.2.0.202").unwrap().to_string(), "0.2.0.202");
        assert_eq!(Version::parse("1").unwrap().to_string(), "1");
    }

    #[test]
    fn parses_the_unprefixed_versions_the_readers_hand_back() {
        assert_eq!(Version::parse("0.4.0").unwrap().to_string(), "0.4.0");
        assert_eq!(Version::parse("  0.4.1\r\n").unwrap().to_string(), "0.4.1");
        assert_eq!(Version::parse("V0.4.1").unwrap().to_string(), "0.4.1");
        // 形状不认识的返回值：读不到就不检查，绝不截取前几位。
        assert!(Version::parse("0.4.0 (build 7)").is_none());
        assert!(Version::parse("").is_none());
        assert!(Version::parse("unknown").is_none());
    }

    #[test]
    fn the_packaged_version_comes_from_the_package_alone() {
        let probes = probes(
            PackageVersionProbe::Version("0.2.0.202".into()),
            Some("0.4.0"),
            Some("0.4.0"),
        );
        let installed = installed_version(&probes).expect("a packaged build has a version");
        assert_eq!(installed.version.to_string(), "0.2.0.202");
        assert_eq!(installed.source, VersionSource::Packaged);
    }

    #[test]
    fn a_packaged_build_without_a_readable_package_version_does_not_fall_back() {
        let probes = probes(
            PackageVersionProbe::Unavailable,
            Some("0.4.0"),
            Some("0.4.0"),
        );
        assert!(installed_version(&probes).is_none());
    }

    #[test]
    fn an_nsis_install_reads_the_uninstall_entry() {
        let probes = probes(PackageVersionProbe::NotPackaged, Some("0.4.0"), Some("0.3.9"));
        let installed = installed_version(&probes).expect("the uninstall entry has a version");
        assert_eq!(installed.version.to_string(), "0.4.0");
        assert_eq!(installed.source, VersionSource::UninstallEntry);
    }

    #[test]
    fn an_unreadable_uninstall_entry_falls_back_to_the_executable() {
        let probes = probes(
            PackageVersionProbe::NotPackaged,
            Some("0.4.0 (build 7)"),
            Some("0.4.0"),
        );
        let installed = installed_version(&probes).expect("the executable declares a version");
        assert_eq!(installed.version.to_string(), "0.4.0");
        assert_eq!(installed.source, VersionSource::Executable);
    }

    #[test]
    fn nothing_readable_means_no_check_at_all() {
        assert!(installed_version(&probes(PackageVersionProbe::NotPackaged, None, None)).is_none());
        assert!(installed_version(&probes(
            PackageVersionProbe::NotPackaged,
            Some("not a version"),
            Some(""),
        ))
        .is_none());
    }

    /// 手动核对用，默认忽略：它读的是**这台机器上真的装了什么**，在 CI 上没有任何意义。
    ///
    /// 用法：`cargo test --lib -- --ignored this_machine_reads --nocapture`，然后把打出来的版本与
    /// `Get-AppxPackage com.dsh.wallpaper`（打包态）或卸载项的 `DisplayVersion` 与
    /// `Get-Item <exe> | % VersionInfo`（NSIS）对一遍 —— 那正是计划书 §八 10 要的核对。
    #[test]
    #[ignore = "reads this machine's install instead of a fixture"]
    fn this_machine_reads_its_own_installed_version() {
        // 三条读取器各自的原始答案：哪一条读到了、另外两条为什么没读到，一眼能看出来。
        println!(
            "包版本（MSIX）：{:?}",
            crate::windows_integration::current_package_version()
        );
        println!(
            "卸载项的 DisplayVersion：{:?}",
            crate::windows_integration::uninstall_display_version()
        );
        // 注意：测试进程自己的 exe 没有 VERSIONINFO，所以这一行通常是 `None` —— 它证明的是
        // "读不到就是读不到"，不是这台机器上主程序的版本。
        println!(
            "测试进程 exe 的 VERSIONINFO：{:?}",
            std::env::current_exe()
                .ok()
                .and_then(|path| crate::harness_targets::file_version(&path))
        );
        match installed_version(&VersionProbes::system()) {
            Some(installed) => println!(
                "本机当前版本：{}（读取器：{:?}）",
                installed.version, installed.source
            ),
            None => println!("本机读不到当前版本（没装，或包版本读不出来）⇒ 不检查"),
        }
    }
}
