use super::{require_background, require_settings, require_wallpaper_surface};

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn show_deepseek_login(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_wallpaper_surface(&caller)?;
    tauri::async_runtime::spawn_blocking(move || crate::deepseek_web::show_login(&app))
        .await
        .map_err(|error| format!("DeepSeek 应用内页面启动未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn deepseek_web_ensure(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
) -> Result<(), String> {
    require_background(&caller)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::deepseek_web::ensure(&app, conversation_id, new_conversation.unwrap_or(false))
    })
    .await
    .map_err(|error| format!("DeepSeek 网页预热未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn deepseek_web_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<crate::deepseek_web::WebStatus, String> {
    require_background(&caller)?;
    crate::deepseek_web::status(&app).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn deepseek_web_history(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
) -> Result<crate::deepseek_web::WebHistory, String> {
    require_background(&caller)?;
    crate::deepseek_web::history(&app, conversation_id, new_conversation.unwrap_or(false)).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn deepseek_web_adapter_config_status(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<crate::deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || crate::deepseek_web_config::status(&app))
        .await
        .map_err(|error| format!("读取 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn open_deepseek_web_adapter_config(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<crate::deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || crate::deepseek_web_config::open(&app))
        .await
        .map_err(|error| format!("打开 DeepSeek 网页配置未完成：{error}"))?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn reset_deepseek_web_adapter_config(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<crate::deepseek_web_config::AdapterConfigStatus, String> {
    require_settings(&caller)?;
    tauri::async_runtime::spawn_blocking(move || crate::deepseek_web_config::reset(&app))
        .await
        .map_err(|error| format!("恢复 DeepSeek 网页配置未完成：{error}"))?
}
