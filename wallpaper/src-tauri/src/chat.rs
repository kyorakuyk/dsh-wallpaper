use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

#[derive(Default)]
pub struct ChatState {
    api_cancel: Mutex<Option<oneshot::Sender<()>>>,
    harness_cancel: Mutex<Option<oneshot::Sender<()>>>,
    harness_session: Mutex<Option<String>>,
    api_conversations: Mutex<HashMap<String, ApiConversation>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Default)]
struct ApiConversation {
    messages: Vec<ApiMessage>,
}

fn new_conversation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("api-{now:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum ChatEvent {
    Status {
        activity: String,
    },
    Delta {
        text: String,
    },
    Message {
        role: String,
        content: String,
    },
    Usage {
        input: u64,
        output: u64,
        #[serde(rename = "cacheRead", skip_serializing_if = "Option::is_none")]
        cache_read: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
    },
    Model {
        provider: Option<String>,
        model: String,
        tier: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
    },
    Error {
        code: String,
        recoverable: bool,
        message: String,
    },
}

#[derive(Deserialize)]
struct ApiChunk {
    choices: Option<Vec<Choice>>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct Choice {
    delta: Delta,
}
#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}
#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    prompt_cache_hit_tokens: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessSessionRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_session_id: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HarnessSessionResponse {
    session_id: String,
    provider: Option<String>,
    model: Option<String>,
}

fn emit_error(app: &AppHandle, code: &str, recoverable: bool, message: impl Into<String>) {
    let _ = app.emit(
        "chat-event",
        ChatEvent::Error {
            code: code.into(),
            recoverable,
            message: message.into(),
        },
    );
}

pub async fn send_api(
    app: AppHandle,
    state: tauri::State<'_, ChatState>,
    text: String,
    base_url: String,
    model: String,
    conversation_id: Option<String>,
) -> Result<String, String> {
    cancel_api(&state);
    let conversation_id = conversation_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(new_conversation_id);
    let messages = {
        let mut conversations = state
            .api_conversations
            .lock()
            .map_err(|_| "API conversation state poisoned")?;
        let conversation = conversations.entry(conversation_id.clone()).or_default();
        conversation.messages.push(ApiMessage {
            role: "user".into(),
            content: text,
        });
        conversation.messages.clone()
    };
    let key = keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|e| e.to_string())?
        .get_password()
        .map_err(|_| "未在 Windows 凭据管理器配置 API Key".to_string())?;
    let response = reqwest::Client::new().post(format!("{}/chat/completions", base_url.trim_end_matches('/')))
        .bearer_auth(key)
        .json(&serde_json::json!({ "model": model, "stream": true, "stream_options": { "include_usage": true }, "messages": messages }))
        .send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("DeepSeek API HTTP {}", response.status()));
    }
    let _ = app.emit(
        "chat-event",
        ChatEvent::Model {
            provider: Some("deepseek".into()),
            model,
            tier: "unknown".into(),
            effort: None,
        },
    );
    let _ = app.emit(
        "chat-event",
        ChatEvent::Status {
            activity: "streaming".into(),
        },
    );
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    *state
        .api_cancel
        .lock()
        .map_err(|_| "API cancel state poisoned")? = Some(cancel_tx);
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
    loop {
        tokio::select! {
            _ = &mut cancel_rx => { let _ = app.emit("chat-event", ChatEvent::Status { activity: "idle".into() }); break; }
            chunk = stream.next() => {
                let Some(chunk) = chunk else { break };
                buffer.push_str(&String::from_utf8_lossy(&chunk.map_err(|e| e.to_string())?));
                while let Some(end) = buffer.find("\n\n") {
                    let record = buffer[..end].to_string(); buffer.drain(..end + 2);
                    for line in record.lines().filter_map(|line| line.strip_prefix("data: ")) {
                        if line == "[DONE]" { continue; }
                        if let Ok(data) = serde_json::from_str::<ApiChunk>(line) {
                            if let Some(delta) = data.choices.and_then(|v| v.into_iter().next()).and_then(|c| c.delta.content) { full.push_str(&delta); let _ = app.emit("chat-event", ChatEvent::Delta { text: delta }); }
                            if let Some(usage) = data.usage { let cached = usage.prompt_cache_hit_tokens.unwrap_or(0); let _ = app.emit("chat-event", ChatEvent::Usage { input: usage.prompt_tokens.saturating_sub(cached), output: usage.completion_tokens, cache_read: usage.prompt_cache_hit_tokens, cost: None }); }
                        }
                    }
                }
            }
        }
    }
    if !full.is_empty() {
        if let Ok(mut conversations) = state.api_conversations.lock() {
            conversations
                .entry(conversation_id.clone())
                .or_default()
                .messages
                .push(ApiMessage {
                    role: "assistant".into(),
                    content: full.clone(),
                });
        }
        let _ = app.emit(
            "chat-event",
            ChatEvent::Message {
                role: "assistant".into(),
                content: full,
            },
        );
    }
    let _ = app.emit(
        "chat-event",
        ChatEvent::Status {
            activity: "done".into(),
        },
    );
    if let Ok(mut guard) = state.api_cancel.lock() {
        guard.take();
    }
    Ok(conversation_id)
}

pub fn api_history(state: &ChatState, conversation_id: &str) -> Result<Value, String> {
    let conversations = state
        .api_conversations
        .lock()
        .map_err(|_| "API conversation state poisoned")?;
    let messages = conversations
        .get(conversation_id)
        .map(|conversation| conversation.messages.clone())
        .unwrap_or_default();
    Ok(serde_json::json!({ "messages": messages }))
}

pub fn cancel_api(state: &ChatState) {
    if let Ok(mut guard) = state.api_cancel.lock() {
        if let Some(cancel) = guard.take() {
            let _ = cancel.send(());
        }
    }
}

fn bridge_token_path() -> Result<PathBuf, String> {
    if let Ok(root) = std::env::var("DSH_HOME") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed)
                .join("wallpaper")
                .join("bridge-token"));
        }
    }
    dirs::home_dir()
        .map(|home| home.join(".dsh").join("wallpaper").join("bridge-token"))
        .ok_or_else(|| "无法确定 DSH 用户目录".into())
}

fn read_bridge_token() -> Result<String, String> {
    let token = std::fs::read_to_string(bridge_token_path()?).map_err(|_| {
        "未找到 DSH 壁纸 bridge token；请先安装并启动 dsh-wallpaper-bridge".to_string()
    })?;
    let token = token.trim().to_string();
    if token.len() < 32 {
        return Err("DSH bridge token 无效".into());
    }
    Ok(token)
}

fn auth(client: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    client.bearer_auth(token)
}

pub async fn harness_connect(
    app: AppHandle,
    state: tauri::State<'_, ChatState>,
    resume_session_id: Option<String>,
) -> Result<String, String> {
    cancel_harness_stream(&state);
    let token = read_bridge_token()?;
    let response = auth(
        reqwest::Client::new().post("http://127.0.0.1:3080/api/wallpaper/v1/sessions"),
        &token,
    )
    .json(&HarnessSessionRequest {
        resume_session_id: resume_session_id.as_deref(),
    })
    .send()
    .await
    .map_err(|e| format!("连接 DSH bridge 失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("DSH bridge HTTP {}", response.status()));
    }
    let session: HarnessSessionResponse = response.json().await.map_err(|e| e.to_string())?;
    *state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")? = Some(session.session_id.clone());
    if let Some(model) = session.model {
        let _ = app.emit(
            "chat-event",
            ChatEvent::Model {
                provider: session.provider,
                model,
                tier: "unknown".into(),
                effort: None,
            },
        );
    }
    spawn_harness_stream(app, state.inner(), session.session_id.clone(), token);
    Ok(session.session_id)
}

fn spawn_harness_stream(app: AppHandle, state: &ChatState, session_id: String, token: String) {
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    if let Ok(mut guard) = state.harness_cancel.lock() {
        *guard = Some(cancel_tx);
    }
    tauri::async_runtime::spawn(async move {
        let url = format!(
            "http://127.0.0.1:3080/api/wallpaper/v1/sessions/{}/events",
            urlencoding::encode(&session_id)
        );
        let response = match auth(reqwest::Client::new().get(url), &token).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                emit_error(
                    &app,
                    "HARNESS_SSE_HTTP",
                    true,
                    format!("DSH SSE HTTP {}", response.status()),
                );
                return;
            }
            Err(error) => {
                emit_error(&app, "HARNESS_SSE_CONNECT", true, error.to_string());
                return;
            }
        };
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        loop {
            tokio::select! {
                _ = &mut cancel_rx => break,
                chunk = stream.next() => {
                    let Some(chunk) = chunk else { emit_error(&app, "HARNESS_DISCONNECTED", true, "DSH bridge 事件流已断开"); break; };
                    match chunk {
                        Ok(bytes) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                            while let Some(end) = buffer.find("\n\n") {
                                let record = buffer[..end].to_string(); buffer.drain(..end + 2);
                                for line in record.lines().filter_map(|line| line.strip_prefix("data: ")) {
                                    if let Ok(mut event) = serde_json::from_str::<Value>(line) {
                                        if event.get("type").and_then(Value::as_str) == Some("model") && event.get("tier").is_none() {
                                            if let Some(map) = event.as_object_mut() { map.insert("tier".into(), Value::String("unknown".into())); }
                                        }
                                        let _ = app.emit("chat-event", event);
                                    }
                                }
                            }
                        }
                        Err(error) => { emit_error(&app, "HARNESS_SSE_READ", true, error.to_string()); break; }
                    }
                }
            }
        }
    });
}

pub async fn harness_send(state: tauri::State<'_, ChatState>, text: String) -> Result<(), String> {
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let url = format!(
        "http://127.0.0.1:3080/api/wallpaper/v1/sessions/{}/messages",
        urlencoding::encode(&session_id)
    );
    let response = auth(reqwest::Client::new().post(url), &token)
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("DSH bridge HTTP {}", response.status()));
    }
    Ok(())
}

pub async fn harness_history(state: tauri::State<'_, ChatState>) -> Result<Value, String> {
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let url = format!(
        "http://127.0.0.1:3080/api/wallpaper/v1/sessions/{}/history",
        urlencoding::encode(&session_id)
    );
    let response = auth(reqwest::Client::new().get(url), &token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("DSH bridge HTTP {}", response.status()));
    }
    response.json().await.map_err(|e| e.to_string())
}

pub async fn harness_cancel(state: tauri::State<'_, ChatState>) -> Result<(), String> {
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let url = format!(
        "http://127.0.0.1:3080/api/wallpaper/v1/sessions/{}/cancel",
        urlencoding::encode(&session_id)
    );
    let response = auth(reqwest::Client::new().post(url), &token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("DSH bridge HTTP {}", response.status()));
    }
    Ok(())
}

pub fn cancel_harness_stream(state: &ChatState) {
    if let Ok(mut guard) = state.harness_cancel.lock() {
        if let Some(cancel) = guard.take() {
            let _ = cancel.send(());
        }
    }
}
