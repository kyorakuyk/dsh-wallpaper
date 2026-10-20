use tauri::WebviewWindow;

#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DwmExtendFrameIntoClientArea, DwmGetWindowAttribute, DwmSetWindowAttribute,
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_COLOR_NONE,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
        },
        Gdi::ClientToScreen,
    },
    UI::{
        Controls::MARGINS,
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetClientRect, GetWindowLongPtrW, GetWindowRect, SetWindowPos, GWL_STYLE, HTBOTTOM,
            HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_ACTIVATE,
            WM_NCACTIVATE, WM_NCDESTROY, WM_NCHITTEST, WS_CAPTION, WS_THICKFRAME,
        },
    },
};

#[cfg(all(windows, not(feature = "lite")))]
use windows::Win32::UI::WindowsAndMessaging::WM_NCCALCSIZE;

#[cfg(windows)]
pub fn configure_settings_window(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = HWND(window.hwnd().map_err(|error| error.to_string())?.0);
    // SAFETY: `hwnd` is the settings window handle just resolved from the live Tauri window; SetWindowSubclass also takes a static proc, a constant subclass id and no reference data, so nothing Rust-owned crosses the boundary.
    unsafe {
        if !SetWindowSubclass(hwnd, Some(settings_window_subclass), 0x4453_4853, 0).as_bool() {
            return Err("无法监听设置窗口边框状态".into());
        }
    }
    // 子类**先装**，再强制一次框架重算。
    //
    // 顺序很关键（实测 0.2.0.97 才知道）：Windows 会把客户区矩形**缓存**起来，
    // 只在 `WM_NCCALCSIZE` 时重算；而 tao 创建窗口时（我们的子类还不存在）已经算过一遍，
    // 于是子类里的 `WM_NCCALCSIZE → 0`（去掉非客户区）永远等不到消息，
    // 客户区一直是 `(11,2)` 的旧值。`SWP_FRAMECHANGED` 会逼出这次重算。
    // SAFETY: the same live settings window handle; None is passed as the insert-after window and every size is zero, so no pointer is borrowed and a failure is only logged.
    unsafe {
        if let Err(error) = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        ) {
            log::warn!("settings window: forcing a frame recalculation failed: {error}");
        }
    }
    apply_settings_window_frame_policy(hwnd)?;
    // 这行现在同时是自检：客户区起点必须是 (0,0)，否则说明非客户区还在。
    log_settings_window_metrics(hwnd, "configure");
    Ok(())
}

#[cfg(windows)]
fn log_settings_window_metrics(hwnd: HWND, stage: &str) {
    // SAFETY: `hwnd` is the settings window the caller just configured; the RECT and POINT out-parameters are local stack values alive across these synchronous calls, and each is read only after its call succeeded.
    unsafe {
        let mut outer = RECT::default();
        let mut client = RECT::default();
        let mut origin = POINT::default();
        if GetWindowRect(hwnd, &mut outer).is_ok()
            && GetClientRect(hwnd, &mut client).is_ok()
            && ClientToScreen(hwnd, &mut origin).as_bool()
        {
            log::info!(
                "settings frame {stage}: outer=({},{} {}x{}), client-origin-offset=({},{}), client={}x{}",
                outer.left,
                outer.top,
                outer.right - outer.left,
                outer.bottom - outer.top,
                origin.x - outer.left,
                origin.y - outer.top,
                client.right - client.left,
                client.bottom - client.top,
            );
        }
    }
}

/// 无客户区之后，自己补的缩放命中带宽度（逻辑像素）。
///
/// Windows 的可调边框在 150% 缩放下实测是 11 物理像素（≈7.3 逻辑像素），这里取 8 逻辑像素，
/// 与系统手感一致。
#[cfg(windows)]
const SETTINGS_RESIZE_BAND_PX: i32 = 8;

#[cfg(windows)]
fn apply_settings_window_frame_policy(hwnd: HWND) -> Result<(), String> {
    // SAFETY: `hwnd` is the live settings window; each Dwm* call receives a pointer to a local value whose type matches the attribute, together with that value's exact size, and every local outlives the synchronous call.
    unsafe {
        // The rounded outline is rendered by the settings web UI. Windows 11
        // otherwise adds its own one-pixel DWM border around the transparent
        // top-level HWND, which shows up as a white halo outside the CSS curve.
        let border_color = DWMWA_COLOR_NONE;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            std::ptr::from_ref(&border_color).cast(),
            std::mem::size_of_val(&border_color) as u32,
        )
        .map_err(|error| format!("无法关闭设置窗口系统边框：{error}"))?;

        // A decoration-less yet resizable Tao window still owns a two-pixel
        // non-client strip at its top. Give DWM the exact opaque host color
        // so it cannot expose the desktop/white fallback during repaints.
        // COLORREF is 0x00BBGGRR: #0d1625.
        let caption_color: u32 = 0x0025_160d;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            std::ptr::from_ref(&caption_color).cast(),
            std::mem::size_of_val(&caption_color) as u32,
        )
        .map_err(|error| format!("无法设置设置窗口顶部颜色：{error}"))?;

        // 深色框架：本机 Windows 是浅色主题，DWM 用**系统浅色**画窗口框架
        // （实测：左/右/下各 11 物理像素亮边；顶部因为设置了 caption 颜色所以是暗的）。
        // 那圈亮框正是用户说的"尖角难看"——它方，而且与深色面板对比强烈。
        // 打开沉浸式深色模式后，框架与边框按深色绘制，与面板同族。
        let dark_mode: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark_mode).cast(),
            std::mem::size_of_val(&dark_mode) as u32,
        )
        .map_err(|error| format!("无法把设置窗口边框切到深色：{error}"))?;

        // 把系统框架"吃进"整个客户区（-1 = 整窗玻璃）。
        //
        // 不加这一步，可调边框占的那 11 像素永远在客户区之外，CSS 只能画到客户区为止，
        // 深色面板外面必然留一圈系统色的框。扩展之后客户区覆盖整个窗口矩形，面板可以
        // 一直画到边缘；DWM 仍在外面套上系统圆角与阴影，并保留边框上的拖拽命中区
        // （所以窗口依旧可缩放）。
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        DwmExtendFrameIntoClientArea(hwnd, &margins)
            .map_err(|error| format!("无法把设置窗口系统框架扩展到客户区：{error}"))?;

        // 圆角交回 Windows 11 自己决定（`DWMWCP_DEFAULT` 对普通窗口就是圆角），
        // 不再用 `DWMWCP_DONOTROUND` 把它关掉。
        //
        // 起因：用户看到设置中心是**尖角**。原因是 `.settings-window` 用不透明底色
        // 铺满整个方窗，盖住了内层 `.settings-app` 的圆角；而这里又把 DWM 圆角关掉，
        // 于是没有任何一层在画圆角。
        //
        // 现在两边对齐：DWM 用系统默认半径（Windows 11 为 8 逻辑像素），
        // `SettingsWindow.css` 里的 `.settings-app` 也用 8px 且根节点改为透明。
        // 两边半径一致，才不会出现"CSS 曲线之外露出桌面"的第二道弧。
        let corner_preference = DWMWCP_DEFAULT;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner_preference).cast(),
            std::mem::size_of_val(&corner_preference) as u32,
        )
        .map_err(|error| format!("无法启用设置窗口系统圆角：{error}"))?;

        // 读回一次：圆角到底生效没有，必须是日志里能一眼看到的结论。
        // 同时打上窗口样式：DWM 只对带框架的窗口套用系统圆角，样式里没有
        // WS_THICKFRAME / WS_CAPTION 时"圆角没生效"就是这一条的原因。
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as isize;
        let thick_frame = style & WS_THICKFRAME.0 as isize != 0;
        let caption = style & WS_CAPTION.0 as isize != 0;
        let mut applied: u32 = 0;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_mut(&mut applied).cast(),
            std::mem::size_of_val(&applied) as u32,
        )
        .is_ok()
        {
            let round = match applied {
                0 | 2 => "rounded (system default)",
                3 => "rounded small",
                1 => "DO NOT ROUND",
                _ => "unknown",
            };
            log::info!(
                "settings frame: corner preference read back = {applied} ({round}); style=0x{style:X} thick_frame={thick_frame} caption={caption}"
            );
        }
    }
    Ok(())
}

/// 无客户区窗口的缩放命中测试：把窗口边缘 `SETTINGS_RESIZE_BAND_PX` 逻辑像素报成边框。
///
/// 去掉了非客户区（见 `settings_window_subclass` 的 `WM_NCCALCSIZE`），Windows 就不再
/// 提供"拖边改大小"的命中区，这里补回来。只在贴边那一圈生效，其余返回 `None`，
/// 调用方原样交回默认结果——**绝不能在客户区内部乱报**，否则会吞掉 WebView 的点击。
#[cfg(windows)]
unsafe fn resize_edge(hwnd: HWND, lparam: LPARAM) -> Option<u32> {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return None;
    }
    // lparam 是屏幕坐标的打包值，低 16 位 x、高 16 位 y（各自带符号）。
    let packed = lparam.0 as u32;
    let x = (packed as u16 as i16) as i32;
    let y = ((packed >> 16) as u16 as i16) as i32;
    let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
    let band = (SETTINGS_RESIZE_BAND_PX as f64 * scale).round().max(1.0) as i32;

    let left = x - rect.left;
    let right = rect.right - x;
    let top = y - rect.top;
    let bottom = rect.bottom - y;
    if left < 0 || top < 0 || right < 0 || bottom < 0 {
        return None; // 点在窗口之外（例如阴影区），交给默认处理
    }
    let (west, east) = (left <= band, right <= band);
    let (north, south) = (top <= band, bottom <= band);
    Some(match (west, east, north, south) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => return None,
    })
}

#[cfg(windows)]
unsafe extern "system" fn settings_window_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    // 去掉非客户区：让**客户区等于整个窗口矩形**。
    //
    // 为什么必须这么做（实测，0.2.0.96）：只要窗口还留着标准框架，客户区就永远比窗口矩形
    // 小 `(11,2)`（左/右/下 11 物理像素、顶 2 像素，150% 缩放）。CSS 只能画到客户区为止，
    // 于是深色面板外面必然留一圈由 **系统主题** 决定颜色的框——浅色主题下就是用户看到的
    // 那圈白（"尖角难看"的真正来源）。改 caption/border 颜色只是在跟这圈框较劲，
    // 换主题就又会不一致。
    //
    // 非客户区归零之后：面板可以一直画到窗口边缘；DWM 依然按 `DWMWCP_DEFAULT` 在外面套
    // 系统圆角与阴影（`DwmExtendFrameIntoClientArea(-1)` 就是为无框架窗口保留这些而设的），
    // 而系统圆角半径与 `.settings-app` 的 8px 一致，两道弧重合。
    //
    // 代价与补偿：框架消失后 Windows 不再提供"拖边改大小"，所以缩放改由**渲染层**发起
    // （`SettingsWindow.tsx` 的 `.settings-resize-*` 边条调 `startResizeDragging`）。
    // 为什么不能像过去那样靠窗口自己的命中测试：WebView2 渲染窗口属于**另一个进程**、
    // 铺满整个客户区，鼠标命中测试归它，父窗口的 `WM_NCHITTEST` 根本不会被问到——
    // 这与桌面输入模型里那条实测结论是同一个原因（见
    // `docs/evidence/input-model-desktop-hit-testing.md`）。下面 `resize_edge` 只在
    // 客户区之外/未被 WebView 覆盖的地方还会被用到，留着作为兜底。
    //
    // **Lite 版不加**：Lite 的设置界面是另一条工作流的文件，没有配套的边条，
    // 去掉框架会让它既没边框也不能缩放。
    #[cfg(not(feature = "lite"))]
    if message == WM_NCCALCSIZE && wparam.0 != 0 {
        return LRESULT(0);
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    match message {
        // 自绘的缩放边框：非客户区没了，命中测试得自己给。
        // 只报边缘那 8 逻辑像素，其余一律交回默认结果，避免吞掉 WebView 的点击。
        WM_NCHITTEST => {
            if let Some(edge) = resize_edge(hwnd, lparam) {
                return LRESULT(edge as isize);
            }
        }
        // DWM may restore the active accent border after processing either of
        // these messages. Reapply the policy after the default window proc.
        WM_ACTIVATE | WM_NCACTIVATE => {
            let _ = apply_settings_window_frame_policy(hwnd);
            log_settings_window_metrics(hwnd, "activate");
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(settings_window_subclass), 0x4453_4853);
        }
        _ => {}
    }
    result
}

#[cfg(not(windows))]
pub fn configure_settings_window(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
}
