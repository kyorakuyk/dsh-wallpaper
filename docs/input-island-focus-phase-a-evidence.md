# 输入岛焦点修复：A 阶段真机取证记录

对应施工文档：`input-island-keyboard-focus-repair-plan.md` §3.A。
记录时间：2026-09-25 00:04。埋点为开发构建专用，已确认编译不进 release。

本文件只记录窗口类名、PID、句柄与判定结果，不含按键内容、窗口标题、输入正文或会话内容。

## 1. 埋点触发情况

一次真实点击输入岛后，子类过程记录到的消息仅一条：

```
focus trace [WM_ACTIVATE/WM_SETFOCUS]:
  point_hit=None  foreground=Progman  foreground_pid=30940  focus=Tauri Window
```

**未出现 `WM_MOUSEACTIVATE`，也未出现 `WM_LBUTTONDOWN`。**

结论：鼠标消息由另一进程的 WebView2 子窗口接收，不会进入我们的子类过程。
因此「在这次点击里请求激活」没有原生入口，触发点必须走前端输入岛 `pointerdown`，
再经原生命令核验后执行交接（文档 §3.A 已预留该路径及其核验要求）。

## 2. 失败状态的精确描述

```
foreground = Progman (Explorer)
GetGUIThreadInfo(0).hwndFocus = 我们的 WebView
```

即：**键盘焦点已在 WebView 上，但全局前台是 Progman（Explorer 的桌面窗口）**。
按键因此被前台窗口吃掉。这解释了「单击无反应」与「Enter 也无反应」。

对照「能用」时的实测状态：

```
foreground = Tauri Window (dsh-wallpaper)
GetGUIThreadInfo(0).hwndFocus = Chrome_WidgetWin_1 (msedgewebview2)
```

两种可用状态的共同点：**前台属于桌面层级（壁纸自身或 Progman），键盘通道才能下传到 WebView**。

## 3. 分级日志已按 §3.C 生效

旧日志在 `SetFocus` 返回成功时报「restored」，把失败的修复伪装成成功。现改为读回判定：

```
keyboard channel restored: foreground is the wallpaper and focus is on the WebView
thread focus set, but the keyboard channel is not restored: foreground_is_wallpaper=false focus_on_webview=true
```

本次实测触发的正是后一句 —— 分级确实把假成功拦住了。

## 4. 宿主挂载状态（与文档 §1 一致）

```
background 0x5C155E attached to WorkerW 0x101B2
Explorer 未枚举独立 WorkerW，暂以 Progman 作为壁纸降级宿主：0x101B2
wallpaper host degraded: mode=progman generation=2 recoveries=1
```

真实宿主 `0x5C155E` 是 Progman 的子窗口，**文档 §1 的前提成立**。

一次独立枚举曾误得「顶层窗口、无父窗口」，原因是 `EnumWindows` 不返回子窗口；
该结论已作废，不作为任何改动依据。教训：**测量工具本身的能力边界会伪装成事实。**

## 5. 对 B 阶段的推论

「双击切换桌面可用」的机制是：它使 **Progman 成为前台**，键盘通道沿 Progman 下传到子窗口，
WebView 因而收到按键。这与 §2 的状态描述一致。

因此 B 阶段应做的不是继续在外围追加 `SetFocus`／DOM `focus()`／重挂载，而是
**由一次经核验的真实点击触发的前台交接**，并读回 `GetForegroundWindow`、
`GetGUIThreadInfo(0).hwndFocus` 与页面 `document.activeElement` 作为验收依据。

是否需要在现有结构内完成、还是需要受限的顶层交互层，取决于交接能否稳定生效；
若 Progman 子窗口路线无法稳定取得全局前台，按 §3.B 停止该路线并另提方案。
