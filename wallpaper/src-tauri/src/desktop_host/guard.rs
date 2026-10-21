// 主线程专属任务（读 Tauri 运行期窗口状态）的显式标记。
//
// **为什么要这套东西**：`get_webview_window` / `Manager` 取窗口这条路会 `Webview::clone`，
// 而它在 wry 里动的是 `DetachedWebview` 的引用计数 —— 和事件循环的 `Context` 同一份共享
// 状态。从后台线程取一次就等于并发改它，破坏是滞后的：先在后台线程打坏，很久以后主线程
// 在窗口消息里拿垃圾指针崩掉（0.2.0 与 0.4.7 两份转储的故障指令与寄存器布局同构，见
// `dsh-wallpaper.exe.8376.dmp` 的 `Context::clone` -> `__fastfail(7)`）。
//
// 跨线程**安全**的东西只有两类，其余一律要在主线程做：
//   * `AppHandle` 的 `emit` / `emit_to` / `state` / `try_state` / `run_on_main_thread`
//     （`run_on_main_thread` 只是往事件循环投递一条消息，不阻塞调用方，见
//     `tauri-runtime-wry` 的 `send_user_message`）；
//   * **纯值**：`HWND` 这样的句柄整数，连同已经算好的尺寸/缩放因子。
//   `WebviewWindow` 句柄本身**不是**纯值 —— 克隆它就是 `Context::clone`。
//
// 跨线程性本身没法在单测里跑（测试线程不是主线程），所以把"谁必须在主线程"这件事做成
// 下面这套显式、可测的标记 + 防线。
#[cfg(windows)]
std::thread_local! {
    /// 事件循环线程在执行主线程闭包时立起；线程局部量天然按线程隔离，于是"标记可见"
    /// 就等价于"我现在正在那个闭包里执行"。
    static MAIN_THREAD_WINDOW_STATE_RUNNING: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    /// **重入**标记：`run` 的调用栈还没退出时又来一次，说明这不是真正的事件循环线程，
    /// 而是一个复用了标记的后台线程（那正是要抓的错法）。
    static MAIN_THREAD_WINDOW_STATE_ACTING: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// 在主线程上执行主线程专属的 `f`（读 Tauri 运行期窗口状态），并立起可自证的标记。
///
/// 撤销走 RAII：闭包里的 `return`（调用点就有几处提前返回）和 panic 都必须把标记还回去，
/// 否则污染会留在线程局部量里，后面全变成假阳性。
#[cfg(windows)]
// 这条防线只抓一种错法：**后台线程带着主线程标记**（有人把后台工作塞进主线程闭包，或反过来复用了标记）。
// 它**抓不到**另一种：后台线程不带标记、直接去碰运行期窗口状态 —— 那种写法照样会 clone wry 的引用计数而不报警。
// 悬浮球那次就是漏在盲区里：`cursor_on_desktop_surface_via_label` 在球线程里取了 `background` 窗口对象。
// 所以纪律不是"有防线就安全"，而是：**取窗口对象只能在主线程做**；线程里只允许出现纯值句柄（见句柄槽位）。
pub(super) fn run_main_thread_window_state_task(f: impl FnOnce()) {
    let previous_running = MAIN_THREAD_WINDOW_STATE_RUNNING.with(|flag| flag.replace(true));
    let reentrant = MAIN_THREAD_WINDOW_STATE_ACTING.with(|flag| flag.replace(true));
    assert!(
        !reentrant,
        "主线程标记重入：读运行期窗口状态的闭包又派发了一次，调用栈不是事件循环"
    );
    let _restore_acting = ThreadLocalFlagRestore::new(|| {
        MAIN_THREAD_WINDOW_STATE_ACTING.with(|flag| flag.set(false))
    });
    let _restore_running = ThreadLocalFlagRestore::new(move || {
        MAIN_THREAD_WINDOW_STATE_RUNNING.with(|flag| flag.set(previous_running))
    });
    f();
}

/// 此刻是否正跑在主线程（= 事件循环线程）的闭包里。
#[cfg(windows)]
fn is_running_on_main_thread() -> bool {
    MAIN_THREAD_WINDOW_STATE_RUNNING.with(|flag| flag.get())
}

/// 后台线程的入口防线：这条线程此刻绝不能带着"我在主线程"的标记。
///
/// 命中即说明有人把后台工作塞进了主线程闭包（或反过来复用了标记）。日志用 `error`
/// 是为了在任何构建里都留痕；调试构建再断言一次，让写错的人立刻看到。
#[cfg(windows)]
pub(crate) fn assert_not_acting_as_main_thread() {
    if is_running_on_main_thread() {
        log::error!(
            "线程亲和性违例：后台线程被当成主线程使用；读运行期窗口状态的代码必须在事件循环线程上"
        );
        debug_assert!(
            false,
            "后台线程带着主线程标记运行：读运行期窗口状态的代码必须在事件循环线程上"
        );
    }
}

/// 线程局部标记的撤销器（见 [`run_main_thread_window_state_task`]）。
#[cfg(windows)]
struct ThreadLocalFlagRestore<F: FnOnce()> {
    restore: Option<F>,
}

#[cfg(windows)]
impl<F: FnOnce()> ThreadLocalFlagRestore<F> {
    fn new(restore: F) -> Self {
        Self {
            restore: Some(restore),
        }
    }
}

#[cfg(windows)]
impl<F: FnOnce()> Drop for ThreadLocalFlagRestore<F> {
    fn drop(&mut self) {
        // `FnOnce` 在 `Drop` 里只能取一次：用 `Option` 换出来。
        if let Some(restore) = self.restore.take() {
            restore();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这一组盯的是「后台线程不许碰 Tauri 运行期窗口状态」这条规矩的可测部分。
    ///
    /// 崩溃取证：`dsh-wallpaper.exe.8376.dmp` 里后台线程在 `Context::clone` 命中引用计数
    /// 溢出哨兵（`ud2`），调用链是
    /// `Context::clone` <- `Webview::clone` <- `AppManager::get_webview` <- `get_webview_window`
    /// <- `start_desktop_workspace_monitor` 的线程闭包。跨线程性本身在单测里跑不了（测试线程
    /// 不是主线程），所以测的是把这件事显式化的那个辅助函数本身。
    #[cfg(windows)]
    #[test]
    fn a_background_thread_carrying_the_main_thread_marker_is_flagged() {
        assert!(!is_running_on_main_thread());
        let offending = std::thread::spawn(|| {
            MAIN_THREAD_WINDOW_STATE_RUNNING.with(|flag| flag.set(true));
            // 后台线程带着主线程标记：这正是 0.4.7 转储里那条线程的处境。
            let flagged = std::panic::catch_unwind(assert_not_acting_as_main_thread).is_err();
            MAIN_THREAD_WINDOW_STATE_RUNNING.with(|flag| flag.set(false));
            flagged
        })
        .join()
        .expect("探针线程不应 panic");
        assert!(
            offending,
            "后台线程带主线程标记时必须被防线抓住：抓不住就说明防线没接线"
        );
    }

    /// 标记不能漏出去：闭包里的提前 `return` 是真实写法（主线程派发点就有几处），
    /// 漏出去之后所有后台线程都会变成假阳性。
    #[cfg(windows)]
    #[test]
    fn the_main_thread_marker_is_cleared_when_the_task_returns_early() {
        let marker_was_visible = std::cell::Cell::new(false);
        run_main_thread_window_state_task(|| {
            marker_was_visible.set(is_running_on_main_thread());
            // 提前退出：`run` 的撤销必须是 RAII，否则标记留在线程局部量里。
            if marker_was_visible.get() {
                return;
            }
        });
        assert!(
            marker_was_visible.get(),
            "任务体里必须看得到主线程标记；看不到说明标记没立起来，防线是空的"
        );
        // 任务体是提前 `return` 退出的，标记必须已经还回去。
        assert!(!is_running_on_main_thread());
        assert_not_acting_as_main_thread();
        // 第二次派发不能被误判成重入（`acting` 也必须干净）。
        run_main_thread_window_state_task(|| {});
        assert!(!is_running_on_main_thread());
    }
}
