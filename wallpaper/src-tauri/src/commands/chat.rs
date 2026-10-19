use crate::*;
use super::{require_api_history_owner, require_background, require_wallpaper_surface};

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn send_chat(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    web_state: tauri::State<'_, deepseek_web::DeepSeekWebState>,
    mode: String,
    text: String,
    conversation_id: Option<String>,
    request_id: Option<String>,
    new_conversation: Option<bool>,
    base_url: Option<String>,
    model: Option<String>,
    price_input_per_million: Option<f64>,
    price_output_per_million: Option<f64>,
) -> Result<Option<String>, String> {
    require_background(&caller)?;
    match mode.as_str() {
        "deepseek-web" => deepseek_web::send(
            app,
            web_state.inner(),
            text,
            conversation_id,
            request_id,
            new_conversation.unwrap_or(false),
        )
        .await
        .map(Some),
        "deepseek-api" => chat::send_api(
            app,
            state,
            text,
            base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            model.unwrap_or_else(|| "deepseek-chat".into()),
            conversation_id,
            request_id,
            price_input_per_million,
            price_output_per_million,
        )
        .await
        .map(Some),
        "harness" => chat::harness_send(state, text).await.map(|_| None),
        _ => Err("DeepSeek 网页桥接需要登录 WebView 和 DOM adapter".into()),
    }
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) fn api_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    conversation_id: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::api_history(state.inner(), &conversation_id, limit)
}

/// List the durable API transcripts so the settings center can offer a delete
/// control. Returns metadata only; message bodies stay behind `api_history`.
///
/// The settings WebView is a separate process with an isolated storage
/// partition, so its in-memory archive is a snapshot from its own start. The
/// listing therefore reads the archive through the store rather than reporting
/// that snapshot.
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn list_api_conversations(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_api_history_owner(&caller)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || chat::list_api_conversations(&state))
        .await
        .map_err(|_| "读取 API 会话列表失败".to_string())?
}

/// Deleting durable API history touches the DPAPI archive, which is a file
/// write plus a cross-process mutex wait. Keep it off the WebView command
/// thread so a slow disk cannot freeze the calling surface.
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn delete_api_conversation(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    conversation_id: String,
) -> Result<bool, String> {
    require_api_history_owner(&caller)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        chat::delete_api_conversation(&state, &conversation_id)
    })
    .await
    .map_err(|_| "删除 API 会话记录失败".to_string())?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn clear_api_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<usize, String> {
    require_api_history_owner(&caller)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || chat::clear_api_history(&state))
        .await
        .map_err(|_| "清空 API 会话记录失败".to_string())?
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn cancel_chat(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    web_state: tauri::State<'_, deepseek_web::DeepSeekWebState>,
    mode: String,
) -> Result<(), String> {
    require_background(&caller)?;
    if mode == "deepseek-web" {
        deepseek_web::cancel(&app, web_state.inner()).await
    } else if mode == "harness" {
        chat::harness_cancel(state).await
    } else {
        chat::cancel_api(&state);
        Ok(())
    }
}

/// 枚举 DeepSeek API 端点自己列出的模型（兼容接口的 `/models`）。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn api_models(caller: tauri::WebviewWindow, base_url: String) -> Result<serde_json::Value, String> {
    // 两个界面都要用这一份目录：壁纸端的模型选择器（Harness 兼容端点）与设置中心的
    // 「测试／刷新」（用户要在那里看到"现在有哪些模型"）。**不是**放宽给球或网页窗口——
    // 它仍然只认这两个表面，`require_wallpaper_surface` 就是这条边界。
    require_wallpaper_surface(&caller)?;
    chat::api_models(base_url).await
}
