use serde::Serialize;

#[cfg(windows)]
use windows::{
    core::{w, HSTRING, PCWSTR, PWSTR},
    ApplicationModel::{StartupTask, StartupTaskState},
    Win32::{
        Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER,
            ERROR_MORE_DATA, ERROR_SUCCESS,
        },
        Storage::Packaging::Appx::{GetCurrentPackageFamilyName, GetCurrentPackageFullName},
        System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER,
            KEY_READ, KEY_SET_VALUE,
        },
    },
};

/// Returns whether this process has a Windows package identity. Windows
/// documents `APPMODEL_ERROR_NO_PACKAGE` as the explicit unpackaged result;
/// the length probe is sufficient, so no package name is retained or exposed.
#[cfg(windows)]
fn has_package_identity() -> Result<bool, String> {
    let mut length = 0u32;
    // SAFETY: this is a length-only probe: the name buffer is None, so the API writes just the required length into the local u32.
    let status = unsafe { GetCurrentPackageFullName(&mut length, None) };
    if status == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(false);
    }
    if status == ERROR_INSUFFICIENT_BUFFER || status.is_ok() {
        return Ok(true);
    }
    Err(format!(
        "无法判断当前应用的 MSIX 包身份（Windows 错误码 {}）；为避免错误接管，已取消操作。",
        status.0
    ))
}

/// 当前进程的 MSIX 包版本，按"有没有包身份、读不读得到版本"分三种答案。
///
/// 更新检测要用它（计划书 `docs/plans/wallpaper-update-detection-plan.md` §3.1），但它是
/// Windows 集成的事，所以类型留在这里：
///
///  - `NotPackaged`：进程没有包身份（`tauri dev`，或 NSIS 安装）⇒ 调用方换别的读取器；
///  - `Unavailable`：**有**包身份却读不出包版本 ⇒ 调用方必须**不检查**，不许退到别的读取器去猜
///    —— 猜出来的数字会让设置里显示的当前版本与 `Get-AppxPackage` 对不上（同计划书 §八 10）；
///  - `Version`：包版本。
#[cfg(not(feature = "lite"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PackageVersionProbe {
    NotPackaged,
    Unavailable,
    Version(String),
}

/// 读当前进程的 MSIX 包版本（`Package.Current.Id.Version` 的等价物）。
///
/// 走 `GetCurrentPackageFullName` 而不是 WinRT 的 `Package.Current`：包全名的第二段就是版本
/// （`<Name>_<Version>_<Arch>_<PublisherId>`），这条路上不需要 WinRT 初始化，判据与
/// [`has_package_identity`] 完全同一处 —— "任何 WinRT 调用之前先要包身份"那条纪律照旧。
#[cfg(all(windows, not(feature = "lite")))]
pub(crate) fn current_package_version() -> PackageVersionProbe {
    match has_package_identity() {
        Ok(false) => PackageVersionProbe::NotPackaged,
        // 判断不了就不检查：当作"没打包"会退去读注册表，那是在猜一个版本。
        Err(error) => {
            log::warn!("更新检查：{error}");
            PackageVersionProbe::Unavailable
        }
        Ok(true) => {
            let Some(full_name) = current_package_full_name() else {
                log::warn!("更新检查：读不到当前应用的 MSIX 包全名，本次不检查");
                return PackageVersionProbe::Unavailable;
            };
            match package_version_in_full_name(&full_name) {
                Some(version) => PackageVersionProbe::Version(version.to_string()),
                None => {
                    log::warn!("更新检查：包全名的形状不认识（{full_name}），本次不检查");
                    PackageVersionProbe::Unavailable
                }
            }
        }
    }
}

/// 包全名。长度先探一次再读，与 [`current_run_entry_command`] 里读包族名同一写法。
#[cfg(all(windows, not(feature = "lite")))]
fn current_package_full_name() -> Option<String> {
    let mut length = 0u32;
    // SAFETY: a length-only probe with a None name buffer; the API writes only the required length into the local u32.
    let probe = unsafe { GetCurrentPackageFullName(&mut length, None) };
    if probe != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return None;
    }
    // 长度含结尾的 0；多留一个元素，并把容量显式传回去。
    let mut buffer = vec![0u16; length as usize + 1];
    let mut capacity = buffer.len() as u32;
    // SAFETY: `buffer` is a heap Vec of length plus one elements and `capacity` is exactly its length, so the write is bounded; the Vec is not moved during the call, and the written count is re-read from `capacity`.
    let read = unsafe { GetCurrentPackageFullName(&mut capacity, Some(PWSTR(buffer.as_mut_ptr()))) };
    if !read.is_ok() || capacity == 0 {
        return None;
    }
    let full_name = String::from_utf16_lossy(&buffer[..capacity as usize]);
    let full_name = full_name.trim_end_matches('\0').to_string();
    (!full_name.is_empty()).then_some(full_name)
}

/// 包全名里的版本段：`<Name>_<Version>_<Arch>[_<ResourceId>]_<PublisherId>` 的第二段。
///
/// 纯函数，形状不认识就返回 `None`（调用方据此不检查）。
#[cfg(not(feature = "lite"))]
fn package_version_in_full_name(full_name: &str) -> Option<&str> {
    let version = full_name.split('_').nth(1)?.trim();
    (!version.is_empty()).then_some(version)
}

#[cfg(all(not(windows), not(feature = "lite")))]
pub(crate) fn current_package_version() -> PackageVersionProbe {
    // 非 Windows 没有"包身份"这回事。
    PackageVersionProbe::NotPackaged
}

/// Per-user autostart location for builds Windows will not start through a
/// package StartupTask.
///
/// `reg.exe` needs the hive inside the path and answers "Invalid key name"
/// without it, while `RegOpenKeyExW` receives the hive as its own argument.
/// Both spellings are kept side by side, and a unit test asserts that they still
/// name the same key: using the hive-less one for `reg.exe` is how the repair
/// silently stopped writing the entry after a package update.
#[cfg(windows)]
const RUN_KEY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_KEY_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE_NAME: &str = "dsh-wallpaper";
/// The `<Application Id>` declared by this edition's `AppxManifest.xml`. The
/// shell needs it, together with the package family name, to address the
/// packaged app's launch alias.
///
/// The two editions ship different manifests *and* different packages
/// (`com.dsh.wallpaper` / `com.dsh.wallpaper.lite`), so one shared value made
/// the Lite package record an alias for an application its manifest does not
/// declare — an entry that resolves to nothing at logon.
#[cfg(all(windows, feature = "lite"))]
const PACKAGE_APPLICATION_ID: &str = "WallpaperLite";
#[cfg(all(windows, not(feature = "lite")))]
const PACKAGE_APPLICATION_ID: &str = "Wallpaper";

/// The per-user autostart value a packaged build must record.
///
/// A packaged build must never record its own path: Tauri reports the
/// version-pinned `WindowsApps\..._<version>_...` directory, which the next
/// package update deletes, and that is precisely how autostart disappeared
/// between 0.2.0.71 and 0.2.0.74. The shell resolves the `AppsFolder` alias
/// through the current package registration instead, so the value recorded
/// once keeps launching every later version.
#[cfg(windows)]
fn packaged_run_entry_command(package_family_name: &str) -> String {
    format!(r"explorer.exe shell:AppsFolder\{package_family_name}!{PACKAGE_APPLICATION_ID}")
}

/// Whether a recorded Run value still launches the build that needs it.
/// Values are written by `reg.exe`, by older releases and occasionally by
/// hand, so quoting, surrounding whitespace and letter case must not be
/// mistaken for a version change.
#[cfg(windows)]
fn autostart_commands_match(recorded: &str, expected: &str) -> bool {
    fn normalize(value: &str) -> String {
        value.replace('"', "").trim().to_lowercase()
    }
    !expected.is_empty() && normalize(recorded) == normalize(expected)
}

/// The Run value this build needs. A packaged install uses the version-stable
/// shell alias; an unpackaged development or NSIS build keeps naming its own
/// executable, which no MSIX update can move.
#[cfg(windows)]
pub(crate) fn current_run_entry_command() -> Result<String, String> {
    if !has_package_identity()? {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        return Ok(exe.to_string_lossy().into_owned());
    }
    let mut length = 0u32;
    // SAFETY: a length-only probe with a None name buffer; the API writes only the required length into the local u32.
    let probe = unsafe { GetCurrentPackageFamilyName(&mut length, None) };
    if probe != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(format!(
            "无法读取当前应用的 MSIX 包族名（Windows 错误码 {}）。",
            probe.0
        ));
    }
    // The length includes the terminator; supply one spare element and pass the
    // capacity back explicitly so the second call can report how much was written.
    let mut buffer = vec![0u16; length as usize + 1];
    let mut capacity = buffer.len() as u32;
    let read =
        // SAFETY: `buffer` is a heap Vec of length plus one elements and `capacity` is exactly its length, so the write is bounded; the Vec stays put for the duration of the call.
        unsafe { GetCurrentPackageFamilyName(&mut capacity, Some(PWSTR(buffer.as_mut_ptr()))) };
    if !read.is_ok() || capacity == 0 {
        return Err(format!(
            "无法读取当前应用的 MSIX 包族名（Windows 错误码 {}）。",
            read.0
        ));
    }
    let family = String::from_utf16_lossy(&buffer[..capacity as usize]);
    let family = family.trim_end_matches('\0');
    if family.is_empty() {
        return Err("无法读取当前应用的 MSIX 包族名。".into());
    }
    Ok(packaged_run_entry_command(family))
}

/// The value the per-user Run entry currently records, if it exists at all.
#[cfg(windows)]
fn run_entry_command() -> Result<Option<String>, String> {
    // `RegOpenKeyExW` and `RegQueryValueExW` take the hive and the value name
    // separately, so both are spelled from the same constants the writer uses.
    let subkey = RUN_KEY_SUBKEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let value_name = RUN_VALUE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let mut key = windows::Win32::System::Registry::HKEY::default();
    // SAFETY: `subkey` is a local NUL-terminated UTF-16 Vec and `key` is a local HKEY out-parameter; a failed open leaves the handle unused and a successful one is closed exactly once below.
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if open_status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if open_status != ERROR_SUCCESS {
        return Err(format!(
            "无法读取当前用户开机启动项（错误码 {}）。",
            open_status.0
        ));
    }

    let mut value_size = 0u32;
    // SAFETY: no data buffer is passed (None), only the local `value_size` receives the required size, and `key` is valid because the open status was checked above.
    let query_status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            None,
            Some(&mut value_size),
        )
    };
    if query_status == ERROR_FILE_NOT_FOUND {
        // SAFETY: `key` was opened successfully above and this is its single close on the early return.
        let _ = unsafe { RegCloseKey(key) };
        return Ok(None);
    }
    if query_status != ERROR_SUCCESS && query_status != ERROR_MORE_DATA {
        // SAFETY: the same handle from the single successful open is closed here, exactly once on this error path.
        let _ = unsafe { RegCloseKey(key) };
        return Err(format!(
            "无法读取 DSH Wallpaper 开机启动项（错误码 {}）。",
            query_status.0
        ));
    }
    if value_size == 0 {
        // SAFETY: `key` is still the handle from the successful open and is closed once before the early return.
        let _ = unsafe { RegCloseKey(key) };
        return Ok(Some(String::new()));
    }

    // The value is a UTF-16 `REG_SZ`. Ask for one spare byte: a size that does
    // not count the terminator must not make the second read fail.
    let mut buffer = vec![0u8; value_size as usize + 2];
    let mut read_size = buffer.len() as u32;
    // SAFETY: `buffer` is a local Vec with one spare byte beyond the reported size, its length is passed as `read_size`, and `key` stays open across the call and is closed right after.
    let read_status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            Some(buffer.as_mut_ptr()),
            Some(&mut read_size),
        )
    };
    // SAFETY: this closes the handle from the single successful open exactly once, before the buffer is decoded.
    let _ = unsafe { RegCloseKey(key) };
    if read_status != ERROR_SUCCESS {
        return Err(format!(
            "无法读取 DSH Wallpaper 开机启动项（错误码 {}）。",
            read_status.0
        ));
    }
    let units = buffer[..read_size as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<u16>>();
    let value = String::from_utf16_lossy(&units);
    Ok(Some(value.trim_end_matches('\0').to_string()))
}

/// 卸载项的子键名候选，按"最可能先试"排序。
///
/// 计划书写的是 `<app>_is1`；本机实测（2026-10-01，NSIS 装的是 0.4.0）真名就是产品名
/// `dsh-wallpaper` —— 没有后缀。两种拼法都试，先试实测到的那个：将来 Tauri 的模板改回带后缀、
/// 或者产品名改成包标识符，这条读取器都不用动。
#[cfg(all(windows, not(feature = "lite")))]
const UNINSTALL_KEY_CANDIDATES: [&str; 4] = [
    "dsh-wallpaper",
    "dsh-wallpaper_is1",
    "com.dsh.wallpaper",
    "com.dsh.wallpaper_is1",
];

/// 非打包安装写在卸载项里的 `DisplayVersion`（NSIS 等）。
///
/// 更新检测的第二顺位读取器：打包态走包版本，这里只给没有包身份的进程用（计划书 §3.1）。
/// 与开机自启那条读取器不同，这条不必把失败原因给用户看 —— 键不在、值不在、读不出来，三者都是
/// "没读到"，细节留在日志里，由调用方决定不检查。
#[cfg(all(windows, not(feature = "lite")))]
pub(crate) fn uninstall_display_version() -> Option<String> {
    for subkey in UNINSTALL_KEY_CANDIDATES {
        let path = format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{subkey}");
        match read_hkcu_string(&path, "DisplayVersion") {
            Ok(Some(value)) => {
                let value = value.trim().to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
            Ok(None) => {}
            Err(status) => log::warn!("更新检查：读卸载项 {subkey} 失败（Windows 错误码 {status}）"),
        }
    }
    None
}

/// 读 `HKCU` 下的一个 `REG_SZ` 值。`Ok(None)` 是"键或值不在"，`Err(错误码)` 是别的失败。
#[cfg(all(windows, not(feature = "lite")))]
fn read_hkcu_string(subkey: &str, value_name: &str) -> Result<Option<String>, u32> {
    let wide = |text: &str| -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    };
    let subkey = wide(subkey);
    let value_name = wide(value_name);
    let mut key = windows::Win32::System::Registry::HKEY::default();
    // SAFETY: both name pointers come from local NUL-terminated UTF-16 Vecs and `key` is a local HKEY out-parameter; failure leaves the handle unused and success is closed exactly once below.
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if open_status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if open_status != ERROR_SUCCESS {
        return Err(open_status.0);
    }

    let mut value_size = 0u32;
    // SAFETY: no data buffer is passed, only the local `value_size` receives the required size, and `key` is valid because the open succeeded above.
    let query_status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            None,
            Some(&mut value_size),
        )
    };
    if query_status == ERROR_FILE_NOT_FOUND {
        // SAFETY: `key` was opened successfully above and this is its single close on the early return.
        let _ = unsafe { RegCloseKey(key) };
        return Ok(None);
    }
    if query_status != ERROR_SUCCESS && query_status != ERROR_MORE_DATA {
        // SAFETY: the same handle is closed here, exactly once on this error path.
        let _ = unsafe { RegCloseKey(key) };
        return Err(query_status.0);
    }
    if value_size == 0 {
        // SAFETY: `key` is still the handle from the successful open and is closed once before the early return.
        let _ = unsafe { RegCloseKey(key) };
        return Ok(Some(String::new()));
    }

    // 值是 UTF-16 的 `REG_SZ`：多要两个字节，免得"长度不计结尾 0"让第二次读失败。
    let mut buffer = vec![0u8; value_size as usize + 2];
    let mut read_size = buffer.len() as u32;
    // SAFETY: `buffer` has one spare byte beyond the reported size, its length is passed as `read_size`, and `key` is closed immediately after the call.
    let read_status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            None,
            None,
            Some(buffer.as_mut_ptr()),
            Some(&mut read_size),
        )
    };
    // SAFETY: closes the handle from the single successful open exactly once, before the value is decoded.
    let _ = unsafe { RegCloseKey(key) };
    if read_status != ERROR_SUCCESS {
        return Err(read_status.0);
    }
    let units = buffer[..read_size as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<u16>>();
    Ok(Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string(),
    ))
}

#[cfg(all(not(windows), not(feature = "lite")))]
pub(crate) fn uninstall_display_version() -> Option<String> {
    // 非 Windows 没有 HKCU 卸载项。
    None
}

/// The value line `reg query` prints, pulled out of its report.
///
/// The output is a blank line, the key path in brackets and the value itself,
/// with name, type and data separated by runs of spaces:
///
/// ```text
///
/// HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
///     dsh-wallpaper    REG_SZ    explorer.exe shell:AppsFolder\...!Wallpaper
/// ```
///
/// Only the type token is used as a delimiter, because the data contains spaces
/// of its own.
#[cfg(windows)]
fn parse_reg_query_value(output: &str, value_name: &str) -> Option<String> {
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix(value_name) else {
            continue;
        };
        // The name has to be a whole token: `dsh-wallpaper-old` is another value.
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(type_end) = rest.find(char::is_whitespace) else {
            continue;
        };
        if !rest[..type_end].starts_with("REG_") {
            continue;
        }
        return Some(rest[type_end..].trim_start().to_string());
    }
    None
}

/// What a child `reg.exe` — and therefore the shell at logon — sees in the same
/// key.
///
/// MSIX redirects a packaged app's HKCU writes into a private per-user hive
/// which is *merged over* the real key when the app reads it. An in-process
/// delete therefore leaves a tombstone that hides the real value from the app
/// only, while Windows keeps launching the app at logon: the app reported
/// 「当前用户启动项里没有 DSH Wallpaper」 for a value that was sitting in the
/// real key the whole time. The manifest declares this key unvirtualized, and
/// this read is the cross-check that says whether that declaration is in force.
#[cfg(windows)]
fn run_entry_command_via_child() -> Result<Option<String>, String> {
    use std::process::Command;
    let mut cmd = Command::new("reg");
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x08000000);
    cmd.args(["query", RUN_KEY_PATH, "/v", RUN_VALUE_NAME]);
    let output = cmd.output().map_err(|error| error.to_string())?;
    match output.status.code() {
        Some(0) => Ok(parse_reg_query_value(
            &String::from_utf8_lossy(&output.stdout),
            RUN_VALUE_NAME,
        )),
        // `reg.exe` answers 1 for "no such key or value", which is the same state
        // the in-process read reports as "nothing recorded".
        Some(1) => Ok(None),
        other => Err(format!(
            "无法读取当前用户开机启动项（reg.exe 退出码 {}）",
            other.unwrap_or(-1)
        )),
    }
}

/// What the compatibility Run entry says now, together with the two values
/// that produced the answer.
///
/// The read-back decides what the settings page shows, so it must not guess:
/// "nothing is recorded", "another build is recorded" and "the registry could
/// not be read" used to collapse into one silent `false`, and a log line could
/// then contradict the switch without either side saying which value it saw.
/// Errors are folded in rather than propagated for the same reason — the
/// caller has to be able to name which of the three states it is in.
#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct RunEntryCheck {
    pub(crate) matches: bool,
    /// The recorded value, when the registry read succeeded.
    pub(crate) recorded: Option<String>,
    /// The value this build needs, when it could be computed.
    pub(crate) expected: Option<String>,
    /// Why the check could not be completed, when it could not.
    pub(crate) error: Option<String>,
}

#[cfg(windows)]
pub(crate) fn check_run_entry() -> RunEntryCheck {
    let expected = match current_run_entry_command() {
        Ok(command) => command,
        Err(error) => {
            return RunEntryCheck {
                matches: false,
                recorded: None,
                expected: None,
                error: Some(error),
            }
        }
    };
    let recorded = match run_entry_command() {
        Ok(value) => value,
        Err(error) => {
            return RunEntryCheck {
                matches: false,
                recorded: None,
                expected: Some(expected),
                error: Some(error),
            }
        }
    };
    let matches = match recorded.as_deref() {
        Some(value) => autostart_commands_match(value, &expected),
        None => false,
    };
    // The two views are supposed to be the same key, and they silently were not:
    // the app read its own private tombstone while the real value stayed exactly
    // where the app had just written it. The manifest declares this key
    // unvirtualized; this is where a regression of that declaration becomes a
    // line in the log instead of a settings page that contradicts Windows.
    match run_entry_command_via_child() {
        Ok(child) if child == recorded => {}
        Ok(child) => log::warn!(
            "开机自启回读：包内视图与 reg.exe 视图不一致（包内={recorded:?} reg={child:?}）；写入虚拟化又对 Run 键生效了，开关显示的将不是登录时真正执行的那个值"
        ),
        Err(error) => log::warn!("开机自启回读：reg.exe 交叉检查失败：{error}"),
    }
    RunEntryCheck {
        matches,
        recorded,
        expected: Some(expected),
        error: None,
    }
}

/// Why the compatibility entry does not carry autostart, in the words the
/// settings page shows. Deliberately shorter than the log line: the page names
/// the state, the log names the values.
#[cfg(windows)]
fn run_entry_refusal(check: &RunEntryCheck) -> String {
    if let Some(error) = check.error.as_deref() {
        return error.to_string();
    }
    match check.recorded.as_deref() {
        None => "当前用户启动项里没有 DSH Wallpaper。".into(),
        Some(_) => "当前用户启动项指向的不是本次安装的版本。".into(),
    }
}

/// The exact `reg.exe` invocation that records the entry. Kept apart from the
/// spawn so a unit test can pin that the path names the hive: `reg.exe` rejects
/// one that does not, and that rejection left autostart unfixed after a package
/// update because the error was not distinguishing "not written" from "no
/// permission".
#[cfg(windows)]
fn run_entry_add_arguments(command: &str) -> Vec<String> {
    vec![
        "add".into(),
        RUN_KEY_PATH.into(),
        "/V".into(),
        RUN_VALUE_NAME.into(),
        "/D".into(),
        command.into(),
        "/F".into(),
    ]
}

/// Record (or refresh) the per-user Run entry.
///
/// `reg.exe` stays the writer because it is the compatibility path older
/// releases used, so an entry it updates remains readable by them.
#[cfg(windows)]
pub(crate) fn write_run_entry(command: &str) -> Result<(), String> {
    use std::process::Command;
    let mut cmd = Command::new("reg");
    // `reg.exe` is only a compatibility fallback. Keep it out of the user's
    // desktop even when the host is a GUI-subsystem process.
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x08000000);
    cmd.args(run_entry_add_arguments(command));
    let status = cmd.status().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        // Carry the exit code. Without it a failure here could not be told apart
        // from a rejected key path, a rejected value or a missing `reg.exe`.
        Err(format!(
            "更新当前用户开机自启失败（reg.exe 退出码 {}）",
            status.code().unwrap_or(-1)
        ))
    }
}

/// Prefer the package-owned StartupTask when the app is running from an MSIX.
/// Older packages and unpackaged development/NSIS builds do not carry the
/// extension, so callers may fall back to the legacy per-user Run entry.
#[cfg(windows)]
pub(crate) fn set_startup_task(enabled: bool) -> Result<Option<bool>, String> {
    if !has_package_identity()? {
        return Ok(None);
    }
    let task_id = HSTRING::from("DshWallpaperStartup");
    let task = match StartupTask::GetAsync(&task_id).and_then(|operation| operation.get()) {
        Ok(task) => task,
        // A package built before the StartupTask manifest extension is still
        // supported through the Run-key compatibility path.
        //
        // **但这条降级必须留痕**：另一位 agent 报告"注册表键从 09-22 到今始终不存在，manifest 与
        // TaskId 一致、系统策略正常，Windows 就是从未实例化这个任务，现在自启全靠 Run 键在扛"——
        // 而这里原本把 GetAsync 的 HRESULT 用 `Err(_)` 直接吞掉，于是"主路径为什么失效"在日志里
        // 一个字都没有，只能靠猜。降级行为不变（仍然返回 Ok(None) 走兼容路径），只是把原因说出来。
        Err(error) => {
            log::warn!(
                "启动任务不可用（GetAsync({task_id:?}) 失败：{error}）；本次改走 Run 键兼容路径"
            );
            return Ok(None);
        }
    };
    if !enabled {
        task.Disable()
            .map_err(|error| format!("无法关闭 DSH Wallpaper 启动任务：{error}"))?;
        return Ok(Some(false));
    }
    let state = task
        .RequestEnableAsync()
        .and_then(|operation| operation.get())
        .map_err(|error| format!("无法启用 DSH Wallpaper 启动任务：{error}"))?;
    if state == StartupTaskState::Enabled {
        Ok(Some(true))
    } else if state == StartupTaskState::DisabledByUser {
        Err("Windows 已禁用 DSH Wallpaper 启动任务；请在系统设置中允许开机启动。".into())
    } else if state == StartupTaskState::DisabledByPolicy {
        Err("Windows 策略禁止 DSH Wallpaper 开机启动。".into())
    } else {
        Err(format!(
            "DSH Wallpaper 启动任务未启用（状态码 {}）。",
            state.0
        ))
    }
}

/// The setting center must display the state Windows actually registered,
/// rather than a stale renderer preference.  `source` is deliberately a
/// small diagnostic value so the UI can explain why an update is needed
/// without exposing registry contents or package internals.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutostartStatus {
    pub(crate) enabled: bool,
    pub(crate) source: String,
    /// Why the state is what it is, in the words the settings page shows.
    ///
    /// The renderer used to guess: every refusal produced the same sentence
    /// about "系统启动应用权限", which sends the user to a Windows setting that
    /// on this machine was never the problem. The cause is known here — Windows
    /// refused the startup task with a specific code, the recorded entry names
    /// another build, or the registry could not be read — so it travels out
    /// instead of a canned sentence.
    pub(crate) reason: Option<String>,
}

#[cfg(windows)]
pub(crate) fn remove_legacy_run_entry() -> Result<(), String> {
    let mut key = windows::Win32::System::Registry::HKEY::default();
    // SAFETY: the subkey is a static `w!` string, `key` is a local HKEY out-parameter, and a failed open leaves it unused; the handle is closed exactly once below.
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if open_status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if open_status != ERROR_SUCCESS {
        return Err(format!(
            "无法更新当前用户开机启动项（错误码 {}）。",
            open_status.0
        ));
    }
    // SAFETY: `key` comes from the successful open above and is still open here; the value name is a static `w!` string.
    let delete_status = unsafe { RegDeleteValueW(key, w!("dsh-wallpaper")) };
    // SAFETY: closes the handle from the single successful open exactly once, after the delete result has been captured.
    let _ = unsafe { RegCloseKey(key) };
    if delete_status == ERROR_SUCCESS || delete_status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "无法移除旧版 DSH Wallpaper 开机启动项（错误码 {}）。",
            delete_status.0
        ))
    }
}

#[cfg(windows)]
pub(crate) fn autostart_status() -> Result<AutostartStatus, String> {
    // One read-back, one log line, one set of values for every branch below:
    // when the switch and the log disagree about autostart, this is the line
    // that says which value each side was looking at.
    let check = check_run_entry();
    log::info!(
        "开机自启回读：matches={} recorded={:?} expected={:?} error={:?}",
        check.matches,
        check.recorded,
        check.expected,
        check.error
    );

    if has_package_identity()? {
        let task_id = HSTRING::from("DshWallpaperStartup");
        match StartupTask::GetAsync(&task_id).and_then(|operation| operation.get()) {
            Ok(task) => {
                let state = task
                    .State()
                    .map_err(|error| format!("无法读取 DSH Wallpaper 启动任务状态：{error}"))?;
                let status = match state {
                    StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => {
                        AutostartStatus {
                            enabled: true,
                            source: "startup-task".into(),
                            reason: None,
                        }
                    }
                    StartupTaskState::DisabledByUser => AutostartStatus {
                        enabled: false,
                        source: "disabled-by-user".into(),
                        reason: Some(
                            "Windows 已禁用本应用的开机启动任务；可在系统设置的「启动应用」里重新允许。"
                                .into(),
                        ),
                    },
                    StartupTaskState::DisabledByPolicy => AutostartStatus {
                        enabled: false,
                        source: "disabled-by-policy".into(),
                        reason: Some("Windows 策略禁止本应用开机启动。".into()),
                    },
                    // A package update can add StartupTask to an app that was
                    // previously using the HKCU Run compatibility path. Keep the
                    // effective preference enabled until the user explicitly
                    // changes it; otherwise the new package would appear to have
                    // silently turned autostart off during an update. A Run entry
                    // only counts while it still launches this build: an entry left
                    // by a superseded version names a deleted `WindowsApps`
                    // directory and would otherwise report working autostart that
                    // no longer exists.
                    StartupTaskState::Disabled => {
                        if check.matches {
                            AutostartStatus {
                                enabled: true,
                                source: "run".into(),
                                reason: Some(
                                    "Windows 启动任务处于关闭状态，当前由当前用户启动项承载。".into(),
                                ),
                            }
                        } else {
                            AutostartStatus {
                                enabled: false,
                                source: "startup-task".into(),
                                // Now that the task registers on this machine this
                                // branch is reachable, and it must name the task
                                // rather than the fallback: the Run entry is not
                                // what decides whether logon starts the app.
                                reason: Some(
                                    "Windows 启动任务处于关闭状态，登录时不会启动本应用。".into(),
                                ),
                            }
                        }
                    }
                    other => AutostartStatus {
                        enabled: false,
                        source: "startup-task".into(),
                        reason: Some(format!(
                            "Windows 启动任务未启用（状态码 {}）；当前用户启动项里没有本应用。",
                            other.0
                        )),
                    },
                };
                return Ok(status);
            }
            // A package installed before the StartupTask extension was added is
            // still readable through the compatibility Run entry — and so is an
            // install where Windows refuses to instantiate the task at all,
            // which is what happens on the machine this was diagnosed on:
            // `GetAsync` answers E_INVALIDARG while the Run entry works. The
            // refusal is carried out as the reason, because "Windows 启动任务
            // 不可用" and "没有开启自启" were previously the same silent `false`.
            Err(error) => {
                log::warn!(
                    "启动任务不可用（GetAsync({task_id:?}) 失败：{error}）；本次改走 Run 键兼容路径"
                );
                if check.matches {
                    return Ok(AutostartStatus {
                        enabled: true,
                        source: "run".into(),
                        reason: Some(format!(
                            "Windows 启动任务不可用（{error}）；当前由当前用户启动项承载。"
                        )),
                    });
                }
                return Ok(AutostartStatus {
                    enabled: false,
                    source: "none".into(),
                    reason: Some(format!(
                        "Windows 启动任务不可用（{error}）；{}",
                        run_entry_refusal(&check)
                    )),
                });
            }
        }
    }

    if check.matches {
        return Ok(AutostartStatus {
            enabled: true,
            source: "run".into(),
            reason: None,
        });
    }
    // An entry recorded by a superseded version is deliberately not reported
    // as enabled: it names a path the package update deleted, so this build
    // really does not start at logon. The startup check repairs it.
    Ok(AutostartStatus {
        enabled: false,
        source: "none".into(),
        reason: Some(run_entry_refusal(&check)),
    })
}

/// Apply the user's autostart choice and report the state Windows ends up in.
///
/// The result is read back from the system in every case: `reg.exe` can report
/// success for a value Windows then ignores, and a settings page that shows the
/// requested state instead of the real one is how "关掉之后打不开" survived
/// three releases. The change is logged together with its outcome, so a refusal
/// that never reaches the log cannot happen again.
#[cfg(windows)]
pub(crate) fn set_autostart(enabled: bool) -> Result<AutostartStatus, String> {
    let outcome = if set_startup_task(enabled)?.is_some() {
        // A package StartupTask is the authoritative autostart path. Drop any
        // legacy Run value left by an older build so the single-instance guard
        // does not needlessly process a second launch attempt.
        let _ = remove_legacy_run_entry();
        autostart_status()
    } else {
        // 注册表 Run 键：开机自启 dsh-wallpaper
        //  开启: 写入当前构建需要的启动命令
        //  关闭: 删除该值
        // An MSIX install must record the shell's version-stable launch alias
        // rather than its own versioned `WindowsApps` path, which the next
        // package update deletes.
        let write = if enabled {
            current_run_entry_command().and_then(|command| write_run_entry(&command))
        } else {
            remove_legacy_run_entry()
        };
        match write {
            Ok(()) => autostart_status(),
            Err(error) => Err(error),
        }
    };
    match &outcome {
        Ok(status) => log::info!(
            "开机自启变更：requested={} enabled={} source={} reason={:?}",
            enabled,
            status.enabled,
            status.source,
            status.reason
        ),
        Err(error) => log::warn!("开机自启变更失败：requested={enabled} error={error}"),
    }
    outcome
}

/// Carry an enabled autostart preference across a package update.
///
/// A package update installs into a new versioned `WindowsApps` directory and
/// deletes the previous one, so a per-user Run entry recorded by the old build
/// stops launching anything the moment the new version is installed — which is
/// exactly how autostart vanished between 0.2.0.71 and 0.2.0.74. The
/// authoritative package StartupTask is preferred, and when Windows cannot
/// offer it the compatibility entry is refreshed so it names this version.
#[cfg(windows)]
pub(crate) fn migrate_legacy_autostart() -> Result<Option<AutostartStatus>, String> {
    if !has_package_identity()? {
        // Unpackaged development and NSIS builds only ever have the Run entry,
        // and nothing about an MSIX update can invalidate it.
        return Ok(None);
    }
    // Absence is the user's choice: never enable autostart on their behalf.
    let Some(recorded) = run_entry_command()? else {
        return Ok(Some(autostart_status()?));
    };

    // 1. Prefer the package task: it survives updates without our help.
    match set_startup_task(true) {
        Ok(Some(true)) => {
            remove_legacy_run_entry()?;
            return Ok(Some(autostart_status()?));
        }
        Ok(_) => {}
        // Windows may refuse, for example when the user disabled the task in
        // Task Manager. Keep the compatibility entry working instead of
        // leaving autostart broken for as long as the refusal lasts.
        Err(error) => log::warn!("开机启动任务不可用，改用当前用户启动项：{error}"),
    }

    // 2. The compatibility entry must name this version. Refreshing it is what
    //    makes an already-enabled autostart survive the update without the
    //    user having to toggle the setting again.
    let expected = current_run_entry_command()?;
    if !autostart_commands_match(&recorded, &expected) {
        write_run_entry(&expected)?;
        log::info!("已将当前用户开机自启重新指向本次安装的版本");
    }
    Ok(Some(autostart_status()?))
}

#[cfg(not(windows))]
pub(crate) fn migrate_legacy_autostart() -> Result<Option<AutostartStatus>, String> {
    Ok(None)
}

#[cfg(not(windows))]
pub(crate) fn autostart_status() -> Result<AutostartStatus, String> {
    Ok(AutostartStatus {
        enabled: false,
        source: "unsupported".into(),
        reason: Some("当前系统不支持本应用的开机自启。".into()),
    })
}

#[cfg(not(windows))]
pub(crate) fn set_autostart(_: bool) -> Result<AutostartStatus, String> {
    autostart_status()
}

#[cfg(not(windows))]
pub(crate) fn set_startup_task(_: bool) -> Result<Option<bool>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 包全名的第二段就是版本（`<Name>_<Version>_<Arch>_<PublisherId>`）；形状不认识就返回
    /// `None`，由调用方决定"不检查"。
    #[cfg(not(feature = "lite"))]
    #[test]
    fn the_package_version_is_the_second_field_of_the_full_name() {
        assert_eq!(
            package_version_in_full_name("com.dsh.wallpaper_0.2.0.202_x64__pdxj8y3r6rm5g"),
            Some("0.2.0.202")
        );
        // 带资源 id 的形状（`<Name>_<Version>_<Arch>_<ResourceId>_<PublisherId>`）同样是第二段。
        assert_eq!(
            package_version_in_full_name("com.dsh.wallpaper_1.0.0.0_neutral_~_pdxj8y3r6rm5g"),
            Some("1.0.0.0")
        );
        for broken in ["", "com.dsh.wallpaper", "com.dsh.wallpaper_"] {
            assert!(
                package_version_in_full_name(broken).is_none(),
                "{broken:?} has no version field"
            );
        }
    }

    /// 卸载项键名的候选表里必须留着**实测到过**的那两个：计划书的 `<app>_is1` 在这台机器上
    /// 并不存在，真名是产品名本身（见 `UNINSTALL_KEY_CANDIDATES` 的说明）。
    #[cfg(not(feature = "lite"))]
    #[test]
    fn the_measured_uninstall_key_stays_in_the_candidate_list() {
        assert!(UNINSTALL_KEY_CANDIDATES.contains(&"dsh-wallpaper"));
        assert!(UNINSTALL_KEY_CANDIDATES.contains(&"dsh-wallpaper_is1"));
    }

    #[test]
    fn packaged_autostart_records_the_version_stable_shell_alias() {
        let command = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        // The application half is this edition's own `<Application Id>`; the
        // test below checks it against the manifest that edition ships.
        assert_eq!(
            command,
            format!(r"explorer.exe shell:AppsFolder\com.dsh.wallpaper_pdxj8y3r6rm5g!{PACKAGE_APPLICATION_ID}")
        );
        // Whatever identifies the release must stay out of the autostart value:
        // a package update deletes the directory that carries it.
        assert!(!command.contains("WindowsApps"));
        assert!(!command.contains("0.2.0"));
    }

    #[test]
    fn an_entry_left_by_a_superseded_version_is_not_a_match() {
        let expected = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        // The exact value 0.2.0.71 wrote, which 0.2.0.74 could no longer launch.
        let stale = r"C:\Program Files\WindowsApps\com.dsh.wallpaper_0.2.0.71_x64__pdxj8y3r6rm5g\dsh-wallpaper.exe";
        assert!(!autostart_commands_match(stale, &expected));
    }

    #[test]
    fn recorded_run_values_are_compared_leniently() {
        let expected = r"C:\Program Files\WindowsApps\com.dsh.wallpaper_0.2.0.74_x64__pdxj8y3r6rm5g\dsh-wallpaper.exe";
        assert!(autostart_commands_match(
            r#"  "C:\Program Files\WindowsApps\com.dsh.wallpaper_0.2.0.74_x64__pdxj8y3r6rm5g\dsh-wallpaper.exe"  "#,
            expected
        ));
        assert!(autostart_commands_match(
            r"c:\program files\windowsapps\COM.DSH.WALLPAPER_0.2.0.74_X64__PDXJ8Y3R6RM5G\dsh-wallpaper.exe",
            expected
        ));
        assert!(!autostart_commands_match("", expected));
    }

    #[test]
    fn a_missing_expected_command_never_matches() {
        assert!(!autostart_commands_match("anything", ""));
        assert!(!autostart_commands_match("", ""));
    }

    #[test]
    fn the_autostart_alias_names_the_edition_it_was_installed_from() {
        // Two editions, two manifests, two `<Application Id>`s. The alias half
        // has to come from the manifest of the edition that recorded it: the
        // shell resolves `family!id`, and an id the package does not declare
        // launches nothing at logon.
        let (manifest, id) = if cfg!(feature = "lite") {
            (
                include_str!("../../../../packaging/msix/AppxManifest-Lite.xml"),
                "WallpaperLite",
            )
        } else {
            (
                include_str!("../../../../packaging/msix/AppxManifest.xml"),
                "Wallpaper",
            )
        };
        assert_eq!(id, PACKAGE_APPLICATION_ID);
        assert!(
            manifest.contains(&format!(r#"<Application Id="{id}""#)),
            "{id} is not the application this edition's manifest declares"
        );
        let command = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        assert!(command.ends_with(&format!("!{id}")), "{command}");
    }

    #[test]
    fn a_working_run_entry_is_recognised_as_this_build() {
        // The value this machine's Run key holds for 0.2.0.178: the toggle
        // reported it as "not applied" while the status read for the same value
        // logged `source=run enabled=true`. Both halves of the alias are pinned
        // by the test above, so this one fails if either drifts.
        let expected = packaged_run_entry_command("com.dsh.wallpaper_pdxj8y3r6rm5g");
        let recorded = format!(
            r"explorer.exe shell:AppsFolder\com.dsh.wallpaper_pdxj8y3r6rm5g!{PACKAGE_APPLICATION_ID}"
        );
        assert!(autostart_commands_match(&recorded, &expected));
        // `reg.exe add /D` stores quoting when the value arrives quoted from a
        // shell, and a quoted value still launches the same application.
        assert!(autostart_commands_match(&format!("\"{recorded}\""), &expected));
    }

    #[test]
    fn a_reg_query_report_is_read_as_the_recorded_value() {
        // Captured on this machine: `reg query <Run key> /v dsh-wallpaper`.
        let report = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    dsh-wallpaper    REG_SZ    explorer.exe shell:AppsFolder\\com.dsh.wallpaper_pdxj8y3r6rm5g!Wallpaper\r\n\r\n";
        assert_eq!(
            parse_reg_query_value(report, RUN_VALUE_NAME).as_deref(),
            Some(r"explorer.exe shell:AppsFolder\com.dsh.wallpaper_pdxj8y3r6rm5g!Wallpaper")
        );
        // A different value whose name merely starts the same is not ours.
        assert_eq!(
            parse_reg_query_value("    dsh-wallpaper-old    REG_SZ    x", RUN_VALUE_NAME),
            None
        );
        // A line that is not a typed value must not be handed out as data.
        assert_eq!(
            parse_reg_query_value("    dsh-wallpaper    NOT_A_TYPE    x", RUN_VALUE_NAME),
            None
        );
    }

    /// The span from an opening tag to its closing tag, so a test can assert
    /// nesting instead of a substring that a misplaced element satisfies too.
    fn span<'a>(source: &'a str, open: &str, close: &str) -> &'a str {
        let from = source
            .find(open)
            .unwrap_or_else(|| panic!("{open} is missing"));
        let to = source[from..]
            .find(close)
            .unwrap_or_else(|| panic!("{close} never closes {open}"));
        &source[from..from + to + close.len()]
    }

    fn edition_manifest() -> &'static str {
        if cfg!(feature = "lite") {
            include_str!("../../../../packaging/msix/AppxManifest-Lite.xml")
        } else {
            include_str!("../../../../packaging/msix/AppxManifest.xml")
        }
    }

    #[test]
    fn the_manifest_declares_only_namespaces_windows_knows() {
        // An unknown namespace is not a packaging error — `IgnorableNamespaces`
        // exists precisely so Windows can skip what it does not know. That is how
        // `windows.startupTask` sat in every release while registering nothing:
        // the URI said `…/uap/windows/10/5` instead of `…/uap/windows10/5`, so the
        // extension was ignored, `GetAsync` answered 参数错误。 (0x80070057) and no
        // AppModel SystemAppData key was ever created. `makeappx` was happy.
        let manifest = edition_manifest();
        let known = [
            "http://schemas.microsoft.com/appx/manifest/foundation/windows10",
            "http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities",
            "http://schemas.microsoft.com/appx/manifest/uap/windows10",
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/5",
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "http://schemas.microsoft.com/appx/manifest/virtualization/windows10",
        ];
        let mut declared = 0;
        for (index, _) in manifest.match_indices("xmlns:") {
            let rest = &manifest[index..];
            let Some(open) = rest.find('"') else { continue };
            let Some(close) = rest[open + 1..].find('"') else {
                continue;
            };
            let uri = &rest[open + 1..open + 1 + close];
            declared += 1;
            assert!(
                known.contains(&uri),
                "{uri} is not a manifest namespace Windows knows"
            );
        }
        assert!(declared >= 5, "manifest declared only {declared} namespaces");
    }

    #[test]
    fn the_startup_task_sits_where_the_schema_puts_it() {
        // Package → Applications → Application → Extensions → uap5:Extension →
        // uap5:StartupTask. Without the `Extensions` container the package still
        // installs and Windows never registers the task: `GetAsync` answered
        // 参数错误。 (0x80070057) for every build up to 0.2.0.180, and the AppModel
        // SystemAppData key was never created. Thirteen other packages on this
        // machine declare windows.startupTask, and all thirteen have the
        // container — a substring check cannot tell the two shapes apart.
        let manifest = edition_manifest();
        let application = span(manifest, "<Application ", "</Application>");
        let extensions = span(application, "<Extensions>", "</Extensions>");
        let extension = span(
            extensions,
            r#"<uap5:Extension Category="windows.startupTask""#,
            "</uap5:Extension>",
        );
        assert!(extension.contains(r#"<uap5:StartupTask TaskId="DshWallpaperStartup""#));
    }

    #[test]
    fn the_run_key_is_declared_unvirtualized_by_the_manifest_this_edition_ships() {
        // MSIX redirects a packaged app's HKCU writes into a private per-user
        // hive that is merged over the real key when it reads. For the Run key
        // that means an in-process delete leaves a tombstone which hides the
        // real value from the app while Windows keeps launching it at logon —
        // 「当前用户启动项里没有 DSH Wallpaper」 for a value sitting right there.
        let manifest = edition_manifest();
        let excluded = format!(
            "<virtualization:ExcludedKey>HKEY_CURRENT_USER\\{RUN_KEY_SUBKEY}</virtualization:ExcludedKey>"
        );
        assert!(
            manifest.contains(&excluded),
            "the manifest this edition ships does not declare {excluded}"
        );
        assert!(manifest.contains(r#"<rescap:Capability Name="unvirtualizedResources" />"#));
    }

    #[test]
    fn a_refused_autostart_names_its_own_state() {
        let absent = RunEntryCheck {
            matches: false,
            recorded: None,
            expected: Some("expected".into()),
            error: None,
        };
        let stale = RunEntryCheck {
            matches: false,
            recorded: Some("old".into()),
            expected: Some("expected".into()),
            error: None,
        };
        let unreadable = RunEntryCheck {
            matches: false,
            recorded: None,
            expected: None,
            error: Some("无法读取当前用户开机启动项（错误码 5）。".into()),
        };
        assert!(run_entry_refusal(&absent).contains("没有"));
        assert_ne!(run_entry_refusal(&absent), run_entry_refusal(&stale));
        // A read failure is reported as itself, never as "nothing is recorded":
        // those two states need different fixes.
        assert_eq!(
            run_entry_refusal(&unreadable),
            "无法读取当前用户开机启动项（错误码 5）。"
        );
    }

    #[test]
    fn the_autostart_key_paths_name_the_same_key() {
        assert!(RUN_KEY_PATH.starts_with(r"HKCU\"));
        assert_eq!(RUN_KEY_PATH.strip_prefix(r"HKCU\"), Some(RUN_KEY_SUBKEY));
    }

    #[test]
    fn the_run_entry_writer_names_the_hive() {
        // `reg.exe` answers "Invalid key name" for a path without a hive, and
        // the repair then left the entry pointing at the previous version's
        // deleted directory without saying why.
        let arguments = run_entry_add_arguments(r"explorer.exe shell:AppsFolder\pf!Wallpaper");
        let as_str: Vec<&str> = arguments.iter().map(String::as_str).collect();
        assert_eq!(
            as_str,
            vec![
                "add",
                RUN_KEY_PATH,
                "/V",
                RUN_VALUE_NAME,
                "/D",
                r"explorer.exe shell:AppsFolder\pf!Wallpaper",
                "/F",
            ]
        );
        assert!(as_str[1].starts_with(r"HKCU\"));
    }
}
