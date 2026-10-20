use crate::*;
use crate::commands::require_background;

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn connect_harness(
    caller: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, chat::ChatState>,
    resume_session_id: Option<String>,
    connection_id: String,
    model: Option<String>,
    preset: Option<String>,
) -> Result<String, String> {
    require_background(&caller)?;
    // The session follows the endpoint selected in settings, resolved here rather
    // than trusted from the caller: the renderer cannot name an arbitrary port,
    // and the monitor reads the same value, so status and sessions cannot
    // disagree about which client is in use.
    let endpoint_port = Some(crate::commands::harness_status::harness_endpoint_port());
    chat::harness_connect(app, state, resume_session_id, connection_id, model, endpoint_port, preset).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_history(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_history(state).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_presets(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_presets(state).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_set_preset(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    preset: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_set_preset(state, preset).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_controls(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_controls(state).await
}

/// 枚举当前 Harness 主体（检出 / 桌面 / 官壳）可用的模型。
///
/// 名单由宿主提供、经桥接转发；宿主没有该能力时桥接报 `supported: false`，
/// 前端据此退化成"只显示当前模型"，而不是摆一串会被拒绝的 id。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_models(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_models(state).await
}

/// 把壁纸选定的模型推给 Harness 宿主，使宿主默认模型与壁纸一致。
#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_set_model(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    model: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_set_model(state, model).await
}

#[tauri::command]
#[cfg(not(feature = "lite"))]
pub(crate) async fn harness_set_permission(
    caller: tauri::WebviewWindow,
    state: tauri::State<'_, chat::ChatState>,
    permission: String,
) -> Result<serde_json::Value, String> {
    require_background(&caller)?;
    chat::harness_set_permission(state, permission).await
}
