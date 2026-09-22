//! Declarative configuration for the DeepSeek Web DOM adapter.
//!
//! The executable owns the adapter engine and its safety limits. This file
//! only supplies selector and label data, so a site markup adjustment can be
//! distributed as a small user override without shipping arbitrary code.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(windows)]
use tauri::{AppHandle, Manager};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const CONFIG_FILE_NAME: &str = "deepseek-web-adapter.override.json";
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_LIST_ITEMS: usize = 32;
const MAX_SELECTOR_BYTES: usize = 1024;
const MAX_TOKEN_BYTES: usize = 128;
const MAX_VERSION_BYTES: usize = 64;
const BUILTIN_CONFIG_JSON: &str = include_str!("../config/deepseek-web-adapter.json");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeepSeekWebAdapterConfig {
    pub schema_version: u32,
    pub adapter_version: String,
    /// Kept fixed by validation; the adapter never sends credentials outside
    /// the official DeepSeek host.
    pub site_origin: String,
    pub conversation_path_template: String,
    pub composer_selectors: Vec<String>,
    pub assistant_selectors: Vec<String>,
    pub markdown_selectors: Vec<String>,
    pub message_selectors: Vec<String>,
    pub app_shell_selectors: Vec<String>,
    pub loading_class: String,
    pub send_tokens: Vec<String>,
    pub stop_tokens: Vec<String>,
    pub terminal_tokens: Vec<String>,
    pub login_tokens: Vec<String>,
    pub assistant_role_tokens: Vec<String>,
    pub user_role_tokens: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LoadedAdapterConfig {
    pub config: DeepSeekWebAdapterConfig,
    pub source: &'static str,
    pub path: PathBuf,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterConfigStatus {
    pub schema_version: u32,
    pub adapter_version: String,
    pub source: String,
    pub path: String,
    pub warning: Option<String>,
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.as_bytes().len() <= max_bytes
        && !value.chars().any(|character| character.is_control())
        && !value.to_ascii_lowercase().contains("javascript:")
}

fn valid_list(values: &[String], max_bytes: usize) -> bool {
    !values.is_empty()
        && values.len() <= MAX_LIST_ITEMS
        && values.iter().all(|value| valid_text(value, max_bytes))
}

pub fn validate(config: &DeepSeekWebAdapterConfig) -> Result<(), String> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "不支持的 DeepSeek 网页配置版本：{}",
            config.schema_version
        ));
    }
    if !valid_text(&config.adapter_version, MAX_VERSION_BYTES) {
        return Err("适配器版本号无效。".into());
    }
    if config.site_origin != "https://chat.deepseek.com" {
        return Err("网页适配器只能使用官方 DeepSeek 地址。".into());
    }
    if !valid_text(&config.conversation_path_template, 256)
        || !config.conversation_path_template.starts_with('/')
        || config.conversation_path_template.matches("{id}").count() != 1
        || config.conversation_path_template.contains('?')
        || config.conversation_path_template.contains('#')
        || config.conversation_path_template.contains("..")
        || config
            .conversation_path_template
            .chars()
            .any(|character| !character.is_ascii() || character.is_control())
    {
        return Err("会话路由模板无效；必须是包含一个 {id} 的路径。".into());
    }
    if !valid_list(&config.composer_selectors, MAX_SELECTOR_BYTES)
        || !valid_list(&config.assistant_selectors, MAX_SELECTOR_BYTES)
        || !valid_list(&config.markdown_selectors, MAX_SELECTOR_BYTES)
        || !valid_list(&config.message_selectors, MAX_SELECTOR_BYTES)
        || !valid_list(&config.app_shell_selectors, MAX_SELECTOR_BYTES)
    {
        return Err("网页适配器选择器配置无效。".into());
    }
    if !valid_text(&config.loading_class, MAX_TOKEN_BYTES)
        || config
            .loading_class
            .chars()
            .any(|character| !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_'))
    {
        return Err("网页加载状态 class 无效。".into());
    }
    if !valid_list(&config.send_tokens, MAX_TOKEN_BYTES)
        || !valid_list(&config.stop_tokens, MAX_TOKEN_BYTES)
        || !valid_list(&config.terminal_tokens, MAX_TOKEN_BYTES)
        || !valid_list(&config.login_tokens, MAX_TOKEN_BYTES)
        || !valid_list(&config.assistant_role_tokens, MAX_TOKEN_BYTES)
        || !valid_list(&config.user_role_tokens, MAX_TOKEN_BYTES)
    {
        return Err("网页适配器标签配置无效。".into());
    }
    Ok(())
}

pub(crate) fn builtin_config() -> Result<DeepSeekWebAdapterConfig, String> {
    let config = serde_json::from_str::<DeepSeekWebAdapterConfig>(BUILTIN_CONFIG_JSON)
        .map_err(|error| format!("内置 DeepSeek 网页配置损坏：{error}"))?;
    validate(&config).map_err(|error| format!("内置 DeepSeek 网页配置无效：{error}"))?;
    Ok(config)
}

#[cfg(windows)]
pub fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(CONFIG_FILE_NAME))
        .map_err(|error| format!("无法定位 DeepSeek 网页配置目录：{error}"))
}

#[cfg(windows)]
fn read_override(path: &Path) -> Result<DeepSeekWebAdapterConfig, String> {
    let metadata = std::fs::metadata(path).map_err(|error| format!("无法读取配置大小：{error}"))?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("本地网页适配器配置超过 64 KB。".into());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("无法读取网页适配器配置：{error}"))?;
    let config = serde_json::from_slice::<DeepSeekWebAdapterConfig>(&bytes)
        .map_err(|error| format!("网页适配器 JSON 无效：{error}"))?;
    validate(&config)?;
    Ok(config)
}

#[cfg(windows)]
pub fn load(app: &AppHandle) -> Result<LoadedAdapterConfig, String> {
    let builtin = builtin_config()?;
    let path = config_path(app)?;
    if !path.exists() {
        return Ok(LoadedAdapterConfig {
            config: builtin,
            source: "builtin",
            path,
            warning: None,
        });
    }
    match read_override(&path) {
        Ok(config) => Ok(LoadedAdapterConfig {
            config,
            source: "local",
            path,
            warning: None,
        }),
        Err(error) => Ok(LoadedAdapterConfig {
            config: builtin,
            source: "builtin",
            path,
            warning: Some(format!("本地配置无效，已回退内置配置：{error}")),
        }),
    }
}

#[cfg(windows)]
fn status_of(loaded: &LoadedAdapterConfig) -> AdapterConfigStatus {
    AdapterConfigStatus {
        schema_version: loaded.config.schema_version,
        adapter_version: loaded.config.adapter_version.clone(),
        source: loaded.source.into(),
        path: loaded.path.to_string_lossy().into_owned(),
        warning: loaded.warning.clone(),
    }
}

#[cfg(windows)]
pub fn status(app: &AppHandle) -> Result<AdapterConfigStatus, String> {
    Ok(status_of(&load(app)?))
}

#[cfg(windows)]
fn write_override(path: &Path, config: &DeepSeekWebAdapterConfig) -> Result<(), String> {
    validate(config)?;
    let parent = path
        .parent()
        .ok_or_else(|| "网页适配器配置目录无效。".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建网页适配器配置目录：{error}"))?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| format!("网页适配器配置序列化失败：{error}"))?;
    let temporary = parent.join(format!(".{CONFIG_FILE_NAME}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("无法写入网页适配器配置：{error}"))?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("无法替换网页适配器配置：{error}"));
    }
    Ok(())
}

#[cfg(windows)]
pub fn open(app: &AppHandle) -> Result<AdapterConfigStatus, String> {
    let path = config_path(app)?;
    if !path.exists() {
        write_override(&path, &builtin_config()?)?;
    }
    let mut command = std::process::Command::new("notepad.exe");
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
    command
        .arg(&path)
        .spawn()
        .map_err(|error| format!("无法打开网页适配器配置：{error}"))?;
    status(app)
}

#[cfg(windows)]
pub fn reset(app: &AppHandle) -> Result<AdapterConfigStatus, String> {
    let path = config_path(app)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法移除网页适配器 override：{error}")),
    }
    status(app)
}

#[cfg(test)]
mod tests {
    use super::{builtin_config, validate, DeepSeekWebAdapterConfig};

    #[test]
    fn builtin_config_is_valid_and_declarative() {
        let config = builtin_config().expect("builtin adapter config");
        assert_eq!(config.conversation_path_template, "/a/chat/s/{id}");
        assert!(config
            .composer_selectors
            .iter()
            .any(|value| value == "textarea"));
    }

    #[test]
    fn config_cannot_change_the_official_origin_or_execute_code() {
        let mut config = builtin_config().expect("builtin adapter config");
        config.site_origin = "https://evil.example".into();
        assert!(validate(&config).is_err());

        let mut config = builtin_config().expect("builtin adapter config");
        config.composer_selectors = vec!["javascript:alert(1)".into()];
        assert!(validate(&config).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected_by_the_json_schema() {
        let result = serde_json::from_str::<DeepSeekWebAdapterConfig>(
            r#"{"schemaVersion":1,"adapterVersion":"test","siteOrigin":"https://chat.deepseek.com","conversationPathTemplate":"/a/chat/s/{id}","composerSelectors":["textarea"],"assistantSelectors":[".assistant"],"markdownSelectors":[".markdown"],"messageSelectors":["article"],"appShellSelectors":[".app"],"loadingClass":"loading","sendTokens":["send"],"stopTokens":["stop"],"terminalTokens":["retry"],"loginTokens":["login"],"assistantRoleTokens":["assistant"],"userRoleTokens":["user"],"unexpected":true}"#,
        );
        assert!(result.is_err());
    }
}
