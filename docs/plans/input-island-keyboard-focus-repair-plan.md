# 输入岛单击后无法输入：Windows 焦点修复施工文档

状态：待实施。基线为 `codex/startup-render-handoff` 的 `1254879`（2026-09-25）。本文件是交给下游 agent 的施工要求，不代表修复或真机验收已经完成。

## 1. 目标与已确认的根因

目标：用户正在其他应用中工作，随后**单击可见的壁纸输入岛**，即可输入文字；Enter/Ctrl+Enter 按现有设置发送。无需先双击切换表／里桌面，也不能靠多点一次空白桌面恢复。

2026-09-25 在 Windows 真机复现，运行的是源码调试版 `target/debug/dsh-wallpaper.exe`，壁纸宿主当时挂在 `Progman`。只读采样同时记录了 `WindowFromPoint`、`GetForegroundWindow`、前台线程的 `GetGUIThreadInfo(0).hwndFocus` 和壁纸线程的焦点。没有记录键入内容、窗口标题或会话正文。

| 阶段 | 鼠标／键盘命中 | Windows 前台及其键盘焦点 | 结果 |
| --- | --- | --- | --- |
| 07:48:04–10，单击输入岛所在壁纸区域 | `WindowFromPoint` 返回壁纸的 `Chrome_RenderWidgetHostHWND`，其父链经过 `WRY_WEBVIEW → Tauri Window → Progman` | 前台仍为原应用或 `Shell_TrayWnd`；前台焦点为原应用或 `Windows.UI.Input.InputSite.WindowClass` | 鼠标可命中 WebView，键盘没有转交给壁纸 |
| 07:48:09，按 X | 壁纸线程自身的焦点仍显示为 WebView2 子窗口 | **全局前台**仍为 `Shell_TrayWnd`，其焦点仍是任务栏输入窗口 | X 未进入输入框 |
| 07:48:10–12，表／里桌面切换后 | 日志出现 `desktop focus restored to the wallpaper WebView`，再按 X | 前台变为 `Progman`，全局焦点变为 WebView2 子窗口 | 输入恢复 |

因此这次故障的主因已由真机区分出来：**线程内 `SetFocus` 成功，不等于该线程取得 Windows 全局前台键盘通道。** 正常点击已命中 WebView，不能把这次故障归咎于交互热区漏点；Enter 无响应是输入框没有收到按键的下游症状。

当前根窗口是 `Progman` 子窗口，实测扩展样式中**没有** `WS_EX_NOACTIVATE` 位。不要把“去掉 `WS_EX_NOACTIVATE`”写成修复。安放壁纸时使用 `SWP_NOACTIVATE`／`SW_SHOWNA`，而普通点击该子窗口没有使它成为全局前台；这才是需要处理的交接边界。

## 2. 当前代码的失效链路

1. `wallpaper/src-tauri/src/windows_integration.rs`：`attach_to_workerw` 把背景窗口改成 `WS_CHILD` 并挂到 `WorkerW`／`Progman`，安放时使用不激活标志（约 1270–1335 行）。
2. 同文件 `interaction_subclass_proc` 的 `WM_NCHITTEST` 分支只决定点击是否穿透（约 1691–1740 行）。本次采样证明用户点击处命中了 WebView2 子窗口；该分支不是本次故障的主因，但修复必须继续遵守其热区边界。
3. `start_foreground_monitor` 每 120 ms 读取 `GetForegroundWindow`，只在前台窗口类为 `WorkerW`／`Progman`（或桌面窗口）时认定桌面在前台；仅在状态变为 true 时调用 `restore_desktop_keyboard_focus`（约 1771–1811 行）。单击输入岛后前台仍是原应用／任务栏，所以这条恢复路径没有触发。
4. `restore_desktop_keyboard_focus` 对 Tauri 背景根 HWND 调用 `SetFocus`（约 1814–1851 行）。现有“focus restored”日志只能证明该 API 返回成功，不能证明 `GetGUIThreadInfo(0).hwndFocus` 已指向 WebView2，也不能证明页面收到 `keydown`。
5. `wallpaper/src/App.tsx` 的 `composerEpoch` 重建仅由 `desktopForeground` 的 false→true 边沿触发（约 335–354 行）；`wallpaper/src/features/chat/ConversationBubble.tsx` 的 `textarea.focus()` 也依赖挂载或该边沿（约 139–163 行）。当前台一直停留在原应用，这些前端补救不会执行；即使执行，DOM 焦点也不能单独转移 Windows 全局前台。
6. 表／里桌面切换会改变 Explorer 图标层和壁纸交互状态；真机采样中它使前台变为 `Progman`，现有恢复路径随后才生效。不要继续把“切换后能用”解释成发送逻辑或输入组件本身修好了。

## 3. 施工顺序

### A. 先做最小诊断，验证可接收的原生点击事件

- 在现有根窗口子类或输入岛 `pointerdown` 通道中，确认哪一个事件能在**真实左键单击输入热区**时可靠触发。先试 `WM_MOUSEACTIVATE`／相关鼠标消息；WebView2 子窗口属于另一个进程，不能假定父窗口一定收到其 `WM_LBUTTONDOWN`。若改走前端事件，原生命令仍须核验当前物理左键、光标在有效交互热区内，且光标下的 HWND 父链属于壁纸，不可把任意 IPC 调用当作用户点击。
- 诊断只记录时间、窗口类、PID、HWND、热区命中、前台／前台焦点和调用结果；不记录按键、窗口标题、输入内容或会话。调试埋点限开发构建或开关控制，不增加常驻高频日志。
- 记录输入岛单击前后与表／里桌面切换后的状态。以**全局前台焦点**为最终判据；壁纸线程本地 `GetFocus` 或单次 `SetFocus` 成功不算验收。

### B. 处理一次由用户点击授权的前台交接

- 把激活动作绑定到上一步验证过的输入岛真实点击，尽可能在该输入事件的窗口线程／时序内处理。当前 120 ms 的前台轮询不能作为唯一触发器；延迟到普通定时器后，Windows 对前台切换的限制可能已经改变。
- 使承载键盘输入的窗口取得实际前台资格，再把焦点交给 WebView2 的真实输入窗口／控制器及页面 `textarea`。具体采用现有 HWND、`WM_MOUSEACTIVATE`／Win32 前台 API，还是一层独立且只覆盖输入岛的可激活顶层窗口，应由本机实验决定；**不得仅追加 `SetFocus`、`textarea.focus()`、React remount 或循环重试后宣称完成**。
- 首选在现有结构内完成，并验证 `GetForegroundWindow`、`GetGUIThreadInfo(0).hwndFocus` 与页面 `document.activeElement`。若 `WorkerW`／`Progman` 子窗口无法稳定获得全局键盘通道，停止该路线，提交一次受限的顶层交互层方案及其窗口层级、热区和多屏风险，再实施。不要在失败路径上静默吞掉激活失败。
- 只允许壁纸本地 `background` 表面请求该能力；若增加 Tauri 命令，同步更新 `build.rs`、对应 capability／permission，并保留原生的真实点击与窗口归属核验。远程 DeepSeek WebView 不得获得此能力。

### C. 保持输入状态与桌面行为

- 原有 `chatDraft` 跨输入框重建保留；修复过程中不得丢失半输入文本、光标位置、中文输入法合成状态。避免在每次前台轮询或每个按键上重建 React 组件。
- 非热区、桌面图标、任务栏、右键菜单、设置窗口、全屏应用均不得因壁纸被动显示而被抢焦点。没有真实输入岛点击时，不调用全局激活路径。
- 现有 `WM_NCHITTEST` 的 `HTTRANSPARENT`／里桌面 `HTCLIENT`、区域 session/revision 守卫、负坐标多屏与 DPI 换算必须继续成立。不要扩大整张壁纸的可点击面积。
- 将现有“desktop focus restored”日志改成准确的分级结果：仅 `SetFocus` 成功应称“本线程焦点设置成功”；只有前台及全局焦点读回符合预期才报告“键盘通道恢复”。

## 4. 文件级改动边界

| 文件 | 预期工作 |
| --- | --- |
| `wallpaper/src-tauri/src/windows_integration.rs` | 找到真实点击事件；核验光标和交互热区；执行受限前台交接并读回结果。必要时保留 `start_foreground_monitor` 作为状态观察，不再把它当唯一修复入口。 |
| `wallpaper/src-tauri/src/lib.rs`、`wallpaper/src-tauri/build.rs`、`wallpaper/src-tauri/capabilities/background.json` | 仅当使用前端到原生请求时增加最小 IPC 入口与权限。Lite 和远程网页窗口不得获得新命令。 |
| `wallpaper/src/App.tsx`、`wallpaper/src/features/chat/ConversationBubble.tsx` | 单击输入框后的 DOM 焦点交接与草稿保留；移除验证无效的重复重建／重复聚焦代码。 |
| `wallpaper/src/runtime/interactionRegions.ts` | 复用现有热区数据。只有实测表明 CSS 移动后几何会失效时才补充刷新触发；本次样本已命中 WebView，不得先大改热区系统。 |
| `wallpaper/tests/`、Rust 对应测试模块 | 覆盖点击守卫、状态转换和页面草稿／快捷键；真实 Windows 前台行为另做真机验收。 |

当前工作树已有其他未提交的首帧、多屏和 Lite 改动。先运行 `git status --short`，只修改上述焦点相关区域；保留已有改动，不 reset、不顺手格式化整文件，不把无关内容混进本任务提交。

## 5. 验证与完成门槛

自动检查：`pnpm typecheck`、`pnpm test`、`cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --lib`，以及 Lite 检查。测试应固定“只有验证过的左键热区点击才请求激活”“点击空白桌面不抢焦点”“失败不误报成功”“草稿与 Enter/Ctrl+Enter 语义不变”。这些测试**不能替代**下面的真机点击验收。

Windows 真机至少完成以下检查，并保留不含正文的前后状态记录：

1. 从 Chrome、DSH 桌面端、任务栏各自成为前台的状态开始，单击输入岛一次，输入 X；每条路径连续往返至少 20 次，无需双击表／里桌面。点击后全局键盘焦点必须落在 WebView2 输入链，页面可见文字。
2. 输入半句后切到别的窗口，再单击输入岛；草稿和光标可继续使用。分别试中文输入法合成、Enter 与 Ctrl+Enter。发送回调每次只触发一次。
3. 点击桌面空白、图标、任务栏及普通应用窗口时，壁纸不得主动夺取前台；图标选择、桌面右键、表／里桌面双击继续正常。
4. 在当前 `Progman` 降级宿主与独立 `WorkerW` 宿主各验证一次；覆盖主副屏、负坐标或不同 DPI、锁屏往返和设置窗口关闭后的输入。无法获得某种宿主时如实记录未测，不宣称全场景通过。
5. 开发版通过后再验证实际安装版的进程路径和同样的点击链路。只看构建、单测或“`SetFocus` 返回成功”都不能勾选真机完成。

若前台交接会导致 Explorer 图标层失灵、覆盖其他应用、全屏时抢焦点，或只在单屏／单次尝试中有效，则停止打包，保留诊断记录并给出下一方案。交付时列出改动文件、失败与成功样本、未测边界；本任务不负责 DSH 连接、自启、工作区路径迁移、注册表或证书变更。
