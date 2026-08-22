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
use tauri::{AppHandle, Emitter, EventTarget};
use tokio::sync::oneshot;

use crate::api_persistence::EncryptedJsonStore;

const MAX_HARNESS_MESSAGE_BYTES: usize = 100_000;
/// A normal bearer is short.  Bound the file before allocating so a malformed
/// local path cannot make the resident native process read arbitrary data.
const MAX_BRIDGE_TOKEN_FILE_BYTES: usize = 4 * 1024;
/// DSH Bridge runs on the loopback interface, but it is still a separate
/// process and its SSE output is untrusted at this boundary.  A normal SSE
/// record is a small delta, but the Bridge's final `message` record can carry
/// a whole response, so allow the same largest useful response as the API.
const MAX_HARNESS_SSE_EVENT_BYTES: usize = 4 * 1024 * 1024;
/// A complete event is drained immediately.  Retaining more than this means a
/// peer has not finished one event, which is never necessary for the normal
/// Bridge protocol and must not grow without bound in a resident wallpaper.
const MAX_HARNESS_SSE_BUFFER_BYTES: usize = 256 * 1024;
/// Bound a single transport read before decoding it into an owned UTF-8
/// string. This also limits a peer that packs many records into one read.
const MAX_HARNESS_SSE_CHUNK_BYTES: usize = MAX_HARNESS_SSE_EVENT_BYTES;
/// Non-streaming Bridge responses must be bounded before `bytes()` allocates
/// them. The live-session endpoint is tiny; history has an explicit larger
/// ceiling because it can contain several completed turns.
const MAX_HARNESS_SESSION_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_HARNESS_HISTORY_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_HARNESS_HISTORY_MESSAGES: usize = 256;
const MAX_HARNESS_HISTORY_ID_BYTES: usize = 200;
/// Bound values that originate in a renderer or an arbitrary compatible API
/// endpoint.  The encrypted archive has a larger total ceiling, but allowing
/// one request or a never-ending SSE record to consume that entire budget is
/// neither useful nor safe for a long-running wallpaper process.
const MAX_API_MESSAGE_BYTES: usize = 100_000;
const MAX_API_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_API_SSE_BUFFER_BYTES: usize = 256 * 1024;
const MAX_API_RATE_PER_MILLION: f64 = 1_000_000.0;
/// Preserve room for the latest turn while preventing a restored transcript
/// from becoming an unbounded request body. This is a byte bound because the
/// compatible API accepts UTF-8 strings rather than an application token
/// counter. Older messages are omitted first; the active user turn is never
/// silently truncated.
const MAX_API_REQUEST_CONTEXT_BYTES: usize = 2 * 1024 * 1024;

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
    /// A process-local transaction guard covers current-request ownership,
    /// transcript mutation and persistence as one unit.  The encrypted store
    /// adds a named cross-process lock and reload/merge for a second process.
    api_transcript_transaction: Arc<Mutex<()>>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self::with_store(default_api_conversation_store())
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
            api_transcript_transaction: Arc::default(),
        }
    }

    fn persist_api_conversations(&self) -> Result<(), String> {
        let _transaction = self
            .api_transcript_transaction
            .lock()
            .map_err(|_| "API transcript transaction state poisoned".to_string())?;
        self.persist_api_conversations_locked()
    }

    fn persist_api_conversations_locked(&self) -> Result<(), String> {
        let store = self
            .api_store
            .lock()
            .map_err(|_| "API conversation storage state poisoned".to_string())?;
        if !store.writable {
            return Err("加密 API 会话记录不可用；为保护已有记录，本次不会覆盖它。".into());
        }
        let local_conversations = self
            .api_conversations
            .lock()
            .map_err(|_| "API conversation state poisoned".to_string())?
            .clone();
        let merged = store
            .store
            .update::<ApiConversationArchive, _, _>(|existing| {
                let mut conversations = match existing {
                    Some(archive) if archive.schema_version == API_CONVERSATION_SCHEMA_VERSION => {
                        archive.conversations
                    }
                    // Never overwrite an archive written by a newer schema.
                    Some(_) => {
                        return Err(crate::api_persistence::PersistenceError::InvalidArchive)
                    }
                    None => HashMap::new(),
                };
                merge_api_conversations(&mut conversations, local_conversations);
                Ok((
                    ApiConversationArchive {
                        schema_version: API_CONVERSATION_SCHEMA_VERSION,
                        conversations: conversations.clone(),
                    },
                    conversations,
                ))
            })
            .map_err(|_| "无法保存加密 API 会话记录；已有记录未被覆盖。".to_string())?;
        // Adopt the durable merged view while still inside the transaction.
        // This prevents a second process's transcript from being forgotten by
        // the next local append.
        *self
            .api_conversations
            .lock()
            .map_err(|_| "API conversation state poisoned".to_string())? = merged;
        Ok(())
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

/// API transcripts are a per-user durable record, never a best-effort temp
/// artifact.  If Windows cannot provide LocalAppData, deliberately create an
/// unavailable store outside any shared temp directory; the chat state then
/// remains usable in memory but refuses to write a transcript until a proper
/// user data directory exists.
fn default_api_conversation_store() -> EncryptedJsonStore {
    match dirs::data_local_dir() {
        Some(root) => EncryptedJsonStore::new(
            root.join("dsh-wallpaper")
                .join("api-conversations.v1.dpapi"),
        ),
        None => EncryptedJsonStore::unavailable(),
    }
}

struct ApiCancellation {
    request_id: u64,
    sender: Option<oneshot::Sender<()>>,
    /// An explicit user stop retains ownership until the streaming task emits
    /// its terminal idle state.  A newer request instead replaces this record,
    /// which makes the old task stale and prevents it from publishing.
    cancelled: bool,
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

/// Merge archives by durable message identity.  A user may cancel a response
/// after partial text has arrived, and two process lifetimes can legitimately
/// append to different conversations.  Preserve both transcripts, retain a
/// deterministic chronological order, and never duplicate a message already
/// committed by an earlier transaction.
fn merge_api_conversations(
    destination: &mut HashMap<String, ApiConversation>,
    source: HashMap<String, ApiConversation>,
) {
    for (conversation_id, incoming) in source {
        let target = destination.entry(conversation_id).or_default();
        for message in incoming.messages {
            if !target
                .messages
                .iter()
                .any(|existing| existing.id == message.id)
            {
                target.messages.push(message);
            }
        }
        target.messages.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
    }
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
        // A custom API endpoint must not turn one configured request into an
        // invisible redirected request with the user's bearer credential.
        // The UI can show the explicit HTTP failure for a moved endpoint.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法初始化 DeepSeek API 客户端".to_string())
}

/// The API key belongs to a configured service endpoint.  Accept public HTTPS
/// endpoints and a deliberately local HTTP endpoint for compatible development
/// servers; never send a Credential-Manager bearer token to an arbitrary
/// plaintext host, URL with embedded credentials, fragment, or opaque scheme.
fn normalized_api_base_url(value: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(value.trim()).map_err(|_| "DeepSeek API 地址无效。")?;
    if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
        return Err("DeepSeek API 地址不能包含账号、密码或片段。".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "DeepSeek API 地址必须包含主机名。".to_string())?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => {
            return Err("DeepSeek API 地址必须使用 HTTPS；仅本机 loopback 允许 HTTP。".into())
        }
        _ => return Err("DeepSeek API 地址仅支持 HTTPS，或本机 loopback HTTP。".into()),
    }
    // The completion route is joined as a path rather than string-concatenated
    // so a query string or trailing slash cannot alter its authority.
    if url.query().is_some() {
        return Err("DeepSeek API 地址不能包含查询参数。".into());
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn api_completion_url(base_url: &str) -> Result<reqwest::Url, String> {
    let normalized = normalized_api_base_url(base_url)?;
    let mut url = reqwest::Url::parse(&normalized).map_err(|_| "DeepSeek API 地址无效。")?;
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{path}/chat/completions"));
    Ok(url)
}

fn api_request_messages(history: Vec<ApiMessage>, user_text: &str) -> Result<Vec<Value>, String> {
    let user_bytes = user_text.as_bytes().len();
    if user_bytes > MAX_API_REQUEST_CONTEXT_BYTES {
        return Err(format!(
            "本轮 API 消息不能超过 {MAX_API_REQUEST_CONTEXT_BYTES} 字节。"
        ));
    }
    let mut used = user_bytes;
    let mut selected = Vec::new();
    // Preserve chronological order in the submitted JSON, while selecting
    // from newest to oldest so an old archive cannot crowd out recent context.
    for message in history.into_iter().rev() {
        let bytes = message.content.as_bytes().len();
        if bytes > MAX_API_REQUEST_CONTEXT_BYTES.saturating_sub(used) {
            continue;
        }
        used += bytes;
        selected.push(message);
    }
    selected.reverse();
    Ok(selected
        .into_iter()
        .map(|message| serde_json::json!({ "role": message.role, "content": message.content }))
        .chain(std::iter::once(
            serde_json::json!({ "role": "user", "content": user_text }),
        ))
        .collect())
}

fn bridge_request_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // The Bridge bearer token is only valid for our loopback peer.  Do
        // not inherit HTTP(S)_PROXY or system-proxy settings here: a malformed
        // NO_PROXY environment variable must never route that token elsewhere.
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "无法初始化 DSH bridge 客户端".to_string())
}

fn bridge_stream_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // See `bridge_request_client`: the SSE request carries the same
        // bearer token and must remain a direct loopback connection.
        .no_proxy()
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
    let _transaction = state
        .api_transcript_transaction
        .lock()
        .map_err(|_| "API transcript transaction state poisoned")?;
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
        cancelled: false,
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
                .map(|active| active.request_id == request_id && !active.cancelled)
        })
        .unwrap_or(false)
}

/// A user-cancelled request still owns its terminal transition; a superseded
/// request does not.  Keeping these concepts separate avoids the old bug where
/// `cancel_api` removed the slot and the stream could never emit `idle`.
fn owns_api_request(state: &ChatState, request_id: u64) -> bool {
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

fn is_api_request_cancelled(state: &ChatState, request_id: u64) -> bool {
    state
        .api_cancel
        .lock()
        .ok()
        .and_then(|active| {
            active
                .as_ref()
                .filter(|active| active.request_id == request_id)
                .map(|active| active.cancelled)
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

/// Append one decoded DSH Bridge SSE read and return only complete bounded
/// records. A final delimiter may arrive after an otherwise full event, so
/// drain before testing the retained tail; retained incomplete data has its
/// own tighter cap while a completed final response may use the event cap.
fn drain_bounded_harness_sse_records(buffer: &mut String, text: &str) -> Result<Vec<String>, ()> {
    if text.len() > MAX_HARNESS_SSE_CHUNK_BYTES
        || buffer.len() > MAX_HARNESS_SSE_BUFFER_BYTES
        || buffer.len().saturating_add(text.len()) > MAX_HARNESS_SSE_EVENT_BYTES
    {
        return Err(());
    }
    buffer.push_str(text);
    let records = drain_sse_records(buffer);
    if records
        .iter()
        .any(|record| record.len() > MAX_HARNESS_SSE_EVENT_BYTES)
        || buffer.len() > MAX_HARNESS_SSE_BUFFER_BYTES
    {
        return Err(());
    }
    Ok(records)
}

/// EOF makes a trailing bare CR and a data-bearing tail complete.  Apply the
/// same event cap before forwarding that final record to the renderer.
fn finish_bounded_harness_sse_records(buffer: &mut String) -> Result<Vec<String>, ()> {
    if buffer.len() > MAX_HARNESS_SSE_BUFFER_BYTES {
        return Err(());
    }
    let records = finish_sse_records(buffer);
    if records
        .iter()
        .any(|record| record.len() > MAX_HARNESS_SSE_EVENT_BYTES)
    {
        return Err(());
    }
    Ok(records)
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
    ApprovalRequired {
        #[serde(rename = "sessionId")]
        session_id: String,
        summary: String,
    },
    Error {
        code: String,
        recoverable: bool,
        message: String,
    },
}

/// The Bridge runs in a separate local process, so its SSE payloads must be
/// parsed into this closed protocol before they reach a WebView.  Do not relay
/// arbitrary JSON values: otherwise a compatible-but-buggy bridge could add
/// renderer-visible fields or forge an event shape the UI was not designed to
/// handle.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum BridgeEvent {
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
        #[serde(rename = "cacheRead")]
        cache_read: Option<u64>,
        cost: Option<f64>,
    },
    Model {
        provider: Option<String>,
        model: String,
        effort: Option<String>,
    },
    ApprovalRequired {
        #[serde(rename = "sessionId")]
        session_id: String,
        summary: String,
    },
    Error {
        code: String,
        recoverable: bool,
        message: String,
    },
    Disconnected {
        recoverable: bool,
    },
}

const MAX_HARNESS_EVENT_TEXT_BYTES: usize = MAX_HARNESS_MESSAGE_BYTES;
const MAX_HARNESS_EVENT_IDENTIFIER_BYTES: usize = 200;
const MAX_HARNESS_EVENT_PROVIDER_BYTES: usize = 200;
const MAX_HARNESS_EVENT_SUMMARY_BYTES: usize = 500;
const MAX_HARNESS_EVENT_TOKEN_COUNT: u64 = 1_000_000_000_000;
const MAX_HARNESS_EVENT_COST: f64 = 1_000_000.0;

fn bounded_bridge_text(value: String, maximum: usize) -> Option<String> {
    (value.as_bytes().len() <= maximum).then_some(value)
}

fn valid_bridge_activity(value: &str) -> bool {
    matches!(
        value,
        "idle" | "sending" | "thinking" | "streaming" | "tool" | "done"
    )
}

fn valid_bridge_role(value: &str) -> bool {
    matches!(value, "user" | "assistant")
}

/// Converts an untrusted Bridge event into the exact renderer protocol.  The
/// native owner stamps backend/session/request metadata later, so all origin
/// fields supplied by the bridge are rejected by `deny_unknown_fields`.
fn parse_bridge_event(line: &str, expected_session_id: &str) -> Option<ChatEvent> {
    let event = serde_json::from_str::<BridgeEvent>(line).ok()?;
    match event {
        BridgeEvent::Status { activity } if valid_bridge_activity(&activity) => {
            Some(ChatEvent::Status { activity })
        }
        BridgeEvent::Delta { text } => bounded_bridge_text(text, MAX_HARNESS_EVENT_TEXT_BYTES)
            .map(|text| ChatEvent::Delta { text }),
        BridgeEvent::Message { role, content } if valid_bridge_role(&role) => {
            bounded_bridge_text(content, MAX_HARNESS_EVENT_TEXT_BYTES).map(|content| {
                ChatEvent::Message {
                    role,
                    content,
                    usage: None,
                }
            })
        }
        BridgeEvent::Usage {
            input,
            output,
            cache_read,
            cost,
        } if input <= MAX_HARNESS_EVENT_TOKEN_COUNT && output <= MAX_HARNESS_EVENT_TOKEN_COUNT => {
            let cache_read = cache_read.map(|value| value.min(input));
            let cost = cost.filter(|value| {
                value.is_finite() && (0.0..=MAX_HARNESS_EVENT_COST).contains(value)
            });
            Some(ChatEvent::Usage {
                input,
                output,
                cache_read,
                cost,
                estimated: cost.map(|_| false),
            })
        }
        BridgeEvent::Model {
            provider,
            model,
            effort,
        } => {
            let provider = match provider {
                Some(value) => Some(bounded_bridge_text(
                    value,
                    MAX_HARNESS_EVENT_PROVIDER_BYTES,
                )?),
                None => None,
            };
            let model = bounded_bridge_text(model, MAX_HARNESS_EVENT_IDENTIFIER_BYTES)?;
            let effort = match effort {
                Some(value) => Some(bounded_bridge_text(
                    value,
                    MAX_HARNESS_EVENT_IDENTIFIER_BYTES,
                )?),
                None => None,
            };
            Some(ChatEvent::Model {
                provider,
                model,
                tier: "unknown".into(),
                effort,
            })
        }
        BridgeEvent::ApprovalRequired {
            session_id,
            summary,
        } if session_id == expected_session_id => {
            let summary = bounded_bridge_text(summary, MAX_HARNESS_EVENT_SUMMARY_BYTES)?;
            Some(ChatEvent::ApprovalRequired {
                session_id: expected_session_id.into(),
                summary,
            })
        }
        BridgeEvent::Error {
            code,
            recoverable,
            message,
        } => {
            let code = bounded_bridge_text(code, MAX_HARNESS_EVENT_IDENTIFIER_BYTES)?;
            let message = bounded_bridge_text(message, MAX_HARNESS_EVENT_TEXT_BYTES)?;
            Some(ChatEvent::Error {
                code,
                recoverable,
                message,
            })
        }
        BridgeEvent::Disconnected { recoverable } if recoverable => Some(ChatEvent::Error {
            code: "HARNESS_DISCONNECTED".into(),
            recoverable: true,
            message: "DSH bridge 事件流已断开".into(),
        }),
        _ => None,
    }
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
            input_per_million: input_per_million.filter(|price| {
                price.is_finite() && (0.0..=MAX_API_RATE_PER_MILLION).contains(price)
            }),
            output_per_million: output_per_million.filter(|price| {
                price.is_finite() && (0.0..=MAX_API_RATE_PER_MILLION).contains(price)
            }),
        }
    }

    fn usage(self, usage: Usage) -> ApiUsage {
        // Provider usage is untrusted input.  A cache-read count cannot exceed
        // the reported prompt count; clamp it before display and pricing so a
        // malformed response cannot manufacture nonsensical token accounting.
        let cache_read = usage
            .prompt_cache_hit_tokens
            .map(|value| value.min(usage.prompt_tokens));
        let input = usage.prompt_tokens.saturating_sub(cache_read.unwrap_or(0));
        // Cache reads are reported separately in the UI, but no separate
        // cache rate is configured. Charge them at the input rate for a
        // conservative estimate instead of silently making them free.
        let estimated_input = input.saturating_add(cache_read.unwrap_or(0));
        let cost = self
            .input_per_million
            .zip(self.output_per_million)
            .and_then(|(input_price, output_price)| {
                let cost = (estimated_input as f64 * input_price
                    + usage.completion_tokens as f64 * output_price)
                    / 1_000_000.0;
                cost.is_finite().then_some(cost)
            });
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
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessConnection {
    pub session_id: String,
    pub status: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeErrorResponse {
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HarnessHistoryResponse {
    session_id: String,
    messages: Vec<HarnessHistoryMessage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessHistoryMessage {
    id: String,
    role: String,
    content: String,
}

fn valid_bridge_session_id(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes().len() <= MAX_HARNESS_EVENT_IDENTIFIER_BYTES
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
}

/// Read a bounded non-streaming body before JSON decoding. `reqwest` has no
/// useful default cap here, while the Bridge is a separate local process and
/// could otherwise make the resident wallpaper allocate an arbitrary body.
async fn bounded_bridge_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    maximum: usize,
    error: &'static str,
) -> Result<T, String> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(error.into());
    }
    // Do not use `Response::bytes()` here. Without a Content-Length header
    // reqwest would retain the entire peer response before we can enforce the
    // protocol ceiling. Accumulate one transport chunk at a time instead.
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(maximum.min(64 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| error.to_string())?;
        if chunk.len() > maximum.saturating_sub(body.len()) {
            return Err(error.into());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| error.into())
}

fn valid_bridge_status(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes().len() <= MAX_HARNESS_EVENT_IDENTIFIER_BYTES
        && !value.chars().any(char::is_control)
}

fn parse_harness_connection(session: HarnessConnection) -> Result<HarnessConnection, String> {
    if !valid_bridge_session_id(&session.session_id) {
        return Err("DSH bridge 返回了无效的会话标识。".into());
    }
    // DSH may add diagnostic lifecycle labels over time. The wallpaper does
    // not branch on this field; constrain it as data instead of freezing the
    // client to a short allow-list that would reject a compatible bridge.
    if !valid_bridge_status(&session.status) {
        return Err("DSH bridge 返回了无效的会话状态。".into());
    }
    let provider = match session.provider {
        Some(value) => Some(
            bounded_bridge_text(value, MAX_HARNESS_EVENT_PROVIDER_BYTES)
                .ok_or("DSH bridge 返回了无效的会话元数据。")?,
        ),
        None => None,
    };
    let model = match session.model {
        Some(value) => Some(
            bounded_bridge_text(value, MAX_HARNESS_EVENT_IDENTIFIER_BYTES)
                .ok_or("DSH bridge 返回了无效的会话元数据。")?,
        ),
        None => None,
    };
    Ok(HarnessConnection {
        session_id: session.session_id,
        status: session.status,
        provider,
        model,
    })
}

fn parse_harness_history(
    expected_session_id: &str,
    history: HarnessHistoryResponse,
) -> Result<Value, String> {
    if history.session_id != expected_session_id || !valid_bridge_session_id(&history.session_id) {
        return Err("DSH bridge 返回了不匹配的历史会话。".into());
    }
    if history.messages.len() > MAX_HARNESS_HISTORY_MESSAGES {
        return Err("DSH bridge 返回的历史记录过多。".into());
    }
    let mut messages = Vec::with_capacity(history.messages.len());
    for message in history.messages {
        if message.id.is_empty()
            || message.id.as_bytes().len() > MAX_HARNESS_HISTORY_ID_BYTES
            || !valid_bridge_role(&message.role)
            || message.content.as_bytes().len() > MAX_HARNESS_EVENT_TEXT_BYTES
        {
            return Err("DSH bridge 返回了无效的历史记录。".into());
        }
        messages.push(serde_json::json!({
            "id": message.id,
            "role": message.role,
            "content": message.content,
        }));
    }
    Ok(serde_json::json!({
        "sessionId": expected_session_id,
        "messages": messages,
    }))
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
    // Conversation bodies are private to the WorkerW wallpaper WebView. The
    // settings surface must never receive a process-global chat event merely
    // because it happens to be open at the same time.
    let _ = app.emit_to(
        EventTarget::webview_window("background"),
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

/// User cancellation deliberately stops incremental output, but the request
/// still owns one controlled terminal transition. This narrower predicate is
/// never used for ordinary deltas or errors: it exists only so an explicit
/// Stop cannot leave the composer permanently in its streaming state.
fn emit_owned_api_event(
    app: &AppHandle,
    state: &ChatState,
    conversation_id: &str,
    request_id: u64,
    event_request_id: &str,
    event: ChatEvent,
) -> bool {
    if owns_api_request(state, request_id) {
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

/// Forward only complete, bounded SSE records from the local Bridge.  The
/// Bridge cannot choose event shape or origin: native code validates it
/// against `BridgeEvent` and stamps backend/session/request metadata before
/// emitting to the wallpaper WebView.
fn forward_harness_sse_records(
    app: &AppHandle,
    state: &ChatState,
    stream_id: u64,
    session_id: &str,
    connection_id: &str,
    records: Vec<String>,
) {
    for record in records {
        let Some(line) = sse_record_payload(&record) else {
            continue;
        };
        let Some(event) = parse_bridge_event(&line, session_id) else {
            // A malformed or out-of-contract event is not actionable UI
            // state. The bridge may still send a later valid event, so keep
            // the stream alive rather than surfacing raw local process data.
            continue;
        };
        if is_current_harness_stream(state, stream_id, session_id) {
            emit_scoped(
                app,
                "harness",
                session_id,
                Some(connection_id.to_string()),
                event,
            );
        }
    }
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
    if trimmed_text.len() > MAX_API_MESSAGE_BYTES {
        finish_api_request(&state, request_id);
        return Err(format!(
            "单条 API 消息不能超过 {MAX_API_MESSAGE_BYTES} 字节。"
        ));
    }
    let completion_url = match api_completion_url(&base_url) {
        Ok(url) => url,
        Err(error) => {
            finish_api_request(&state, request_id);
            return Err(error);
        }
    };
    let history = {
        let conversations = state
            .api_conversations
            .lock()
            .map_err(|_| "API conversation state poisoned")?;
        conversations
            .get(&conversation_id)
            .map(|conversation| conversation.messages.clone())
            .unwrap_or_default()
    };
    let messages = match api_request_messages(history, trimmed_text) {
        Ok(messages) => messages,
        Err(error) => {
            finish_api_request(&state, request_id);
            return Err(error);
        }
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
        .post(completion_url)
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
                        Ok(tail) => {
                            if buffer.len().saturating_add(tail.len()) > MAX_API_SSE_BUFFER_BYTES {
                                stream_error = Some("DeepSeek API 返回的流式记录过大，已安全停止接收。".into());
                            } else {
                                buffer.push_str(&tail);
                            }
                        }
                        Err(_) => {
                        stream_error = Some(generic_api_error("流式编码"));
                        }
                    }
                    break;
                };
                match chunk {
                    Ok(bytes) => {
                        if buffer.len().saturating_add(bytes.len()) > MAX_API_SSE_BUFFER_BYTES {
                            stream_error = Some("DeepSeek API 返回的流式记录过大，已安全停止接收。".into());
                            break;
                        }
                        let decoded = match decoder.push(&bytes) {
                            Ok(decoded) => decoded,
                            Err(_) => { stream_error = Some(generic_api_error("流式编码")); break; }
                        };
                        buffer.push_str(&decoded);
                        if buffer.len() > MAX_API_SSE_BUFFER_BYTES {
                            stream_error = Some("DeepSeek API 返回的流式记录过大，已安全停止接收。".into());
                            break;
                        }
                        for record in drain_sse_records(&mut buffer) {
                            process_api_sse_record(&app, &state, &conversation_id, request_id, &event_request_id, &record, &mut full, &mut final_usage, pricing);
                            if full.len() > MAX_API_RESPONSE_BYTES {
                                stream_error = Some(format!("DeepSeek API 回复不能超过 {MAX_API_RESPONSE_BYTES} 字节。"));
                                break;
                            }
                        }
                        if stream_error.is_some() { break; }
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
            if full.len() > MAX_API_RESPONSE_BYTES {
                stream_error = Some(format!(
                    "DeepSeek API 回复不能超过 {MAX_API_RESPONSE_BYTES} 字节。"
                ));
                break;
            }
        }
    }
    // A newer request superseded this one. Do not publish stale terminal state
    // or mutate the transcript. A user cancellation retains ownership so the
    // stream can persist its partial response and emit the controlled idle
    // transition below.
    if !owns_api_request(&state, request_id) {
        return Ok(conversation_id);
    }
    let canceled = canceled || is_api_request_cancelled(&state, request_id);
    // Ownership, append and durable merge are one transaction. This prevents a
    // completed older request from snapshotting over a newer request that was
    // installed between the prior `is_current` check and disk replacement.
    let persistence_error = (|| -> Result<(), String> {
        let _transaction = state
            .api_transcript_transaction
            .lock()
            .map_err(|_| "API transcript transaction state poisoned".to_string())?;
        if !owns_api_request(&state, request_id) {
            return Ok(());
        }
        {
            let mut conversations = state
                .api_conversations
                .lock()
                .map_err(|_| "API conversation state poisoned".to_string())?;
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
        state.persist_api_conversations_locked()
    })()
    .err();
    if !full.is_empty() {
        let emit_message = if canceled {
            emit_owned_api_event
        } else {
            emit_current_api_event
        };
        let _ = emit_message(
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
        let _ = emit_owned_api_event(
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
        // Keep the ownership marker until the streaming task has emitted its
        // terminal `idle` state.  The explicit `cancelled` bit blocks all
        // subsequent deltas while allowing that task to persist any partial
        // response and clean up only its own slot.
        if let Some(active) = guard.as_mut() {
            active.cancelled = true;
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

fn validate_bridge_token(token: String) -> Result<String, String> {
    let token = token.trim().to_string();
    if token.len() < 32 || token.len() > MAX_BRIDGE_TOKEN_FILE_BYTES {
        return Err("DSH bridge token 无效".into());
    }
    Ok(token)
}

/// Pure policy check used by the Windows reader after `GetSecurityInfo` has
/// obtained an OS-validated ACL. The bridge writer creates exactly one,
/// non-inherited Full Control allow ACE for the current user; accepting a
/// weaker or broader shape here would quietly defeat that contract.
fn bridge_token_acl_is_private(
    owner_matches_current_user: bool,
    dacl_present_and_protected: bool,
    ace_count: u32,
    allow_ace: bool,
    ace_flags: u8,
    ace_mask: u32,
    full_access_mask: u32,
    ace_sid_matches_current_user: bool,
) -> bool {
    owner_matches_current_user
        && dacl_present_and_protected
        && ace_count == 1
        && allow_ace
        && ace_flags == 0
        && ace_mask == full_access_mask
        && ace_sid_matches_current_user
}

#[cfg(not(windows))]
fn read_bridge_token() -> Result<String, String> {
    let path = bridge_token_path()?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| {
        "未找到 DSH 壁纸 bridge token；请先安装并启动 dsh-wallpaper-bridge".to_string()
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_BRIDGE_TOKEN_FILE_BYTES as u64
    {
        return Err("DSH bridge token 文件不安全".into());
    }
    let token =
        std::fs::read_to_string(path).map_err(|_| "无法读取 DSH bridge token".to_string())?;
    validate_bridge_token(token)
}

#[cfg(windows)]
fn read_bridge_token() -> Result<String, String> {
    use std::{ffi::OsStr, mem::size_of, os::windows::ffi::OsStrExt};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL},
            Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            Security::{
                EqualSid, GetAce, GetAclInformation, GetLengthSid, GetSecurityDescriptorControl,
                GetTokenInformation, IsValidSid, TokenUser, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
                ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR, PSID, SE_DACL_PRESENT, SE_DACL_PROTECTED, SID, TOKEN_QUERY,
                TOKEN_USER,
            },
            Storage::FileSystem::{
                CreateFileW, GetFileInformationByHandle, GetFileSizeEx, GetFileType, ReadFile,
                BY_HANDLE_FILE_INFORMATION, FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY,
                FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_DATA,
                FILE_SHARE_NONE, FILE_TYPE_DISK, OPEN_EXISTING, READ_CONTROL,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    struct SecurityDescriptorGuard(PSECURITY_DESCRIPTOR);
    impl Drop for SecurityDescriptorGuard {
        fn drop(&mut self) {
            if !self.0 .0.is_null() {
                unsafe {
                    let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(self.0 .0)));
                }
            }
        }
    }

    fn insecure_token_file() -> String {
        // Deliberately omit a filesystem path or token content from an error
        // that can be returned to a renderer or logs.
        "DSH bridge token 文件不安全；请重启或重新安装 dsh-wallpaper-bridge".into()
    }

    unsafe fn current_user_sid() -> Result<(HandleGuard, Vec<usize>, PSID), String> {
        let mut raw = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)
            .map_err(|_| insecure_token_file())?;
        let token = HandleGuard(raw);
        let mut needed = 0u32;
        match GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) {
            Ok(()) => return Err(insecure_token_file()),
            Err(error)
                if windows::Win32::Foundation::WIN32_ERROR::from_error(&error)
                    == Some(ERROR_INSUFFICIENT_BUFFER) => {}
            Err(_) => return Err(insecure_token_file()),
        }
        if needed < size_of::<TOKEN_USER>() as u32 || needed > 64 * 1024 {
            return Err(insecure_token_file());
        }
        // `TOKEN_USER` has pointer alignment. A `Vec<u8>` only promises byte
        // alignment, so casting it to `TOKEN_USER` would be undefined behavior
        // on architectures that require aligned reads. Keep the backing storage
        // word-aligned for the lifetime of the returned SID pointer instead.
        let byte_count = usize::try_from(needed).map_err(|_| insecure_token_file())?;
        let word_size = size_of::<usize>();
        let words = byte_count
            .checked_add(word_size.saturating_sub(1))
            .map(|total| total / word_size)
            .filter(|count| *count > 0)
            .ok_or_else(insecure_token_file)?;
        let mut storage = vec![0usize; words];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .map_err(|_| insecure_token_file())?;
        let user = &*(storage.as_ptr().cast::<TOKEN_USER>());
        if user.User.Sid.0.is_null() || !IsValidSid(user.User.Sid).as_bool() {
            return Err(insecure_token_file());
        }
        Ok((token, storage, user.User.Sid))
    }

    let path = bridge_token_path()?;
    let wide: Vec<u16> = OsStr::new(&path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Do not follow a final reparse point, do not share this handle with a
    // writer/deleter, and use it for both ACL inspection and token bytes.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            READ_CONTROL.0 | FILE_READ_DATA.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map(HandleGuard)
    .map_err(|_| "未找到 DSH 壁纸 bridge token；请先安装并启动 dsh-wallpaper-bridge".to_string())?;

    unsafe {
        if GetFileType(handle.0) != FILE_TYPE_DISK {
            return Err(insecure_token_file());
        }
        let mut file_info = BY_HANDLE_FILE_INFORMATION::default();
        GetFileInformationByHandle(handle.0, &mut file_info).map_err(|_| insecure_token_file())?;
        if file_info.dwFileAttributes
            & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0)
            != 0
        {
            return Err(insecure_token_file());
        }

        let (_process_token, token_user_storage, current_sid) = current_user_sid()?;
        // Keep the token-information backing bytes alive through SID checks.
        let _keep_current_sid_alive = token_user_storage;
        let mut owner = PSID::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            handle.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor),
        )
        .ok()
        .map_err(|_| insecure_token_file())?;
        let _descriptor = SecurityDescriptorGuard(descriptor);
        // A missing descriptor or DACL is not equivalent to a private token
        // file. Reject it before handing either pointer to additional Win32
        // parsing APIs.
        if descriptor.0.is_null() || dacl.is_null() {
            return Err(insecure_token_file());
        }
        let owner_matches_current_user = !owner.0.is_null()
            && IsValidSid(owner).as_bool()
            && EqualSid(owner, current_sid).is_ok();

        let mut control = 0u16;
        let mut revision = 0u32;
        GetSecurityDescriptorControl(descriptor, &mut control, &mut revision)
            .map_err(|_| insecure_token_file())?;
        let control = windows::Win32::Security::SECURITY_DESCRIPTOR_CONTROL(control);
        let dacl_present_and_protected =
            control.contains(SE_DACL_PRESENT) && control.contains(SE_DACL_PROTECTED);

        let mut acl_info = ACL_SIZE_INFORMATION::default();
        GetAclInformation(
            dacl,
            (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            windows::Win32::Security::AclSizeInformation,
        )
        .map_err(|_| insecure_token_file())?;
        if acl_info.AceCount == 0
            || acl_info.AclBytesInUse < size_of::<ACL>() as u32
            || acl_info.AclBytesInUse > (*dacl).AclSize as u32
        {
            return Err(insecure_token_file());
        }
        let mut raw_ace: *mut core::ffi::c_void = std::ptr::null_mut();
        GetAce(dacl, 0, &mut raw_ace).map_err(|_| insecure_token_file())?;
        if raw_ace.is_null() {
            return Err(insecure_token_file());
        }
        let header = &*(raw_ace.cast::<ACE_HEADER>());
        if header.AceSize < size_of::<ACCESS_ALLOWED_ACE>() as u16
            || header.AceSize as u32 > acl_info.AclBytesInUse
        {
            return Err(insecure_token_file());
        }
        let ace = &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>());
        let ace_sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        let ace_sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        let available_sid_bytes = (header.AceSize as usize).saturating_sub(ace_sid_offset);
        // `IsValidSid` and `GetLengthSid` accept only a pointer, so they cannot
        // know the enclosing ACE boundary. Check the fixed SID header and its
        // declared sub-authority tail ourselves before passing the pointer to
        // either API. This keeps a malformed on-disk ACL from making Win32 read
        // beyond this ACE even though `GetAce` returned a pointer.
        let sid_header_bytes = std::mem::offset_of!(SID, SubAuthority);
        let ace_sid_fits = !ace_sid.0.is_null() && available_sid_bytes >= sid_header_bytes;
        let declared_sid_bytes = if ace_sid_fits {
            let sub_authority_count = (*ace_sid.0.cast::<SID>()).SubAuthorityCount as usize;
            sid_header_bytes.checked_add(sub_authority_count.saturating_mul(size_of::<u32>()))
        } else {
            None
        };
        let ace_sid_fits = declared_sid_bytes
            .filter(|sid_length| *sid_length > 0 && *sid_length <= available_sid_bytes)
            .is_some_and(|sid_length| {
                IsValidSid(ace_sid).as_bool()
                    && usize::try_from(GetLengthSid(ace_sid)).ok() == Some(sid_length)
            });
        let ace_sid_matches_current_user =
            !ace_sid.0.is_null() && ace_sid_fits && EqualSid(ace_sid, current_sid).is_ok();
        if !bridge_token_acl_is_private(
            owner_matches_current_user,
            dacl_present_and_protected,
            acl_info.AceCount,
            header.AceType == 0,
            header.AceFlags,
            ace.Mask,
            FILE_ALL_ACCESS.0,
            ace_sid_matches_current_user,
        ) {
            return Err(insecure_token_file());
        }

        let mut size = 0i64;
        GetFileSizeEx(handle.0, &mut size).map_err(|_| insecure_token_file())?;
        if !(1..=MAX_BRIDGE_TOKEN_FILE_BYTES as i64).contains(&size) {
            return Err(insecure_token_file());
        }
        let mut bytes = vec![0u8; size as usize];
        let mut read = 0u32;
        ReadFile(handle.0, Some(&mut bytes), Some(&mut read), None)
            .map_err(|_| insecure_token_file())?;
        if read as usize != bytes.len() {
            return Err(insecure_token_file());
        }
        validate_bridge_token(String::from_utf8(bytes).map_err(|_| insecure_token_file())?)
    }
}

fn auth(client: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    client.bearer_auth(token)
}

pub async fn harness_connect(
    app: AppHandle,
    state: tauri::State<'_, ChatState>,
    resume_session_id: Option<String>,
    connection_id: String,
    model: Option<String>,
) -> Result<String, String> {
    let connection_id = connection_id.trim().to_string();
    if connection_id.is_empty() || connection_id.len() > 200 {
        return Err("Harness 连接标识无效".into());
    }
    cancel_harness_stream(&state);
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let model = model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned);
    if model.as_ref().is_some_and(|model| !valid_bridge_status(model)) {
        return Err("Harness 模型标识无效".into());
    }
    let create_session = |resume_session_id: Option<&str>| {
        auth(
            client.post("http://127.0.0.1:3080/api/wallpaper/v1/sessions"),
            &token,
        )
        .json(&HarnessSessionRequest { resume_session_id, model: model.as_deref() })
        .send()
    };
    let mut response = create_session(resume_session_id.as_deref())
        .await
        .map_err(|_| generic_harness_error("连接"))?;
    // Persistence is optional in DSH. Only the explicit bridge 409 contract
    // gets a one-time new-session retry; never turn arbitrary failed resumes
    // into a fresh transcript silently.
    if resume_session_id.is_some() && response.status() == reqwest::StatusCode::CONFLICT {
        let resume_unavailable = bounded_bridge_json::<BridgeErrorResponse>(
            response,
            MAX_HARNESS_SESSION_RESPONSE_BYTES,
            "DSH bridge 返回了无法识别的会话响应。",
        )
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
    let session = parse_harness_connection(
        bounded_bridge_json::<HarnessConnection>(
            response,
            MAX_HARNESS_SESSION_RESPONSE_BYTES,
            "DSH bridge 返回了无法识别的会话响应。",
        )
        .await?,
    )?;
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
                        let tail = match decoder.finish() {
                            Ok(tail) => tail,
                            Err(_) => {
                                emit_stream_error("HARNESS_SSE_ENCODING", generic_harness_error("事件流编码"));
                                break;
                            }
                        };
                        let records = match drain_bounded_harness_sse_records(&mut buffer, &tail) {
                            Ok(records) => records,
                            Err(()) => {
                                emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                                break;
                            }
                        };
                        forward_harness_sse_records(&app, &state, stream_id, &session_id, &connection_id, records);
                        let records = match finish_bounded_harness_sse_records(&mut buffer) {
                            Ok(records) => records,
                            Err(()) => {
                                emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                                break;
                            }
                        };
                        forward_harness_sse_records(&app, &state, stream_id, &session_id, &connection_id, records);
                        emit_stream_error("HARNESS_DISCONNECTED", "DSH bridge 事件流已断开".into());
                        break;
                    };
                    match chunk {
                        Ok(bytes) => {
                            if bytes.len() > MAX_HARNESS_SSE_CHUNK_BYTES {
                                emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                                break;
                            }
                            let decoded = match decoder.push(&bytes) {
                                Ok(decoded) => decoded,
                                Err(_) => { emit_stream_error("HARNESS_SSE_ENCODING", generic_harness_error("事件流编码")); break; }
                            };
                            let records = match drain_bounded_harness_sse_records(&mut buffer, &decoded) {
                                Ok(records) => records,
                                Err(()) => {
                                    emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                                    break;
                                }
                            };
                            forward_harness_sse_records(&app, &state, stream_id, &session_id, &connection_id, records);
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
    let history = bounded_bridge_json::<HarnessHistoryResponse>(
        response,
        MAX_HARNESS_HISTORY_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的历史记录。",
    )
    .await?;
    parse_harness_history(&session_id, history)
}

pub async fn harness_presets() -> Result<Value, String> {
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let response = auth(client.get("http://127.0.0.1:3080/api/wallpaper/v1/control/presets"), &token)
        .send().await.map_err(|_| generic_harness_error("模式目录读取"))?;
    if !response.status().is_success() { return Err(generic_bridge_http_error(response.status())); }
    bounded_bridge_json::<Value>(response, MAX_HARNESS_SESSION_RESPONSE_BYTES, "DSH bridge 返回了无法识别的模式目录。" ).await
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
        api_completion_url, api_request_messages, bridge_token_acl_is_private,
        drain_bounded_harness_sse_records, drain_sse_records, finish_api_request,
        finish_bounded_harness_sse_records, finish_harness_stream, finish_sse_records,
        is_current_api_request, is_current_harness_stream, owns_api_request, parse_bridge_event,
        parse_harness_connection, parse_harness_history, sse_record_payload, ApiMessage,
        ApiPricing, ApiUsage, ChatEvent, ChatState, HarnessConnection, HarnessHistoryMessage,
        HarnessHistoryResponse, HarnessStreamCancellation, Usage, Utf8StreamDecoder,
        MAX_API_RATE_PER_MILLION, MAX_API_REQUEST_CONTEXT_BYTES, MAX_HARNESS_EVENT_TEXT_BYTES,
        MAX_HARNESS_MESSAGE_BYTES, MAX_HARNESS_SSE_BUFFER_BYTES, MAX_HARNESS_SSE_EVENT_BYTES,
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
    fn harness_sse_enforces_bounded_retention_and_complete_event_limits() {
        let mut incomplete = String::new();
        assert!(drain_bounded_harness_sse_records(
            &mut incomplete,
            &format!("data: {}", "x".repeat(MAX_HARNESS_SSE_BUFFER_BYTES)),
        )
        .is_err());
        // A complete final message may be larger than the retained incomplete
        // buffer, as long as it fits the protocol's response/event limit.
        let mut complete = String::new();
        let payload = "x".repeat(MAX_HARNESS_SSE_BUFFER_BYTES + 1);
        let record = format!("data: {payload}\n\n");
        let records = drain_bounded_harness_sse_records(&mut complete, &record)
            .expect("complete bounded record");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].len(), record.len() - 2);
        assert!(complete.is_empty());

        let mut oversized = String::new();
        let record = format!("data: {}\n\n", "x".repeat(MAX_HARNESS_SSE_EVENT_BYTES));
        assert!(drain_bounded_harness_sse_records(&mut oversized, &record).is_err());

        let mut eof_tail = format!("data: {}", "x".repeat(MAX_HARNESS_SSE_BUFFER_BYTES + 1));
        assert!(finish_bounded_harness_sse_records(&mut eof_tail).is_err());
    }

    #[test]
    fn bridge_sse_events_are_closed_typed_and_origin_stamped_by_native_code() {
        let session_id = "wallpaper-session";
        assert!(matches!(
            parse_bridge_event(r#"{"type":"delta","text":"鲸鱼"}"#, session_id),
            Some(ChatEvent::Delta { text }) if text == "鲸鱼"
        ));
        assert!(matches!(
            parse_bridge_event(
                r#"{"type":"approval-required","sessionId":"wallpaper-session","summary":"工具需要批准"}"#,
                session_id,
            ),
            Some(ChatEvent::ApprovalRequired { session_id: event_session_id, summary })
                if event_session_id == session_id && summary == "工具需要批准"
        ));
        assert!(parse_bridge_event(
            r#"{"type":"delta","text":"x","backend":"deepseek-api"}"#,
            session_id,
        )
        .is_none());
        assert!(
            parse_bridge_event(r#"{"type":"status","activity":"forged"}"#, session_id,).is_none()
        );
        assert!(parse_bridge_event(
            r#"{"type":"approval-required","sessionId":"other-session","summary":"工具需要批准"}"#,
            session_id,
        )
        .is_none());
    }

    #[test]
    fn bridge_sse_events_reject_oversized_text_and_non_finite_costs() {
        let session_id = "wallpaper-session";
        let oversized = format!(
            r#"{{"type":"delta","text":"{}"}}"#,
            "x".repeat(MAX_HARNESS_EVENT_TEXT_BYTES + 1)
        );
        assert!(parse_bridge_event(&oversized, session_id).is_none());
        assert!(matches!(
            parse_bridge_event(
                r#"{"type":"usage","input":10,"output":2,"cacheRead":50,"cost":1.5}"#,
                session_id,
            ),
            Some(ChatEvent::Usage {
                input: 10,
                output: 2,
                cache_read: Some(10),
                cost: Some(1.5),
                ..
            })
        ));
        assert!(parse_bridge_event(
            r#"{"type":"usage","input":10,"output":2,"cost":1e999}"#,
            session_id,
        )
        .is_none());
    }

    #[test]
    fn bridge_non_streaming_responses_are_closed_typed_and_bounded() {
        assert!(parse_harness_connection(HarnessConnection {
            session_id: "wallpaper-session".into(),
            status: "idle".into(),
            provider: Some("deepseek".into()),
            model: Some("deepseek-chat".into()),
        })
        .is_ok());
        for invalid_id in ["", "../outside", "has/control", &"x".repeat(201)] {
            assert!(parse_harness_connection(HarnessConnection {
                session_id: invalid_id.into(),
                status: "idle".into(),
                provider: None,
                model: None,
            })
            .is_err());
        }
        assert!(parse_harness_connection(HarnessConnection {
            session_id: "wallpaper-session".into(),
            status: "waiting-for-tool".into(),
            provider: None,
            model: None,
        })
        .is_ok());
        assert!(parse_harness_connection(HarnessConnection {
            session_id: "wallpaper-session".into(),
            status: "contains\ncontrol".into(),
            provider: None,
            model: None,
        })
        .is_err());

        let valid = HarnessHistoryResponse {
            session_id: "wallpaper-session".into(),
            messages: vec![HarnessHistoryMessage {
                id: "message-1".into(),
                role: "assistant".into(),
                content: "鲸鱼回复".into(),
            }],
        };
        let parsed = parse_harness_history("wallpaper-session", valid).expect("valid history");
        assert_eq!(parsed["messages"][0]["content"], "鲸鱼回复");
        assert!(parse_harness_history(
            "wallpaper-session",
            HarnessHistoryResponse {
                session_id: "other-session".into(),
                messages: vec![],
            },
        )
        .is_err());
        assert!(parse_harness_history(
            "wallpaper-session",
            HarnessHistoryResponse {
                session_id: "wallpaper-session".into(),
                messages: vec![HarnessHistoryMessage {
                    id: "message-1".into(),
                    role: "system".into(),
                    content: "not a renderer role".into(),
                }],
            },
        )
        .is_err());
    }

    #[test]
    fn bridge_token_acl_policy_requires_one_private_full_control_allow_ace() {
        assert!(bridge_token_acl_is_private(
            true,
            true,
            1,
            true,
            0,
            0x001F_01FF,
            0x001F_01FF,
            true,
        ));
        for rejected in [
            bridge_token_acl_is_private(false, true, 1, true, 0, 7, 7, true),
            bridge_token_acl_is_private(true, false, 1, true, 0, 7, 7, true),
            bridge_token_acl_is_private(true, true, 0, true, 0, 7, 7, true),
            bridge_token_acl_is_private(true, true, 2, true, 0, 7, 7, true),
            bridge_token_acl_is_private(true, true, 1, false, 0, 7, 7, true),
            bridge_token_acl_is_private(true, true, 1, true, 16, 7, 7, true),
            bridge_token_acl_is_private(true, true, 1, true, 0, 1, 7, true),
            bridge_token_acl_is_private(true, true, 1, true, 0, 7, 7, false),
        ] {
            assert!(!rejected);
        }
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
    fn cancelled_request_keeps_terminal_ownership_but_stops_publishing() {
        let state = ChatState::default();
        let request = super::begin_api_request(&state).expect("request").0;
        assert!(is_current_api_request(&state, request));
        super::cancel_api(&state);
        assert!(owns_api_request(&state, request));
        assert!(!is_current_api_request(&state, request));
        assert!(finish_api_request(&state, request));
        assert!(!owns_api_request(&state, request));
    }

    #[test]
    fn api_base_url_policy_protects_bearer_credentials() {
        assert_eq!(
            api_completion_url("https://api.deepseek.com/v1/")
                .expect("https URL")
                .as_str(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert!(api_completion_url("http://127.0.0.1:8080/v1").is_ok());
        for unsafe_url in [
            "http://api.example.test/v1",
            "https://user:password@api.example.test/v1",
            "https://api.example.test/v1?redirect=https://evil.test",
            "https://api.example.test/v1#fragment",
            "file:///C:/not-an-api",
        ] {
            assert!(api_completion_url(unsafe_url).is_err(), "{unsafe_url}");
        }
    }

    #[test]
    fn request_context_keeps_newest_history_under_a_byte_ceiling() {
        let history = vec![
            ApiMessage {
                id: "old".into(),
                role: "user".into(),
                content: "x".repeat(MAX_API_REQUEST_CONTEXT_BYTES),
                created_at: 1,
                usage: None,
            },
            ApiMessage {
                id: "recent".into(),
                role: "assistant".into(),
                content: "recent".into(),
                created_at: 2,
                usage: None,
            },
        ];
        let messages = api_request_messages(history, "new turn").expect("bounded context");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["content"], "recent");
        assert_eq!(messages[1]["content"], "new turn");
        assert!(
            api_request_messages(vec![], &"x".repeat(MAX_API_REQUEST_CONTEXT_BYTES + 1)).is_err()
        );
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

        let malformed =
            ApiPricing::from_options(Some(MAX_API_RATE_PER_MILLION + 1.0), Some(f64::INFINITY))
                .usage(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 10,
                    prompt_cache_hit_tokens: Some(999),
                });
        assert_eq!(malformed.cache_read, Some(10));
        assert_eq!(malformed.input, 0);
        assert_eq!(malformed.cost, None);
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
