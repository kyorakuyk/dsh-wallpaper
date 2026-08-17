use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

const MAX_HARNESS_MESSAGE_BYTES: usize = 100_000;

fn generic_api_error(stage: &str) -> String {
    format!("DeepSeek API {stage}失败；请检查地址、网络与访问密钥后重试。")
}

fn generic_harness_error(stage: &str) -> String {
    format!("DSH bridge {stage}失败；请确认 Harness 与壁纸 bridge 仍在运行。")
}

fn generic_bridge_http_error(status: reqwest::StatusCode) -> String {
    format!(
        "DSH bridge 请求被拒绝（HTTP {}）。请确认 bridge 版本、会话状态与连接后重试。",
        status.as_u16()
    )
}

#[derive(Default)]
pub struct ChatState {
    api_cancel: Mutex<Option<ApiCancellation>>,
    harness_cancel: Mutex<Option<oneshot::Sender<()>>>,
    harness_session: Mutex<Option<String>>,
    api_conversations: Mutex<HashMap<String, ApiConversation>>,
}

struct ApiCancellation {
    request_id: u64,
    sender: Option<oneshot::Sender<()>>,
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
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("api-{now:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn next_api_request_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn api_stream_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // A response stream can legitimately last far longer than reqwest's
        // default whole-request timeout. Cancellation remains explicit through
        // the oneshot held by ChatState.
        .timeout(Duration::from_secs(24 * 60 * 60))
        .connect_timeout(Duration::from_secs(12))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .map_err(|_| "无法初始化 DeepSeek API 客户端".to_string())
}

fn bridge_request_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "无法初始化 DSH bridge 客户端".to_string())
}

fn bridge_stream_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // Same as API streaming: a healthy idle SSE connection must not be
        // cut after an arbitrary total duration.
        .timeout(Duration::from_secs(24 * 60 * 60))
        .connect_timeout(Duration::from_secs(3))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .map_err(|_| "无法初始化 DSH bridge 事件流客户端".to_string())
}

fn begin_api_request(state: &ChatState) -> Result<(u64, oneshot::Receiver<()>), String> {
    let (sender, receiver) = oneshot::channel();
    let request_id = next_api_request_id();
    let mut guard = state
        .api_cancel
        .lock()
        .map_err(|_| "API cancel state poisoned")?;
    *guard = Some(ApiCancellation {
        request_id,
        sender: Some(sender),
    });
    Ok((request_id, receiver))
}

/// Clears only the request that installed the cancellation slot. An older
/// streaming task must never clear the slot belonging to a newer request.
fn finish_api_request(state: &ChatState, request_id: u64) -> bool {
    let Ok(mut guard) = state.api_cancel.lock() else {
        return false;
    };
    if guard
        .as_ref()
        .is_some_and(|active| active.request_id == request_id)
    {
        guard.take();
        true
    } else {
        false
    }
}

/// Extract complete Server-Sent Event records without assuming a particular
/// line ending. Providers commonly use CRLF while local test servers often
/// use LF; both are valid SSE framing.
fn drain_sse_records(buffer: &mut String) -> Vec<String> {
    let mut records = Vec::new();
    loop {
        let lf_end = buffer.find("\n\n");
        let crlf_end = buffer.find("\r\n\r\n");
        let Some((end, delimiter_length)) = (match (lf_end, crlf_end) {
            (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
            (Some(lf), _) => Some((lf, 2)),
            (_, Some(crlf)) => Some((crlf, 4)),
            (None, None) => None,
        }) else {
            break;
        };
        records.push(buffer[..end].to_string());
        buffer.drain(..end + delimiter_length);
    }
    records
}

/// Combines all `data:` lines according to the SSE framing rule. It accepts
/// both `data:value` and `data: value`, which are equally valid on the wire.
fn sse_record_payload(record: &str) -> Option<String> {
    let data: Vec<&str> = record
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|value| value.strip_prefix(' ').unwrap_or(value))
        .collect();
    (!data.is_empty()).then(|| data.join("\n"))
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
    let (request_id, mut cancel_rx) = begin_api_request(&state)?;
    let conversation_id = conversation_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(new_conversation_id);
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        finish_api_request(&state, request_id);
        return Err("消息不能为空".into());
    }
    let messages = {
        let conversations = state
            .api_conversations
            .lock()
            .map_err(|_| "API conversation state poisoned")?;
        let mut messages = conversations
            .get(&conversation_id)
            .map(|conversation| conversation.messages.clone())
            .unwrap_or_default();
        messages.push(ApiMessage {
            role: "user".into(),
            content: trimmed_text.to_string(),
        });
        messages
    };
    let key_entry = match keyring::Entry::new("dsh-wallpaper", "deepseek-api") {
        Ok(entry) => entry,
        Err(_) => {
            finish_api_request(&state, request_id);
            return Err("无法访问 Windows 凭据管理器。请检查系统凭据服务后重试。".into());
        }
    };
    let key = key_entry.get_password().map_err(|_| {
        finish_api_request(&state, request_id);
        "未在 Windows 凭据管理器配置 API Key".to_string()
    })?;
    let client = match api_stream_client() {
        Ok(client) => client,
        Err(error) => {
            finish_api_request(&state, request_id);
            return Err(error);
        }
    };
    let response = client
        .post(format!("{}/chat/completions", base_url.trim_end_matches('/')))
        .bearer_auth(key)
        .json(&serde_json::json!({ "model": model, "stream": true, "stream_options": { "include_usage": true }, "messages": messages }))
        .send()
        .await
        .map_err(|_| {
            finish_api_request(&state, request_id);
            generic_api_error("连接")
        })?;
    if !response.status().is_success() {
        finish_api_request(&state, request_id);
        return Err(format!(
            "DeepSeek API 请求被拒绝（HTTP {}）。请检查访问密钥、模型与账户状态后重试。",
            response.status().as_u16()
        ));
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
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
    let mut canceled = false;
    let mut stream_error: Option<String> = None;
    loop {
        tokio::select! {
            _ = &mut cancel_rx => { canceled = true; break; }
            chunk = stream.next() => {
                let Some(chunk) = chunk else { break };
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        for record in drain_sse_records(&mut buffer) {
                            if let Some(line) = sse_record_payload(&record) {
                                if line == "[DONE]" { continue; }
                                if let Ok(data) = serde_json::from_str::<ApiChunk>(&line) {
                                    if let Some(delta) = data.choices.and_then(|v| v.into_iter().next()).and_then(|c| c.delta.content) { full.push_str(&delta); let _ = app.emit("chat-event", ChatEvent::Delta { text: delta }); }
                                    if let Some(usage) = data.usage { let cached = usage.prompt_cache_hit_tokens.unwrap_or(0); let _ = app.emit("chat-event", ChatEvent::Usage { input: usage.prompt_tokens.saturating_sub(cached), output: usage.completion_tokens, cache_read: usage.prompt_cache_hit_tokens, cost: None }); }
                                }
                            }
                        }
                    }
                    Err(_) => { stream_error = Some(generic_api_error("流式连接")); break; }
                }
            }
        }
    }
    let current = finish_api_request(&state, request_id);
    // A newer request superseded this one. Do not publish stale terminal
    // state or mutate the shared conversation transcript.
    if !current {
        return Ok(conversation_id);
    }
    if let Ok(mut conversations) = state.api_conversations.lock() {
        let conversation = conversations.entry(conversation_id.clone()).or_default();
        conversation.messages.push(ApiMessage {
            role: "user".into(),
            content: trimmed_text.to_string(),
        });
        if !full.is_empty() {
            conversation.messages.push(ApiMessage {
                role: "assistant".into(),
                content: full.clone(),
            });
        }
    }
    if !full.is_empty() {
        let _ = app.emit(
            "chat-event",
            ChatEvent::Message {
                role: "assistant".into(),
                content: full,
            },
        );
    }
    if canceled {
        let _ = app.emit(
            "chat-event",
            ChatEvent::Status {
                activity: "idle".into(),
            },
        );
    } else if let Some(error) = stream_error {
        emit_error(&app, "DEEPSEEK_API_STREAM", true, error);
        let _ = app.emit(
            "chat-event",
            ChatEvent::Status {
                activity: "idle".into(),
            },
        );
    } else {
        let _ = app.emit(
            "chat-event",
            ChatEvent::Status {
                activity: "done".into(),
            },
        );
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
        if let Some(active) = guard.as_mut() {
            if let Some(cancel) = active.sender.take() {
                let _ = cancel.send(());
            }
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
    let client = bridge_request_client()?;
    let response = auth(
        client.post("http://127.0.0.1:3080/api/wallpaper/v1/sessions"),
        &token,
    )
    .json(&HarnessSessionRequest {
        resume_session_id: resume_session_id.as_deref(),
    })
    .send()
    .await
    .map_err(|_| generic_harness_error("连接"))?;
    if !response.status().is_success() {
        return Err(generic_bridge_http_error(response.status()));
    }
    let session: HarnessSessionResponse = response
        .json()
        .await
        .map_err(|_| "DSH bridge 返回了无法识别的会话响应。".to_string())?;
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
        let client = match bridge_stream_client() {
            Ok(client) => client,
            Err(error) => {
                emit_error(&app, "HARNESS_SSE_CLIENT", true, error);
                return;
            }
        };
        let response = match auth(client.get(url), &token).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                emit_error(
                    &app,
                    "HARNESS_SSE_HTTP",
                    true,
                    generic_bridge_http_error(response.status()),
                );
                return;
            }
            Err(_) => {
                emit_error(
                    &app,
                    "HARNESS_SSE_CONNECT",
                    true,
                    generic_harness_error("事件流连接"),
                );
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
                            for record in drain_sse_records(&mut buffer) {
                                if let Some(line) = sse_record_payload(&record) {
                                    if let Ok(mut event) = serde_json::from_str::<Value>(&line) {
                                        if event.get("type").and_then(Value::as_str) == Some("model") && event.get("tier").is_none() {
                                            if let Some(map) = event.as_object_mut() { map.insert("tier".into(), Value::String("unknown".into())); }
                                        }
                                        let _ = app.emit("chat-event", event);
                                    }
                                }
                            }
                        }
                        Err(_) => { emit_error(&app, "HARNESS_SSE_READ", true, generic_harness_error("事件流读取")); break; }
                    }
                }
            }
        }
    });
}

pub async fn harness_send(state: tauri::State<'_, ChatState>, text: String) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("消息不能为空".into());
    }
    if text.len() > MAX_HARNESS_MESSAGE_BYTES {
        return Err("消息过长；单条消息不能超过 100,000 个 UTF-8 字节".into());
    }
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
    let client = bridge_request_client()?;
    let response = auth(client.post(url), &token)
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .map_err(|_| "发送到 DSH bridge 失败；请确认 Harness 仍在运行。".to_string())?;
    if !response.status().is_success() {
        return Err(generic_bridge_http_error(response.status()));
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
    let client = bridge_request_client()?;
    let response = auth(client.get(url), &token)
        .send()
        .await
        .map_err(|_| generic_harness_error("历史读取"))?;
    if !response.status().is_success() {
        return Err(generic_bridge_http_error(response.status()));
    }
    response
        .json()
        .await
        .map_err(|_| "DSH bridge 返回了无法识别的历史记录。".to_string())
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
    let client = bridge_request_client()?;
    let response = auth(client.post(url), &token)
        .send()
        .await
        .map_err(|_| generic_harness_error("取消请求"))?;
    if !response.status().is_success() {
        return Err(generic_bridge_http_error(response.status()));
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

#[cfg(test)]
mod tests {
    use super::{
        drain_sse_records, finish_api_request, sse_record_payload, ChatState,
        MAX_HARNESS_MESSAGE_BYTES,
    };

    #[test]
    fn sse_parser_accepts_lf_and_crlf_records_across_chunks() {
        let mut buffer = "data: one\n\n".to_string();
        buffer.push_str("data: two\r\n\r\npartial");
        let records = drain_sse_records(&mut buffer);
        assert_eq!(records, ["data: one", "data: two"]);
        assert_eq!(buffer, "partial");
        assert_eq!(sse_record_payload(&records[0]), Some("one".into()));
        assert_eq!(sse_record_payload(&records[1]), Some("two".into()));
    }

    #[test]
    fn sse_parser_combines_multiline_data_and_ignores_comments() {
        let record = ": heartbeat\ndata:first\ndata: second\nevent: ignored";
        assert_eq!(sse_record_payload(record), Some("first\nsecond".into()));
        assert_eq!(sse_record_payload(": heartbeat"), None);
    }

    #[test]
    fn stale_request_cannot_clear_newer_cancellation_slot() {
        let state = ChatState::default();
        let first = super::begin_api_request(&state).expect("first request").0;
        let second = super::begin_api_request(&state).expect("second request").0;
        assert!(!finish_api_request(&state, first));
        assert!(finish_api_request(&state, second));
    }

    #[test]
    fn bridge_message_limit_matches_the_public_bridge_contract() {
        assert_eq!(MAX_HARNESS_MESSAGE_BYTES, 100_000);
    }

    #[test]
    fn harness_message_limit_uses_utf8_bytes() {
        assert!("界".repeat(33_333).len() <= MAX_HARNESS_MESSAGE_BYTES);
        assert!("界".repeat(33_334).len() > MAX_HARNESS_MESSAGE_BYTES);
    }
}
