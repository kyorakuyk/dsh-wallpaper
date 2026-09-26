# 输入模型取证：桌面上的一次点击归谁

记录时间：2026-09-26（进程时钟）。被测进程：已安装 `0.2.0.85`（`dsh-wallpaper.exe` pid 37432，WebView2 子进程 pid 40620/40556）。
复现脚本：`input-probe/zorder.ps1`、`input-probe/desktop-tree.ps1`、`input-probe/webview-uia.ps1`、`input-probe/who-owns-the-click.ps1`
（前四个只读、不修改任何窗口；`input-probe/capsule-click-experiment.ps1` 会合成点击，需显式 `-Run`）。

本文回答三个问题，并把「点击流向」写成可复述的模型：

1. 桌面图标为什么能点——是谁让点击穿透到 Explorer 的？
2. 立绘 / 输入岛为什么能点——是谁在那些地方把穿透关掉的？
3. 胶囊为什么两样都不行——既收不到自己的点击，又不像「点到了桌面」？

方法：先读代码定位候选机制，再用窗口栈、Z 序、UI Automation 实测逐个排除。所有结论标注 **实测 / 代码 / 推断 / 未知**。

---

## 1. 分层与 Z 序（实测）

`GetShellWindow()` → `Progman 0x000101BA`（explorer, pid 13628）。用 `GetWindow(GW_CHILD)` + `GW_HWNDNEXT`
走**真实兄弟 Z 序**（不是 `EnumChildWindows` 的扁平遍历），Pid 37632 一侧为我们自己：

| # | 窗口 | 归属 | 可见 | 备注 |
| --- | --- | --- | --- | --- |
| 0 | `SHELLDLL_DefView` (0x00020186) | explorer | 是 | 图标层容器，全屏 `(0,0)-(2560,1600)` |
| 1 | `DSHWallpaperNativeBootstrap` (0x002508D4) | 我们 | **否** | `WS_EX_TRANSPARENT`，首帧遮罩 |
| 2 | `Tauri Window` "DSH Wallpaper" (0x00560BA4) | 我们 | 是 | **壁纸 WebView 宿主**，全屏 |
| 3 | `WorkerW` (0x00040956) | explorer | 是 | 空宿主 |

`SHELLDLL_DefView` 的子窗口 `SysListView32`（"FolderView"）同样全屏 `(0,0)-(2560,1600)`、可见——
它就是桌面图标与桌面右键菜单的接收者。

**结论（实测）**：壁纸宿主是 Progman 的**直接子窗口、且排在图标层之后（下面）**。
这正是 `place_wallpaper_layers()` 中 `SetWindowPos(background, Some(icon_view), …)` 的意图
（代码：`windows_integration.rs` 约 1230–1237 行，「把壁纸放到桌面图标层后方」）。

## 2. 点击流向（模型）

Windows 的命中测试只问**该点上 Z 序最靠前的可见窗口**。于是：

### 表桌面（DefView 可见）

```
物理点击
  → 命中测试：该点最前可见窗口 = SysListView32（Explorer，在壁纸之上）
  → 鼠标消息交给 Explorer（图标选择 / 右键菜单 / 桌面空白）
  → 壁纸 WebView 收不到任何鼠标消息
  → DOM 事件（click / pointerdown / mouseenter）永不发生
```

`interaction_subclass_proc` 的 `WM_NCHITTEST` 分支在这里**不会被调用**：它装在壁纸宿主上，
而宿主不是该点的最前窗口。这与既有取证一致：`docs/evidence/input-island-focus-native-route-closed.md`
第 1 节实测「`WM_MOUSEACTIVATE` / `WM_LBUTTONDOWN` / `WM_NCHITTEST` 一次都没有到达」。
当时的解释是「WebView2 子窗口属于另一个进程」；现在有了 Z 序，**更根本的原因是不该由我们命中**。

### 里桌面（DefView 被隐藏）

双击桌面空白进入里桌面时，`set_desktop_icons_visible(false)` 执行
`ShowWindow(SHELLDLL_DefView, SW_HIDE)`（代码：第 961–978 行）。隐藏的窗口不参与命中测试，
于是 Progman 栈里最前的**可见**窗口变成 `Tauri Window`：

```
物理点击
  → 命中测试落到壁纸宿主 0x00560BA4
  → WM_NCHITTEST 子类命中 INNER_WORKSPACE_ACTIVE 分支 → 返回 HTCLIENT（1）
  → 正常客户区投递 → WebView2 renderer → DOM pointerdown
  → verify_island_click 核验（物理左键 + 热区 + HWND 父链属于壁纸）→ 前台交接 → 可输入
```

**这就是「键盘修复」那时能生效的全部前提：先进入里桌面，隐藏图标层。**

### 全局旁路：唯一能在表桌面看见点击的机制

`start_desktop_workspace_monitor`（代码：第 2365 行起）是一条 16ms 轮询线程，与窗口消息无关：

```
GetAsyncKeyState(VK_LBUTTON) 的下降沿
  && cursor_is_on_desktop_surface(background)   // 光标下 HWND 的父链能走到 Progman/桌面类
  && !cursor_hits_interaction_region(background) // 光标不落在任何已发布热区内
  && cursor_is_over_desktop_blank(automation)    // UIA ElementFromPoint 向上 4 层没有 ListItem
  ⇒ 500ms 内第二次 ⇒ 切表/里桌面
```

## 3. 已发布热区：来源、内容与坐标（代码 + 实测）

- **来源**：前端 `App.tsx` 把 `[data-interaction-region]` 元素的 `getBoundingClientRect()` 发布给原生
  （`src/runtime/interactionRegions.ts`），MutationObserver + resize 触发，rAF 去抖。
- **当前全部 8 个 region id**（代码）：`widget:<id>`、`auth`、`appearance-drawer`、
  `chat`（对话整体）、`chat-collapsed`（**胶囊**）、`chat-history`、`portrait`、`drawer`。
- **实测**：通过 UI Automation 直接读 WebView 的 DOM 树（`webview-uia.ps1`），当前处于折叠态：

  | 元素 | UIA 名称 | 物理矩形 |
  | --- | --- | --- |
  | 胶囊（折叠输入岛） | `展开 AI 对话` | `(1138,1454)-(1423,1521)`，285×67 |
  | 立绘 | `黑红幼年鲸鱼娘` | `(1957,906)-(2408,1537)`，451×631 |
  | 问候语 | `早上好！今天要做什么呢？` | `(2049,861)-(2305,888)` |

- **缩放因子实测为 1.5**：`styles.css` 中 `.portrait-slot{width:300px}`，实测立绘宽 451 物理像素 →
  451/300 = 1.503。故 `devicePixelRatio = 1.5`，发布的热区（CSS×1.5）确实是物理像素。
- **原点实测无偏移**：`ClientToScreen(壁纸宿主)` = `(0,0)`，WebView 客户区原点也是 `(0,0)`
  （宿主窗口矩形 `(-11,-2)-(2571,1611)`，多出的 11/2 像素是窗口边框，不在客户区内）。
  故 `local = 光标 - (0,0)`，与 DOM 坐标一一对应。

**已排除的两个假设（实测）**：

| 假设 | 判定 | 依据 |
| --- | --- | --- |
| 热区命令没有授权，前端调用被拒 | **排除** | `capabilities/background.json` 第 25–26 行已授予 `allow-begin-interaction-region-session`、`allow-update-interaction-regions` |
| 坐标/缩放换算错了，热区整体偏移 | **排除** | `ClientToScreen` = (0,0)；DPR = 1.5 |

## 4. 三问三答

**问 1：桌面图标为什么能点？**
不是「点击穿透过去了」，而是**图标层本来就在壁纸之上**（实测 Z 序 #0 对 #2）。
`WM_NCHITTEST` 里的 `HTTRANSPARENT` 分支在表桌面**根本没有机会执行**，它是一条死路。
所以过去「让点击穿透」的说法是描述性的，机制并不存在。

**问 2：立绘 / 输入岛为什么能点？**
只在**里桌面**能点：`ShowWindow(SHELLDLL_DefView, SW_HIDE)` 之后，壁纸宿主成为该点最前的可见窗口，
`WM_NCHITTEST` 命中 `INNER_WORKSPACE_ACTIVE` 分支返回 `HTCLIENT`，点击才落到 WebView 的 DOM。
表桌面下没有任何 DOM 点击入口。

**问 3：胶囊为什么两样都不行？**
胶囊是 `ConversationBubble` 折叠态的 `<button data-interaction-region="chat-collapsed">`，
它**有**热区（实测矩形见 §3），但：

- 表桌面上点击它的物理落点是 **SysListView32**，DOM 永远收不到 `click`，所以「点不动」；
- 它也确实**没有挡住桌面**——那次点击被 Explorer 处理了；
- 用户感到的「双击切了表/里」来自 §2 的全局旁路：当点击点不落在任何已发布热区内时，
  该机制把这次点击判成「桌面空白双击」。UI 上没有第二个判据能反驳它。

**架构结论（本节最重要）**：
> **壁纸 WebView 在表桌面无法收到任何鼠标消息。**
> 因此「表桌面单击胶囊 = 进入里桌面 + 弹出输入岛」**不可能用 DOM 事件实现**，
> 它必须做进那条已存在的原生全局线程：把胶囊看成「一个已发布热区」，
> 在该热区内发生真实左键单击时由原生切换工作区并通知前端展开。
> 同理，胶囊的悬停高亮、X 按钮、立绘点击，在表桌面都没有 DOM 入口。

## 5. 由模型推出的缺陷（待修，未修）

- **D1**：任何「只存在于表桌面」的交互控件（胶囊、X、立绘）都不可能被 DOM 事件触发。必须走原生热区线路。
- **D2**：`HTTRANSPARENT` 分支是死代码级复杂度：表桌面轮不到它，里桌面一律返回 `HTCLIENT`。
  应当把它的真实作用写清楚或删掉，避免下次又按它推理。
- **D3**：UIA「空白桌面」判据对我们自己的 WebView 也返回「空白」（它只往上看 4 层找 `ListItem`）。
  所以**已发布热区是唯一判据**：热区漏一处，那次 UI 点击就会被当成桌面空白双击（= ②「双击控件切桌面」）。
- **D4**：热区列表没有活性校验。列表为空/过期时不会报错，而是静默退化成「到处是桌面空白」。
  单次观测无法区分「热区正确」与「热区为空」。
- **D5**：热区只发布**容器矩形**，不发布「可交互后代矩形的并集」；且只由 DOM 变更/resize 触发，
  CSS transition/transform 造成的位移不会重发。渲染在容器矩形之外的弹出层（浮层、下拉、tips）
  天然没有热区。

## 6. 未闭合问题与关闭它的实验

**未知（必须实测才能定）**：真实点击发生时，原生手里的热区列表**到底包含什么**。
目前只能确认命令已授权、坐标换算正确、既有取证显示 `verify_island_click` 曾 22 次核验通过
（说明该列表在里桌面确实有效）；但**无法在运行中读出列表内容**——
该 WebView2 没有开远程调试端口（实测无监听套接字），日志级别为 Info 且当前日志文件为 0 字节。

**实验协议（需要桌面可见数秒，全自动、可逆）**：

1. 让桌面可见（最小化前台窗口），确认 `SHELLDLL_DefView` 可见（= 表桌面）。
2. 记录 `SHELLDLL_DefView` 可见性基线。
3. 用 `SendInput` 在**胶囊矩形中心** (1280,1487) 合成一次双击。
   - 图标层仍可见 ⇒ 热区判定正确，② 的成因不在胶囊；
   - 图标层被隐藏 ⇒ 复现 ②：热区判定失败，胶囊点击被当成桌面空白双击。
4. 再在胶囊下方 5px（`display-interaction-layer` 的空白处，任何热区之外）合成一次双击。
   - 预期切换 ⇒ 验证「不在热区即桌面空白」这条规则本身成立。
5. 若已进入里桌面，再于桌面空白处双击切回，并复核 `SHELLDLL_DefView` 恢复可见。

落点选在屏幕底部中央与下方，桌面图标在左侧，不会误选图标；步骤 5 保证状态可还原。

## 7. 复现命令

```powershell
# Z 序（前台→后台）：证明壁纸在图标层之下
& docs/evidence/input-probe/zorder.ps1
# 完整桌面栈 + 指定点由谁接收点击（需桌面可见时才有意义）
& docs/evidence/input-probe/desktop-tree.ps1 -ProbePid 37432 -ProbeX 1280 -ProbeY 1487
# 直接读 WebView 的 DOM 热区几何（物理像素）
& docs/evidence/input-probe/webview-uia.ps1 -Hwnd 0x000B0BFA -MaxElements 120
# §6 的实验（默认为 dry run，只有加 -Run 才会合成点击）
& docs/evidence/input-probe/capsule-click-experiment.ps1
```

## 8. 未验证声明

- 本文所有实测来自**已安装版 0.2.0.85** 与**当前这一次运行**；宿主形态为 Progman 降级模式
  （独立 WorkerW 宿主未测，其栈序可能不同）。
- `WindowFromPoint` 的逐点判定在写本文时无法执行：前台窗口全屏覆盖了桌面（实测该点为
  `FLUTTERVIEW` / DSH GUI）。§2 的表桌面结论由**Z 序 + 全屏覆盖的列表视图**推出，仍属**推断**，
  由 §6 的实验直接验证。
- 「里桌面才可点」这一条在本次会话中没有重新实测，依据是代码（`SW_HIDE`）与既有取证的组合。
- 胶囊热区是否真的被发布，仍是 §6 的未知。**（已由 §9 前的实验关闭：热区里有胶囊，判定准确。）**

---

## 9. 悬浮球增量 1：实测验收（2026-09-26，安装版 0.2.0.88）

背景与拍板见 [`../plans/interaction-handover.md`](../plans/interaction-handover.md) §3.1：胶囊从壁纸场景搬出来，
做成独立顶层窗口（"悬浮球"），因为壁纸宿主在图标层之下、表桌面收不到任何鼠标消息（§1–§4）。

测量方法：`input-probe/ball-popout-experiment.ps1`（扫过球的停靠位置，逐点记录归属窗口、是否算桌面表面、
球的矩形与是否上屏）+ `input-probe/ball-trigger-band-sweep.ps1`（底部地理）。两者都用
`input-probe/run-capsule-experiment-with-desktop.ps1 -Payload ...` 临时让开桌面，结束后自动还原窗口与光标。

| 契约要求 | 实测结果 |
| --- | --- |
| 顶层窗口（无父窗口） | `parent = none` ✓ —— 表/里双击监控因此天然忽略球上的点击 |
| 平时整体在屏幕之外 | 基线矩形 `(1118,1604)-(1442,1712)`，屏幕高 1600 ⇒ 0 像素可见 ✓ |
| 样式 `TOOLWINDOW\|NOACTIVATE`，不置顶、不 `TRANSPARENT` | 自报 `exstyle=0x08000190`（NOACTIVATE\|TOOLWINDOW\|WINDOWEDGE\|CONTROLPARENT），无 TOPMOST/TRANSPARENT ✓ |
| Z 槽：图标层之上、普通应用之下 | 自报 `z-slot ok front=IME back=Progman`（球的**下一个** Z 邻居就是 shell 窗口）✓ |
| 鼠标靠近才弹出 | 停在 `y=1530` 首次上屏；`y=1400`（热区外 4px）不上屏 ✓ |
| 弹出后能收到真实鼠标输入 | 球心 `(1280,1474)` 的命中窗口沿父链可上溯到球窗口 = True ✓ |
| 全程不抢前台 | 弹出前后 `GetForegroundWindow` 不变 ✓ |
| 停靠位置稳定、底边贴任务栏上沿 | 多次采样矩形恒为 `(1118,1420)-(1442,1528)`；底边距屏幕下边缘 72px = 一个任务栏高度 ✓ |

### 两个只有实测才会暴露的坑（都已修，勿回退）

1. **热区不能取"屏幕最底下几像素"**。本机是自动隐藏任务栏：光标一到下边缘，任务栏升起接管整条底带
   （实测 `y=1560` 起归属 `MSTaskSwWClass`，父链是 `Shell_TrayWnd`），而 `cursor_is_on_desktop_surface`
   只认 `Progman`/`WorkerW`（`is_desktop_foreground_class`），任务栏一律判否——于是球在最该弹出的位置
   **永远弹不出来**（实测 39 次采样、0 次上屏）。改为「球弹出位置所在矩形外扩 16px」为热区：
   该区域在任务栏升起线之上、属于桌面，判据可以通过。
2. **显示中不得因任务栏留白变化而挪位**。任务栏升起的瞬间 `desktop_layout_metrics` 报
   `taskbar_visible=true`，留白从 48 逻辑像素翻倍到 96，球弹出后会自己向上跳一个任务栏高度
   （实测同一位置先后读到 `(1118,1420)-(1442,1528)` 与 `(1118,1348)-(1442,1456)`）。
   球恰恰是因为光标碰过底部才弹出，这条抖动必然发生。现在显示期间沿用弹出那一刻的 `shown_y`，
   只有主屏尺寸/缩放真的变化才重新落位。

### 仍未验证

- **观感**：透明是否真的透过、胶囊外观与圆角形状，需要人眼确认；探针只能证明窗口在那里、能收点击、
  不抢前台。`exstyle` 里**没有** `WS_EX_LAYERED`——若观感不对，第一个要看的就是它。
- 「最大化应用盖住球 ⇒ 看不见」（= 只在桌面显现）没有逐场景实测，依据是 Z 槽夹在 Progman 与普通应用之间。
- 任务栏常显、多屏、负坐标三种形态未测。
