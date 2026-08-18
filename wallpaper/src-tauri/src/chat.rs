use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    str::Utf8Error,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::api_persistence::EncryptedJsonStore;

const MAX_HARNESS_MESSAGE_BYTES: usize = 100_000;

/// Incrementally decodes a UTF-8 byte stream without replacing a code point
/// split across network chunks. Both OpenAI-compatible responses and the
/// local DSH bridge use UTF-8 SSE, so one decoder serves both paths.
#[derive(Default)]
struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<String, Utf8Error> {
        self.pending.extend_from_slice(chunk);
        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let decoded = text.to_owned();
                self.pending.clear();
                Ok(decoded)
            }
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                let decoded = std::str::from_utf8(&self.pending[..valid])?.to_owned();
                self.pending.drain(..valid);
                Ok(decoded)
            }
            Err(error) => Err(error),
        }
    }

    fn finish(&mut self) -> Result<String, Utf8Error> {
        let decoded = std::str::from_utf8(&self.pending)?.to_owned();
        self.pending.clear();
        Ok(decoded)
    }
}

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

/// Tauri owns one `ChatState`, but long-lived Harness SSE readers outlive an
/// individual command future. Clones deliberately share these locks so the
/// detached reader can never retain a borrowed `State` reference.
#[derive(Clone)]
pub struct ChatState {
    api_cancel: Arc<Mutex<Option<ApiCancellation>>>,
    harness_cancel: Arc<Mutex<Option<HarnessStreamCancellation>>>,
    harness_session: Arc<Mutex<Option<String>>>,
    api_conversations: Arc<Mutex<HashMap<String, ApiConversation>>>,
    api_store: Arc<Mutex<ApiConversationStore>>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self::with_store(EncryptedJsonStore::new(default_api_conversation_path()))
    }
}

impl ChatState {
    pub fn with_store(store: EncryptedJsonStore) -> Self {
        let (conversations, writable) = match store.load::<ApiConversationArchive>() {
            Ok(Some(archive)) if archive.schema_version == API_CONVERSATION_SCHEMA_VERSION => {
                (archive.conversations, true)
            }
            Ok(Some(_)) => {
                // A future archive must never be overwritten by an older app.
                (HashMap::new(), false)
            }
            Ok(None) => (HashMap::new(), true),
            Err(error) => {
                // Do not log error details here: while they deliberately omit
                // content, callers only need a controlled availability state.
                let _ = error;
                (HashMap::new(), false)
            }
        };
        Self {
            api_cancel: Arc::default(),
            harness_cancel: Arc::default(),
            harness_session: Arc::default(),
            api_conversations: Arc::new(Mutex::new(conversations)),
            api_store: Arc::new(Mutex::new(ApiConversationStore { store, writable })),
        }
    }

    fn persist_api_conversations(&self) -> Result<(), String> {
        let archive = {
            let conversations = self
                .api_conversations
                .lock()
                .map_err(|_| "API conversation state poisoned".to_string())?;
            ApiConversationArchive {
                schema_version: API_CONVERSATION_SCHEMA_VERSION,
                conversations: conversations.clone(),
            }
        };
        let store = self
            .api_store
            .lock()
            .map_err(|_| "API conversation storage state poisoned".to_string())?;
        if !store.writable {
            return Err("加密 API 会话记录不可用；为保护已有记录，本次不会覆盖它。".into());
        }
        store
            .store
            .save(&archive)
            .map_err(|_| "无法保存加密 API 会话记录；已有记录未被覆盖。".to_string())
    }

    #[cfg(test)]
    fn api_conversation(&self, conversation_id: &str) -> Option<ApiConversation> {
        self.api_conversations
            .lock()
            .ok()
            .and_then(|conversations| conversations.get(conversation_id).cloned())
    }

    #[cfg(test)]
    fn append_api_conversation_for_test(&self, conversation_id: &str, message: ApiMessage) {
        if let Ok(mut conversations) = self.api_conversations.lock() {
            conversations
                .entry(conversation_id.into())
                .or_default()
                .messages
                .push(message);
        }
    }
}

const API_CONVERSATION_SCHEMA_VERSION: u32 = 1;

fn default_api_conversation_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dsh-wallpaper")
        .join("api-conversations.v1.dpapi")
}

struct ApiCancellation {
    request_id: u64,
    sender: Option<oneshot::Sender<()>>,
}

struct HarnessStreamCancellation {
    stream_id: u64,
    session_id: String,
    sender: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiMessage {
    id: String,
    role: String,
    content: String,
    created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<ApiUsage>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ApiConversation {
    messages: Vec<ApiMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiUsage {
    input: u64,
    output: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost: Option<f64>,
    estimated: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiConversationArchive {
    schema_version: u32,
    conversations: HashMap<String, ApiConversation>,
}

struct ApiConversationStore {
    store: EncryptedJsonStore,
    writable: bool,
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn new_api_message_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "msg-{:x}-{:x}",
        unix_millis(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn new_api_message(role: &str, content: String, usage: Option<ApiUsage>) -> ApiMessage {
    ApiMessage {
        id: new_api_message_id(),
        role: role.into(),
        content,
        created_at: unix_millis(),
        usage,
    }
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
    // This is defensive in addition to `send_api`'s explicit cancellation:
    // no caller can accidentally leave an older stream able to publish.
    if let Some(mut previous) = guard.take() {
        if let Some(cancel) = previous.sender.take() {
            let _ = cancel.send(());
        }
    }
    *guard = Some(ApiCancellation {
        request_id,
        sender: Some(sender),
    });
    Ok((request_id, receiver))
}

fn is_current_api_request(state: &ChatState, request_id: u64) -> bool {
    state
        .api_cancel
        .lock()
        .ok()
        .and_then(|active| {
            active
                .as_ref()
                .map(|active| active.request_id == request_id)
        })
        .unwrap_or(false)
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

/// The native `chat-event` channel is process-global. Guard every Harness
/// stream before it publishes so a disconnected or replaced session cannot
/// overwrite the active session's composer state.
fn is_current_harness_stream(state: &ChatState, stream_id: u64, session_id: &str) -> bool {
    let active_stream = state
        .harness_cancel
        .lock()
        .ok()
        .and_then(|active| {
            active
                .as_ref()
                .map(|active| active.stream_id == stream_id && active.session_id == session_id)
        })
        .unwrap_or(false);
    let active_session = state
        .harness_session
        .lock()
        .ok()
        .and_then(|session| session.as_ref().map(|session| session == session_id))
        .unwrap_or(false);
    active_stream && active_session
}

fn finish_harness_stream(state: &ChatState, stream_id: u64, session_id: &str) {
    if let Ok(mut active) = state.harness_cancel.lock() {
        if active.as_ref().is_some_and(|current| {
            current.stream_id == stream_id && current.session_id == session_id
        }) {
            active.take();
        }
    }
}

fn clear_harness_session_if(state: &ChatState, session_id: &str) {
    if let Ok(mut active) = state.harness_session.lock() {
        if active.as_deref() == Some(session_id) {
            active.take();
        }
    }
}

/// Extract complete Server-Sent Event records without assuming a particular
/// line ending. WHATWG SSE accepts LF, CRLF, and bare CR; a bare CR at the
/// tail must remain buffered because the next chunk may start with LF.
fn drain_sse_records(buffer: &mut String) -> Vec<String> {
    let mut records = Vec::new();
    let mut record_start = 0;
    let mut line_start = 0;
    // The content of a record ends *before* its final line terminator. Keep
    // that position separately; slicing at `line_start` would retain `\n` or
    // `\r\n` from the preceding data line.
    let mut previous_line_terminator_start: Option<usize> = None;
    let mut index = 0;
    let bytes = buffer.as_bytes();

    while index < bytes.len() {
        let terminator = match bytes[index] {
            b'\n' => Some(1),
            b'\r' if index + 1 < bytes.len() && bytes[index + 1] == b'\n' => Some(2),
            b'\r' if index + 1 < bytes.len() => Some(1),
            // The final CR might form CRLF with the next network chunk.
            b'\r' => break,
            _ => None,
        };
        let Some(terminator_len) = terminator else {
            index += 1;
            continue;
        };
        if index == line_start {
            let record_end = previous_line_terminator_start.unwrap_or(index);
            records.push(buffer[record_start..record_end].to_owned());
            record_start = index + terminator_len;
            previous_line_terminator_start = None;
        } else {
            previous_line_terminator_start = Some(index);
        }
        index += terminator_len;
        line_start = index;
    }
    if record_start > 0 {
        buffer.drain(..record_start);
    }
    records
}

/// At end-of-stream no later byte can turn a final CR into CRLF, so it is a
/// complete line terminator. This is intentionally separate from the normal
/// drain path, which must wait across network chunks.
fn finish_sse_records(buffer: &mut String) -> Vec<String> {
    if buffer.ends_with('\r') {
        buffer.push('\n');
    }
    let mut records = drain_sse_records(buffer);
    // Some otherwise valid streaming implementations close immediately after
    // the final data line instead of writing the optional blank terminator.
    // At EOF no future byte can complete that record, so accept a data-bearing
    // tail rather than silently dropping the assistant's last token.
    if sse_record_payload(buffer).is_some() {
        records.push(std::mem::take(buffer));
    }
    records
}

/// Combines all `data:` lines according to the SSE framing rule. It accepts
/// both `data:value` and `data: value`, which are equally valid on the wire.
fn sse_record_payload(record: &str) -> Option<String> {
    let data: Vec<&str> = record
        .split(['\r', '\n'])
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
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ApiUsage>,
    },
    Usage {
        input: u64,
        output: u64,
        #[serde(rename = "cacheRead", skip_serializing_if = "Option::is_none")]
        cache_read: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        estimated: Option<bool>,
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

#[derive(Clone, Copy, Debug, Default)]
struct ApiPricing {
    input_per_million: Option<f64>,
    output_per_million: Option<f64>,
}

impl ApiPricing {
    fn from_options(input_per_million: Option<f64>, output_per_million: Option<f64>) -> Self {
        Self {
            input_per_million: input_per_million.filter(|price| price.is_finite() && *price >= 0.0),
            output_per_million: output_per_million
                .filter(|price| price.is_finite() && *price >= 0.0),
        }
    }

    fn usage(self, usage: Usage) -> ApiUsage {
        let cache_read = usage.prompt_cache_hit_tokens;
        let input = usage.prompt_tokens.saturating_sub(cache_read.unwrap_or(0));
        // Cache reads are reported separately in the UI, but no separate
        // cache rate is configured. Charge them at the input rate for a
        // conservative estimate instead of silently making them free.
        let estimated_input = input.saturating_add(cache_read.unwrap_or(0));
        let cost = self.input_per_million.zip(self.output_per_million).map(
            |(input_price, output_price)| {
                (estimated_input as f64 * input_price
                    + usage.completion_tokens as f64 * output_price)
                    / 1_000_000.0
            },
        );
        ApiUsage {
            input,
            output: usage.completion_tokens,
            cache_read,
            cost,
            // A user-maintained rate table is necessarily an estimate. This
            // is especially important when a provider reports cache reads but
            // no separate cache-read price has been configured.
            estimated: cost.is_some(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessSessionRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_session_id: Option<&'a str>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessConnection {
    pub session_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Deserialize)]
struct BridgeErrorResponse {
    error: Option<String>,
}

/// `chat-event` is application-global, while the UI has one adapter per
/// backend/conversation. Keep the origin on every native event so an old
/// stream cannot paint into a newly selected backend or transcript.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScopedChatEvent {
    backend: String,
    conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(flatten)]
    event: ChatEvent,
}

fn emit_scoped(
    app: &AppHandle,
    backend: &str,
    conversation_id: &str,
    request_id: Option<String>,
    event: ChatEvent,
) {
    let _ = app.emit(
        "chat-event",
        ScopedChatEvent {
            backend: backend.into(),
            conversation_id: conversation_id.into(),
            request_id,
            event,
        },
    );
}

fn emit_current_api_event(
    app: &AppHandle,
    state: &ChatState,
    conversation_id: &str,
    request_id: u64,
    event_request_id: &str,
    event: ChatEvent,
) -> bool {
    if is_current_api_request(state, request_id) {
        emit_scoped(
            app,
            "deepseek-api",
            conversation_id,
            Some(event_request_id.to_string()),
            event,
        );
        true
    } else {
        false
    }
}

fn scoped_harness_event(mut event: Value, session_id: &str, connection_id: &str) -> Value {
    let Some(map) = event.as_object_mut() else {
        return serde_json::json!({
            "type": "error",
            "code": "HARNESS_SSE_INVALID_EVENT",
            "recoverable": true,
            "message": "DSH bridge 返回了无法识别的事件。",
            "backend": "harness",
            "conversationId": session_id,
            "requestId": connection_id,
        });
    };
    // The local bridge must not be able to choose an origin label. These three
    // fields are assigned by the native subscriber that owns the connection.
    map.insert("backend".into(), Value::String("harness".into()));
    map.insert("conversationId".into(), Value::String(session_id.into()));
    map.insert("requestId".into(), Value::String(connection_id.into()));
    if map.get("type").and_then(Value::as_str) == Some("model") && !map.contains_key("tier") {
        map.insert("tier".into(), Value::String("unknown".into()));
    }
    event
}

/// Parse and relay one complete API SSE record. The current-request check is
/// deliberately inside this helper (rather than only at loop boundaries): a
/// cancelled or superseded stream can have already-buffered chunks.
fn process_api_sse_record(
    app: &AppHandle,
    state: &ChatState,
    conversation_id: &str,
    request_id: u64,
    event_request_id: &str,
    record: &str,
    full: &mut String,
    final_usage: &mut Option<ApiUsage>,
    pricing: ApiPricing,
) {
    if !is_current_api_request(state, request_id) {
        return;
    }
    let Some(line) = sse_record_payload(record) else {
        return;
    };
    if line == "[DONE]" {
        return;
    }
    let Ok(data) = serde_json::from_str::<ApiChunk>(&line) else {
        return;
    };
    if let Some(delta) = data
        .choices
        .and_then(|choices| choices.into_iter().next())
        .and_then(|choice| choice.delta.content)
    {
        full.push_str(&delta);
        let _ = emit_current_api_event(
            app,
            state,
            conversation_id,
            request_id,
            event_request_id,
            ChatEvent::Delta { text: delta },
        );
    }
    if let Some(usage) = data.usage {
        let usage = pricing.usage(usage);
        *final_usage = Some(usage.clone());
        let _ = emit_current_api_event(
            app,
            state,
            conversation_id,
            request_id,
            event_request_id,
            ChatEvent::Usage {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cost: usage.cost,
                estimated: usage.cost.map(|_| usage.estimated),
            },
        );
    }
}

pub async fn send_api(
    app: AppHandle,
    state: tauri::State<'_, ChatState>,
    text: String,
    base_url: String,
    model: String,
    conversation_id: Option<String>,
    event_request_id: Option<String>,
    price_input_per_million: Option<f64>,
    price_output_per_million: Option<f64>,
) -> Result<String, String> {
    cancel_api(&state);
    let (request_id, mut cancel_rx) = begin_api_request(&state)?;
    let conversation_id = conversation_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(new_conversation_id);
    let event_request_id = event_request_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| request_id.to_string());
    let trimmed_text = text.trim();
    let pricing = ApiPricing::from_options(price_input_per_million, price_output_per_million);
    if trimmed_text.is_empty() {
        finish_api_request(&state, request_id);
        return Err("消息不能为空".into());
    }
    let messages =
        {
            let conversations = state
                .api_conversations
                .lock()
                .map_err(|_| "API conversation state poisoned")?;
            let messages = conversations
                .get(&conversation_id)
                .map(|conversation| conversation.messages.clone())
                .unwrap_or_default();
            messages
            .into_iter()
            .map(|message| serde_json::json!({ "role": message.role, "content": message.content }))
            .chain(std::iter::once(serde_json::json!({ "role": "user", "content": trimmed_text })))
            .collect::<Vec<_>>()
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
    if !emit_current_api_event(
        &app,
        &state,
        &conversation_id,
        request_id,
        &event_request_id,
        ChatEvent::Model {
            provider: Some("deepseek".into()),
            model,
            tier: "unknown".into(),
            effort: None,
        },
    ) {
        return Ok(conversation_id);
    }
    if !emit_current_api_event(
        &app,
        &state,
        &conversation_id,
        request_id,
        &event_request_id,
        ChatEvent::Status {
            activity: "streaming".into(),
        },
    ) {
        return Ok(conversation_id);
    }
    let mut stream = response.bytes_stream();
    let mut decoder = Utf8StreamDecoder::default();
    let mut buffer = String::new();
    let mut full = String::new();
    let mut final_usage: Option<ApiUsage> = None;
    let mut canceled = false;
    let mut stream_error: Option<String> = None;
    loop {
        tokio::select! {
            _ = &mut cancel_rx => { canceled = true; break; }
            chunk = stream.next() => {
                let Some(chunk) = chunk else {
                    match decoder.finish() {
                        Ok(tail) => buffer.push_str(&tail),
                        Err(_) => {
                        stream_error = Some(generic_api_error("流式编码"));
                        }
                    }
                    break;
                };
                match chunk {
                    Ok(bytes) => {
                        let decoded = match decoder.push(&bytes) {
                            Ok(decoded) => decoded,
                            Err(_) => { stream_error = Some(generic_api_error("流式编码")); break; }
                        };
                        buffer.push_str(&decoded);
                        for record in drain_sse_records(&mut buffer) {
                            process_api_sse_record(&app, &state, &conversation_id, request_id, &event_request_id, &record, &mut full, &mut final_usage, pricing);
                        }
                    }
                    Err(_) => { stream_error = Some(generic_api_error("流式连接")); break; }
                }
            }
        }
    }
    if !canceled && stream_error.is_none() {
        for record in finish_sse_records(&mut buffer) {
            process_api_sse_record(
                &app,
                &state,
                &conversation_id,
                request_id,
                &event_request_id,
                &record,
                &mut full,
                &mut final_usage,
                pricing,
            );
        }
    }
    // A newer request superseded this one. Do not publish stale terminal
    // state or mutate the shared conversation transcript.
    if !is_current_api_request(&state, request_id) {
        return Ok(conversation_id);
    }
    if let Ok(mut conversations) = state.api_conversations.lock() {
        let conversation = conversations.entry(conversation_id.clone()).or_default();
        conversation
            .messages
            .push(new_api_message("user", trimmed_text.to_string(), None));
        if !full.is_empty() {
            conversation.messages.push(new_api_message(
                "assistant",
                full.clone(),
                final_usage.clone(),
            ));
        }
    }
    // The in-memory transcript remains usable for this run if disk storage is
    // temporarily unavailable. The warning is returned through the normal
    // controlled error event below; existing ciphertext is never overwritten.
    let persistence_error = state.persist_api_conversations().err();
    if !full.is_empty() {
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Message {
                role: "assistant".into(),
                content: full,
                usage: final_usage,
            },
        );
    }
    if canceled {
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Status {
                activity: "idle".into(),
            },
        );
    } else if let Some(error) = stream_error {
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Error {
                code: "DEEPSEEK_API_STREAM".into(),
                recoverable: true,
                message: error,
            },
        );
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Status {
                activity: "idle".into(),
            },
        );
    } else if persistence_error.is_none() {
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Status {
                activity: "done".into(),
            },
        );
    }
    if let Some(message) = persistence_error {
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Error {
                code: "API_TRANSCRIPT_PERSISTENCE".into(),
                recoverable: true,
                message,
            },
        );
        let _ = emit_current_api_event(
            &app,
            &state,
            &conversation_id,
            request_id,
            &event_request_id,
            ChatEvent::Status {
                activity: "idle".into(),
            },
        );
    }
    finish_api_request(&state, request_id);
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
        // Remove the ownership marker before waking the task. A buffered
        // response can otherwise observe itself as current after cancellation
        // and append a stale terminal message to the transcript.
        if let Some(mut active) = guard.take() {
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
    connection_id: String,
) -> Result<String, String> {
    let connection_id = connection_id.trim().to_string();
    if connection_id.is_empty() || connection_id.len() > 200 {
        return Err("Harness 连接标识无效".into());
    }
    cancel_harness_stream(&state);
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let create_session = |resume_session_id: Option<&str>| {
        auth(
            client.post("http://127.0.0.1:3080/api/wallpaper/v1/sessions"),
            &token,
        )
        .json(&HarnessSessionRequest { resume_session_id })
        .send()
    };
    let mut response = create_session(resume_session_id.as_deref())
        .await
        .map_err(|_| generic_harness_error("连接"))?;
    // Persistence is optional in DSH. Only the explicit bridge 409 contract
    // gets a one-time new-session retry; never turn arbitrary failed resumes
    // into a fresh transcript silently.
    if resume_session_id.is_some() && response.status() == reqwest::StatusCode::CONFLICT {
        let resume_unavailable = response
            .json::<BridgeErrorResponse>()
            .await
            .ok()
            .and_then(|body| body.error)
            .as_deref()
            == Some("resume-unavailable");
        if resume_unavailable {
            response = create_session(None)
                .await
                .map_err(|_| generic_harness_error("连接"))?;
        } else {
            return Err(generic_bridge_http_error(reqwest::StatusCode::CONFLICT));
        }
    }
    if !response.status().is_success() {
        return Err(generic_bridge_http_error(response.status()));
    }
    let session: HarnessConnection = response
        .json()
        .await
        .map_err(|_| "DSH bridge 返回了无法识别的会话响应。".to_string())?;
    *state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")? = Some(session.session_id.clone());
    if let Err(error) = connect_harness_events(
        app,
        state.inner().clone(),
        session.session_id.clone(),
        token,
        connection_id,
    )
    .await
    {
        clear_harness_session_if(state.inner(), &session.session_id);
        return Err(error);
    }
    Ok(session.session_id)
}

async fn connect_harness_events(
    app: AppHandle,
    state: ChatState,
    session_id: String,
    token: String,
    connection_id: String,
) -> Result<(), String> {
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    static STREAM_COUNTER: AtomicU64 = AtomicU64::new(1);
    let stream_id = STREAM_COUNTER.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut guard) = state.harness_cancel.lock() {
        *guard = Some(HarnessStreamCancellation {
            stream_id,
            session_id: session_id.clone(),
            sender: Some(cancel_tx),
        });
    }
    let url = format!(
        "http://127.0.0.1:3080/api/wallpaper/v1/sessions/{}/events",
        urlencoding::encode(&session_id)
    );
    let client = match bridge_stream_client() {
        Ok(client) => client,
        Err(error) => {
            finish_harness_stream(&state, stream_id, &session_id);
            return Err(error);
        }
    };
    // `send()` resolves after response headers arrive. The bridge registers the
    // client before it flushes those headers, so this await is the readiness
    // barrier that prevents the first POST/followup from being missed.
    let response = match auth(client.get(url), &token).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            finish_harness_stream(&state, stream_id, &session_id);
            return Err(generic_bridge_http_error(response.status()));
        }
        Err(_) => {
            finish_harness_stream(&state, stream_id, &session_id);
            return Err(generic_harness_error("事件流连接"));
        }
    };
    // The bridge sets this only after it has inserted the response into the
    // live subscriber set. Require the handshake rather than assuming an HTTP
    // 200 means the first `followup()` cannot win the race.
    if response
        .headers()
        .get("x-dsh-wallpaper-sse-ready")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        finish_harness_stream(&state, stream_id, &session_id);
        return Err("DSH bridge 事件流未确认就绪；请升级壁纸 bridge 后重试。".into());
    }
    tauri::async_runtime::spawn(async move {
        let emit_stream_error = |code: &str, message: String| {
            if is_current_harness_stream(&state, stream_id, &session_id) {
                emit_scoped(
                    &app,
                    "harness",
                    &session_id,
                    Some(connection_id.clone()),
                    ChatEvent::Error {
                        code: code.into(),
                        recoverable: true,
                        message,
                    },
                );
            }
        };
        let mut stream = response.bytes_stream();
        let mut decoder = Utf8StreamDecoder::default();
        let mut buffer = String::new();
        loop {
            tokio::select! {
                _ = &mut cancel_rx => break,
                chunk = stream.next() => {
                    let Some(chunk) = chunk else {
                        match decoder.finish() {
                            Ok(tail) => buffer.push_str(&tail),
                            Err(_) => emit_stream_error("HARNESS_SSE_ENCODING", generic_harness_error("事件流编码")),
                        }
                        for record in finish_sse_records(&mut buffer) {
                            if let Some(line) = sse_record_payload(&record) {
                                    if let Ok(event) = serde_json::from_str::<Value>(&line) {
                                    if is_current_harness_stream(&state, stream_id, &session_id) {
                                        let _ = app.emit("chat-event", scoped_harness_event(event, &session_id, &connection_id));
                                    }
                                }
                            }
                        }
                        emit_stream_error("HARNESS_DISCONNECTED", "DSH bridge 事件流已断开".into());
                        break;
                    };
                    match chunk {
                        Ok(bytes) => {
                            let decoded = match decoder.push(&bytes) {
                                Ok(decoded) => decoded,
                                Err(_) => { emit_stream_error("HARNESS_SSE_ENCODING", generic_harness_error("事件流编码")); break; }
                            };
                            buffer.push_str(&decoded);
                            for record in drain_sse_records(&mut buffer) {
                                if let Some(line) = sse_record_payload(&record) {
                                    if let Ok(event) = serde_json::from_str::<Value>(&line) {
                                        if is_current_harness_stream(&state, stream_id, &session_id) {
                                            let _ = app.emit("chat-event", scoped_harness_event(event, &session_id, &connection_id));
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => { emit_stream_error("HARNESS_SSE_READ", generic_harness_error("事件流读取")); break; }
                    }
                }
            }
        }
        finish_harness_stream(&state, stream_id, &session_id);
    });
    Ok(())
}

pub async fn harness_send(state: tauri::State<'_, ChatState>, text: String) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("消息不能为空".into());
    }
    if text.as_bytes().len() > MAX_HARNESS_MESSAGE_BYTES {
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
        if let Some(mut active) = guard.take() {
            if let Some(cancel) = active.sender.take() {
                let _ = cancel.send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        drain_sse_records, finish_api_request, finish_harness_stream, finish_sse_records,
        is_current_harness_stream, sse_record_payload, ApiPricing, ApiUsage, ChatState,
        HarnessStreamCancellation, Usage, Utf8StreamDecoder, MAX_HARNESS_MESSAGE_BYTES,
    };
    use crate::api_persistence::EncryptedJsonStore;
    use tokio::sync::oneshot;

    #[test]
    fn sse_parser_accepts_lf_crlf_and_bare_cr_records_across_chunks() {
        let mut buffer = "data: one\n\n".to_string();
        buffer.push_str("data: two\r\n\r\ndata: three\r\rpartial");
        let records = drain_sse_records(&mut buffer);
        assert_eq!(records, ["data: one", "data: two", "data: three"]);
        assert_eq!(buffer, "partial");
        assert_eq!(sse_record_payload(&records[0]), Some("one".into()));
        assert_eq!(sse_record_payload(&records[1]), Some("two".into()));
        assert_eq!(sse_record_payload(&records[2]), Some("three".into()));
    }

    #[test]
    fn sse_parser_combines_multiline_data_and_ignores_comments() {
        let record = ": heartbeat\rdata:first\r\ndata: second\nevent: ignored";
        assert_eq!(sse_record_payload(record), Some("first\nsecond".into()));
        assert_eq!(sse_record_payload(": heartbeat"), None);
    }

    #[test]
    fn sse_parser_waits_for_a_split_crlf_then_flushes_terminal_bare_cr() {
        let mut buffer = "data: split\r".to_string();
        assert!(drain_sse_records(&mut buffer).is_empty());
        buffer.push_str("\n\r\n");
        assert_eq!(drain_sse_records(&mut buffer), ["data: split"]);
        assert!(buffer.is_empty());

        buffer.push_str("data: terminal\r\r");
        assert_eq!(finish_sse_records(&mut buffer), ["data: terminal"]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn utf8_stream_decoder_preserves_a_codepoint_split_across_chunks() {
        let mut decoder = Utf8StreamDecoder::default();
        let whale = "鲸".as_bytes();
        assert_eq!(decoder.push(&whale[..1]).expect("partial UTF-8"), "");
        assert_eq!(decoder.push(&whale[1..]).expect("complete UTF-8"), "鲸");
        assert_eq!(decoder.finish().expect("empty tail"), "");
    }

    #[test]
    fn stale_harness_stream_cannot_publish_or_clear_the_newer_stream() {
        let state = ChatState::default();
        let (sender, _receiver) = oneshot::channel();
        *state.harness_cancel.lock().expect("state lock") = Some(HarnessStreamCancellation {
            stream_id: 22,
            session_id: "newer-session".into(),
            sender: Some(sender),
        });
        *state.harness_session.lock().expect("session lock") = Some("newer-session".into());
        assert!(!is_current_harness_stream(&state, 21, "older-session"));
        finish_harness_stream(&state, 21, "older-session");
        assert!(is_current_harness_stream(&state, 22, "newer-session"));
        finish_harness_stream(&state, 22, "newer-session");
        assert!(!is_current_harness_stream(&state, 22, "newer-session"));
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

    #[test]
    fn api_pricing_requires_both_rates_and_keeps_zero_as_configured() {
        let missing_rate = ApiPricing::from_options(Some(2.0), None).usage(Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            prompt_cache_hit_tokens: Some(40),
        });
        assert_eq!(missing_rate.cost, None);

        let priced = ApiPricing::from_options(Some(2.0), Some(3.0)).usage(Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            prompt_cache_hit_tokens: Some(40),
        });
        // Cache rate is not configured separately, so cache reads use the
        // input price and are visibly marked as an estimate.
        assert_eq!(priced.input, 60);
        assert_eq!(priced.cache_read, Some(40));
        assert_eq!(priced.cost, Some(0.00026));
        assert!(priced.estimated);

        let zero = ApiPricing::from_options(Some(0.0), Some(0.0)).usage(Usage {
            prompt_tokens: 1,
            completion_tokens: 1,
            prompt_cache_hit_tokens: None,
        });
        assert_eq!(zero.cost, Some(0.0));
    }

    #[cfg(windows)]
    #[test]
    fn api_transcript_survives_state_recreation_with_ids_timestamps_and_usage() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));
        state.append_api_conversation_for_test(
            "conversation-a",
            super::new_api_message(
                "assistant",
                "persist me".into(),
                Some(ApiUsage {
                    input: 3,
                    output: 4,
                    cache_read: None,
                    cost: Some(0.0001),
                    estimated: true,
                }),
            ),
        );
        state
            .persist_api_conversations()
            .expect("persist transcript");

        let restored = ChatState::with_store(EncryptedJsonStore::new(&path));
        let messages = restored
            .api_conversation("conversation-a")
            .expect("restored conversation")
            .messages;
        assert_eq!(messages.len(), 1);
        assert!(!messages[0].id.is_empty());
        assert!(messages[0].created_at > 0);
        assert_eq!(
            messages[0].usage.as_ref().and_then(|usage| usage.cost),
            Some(0.0001)
        );
    }
}
