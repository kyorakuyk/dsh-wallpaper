#[cfg(windows)]
use crate::app_core::{AppAction, AppCore};
#[cfg(windows)]
use tauri::{Emitter, EventTarget, Manager};

/// The wallpaper host is the sole consumer of system, tray, and desktop
/// workspace events.  Keep native notifications out of the settings WebView:
/// it has no need to observe chat/session state or to receive a future event
/// carrying privacy-sensitive desktop metadata.
///
/// 悬浮球监控线程也要用这个标签：它在**主线程**解析桌面宿主的句柄并带成整数进线程
/// （见 `floating_ball::start_ball_monitor`），标签单点定义在这里，免得那边写死一份字符串。
#[cfg(windows)]
pub(crate) const BACKGROUND_WINDOW_LABEL: &str = "background";

#[cfg(windows)]
pub(crate) fn emit_to_background<S: serde::Serialize + Clone>(
    app: &tauri::AppHandle,
    event: &str,
    payload: S,
) {
    let _ = app.emit_to(
        EventTarget::webview_window(BACKGROUND_WINDOW_LABEL),
        event,
        payload,
    );
}

/// Tell the wallpaper host about a real session or power transition, and leave a
/// line behind.
///
/// These four events decide whether the desktop leaves the sleeping portrait, and
/// the log used to say nothing about them: a wake that did not take effect could
/// not be told apart from Windows never telling us, so the only available answer
/// was a guess. Silence in the log now means the event never arrived; a line
/// means it did, and the renderer owned the outcome.
#[cfg(windows)]
pub(crate) fn emit_system_session(app: &tauri::AppHandle, transition: &'static str) {
    log::info!("系统会话事件：{transition}");
    emit_to_background(app, "system-session", transition);
}

#[cfg(windows)]
pub(crate) fn dispatch_system_action(app: &tauri::AppHandle, action: AppAction) {
    let Some(core) = app.try_state::<AppCore>() else {
        return;
    };
    let snapshot = core.dispatch(action);
    emit_to_background(app, "app-snapshot", &snapshot);
}
