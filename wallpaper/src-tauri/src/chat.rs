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
/// 渲染端要认的错误码：这条 Harness 会话已被用户在桌面端归档（桥不再接受它的消息）。
///
/// 与桥的 `session-archived` 一一对应，是一条**跨进程契约**：改这里就要改渲染端的判断，
/// 两边各有一条测试钉住同一个字面量。
pub const HARNESS_SESSION_ARCHIVED: &str = "HARNESS_SESSION_ARCHIVED";
/// "这条会话还没建立"的稳定标记，与渲染端 `nativeAdapter.ts` 的 `HARNESS_NO_SESSION` 一一对应。
///
/// 为什么需要它：会话是在 `POST /sessions` 时建立的，而**切换主体不会重建渲染端的聊天适配器**
/// （适配器持有的是"连着哪个端点、哪条会话"）。于是出现这样一种边界：原生侧的探测范围已换成新主体
/// （指示灯因此变绿），事件流和会话却还留在旧端点上 —— 用户看到"已连接"，一发消息却被告知会话没有
/// 建立。一句普通字符串渲染端接不住，只能把它显示出来；带上这个标记，它就能**先连上再重发一次**。
pub const HARNESS_NO_SESSION: &str = "HARNESS_NO_SESSION";

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
/// No-data deadline for one read on an API response stream. The *total*
/// lifetime stays long because a single answer can legitimately stream for
/// minutes, but a half-open connection that stops producing bytes must not hold
/// the composer's request slot until the 24-hour ceiling expires. Every
/// received chunk restarts this deadline, so a slow but live model is never
/// mistaken for a stalled one.
const API_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// The DSH bridge heartbeats every 15s, so this window tolerates three missed
/// beats before the desktop declares the event stream dead.
const HARNESS_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Stable error code the renderer can rely on for a stalled stream. It is
/// deliberately distinct from a transport failure so the UI can say "no data
/// for N seconds" instead of blaming the network.
const API_STREAM_IDLE_TIMEOUT_CODE: &str = "DEEPSEEK_API_IDLE_TIMEOUT";
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

/// Whether a re-request may replace the dropped event stream.
///
/// A reconnect is only accepted when the response is successful *and* carries
/// the ready acknowledgement. A 200 without it means a Bridge answered on the
/// same path but did not subscribe this reader, so treating it as a live stream
/// would leave the wallpaper waiting forever on a response nobody writes to.
///
/// Takes the two primitives rather than the response so the rule is testable
/// without an `AppHandle` or a real socket, which the surrounding loop requires.
fn reconnect_is_accepted(status: u16, ready_header: Option<&str>) -> bool {
    (200..300).contains(&status) && ready_header == Some("1")
}

fn reconnect_response_is_ready(response: &reqwest::Response) -> bool {
    reconnect_is_accepted(
        response.status().as_u16(),
        response
            .headers()
            .get("x-dsh-wallpaper-sse-ready")
            .and_then(|value| value.to_str().ok()),
    )
}

fn generic_harness_error(stage: &str) -> String {
    format!("DSH bridge {stage}失败；请确认 Harness 与壁纸 bridge 仍在运行。")
}

/// Absolute URL for a wallpaper Bridge route on the selected endpoint.
///
/// Every harness request goes through here so no call site can silently keep
/// pointing at the default port. That mattered: the wallpaper probed one
/// hardcoded port while the official desktop shell listens on 19387, so a session
/// created against the selected shell would have been sent to whatever happened to
/// be on 3080 — or to nothing at all.
fn harness_url(port: u16, route: &str) -> String {
    format!("http://127.0.0.1:{port}/api/wallpaper/v1{route}")
}

fn generic_bridge_http_error(status: reqwest::StatusCode) -> String {
    format!(
        "DSH bridge 请求被拒绝（HTTP {}）。请确认 bridge 版本、会话状态与连接后重试。",
        status.as_u16()
    )
}

/// Explain a specific rejection instead of collapsing every status into one
/// sentence. The 404 case is the one the compatibility work exists for: `/status`
/// answered, so a Bridge *is* mounted, but the session route did not exist. That
/// is what a stale or partially composed Bridge looks like from the client, and
/// nothing the user can do inside the composer will fix it.
fn harness_http_error(status: reqwest::StatusCode, route: &str) -> String {
    match status.as_u16() {
        401 => "DSH bridge 拒绝了本机令牌（HTTP 401）。请重启壁纸应用以重新生成并读取令牌。".to_string(),
        404 if route == "sessions" => "DSH bridge 已响应，但没有会话接口（HTTP 404）。通常是 profile 内安装的 Bridge 版本过旧或与当前 DSH 不兼容；请更新 Bridge 后重试。".to_string(),
        404 => "DSH bridge 没有该接口（HTTP 404），当前 Bridge 版本可能过旧。".to_string(),
        409 => "DSH bridge 会话冲突（HTTP 409）；该会话可能已被其他 DSH 实例占用。".to_string(),
        413 => "消息超出 DSH bridge 允许的大小（HTTP 413）。".to_string(),
        429 => "DSH bridge 已达到并发上限（HTTP 429）；请关闭部分桌面会话后重试。".to_string(),
        503 => "DSH bridge 暂不可用（HTTP 503）；可能正在关闭或令牌尚未就绪。".to_string(),
        _ => generic_bridge_http_error(status),
    }
}

/// What one watchdog-guarded stream read produced.
enum StreamRead<T> {
    /// A transport event arrived (possibly the end of the stream).
    Event(Option<T>),
    /// The peer produced nothing within the idle deadline.
    Idle,
    /// The user cancelled: cancellation always wins over the idle deadline.
    Cancelled,
}

/// Await the next item on `stream` with an idle deadline.
///
/// `timeout` is a *per-read* budget, not a total lifetime: every event resets
/// it. Cancellation is polled independently and takes precedence, so a user
/// stop is never reported as a timeout.
async fn next_with_idle_timeout<S, T>(
    stream: &mut S,
    cancel: &mut oneshot::Receiver<()>,
    idle: Duration,
) -> StreamRead<T>
where
    S: futures_util::Stream<Item = T> + Unpin,
{
    let deadline = tokio::time::sleep(idle);
    tokio::pin!(deadline);
    tokio::select! {
        biased;
        _ = &mut *cancel => StreamRead::Cancelled,
        item = stream.next() => StreamRead::Event(item),
        _ = &mut deadline => StreamRead::Idle,
    }
}

fn api_idle_timeout_error() -> String {
    format!(
        "DeepSeek API 已 {} 秒没有返回数据，本次请求已安全停止；请检查网络后重试。",
        API_STREAM_IDLE_TIMEOUT.as_secs()
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
    /// The transcript this process is currently reading. Set when a send
    /// starts, so a listing can mark it and so trimming protects it even when
    /// the caller cannot name it.
    active_api_conversation: Arc<Mutex<Option<String>>>,
    /// The DSH endpoint port the current session talks to.
    ///
    /// Shared like the other locks so a detached SSE reader sees the same
    /// endpoint the session was created against, even if the user later changes
    /// the dropdown. The three client shapes listen on different ports, so a
    /// session opened against the official desktop shell must not have its
    /// messages sent to whatever happens to be on the CLI's default port.
    harness_port: Arc<Mutex<Option<u16>>>,
}

/// DSH's own web default; the fallback when nothing has been selected yet.
pub const DEFAULT_HARNESS_PORT: u16 = 3080;

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
            active_api_conversation: Arc::default(),
            harness_port: Arc::default(),
        }
    }

    /// The endpoint this session talks to.
    pub fn harness_port(&self) -> u16 {
        self.harness_port
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .unwrap_or(DEFAULT_HARNESS_PORT)
    }

    /// Point this session at an endpoint. Called when a session is created, so a
    /// later change of the settings dropdown cannot redirect an open session.
    pub fn set_harness_port(&self, port: u16) {
        if let Ok(mut guard) = self.harness_port.lock() {
            *guard = Some(port);
        }
    }

    /// Read the durable archive without adopting it.
    ///
    /// Used by the settings-facing listing, which must observe deletions and
    /// other processes' writes rather than this process's snapshot. Reading
    /// never overwrites the archive: an unreadable or future-schema archive is
    /// reported as an error, exactly like a load.
    fn load_api_archive(&self) -> Result<HashMap<String, ApiConversation>, String> {
        let store_ref = {
            let store = self
                .api_store
                .lock()
                .map_err(|_| "API conversation storage state poisoned".to_string())?;
            store.store.clone()
        };
        match store_ref.load::<ApiConversationArchive>() {
            Ok(Some(archive)) if archive.schema_version == API_CONVERSATION_SCHEMA_VERSION => {
                Ok(archive.conversations)
            }
            Ok(Some(_)) => Err("API 会话记录由更高版本写入；本版本不会修改它。".into()),
            Ok(None) => Ok(HashMap::new()),
            Err(_) => Err("无法读取加密 API 会话记录。".into()),
        }
    }

    /// Record which transcript this process is currently reading.
    fn set_active_api_conversation(&self, conversation_id: &str) {
        if let Ok(mut active) = self.active_api_conversation.lock() {
            *active = Some(conversation_id.to_string());
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
        self.persist_api_conversations_for(None)
    }

    /// Persist the in-memory transcripts, trimming to the durable budget first.
    ///
    /// `protected` names the transcript the user is currently in; it is the
    /// last thing trimmed.  A write that still cannot fit gives up for the rest
    /// of this process (`writable = false`) so every later turn does not repeat
    /// a full copy/merge/serialize cycle against a doomed archive.
    fn persist_api_conversations_for(&self, protected: Option<&str>) -> Result<(), String> {
        let mut store = self
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
        // Clone the store handle so the (slow) cross-process transaction does
        // not hold the store mutex. The `writable` flag, not the mutex, is what
        // concurrent turns observe.
        let store_ref = store.store.clone();
        let outcome = store_ref.update::<ApiConversationArchive, _, _>(|existing| {
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
            trim_api_archive(&mut conversations, protected);
            let archive = ApiConversationArchive {
                schema_version: API_CONVERSATION_SCHEMA_VERSION,
                conversations: conversations.clone(),
            };
            // Fail closed inside the transaction, so an archive that still
            // cannot fit never replaces the ciphertext already on disk.
            crate::api_persistence::EncryptedJsonStore::ensure_fits(&archive)?;
            Ok((archive, conversations))
        });
        match outcome {
            Ok(merged) => {
                // Adopt the durable merged view while still inside the
                // transaction. This prevents a second process's transcript from
                // being forgotten by the next local append.
                *self
                    .api_conversations
                    .lock()
                    .map_err(|_| "API conversation state poisoned".to_string())? = merged;
                Ok(())
            }
            Err(crate::api_persistence::PersistenceError::TooLarge) => {
                // Stop retrying for this process instead of re-copying a
                // hopeless archive on every later turn. The existing ciphertext
                // is untouched and in-memory chat keeps working.
                store.writable = false;
                Err("API 会话记录已达到容量上限，本次运行不再写入磁盘；已有记录未被覆盖。请在设置中删除部分 API 会话后重启应用。".into())
            }
            Err(_) => Err("无法保存加密 API 会话记录；已有记录未被覆盖。".to_string()),
        }
    }

    /// Delete one API transcript under both the in-process transaction guard
    /// and the cross-process store lock, so a concurrent writer cannot
    /// resurrect the deleted conversation through a stale merge.
    pub fn delete_api_conversation(&self, conversation_id: &str) -> Result<bool, String> {
        let mut removed = false;
        self.update_api_archive(|conversations| {
            removed = conversations.remove(conversation_id).is_some();
        })?;
        if removed {
            // A deleted transcript must not stay claimed as this process's
            // active one, or a later listing would mark a nonexistent row.
            if let Ok(mut active) = self.active_api_conversation.lock() {
                if active.as_deref() == Some(conversation_id) {
                    active.take();
                }
            }
        }
        Ok(removed)
    }

    /// Delete every API transcript this process can see.
    pub fn clear_api_conversations(&self) -> Result<usize, String> {
        let mut count = 0;
        self.update_api_archive(|conversations| {
            count = conversations.len();
            conversations.clear();
        })?;
        if let Ok(mut active) = self.active_api_conversation.lock() {
            active.take();
        }
        Ok(count)
    }

    /// Rewrite the durable archive through one locked read/modify/write.  The
    /// operation is deliberately infallible: an unexpected failure must leave
    /// the on-disk ciphertext untouched, not half-applied.
    fn update_api_archive<F>(&self, mutate: F) -> Result<(), String>
    where
        F: FnOnce(&mut HashMap<String, ApiConversation>),
    {
        let _transaction = self
            .api_transcript_transaction
            .lock()
            .map_err(|_| "API transcript transaction state poisoned".to_string())?;
        let mut store = self
            .api_store
            .lock()
            .map_err(|_| "API conversation storage state poisoned".to_string())?;
        if !store.writable {
            return Err("加密 API 会话记录不可用；为保护已有记录，本次不会覆盖它。".into());
        }
        let store_ref = store.store.clone();
        let outcome = store_ref.update::<ApiConversationArchive, _, _>(|existing| {
            let mut conversations = match existing {
                Some(archive) if archive.schema_version == API_CONVERSATION_SCHEMA_VERSION => {
                    archive.conversations
                }
                Some(_) => {
                    return Err(crate::api_persistence::PersistenceError::InvalidArchive)
                }
                None => HashMap::new(),
            };
            mutate(&mut conversations);
            let archive = ApiConversationArchive {
                schema_version: API_CONVERSATION_SCHEMA_VERSION,
                conversations: conversations.clone(),
            };
            crate::api_persistence::EncryptedJsonStore::ensure_fits(&archive)?;
            Ok((archive, conversations))
        });
        match outcome {
            Ok(merged) => {
                *self
                    .api_conversations
                    .lock()
                    .map_err(|_| "API conversation state poisoned".to_string())? = merged;
                Ok(())
            }
            Err(crate::api_persistence::PersistenceError::TooLarge) => Err(
                "API 会话记录达到容量上限；请先删除部分 API 会话后重试。".into(),
            ),
            Err(_) => Err("无法更新加密 API 会话记录；已有记录未被覆盖。".to_string()),
        }
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
#[serde(rename_all = "camelCase")]
struct ApiConversation {
    messages: Vec<ApiMessage>,
    /// Last time this transcript was written.  Older archives predate the
    /// field, so a missing value falls back to the newest message timestamp.
    #[serde(default, skip_serializing_if = "is_zero")]
    updated_at: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl ApiConversation {
    /// `updated_at` is the eviction key.  Never let a transcript that predates
    /// the field (or one whose clock is missing) sort as newer than a real one.
    fn effective_updated_at(&self) -> u64 {
        if self.updated_at != 0 {
            return self.updated_at;
        }
        self.messages
            .iter()
            .map(|message| message.created_at)
            .max()
            .unwrap_or(0)
    }
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
        target.updated_at = target.updated_at.max(incoming.updated_at);
    }
}

/// Target size for one encrypted API archive.  The persistence layer still
/// refuses anything above its own hard ceiling; this lower budget is what the
/// application trims to *before* attempting a write, so a long-lived install
/// drops its oldest turns instead of permanently failing to persist.
pub const API_CONVERSATION_TARGET_PLAINTEXT_BYTES: usize = 12 * 1024 * 1024;
/// Never split a message body, so the only lever left for one conversation is
/// how many recent turns it keeps.
const MAX_API_MESSAGES_PER_CONVERSATION: usize = 400;
/// A conversation is one transcript, not one message: keep a generous number
/// of them and evict by last activity instead of by insertion order.
const MAX_API_CONVERSATIONS: usize = 200;
const MAX_API_TRIM_PASSES: usize = 64;

/// Effective activity rank used for eviction.
fn conversation_rank(conversation: &ApiConversation) -> u64 {
    conversation.effective_updated_at()
}

fn archive_fits(conversations: &HashMap<String, ApiConversation>) -> bool {
    let len: usize = EncryptedJsonStore::serialized_len(&ApiConversationArchive {
        schema_version: API_CONVERSATION_SCHEMA_VERSION,
        conversations: conversations.clone(),
    })
    .unwrap_or(usize::MAX);
    len <= API_CONVERSATION_TARGET_PLAINTEXT_BYTES
}

/// Trim an archive to the persistence budget without ever cutting a message
/// body in half and without rewriting the identity (id/timestamp) of a
/// message it keeps.
///
/// Order of sacrifice, cheapest loss first:
/// 1. everything older than the per-conversation message window;
/// 2. whole *other* conversations, oldest activity first;
/// 3. only as a last resort, the oldest messages of `protected` itself, which
///    is the transcript the user is currently reading.
///
/// Returns `true` when anything was dropped.
fn trim_api_archive(
    conversations: &mut HashMap<String, ApiConversation>,
    protected: Option<&str>,
) -> bool {
    let mut dropped = false;

    // 1. Per-conversation window, anchored at the newest message.
    for conversation in conversations.values_mut() {
        if conversation.messages.len() > MAX_API_MESSAGES_PER_CONVERSATION {
            let excess = conversation.messages.len() - MAX_API_MESSAGES_PER_CONVERSATION;
            conversation.messages.drain(..excess);
            dropped = true;
        }
    }
    if archive_fits(conversations) {
        return dropped;
    }

    // 2. Conversation count, oldest activity first.
    if conversations.len() > MAX_API_CONVERSATIONS {
        for (id, _) in ranked_eviction_order(conversations, protected)
            .into_iter()
            .take(conversations.len() - MAX_API_CONVERSATIONS)
        {
            conversations.remove(&id);
            dropped = true;
        }
        if archive_fits(conversations) {
            return true;
        }
    }
    // 3. Still too large: drop whole conversations by age, keeping the one the
    // caller protects until nothing else is left.
    //
    // There is deliberately no further step. If `protected` alone exceeds the
    // budget, refusing the write (the caller's `TooLarge` path) is the correct
    // outcome: silently mutilating the transcript the user is reading is worse
    // than keeping it in memory and telling them history cannot be persisted.
    for _ in 0..MAX_API_TRIM_PASSES {
        let Some((id, _)) = ranked_eviction_order(conversations, protected)
            .into_iter()
            .next()
        else {
            break;
        };
        conversations.remove(&id);
        dropped = true;
        if archive_fits(conversations) {
            return true;
        }
    }
    dropped
}

/// Candidates for eviction, least recently active first.  An empty message
/// list with no timestamp sorts first, so a corrupt or empty entry never
/// outranks a real transcript.
fn ranked_eviction_order(
    conversations: &HashMap<String, ApiConversation>,
    protected: Option<&str>,
) -> Vec<(String, u64)> {
    let mut ranked: Vec<(String, u64)> = conversations
        .iter()
        .filter(|(id, _)| Some(id.as_str()) != protected)
        .map(|(id, conversation)| (id.clone(), conversation_rank(conversation)))
        .collect();
    ranked.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    ranked
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

/// The model-list route on the same endpoint the key was configured for.
///
/// Built from [`normalized_api_base_url`] for the same reason the completion
/// route is: the bearer key must never be sent to a host that policy would not
/// accept, and the route is joined as a path so a query string cannot change the
/// authority.
fn api_models_url(base_url: &str) -> Result<reqwest::Url, String> {
    let normalized = normalized_api_base_url(base_url)?;
    let mut url = reqwest::Url::parse(&normalized).map_err(|_| "DeepSeek API 地址无效。")?;
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{path}/models"));
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
    QuestionRequired {
        #[serde(rename = "sessionId")]
        session_id: String,
        questions: Vec<ChatQuestion>,
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

#[derive(Serialize, Clone)]
struct ChatQuestionOption {
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Serialize, Clone)]
struct ChatQuestion {
    id: String,
    question: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<Vec<ChatQuestionOption>>,
    #[serde(rename = "multiSelect", skip_serializing_if = "Option::is_none")]
    multi_select: Option<bool>,
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
    QuestionRequired {
        #[serde(rename = "sessionId")]
        session_id: String,
        questions: Vec<BridgeQuestion>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeQuestionOption {
    label: String,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeQuestion {
    id: String,
    question: String,
    detail: Option<String>,
    header: Option<String>,
    options: Option<Vec<BridgeQuestionOption>>,
    #[serde(rename = "multiSelect")]
    multi_select: Option<bool>,
}

const MAX_HARNESS_EVENT_TEXT_BYTES: usize = MAX_HARNESS_MESSAGE_BYTES;
const MAX_HARNESS_EVENT_IDENTIFIER_BYTES: usize = 200;
const MAX_HARNESS_EVENT_PROVIDER_BYTES: usize = 200;
const MAX_HARNESS_EVENT_SUMMARY_BYTES: usize = 500;
const MAX_HARNESS_EVENT_QUESTION_COUNT: usize = 8;
const MAX_HARNESS_EVENT_OPTION_COUNT: usize = 12;
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
        BridgeEvent::QuestionRequired {
            session_id,
            questions,
        } if session_id == expected_session_id
            && !questions.is_empty()
            && questions.len() <= MAX_HARNESS_EVENT_QUESTION_COUNT =>
        {
            let mut safe_questions = Vec::with_capacity(questions.len());
            for question in questions {
                let id = bounded_bridge_text(question.id, MAX_HARNESS_EVENT_IDENTIFIER_BYTES)?;
                let text = bounded_bridge_text(question.question, MAX_HARNESS_EVENT_SUMMARY_BYTES)?;
                let detail = match question.detail {
                    Some(value) => {
                        Some(bounded_bridge_text(value, MAX_HARNESS_EVENT_SUMMARY_BYTES)?)
                    }
                    None => None,
                };
                let header = match question.header {
                    Some(value) => Some(bounded_bridge_text(
                        value,
                        MAX_HARNESS_EVENT_IDENTIFIER_BYTES,
                    )?),
                    None => None,
                };
                let options = match question.options {
                    Some(options) if options.len() <= MAX_HARNESS_EVENT_OPTION_COUNT => {
                        let mut safe_options = Vec::with_capacity(options.len());
                        for option in options {
                            safe_options.push(ChatQuestionOption {
                                label: bounded_bridge_text(
                                    option.label,
                                    MAX_HARNESS_EVENT_SUMMARY_BYTES,
                                )?,
                                description: match option.description {
                                    Some(value) => Some(bounded_bridge_text(
                                        value,
                                        MAX_HARNESS_EVENT_SUMMARY_BYTES,
                                    )?),
                                    None => None,
                                },
                            });
                        }
                        Some(safe_options)
                    }
                    Some(_) => return None,
                    None => None,
                };
                safe_questions.push(ChatQuestion {
                    id,
                    question: text,
                    detail,
                    header,
                    options,
                    multi_select: question.multi_select,
                });
            }
            Some(ChatEvent::QuestionRequired {
                session_id: expected_session_id.into(),
                questions: safe_questions,
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

/// A normal DSH turn can close its SSE response after publishing the final
/// assistant message or `status: done`. Treat that EOF as a clean completion;
/// only a stream that ends before either terminal marker is a disconnect.
fn harness_records_have_terminal_event(records: &[String], session_id: &str) -> bool {
    records
        .iter()
        .filter_map(|record| {
            sse_record_payload(record).and_then(|line| parse_bridge_event(&line, session_id))
        })
        .any(|event| match event {
            ChatEvent::Message { role, .. } => role == "assistant",
            ChatEvent::Status { activity } => activity == "done",
            _ => false,
        })
}

/// The Bridge SSE endpoint is expected to stay open, but some DSH/WebServer
/// combinations close a response after a completed turn.  A clean idle
/// snapshot is also not an error.  Keep these cases distinguishable from a
/// stream that drops while a turn is still running so the native reader can
/// reconnect without leaving the composer stuck in `thinking`.
fn harness_records_have_turn_activity(records: &[String], session_id: &str) -> bool {
    records
        .iter()
        .filter_map(|record| {
            sse_record_payload(record).and_then(|line| parse_bridge_event(&line, session_id))
        })
        .any(|event| match event {
            ChatEvent::Status { activity } => !matches!(activity.as_str(), "idle" | "done"),
            ChatEvent::Delta { .. } | ChatEvent::Message { .. } => true,
            _ => false,
        })
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

/// The Bridge's history envelope.
///
/// Deliberately **not** `deny_unknown_fields`, and that is a bug fix rather than a
/// relaxation. The Bridge answers with `sessionId`, `messages`, `truncated` and
/// `limits`; rejecting the two fields this struct does not name made the whole
/// deserialization fail, so *every* history read errored and the wallpaper showed
/// an empty transcript while DSH held a full one. Adding a field to a response is
/// backward-compatible exactly when the client ignores what it does not model, so
/// a newer Bridge must not be able to break an older wallpaper this way.
///
/// `truncated` is modelled because it is information the reader needs: the Bridge
/// caps how much history it returns, and without this the UI cannot tell "this is
/// the whole conversation" from "older turns were dropped".
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HarnessHistoryResponse {
    session_id: String,
    messages: Vec<HarnessHistoryMessage>,
    #[serde(default)]
    truncated: bool,
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
        // Surfaced so the reader can tell a whole conversation from a capped one.
        // The Bridge bounds how much history it returns; silently presenting a
        // truncated transcript as complete is the failure this prevents.
        "truncated": history.truncated,
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
    // Record the transcript this turn belongs to, so a listing can mark it and
    // so trimming protects it.
    state.set_active_api_conversation(&conversation_id);
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
    let mut idle_timed_out = false;
    loop {
        // Every event restarts the idle deadline; cancellation is polled with
        // priority so a user stop is never reported as a timeout.
        let chunk = match next_with_idle_timeout(&mut stream, &mut cancel_rx, API_STREAM_IDLE_TIMEOUT).await {
            StreamRead::Cancelled => {
                canceled = true;
                break;
            }
            StreamRead::Idle => {
                idle_timed_out = true;
                break;
            }
            StreamRead::Event(chunk) => chunk,
        };
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
    if idle_timed_out {
        stream_error = Some(api_idle_timeout_error());
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
            // Record activity for eviction order, never for display: messages
            // keep their original ids and timestamps.
            conversation.updated_at = conversation
                .messages
                .last()
                .map(|message| message.created_at)
                .unwrap_or_else(unix_millis)
                .max(conversation.updated_at);
        }
        // The transcript the user is in is the last candidate for trimming.
        state.persist_api_conversations_for(Some(&conversation_id))
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
                // A stalled stream gets a stable, distinct code so the renderer
                // can explain "no data for N seconds" instead of reporting a
                // generic transport failure.
                code: if idle_timed_out {
                    API_STREAM_IDLE_TIMEOUT_CODE.into()
                } else {
                    "DEEPSEEK_API_STREAM".into()
                },
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

/// Default and maximum number of messages returned to the renderer in one
/// `api_history` call. The archive can hold far more than any composer can
/// display, and cloning a multi-megabyte transcript across IPC on every resume
/// is what this bound exists to prevent.
pub const DEFAULT_API_HISTORY_MESSAGES: usize = 200;
pub const MAX_API_HISTORY_MESSAGES: usize = 1000;

/// Read a bounded window of one API transcript.
///
/// `limit` counts from the newest message backwards, so the composer always
/// receives the most recent context first and the extra bytes cross IPC only
/// when the user explicitly asks for earlier turns.
pub fn api_history(
    state: &ChatState,
    conversation_id: &str,
    limit: Option<usize>,
) -> Result<Value, String> {
    let conversations = state
        .api_conversations
        .lock()
        .map_err(|_| "API conversation state poisoned")?;
    let stored = conversations
        .get(conversation_id)
        .map(|conversation| conversation.messages.as_slice())
        .unwrap_or(&[]);
    let requested = limit
        .unwrap_or(DEFAULT_API_HISTORY_MESSAGES)
        .clamp(1, MAX_API_HISTORY_MESSAGES);
    let returned = requested.min(stored.len());
    let start = stored.len() - returned;
    let messages = stored[start..].to_vec();
    let bytes = EncryptedJsonStore::serialized_len(&messages).unwrap_or(0);
    Ok(serde_json::json!({
        "messages": messages,
        "totalMessages": stored.len(),
        "hasMore": start > 0,
        "bytes": bytes,
        "limit": requested,
    }))
}

/// Delete one durable API transcript. Returns `true` when it existed.
pub fn delete_api_conversation(state: &ChatState, conversation_id: &str) -> Result<bool, String> {
    let trimmed = conversation_id.trim();
    if trimmed.is_empty() {
        return Err("会话标识无效".into());
    }
    state.delete_api_conversation(trimmed)
}

/// Delete every durable API transcript this process can see.
pub fn clear_api_history(state: &ChatState) -> Result<usize, String> {
    state.clear_api_conversations()
}

/// Metadata for one durable API transcript. Message bodies are deliberately
/// absent: the settings center needs to identify and delete a transcript, not
/// to read it, so a listing can never become a second transcript exposure path.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiConversationSummary {
    id: String,
    message_count: usize,
    /// Serialized size of this transcript — the number the trim budget acts on.
    bytes: usize,
    updated_at: u64,
    first_message_at: u64,
    last_message_at: u64,
    /// True for the transcript this process is currently reading.
    active: bool,
}

/// List the durable API transcripts, most recent activity first.
///
/// This reads the archive through the store instead of reporting
/// `api_conversations`: the settings WebView is a separate process with its own
/// state instance, so its in-memory map is a snapshot from its own start and a
/// transcript deleted moments ago would still appear there.
pub fn list_api_conversations(state: &ChatState) -> Result<Value, String> {
    let stored = state.load_api_archive()?;
    let active = state
        .active_api_conversation
        .lock()
        .ok()
        .and_then(|active| active.clone());
    let mut summaries: Vec<ApiConversationSummary> = stored
        .iter()
        .map(|(id, conversation)| ApiConversationSummary {
            id: id.clone(),
            message_count: conversation.messages.len(),
            bytes: EncryptedJsonStore::serialized_len(conversation).unwrap_or(0),
            updated_at: conversation.effective_updated_at(),
            first_message_at: conversation
                .messages
                .first()
                .map(|message| message.created_at)
                .unwrap_or(0),
            last_message_at: conversation
                .messages
                .last()
                .map(|message| message.created_at)
                .unwrap_or(0),
            active: active.as_deref() == Some(id.as_str()),
        })
        .collect();
    summaries.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let total_bytes: usize = summaries.iter().map(|summary| summary.bytes).sum();
    let total_messages: usize = summaries.iter().map(|summary| summary.message_count).sum();
    Ok(serde_json::json!({
        "conversations": summaries,
        "totalBytes": total_bytes,
        "totalMessages": total_messages,
        "budgetBytes": API_CONVERSATION_TARGET_PLAINTEXT_BYTES,
        "maxBytes": crate::api_persistence::MAX_PLAINTEXT_BYTES,
    }))
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
    endpoint_port: Option<u16>,
) -> Result<String, String> {
    let connection_id = connection_id.trim().to_string();
    if connection_id.is_empty() || connection_id.len() > 200 {
        return Err("Harness 连接标识无效".into());
    }
    // The endpoint to attempt. Deliberately *not* recorded on the session yet:
    // a failed connect must leave an existing session's endpoint untouched, or a
    // failed attempt at a new client would silently repoint the old conversation
    // at a client that never answered.
    let port = endpoint_port.unwrap_or_else(|| state.harness_port());
    // 这一行是这几轮最缺的东西：连接尝试**必须留下痕迹**。此前日志里只有"启动/门票"那类，
    // 于是"切换主体后到底连了哪个端口"只能靠猜 —— 而真正的原因恰恰是：不带端点参数时，原生会
    // 退回"这条会话当初钉下的端口"，那个端口属于**上一个**主体。
    log::info!(
        "harness connect: requested_port={:?} resolved_port={port} pinned_port={} resume={} model={}",
        endpoint_port,
        state.harness_port(),
        resume_session_id.is_some(),
        model.as_deref().unwrap_or("-")
    );
    // **先建新的，成功之后才拆旧的**（make-before-break）。这里原来是直接 `cancel_harness_stream`：
    // 于是切换主体失败时，用户连原本能用的那条连接也一起丢了 —— 而紧挨着的这段注释早就承诺过
    // "失败的连接不得改动已有会话的端点"，只是事件流那一半没做到。取消挪到新会话建立之后。
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let model = model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned);
    if model
        .as_ref()
        .is_some_and(|model| !valid_bridge_status(model))
    {
        return Err("Harness 模型标识无效".into());
    }
    let create_session = |resume_session_id: Option<&str>| {
        auth(
            client.post(harness_url(port, "/sessions")),
            &token,
        )
        .json(&HarnessSessionRequest {
            resume_session_id,
            model: model.as_deref(),
        })
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
            return Err(harness_http_error(reqwest::StatusCode::CONFLICT, "sessions"));
        }
    }
    if !response.status().is_success() {
        // `sessions` is the route the Bridge must have registered for the
        // capability it advertised, so a 404 here is a version/installation
        // problem rather than a transient connection failure.
        log::warn!("harness connect failed: port={port} status={}", response.status());
        return Err(harness_http_error(response.status(), "sessions"));
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
    // Only now that a session exists on this endpoint does the session adopt it,
    // so every later request for this conversation goes to the client that
    // actually answered.
    state.set_harness_port(port);
    // 新会话已经建立，现在才放弃旧的：这一步之后"当前连接"才真正换人。切换失败时根本走不到这里，
    // 旧的事件流与会话原样留着（用户要的正是这个：没连上就继续持有旧的，但界面按未接入处理）。
    cancel_harness_stream(&state);
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
    let port = state.harness_port();
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
    let url = harness_url(port, &format!("/sessions/{}/events", urlencoding::encode(&session_id)));
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
    let response = match auth(client.get(&url), &token).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            finish_harness_stream(&state, stream_id, &session_id);
            return Err(harness_http_error(response.status(), "events"));
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

        // Keep one cancellation slot for the lifetime of the adapter.  A
        // completed SSE response must not tear down that slot: reconnecting
        // here is what lets a second turn reach the same desktop composer.
        let mut response = response;
        'reconnect: loop {
            let mut stream = response.bytes_stream();
            let mut decoder = Utf8StreamDecoder::default();
            let mut buffer = String::new();
            let mut turn_completed = false;
            let mut turn_started = false;
            let clean_eof: bool;

            loop {
                // The bridge heartbeats every 15s. A stream that produces
                // nothing for a whole minute is dead, and without this deadline
                // the reader would sit here until the 24-hour ceiling while the
                // composer waits for a turn that can never arrive.
                let chunk = match next_with_idle_timeout(
                    &mut stream,
                    &mut cancel_rx,
                    HARNESS_STREAM_IDLE_TIMEOUT,
                )
                .await
                {
                    StreamRead::Cancelled => break 'reconnect,
                    StreamRead::Idle => {
                        emit_stream_error(
                            "HARNESS_SSE_IDLE_TIMEOUT",
                            format!(
                                "DSH bridge 已 {} 秒没有心跳或事件，事件流已安全断开。",
                                HARNESS_STREAM_IDLE_TIMEOUT.as_secs()
                            ),
                        );
                        break 'reconnect;
                    }
                    StreamRead::Event(chunk) => chunk,
                };
                let Some(chunk) = chunk else {
                    let tail = match decoder.finish() {
                        Ok(tail) => tail,
                        Err(_) => {
                            emit_stream_error("HARNESS_SSE_ENCODING", generic_harness_error("事件流编码"));
                            break 'reconnect;
                        }
                    };
                    let records = match drain_bounded_harness_sse_records(&mut buffer, &tail) {
                        Ok(records) => records,
                        Err(()) => {
                            emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                            break 'reconnect;
                        }
                    };
                    turn_started |= harness_records_have_turn_activity(&records, &session_id);
                    turn_completed |= harness_records_have_terminal_event(&records, &session_id);
                    forward_harness_sse_records(&app, &state, stream_id, &session_id, &connection_id, records);
                    let records = match finish_bounded_harness_sse_records(&mut buffer) {
                        Ok(records) => records,
                        Err(()) => {
                            emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                            break 'reconnect;
                        }
                    };
                    turn_started |= harness_records_have_turn_activity(&records, &session_id);
                    turn_completed |= harness_records_have_terminal_event(&records, &session_id);
                    forward_harness_sse_records(&app, &state, stream_id, &session_id, &connection_id, records);
                    clean_eof = !turn_started || turn_completed;
                    break;
                };
                match chunk {
                    Ok(bytes) => {
                        if bytes.len() > MAX_HARNESS_SSE_CHUNK_BYTES {
                            emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                            break 'reconnect;
                        }
                        let decoded = match decoder.push(&bytes) {
                            Ok(decoded) => decoded,
                            Err(_) => { emit_stream_error("HARNESS_SSE_ENCODING", generic_harness_error("事件流编码")); break 'reconnect; }
                        };
                        let records = match drain_bounded_harness_sse_records(&mut buffer, &decoded) {
                            Ok(records) => records,
                            Err(()) => {
                                emit_stream_error("HARNESS_SSE_LIMIT", "DSH bridge 返回的流式事件过大，已安全停止接收。".into());
                                break 'reconnect;
                            }
                        };
                        turn_started |= harness_records_have_turn_activity(&records, &session_id);
                        turn_completed |= harness_records_have_terminal_event(&records, &session_id);
                        forward_harness_sse_records(&app, &state, stream_id, &session_id, &connection_id, records);
                    }
                    Err(_) if turn_completed || !turn_started => {
                        clean_eof = true;
                        break;
                    }
                    Err(_) => {
                        // A transport read error can happen after the
                        // peer has already committed the assistant
                        // message. Treat it like a reconnectable EOF;
                        // the history reconciler will fill any missed
                        // terminal event without flashing a false
                        // "bridge offline" error in the UI.
                        clean_eof = true;
                        break;
                    }
                }
            }

            if !clean_eof {
                emit_stream_error("HARNESS_DISCONNECTED", "DSH bridge 事件流已断开".into());
                break;
            }

            // Do not spin if the host closes an idle response.  The short
            // delay also gives DSH time to re-register the next subscriber.
            // `cancel_rx` is drained in this select, so a Stop that arrives
            // between two connections ends the reader instead of being lost in
            // the HTTP re-request below.
            let mut reconnect_delay = Box::pin(tokio::time::sleep(Duration::from_millis(20)));
            tokio::select! {
                biased;
                _ = &mut cancel_rx => break 'reconnect,
                _ = &mut reconnect_delay => {}
            }
            // The re-request itself could hang on a wedged loopback socket, so
            // it gets the same idle deadline.
            let mut reconnect_request = Box::pin(auth(client.get(&url), &token).send());
            let next = tokio::select! {
                biased;
                _ = &mut cancel_rx => break 'reconnect,
                _ = tokio::time::sleep(HARNESS_STREAM_IDLE_TIMEOUT) => {
                    emit_stream_error(
                        "HARNESS_SSE_IDLE_TIMEOUT",
                        format!(
                            "DSH bridge 在 {} 秒内没有接受新的心跳连接；事件流已停止。",
                            HARNESS_STREAM_IDLE_TIMEOUT.as_secs()
                        ),
                    );
                    break 'reconnect;
                }
                result = &mut reconnect_request => result,
            };
            let next = match next {
                Ok(next) if reconnect_response_is_ready(&next) => next,
                Ok(next) => {
                    emit_stream_error(
                        "HARNESS_DISCONNECTED",
                        // Use the same specific mapping as the first connect, so a
                        // reconnect that fails with a 404 on the event stream names
                        // the stale-Bridge cause instead of the generic wording.
                        harness_http_error(next.status(), "events"),
                    );
                    break;
                }
                Err(_) => {
                    emit_stream_error("HARNESS_DISCONNECTED", generic_harness_error("事件流重连"));
                    break;
                }
            };
            response = next;
        }
        finish_harness_stream(&state, stream_id, &session_id);
    });
    Ok(())
}

pub async fn harness_send(state: tauri::State<'_, ChatState>, text: String) -> Result<(), String> {
    let port = state.harness_port();
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
        .ok_or_else(|| format!("{HARNESS_NO_SESSION}: Harness 会话尚未建立"))?;
    let token = read_bridge_token()?;
    let url = harness_url(port, &format!("/sessions/{}/messages", urlencoding::encode(&session_id)));
    let client = bridge_request_client()?;
    let response = auth(client.post(url), &token)
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .map_err(|_| "发送到 DSH bridge 失败；请确认 Harness 仍在运行。".to_string())?;
    if response.status() == reqwest::StatusCode::CONFLICT {
        // 用户在桌面端把这条会话**归档**掉之后，它在我们缓存里还在，但桥已经不再往它里面写东西了：
        // 桥会回 `session-archived` 并释放句柄。这不是"发送失败"，而是"这条会话没了"。
        //
        // 这里刻意**不**自己重连：重连要重新建立 SSE 事件流，而那条流是渲染端按连接生命周期建的
        // ——由它接手才不会出现"有会话、没事件流"。所以只把缓存里的会话清掉，并回一个**可识别**的
        // 错误：渲染端看到 `HARNESS_SESSION_ARCHIVED` 就会重连并把这句重发一次。
        let archived = bounded_bridge_json::<BridgeErrorResponse>(
            response,
            MAX_HARNESS_SESSION_RESPONSE_BYTES,
            "DSH bridge 返回了无法识别的会话响应。",
        )
        .await
        .ok()
        .and_then(|body| body.error)
        .as_deref()
            == Some("session-archived");
        if archived {
            clear_harness_session_if(&state, &session_id);
            return Err(format!(
                "{HARNESS_SESSION_ARCHIVED}: 这条会话已在桌面端归档，桥不再接受它的消息。"
            ));
        }
        // 别的 409（例如"另一台 DSH 占用"）保持原来的说法，不要冒充归档。
        return Err(harness_http_error(reqwest::StatusCode::CONFLICT, "messages"));
    }
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "messages"));
    }
    Ok(())
}

pub async fn harness_history(state: tauri::State<'_, ChatState>) -> Result<Value, String> {
    let port = state.harness_port();
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let url = harness_url(port, &format!("/sessions/{}/history", urlencoding::encode(&session_id)));
    let client = bridge_request_client()?;
    let response = auth(client.get(url), &token)
        .send()
        .await
        .map_err(|_| generic_harness_error("历史读取"))?;
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "history"));
    }
    // The envelope is read tolerantly on purpose: the Bridge adds fields over time
    // (`truncated`, `limits`) and a client that rejects what it does not model
    // breaks every read the moment the host is upgraded. The per-message shape
    // stays strict, because those fields are shown verbatim.
    let history = bounded_bridge_json::<HarnessHistoryResponse>(
        response,
        MAX_HARNESS_HISTORY_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的历史记录。",
    )
    .await?;
    parse_harness_history(&session_id, history)
}

pub async fn harness_presets(state: tauri::State<'_, ChatState>) -> Result<Value, String> {
    // Presets are a host-wide query, so they must go to the same host the session
    // is on. Reading the session's endpoint rather than a constant is what makes
    // that true: this used to always query DSH's default port, so selecting the
    // official desktop shell showed its sessions' presets from the wrong client.
    let port = state.harness_port();
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let response = auth(
        client.get(harness_url(harness_query_port(&state), "/control/presets")),
        &token,
    )
    .send()
    .await
    .map_err(|_| generic_harness_error("模式目录读取"))?;
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "presets"));
    }
    bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的模式目录。",
    )
    .await
}

/// Enumerate the models the connected Harness host can run right now.
///
/// The catalog is host-owned: the Bridge asks the host's LLM seam and reports
/// `supported: false` when that seam is absent, so the wallpaper can tell
/// "this host cannot be asked" apart from "this host offers nothing".
/// The endpoint is read from the *session*, exactly like `harness_presets`, so
/// the answer describes the subject actually in use (检出 / 桌面 / 官壳).
/// 该向哪台宿主问"与本次会话无关"的问题（模式目录、模型目录）。
///
/// `harness_port()` 只在**建立会话时**被设置，没有会话时会退回默认端口（3080）——那里没有
/// 桥接。于是"刚打开壁纸、还没开始对话"时，模型目录和模式目录一律读不到（实测：前端提示
/// "DSH bridge 模型目录读取失败"，原生日志里连一行请求记录都没有，因为连接直接被拒绝）。
/// 这种情况下要用**发现到的端点**：它才是这台机器上正在运行的那台宿主。
fn harness_query_port(state: &ChatState) -> u16 {
    let live_session = state
        .harness_session
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    if live_session {
        return state.harness_port();
    }
    #[cfg(not(feature = "lite"))]
    {
        crate::harness_endpoint_port()
    }
    #[cfg(feature = "lite")]
    {
        DEFAULT_HARNESS_PORT
    }
}

/// 模型目录的摘要，只用于日志：前端"下拉里没有选项"时，第一个要看的就是枚举到底拿到了什么。
fn summarise_model_payload(value: &Value) -> String {
    let supported = value.get("supported").and_then(Value::as_bool);
    let provider = value.get("provider").and_then(Value::as_str);
    let current = value
        .get("current")
        .and_then(|current| current.get("model"))
        .and_then(Value::as_str);
    let models = value.get("models").and_then(Value::as_array);
    let ids = models
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    format!(
        "supported={:?} provider={:?} current={:?} count={} ids=[{}]",
        supported,
        provider,
        current,
        models.map(Vec::len).unwrap_or(0),
        ids
    )
}

pub async fn harness_models(state: tauri::State<'_, ChatState>) -> Result<Value, String> {
    let port = harness_query_port(&state);
    // 这三步原先都是哑的：令牌读不到、客户端建不起来、连接被拒绝，前端只会看到同一句
    // "模型目录读取失败"，而日志里一行都没有——"命令没被调用"和"调用了但请求没发出去"
    // 无法区分。每一步都留痕。
    log::info!("harness models: querying port {port}");
    let token = match read_bridge_token() {
        Ok(token) => token,
        Err(error) => {
            log::warn!("harness models: bridge token unavailable: {error}");
            return Err(error);
        }
    };
    let client = match bridge_request_client() {
        Ok(client) => client,
        Err(error) => {
            log::warn!("harness models: request client unavailable: {error}");
            return Err(error);
        }
    };
    let response = match auth(client.get(harness_url(port, "/control/models")), &token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            log::warn!("harness models: request to port {port} failed: {error}");
            return Err(generic_harness_error("模型目录读取"));
        }
    };
    if !response.status().is_success() {
        let status = response.status().as_u16();
        log::info!("harness models: HTTP {status}");
        return Err(harness_http_error(response.status(), "models"));
    }
    let payload = bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的模型目录。",
    )
    .await?;
    log::info!(
        "harness models (port {port}): {}",
        summarise_model_payload(&payload)
    );
    Ok(payload)
}

/// 把壁纸选定的模型推给宿主，让宿主的默认模型跟着走。
///
/// 同步方向是"壁纸端为源"：这样从 DSH 界面开的新会话也用同一个模型，宿主重启后依然如此
/// （宿主把选择存进自己的设置）。宿主不具备持久化能力时桥接回 501，这里如实报错——壁纸
/// 自己那次会话仍然用选定模型，所以这只是"同步没做成"，不是聊天不可用。
pub async fn harness_set_model(
    state: tauri::State<'_, ChatState>,
    model: String,
) -> Result<Value, String> {
    let model = model.trim().to_owned();
    if model.is_empty() {
        return Err("模型不能为空。".into());
    }
    let port = harness_query_port(&state);
    let token = match read_bridge_token() {
        Ok(token) => token,
        Err(error) => {
            log::warn!("harness set model: bridge token unavailable: {error}");
            return Err(error);
        }
    };
    let client = bridge_request_client()?;
    let response = match auth(client.post(harness_url(port, "/control/models")), &token)
        .json(&serde_json::json!({ "model": model }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            log::warn!("harness set model: request to port {port} failed: {error}");
            return Err(generic_harness_error("模型同步"));
        }
    };
    let status = response.status();
    if !status.is_success() {
        // 501 是宿主不支持同步（没有 saveSelection），与"请求失败"区分开。
        log::warn!("harness set model: HTTP {} for {model}", status.as_u16());
        return Err(harness_http_error(status, "model"));
    }
    let payload = bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的模型同步结果。",
    )
    .await?;
    log::info!("harness set model (port {port}): {model} -> {payload}");
    Ok(payload)
}

/// Enumerate the models the configured DeepSeek API endpoint offers.
///
/// `/models` is part of the compatible API surface, so the list is the
/// endpoint's own answer rather than a hardcoded pair of ids. A gateway that
/// does not implement the route is reported as unsupported (not as a failure):
/// the picker then keeps showing the configured model alone.
pub async fn api_models(base_url: String) -> Result<Value, String> {
    let url = api_models_url(&base_url)?;
    let key_entry = keyring::Entry::new("dsh-wallpaper", "deepseek-api")
        .map_err(|_| "无法访问 Windows 凭据管理器。请检查系统凭据服务后重试。".to_string())?;
    let key = key_entry
        .get_password()
        .map_err(|_| "未在 Windows 凭据管理器配置 API Key".to_string())?;
    let client = api_stream_client()?;
    let response = client
        .get(url)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| generic_api_error("连接"))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED {
        return Ok(serde_json::json!({ "supported": false, "models": [] }));
    }
    if !status.is_success() {
        return Err(format!(
            "DeepSeek API 拒绝读取模型列表（HTTP {}）。请检查访问密钥与账户状态后重试。",
            status.as_u16()
        ));
    }
    let payload = bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DeepSeek API 返回了无法识别的模型列表。",
    )
    .await?;
    // OpenAI-compatible endpoints answer `{ data: [{ id, ... }] }`; anything else
    // is reported as unsupported rather than guessed at.
    let models = payload
        .get("data")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("id").and_then(Value::as_str)?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    let name = entry
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or(id);
                    Some(serde_json::json!({ "id": id, "name": name }))
                })
                .collect::<Vec<_>>()
        });
    let Some(models) = models else {
        log::info!("api models: endpoint returned no `data` array; reporting unsupported");
        return Ok(serde_json::json!({ "supported": false, "models": [] }));
    };
    let payload = serde_json::json!({ "supported": true, "models": models });
    log::info!("api models: {}", summarise_model_payload(&payload));
    Ok(payload)
}

pub async fn harness_set_preset(
    state: tauri::State<'_, ChatState>,
    preset: String,
) -> Result<Value, String> {
    let port = state.harness_port();
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let url = harness_url(port, &format!("/control/sessions/{}/preset", urlencoding::encode(&session_id)));
    let response = auth(client.post(url), &token)
        .json(&serde_json::json!({ "agentPreset": preset }))
        .send()
        .await
        .map_err(|_| generic_harness_error("模式切换"))?;
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "preset"));
    }
    bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的模式响应。",
    )
    .await
}

pub async fn harness_controls(state: tauri::State<'_, ChatState>) -> Result<Value, String> {
    let port = state.harness_port();
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let url = harness_url(port, &format!("/control/sessions/{}", urlencoding::encode(&session_id)));
    let response = auth(client.get(url), &token)
        .send()
        .await
        .map_err(|_| generic_harness_error("控制目录读取"))?;
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "controls"));
    }
    bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的控制目录。",
    )
    .await
}

pub async fn harness_set_permission(
    state: tauri::State<'_, ChatState>,
    permission: String,
) -> Result<Value, String> {
    let port = state.harness_port();
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let client = bridge_request_client()?;
    let url = harness_url(port, &format!("/control/sessions/{}/permission", urlencoding::encode(&session_id)));
    let response = auth(client.post(url), &token)
        .json(&serde_json::json!({ "permission": permission }))
        .send()
        .await
        .map_err(|_| generic_harness_error("权限切换"))?;
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "permission"));
    }
    bounded_bridge_json::<Value>(
        response,
        MAX_HARNESS_SESSION_RESPONSE_BYTES,
        "DSH bridge 返回了无法识别的权限响应。",
    )
    .await
}

pub async fn harness_cancel(state: tauri::State<'_, ChatState>) -> Result<(), String> {
    let session_id = state
        .harness_session
        .lock()
        .map_err(|_| "Harness session state poisoned")?
        .clone()
        .ok_or("Harness 会话尚未建立")?;
    let token = read_bridge_token()?;
    let port = state.harness_port();
    let url = harness_url(port, &format!("/sessions/{}/cancel", urlencoding::encode(&session_id)));
    let client = bridge_request_client()?;
    let response = auth(client.post(url), &token)
        .send()
        .await
        .map_err(|_| generic_harness_error("取消请求"))?;
    if !response.status().is_success() {
        return Err(harness_http_error(response.status(), "cancel"));
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
mod stream_watchdog_tests {
    use super::{
        api_idle_timeout_error, next_with_idle_timeout, StreamRead, API_STREAM_IDLE_TIMEOUT,
        API_STREAM_IDLE_TIMEOUT_CODE, HARNESS_STREAM_IDLE_TIMEOUT,
    };
    use std::collections::VecDeque;
    use std::time::Duration;
    use tokio::sync::oneshot;

    /// A test-only live stream: it waits `delay` before every item and then
    /// yields it, so a test can prove the idle deadline is per-read and not a
    /// total lifetime. A delay of `None` means "never produce another item".
    fn scripted_stream(delays: Vec<Option<Duration>>) -> impl futures_util::Stream<Item = u8> + Unpin {
        let mut queue: VecDeque<Option<Duration>> = delays.into();
        Box::pin(futures_util::stream::unfold(0u8, move |count| {
            let delay = queue.pop_front();
            async move {
                match delay {
                    Some(Some(duration)) => {
                        tokio::time::sleep(duration).await;
                        Some((count, count + 1))
                    }
                    // Never yields: models a peer that stopped sending bytes.
                    Some(None) => {
                        std::future::pending::<()>().await;
                        unreachable!()
                    }
                    None => None,
                }
            }
        }))
    }

    #[tokio::test]
    async fn an_idle_read_reports_a_timeout_instead_of_waiting_forever() {
        let mut stream = scripted_stream(vec![None]);
        let (_cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        let outcome =
            next_with_idle_timeout(&mut stream, &mut cancel_rx, Duration::from_millis(30)).await;
        assert!(matches!(outcome, StreamRead::Idle));
    }

    #[tokio::test]
    async fn every_event_restarts_the_idle_deadline() {
        // Three items arriving once per 20ms keep a 60ms deadline from ever
        // firing, which is what stops a slow-but-live model being killed.
        let mut stream = scripted_stream(vec![
            Some(Duration::from_millis(20)),
            Some(Duration::from_millis(20)),
            Some(Duration::from_millis(20)),
        ]);
        let (_cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        for expected in 0..3u8 {
            let outcome =
                next_with_idle_timeout(&mut stream, &mut cancel_rx, Duration::from_millis(60)).await;
            match outcome {
                StreamRead::Event(Some(item)) => assert_eq!(item, expected),
                _ => panic!("event {expected} was not delivered"),
            }
        }
    }

    #[tokio::test]
    async fn end_of_stream_is_not_reported_as_an_idle_timeout() {
        let mut stream = scripted_stream(vec![]);
        let (_cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        let outcome =
            next_with_idle_timeout(&mut stream, &mut cancel_rx, Duration::from_millis(50)).await;
        assert!(matches!(outcome, StreamRead::Event(None)));
    }

    #[tokio::test]
    async fn cancellation_wins_over_an_elapsed_idle_deadline() {
        // The deadline has already passed and no data will ever arrive. A stop
        // arriving at the same moment must be reported as cancellation, so
        // cleanup happens exactly once through the cancel path.
        let mut stream = scripted_stream(vec![None]);
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        let _ = cancel_tx.send(());
        tokio::time::sleep(Duration::from_millis(20)).await;
        let outcome =
            next_with_idle_timeout(&mut stream, &mut cancel_rx, Duration::from_millis(10)).await;
        assert!(matches!(outcome, StreamRead::Cancelled));
    }

    #[test]
    fn idle_deadlines_match_the_documented_contract() {
        // API: no data for two minutes. Harness: the bridge heartbeats every
        // 15s, so a full minute covers three missed beats.
        assert_eq!(API_STREAM_IDLE_TIMEOUT, Duration::from_secs(120));
        assert_eq!(HARNESS_STREAM_IDLE_TIMEOUT, Duration::from_secs(60));
        assert!(HARNESS_STREAM_IDLE_TIMEOUT < API_STREAM_IDLE_TIMEOUT);
        assert_eq!(API_STREAM_IDLE_TIMEOUT_CODE, "DEEPSEEK_API_IDLE_TIMEOUT");
        assert!(api_idle_timeout_error().contains("120"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        api_completion_url, api_history, api_request_messages, bridge_token_acl_is_private,
        drain_bounded_harness_sse_records, drain_sse_records, finish_api_request,
        finish_bounded_harness_sse_records, finish_harness_stream, finish_sse_records,
        harness_http_error, harness_records_have_terminal_event, harness_records_have_turn_activity,
        harness_url, is_current_api_request, is_current_harness_stream, owns_api_request, parse_bridge_event, reconnect_is_accepted,
        parse_harness_connection, parse_harness_history, sse_record_payload, trim_api_archive,
        ApiConversation, ApiConversationArchive, ApiMessage, ApiPricing, ApiUsage, ChatEvent,
        ChatState, HarnessConnection, HarnessHistoryMessage, HarnessHistoryResponse,
        HarnessStreamCancellation, Usage, Utf8StreamDecoder, API_CONVERSATION_SCHEMA_VERSION,
        DEFAULT_API_HISTORY_MESSAGES, DEFAULT_HARNESS_PORT, MAX_API_HISTORY_MESSAGES, MAX_API_MESSAGES_PER_CONVERSATION,
        MAX_API_RATE_PER_MILLION, MAX_API_REQUEST_CONTEXT_BYTES, MAX_HARNESS_EVENT_TEXT_BYTES,
        MAX_HARNESS_MESSAGE_BYTES, MAX_HARNESS_SSE_BUFFER_BYTES, MAX_HARNESS_SSE_EVENT_BYTES,
    };
    use crate::api_persistence::EncryptedJsonStore;
    use std::collections::HashMap;
    use tokio::sync::oneshot;

    /// Every harness request must carry the selected endpoint's port.
    ///
    /// This is the regression that matters for the three-client support: the
    /// wallpaper once built every harness URL from a hardcoded 3080, so a session
    /// opened against the official desktop shell (19387) would have sent its
    /// messages to whatever happened to be on the CLI's default port — or to
    /// nothing, reporting a misleading "bridge not running".
    #[test]
    fn harness_requests_carry_the_selected_endpoint() {
        assert_eq!(
            harness_url(19387, "/sessions"),
            "http://127.0.0.1:19387/api/wallpaper/v1/sessions"
        );
        assert_eq!(
            harness_url(43120, "/sessions/abc/events"),
            "http://127.0.0.1:43120/api/wallpaper/v1/sessions/abc/events"
        );
        // The default is DSH's own web port, so nothing changes for a CLI Host.
        assert_eq!(
            harness_url(DEFAULT_HARNESS_PORT, "/control/presets"),
            "http://127.0.0.1:3080/api/wallpaper/v1/control/presets"
        );
    }

    /// A session remembers the endpoint it was created against.
    #[test]
    fn a_session_keeps_the_endpoint_it_started_on() {
        let state = ChatState::default();
        // Nothing selected yet: DSH's default, which is what a CLI Host uses.
        assert_eq!(state.harness_port(), DEFAULT_HARNESS_PORT);
        state.set_harness_port(19387);
        assert_eq!(state.harness_port(), 19387);
        // Clones share the lock, so a detached SSE reader sees the same endpoint
        // as the command that created it.
        let reader = state.clone();
        state.set_harness_port(43120);
        assert_eq!(reader.harness_port(), 43120);
    }

    /// Attempting another endpoint must not repoint the open session.
    ///
    /// The regression this pins: the endpoint used to be recorded *before* the
    /// connect was known to succeed, so a failed attempt at a different client
    /// left the existing conversation addressing a client that never answered —
    /// every later message, history read and cancel would have gone there.
    ///
    /// Asserted through the state the command's ordering has to produce: reading
    /// the endpoint to attempt is separate from adopting it, so a session on 19387
    /// is still on 19387 after a failed attempt at 43120.
    #[test]
    fn a_failed_connect_does_not_repoint_the_session() {
        let state = ChatState::default();
        state.set_harness_port(19387);

        // What `harness_connect` does before the request: resolve, do not adopt.
        let attempted = Some(43120u16).unwrap_or_else(|| state.harness_port());
        assert_eq!(attempted, 43120, "the attempt targets the new endpoint");

        // The request fails here, so adoption never happens. The session must
        // still address the client it was created against.
        assert_eq!(
            state.harness_port(),
            19387,
            "a failed attempt must not repoint the open session"
        );

        // Adoption only follows a parsed session, and then it does take effect.
        state.set_harness_port(attempted);
        assert_eq!(state.harness_port(), 43120);
    }

    /// Plan §3 requires "SSE 断线重连" to be covered. The reconnect *loop* needs an    /// `AppHandle` and a live socket, but its acceptance rule does not, so the
    /// rule that decides whether a replacement stream is usable is pinned here.
    #[test]
    fn a_reconnect_is_only_accepted_when_it_is_ready() {
        // The Bridge acknowledges a subscription with this header. Without it a
        // 200 means "answered" but not "subscribed", and the reader would wait on
        // a response nobody writes to.
        assert!(reconnect_is_accepted(200, Some("1")));
        assert!(!reconnect_is_accepted(200, None));
        assert!(!reconnect_is_accepted(200, Some("0")));
        // A non-2xx is not a stream regardless of any header.
        for status in [301u16, 400, 401, 404, 409, 429, 500, 503] {
            assert!(!reconnect_is_accepted(status, Some("1")), "status {status}");
        }
        // Other 2xx responses are still success, matching `is_success()`.
        assert!(reconnect_is_accepted(204, Some("1")));
    }

    /// Each rejection must name the fix, and the 404 case must point at the
    /// installation rather than at the connection: it is the symptom of a stale
    /// or partially composed Bridge, which no in-composer action resolves.
    #[test]
    fn harness_http_errors_name_the_actual_cause() {
        use reqwest::StatusCode;
        let not_found_sessions = harness_http_error(StatusCode::NOT_FOUND, "sessions");
        assert!(not_found_sessions.contains("404"));
        assert!(not_found_sessions.contains("Bridge 版本过旧") || not_found_sessions.contains("更新 Bridge"));
        // A 404 on a different route must not claim the same remedy.
        assert_ne!(harness_http_error(StatusCode::NOT_FOUND, "history"), not_found_sessions);
        assert!(harness_http_error(StatusCode::UNAUTHORIZED, "sessions").contains("令牌"));
        assert!(harness_http_error(StatusCode::TOO_MANY_REQUESTS, "sessions").contains("并发上限"));
        assert!(harness_http_error(StatusCode::SERVICE_UNAVAILABLE, "sessions").contains("暂不可用"));
        assert!(harness_http_error(StatusCode::CONFLICT, "sessions").contains("冲突"));
        // An unmapped status keeps the generic wording rather than inventing one.
        assert!(harness_http_error(StatusCode::BAD_GATEWAY, "sessions").contains("请求被拒绝"));
        // No message may leak a token, a path, or an exception body.
        for status in [StatusCode::NOT_FOUND, StatusCode::UNAUTHORIZED, StatusCode::CONFLICT] {
            let message = harness_http_error(status, "sessions");
            assert!(!message.contains("Bearer"), "{message}");
            assert!(!message.contains(":\\"), "{message}");
        }
    }

    fn message(id: &str, created_at: u64, content: &str) -> ApiMessage {
        ApiMessage {
            id: id.into(),
            role: "assistant".into(),
            content: content.into(),
            created_at,
            usage: None,
        }
    }

    /// The exact plaintext size the archive would occupy on disk.
    fn archive_len(conversations: &HashMap<String, ApiConversation>) -> usize {
        EncryptedJsonStore::serialized_len(&ApiConversationArchive {
            schema_version: API_CONVERSATION_SCHEMA_VERSION,
            conversations: conversations.clone(),
        })
        .expect("serializable archive")
    }

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
                r#"{"type":"question-required","sessionId":"wallpaper-session","questions":[{"id":"choice","question":"继续吗？","options":[{"label":"继续"}]}]}"#,
                session_id,
            ),
            Some(ChatEvent::QuestionRequired { questions, .. }) if questions.len() == 1 && questions[0].question == "继续吗？"
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
            truncated: false,
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
                truncated: false,
                messages: vec![],
            },
        )
        .is_err());
        assert!(parse_harness_history(
            "wallpaper-session",
            HarnessHistoryResponse {
                session_id: "wallpaper-session".into(),
                truncated: false,
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
    fn normal_terminal_sse_records_do_not_count_as_disconnects() {
        let session = "wallpaper-session";
        assert!(harness_records_have_terminal_event(
            &[
                "data: {\"type\":\"message\",\"role\":\"assistant\",\"content\":\"完成\"}\n\n"
                    .into(),
            ],
            session,
        ));
        assert!(harness_records_have_terminal_event(
            &["data: {\"type\":\"status\",\"activity\":\"done\"}\n\n".into()],
            session,
        ));
        assert!(!harness_records_have_terminal_event(
            &["data: {\"type\":\"status\",\"activity\":\"streaming\"}\n\n".into()],
            session,
        ));
    }

    #[test]
    fn idle_sse_snapshot_is_reconnectable_but_active_turn_is_not_clean_eof() {
        let session = "wallpaper-session";
        assert!(!harness_records_have_turn_activity(
            &["data: {\"type\":\"status\",\"activity\":\"idle\"}\n\n".into()],
            session,
        ));
        assert!(harness_records_have_turn_activity(
            &["data: {\"type\":\"status\",\"activity\":\"streaming\"}\n\n".into()],
            session,
        ));
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

    /// A newer Bridge may add fields to the history envelope.
    ///
    /// The regression this pins: the envelope used `deny_unknown_fields` while the
    /// Bridge answered with `truncated` and `limits`, so deserialization failed for
    /// *every* read. The wallpaper then showed an empty transcript while DSH held a
    /// full one — the reported "past messages disappear after switching clients" —
    /// because the chat bootstrap aborts before it can publish any history.
    #[test]
    fn history_accepts_the_envelope_a_newer_bridge_sends() {
        // Exactly the shape the Bridge produces, including the field the client
        // does not model.
        let document = serde_json::json!({
            "sessionId": "wallpaper-2026-09-25",
            "truncated": false,
            "limits": { "maxMessages": 256, "maxBytes": 4 * 1024 * 1024 },
            "messages": [
                { "id": "c0799136-b1f9-4804-a088-f66bd328be48", "role": "user", "content": "你好？" },
                { "id": "366b919d-500a-4864-85a5-7ba151557509", "role": "assistant", "content": "我在。" },
            ],
        });
        let envelope: super::HarnessHistoryResponse =
            serde_json::from_value(document).expect("an added field must not break the read");
        let parsed = super::parse_harness_history("wallpaper-2026-09-25", envelope)
            .expect("a valid envelope parses");
        assert_eq!(parsed["messages"].as_array().map(Vec::len), Some(2));
        // The truncation flag reaches the caller instead of being dropped.
        assert_eq!(parsed["truncated"], serde_json::json!(false));
    }

    /// A capped read must be reportable, not silently presented as complete.
    #[test]
    fn history_surfaces_that_a_read_was_capped() {
        let document = serde_json::json!({
            "sessionId": "wallpaper-2026-09-25",
            "truncated": true,
            "limits": { "maxMessages": 256, "maxBytes": 4 * 1024 * 1024 },
            "messages": [
                { "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "role": "assistant", "content": "tail" },
            ],
        });
        let envelope: super::HarnessHistoryResponse = serde_json::from_value(document).expect("parses");
        let parsed = super::parse_harness_history("wallpaper-2026-09-25", envelope).expect("parses");
        assert_eq!(parsed["truncated"], serde_json::json!(true));
    }

    /// An absent `truncated` stays false, so an older Bridge still works.
    #[test]
    fn history_defaults_an_absent_truncation_flag() {
        let document = serde_json::json!({
            "sessionId": "wallpaper-2026-09-25",
            "messages": [],
        });
        let envelope: super::HarnessHistoryResponse = serde_json::from_value(document).expect("parses");
        assert!(!envelope.truncated);
    }

    /// Strictness stays where it protects the transcript.
    ///
    /// The envelope tolerates added fields; an individual message must not, because
    /// its fields are presented verbatim.
    #[test]
    fn an_unknown_message_field_is_still_rejected() {
        let document = serde_json::json!({
            "sessionId": "wallpaper-2026-09-25",
            "messages": [
                { "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "role": "user", "content": "x", "extra": 1 },
            ],
        });
        assert!(serde_json::from_value::<super::HarnessHistoryResponse>(document).is_err());
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

    #[test]
    fn trimming_over_budget_evicts_the_least_recent_conversation_first() {
        // Many large conversations: the budget is exceeded, so whole
        // transcripts are evicted by activity instead of the archive
        // permanently failing to save.
        let policy = "x".repeat(200_000);
        let mut conversations: HashMap<String, ApiConversation> = HashMap::new();
        for index in 0..80u64 {
            conversations.insert(
                format!("old-{index:03}"),
                ApiConversation {
                    messages: vec![
                        message(&format!("old-{index:03}-u"), index * 10 + 1, &policy),
                        message(&format!("old-{index:03}-a"), index * 10 + 2, &policy),
                    ],
                    updated_at: index * 10 + 2,
                },
            );
        }
        conversations.insert(
            "current".into(),
            ApiConversation {
                messages: vec![
                    message("current-u", 1_000_000, "still here"),
                    message("current-a", 1_000_001, "still here too"),
                ],
                updated_at: 1_000_001,
            },
        );
        assert!(archive_len(&conversations) > super::API_CONVERSATION_TARGET_PLAINTEXT_BYTES);

        assert!(trim_api_archive(&mut conversations, Some("current")));
        assert!(
            archive_len(&conversations) <= super::API_CONVERSATION_TARGET_PLAINTEXT_BYTES,
            "trimming must reach the target budget"
        );
        // The current transcript is intact, down to its message identity.
        let current = conversations.get("current").expect("protected conversation");
        assert_eq!(current.messages.len(), 2);
        assert_eq!(current.messages[0].id, "current-u");
        assert_eq!(current.messages[1].id, "current-a");
        assert_eq!(current.messages[1].created_at, 1_000_001);
        // The most recently active evicted conversation survives longest.
        assert!(conversations.contains_key("old-079"));
        assert!(!conversations.contains_key("old-000"));
    }

    #[test]
    fn trimming_never_cuts_a_message_body_and_refuses_a_hopeless_conversation() {
        // One protected conversation larger than the budget with no other
        // conversation to evict. Bodies must stay whole, so the only correct
        // outcome is "nothing to trim" - the caller then refuses the write and
        // keeps the transcript in memory instead of shredding it.
        let one_turn = "y".repeat(2 * 1024 * 1024);
        let mut messages = Vec::new();
        for index in 0..6u64 {
            messages.push(message(&format!("u-{index}"), index * 2 + 1, &one_turn));
            messages.push(message(&format!("a-{index}"), index * 2 + 2, &one_turn));
        }
        let mut conversations: HashMap<String, ApiConversation> = HashMap::new();
        conversations.insert(
            "current".into(),
            ApiConversation {
                messages: messages.clone(),
                updated_at: 99,
            },
        );
        let before = archive_len(&conversations);
        assert!(before > super::API_CONVERSATION_TARGET_PLAINTEXT_BYTES);

        assert!(!trim_api_archive(&mut conversations, Some("current")));
        let current = conversations.get("current").expect("protected conversation");
        // Every message survives, with its identity and its full body.
        assert_eq!(current.messages.len(), messages.len());
        for (kept, original) in current.messages.iter().zip(messages.iter()) {
            assert_eq!(kept.id, original.id);
            assert_eq!(kept.created_at, original.created_at);
            assert_eq!(kept.content.len(), one_turn.len());
        }
        assert_eq!(archive_len(&conversations), before);
    }

    #[test]
    fn trimming_applies_a_per_conversation_message_window() {
        let mut messages = Vec::new();
        for index in 0..(MAX_API_MESSAGES_PER_CONVERSATION as u64 + 5) {
            messages.push(message(&format!("m-{index}"), index + 1, "短"));
        }
        let mut conversations: HashMap<String, ApiConversation> = HashMap::new();
        conversations.insert(
            "current".into(),
            ApiConversation {
                messages,
                updated_at: 999,
            },
        );

        assert!(trim_api_archive(&mut conversations, Some("current")));
        let current = conversations.get("current").expect("conversation");
        assert_eq!(current.messages.len(), MAX_API_MESSAGES_PER_CONVERSATION);
        // The newest turns are the ones kept.
        assert_eq!(current.messages.last().map(|m| m.id.as_str()), Some("m-404"));
        assert_eq!(current.messages.first().map(|m| m.id.as_str()), Some("m-5"));
    }

    #[test]
    fn api_history_returns_only_the_newest_requested_window() {
        let state = ChatState::default();
        for index in 0..10u64 {
            state.append_api_conversation_for_test(
                "conversation-a",
                message(&format!("m-{index}"), index + 1, "content"),
            );
        }

        let full = api_history(&state, "conversation-a", None).expect("history");
        assert_eq!(full["messages"].as_array().map(Vec::len), Some(10));
        assert_eq!(full["totalMessages"], 10);
        assert_eq!(full["hasMore"], false);

        let window = api_history(&state, "conversation-a", Some(3)).expect("windowed history");
        let messages = window["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        // Newest-last order is preserved, so the composer keeps chronology.
        assert_eq!(messages[0]["id"], "m-7");
        assert_eq!(messages[2]["id"], "m-9");
        assert_eq!(window["totalMessages"], 10);
        assert_eq!(window["hasMore"], true);
        assert!(window["bytes"].as_u64().unwrap_or(0) > 0);

        // The renderer cannot ask for an unbounded clone.
        let clamped = api_history(&state, "conversation-a", Some(usize::MAX)).expect("clamped");
        assert_eq!(clamped["limit"], MAX_API_HISTORY_MESSAGES);
        let zero = api_history(&state, "conversation-a", Some(0)).expect("minimum window");
        assert_eq!(zero["limit"], 1);
        assert_eq!(zero["messages"].as_array().map(Vec::len), Some(1));

        let missing = api_history(&state, "missing", None).expect("empty history");
        assert_eq!(missing["messages"].as_array().map(Vec::len), Some(0));
        assert_eq!(missing["hasMore"], false);
        assert_eq!(DEFAULT_API_HISTORY_MESSAGES, 200);
    }

    #[cfg(windows)]
    #[test]
    fn oversized_save_stops_retrying_and_leaves_the_stored_archive_untouched() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));

        // One message larger than the whole plaintext ceiling: trimming cannot
        // help, so persistence must give up instead of looping forever.
        state.append_api_conversation_for_test(
            "huge",
            message(
                "huge-1",
                1,
                &"z".repeat(super::MAX_API_REQUEST_CONTEXT_BYTES + 1),
            ),
        );
        state.append_api_conversation_for_test(
            "huge",
            message("huge-2", 2, &"z".repeat(17 * 1024 * 1024)),
        );
        let measured = archive_len(&state.api_conversations.lock().expect("state").clone());
        assert!(
            measured > crate::api_persistence::MAX_PLAINTEXT_BYTES,
            "fixture must exceed the hard ceiling, got {measured}"
        );
        // Persist with the oversized transcript protected: it is the one the
        // user is in, so the trimmer must refuse rather than silently drop it.
        let error = state
            .persist_api_conversations_for(Some("huge"))
            .expect_err("an oversized archive must be refused");
        assert!(error.contains("容量上限"), "unexpected error: {error}");
        assert!(
            !path.exists(),
            "a refused write must not create or replace the archive"
        );

        // The in-memory transcript still works, and the second attempt is
        // refused without touching the store again.
        assert_eq!(
            state
                .api_conversation("huge")
                .expect("in-memory conversation")
                .messages
                .len(),
            2
        );
        let second = state
            .persist_api_conversations()
            .expect_err("persistence stays disabled for this process");
        assert!(second.contains("不可用"), "unexpected error: {second}");
        assert!(!path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn delete_and_clear_remove_durable_conversations_without_resurrection() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));
        state.append_api_conversation_for_test("conversation-a", message("a-1", 1, "a"));
        state.append_api_conversation_for_test("conversation-b", message("b-1", 2, "b"));
        state
            .persist_api_conversations()
            .expect("persist both conversations");

        // A second process view: its in-memory map was loaded before the
        // delete, so a naive merge would resurrect the removed transcript.
        let stale = ChatState::with_store(EncryptedJsonStore::new(&path));
        assert!(stale.api_conversation("conversation-a").is_some());

        assert!(super::delete_api_conversation(&state, "conversation-a").expect("delete"));
        assert!(!super::delete_api_conversation(&state, "conversation-a").expect("idempotent"));

        let restored = ChatState::with_store(EncryptedJsonStore::new(&path));
        assert!(restored.api_conversation("conversation-a").is_none());
        assert!(restored.api_conversation("conversation-b").is_some());

        assert_eq!(super::clear_api_history(&state).expect("clear"), 1);
        let after_clear = ChatState::with_store(EncryptedJsonStore::new(&path));
        assert!(after_clear.api_conversation("conversation-b").is_none());
        assert_eq!(super::clear_api_history(&state).expect("clear again"), 0);
        assert!(super::delete_api_conversation(&state, "   ").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn persistence_still_fails_closed_for_a_corrupt_archive() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        std::fs::write(&path, b"not a DPAPI blob").expect("corrupt archive");
        let before = std::fs::read(&path).expect("before");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));

        assert!(state.persist_api_conversations().is_err());
        assert_eq!(std::fs::read(&path).expect("after"), before);
        // A corrupt archive is not a size problem: it must not be reported as
        // the recoverable capacity error.
        let error = state.persist_api_conversations().expect_err("still refused");
        assert!(!error.contains("容量上限"), "unexpected error: {error}");
    }

    #[cfg(windows)]
    #[test]
    fn a_trimmed_archive_is_written_instead_of_failing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));
        // Build an archive above the *target* budget but below the hard
        // ceiling, which is exactly the "long-lived install" case: the save
        // must succeed by trimming, not fail.
        let filler = "f".repeat(330_000);
        for index in 0..20u64 {
            for turn in 0..2u64 {
                state.append_api_conversation_for_test(
                    &format!("conversation-{index:02}"),
                    message(
                        &format!("c{index}-m{turn}"),
                        index * 10 + turn + 1,
                        &filler,
                    ),
                );
            }
        }
        let stored = state
            .api_conversations
            .lock()
            .expect("state")
            .clone();
        let measured = archive_len(&stored);
        assert!(
            measured > super::API_CONVERSATION_TARGET_PLAINTEXT_BYTES,
            "fixture must exceed the target budget, got {measured}"
        );
        assert!(
            measured < crate::api_persistence::MAX_PLAINTEXT_BYTES,
            "fixture must stay under the hard ceiling, got {measured}"
        );

        state
            .persist_api_conversations()
            .expect("a trimmable archive must still be written");
        let restored = ChatState::with_store(EncryptedJsonStore::new(&path));
        let after = restored.api_conversations.lock().expect("restored").clone();
        assert!(!after.is_empty(), "trimming must not empty the archive");
        assert!(archive_len(&after) <= super::API_CONVERSATION_TARGET_PLAINTEXT_BYTES);
    }

    #[cfg(windows)]
    #[test]
    fn listing_api_conversations_reports_metadata_without_message_bodies() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));
        state.append_api_conversation_for_test(
            "older",
            message("older-1", 10, &"o".repeat(500)),
        );
        state.append_api_conversation_for_test(
            "newer",
            message("newer-1", 20, "second transcript"),
        );
        state
            .persist_api_conversations()
            .expect("persist both conversations");

        let listing = super::list_api_conversations(&state).expect("listing");
        let conversations = listing["conversations"].as_array().expect("conversations");
        assert_eq!(conversations.len(), 2);
        // Most recent activity first.
        assert_eq!(conversations[0]["id"], "newer");
        assert_eq!(conversations[1]["id"], "older");
        assert_eq!(conversations[1]["messageCount"], 1);
        assert_eq!(conversations[1]["firstMessageAt"], 10);
        assert_eq!(conversations[1]["lastMessageAt"], 10);
        assert!(
            conversations[1]["bytes"].as_u64().unwrap_or(0) >= 500,
            "per-conversation size must reflect its own payload"
        );
        assert_eq!(listing["totalMessages"], 2);
        assert_eq!(
            listing["budgetBytes"],
            super::API_CONVERSATION_TARGET_PLAINTEXT_BYTES
        );
        assert_eq!(
            listing["maxBytes"],
            crate::api_persistence::MAX_PLAINTEXT_BYTES
        );
        // A listing is not a second transcript exposure path.
        assert!(!listing.to_string().contains("second transcript"));
        assert!(!listing.to_string().contains(&"o".repeat(500)));
    }

    #[cfg(windows)]
    #[test]
    fn listing_reflects_a_deletion_made_by_another_state_instance() {
        // The wallpaper and the settings center are separate processes with
        // separate ChatState instances. The settings listing must read the
        // archive, not its own snapshot, or a deleted row would reappear.
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let background = ChatState::with_store(EncryptedJsonStore::new(&path));
        background.append_api_conversation_for_test("conversation-a", message("a-1", 1, "a"));
        background.append_api_conversation_for_test("conversation-b", message("b-1", 2, "b"));
        background
            .persist_api_conversations()
            .expect("persist both");

        let settings = ChatState::with_store(EncryptedJsonStore::new(&path));
        let before = super::list_api_conversations(&settings).expect("listing");
        assert_eq!(before["conversations"].as_array().map(Vec::len), Some(2));

        // The background deletes one transcript.
        assert!(super::delete_api_conversation(&background, "conversation-a").expect("delete"));

        // The settings snapshot still holds both, but the listing must not.
        let after = super::list_api_conversations(&settings).expect("listing");
        let ids: Vec<&str> = after["conversations"]
            .as_array()
            .expect("conversations")
            .iter()
            .filter_map(|entry| entry["id"].as_str())
            .collect();
        assert_eq!(ids, ["conversation-b"]);

        // And clearing is visible immediately too.
        assert_eq!(super::clear_api_history(&background).expect("clear"), 1);
        let cleared = super::list_api_conversations(&settings).expect("listing");
        assert_eq!(cleared["conversations"].as_array().map(Vec::len), Some(0));
    }

    #[cfg(windows)]
    #[test]
    fn listing_marks_the_active_transcript_and_releases_it_on_delete() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));
        state.append_api_conversation_for_test("current", message("c-1", 1, "c"));
        state.append_api_conversation_for_test("other", message("o-1", 2, "o"));
        state.persist_api_conversations().expect("persist");
        state.set_active_api_conversation("current");

        let listing = super::list_api_conversations(&state).expect("listing");
        let rows = listing["conversations"].as_array().expect("conversations");
        let active: Vec<&str> = rows
            .iter()
            .filter(|row| row["active"] == true)
            .filter_map(|row| row["id"].as_str())
            .collect();
        assert_eq!(active, ["current"]);

        assert!(super::delete_api_conversation(&state, "current").expect("delete"));
        let after = super::list_api_conversations(&state).expect("listing");
        assert!(
            after["conversations"]
                .as_array()
                .expect("conversations")
                .iter()
                .all(|row| row["active"] == false),
            "a deleted transcript must not stay marked active"
        );
    }

    #[cfg(windows)]
    #[test]
    fn listing_fails_closed_for_a_corrupt_or_future_archive() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        std::fs::write(&path, b"not a DPAPI blob").expect("corrupt archive");
        let state = ChatState::with_store(EncryptedJsonStore::new(&path));
        // A listing must never turn a damaged archive into an empty-looking
        // list, which would read to the user as "everything was deleted".
        assert!(super::list_api_conversations(&state).is_err());
    }
}
