# Bridge 能否让官方桌面壳「启动即隐藏」——只读取证

> 2026-09-29 只读取证。本文件回答一个明确问题：**运行在 DSH 宿主运行时内部的 Bridge 插件，能否让官方桌面壳（`DeepSeek Harness.exe`）的窗口在启动时就保持隐藏，而不是先出现再被隐藏**。调查过程未修改任何代码、未启动或停止任何进程、未安装任何包；唯一写入是本文件本身。
>
> 取证对象是本机当前安装的官方桌面版：`D:\Family\dsh-official`，其运行时为 `0.2.0-rc.1`。

## 引用约定

- `app.asar!/lib/main.js:11521` 表示 `D:\Family\dsh-official\resources\app.asar` 内条目 `lib/main.js` 的第 11521 行（按该条目文本的换行计数，不是磁盘字节偏移）。
- `app.asar!/dsh/node_modules/...` 表示 asar 内的运行时包树。
- 源码检出指 `D:\Family\DeepSeekHarness\deepseek-harness`（`C:\DeepSeekHarness.old\deepseek-harness` 为同一提交的另一份拷贝，两者 HEAD 均为 `47f943859bef60e4160492346772ded9b24f765a`，2026-08-13）。
- 「扫描结果」指本次在内存中遍历 asar 条目并用正则逐文件匹配得到的结论，命令见文末第 8 节。

## 0. 结论

**不能，今天不存在这条路径。** 窗口的可见性完全由 Electron 主进程自己决定：主进程先用 `show: false` 创建窗口，等宿主运行时子进程通过私有 IPC 发来 `ready` 之后，才调用 `window.show()`。Bridge 运行在那个子进程内，既没有任何插件可见的「窗口/托盘/应用生命周期」服务，也没有第二条能触达主进程的通道；唯一的子进程 → 主进程通道（`process.send`）只被 `@deepseek-ai/dsh-desktop-host` 一个包使用，且只发送四种固定类型的消息。

置信度：**高**，仅限「本机安装的 `0.2.0-rc.1` 官方桌面版」。依据是 asar 内完整可读的壳主进程代码（`/lib/main.js`，11830 行、未压缩）与对运行时全量 JS 的穷尽扫描；不确定性来自第 7 节列出的未读部分，而不是证据强度。

一句话补充：**「窗口先出现再被隐藏」不是 Bridge 的实现缺陷，而是当前架构的必然结果**——发起点（壁纸）和窗口所有者（闭源壳主进程）之间隔着两个进程边界，且其中一条边界方向不可用。

## 1. 壳的进程结构与启动时序（事实基础）

| 事实 | 证据 |
|---|---|
| 壳是 Electron 应用，主进程代码在 asar 的 `/lib/main.js` | `app.asar!/lib/main.js:5` 唯一一处 Electron 导入：`import { BrowserWindow, Menu, Notification, Tray, WebContentsView, app, clipboard, dialog, ipcMain, nativeImage, nativeTheme, net, powerMonitor, protocol, session, shell, syst…` |
| 主窗口用 `show: false` 创建 | `app.asar!/lib/main.js:10600` `function createWindow(preload, show = false, primary = false)`；`:10606` `show,`；`:11520-11521` `const createMainWindow = () => { const window = createWindow(appPreload, false, true);` |
| 主窗口的渲染进程被沙箱化，只有 contextBridge 白名单 | `app.asar!/lib/main.js:10625-10633` `webPreferences: { preload, nodeIntegration: false, contextIsolation: true, sandbox: true, webSecurity: true, webviewTag: primary, devTools: true }` |
| 宿主运行时是**子进程**，通过 Node 的 `ipc` 通道通信 | `app.asar!/lib/main.js:3673-3691`：入口 `join(runtimeDir, "node_modules", "@deepseek-ai", "dsh-desktop-host", "lib", "index.js")`，`spawn(this.node, ["--expose-internals", …inspect, entry, runtimeDir, projectDir, primaryRuntime, …packageManager], { cwd: this.projectDir, env: desktopNodeEnvironment(...), stdio: ["ignore", "pipe", "pipe", "ipc"] })` |
| 子进程入口固定监听 19387，且不打开浏览器 | `app.asar!/dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js:231-235` `args: ["--no-open", "--port", "19387"]` |
| 真实的就绪 URL 由子进程用「实际端口」拼出来后再上报 | 同上 `:337` `const url = ctx.connection.authenticatedUrl(\`http://127.0.0.1:${String(ctx.webServer.port)}\`)`；`:338-344` `process.send({ type: "ready", url, injections: ctx.webServer.collectIndexInjections() }, …)` |
| 主进程的 `readyPromise` **没有超时**，只由 `ready` 消息解决 | `app.asar!/lib/main.js:3633-3634` `readyPromise = new Promise((resolve, reject) => { this.readyResolve = resolve;`；`:3704-3707` `if (message.type === "ready") this.readyResolve({ url: message.url, injections: message.injections })`；`:3729` `return this.readyPromise` |

### 1.1 启动顺序（关键：显示窗口的决定点在主进程，不在插件）

```text
主进程启动
  → createMainWindow():  createWindow(appPreload, false, true)        main.js:11520-11521
  → reconcileBackend():   await navigateMain(applicationUrl)          main.js:10954-10955
                          await backend.start(...)                     main.js:10956
  → backend.start():      spawn 宿主子进程，await 子进程 ready 消息    main.js:3671-3691 / 10831-10835
  ← 子进程:               process.send({type:"ready", url, injections}) dsh-desktop-host:338-344
  → backend.state.phase === "ready"
  → openInitialWindow():  readWelcomeState()；若 phase !== "ready" 直接返回 main.js:11648-11651
  → enterWorkspace():     await navigateMain(applicationUrl)
                          window.show() / window.showInactive()        main.js:11569-11576
```

也就是说：**壳已经「等运行时」了**——窗口只在 `ready` 之后才显示。它等的是「宿主进程报告就绪」，不是「前端画出第一帧」；`navigateMain` 只 await `window.loadURL(url)`（`app.asar!/lib/main.js:10814`），前端页面随后自己去调 `dshDesktopBoot.ready()` 拉 `streamBaseUrl`（`:11064-11072`）。这与本仓库 `codex/startup-render-handoff` 分支正在处理的「首帧交接」问题是同一时序的另一半。

补充：正常路径下不会先出现欢迎窗口之外的窗口。若登录态不足，`openInitialWindow` 走 `showWelcome()`（`main.js:11655-11661`），欢迎窗口是另一条路径，用 `ready-to-show` 显示（`main.js:10273` 是子模态窗口的 `ready-to-show`，欢迎窗口自身用 `openWelcomeWindow` + `welcomeWindowOptions`，其 `webPreferences` 见 `main.js:7525-7532`）。

## 2. Q1：宿主运行时与 Electron 主进程之间的 IPC 面（枚举）

存在一条 IPC，但它是**父子进程间的私有 Node IPC**，不是 Electron 渲染进程 IPC，也不是插件 API。

### 2.1 子进程 → 主进程（消息类型是闭集）

主进程用一个校验函数决定是否接受，形状不合法就杀掉子进程：

- `app.asar!/lib/main.js:3544-3549` `function isDesktopHostEvent(message) { … switch (candidate.type) { case "shutdown-complete": return true; case "ready": return typeof candidate.url === "string"; case "platform-session": { …`；`:3550-3560` 继续校验 `session` 的 `origin`/`token`/`userId`/`requestHeaders`（并显式拒绝 `authorization`、`x-dsh-auth-token`、`host`、`content-length` 等头）。
- `app.asar!/lib/main.js:3699-3701` `if (!isDesktopHostEvent(message)) { this.fail(new Error("dsh desktop host sent an invalid IPC event")); child.kill("SIGTERM"); return; }`

| 方向 | 消息 | 发送方证据 | 接收方证据 |
|---|---|---|---|
| 子 → 父 | `{ type: "ready", url, injections }` | `dsh-desktop-host:338-344` | `main.js:3704-3707` → `readyResolve({url, injections})` |
| 子 → 父 | `{ type: "platform-session", session }` | `dsh-desktop-host:331-336` | `main.js:3708` → `onPlatformSession` |
| 子 → 父 | `{ type: "shutdown-complete" }` | `dsh-desktop-host:258-262` | `main.js:3709` |
| 子 → 父 | `{ type: "fatal", message, diagnostic }` | `dsh-desktop-host:346-364`（`main().catch`，`process.exitCode = 1`） | `main.js:3711` → `DesktopHostFatalError` |
| 父 → 子 | `{ type: "shutdown" }` | `main.js`（`stop()`/退出路径） | `dsh-desktop-host:263-268` |
| 父 → 子 | `{ type: "quit-inspection", requestId }` | `main.js:3750-3757` | `dsh-desktop-host:269-293` |
| 父 → 子 | `{ type: "update-tasks", requestId, action: "inspect"\|"lock"\|"unlock" }` | `main.js:3737-3744` | `dsh-desktop-host:294-318` |
| 子 → 父 | `{ type: "update-tasks"\|"quit-inspection", requestId, … , error? }` | `dsh-desktop-host:275-288`、`:303-315` | `main.js:3712-3716` 按 `requestId` 兑现 |

**没有任何一条消息与窗口可见性有关。** 遍历上表：`ready` 只带 `url` 与 `injections`，`platform-session` 只带账号会话，`shutdown-complete` 无负载，`fatal` 只带诊断文本。

### 2.2 渲染进程 ↔ 主进程（Electron IPC，按前缀分成五个家族）

| 家族 | 定义处 | 用途 |
|---|---|---|
| `dsh-desktop:*`（25 个 channel） | `app.asar!/lib/main.js:6193-6219`，如 `boot: "dsh-desktop:boot"`、`enterWorkspace: "dsh-desktop:enter-workspace"`、`windowsMenu: "dsh-desktop:windows-menu"` | 主窗口产品 API（引导、侧栏浏览器、目录选择、快捷键、更新、Windows 标题栏外观） |
| `dsh-platform:*`（5 个） | `main.js:5919` `const PLATFORM_IPC = { bootstrap, localeChanged, open, bounds, close }` | 内嵌 Platform 账号视图（`WebContentsView`） |
| `dsh-welcome:*`（9 个） | `main.js:7467-7476` | 欢迎/登录窗口（API key 保存、跳过、开始） |
| `dsh-desktop:mandatory-*`（3 个） | `main.js:8382-8385` | 强制更新遮罩 |
| `dsh-update-dialog:*`（3 个） | `main.js:8991-8994` | 更新对话框 |

主进程对所有产品 IPC 都校验来源：`main.js:6227-6230` `function assertDesktopSender(event, hostnames) { … new URL(senderFrame.url) … }`，产品通道一律写成 `assertDesktopSender(event, ["app"])`（例：`:11065` 引导、`:11469` Windows 菜单）；少数常驻监听（如 `nativeThemeSet`、`localeChanged`）用手写的 `event.sender !== mainWindow.webContents` 比较。允许的文档源是壳自己的自定义协议 `dsh-app://app`（`main.js:6220-6221` `const SCHEME = "dsh-app"`）。

### 2.3 渲染进程能拿到的全部能力（contextBridge 白名单）

`app.asar!/lib/preload-app.cjs:735-789` 的 `createProductApi()` 就是全部产品 API：

- `protocolVersion: 1`、`browser`（侧栏浏览器租约）、`deviceInfo`
- `keyboard.closeWindow(revision)` / `keyboard.subscribe(listener)`
- `shortcuts.get/edit/recording/subscribe`
- `updates.status/open/subscribe`

`preload-app.cjs:790-828` 另外暴露：`dshOnboarding`、`__DSH_DIRECTORY_PICKER__`、`__DSH_HOST_PATHS__`、`dshDesktopBoot`（`ready`/`failed`）、`dshPlatform`（`open`/`setBounds`/`close`）、`dshDesktop`、`__DSH_LOCALE__`。

**没有 `hideWindow`、`showWindow`、`setVisible`、`minimize`、`setSkipTaskbar` 之类的成员。** 在整棵运行时里搜 `hideWindow|showWindow|hideMainWindow|setSkipTaskbar|minimizeToTray` 只命中壳自己的 `/lib/main.js:11512`（见第 8 节扫描结果）。

唯一形似「隐藏窗口」的调用是 `keyboard.closeWindow`，它无法被用来抢先隐藏：

- `app.asar!/lib/main.js:9942-9946` `ipcMain.handle(DESKTOP_IPC.shortcutsCloseWindow, (event, expected) => { const window = assertSender(event); if (expected !== revision || revision === void 0 || recording || !window.isFocused() || !window.isEnabled() || overlayInput(window).blocked) return; window.close(); });`
- 它要求 `expected === revision`（壳自己的快捷键版本号）且窗口**已获得焦点**；窗口尚未显示时不可能满足。
- 即使调用成功，`close` 也只是「收起到托盘」：`main.js:11526-11538` 的 `window.on("close")` 会 `event.preventDefault()` 并调用 `hideMainWindow(window)`（`:11512-11519`，最终 `window.hide()`）。

## 3. Q2：DSH 插件 API 是否暴露桌面 / 窗口 / 托盘 / 生命周期服务

### 3.1 Bridge 实际消费的宿主服务（本仓库侧）

- `bridge/src/host.ts:173-181`：`export const HOST_ADAPTER_SERVICES = ['agentDefaultModel','agentPresets','agents','webServer','workspaceRegistry','permissionPresets','commands'] as const`
- `bridge/src/index.ts:38`：`export const inject = HOST_ADAPTER_SERVICES …`
- 可选服务只有 `llm`，用 `ctx.get('llm')` 探测（`bridge/src/index.ts:290-291`）
- Bridge 对宿主唯一的「出口」是注册 HTTP 路由：`bridge/src/index.ts:1365`、`:1452`、`:1574` 三处 `webServer.register({...})`
- Bridge 没有任何 index.html 注入或客户端代码（在 `bridge/src/index.ts` 中搜 `collectIndexInjections|transformIndex|index.html` 无命中）

结论：Bridge 的宿主面是**纯 HTTP 路由 + 会话/模型/工作区服务**，其中没有一个与桌面壳有关。

### 3.2 运行时侧是否存在桌面类服务（穷尽扫描）

对 `app.asar!/dsh/node_modules/@deepseek-ai/` 下全部 1200 个 `.js/.cjs/.mjs`（<4 MB）逐文件扫描：

| 搜索目标 | 结果 |
|---|---|
| `Tray` / `nativeImage` / `trayIcon` | 仅 `app.asar!/lib/main.js:5`（壳自身）；运行时树 0 命中 |
| `BrowserWindow` / `webContents` / `ipcMain` / `ipcRenderer` / `contextBridge` | 仅 `/lib/main.js` 与 `/lib/preload-*.cjs`；运行时树 0 命中 |
| `"electron"` 模块引用 | 在 `@deepseek-ai` 子树中 **0 命中** |
| `process.send(` / `process.connected` | `dsh-desktop-host/lib/index.js` 7 处（`:249,:253,:261,:332,:338,:354,:363`）+ `dsh-host-directory-picker-native/lib/worker.cjs:230`（该处是 node worker_threads 的父子通道，不是壳通道） |
| `hideWindow` / `showWindow` / `hideMainWindow` / `setSkipTaskbar` | 运行时段 0 命中（全树唯一命中是壳的 `/lib/main.js:11512`） |

也就是说，**整个宿主运行时进程里没有任何代码持有 Electron 对象**；窗口是另一个进程的私有状态。

`dsh-desktop-host` 内部对 `ctx` 的全部用法（这是子进程里唯一知道「自己是被 Electron 拉起来的」的地方）：

- `:105-146` `installDesktopUpdateTaskControl(ctx)`：`ctx.effect`、`ctx.on("connection/request")`、`ctx.get("agents")`、`ctx.get("jobs")` —— 是**闭包**，不是服务
- `:157-179` `installDesktopQuitInspection(ctx)`：同样的 `ctx.get("agents")`/`ctx.get("jobs")`
- `:187-212` `installPlatformSessionPublisher(ctx, publish)`：`ctx.inject(["deepseekAccount"], …)` 后把账号会话通过 `publish` 交给 `process.send`
- `:323-330` `const { ctx } = await application; control.updateTasks = …; control.quitInspection = …; await ctx.plugin(office_exports, {…})`

这四个函数都没有 `ctx.provide(...)` / `super(ctx, 'name')`，因此**没有被注册成任何 Cordis 服务**；插件无法通过 `ctx.get('…')` 或 `inject` 拿到它们。

### 3.3 官方文档侧的插件服务面

- `deepseek-harness/docs/user/develop/framework/service.md:17`：「Any plugin can provide a service for other plugins to consume.」
- 同上 `:26-32` `export const inject = ['tools']`；`:36-53` 用 `class X extends Service { super(ctx, 'name') }` 提供；`:96` 可选依赖用 `ctx.get()`。
- 同上 `:141-143`：「The repository generates the service names, public methods, and source locations into each service's subsystem page … do not maintain a second static list.」——即服务清单由生成的子系统页承载（`deepseek-harness/docs/subsystems/`）。
- 在 `deepseek-harness/docs/subsystems/` 中与桌面壳相关的条目不存在；与「宿主进程能碰到的外部世界」相关的只有 `web-server.md`（HTTP 载体）、`subprocess.md`（子进程）、`sandbox.md`。`docs/subsystems/web-server.md:5` 还写明：「It serves browsers only: Electron loads the built files over `file://` and sends fetch requests through an IPC bridge instead of this server.」——**这句话与本机 `0.2.0-rc.1` 壳的实际行为不一致**（壳实际用 `dsh-app://app/` 从 `dsh-web-frontend/dist` 静态供给首页与 assets，其余请求转发给宿主，`app.asar!/lib/main.js:11046-11055`），说明该文档描述的是壳的另一代实现，不能当作当前壳的规格。
- `deepseek-harness/packages/host/` 下的包名为：`apiproxy`、`directory-picker`、`directory-picker-auto`、`directory-picker-browse`、`directory-picker-native`、`frontend-static`、`plugin-inventory`、`webserver`（命令输出，见第 8 节）。没有 desktop/window/tray 包。

结论：**插件 API（公开检出与运行时段都算）里没有桌面、窗口、托盘或应用生命周期服务；这一层根本不存在，不是「有但没文档」。**

## 4. Q3：壳是否已经等运行时再显示窗口，外部能否影响这个决定

### 4.1 已经等了

见第 1.1 节：`createMainWindow()` → `navigateMain(applicationUrl)` → `backend.start()`（等子进程 `ready`）→ `openInitialWindow()` → `enterWorkspace()` → `window.show()`（`app.asar!/lib/main.js:11648-11662`、`:11569-11576`）。

`openInitialWindow` 还有一道显式门禁：`main.js:11651` `if (isQuitting() || backend.state.phase !== "ready") return;`。

### 4.2 但没有任何外部输入能改变「显示」这个动作

| 可能的外部影响面 | 取证结果 |
|---|---|
| 启动参数 | 全文件只有一处读 `argv`：`app.asar!/lib/main.js:10723` `let raiseAfterUpdate = process.platform === "win32" && process.argv.includes("--updated");` —— 仅影响更新后是否置顶，不涉及隐藏。壳自身没有 `--hidden` 之类开关 |
| 环境变量 | 壳主进程只读 11 个 `DSH_DESKTOP_*`：`DSH_DESKTOP_DEV_APP`(11687)、`DSH_DESKTOP_DIAGNOSTIC_FILE`(11821)、`DSH_DESKTOP_DSH_DIR`(10562)、`DSH_DESKTOP_HOST_INSPECT_PORT`(10571)、`DSH_DESKTOP_MANDATORY_UPDATE_CONFIG`(11763)、`DSH_DESKTOP_OPEN_DEVTOOLS`(11587、11813)、`DSH_DESKTOP_PNPM_ENTRY`(10561)、`DSH_DESKTOP_PRIMARY_RUNTIME_DIR`(10566)、`DSH_DESKTOP_UPDATE_HTTP_IDLE_TIMEOUT_MS`(6942)、`DSH_DESKTOP_UPDATE_JOURNAL_DIR`(10709)。**没有一个与窗口可见性有关** |
| 配置文件 | 搜 `settings.json|config.json|readJsonFile` 无命中；`app.getPath("userData")` 只有两处用途：快捷键存储（`main.js:11058`）与「已确认后台驻留」标记文件 `background-close-confirmed`（`main.js:11451`）。**没有窗口可见性配置** |
| 开机自启类设置 | 搜 `getLoginItemSettings|setLoginItemSettings|openAtLogin|loginItem` **0 命中** —— 壳自身不管理登录项，也没有「最小化启动」偏好 |
| IPC 消息 | 上表 2.1 的四种消息里没有任何可见性字段；`ready` 在主进程侧只被解出 `url` 与 `injections`（`main.js:3704-3707`）。校验函数 `isDesktopHostEvent` 会忽略多余字段，但也**没有任何代码读取它们** |
| 反方向的外部控制（存在，但只能「显示」） | `main.js:11687-11691` `app.setAsDefaultProtocolClient("dsh"); app.on("open-url", (event, url) => { … if (url === "dsh://open" || url === "dsh://open/") focusPrimaryWindow(); })`；`focusPrimaryWindow`（`:11663-11686`）会 `window.restore()` + `window.show()` + `focus()` |

### 4.3 插件唯一能影响窗口时机的机制（不是可行特性）

主进程等待 `ready` 的 promise 没有超时（`main.js:3633-3634`、`:3729`），因此如果宿主 profile 的某个插件在 `apply()` 里永久挂起，`application` 不会 resolve，`process.send({type:"ready"})` 永不发出，窗口就**永不显示**。这是「让窗口不出现」的机制，但它是把壳的启动挂死（并最终由用户看到无窗口进程），不是「静默启动隐藏运行」；Bridge 也不该把这个当成特性使用。它说明的是**没有可用的正向通道**，而不是有。

## 5. Q4：如果今天不存在，最小且诚实的上游改动是什么

### 5.1 改动必须同时落在两侧，而其中一侧是闭源的

- 宿主侧（公开、可读）：`dsh-desktop-host/lib/index.js:338-344` 是 `ready` 的唯一发送点，`:231-235` 是固定启动参数。
- 壳侧（**闭源**）：`app.asar!/lib/main.js:3544-3560` 校验、`:3704-3707` 消费、`:11569-11576` 显示。
  - 该壳不在公开检出里：`deepseek-harness/apps/` 只有 `cli` 与 `web`（命令输出）；`packages/` 中所有包名里没有 desktop/electron 包（第 8 节命令）。
  - `@deepseek-ai/dsh-desktop-host` 在公开检出里也不存在（在 `packages/**/package.json` 的包名中不出现），且**未发布到 npm**：`registry.npmjs.org/@deepseek-ai/dsh-desktop-host` 返回 404；asar 内该包 `package.json` 带 `"private": true`（`app.asar!/dsh/node_modules/@deepseek-ai/dsh-desktop-host/package.json`）。
  - 对照：`@deepseek-ai/dsh` 是公开发布的，`dist-tags` 为 `latest: 0.1.7-rc.2`、`next: 0.2.0-rc.1`（本条为 `registry.npmjs.org` 命令输出），与本机安装的 CLI（`%APPDATA%\npm\node_modules\@deepseek-ai\dsh\package.json` 为 `0.2.0-rc.1`）一致。

因此「最小改动」的形状是一个**双面设计**，类似仓库已有的 directory-picker 先例：

1. 宿主 → 壳的方向：在 `ready` 消息里增加一个可见性/呈现意图字段（例如 `presentation: { window: "hidden" }`），由主进程在 `openInitialWindow`/`enterWorkspace` 阶段消费，改为「不调用 `show()`」或 `showInactive()`；或在之后的进程生命周期内响应新的父 → 子控制请求。
2. 插件 API 方向：新增一个宿主侧能力 seam（Service Definition + Provider），让插件能声明「本次启动不要抢占前台」，并由桌面版 profile 的组合层把它接到第 1 步的消息字段上。参考 `.agents/notes/implemented/architecture/2026-07-28-directory-picker-capability-seam.md:13`（三包 seam：Service Definition + 两个后端）与 `:47`「A future interaction (or an Electron provider of the `native` interaction) is one dual-face backend package — no gateway surgery, no ui-workspace edits.」；`:28` 更直接写着：「The native backend stays. Plugin-form was the point: multiple providers can serve the seam (**an Electron shell would provide the `native` interaction through its own dialog API**).」
3. 已有落地样例证明「壳给渲染进程一个全局、客户端插件读它」是本仓库接受的形态：壳 preload 暴露 `__DSH_DIRECTORY_PICKER__`（`app.asar!/lib/preload-app.cjs:807`），客户端包读取它（`app.asar!/dsh/node_modules/@deepseek-ai/dsh-client-ui-directory-picker-native/lib/client.js:63` `const desktop = globalThis.__DSH_DIRECTORY_PICKER__;`）；`__DSH_HOST_PATHS__` 同理（`dsh-client-ui-conversation/lib/client.js:17911`）。注意这仍受 `sandbox/contextIsolation` 约束（`main.js:10625-10633`），且它是**渲染进程**里的全局，不是宿主进程的服务。

规模评估：**中等偏大，且需要 DeepSeek 官方配合。** 至少涉及：(a) 闭源壳的一个 IPC 字段与一处 `show()` 决策；(b) 一个宿主侧 seam 包（Service Definition）；(c) 桌面 profile 的组合层接线；(d) 文档与测试档位（`docs/testing.md:49` 要求新增能力 seam 在计划阶段就指明每一层覆盖）。仅靠本仓库无法完成其中任何一项。

### 5.2 上游流程（文档原文）

- `deepseek-harness/CONTRIBUTING.md:9`：「DeepSeek Harness is still at an early stage and under active development. We are sorry that we **cannot accept external pull requests at the moment**.」
- 同上 `:11-12`：参与方式是 GitHub Discussions（提问、报缺陷、投票）。
- 同上 `:13-15`：生态参与方式是发布自己的插件并给 GitHub 项目打 `dsh-plugin` topic。
- 同上 `:19-21`：「We do not believe that packages in the official repository are inherently more important than packages created by the community. You may consider this repository an idea, an official showcase, and a source of inspiration, but not a mandate from us.」
- 仓库内部的设计提案流程是 Agent Note：`.agents/notes/README.md:9-14` 定义 `proposed/`→`implemented/`→`rejected/` 生命周期；`:46`「Every non-trivial change MUST add or update at least one Agent Note in the same PR.」该流程服务仓库内贡献者，外部没有 PR 通道，因此也不能作为壁纸侧的上游路径。
- 补充一条与本问题相关的官方口径：已发布的 CLI README（`%APPDATA%\npm\node_modules\@deepseek-ai\dsh\README.md:20`）写明「The `desktop` name is reserved for the Electron-owned profile, so the CLI rejects boot, config-dump, and plugin-management requests for it.」——桌面 profile 归 Electron 壳所有，CLI 侧不能引导/管理它。

## 6. Q5：与「壁纸侧启动后隐藏」的比较与建议

先说一个容易混淆的前提：**壁纸受管启动的 DSH 与官方壳不是同一条路径。** 本仓库施工文档把受管启动描述为「受管 DSH CLI → 127.0.0.1:3080」（`docs/plans/dsh-harness-connection-autostart-compatibility-plan.md:29`），而 CLI 是纯 Node 应用启动器（`%APPDATA%\npm\node_modules\@deepseek-ai\dsh\README.md:5`：`The dsh command is the sole supported Node application launcher`）。那条路径上根本不存在 Electron 窗口，「隐藏窗口」只是**用户另开官方壳**时才出现的问题。

| 维度 | 上游新通道（真正「启动即隐藏」） | 壁纸侧启动后隐藏（当前在做的方案） |
|---|---|---|
| 今天是否可用 | 不可用：需闭源壳改动 + 宿主新 seam；无外部 PR 通道（CONTRIBUTING.md:9） | 可用，全部落在本仓库 |
| 用户体验 | 窗口从头到尾不可见，最干净 | 至少可能出现窗口闪现；闪现时长取决于壁纸命中 `show()` 之后的时机 |
| 成本 | 需求上游排期；三处以上改动 + 文档/测试 | 本仓库实现与验证；需处理与壳 `show()` 的竞态 |
| 主要风险 | 排期不可控；壳每次更新都可能改（本仓库已在 asar 里见过文档与实现漂移，见 `docs/subsystems/web-server.md:5` 与 `main.js:11046-11055` 的矛盾） | 竞态漏帧；隐藏被用户手动唤起（托盘/`dsh://open`/再次启动 exe）打断时的状态复位 |
| 可测性 | 依赖上游 | 本仓库可做真机日志，与其他启动证据同一体系 |

### 6.1 建议

1. **主路线：壁纸侧隐藏（继续当前实现）**，理由是它在今天可交付，而另一条路线需要 DeepSeek 改闭源壳且没有外部贡献通道。
2. **把「隐藏」做成对壳窗口的显式状态管理，而不是一次性抢时机**。本次时序取证给出一个可利用的窗口：主窗口的 HWND 在 `createWindow`（`app.asar!/lib/main.js:10600`、`:11521`）时就已存在并被 `show:false` 保持隐藏，而 `show()` 要等到宿主子进程 `ready` 之后（`:11574`，中间包含整个运行时启动过程）。因此在壳进程刚起来、窗口还不可见的那一段时间里，壁纸有很大的机会先拿到 HWND 并在其上先手操作（例如移出可视区/先保持隐藏），使壳随后调用的 `show()` 不产生用户可见的一帧。**这一条是依据时序证据提出的候选做法，本轮未做真机验证**（本任务为只读）；它必须由实现方用真机日志确认，且要验证是否会干扰壳自身的窗口几何/布局记忆。
3. **回退/降级**：若无法消除闪现，就明确降级为「启动后尽快隐藏 + 托盘/`dsh://open` 可恢复」，并在壁纸侧如实提示「官方壳会短暂出现」，不要把它写成无缝。壳的可恢复入口是托盘点击与菜单项（`main.js:10366-10393`、`:11440-11445`）与 `dsh://open`（`:11687-11691`）。
4. **不作为主路线的备选**：不要尝试用「让宿主不发送 `ready`」来阻止窗口出现（第 4.3 节）。它会把壳的启动挂死，属于防御性失败而非特性。

### 6.2 若要走上游路线，什么必须为真

- 官方愿意在 `ready` 消息（或新的父 → 子控制请求）里携带窗口呈现意图，并在 `enterWorkspace` 处尊重它；
- 或者官方愿意提供一个公开的桌面壳能力 seam（Service Definition + Electron Provider），并在 `desktop` profile 的默认组合层接线；
- 壁纸侧需要一个稳定的「本次启动不要抢占前台」意图表达，且 Bridge 能把它从插件侧透传（今天 Bridge 的宿主面没有这个位置）；
- 在此之前，任何「Bridge 可以让壳启动即隐藏」的说法都不成立。

## 7. 我读不到 / 未验证的部分（诚实边界）

1. **壳的源码**。只能读 asar 里的打包产物 `/lib/main.js`（11830 行，未压缩）与 preload。原始 TypeScript、构建配置、以及"下一次发布是否改了这段"都不可知。
2. **`app.asar.unpacked`**。asar 表里有 1496 个条目没有数值 offset（`unpacked: true`），它们位于 `app.asar.unpacked` 目录，本次扫描跳过了这些条目。就本次结论而言影响很小（它们是原生二进制/资源，而不是 JS 逻辑），但不能说"整棵树的 JS 都读过了"——只能说"asar 内联的 9353 个可读文本文件读过了"。
3. **运行时机器码/原生模块**。`libreoffice-kit-win32-x64`（190 MB）等原生件未扫描，理论上不可排除其中存在与壳通信的代码；但它们不被任何桌面相关逻辑引用。
4. **真实运行期取证**。本任务未启动/停止任何进程，因此"窗口在第几毫秒显示"只有代码时序推断，没有本次实测时间线。父级此前的实测（监听 19387 的 pid 与窗口属主 pid 不同）与本文件的进程结构结论一致。
5. **`wallpaper/src-tauri/`**。该目录正被另一 Agent 修改，本任务约定不触碰，因此第 6 节对"壁纸侧隐藏"的描述基于机制与施工文档，而不是其当前实现代码。
6. **公开检出的版本差异**。两份检出同为 `47f9438`（2026-08-13），`docs/` 描述的部分行为（如 `web-server.md:5`）与本机 `0.2.0-rc.1` 壳不一致；本文件凡涉及壳行为的断言一律以 asar 实测为准，并以「文档 vs 实现漂移」明确标注。

## 8. 证据复现命令

以下命令均在 `D:\Family\DeepSeekHarness\plugins\dsh-wallpaper` 下用 `pwsh` + Node v22.23.2 执行；asar 只读打开，未写入磁盘。

asar 头结构（本机实测）：偏移 0 处 `UInt32LE = 4`；偏移 4 处为 header pickle 长度 `3407208`；偏移 12 处为 JSON 长度 `3407198`；JSON 起于偏移 16；文件数据起点 `8 + 3407208 = 3407216`；条目 offset 相对数据起点。

```js
// 读取 asar 内任意条目（只读、内存内）
const fs = require('fs')
const P = 'D:/Family/dsh-official/resources/app.asar'
const fd = fs.openSync(P, 'r')
const h = Buffer.alloc(16); fs.readSync(fd, h, 0, 16, 0)
const jsonLen = h.readUInt32LE(12), dataStart = 8 + h.readUInt32LE(4)
const jb = Buffer.alloc(jsonLen); fs.readSync(fd, jb, 0, jsonLen, 16)
const tree = JSON.parse(jb.toString('utf8'))
// 之后按 tree.files[...] 逐段下钻，Buffer.alloc(size) + readSync(fd, buf, 0, size, dataStart + Number(offset))
```

本次使用的关键检索（结果见正文各表）：

- 壳主进程：`ipcMain.handle\(`（30 命中）、`ipcMain.on\(`（5）、`webContents.send\(`（17）、`new BrowserWindow`（4）、`ready-to-show`（1）、`show: false`（2）、`new Tray\(`（1）、`requestSingleInstanceLock`（1）、`second-instance`（1）、`process.argv`（1）
- 运行时树：9353 个 `.js/.cjs/.mjs/.d.ts/.ts/.json`（78,858,270 字节，排除 `libreoffice-kit`）逐一匹配 `\bTray\b|nativeImage`、`BrowserWindow|webContents|ipcMain|ipcRenderer|contextBridge`、`hideWindow|showWindow|hideMainWindow|setSkipTaskbar|minimizeToTray`、`process\.send\(`、`dsh-desktop:|dsh-platform:`、`desktopHost|desktop-host`
- `@deepseek-ai` 子树穷尽：1200 个 JS 文件匹配 `process\.send\s*\(|process\.connected`（8 命中，7 处在 `dsh-desktop-host`）与 `["'`]electron["'`]`（0 命中）
- 壳主进程环境变量：正则 `process\.env\.([A-Za-z0-9_]+)` 全量提取（11 个不同名字，见 4.2）
- npm 侧（网络）：
  - `Invoke-WebRequest https://registry.npmjs.org/@deepseek-ai/dsh/latest` → `{"name":"@deepseek-ai/dsh","version":"0.1.7-rc.2"}`
  - `dist-tags` → `alpha: 0.1.7-alpha.2`、`latest: 0.1.7-rc.2`、`next: 0.2.0-rc.1`
  - `https://registry.npmjs.org/@deepseek-ai/dsh-desktop-host` → HTTP 404
  - `npm view @deepseek-ai/dsh-desktop-host version` → `npm error code E404`
- 检出侧：`git -C D:\Family\DeepSeekHarness\deepseek-harness remote -v` → `origin https://github.com/deepseek-ai/deepseek-harness.git`；`git log -1 --format='%H %ci %s'` → `47f943859bef60e4160492346772ded9b24f765a 2026-08-13 19:38:46 +0800 Merge pull request #2519 from deepseek-harness/feat/npm-public`；`packages/**/package.json` 包名筛 `desktop|host|electron` → `@deepseek-ai/dsh-cordis-host-runner`、`@deepseek-ai/dsh-host-apiproxy`、`@deepseek-ai/dsh-host-directory-picker{,-auto,-browse,-native}`、`@deepseek-ai/dsh-host-frontend-static`、`@deepseek-ai/dsh-host-plugin-inventory`、`@deepseek-ai/dsh-host-webserver`、`@fixture/host`

索引过的官方材料（引用 URL）：

- 源码仓库：<https://github.com/deepseek-ai/deepseek-harness>（本机检出 remote 与之一致；本文件引用的 `CONTRIBUTING.md`、`docs/`、`.agents/notes/` 均来自该仓库的检出拷贝）
- npm 包元数据：<https://registry.npmjs.org/@deepseek-ai/dsh>（`latest: 0.1.7-rc.2`、`next: 0.2.0-rc.1`）
- 未发布的桌面宿主包：<https://registry.npmjs.org/@deepseek-ai/dsh-desktop-host>（404）
