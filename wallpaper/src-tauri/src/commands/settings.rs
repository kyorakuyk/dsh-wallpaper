use crate::*;
use super::require_settings;

/// `keyring`'s Windows backend stores string secrets in a Generic Credential
/// as a UTF-16 little-endian byte blob. CredUI returns UTF-16 code units, so
/// encode them explicitly rather than relying on the host representation when
/// passing a raw `CredentialBlob` to `CredWriteW`.
#[cfg(all(windows, not(feature = "lite")))]
fn credential_blob_from_prompt_password(password: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(password.len().saturating_mul(std::mem::size_of::<u16>()));
    for code_unit in password {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

/// Do not use ordinary slice assignment for a secret: the optimizer is
/// permitted to remove a write whose result is never read. Volatile writes
/// make the cleanup observable to the machine, without adding a plaintext
/// dependency or copying the key through the WebView.
#[cfg(all(windows, not(feature = "lite")))]
fn secure_zero_u16(secret: &mut [u16]) {
    for value in secret {
        // SAFETY: value points to the current element of the exclusively borrowed mutable slice and is valid and aligned for one u16 write.
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(all(windows, not(feature = "lite")))]
fn secure_zero_bytes(secret: &mut [u8]) {
    for value in secret {
        // SAFETY: value points to the current element of the exclusively borrowed mutable slice and is valid for one byte write.
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

fn set_autostart_blocking(enabled: bool) -> Result<windows_integration::AutostartStatus, String> {
    // The whole operation lives next to the Windows calls it makes: which path
    // is authoritative, what is written, and — new — the read-back that decides
    // whether the settings page may call the change applied. This wrapper only
    // keeps it off the UI thread.
    windows_integration::set_autostart(enabled)
}

#[tauri::command]
pub(crate) async fn set_autostart(
    caller: tauri::WebviewWindow,
    enabled: bool,
) -> Result<windows_integration::AutostartStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || set_autostart_blocking(enabled))
        .await
        .map_err(|error| format!("开机自启操作未完成：{error}"))?
}

#[tauri::command]
pub(crate) async fn autostart_status(
    caller: tauri::WebviewWindow,
) -> Result<windows_integration::AutostartStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(windows_integration::autostart_status)
        .await
        .map_err(|error| format!("读取开机自启状态未完成：{error}"))?
}

/// 「桌面会话」目录名。与桥那边（`bridge/src/index.ts`）保持一致：**同一个名字**既是工作区标题，
/// 也是壁纸数据目录下的那个子目录名。
#[cfg(not(feature = "lite"))]
const DESKTOP_WORKSPACE_DIRECTORY_NAME: &str = "桌面会话";

/// 桌面会话工作区落点的自检信息（只读）。
///
/// 位置规则与桥一致：**壁纸自己的数据目录**（`%LOCALAPPDATA%\com.dsh.wallpaper`，也就是打包标识）
/// 下的「桌面会话」。安装目录不能用——MSIX 每次升级会把整个安装目录替换掉，写在那里的东西必丢；
/// 数据目录则运行时可写、升级保留、卸载也留得下（用户明确要求"卸载之后也保留桌面会话的数据"）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn desktop_workspace_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
    Ok(desktop_workspace_status_from(&data_dir))
}

/// 项目记忆的文件名。**必须与桥里的 `PROJECT_MEMORY_FILE_NAME` 一致** —— 桥按它读/注入，
/// 设置中心按它打开；两处不同的话，用户在设置里看到的和助手实际用的会是两个文件。
#[cfg(not(feature = "lite"))]
const PROJECT_MEMORY_FILE_NAME: &str = "项目记忆.md";

/// 在资源管理器里打开「项目记忆」：文件在就选中它，不在就打开工作区目录（顺带建出来）。
///
/// 为什么要有这个按钮：记忆文件的**绝对路径不该出现在桌面会话里**（那是聊天面，不是文件管理器），
/// 但用户确实需要一个地方去改它 —— 那个地方就是设置，路径也只在这里出现。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn open_project_memory(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
    let workspace = data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME);
    std::fs::create_dir_all(&workspace).map_err(|error| format!("无法创建桌面会话目录：{error}"))?;
    let memory = workspace.join(PROJECT_MEMORY_FILE_NAME);
    // 有文件就选中它（用户一眼看到要改的东西）；没有就把目录打开，别替他造一个空文件。
    let argument = if memory.is_file() {
        format!("/select,{}", memory.display())
    } else {
        workspace.display().to_string()
    };
    std::process::Command::new("explorer.exe")
        .arg(&argument)
        .spawn()
        .map_err(|error| format!("无法打开资源管理器：{error}"))?;
    Ok(serde_json::json!({
        "opened": argument,
        "memoryFile": memory.display().to_string(),
        "memoryExists": memory.is_file(),
    }))
}

/// 「打开 TUI」：把本机的 TUI（`dst`）在一个**新的终端窗口**里拉起来。
///
/// 为什么这件事必须由原生做：能不能开终端窗口、以及**本机到底有没有装 TUI**，都只有原生能回答。
/// 而这条命令的契约是「缺什么就说什么」：找不到 `dst` 时返回 `opened: false` 加一句怎么办，
/// **绝不静默改成打开浏览器** —— 那等于替用户换了一条他没选的路。
///
/// 与 `open_project_memory` 同样的门：**只有设置中心能调**（`require_settings`）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn open_subject_tui(
    caller: tauri::WebviewWindow,
    args: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let args = harness_launch::normalize_launch_args(args)?;
    let launchers = harness_targets::tui_launcher_paths(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("PATH").ok().as_deref(),
    );
    let Some(launcher) = launchers.first() else {
        return Ok(serde_json::json!({
            "opened": false,
            "reason": "not-installed",
            "message": "本机没有找到 TUI（dst）。安装：npm i -g @deepseek-harness-tui/dsh-tui",
        }));
    };
    let (program, command_args) = harness_launch::tui_launch_command(launcher, &args);
    std::process::Command::new(&program)
        .args(&command_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("无法拉起 TUI：{error}"))?;
    Ok(serde_json::json!({
        "opened": true,
        "launcher": launcher.display().to_string(),
    }))
}

/// 「清除全部用户数据」里，**能由本应用安全删掉**的那几处。
///
/// 刻意不含 WebView2 配置目录（设置与网页登录态都住在里面）：那个目录正被运行中的进程占用，
/// 在这里删它只会删到一半；那条路径由命令自己回报给界面，让用户"退出应用后删掉"。
#[cfg(not(feature = "lite"))]
fn clearable_user_data_targets(data_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME)]
}

/// 清除本应用自己的用户数据：桌面会话工作区 + 凭据管理器里那条 API Key。
///
/// 只做这两件**能确定删干净**的事，并把其余（设置 / 网页登录态）连同路径回报出去 —— 卸载时
/// MSIX 没有自定义卸载界面可以问，所以这个显式入口就是"想清干净的时候能清干净"的答案。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn clear_user_data(caller: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    require_settings(&caller)?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?;

    let mut removed: Vec<String> = Vec::new();
    for target in clearable_user_data_targets(&data_dir) {
        if !target.exists() {
            continue;
        }
        std::fs::remove_dir_all(&target).map_err(|error| format!("无法删除 {}：{error}", target.display()))?;
        removed.push(target.display().to_string());
    }

    let credential_removed = remove_api_key_credential()?;

    Ok(serde_json::json!({
        "removed": removed,
        "credentialRemoved": credential_removed,
        // 这两处不在本进程的删除范围内，如实回报路径，让用户自己决定。
        "manual": [
            { "what": "设置与网页登录态", "path": data_dir.display().to_string() },
            { "what": "桥接凭据", "path": "~/.dsh/wallpaper" },
        ],
    }))
}

/// 删除凭据管理器里那条 API Key；本来就不存在也算成功（返回 `false`）。
#[cfg(all(windows, not(feature = "lite")))]
fn remove_api_key_credential() -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = HSTRING::from("deepseek-api.dsh-wallpaper");
    // SAFETY: target is an owned NUL-terminated HSTRING that remains alive for the synchronous Credential Manager call.
    match unsafe { CredDeleteW(&target, CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(all(not(windows), not(feature = "lite")))]
fn remove_api_key_credential() -> Result<bool, String> {
    Err("仅 Windows 支持凭据管理器。".into())
}

#[cfg(all(test, not(feature = "lite")))]
mod desktop_workspace_tests {
    use super::{desktop_workspace_status_from, DESKTOP_WORKSPACE_DIRECTORY_NAME, PROJECT_MEMORY_FILE_NAME};

    /// 位置规则只有一条：数据目录下的「桌面会话」。
    ///
    /// 这条断言的价值在于它同时否掉了两个诱人的位置：安装目录（MSIX 升级时整体替换，写那里的必丢）
    /// 和 DSH 自己的目录（不是"壁纸的目录"，清理时说不清该不该留）。
    #[test]
    fn the_workspace_lives_beside_the_apps_own_data() {
        let status = desktop_workspace_status_from(std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper"));
        assert_eq!(
            status["workspaceDirectory"].as_str().unwrap(),
            format!(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\{DESKTOP_WORKSPACE_DIRECTORY_NAME}")
        );
        assert_eq!(status["dataDirectory"].as_str().unwrap(), r"C:\Users\me\AppData\Local\com.dsh.wallpaper");
        // 「清除全部用户数据」要删的那条凭据在凭据管理器里，不在文件系统上——把名字一并报出来。
        assert_eq!(status["credentialTarget"].as_str().unwrap(), "deepseek-api.dsh-wallpaper");
        // 打包前目录还不存在时也必须给出路径（桥会按同一条规则重建）。
        assert_eq!(status["workspaceExists"].as_bool(), Some(false));
        // 「项目记忆」的路径也要报出来（设置里那个「打开项目记忆」按钮用它），而且**必须与桥读的
        // 是同一个名字**：两处不同的话，用户在设置里打开的和助手实际用的是两个文件。
        assert_eq!(
            status["memoryFile"].as_str().unwrap(),
            format!(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\{DESKTOP_WORKSPACE_DIRECTORY_NAME}\{PROJECT_MEMORY_FILE_NAME}")
        );
        assert_eq!(status["memoryExists"].as_bool(), Some(false));
    }
}

/// 纯函数：只负责"路径怎么算"，好让测试直接钉住它（真实的数据目录是运行时才知道的）。
#[cfg(not(feature = "lite"))]
fn desktop_workspace_status_from(data_dir: &std::path::Path) -> serde_json::Value {
    let workspace = data_dir.join(DESKTOP_WORKSPACE_DIRECTORY_NAME);
    let memory = workspace.join(PROJECT_MEMORY_FILE_NAME);
    serde_json::json!({
        "dataDirectory": data_dir.display().to_string(),
        "workspaceDirectory": workspace.display().to_string(),
        "workspaceExists": workspace.is_dir(),
        // 助手维护的「项目记忆」（说话人格等长期要求就落在这里）；设置里给它一个打开入口，
        // 桌面会话里不贴路径。
        "memoryFile": memory.display().to_string(),
        "memoryExists": memory.is_file(),
        // 清除全部用户数据时要一并删掉的那条凭据（在凭据管理器里，不在文件系统上）。
        "credentialTarget": "deepseek-api.dsh-wallpaper",
    })
}

/// Set the API-key credential in Windows Credential Manager.
/// 把 API Key 写进当前用户的**通用凭据**（target / 用户名与 `keyring` 的 Windows 映射一致，
/// 所以 API 客户端读到的是同一条；keyring 的 `set_password` 把 UTF-16 小端字节当成 blob，
/// 这里必须写成同一个形状）。
#[cfg(all(windows, not(feature = "lite")))]
fn write_api_key_credential(key: &str) -> Result<(), String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::FILETIME,
            Security::Credentials::{
                CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_ENTERPRISE, CRED_TYPE_GENERIC,
            },
        },
    };

    const CREDENTIAL_TARGET: &str = "deepseek-api.dsh-wallpaper";
    const CREDENTIAL_USERNAME: &str = "deepseek-api";

    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("API Key 不能为空。".into());
    }
    // CredUI 那条路的上限是 256 个 UTF-16 码元；这里直接输入也守同一个界，
    // 免得把一条明显不是 Key 的长文本写进凭据管理器。
    let mut units: Vec<u16> = trimmed.encode_utf16().collect();
    if units.len() > 256 {
        secure_zero_u16(&mut units);
        return Err("API Key 过长（最多 256 个字符）。".into());
    }
    let mut target = CREDENTIAL_TARGET.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut username = CREDENTIAL_USERNAME.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut blob = credential_blob_from_prompt_password(&units);
    let byte_len = u32::try_from(blob.len()).map_err(|_| "API Key 长度无效。".to_string())?;
    let mut credential = CREDENTIALW {
        Flags: CRED_FLAGS::default(),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        Comment: PWSTR::null(),
        LastWritten: FILETIME::default(),
        CredentialBlobSize: byte_len,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_ENTERPRISE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(username.as_mut_ptr()),
    };
    // SAFETY: credential is live stack storage; target and username are NUL-terminated Vecs, and blob pointer/size describe the live blob Vec for this synchronous call.
    let result = unsafe { CredWriteW(&mut credential, 0) }
        .map_err(|error| format!("无法保存 API Key 到 Windows 凭据管理器：{error}"));
    secure_zero_bytes(&mut blob);
    secure_zero_u16(&mut units);
    secure_zero_u16(&mut target);
    secure_zero_u16(&mut username);
    result
}

/// 展示用的脱敏形式：只留头 3 与尾 4，中间一律替换。
///
/// 这是**唯一**被允许离开原生进程的 Key 形态——渲染端没有任何命令能把明文读回来，
/// 设置中心显示的也只是这个字符串。短 Key 不做"留一半"的漂亮处理：本来就短，全遮。
#[cfg(all(windows, not(feature = "lite")))]
fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    let len = chars.len();
    let tail = |count: usize| -> String { chars[len - count..].iter().collect() };
    match len {
        0 => String::new(),
        // 短到没什么可藏的：整串遮掉，别为了好看泄漏一半。
        1..=6 => "•".repeat(len),
        7..=12 => format!("{}{}", "•".repeat(len - 2), tail(2)),
        _ => format!("{}••••••••{}", chars[..3].iter().collect::<String>(), tail(4)),
    }
}

/// Stores the key the user typed in Settings. This is the one place a plaintext API key
/// crosses the Tauri IPC boundary, and it does so **once, on the way in**: the renderer
/// never receives a key back (`api_key_status` answers with `mask_api_key` only), and the
/// chat client reads the credential natively.
///
/// 这条路径取代了原来那个系统凭据对话框（`prompt_for_api_key`）：用户实测明确要求"在我们
/// 的设置窗内输入"，宁可放弃"明文从不经过渲染端"这条性质，换一个正常的输入体验。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn save_api_key(caller: tauri::WebviewWindow, key: String) -> Result<(), String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以更新 API Key。".into());
    }

    #[cfg(windows)]
    {
        write_api_key_credential(&key)
    }

    #[cfg(not(windows))]
    {
        let _ = key;
        Err("API Key 仅支持 Windows 凭据管理器。".into())
    }
}

/// Whether a key is stored, and the masked form to show in Settings.
///
/// 回答里**永远没有明文**：脱敏在原生侧做完，渲染端拿到的最多是一串点和首尾几个字符。
/// 没有 Key 时返回 `present: false`，而不是报错——"还没配"是正常状态。
#[cfg(all(windows, not(feature = "lite")))]
fn api_key_status_from_store() -> Result<serde_json::Value, String> {
    let entry = keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|_| "无法访问 Windows 凭据管理器。请检查系统凭据服务后重试。".to_string())?;
    match entry.get_password() {
        Ok(key) => Ok(serde_json::json!({ "present": true, "masked": mask_api_key(&key) })),
        Err(_) => Ok(serde_json::json!({ "present": false })),
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn api_key_status(caller: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    if caller.label() != SETTINGS_WINDOW_LABEL {
        return Err("仅设置中心可以读取 API Key 状态。".into());
    }

    #[cfg(windows)]
    {
        api_key_status_from_store()
    }

    #[cfg(not(windows))]
    {
        Ok(serde_json::json!({ "present": false }))
    }
}

#[cfg(all(test, windows))]
#[cfg(not(feature = "lite"))]
mod api_credential_tests {
    use super::{credential_blob_from_prompt_password, mask_api_key};

    #[test]
    fn serializes_the_same_utf16_little_endian_shape_as_keyring_windows() {
        assert_eq!(
            credential_blob_from_prompt_password(&[0x0073, 0x006B, 0x4F60]),
            vec![0x73, 0x00, 0x6B, 0x00, 0x60, 0x4F]
        );
    }

    /// 设置中心显示的是这个字符串——它必须"认得出是哪一条"，同时**不能**泄漏中间那段，
    /// 因为这是渲染端唯一能拿到的 Key 形态（没有任何命令能读回明文）。
    #[test]
    fn never_reveals_the_middle_of_a_stored_key() {
        assert_eq!(mask_api_key("sk-1234567890abcdef"), "sk-••••••••cdef");
        // 首尾照旧看得见，中间那段原样字符一个都不许出现。
        let masked = mask_api_key("sk-1234567890abcdef");
        assert!(!masked.contains("1234567890"));
        assert!(masked.ends_with("cdef"));
        // 前后空白不是 Key 的一部分。
        assert_eq!(mask_api_key("  sk-1234567890abcdef  "), "sk-••••••••cdef");
    }

    /// 短 Key 不玩"留一半"的漂亮处理：全遮，宁可少显示也不多泄漏。
    #[test]
    fn hides_short_keys_entirely_rather_than_half_of_them() {
        assert_eq!(mask_api_key(""), "");
        assert_eq!(mask_api_key("abc"), "•••");
        assert_eq!(mask_api_key("abcdef"), "••••••");
        // 再长一点只留最后两个字符。
        assert_eq!(mask_api_key("abcdefghi"), "•••••••hi");
    }

    /// 真机探针：设置中心那个「测试」按钮背后就是这一次调用（`/models` 要密钥，200 即密钥可用，
    /// 返回体就是可用模型列表）。忽略是刻意的：它要联网，而且依赖这台机器真的存了 Key。
    ///
    /// `cargo test --lib api_credential -- --ignored --nocapture`
    #[test]
    #[ignore = "calls the real DeepSeek API with this machine's stored key"]
    fn this_machine_reports_what_testing_the_stored_api_key_would_answer() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let status = super::api_key_status_from_store();
        match &status {
            Ok(value) => eprintln!("stored key: {value}"),
            Err(error) => eprintln!("stored key unreadable: {error}"),
        }
        let answer = rt.block_on(crate::chat::api_models("https://api.deepseek.com".into()));
        match &answer {
            Ok(payload) => {
                let models = payload.get("models").and_then(serde_json::Value::as_array);
                eprintln!(
                    "models -> supported={} count={}",
                    payload.get("supported").and_then(serde_json::Value::as_bool).unwrap_or(false),
                    models.map(Vec::len).unwrap_or(0)
                );
                for model in models.unwrap_or(&vec![]).iter().take(12) {
                    eprintln!("  {} / {}", model.get("id").and_then(serde_json::Value::as_str).unwrap_or("?"), model.get("name").and_then(serde_json::Value::as_str).unwrap_or("?"));
                }
            }
            Err(error) => eprintln!("models rejected -> {error}"),
        }
    }
}
