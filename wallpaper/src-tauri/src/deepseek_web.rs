//! In-app DeepSeek WebView2 transport.
//!
//! This module intentionally talks to the official page through DOM actions
//! only. It does not read cookies, inject credentials, or call an undocumented
//! HTTP endpoint. The page owns its persistent WebView2 profile; the native
//! layer only evaluates a small, versioned DOM adapter and forwards a closed
//! chat-event shape to the wallpaper WebView.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use tauri::webview::NewWindowResponse;
#[cfg(windows)]
use tauri::{
    AppHandle, Emitter, EventTarget, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
#[cfg(windows)]
use tokio::sync::oneshot;
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};

const WINDOW_LABEL: &str = "deepseek-web";
const DOM_SIGNATURE: &str = "deepseek-chat-dom-v2";
const MAX_CALLBACK_BYTES: usize = 4 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 100_000;
const MAX_MESSAGES: usize = 256;
const MAX_IDENTIFIER_BYTES: usize = 200;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const TURN_TIMEOUT: Duration = Duration::from_secs(180);
const PAGE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const QUIET_COMPLETION_POLLS: u8 = 12;
const DEEPSEEK_URL: &str = "https://chat.deepseek.com/";

#[derive(Default)]
pub struct DeepSeekWebState {
    active: Mutex<Option<ActiveRequest>>,
}

struct ActiveRequest {
    request_id: String,
    conversation_id: String,
    cancelled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebStatus {
    pub state: String,
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    pub signature: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebHistory {
    pub messages: Vec<WebHistoryMessage>,
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    pub state: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebHistoryMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebSnapshot {
    signature: String,
    state: String,
    conversation_id: Option<String>,
    model: Option<String>,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    assistant_count: usize,
    #[serde(default)]
    latest_assistant: Option<String>,
    #[serde(default)]
    latest_assistant_key: Option<String>,
    #[serde(default)]
    composer_found: bool,
    #[serde(default)]
    busy: bool,
    #[serde(default)]
    messages: Vec<WebMessage>,
}

#[derive(Clone, Debug, Deserialize)]
struct WebMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Deserialize)]
struct WebActionResult {
    ok: bool,
    #[serde(default)]
    reason: String,
}

/// Decide whether a DOM-polled turn has produced its final reply.  The
/// website's busy marker is only a hint: it can remain visible for a few
/// seconds after the assistant node is committed, so a stable non-empty body
/// is allowed to finish the turn even while `state == "generating"`.
fn web_turn_completion_ready(
    response_started: bool,
    output: &str,
    assistant: &str,
    quiet_polls: u8,
    stable_polls: u8,
) -> bool {
    response_started
        && !output.is_empty()
        && assistant == output
        && (stable_polls >= 2 || quiet_polls >= QUIET_COMPLETION_POLLS)
}

/// Only capture a body after it is known to belong to the current assistant
/// turn.  The baseline assistant body must not be copied into `output` merely
/// because the newly-sent user message increased the transcript count.
fn should_capture_assistant(
    assistant_node_is_new: bool,
    assistant: &str,
    baseline_assistant: &str,
    output: &str,
) -> bool {
    !assistant.is_empty()
        && (assistant_node_is_new || assistant != baseline_assistant)
        && assistant != output
}

#[cfg(windows)]
const INIT_SCRIPT: &str = r#"
(() => {
  const install = () => {
    if (window.__DSHWallpaperDomObserver) return;
    if (!document.documentElement) return;
    const state = { revision: 0 };
    window.__DSHWallpaperDomObserver = state;
    new MutationObserver(() => { state.revision += 1; }).observe(document.documentElement, {
      subtree: true,
      childList: true,
      characterData: true,
    });
  };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', install, { once: true });
  } else {
    install();
  }
})()
"#;

#[cfg(windows)]
const SNAPSHOT_SCRIPT: &str = r#"
(() => {
  const signature = 'deepseek-chat-dom-v2';
  const textOf = (node) => {
    if (!node) return '';
    const rendered = typeof node.innerText === 'string' ? node.innerText : '';
    const raw = rendered.trim() ? rendered : (node.textContent || '');
    return String(raw)
      .replace(/\u00a0/g, ' ')
      .replace(/\r\n?/g, '\n')
      .replace(/[ \t]+/g, ' ')
      .replace(/\n{3,}/g, '\n\n')
      .trim();
  };
  const visible = (node, allowDisplayContents = false) => {
    if (!node) return false;
    const style = getComputedStyle(node);
    const rect = node.getBoundingClientRect();
    return style.display !== 'none'
      && style.visibility !== 'hidden'
      && Number(style.opacity || 1) > 0
      && (allowDisplayContents || rect.width > 0 && rect.height > 0 || node.getClientRects?.().length > 0);
  };
  const disabled = (node) => Boolean(node?.disabled)
    || node?.getAttribute?.('aria-disabled') === 'true'
    || node?.getAttribute?.('data-disabled') === 'true'
    || /(?:^|[\s_-])disabled(?:$|[\s_-])/i.test(typeof node?.className === 'string' ? node.className : '');
  const labelOf = (node) => [node?.getAttribute?.('aria-label') || '', node?.getAttribute?.('title') || '', node?.getAttribute?.('data-testid') || '', textOf(node)].join(' ').toLowerCase();
  const buttons = [...document.querySelectorAll('button,[role="button"]')];
  const composerCandidates = [...document.querySelectorAll('textarea,[contenteditable="true"]')]
    .filter((node) => visible(node, true));
  // DeepSeek's current composer uses a textarea named `search` and renders
  // the send/stop control as an icon-only role=button. Keep the input
  // selection semantic and allow the read-only generation state to be
  // reported as `generating` instead of looking like an unsupported page.
  const composer = composerCandidates.find((node) => !disabled(node));
  const composerSurface = composerCandidates[0];
  const stopButton = buttons.find((node) => {
    if (!visible(node) || disabled(node)) return false;
    const label = labelOf(node);
    return /停止|stop|cancel|interrupt/.test(label);
  });
  const loadingButton = buttons.find((node) => visible(node) && !disabled(node)
    && node.classList?.contains('ds-button--loading'));
  const loginHint = [...document.querySelectorAll('button,a,[role="button"],h1,h2,h3,[class*="sign-in"],[class*="auth"]')].some((node) => visible(node) && /登录|登入|扫码|log\s*in|sign\s*in/.test(labelOf(node)));
  const appShellHint = Boolean(document.querySelector('.ds-button,.ds-textarea,[class*="inputWrapper"],[data-virtual-list-item-key]'));
  const busy = Boolean(stopButton)
    || Boolean(loadingButton)
    || Boolean(composerSurface?.getAttribute('aria-busy') === 'true')
    || Boolean(composerSurface?.closest('[aria-busy="true"]'))
    || Boolean(composerSurface?.readOnly);

  const roleOf = (node) => {
    const raw = [
      node?.getAttribute?.('data-message-author-role') || '',
      node?.getAttribute?.('data-role') || '',
      node?.getAttribute?.('aria-label') || '',
      typeof node?.className === 'string' ? node.className : '',
    ].join(' ').toLowerCase();
    if (/(?:^|[\s_-])(?:assistant|ai-message|bot-message|deepseek-assistant)(?:$|[\s_-])/.test(raw)) return 'assistant';
    if (/(?:^|[\s_-])(?:user|user-message|human-message)(?:$|[\s_-])/.test(raw)) return 'user';
    return null;
  };
  const assistantSelector = '.ds-assistant-message-main-content,[data-message-author-role="assistant"],[data-role="assistant"],[data-role="assistant_message"],[data-testid*="assistant-message"],[class*="assistant-message"]';
  const assistantNodes = [...document.querySelectorAll(assistantSelector)]
    .filter((node) => visible(node, true));
  // A few DeepSeek deployments put the assistant marker on an ancestor and
  // the actual text in a markdown child. Include that child explicitly so a
  // virtualized/display:contents layout cannot make the latest answer look
  // absent to the native poller.
  const markdownAssistantNodes = [...document.querySelectorAll('.ds-markdown,[class*="markdown"]')]
    .filter((node) => {
      const owner = node.closest?.('[data-message-author-role],[data-role],[class*="assistant-message"],[class*="assistant"]');
      return Boolean(owner && /assistant|ai-message|bot-message|deepseek-assistant/i.test([
        owner.getAttribute?.('data-message-author-role') || '',
        owner.getAttribute?.('data-role') || '',
        typeof owner.className === 'string' ? owner.className : '',
      ].join(' ')) && visible(node, true));
    });
  for (const node of markdownAssistantNodes) {
    if (!assistantNodes.some((candidate) => candidate === node || candidate.contains?.(node))) assistantNodes.push(node);
  }
  assistantNodes.sort((left, right) => {
    if (left === right) return 0;
    const relation = left.compareDocumentPosition?.(right) || 0;
    if (relation & Node.DOCUMENT_POSITION_FOLLOWING) return -1;
    if (relation & Node.DOCUMENT_POSITION_PRECEDING) return 1;
    return 0;
  });
  const assistantContentOf = (node) => {
    // DeepSeek currently puts the rendered Markdown in this node, but the
    // node itself can be `display: contents`. Read the Markdown child first
    // and fall back to textContent so a zero-size wrapper never hides a reply.
    const bodies = [...(node.querySelectorAll?.('.ds-markdown,[class*="markdown"],[data-testid*="markdown"]') || [])]
      .filter((candidate) => visible(candidate, true));
    if (!bodies.length) return textOf(node);
    const bodySet = new Set(bodies);
    // A rendered answer can contain nested Markdown nodes, or several
    // same-level blocks for separate paragraphs. Taking only the final body
    // loses every preceding block. Keep only the outermost candidates, then merge
    // sibling blocks in document order.
    const outerBodies = bodies.filter((candidate) => {
      let parent = candidate.parentElement;
      while (parent && parent !== node) {
        if (bodySet.has(parent)) return false;
        parent = parent.parentElement;
      }
      return true;
    });
    return outerBodies.map(textOf).filter(Boolean).join('\n\n') || textOf(node);
  };
  const assistantKeyOf = (node) => node.closest?.('[data-virtual-list-item-key]')?.getAttribute('data-virtual-list-item-key')
    || node.getAttribute?.('data-message-id')
    || node.getAttribute?.('data-id')
    || null;
  const assistantEntries = assistantNodes
    .map((node) => ({ node, role: 'assistant', key: assistantKeyOf(node), content: assistantContentOf(node) }))
    .filter((entry) => entry.content && entry.content.length <= 100000);
  const directAssistantSet = new Set(assistantNodes);
  const explicit = [...document.querySelectorAll('[data-message-author-role],[data-role],[class*="user-message"]')]
    .filter((node) => !directAssistantSet.has(node));
  const virtualItems = [...document.querySelectorAll('[data-virtual-list-item-key]')]
    .filter((node) => !node.querySelector?.(assistantSelector));
  const fallback = [...document.querySelectorAll('article,[data-testid*="message"],[class*="message"]')]
    .filter((node) => !node.querySelector?.(assistantSelector));
  const messages = [];
  const seen = new Set();
  const push = (node, role, content = textOf(node)) => {
    if (!node || seen.has(node) || !content || content.length > 100000) return;
    // Ancestor message wrappers often repeat the direct assistant node. Keep
    // the smallest readable node and let the direct selector own its content.
    if ([...seen].some((previous) => previous.contains?.(node) || node.contains?.(previous))) return;
    seen.add(node);
    messages.push({ node, role, content });
  };
  for (const entry of [...assistantEntries, ...explicit, ...virtualItems, ...fallback]) {
    const node = entry.node || entry;
    if (!visible(node, true)) continue;
    const role = entry.role || roleOf(node) || (node.matches?.('[data-virtual-list-item-key]') ? 'user' : null);
    const content = entry.content || textOf(node);
    if (!role || !content || content.length > 100000) continue;
    push(node, role, content);
  }
  // DOM query groups are not guaranteed to be returned in one merged order;
  // restore document order before exposing the transcript to Rust.
  messages.sort((left, right) => {
    if (left.node === right.node) return 0;
    const relation = left.node.compareDocumentPosition?.(right.node) || 0;
    if (relation & Node.DOCUMENT_POSITION_FOLLOWING) return -1;
    if (relation & Node.DOCUMENT_POSITION_PRECEDING) return 1;
    return 0;
  });
  const normalizedMessages = [];
  for (const message of messages) {
    const previous = normalizedMessages[normalizedMessages.length - 1];
    if (previous && previous.role === message.role && previous.content === message.content) continue;
    normalizedMessages.push({ role: message.role, content: message.content });
  }

  let conversationId = document.querySelector('[data-conversation-id]')?.getAttribute('data-conversation-id') || null;
  const parts = location.pathname.split('/').filter(Boolean);
  if (!conversationId) {
    for (let index = 0; index + 1 < parts.length; index += 1) {
      if (/^(?:chat|conversation|c|s)$/i.test(parts[index]) && /^[A-Za-z0-9_-]{4,200}$/.test(parts[index + 1])) {
        conversationId = parts[index + 1];
        break;
      }
    }
  }
  if (conversationId && !/^[A-Za-z0-9._-]{1,200}$/.test(conversationId)) conversationId = null;

  let model = null;
  for (const node of document.querySelectorAll('[data-testid*="model"],[role="combobox"],button[aria-haspopup="listbox"]')) {
    const candidate = textOf(node);
    if (candidate && candidate.length <= 200 && /deepseek|模型|model|reasoner|chat/i.test(candidate)) {
      model = candidate;
      break;
    }
  }
  const state = composer
    ? (busy ? 'generating' : 'ready')
    : (loginHint ? 'logged-out' : (document.readyState !== 'complete' || !document.body?.innerText || appShellHint ? 'loading' : 'unsupported'));
  return {
    signature,
    state,
    conversationId,
    model,
    revision: window.__DSHWallpaperDomObserver?.revision || 0,
    assistantCount: assistantEntries.length,
    latestAssistant: assistantEntries.at(-1)?.content || null,
    latestAssistantKey: assistantEntries.at(-1)?.key || null,
    composerFound: Boolean(composer),
    busy,
    messages: normalizedMessages,
  };
})()
"#;

#[cfg(windows)]
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(windows)]
fn safe_identifier(value: Option<String>, fallback: String) -> String {
    let Some(value) = value.map(|value| value.trim().to_string()) else {
        return fallback;
    };
    if value.is_empty()
        || value.as_bytes().len() > MAX_IDENTIFIER_BYTES
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        fallback
    } else {
        value
    }
}

#[cfg(windows)]
fn is_deepseek_url(url: &tauri::Url) -> bool {
    if url.as_str() == "about:blank" {
        return true;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    url.scheme() == "https"
        && (host.eq_ignore_ascii_case("chat.deepseek.com")
            || host.to_ascii_lowercase().ends_with(".deepseek.com"))
}

#[cfg(windows)]
fn open_external_url_allowed(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

#[cfg(windows)]
fn open_external(url: &tauri::Url) {
    if !open_external_url_allowed(url) {
        return;
    }
    let mut command = std::process::Command::new("explorer.exe");
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
    let _ = command.arg(url.as_str()).spawn();
}

#[cfg(windows)]
fn ensure_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return Ok(window);
    }
    static BUILD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = BUILD_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "DeepSeek 网页窗口状态不可用".to_string())?;
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return Ok(window);
    }

    let data_directory = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法定位 DeepSeek 网页数据目录：{error}"))?
        .join("deepseek-webview2");
    std::fs::create_dir_all(&data_directory)
        .map_err(|error| format!("无法创建 DeepSeek 网页数据目录：{error}"))?;

    // Build against a local blank document first. Loading a remote SPA as the
    // initial WebView2 navigation can synchronously hold Tao's event loop on
    // some WebView2 versions; navigation is started after the HWND exists.
    let initial_url = "about:blank"
        .parse()
        .map_err(|error| format!("DeepSeek 网页占位地址无效：{error}"))?;
    WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(initial_url))
        .title("DeepSeek · 应用内网页入口")
        .inner_size(560.0, 760.0)
        .min_inner_size(420.0, 620.0)
        .center()
        .decorations(true)
        .resizable(true)
        .focusable(true)
        .skip_taskbar(false)
        .visible(false)
        .data_directory(data_directory)
        .initialization_script(INIT_SCRIPT)
        .on_navigation(|url| {
            if is_deepseek_url(url) {
                true
            } else {
                open_external(url);
                false
            }
        })
        .on_new_window(|url, _features| {
            if !is_deepseek_url(&url) {
                open_external(&url);
            }
            NewWindowResponse::Deny
        })
        .build()
        .map_err(|error| format!("无法创建 DeepSeek 应用内网页窗口：{error}"))
}

#[cfg(windows)]
fn start_navigation(window: &WebviewWindow) -> Result<(), String> {
    let current = window.url().ok();
    if current
        .as_ref()
        .is_some_and(|url| url.as_str() != "about:blank" && is_deepseek_url(url))
    {
        return Ok(());
    }
    let url = DEEPSEEK_URL
        .parse()
        .map_err(|error| format!("DeepSeek 网页地址无效：{error}"))?;
    window
        .navigate(url)
        .map_err(|error| format!("无法打开 DeepSeek 应用内页面：{error}"))
}

#[cfg(windows)]
async fn eval_json<T: DeserializeOwned>(window: &WebviewWindow, script: &str) -> Result<T, String> {
    let (sender, receiver) = oneshot::channel::<String>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let callback_sender = Arc::clone(&sender);
    window
        .eval_with_callback(script.to_string(), move |value| {
            if let Ok(mut sender) = callback_sender.lock() {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(value);
                }
            }
        })
        .map_err(|error| format!("DeepSeek 网页脚本执行失败：{error}"))?;
    let raw = tokio::time::timeout(Duration::from_secs(3), receiver)
        .await
        .map_err(|_| "DeepSeek 网页响应超时；页面可能正在加载。".to_string())?
        .map_err(|_| "DeepSeek 网页没有返回结果。".to_string())?;
    if raw.len() > MAX_CALLBACK_BYTES {
        return Err("DeepSeek 网页返回内容超过安全上限。".into());
    }
    serde_json::from_str::<T>(&raw).or_else(|_| {
        let nested = serde_json::from_str::<String>(&raw)
            .map_err(|error| format!("DeepSeek 网页返回格式无效：{error}"))?;
        serde_json::from_str::<T>(&nested)
            .map_err(|error| format!("DeepSeek 网页返回格式无效：{error}"))
    })
}

#[cfg(windows)]
fn validate_snapshot(mut snapshot: WebSnapshot) -> Result<WebSnapshot, String> {
    if snapshot.signature != DOM_SIGNATURE {
        return Err("DeepSeek 网页适配器版本不匹配。".into());
    }
    if !matches!(
        snapshot.state.as_str(),
        "loading" | "logged-out" | "ready" | "generating" | "unsupported"
    ) {
        return Err("DeepSeek 网页状态无法识别。".into());
    }
    if snapshot.messages.len() > MAX_MESSAGES {
        return Err("DeepSeek 网页会话记录超过安全上限。".into());
    }
    snapshot.messages.retain(|message| {
        matches!(message.role.as_str(), "user" | "assistant")
            && !message.content.is_empty()
            && message.content.as_bytes().len() <= MAX_MESSAGE_BYTES
    });
    if snapshot
        .conversation_id
        .as_ref()
        .is_some_and(|value| value.as_bytes().len() > MAX_IDENTIFIER_BYTES)
    {
        snapshot.conversation_id = None;
    }
    if snapshot
        .model
        .as_ref()
        .is_some_and(|value| value.as_bytes().len() > MAX_IDENTIFIER_BYTES)
    {
        snapshot.model = None;
    }
    if snapshot
        .latest_assistant_key
        .as_ref()
        .is_some_and(|value| value.as_bytes().len() > MAX_IDENTIFIER_BYTES)
    {
        snapshot.latest_assistant_key = None;
    }
    Ok(snapshot)
}

#[cfg(windows)]
async fn snapshot(window: &WebviewWindow) -> Result<WebSnapshot, String> {
    validate_snapshot(eval_json(window, SNAPSHOT_SCRIPT).await?)
}

#[cfg(windows)]
async fn wait_for_page_snapshot(window: &WebviewWindow) -> Result<WebSnapshot, String> {
    let deadline = Instant::now() + PAGE_READY_TIMEOUT;
    let mut last_error: Option<String> = None;
    loop {
        match snapshot(window).await {
            Ok(snapshot) if snapshot.state != "loading" => return Ok(snapshot),
            Ok(_) => {}
            Err(error) => last_error = Some(error),
        }
        if Instant::now() >= deadline {
            return Err(match last_error {
                Some(error) => format!("DeepSeek 网页加载超时；页面可能仍在加载（{error}）。"),
                None => "DeepSeek 网页加载超时；页面可能仍在加载。".into(),
            });
        }
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
}

#[cfg(windows)]
fn prepare_send_script(text: &str) -> Result<String, String> {
    let encoded = serde_json::to_string(text).map_err(|error| error.to_string())?;
    Ok(format!(
        r#"
(() => {{
  const value = {encoded};
  const visible = (node) => {{
    if (!node) return false;
    const style = getComputedStyle(node);
    const rect = node.getBoundingClientRect();
    return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
  }};
  const disabled = (node) => Boolean(node?.disabled)
    || node?.getAttribute?.('aria-disabled') === 'true'
    || node?.getAttribute?.('data-disabled') === 'true';
  const inputs = [...document.querySelectorAll('textarea,[contenteditable="true"]')];
  const input = inputs.find((node) => visible(node) && !disabled(node) && !node.readOnly);
  if (!input) return {{ ok: false, reason: 'composer-not-found' }};
  input.focus();
  if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) {{
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set
      || Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value')?.set;
    setter?.call(input, value);
  }} else {{
    input.replaceChildren(document.createTextNode(value));
    try {{ document.execCommand('insertText', false, value); }} catch (_) {{ input.textContent = value; }}
    if (input.textContent !== value) input.textContent = value;
  }}
  try {{
    input.dispatchEvent(new InputEvent('input', {{ bubbles: true, composed: true, inputType: 'insertText', data: value }}));
  }} catch (_) {{
    input.dispatchEvent(new Event('input', {{ bubbles: true, composed: true }}));
  }}
  return {{ ok: true, reason: 'prepared' }};
}})()
"#
    ))
}

#[cfg(windows)]
const TRIGGER_SEND_SCRIPT: &str = r#"
(() => {
  const visible = (node) => {
    if (!node) return false;
    const style = getComputedStyle(node);
    const rect = node.getBoundingClientRect();
    return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
  };
  const disabled = (node) => Boolean(node?.disabled)
    || node?.getAttribute?.('aria-disabled') === 'true'
    || node?.getAttribute?.('data-disabled') === 'true'
    || /(?:^|[\s_-])disabled(?:$|[\s_-])/i.test(typeof node?.className === 'string' ? node.className : '');
  const labelOf = (node) => [node?.getAttribute?.('aria-label') || '', node?.getAttribute?.('title') || '', node?.getAttribute?.('data-testid') || '', node?.innerText || ''].join(' ').toLowerCase();
  const input = [...document.querySelectorAll('textarea,[contenteditable="true"]')]
    .find((node) => visible(node) && !disabled(node) && !node.readOnly);
  if (!input) return { ok: false, reason: 'composer-not-found' };
  const candidatesIn = (scope) => [...scope.querySelectorAll('button,[role="button"]')]
    .filter((node) => visible(node));
  let action;
  const semantic = candidatesIn(document).filter((node) => !disabled(node)
    && (/发送|send|submit|continue/.test(labelOf(node))
      || /send|submit|continue/i.test(node.getAttribute('data-testid') || '')));
  action = semantic.at(-1);
  if (!action) {
    // The current DeepSeek page renders the main control as an icon-only
    // role=button, with search/thinking/file controls before it. Walk up from
    // the textarea and use the last visible enabled control in the smallest
    // composer scope that owns one.
    let scope = input.parentElement;
    for (let depth = 0; scope && depth < 10; depth += 1, scope = scope.parentElement) {
      const controls = candidatesIn(scope);
      if (controls.length) {
        action = [...controls].reverse().find((node) => !disabled(node));
        if (action) break;
        return { ok: false, reason: 'send-control-not-ready' };
      }
    }
  }
  if (action && !disabled(action)) {
    action.click();
    return { ok: true, reason: 'clicked-primary-control' };
  }
  const buttons = [...document.querySelectorAll('button,[role="button"]')];
  const send = buttons.find((node) => visible(node) && !disabled(node) && /发送|send|submit|continue/.test(labelOf(node)))
    || [...document.querySelectorAll('[data-testid*="send"],[data-testid*="submit"]')].find((node) => visible(node) && !disabled(node));
  if (send) { send.click(); return { ok: true, reason: 'sent' }; }
  const form = input.closest('form');
  if (form?.requestSubmit) { form.requestSubmit(); return { ok: true, reason: 'submitted-form' }; }
  input.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'Enter', code: 'Enter', keyCode: 13, which: 13,
    bubbles: true, cancelable: true, composed: true,
  }));
  return { ok: true, reason: 'submitted-keyboard' };
})()
"#;

#[cfg(windows)]
const STOP_SCRIPT: &str = r#"
(() => {
  const labelOf = (node) => [node?.getAttribute?.('aria-label') || '', node?.getAttribute?.('title') || '', node?.innerText || ''].join(' ').toLowerCase();
  const disabled = (node) => Boolean(node?.disabled)
    || node?.getAttribute?.('aria-disabled') === 'true'
    || node?.getAttribute?.('data-disabled') === 'true';
  const buttons = [...document.querySelectorAll('button,[role="button"]')];
  const stop = buttons.find((node) => !disabled(node) && (/停止|stop|cancel|interrupt/.test(labelOf(node)) || /stop|cancel|interrupt/i.test(node.getAttribute('data-testid') || '')));
  if (!stop) return { ok: false, reason: 'stop-not-found' };
  stop.click();
  return { ok: true, reason: 'stopped' };
})()
"#;

#[cfg(windows)]
fn emit_event(app: &AppHandle, conversation_id: &str, request_id: Option<&str>, mut event: Value) {
    let Some(object) = event.as_object_mut() else {
        return;
    };
    object.insert("backend".into(), json!("deepseek-web"));
    object.insert("conversationId".into(), json!(conversation_id));
    if let Some(request_id) = request_id {
        object.insert("requestId".into(), json!(request_id));
    }
    let _ = app.emit_to(
        EventTarget::webview_window("background"),
        "chat-event",
        event,
    );
}

impl DeepSeekWebState {
    fn begin(&self, request_id: String, conversation_id: String) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "DeepSeek 网页请求状态不可用".to_string())?;
        if active.is_some() {
            return Err("DeepSeek 网页已有一轮请求正在进行。".into());
        }
        *active = Some(ActiveRequest {
            request_id,
            conversation_id,
            cancelled: false,
        });
        Ok(())
    }

    fn cancelled(&self, request_id: &str) -> bool {
        self.active
            .lock()
            .ok()
            .and_then(|active| {
                active
                    .as_ref()
                    // A request that observes a newer request must also stop
                    // publishing. This lets cancel release the slot
                    // immediately without allowing the old poll loop to race
                    // the next turn.
                    .map(|active| active.request_id != request_id || active.cancelled)
            })
            .unwrap_or(true)
    }

    fn clear(&self, request_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if active
                .as_ref()
                .is_some_and(|active| active.request_id == request_id)
            {
                *active = None;
            }
        }
    }

    fn cancel(&self) -> Option<(String, String)> {
        let mut active = self.active.lock().ok()?;
        let request = active.take()?;
        Some((request.request_id, request.conversation_id))
    }
}

#[cfg(windows)]
pub fn ensure(app: &AppHandle) -> Result<(), String> {
    let window = ensure_window(app)?;
    let _ = start_navigation(&window);
    Ok(())
}

#[cfg(not(windows))]
pub fn ensure(_: &tauri::AppHandle) -> Result<(), String> {
    Err("DeepSeek 应用内网页通讯目前仅支持 Windows WebView2。".into())
}

#[cfg(windows)]
pub fn show_login(app: &AppHandle) -> Result<(), String> {
    let window = ensure_window(app)?;
    reveal_window(&window);
    start_navigation(&window)?;
    // A freshly-created WebView2 window can finish its controller setup after
    // the first Tauri `show()` message. Retry once after the native HWND exists
    // so the login action never leaves a hidden, otherwise healthy WebView.
    let retry = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(120));
        reveal_window(&retry);
    });
    Ok(())
}

#[cfg(windows)]
fn reveal_window(window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.unminimize();
    if let Ok(raw) = window.hwnd() {
        let hwnd = HWND(raw.0);
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    let _ = window.set_focus();
}

#[cfg(not(windows))]
pub fn show_login(_: &tauri::AppHandle) -> Result<(), String> {
    Err("DeepSeek 应用内网页通讯目前仅支持 Windows WebView2。".into())
}

#[cfg(windows)]
pub async fn status(app: &AppHandle) -> Result<WebStatus, String> {
    // Keep the resident wallpaper light: selecting the default web route does
    // not create a Chromium renderer until the user opens login or sends the
    // first message. Once created, the same window/profile is reused.
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(WebStatus {
            state: "loading".into(),
            conversation_id: None,
            model: None,
            signature: DOM_SIGNATURE.into(),
        });
    };
    match snapshot(&window).await {
        Ok(snapshot) => Ok(WebStatus {
            state: snapshot.state,
            conversation_id: snapshot.conversation_id,
            model: snapshot.model,
            signature: snapshot.signature,
        }),
        Err(_) => Ok(WebStatus {
            state: "loading".into(),
            conversation_id: None,
            model: None,
            signature: DOM_SIGNATURE.into(),
        }),
    }
}

#[cfg(not(windows))]
pub async fn status(_: &tauri::AppHandle) -> Result<WebStatus, String> {
    Err("DeepSeek 应用内网页通讯目前仅支持 Windows WebView2。".into())
}

#[cfg(windows)]
pub async fn history(app: &AppHandle) -> Result<WebHistory, String> {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(WebHistory {
            messages: Vec::new(),
            conversation_id: None,
            model: None,
            state: "loading".into(),
        });
    };
    let _ = start_navigation(&window);
    let snapshot = match wait_for_page_snapshot(&window).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            log::debug!("deepseek web history snapshot unavailable: {error}");
            return Ok(WebHistory {
                messages: Vec::new(),
                conversation_id: None,
                model: None,
                state: "loading".into(),
            });
        }
    };
    let timestamp = now_millis();
    Ok(WebHistory {
        messages: snapshot
            .messages
            .into_iter()
            .enumerate()
            .map(|(index, message)| WebHistoryMessage {
                id: format!("web-{timestamp}-{index}"),
                role: message.role,
                content: message.content,
                created_at: timestamp,
            })
            .collect(),
        conversation_id: snapshot.conversation_id,
        model: snapshot.model,
        state: snapshot.state,
    })
}

#[cfg(not(windows))]
pub async fn history(_: &tauri::AppHandle) -> Result<WebHistory, String> {
    Err("DeepSeek 应用内网页通讯目前仅支持 Windows WebView2。".into())
}

#[cfg(windows)]
pub async fn send(
    app: AppHandle,
    state: &DeepSeekWebState,
    text: String,
    conversation_id: Option<String>,
    request_id: Option<String>,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("消息不能为空。".into());
    }
    if text.as_bytes().len() > MAX_MESSAGE_BYTES {
        return Err(format!("单条网页消息不能超过 {MAX_MESSAGE_BYTES} 字节。"));
    }
    let window = ensure_window(&app)?;
    start_navigation(&window)?;
    let initial = wait_for_page_snapshot(&window).await?;
    let conversation_id = safe_identifier(
        initial.conversation_id.clone().or(conversation_id),
        format!("web-{}", now_millis()),
    );
    let request_id = safe_identifier(request_id, format!("web-request-{}", now_millis()));

    if initial.state == "logged-out" {
        let _ = show_login(&app);
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({ "type": "auth-required" }),
        );
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({ "type": "status", "activity": "idle" }),
        );
        return Ok(conversation_id);
    }
    if initial.state == "loading" || initial.state == "unsupported" {
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({
                "type": "error",
                "code": "DEEPSEEK_WEB_UNSUPPORTED",
                "recoverable": true,
                "message": "DeepSeek 网页结构无法识别，网页桥接需要更新。"
            }),
        );
        return Ok(conversation_id);
    }
    if initial.state == "generating" {
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({
                "type": "error",
                "code": "DEEPSEEK_WEB_BUSY",
                "recoverable": true,
                "message": "DeepSeek 网页正在生成上一条回复，请稍候。"
            }),
        );
        return Ok(conversation_id);
    }

    // Login is the only time this auxiliary WebView is meant to be visible.
    // Once the official page is ready, keep it out of the user's way while
    // the desktop bubble owns the conversation.
    let _ = window.hide();

    state.begin(request_id.clone(), conversation_id.clone())?;
    let result = async {
        if let Some(model) = initial.model.clone() {
            emit_event(
                &app,
                &conversation_id,
                Some(&request_id),
                json!({ "type": "model", "provider": "deepseek-web", "model": model, "tier": "unknown" }),
            );
        }
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({ "type": "status", "activity": "sending" }),
        );
        let prepared: WebActionResult = eval_json(&window, &prepare_send_script(&text)?).await?;
        if !prepared.ok {
            return Err("DeepSeek 网页输入框尚未准备好，请稍候重试。".into());
        }
        // React owns the page input. The DOM value and the React value are
        // not guaranteed to be committed in the same JavaScript turn, so
        // trigger the icon-only send control only after a short render tick.
        // This is the difference between a visible draft and an accepted
        // message on the current DeepSeek composer.
        let mut action = WebActionResult { ok: false, reason: String::new() };
        for attempt in 0..6 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(80)).await;
            }
            action = eval_json(&window, TRIGGER_SEND_SCRIPT).await?;
            if action.ok {
                break;
            }
        }
        if !action.ok {
            if action.reason == "composer-not-found" {
                return Err("DeepSeek 网页输入框尚未准备好，请稍候重试。".into());
            }
            return Err("DeepSeek 网页发送控件尚未准备好，请稍候重试。".into());
        }

        let baseline_count = initial.messages.len();
        let baseline_assistant_count = initial.assistant_count;
        let baseline_assistant = initial
            .latest_assistant
            .clone()
            .or_else(|| {
                initial
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == "assistant")
                    .map(|message| message.content.clone())
            })
            .unwrap_or_default();
        let baseline_assistant_key = initial.latest_assistant_key.clone();
        let mut output = String::new();
        let mut response_started = false;
        let mut assistant_node_seen = false;
        let mut active_assistant_key: Option<String> = None;
        let mut stable_polls = 0u8;
        let mut quiet_polls = 0u8;
        let mut last_observation = initial.clone();
        let mut reported_missing_body = false;
        let deadline = Instant::now() + TURN_TIMEOUT;
        loop {
            if state.cancelled(&request_id) {
                emit_event(
                    &app,
                    &conversation_id,
                    Some(&request_id),
                    json!({ "type": "status", "activity": "idle" }),
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                log::warn!(
                    "deepseek web turn timed out: conversation={}, request={}, state={}, messages={}, assistant_count={}, latest_assistant_len={}, composer_found={}, busy={}, revision={}",
                    conversation_id,
                    request_id,
                    last_observation.state,
                    last_observation.messages.len(),
                    last_observation.assistant_count,
                    last_observation.latest_assistant.as_ref().map_or(0, |value| value.len()),
                    last_observation.composer_found,
                    last_observation.busy,
                    last_observation.revision,
                );
                return Err("DeepSeek 网页响应超时；可以在网页窗口中检查当前会话。".into());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
            let current = snapshot(&window).await?;
            last_observation = current.clone();
            if current.state == "logged-out" {
                let _ = show_login(&app);
                emit_event(&app, &conversation_id, Some(&request_id), json!({ "type": "auth-required" }));
                return Ok(());
            }
            if current.state == "unsupported" {
                return Err("DeepSeek 网页结构无法识别，网页桥接需要更新。".into());
            }
            let assistant = current
                .latest_assistant
                .as_deref()
                .or_else(|| {
                    current
                        .messages
                        .iter()
                        .rev()
                        .find(|message| message.role == "assistant")
                        .map(|message| message.content.as_str())
                })
                .unwrap_or("");
            if response_started && assistant.is_empty() && !reported_missing_body {
                log::debug!(
                    "deepseek web turn has no assistant body: state={}, messages={}, assistant_count={}, composer_found={}, busy={}, revision={}",
                    current.state,
                    current.messages.len(),
                    current.assistant_count,
                    current.composer_found,
                    current.busy,
                    current.revision,
                );
                reported_missing_body = true;
            }
            let assistant_key_is_new = current
                .latest_assistant_key
                .as_ref()
                .is_some_and(|key| {
                    baseline_assistant_key.as_deref() != Some(key.as_str())
                        && active_assistant_key.as_deref() != Some(key.as_str())
                });
            let assistant_node_is_new = if assistant_key_is_new {
                active_assistant_key = current.latest_assistant_key.clone();
                assistant_node_seen = true;
                true
            } else if current.latest_assistant_key.is_none()
                && current.assistant_count > baseline_assistant_count
                && !assistant_node_seen
            {
                assistant_node_seen = true;
                true
            } else {
                false
            };
            if current.messages.len() > baseline_count
                || assistant_node_is_new
                || (!assistant.is_empty() && assistant != baseline_assistant)
            {
                response_started = true;
            }
            // A new assistant node can legitimately contain the exact same
            // text as the previous answer. Once a new node or a changed body
            // proves that this is the current turn, keep updating `output`
            // even when it eventually equals the baseline text.
            let capture_assistant = should_capture_assistant(
                assistant_node_is_new,
                assistant,
                &baseline_assistant,
                &output,
            );
            if capture_assistant {
                let delta = if assistant_node_is_new || output.is_empty() {
                    assistant
                } else if assistant.starts_with(&output) {
                    &assistant[output.len()..]
                } else {
                    // The page occasionally rewrites the already-rendered
                    // Markdown instead of appending to it. Do not duplicate
                    // the whole body as a delta; the final message event will
                    // publish the replacement body when it becomes stable.
                    ""
                };
                if !delta.is_empty() {
                    emit_event(
                        &app,
                        &conversation_id,
                        Some(&request_id),
                        json!({ "type": "delta", "text": delta }),
                    );
                    emit_event(
                        &app,
                        &conversation_id,
                        Some(&request_id),
                        json!({ "type": "status", "activity": "streaming" }),
                    );
                }
                output = assistant.to_string();
                quiet_polls = 0;
                stable_polls = 0;
            } else {
                if response_started && !output.is_empty() && assistant == output {
                    quiet_polls = quiet_polls.saturating_add(1);
                    // The page can keep a stale "stop generating" button or
                    // mutate a streaming cursor after the answer is already
                    // committed. Content stability is therefore the primary
                    // completion signal; DOM revision and the busy marker are
                    // only hints and must not hold a completed turn forever.
                    if current.state != "generating" || quiet_polls >= QUIET_COMPLETION_POLLS {
                        stable_polls = stable_polls.saturating_add(1);
                    } else {
                        stable_polls = 0;
                    }
                } else {
                    quiet_polls = 0;
                    stable_polls = 0;
                }
            }
            if web_turn_completion_ready(response_started, &output, assistant, quiet_polls, stable_polls) {
                emit_event(
                    &app,
                    &conversation_id,
                    Some(&request_id),
                    json!({ "type": "message", "role": "assistant", "content": output }),
                );
                emit_event(
                    &app,
                    &conversation_id,
                    Some(&request_id),
                    json!({ "type": "status", "activity": "done" }),
                );
                return Ok(());
            }
        }
    }
    .await;
    state.clear(&request_id);
    if let Err(error) = &result {
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({
                "type": "error",
                "code": "DEEPSEEK_WEB_SEND_FAILED",
                "recoverable": true,
                "message": error
            }),
        );
        emit_event(
            &app,
            &conversation_id,
            Some(&request_id),
            json!({ "type": "status", "activity": "idle" }),
        );
    }
    result.map(|_| conversation_id)
}

#[cfg(not(windows))]
pub async fn send(
    _: tauri::AppHandle,
    _: &DeepSeekWebState,
    _: String,
    _: Option<String>,
    _: Option<String>,
) -> Result<String, String> {
    Err("DeepSeek 应用内网页通讯目前仅支持 Windows WebView2。".into())
}

#[cfg(windows)]
pub async fn cancel(app: &AppHandle, state: &DeepSeekWebState) -> Result<(), String> {
    let Some((request_id, conversation_id)) = state.cancel() else {
        return Ok(());
    };
    if let Ok(window) = ensure_window(app) {
        let _: Result<WebActionResult, _> = eval_json(&window, STOP_SCRIPT).await;
    }
    emit_event(
        app,
        &conversation_id,
        Some(&request_id),
        json!({ "type": "status", "activity": "idle" }),
    );
    Ok(())
}

#[cfg(not(windows))]
pub async fn cancel(_: &tauri::AppHandle, _: &DeepSeekWebState) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{should_capture_assistant, web_turn_completion_ready, DeepSeekWebState};

    #[cfg(windows)]
    #[test]
    fn snapshot_script_keeps_direct_assistant_body_and_safe_diagnostics() {
        assert!(super::SNAPSHOT_SCRIPT.contains(".ds-assistant-message-main-content"));
        assert!(super::SNAPSHOT_SCRIPT.contains("latestAssistant"));
        assert!(super::SNAPSHOT_SCRIPT.contains("latestAssistantKey"));
        assert!(super::SNAPSHOT_SCRIPT.contains("assistantCount"));
        assert!(super::SNAPSHOT_SCRIPT.contains("outerBodies"));
        assert!(super::SNAPSHOT_SCRIPT.contains("join('\\n\\n')"));
        assert!(!super::SNAPSHOT_SCRIPT.contains("const body = bodies.at(-1)"));
        assert!(!super::SNAPSHOT_SCRIPT.contains("!dom_changed"));
        assert!(
            super::TRIGGER_SEND_SCRIPT.contains("functionRowRightColumn")
                || super::TRIGGER_SEND_SCRIPT.contains("clicked-primary-control")
        );
    }

    #[test]
    fn a_stable_reply_finishes_even_when_the_page_keeps_busy_hint() {
        assert!(web_turn_completion_ready(
            true,
            "最终回复",
            "最终回复",
            12,
            0,
        ));
    }

    #[test]
    fn a_ready_page_still_requires_two_stable_polls() {
        assert!(!web_turn_completion_ready(
            true,
            "最终回复",
            "最终回复",
            1,
            1
        ));
        assert!(web_turn_completion_ready(
            true,
            "最终回复",
            "最终回复",
            1,
            2
        ));
    }

    #[test]
    fn empty_or_changed_body_never_finishes_a_turn() {
        assert!(!web_turn_completion_ready(true, "", "", 20, 20));
        assert!(!web_turn_completion_ready(true, "旧回复", "新回复", 20, 20));
        assert!(!web_turn_completion_ready(
            false,
            "最终回复",
            "最终回复",
            20,
            20
        ));
    }

    #[test]
    fn assistant_capture_ignores_the_baseline_and_stable_body() {
        assert!(!should_capture_assistant(
            false,
            "上一条回复",
            "上一条回复",
            ""
        ));
        assert!(should_capture_assistant(
            true,
            "与上一条相同",
            "与上一条相同",
            ""
        ));
        assert!(should_capture_assistant(
            false,
            "当前回复新增内容",
            "上一条回复",
            "当前回复"
        ));
        assert!(!should_capture_assistant(
            false,
            "当前回复",
            "上一条回复",
            "当前回复"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn external_navigation_only_launches_http_urls() {
        assert!(super::open_external_url_allowed(
            &"https://example.com".parse().expect("https URL")
        ));
        assert!(super::open_external_url_allowed(
            &"http://example.com".parse().expect("http URL")
        ));
        assert!(!super::open_external_url_allowed(
            &"mailto:test@example.com".parse().expect("mailto URL")
        ));
        assert!(!super::open_external_url_allowed(
            &"file:///C:/Windows/win.ini".parse().expect("file URL")
        ));
    }

    #[test]
    fn cancel_releases_the_slot_without_allowing_the_old_poll_loop_to_publish() {
        let state = DeepSeekWebState::default();
        state
            .begin("old-request".into(), "conversation".into())
            .expect("first request");
        assert_eq!(
            state.cancel(),
            Some(("old-request".into(), "conversation".into()))
        );
        assert!(state.cancelled("old-request"));
        state
            .begin("new-request".into(), "conversation".into())
            .expect("new request can start immediately");
        assert!(state.cancelled("old-request"));
        assert!(!state.cancelled("new-request"));
    }
}
