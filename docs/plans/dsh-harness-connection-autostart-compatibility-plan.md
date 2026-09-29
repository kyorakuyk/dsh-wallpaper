# Harness 连接修复、随壁纸启动与更新兼容施工文档

> 2026-09-24 现场取证后写给下游编码 Agent。本文件是施工任务书，不代表功能已经实现或真机验收通过。

## 0. 目标和工作边界

在 `D:\Family\DeepSeekHarness\plugins\dsh-wallpaper` 施工。用户已选择「**随壁纸启动**」：完整版壁纸每次启动时，若用户打开新开关，异步启动配置的 DeepSeek Harness（DSH）。登录时由壁纸现有的 Windows 自启机制先启动壁纸；不另建 DSH 的 Run 项或计划任务。手动启动壁纸也应触发同一设置。Lite 不引入 Harness。

交付结果应满足：能分辨 DSH 未启动、只有 DSH Web UI、Bridge 正在加载、Bridge 与宿主不兼容、token 不可用及可对话；Bridge 真正就绪后才允许 Harness 模式。DSH 更新影响 Bridge 内部适配时，可以单独更新 Bridge，壁纸与 Bridge 之间的 `/api/wallpaper/v1` 协议保持可用。无法保证任意未来 DSH 版本都兼容；不兼容时必须明确降级和提示，不能把端口开放等同于连接成功。

开始先执行 `git status --short --branch`。当前 `codex/startup-render-handoff` 有未提交的首帧施工改动，另有正在运行的 `target\debug\dsh-wallpaper.exe`；保留这些改动，Rust 测试/构建若遇 exe 文件锁，正常退出该开发版再试。`D:\Family\DeepSeekHarness\deepseek-harness` 也是脏工作树，本任务以只读方式检查它；不要 reset、清理用户 profile 或覆盖会话数据。代码修改主要落在本仓库的 `bridge/` 与完整版 `wallpaper/`。

## 1. 已核实的现状（勿当作修复结果）

| 检查点 | 2026-09-24 结果及含义 |
|---|---|
| 运行服务 | `127.0.0.1:3080` 无监听；当前连接首先缺少正在运行的 DSH。启动 DSH 后仍须继续检查 Bridge。 |
| DSH 源码 | `D:\Family\DeepSeekHarness\deepseek-harness`，根包及 host-webserver 为 `0.1.0-rc.5`；`apps/cli/lib/bin.js` 存在。 |
| Bridge 声明 | `bridge/package.json` 为 `0.1.1`，DSH peer 依赖从 `^0.1.0-rc.6` 起；`bridge/src/protocol.ts` 的 `BRIDGE_VERSION` 却是 `1.1.0`。版本声明需要统一，`rc.5` 的运行时兼容性需要实际验证。 |
| DSH desktop profile | `%USERPROFILE%\.dsh\profiles\desktop\package.json` 用 `file:C:/DeepSeekHarness/plugins/dsh-wallpaper/bridge` 安装插件。`C:\DeepSeekHarness` 是指向 `D:\Family\DeepSeekHarness` 的 junction，**该路径目前有效**；不要仅因盘符不同而改掉它。 |
| 已安装 Bridge | profile 内 `node_modules/dsh-wallpaper-bridge/lib/index.js` 的 SHA-256 前缀为 `04939052`，当前 C/D 源码构建均为 `F8C8DF37`。已安装副本内容落后，尽管三个位置的 `package.json` 都写 `0.1.1`。 |
| token | `%USERPROFILE%\.dsh\wallpaper\bridge-token` 存在、44 字节；`DSH_HOME` 当前未设置。未读取或展示 token；文件存在不能证明当前 Bridge 已加载或 token ACL 有效。 |
| 壁纸启动项 | `HKCU\...\Run` 的 `dsh-wallpaper` 当前指向已安装 MSIX `0.2.0.71`；本机另有源码开发版在运行。开发版启动成功不能代替登录后已安装版本的验收。 |

代码链：

```text
Windows 登录 → 壁纸 StartupTask/Run → 壁纸进程
  → （新开关开启）受管 DSH CLI → 127.0.0.1:3080
  → desktop profile 加载 dsh-wallpaper-bridge → /api/wallpaper/v1/status
  → 原生壳读取私有 token → POST /sessions → SSE /events → 桌面对话
```

目前 `wallpaper/src/App.tsx` 只在用户点击「启动」时调用 `launchDsh`；`wallpaper/src/settings/store.ts` 的 `dshLaunch` 只有 rootPath/profile/command，没有随壁纸启动字段。`launch_dsh` 在 `wallpaper/src-tauri/src/lib.rs` 验证源码根目录、解析 Node/pnpm、占用端口时拒绝启动，并只管理本进程创建的子进程。退出时也只停止受管 DSH。

连接还有一处需要验证的就绪边界：`bridge/src/index.ts` 的公开 `/status` 只依赖 `webServer`，却固定宣告 `sessions/history/sse/cancel/approval-handoff`；这些路由在另一个要求七项服务的 `ctx.inject` 回调中注册。代码没有把所宣告的能力与路由注册结果绑定；宿主更新时若出现状态路由已就绪而会话路由未挂载，壁纸可能见到 `bridge-ready`，随后创建会话却遇到 404。Rust/前端目前把不兼容的状态响应进一步归为 `web-only` 或 `offline`，用户无法知道哪一层坏了。先用真实宿主验证该时序，再修复能力宣告。

## 2. 阶段 A：先修当前连接并留下可复现证据

1. 分开记录：DSH 进程与 3080 监听、`GET /`、`GET /api/wallpaper/v1/status`、token 就绪、带鉴权的 `POST /sessions`、`GET /sessions/{id}/events`。公开 status 可读；不要打印 token、Authorization、对话正文或完整本机私有路径到普通日志。
2. 用当前 DSH CLI 和 `desktop` profile 启动一次，记录退出码、profile 加载错误与 Bridge 状态。先核对 `node.exe`/pnpm 路径、`apps/cli/package.json` 的 `bin` 入口以及保存的 DSH 根目录；不要仅依据 3080 端口推断进程身份。
3. 对 profile 里的 Bridge 做**版本和内容**核对。`file:C:/...` 目前经 junction 能到当前源码，但 `node_modules` 是旧副本。通过 DSH/profile 支持的包管理流程更新该插件，重新读取已安装 `lib/index.js` 的摘要并验证它是本轮测试过的构建。保留 profile 的 package/lock 备份；不要删除整个 `.dsh`、token、sessions 或 workspace，也不要盲目重写 C/D 路径。
4. 对 `rc.5` 宿主与 Bridge `rc.6` peer 声明跑真实挂载试验。若 API 实际兼容，补上有证据的版本范围；若不兼容，修 Bridge 的宿主适配层或给出清楚的版本错误。不能只调宽 peer 范围让安装器闭嘴。
5. 把最先失败的一层、发生时间、HTTP 状态/稳定错误码与修复后复测结果写在交付报告。若 DSH 本身启动失败，先解决启动或配置错误，不能把 Bridge 判为根因。

诊断状态的最低要求：`offline`（无人监听）、`web-only`（DSH Web 可用但无 Bridge status）、`bridge-loading`（status 已注册而会话路由未齐）、`bridge-auth-unavailable`、`bridge-incompatible`、`bridge-ready`。只有最后一种允许发送；不自动切到付费 DeepSeek API。网络、协议和权限错误可以附非敏感原因码，不把原始异常或 token 送到 WebView。

## 3. 阶段 B：使 Bridge 就绪状态可信

- 改 `bridge/src/index.ts`：公开 status 可先挂载，但所宣告的能力须反映**已实际注册**的路由及 token 状态。全服务作用域内注册 control/sessions 路由成功后才报告会话能力；注册部分失败时清理已注册路由，卸载时撤销就绪。宿主依赖迟到时返回加载状态，不宣告 `sessions`。
- `bridge/src/protocol.ts` 与原生 `wallpaper/src-tauri/src/lib.rs`、前端 `wallpaper/src/connect/harness.ts` 保持同一协议判据。可给 v1 status 增加可选、稳定的 `reasonCode`/宿主兼容信息；不要改变现有 v1 会话与 SSE 字段含义。`fetch_harness_status` 收到 Bridge status 但能力不足、协议不匹配或鉴权不可用时应保留对应诊断，不再一律落成 `web-only`。
- `wallpaper/src-tauri/src/chat.rs::harness_connect` 在实际创建会话时仍检查 HTTP 响应和 SSE 握手；探测缓存不能替代实际连接成功。恢复会话失败只在 Bridge 明确返回 `resume-unavailable` 时创建新会话，避免更新后悄悄生成大量冗余会话。
- 保留现有信任边界：只绑定 `127.0.0.1`，随机 bearer token 只由原生壳读取，ACL 失败时 fail closed，桌面 renderer 不拿 token。不得为“兼容”开放任意主机、跳过鉴权或把错误正文原样传给前端。

行为测试须覆盖：status 先于完整服务、服务迟到后转 ready、注册失败回滚、卸载后不可再显示 ready、协议版本不符、token 不可用、创建会话 404/409/503、SSE 断线重连。现有 `bridge/tests/routes.spec.ts` 主要使用 mock；另加一条在真实 DSH `desktop` profile 上跑的烟测，证明路由确实挂载。

## 4. 阶段 C：加入“随壁纸启动 DSH”设置

1. 仅完整版在 `wallpaper/src/settings/store.ts` 的 `DshLaunchSettings` 增加 `autoStartWithWallpaper: boolean`，默认 `false`。设置归一化、广播与旧版数据迁移都要覆盖它。当前版本为 9、存储键含 `v9`；改版时必须迁移已有 rootPath/profile/command、后端选择及其他设置，不能换键后丢弃用户配置。
2. 在设置中心「DeepSeek Harness 启动」卡片显示开关和有效状态。文案写明：**壁纸每次启动时尝试启动 DSH；登录后生效还需要壁纸自身开机自启**。壁纸自启被 Windows 关闭时明确提示；不悄悄修改用户的系统自启意愿。「立即启动」保留为独立操作。缺失或不可用的根目录必须能在设置中修正。
3. 自动启动由 `background` 壁纸宿主发起，设置窗口不能因打开/保存而重复拉起。每个进程最多发起一次自动启动；React 重挂载、settings-changed、锁屏解锁与 HMR 不得引发第二个 DSH。Rust 受管进程状态承担最终 single-flight/幂等保证。启动放到后台任务，不阻塞原生首帧、WorkerW 附着或 Tauri 主线程；手动和自动启动复用同一受验证的服务。
4. 开机只使用已保存且经 canonicalize 的 rootPath/profile；根目录无效时报告待配置，不做全盘扫描，也不从多个候选中擅自选一个。优先按 `apps/cli/package.json` 的受限相对 `bin` 入口解析已构建 CLI；若走 pnpm 回退，保持参数数组与隐藏控制台。`DSH_HOME` 的解析必须与原生 token 读取、Bridge 写入一致。
5. 自动路径不能无提示地执行 `dshLaunch.command` 指向的任意自定义程序。保留现有手动启动能力；自动启动只允许验证过的 Node/pnpm 启动器，或增加明确的用户确认及可信绝对路径约束。不得通过 shell 拼接命令。
6. 3080 已被外部 DSH 占用时不重复启动、不接管或停止该进程，转入 Bridge 状态探测。受管子进程退出、45 秒内 Bridge 不就绪、token 错误或协议不兼容时显示不同原因；不要无限重试或每秒重启。用户退出壁纸时沿用现有“只停止本进程受管 DSH”的所有权边界。
7. 登录自启依赖壁纸已有 StartupTask/Run 实际状态。先核对 [自启恢复方案](./autostart-recovery-plan.md) 与当前实现：现代码的 Run 写入仍使用 `reg.exe`，但本机此刻的 Run 项确实存在并指向已安装 `0.2.0.71`。不要把 2026-09-22 旧诊断当作今天的状态；如果壁纸自启意愿与 Windows 实际状态不一致，修好该依赖后才宣布“DSH 开机自启”验收通过。

## 5. 阶段 D：让 DSH 更新局限在 Bridge 适配层

- 在 Bridge 内建立集中式 DSH 宿主适配层，收口 `agents`、`agentPresets`、`workspaceRegistry`、`permissionPresets`、`commands` 和 `SessionEvent` 的调用与形状校验。当前 `bridge/src/index.ts` 直接依赖这些服务并多处用 `unknown as` 接口断言；把已验证的版本差异放进适配层，无法匹配的宿主只报 `bridge-incompatible`，不能抛到路由外或误报 ready。
- 将 `protocolVersion` 与 Bridge 包版本分开维护：v1 REST/SSE 是壁纸与 Bridge 的稳定边界；Bridge 的发布版本应来自同一来源，不能继续出现 package `0.1.1`、status `1.1.0`、安装副本内容又不同却没有可区分构建号的情况。建议 status 增加非敏感 build 标识，用于定位 profile 是否仍在运行旧插件。
- 形成“当前安装的 DSH `rc.5` + 实际下一目标版本”的构建和真实运行矩阵。每次 DSH 更新先验证 Bridge 编译、加载、status、创建/恢复会话、SSE、取消、权限控制和退出清理，再调整 peer 范围。只在测试证明后宣称支持该版本。若未来协议需要破坏性调整，另开 `/api/wallpaper/v2`，让旧 v1 有明确兼容窗口。
- Bridge 可以独立发版/更新；壁纸仅依赖稳定 v1 协议与能力集合。提供 profile 内 Bridge 更新/版本核验说明或应用内指引，但不要在壁纸登录时静默执行包管理、下载代码或改写 DSH profile。
- 更新 `README.md` 和 `bridge/README.md`：写清随壁纸启动的依赖关系、状态诊断、已测试的 DSH 版本矩阵与 Bridge 独立更新步骤，避免把未真机验收的版本标成已兼容。

## 6. 验证门槛与交付

自动检查至少运行：

```powershell
pnpm typecheck
pnpm test
pnpm -C bridge build
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --all-targets
cargo check --manifest-path wallpaper/src-tauri/Cargo.toml --locked --no-default-features --features lite
pnpm build
pnpm build:lite
git diff --check
```

Windows 真实链路必须用本轮 Bridge 构建及当前源码壁纸验证：手动启动 DSH、确认 3080/status 及完整能力、创建桌面会话、发送一条可辨识测试消息、收到最终回复和完成状态、关闭并重连同一会话、取消一轮、从设置停止受管进程后确认外部 DSH 不受影响。再开启新设置，重启壁纸两次验证每次只启动一个受管进程；DSH 启动慢时壁纸首帧仍正常且没有黑色控制台。DSH 未启动、只有 Web UI、Bridge 缺失/旧副本、token/ACL 不可用、宿主版本不支持、配置路径失效与 3080 被外部服务占用都要有可行动提示。

真正的“登录后自启”要用当前代码的签名测试包在用户会话里验证，记录安装包版本、实际运行 exe 路径、Windows 启动来源、壁纸与 DSH 的启动时间、Bridge ready 时间，以及关机/重登后的进程数量。`pnpm desktop:dev` 成功只证明手动启动。收集日志时遮蔽用户目录、会话正文和任何凭据；从不展示 token 文件内容。

最终报告逐阶段列出：根因证据、修复文件、已支持的 DSH/Bridge/协议版本、测试结果、真实运行证据、尚未覆盖的更新版本与场景、`git status --short`、本地提交 SHA。不要自动 push；失败时停在最先不能通过的阶段，说明所需宿主改动或用户操作。

## 7. 补记（2026-09-30 夜）：施工口径变更 —— 统一到 CLI，桥只装在 `web` 档案

> 用户拍板。本节**取代第 1 节代码链中的"受管 DSH CLI → 3080 → desktop profile 加载 dsh-wallpaper-bridge"一节点**，
> 第 2 至第 5 节的功能要求与第 6 节验收门槛不受影响，仍然适用。
> 取证过程与实测见 [官壳后台启动的取证](../evidence/shell-background-start-window.md) 第八、八点一、八点二节。

### 7.1 改成什么

三种主体**都退化为"跑它的 CLI"**：官壳用它自带的 `resources\runtime\cli\bin\dsh.cmd`（动态解析安装目录），
已安装 CLI 用它自己，源码检出用树里的启动链。于是读写（会话、桥、投递）**只需要兼容 CLI 这一条路径**。

| 角色 | 用什么 | 档案 | 端口 | 窗口 |
| --- | --- | --- | --- | --- |
| 后台（滑槽切 harness、随壁纸自启） | 所选主体的 CLI | **`web`** | **我们显式指定** | 无 |
| 前台（设置「打开」、岛左侧按钮） | 壳 ⇒ AUMID 激活完整壳；CLI/检出 ⇒ 原有网页界面路径 | 各自 | 壳固定 19387 | 有 |

两条硬约束（八点二节的源码取证）：壳的 desktop 宿主把端口**写死 19387** 且 `listen` 失败即启动失败，所以后台
宿主必须显式选另一个端口；会话的跨进程写锁只在"写打开"时获取，**同一会话不能同时被两个宿主驱动**。

### 7.2 桥的安装范围（本次变更的核心）

* **唯一必装目标：壁纸自己启动的那个 `web` 宿主。** 壁纸的输入岛只连它，不连前台壳。
* `desktop` 档案里的桥**只服务于回退路径**（回退开关启用、壁纸去连壳的 19387）。
* **硬条件：回退路径一旦启用，`desktop` 里就必须有可用的桥。** 否则失败形态是"连得上、建不了会话"
  （`/status` 可达而 `/sessions` 返回 404），比"连不上"更难诊断。因此实现回退开关时必须**顺便核验
  desktop 档案里的桥**：缺失、过旧或协议不匹配时给出可行动提示，并在诊断状态机上落到
  `bridge-incompatible`（或更明确的码），**不得落成 `bridge-ready`**。
* 桥本身不省。DSH 对外的三套客户端接口（`dsh-acp`、`dsh-sdk-jsonrpc-server`、`dsh-headless`）自述就是
  stdio、"automation-only"、"no Host, HTTP, or browser layer"，拿不到工作区、会话与审批语义；而壁纸需要的
  语义（桌面会话工作区、`项目记忆.md`、每日会话 id、历史裁剪、模型/预设/权限控制面、token + 私有 ACL）
  在 DSH 原生没有对应物。

### 7.3 现状取证（2026-09-30，本机）

三个档案的依赖都写成 `file:D:/Family/DeepSeekHarness/plugins/dsh-wallpaper/bridge`（指向开发检出），
而 `bridge/package.json` 已是 `0.1.4`。三份安装副本都自称 `0.1.3`，但**内容两两不同，且没有一份等于当前源码构建**：

| 位置 | `lib/index.js` | 协议块 | 协议块字节数 |
| --- | --- | --- | --- |
| 源码构建 | `F2A2E6E12859` | `152B6E4E1653` | 11275 |
| `desktop` | `77D988038AD7` | `B4EFA36CB53D` | 11275 |
| `web` / `dsh-tui` | `C8904D106E35` | `D1FDABD21E4C` | **11253** |

首个差异字节落在 chunk 文件名（`protocol-<hash>.js`），说明确实是三次不同构建而非同一构建的拷贝；
`web`/`dsh-tui` 的协议块比源码**短 22 字节**，是更旧的源码。含义：桌面端与 `dsh-tui` 今天跑的不是同一份桥，
正是第 5 节点名的"安装副本内容不同却没有可区分构建号"。

### 7.4 随之要做的

1. 桥的依赖从 `file:`（指向开发检出）改为**钉住版本**的 npm 包 `dsh-wallpaper-bridge@<x.y.z>`；
   只装进 `web`，`desktop` 按回退需要保留、同样钉版本。不再让两个档案各自指向一份可能不同的本地构建。
2. status 增加非敏感 build 标识（第 5 节已有此要求），用于一眼看出宿主里跑的是哪一份。
3. **端口与发现的简化**：宿主由我们起、端口由我们定之后，`wallpaper/src/connect/endpoints.ts` 的
   19387/3080 候选表、`subjectEndpointPorts` 扫描与端口 pin 自愈逻辑，以及 `connect/probe.ts` 的 3080 轮询，
   退化为"读设置里的 `endpointPort` + 存活检查"。**但第 0 节的原则不变：端口开放不等于连接成功**，
   仍须用真实 `POST /sessions` 加 SSE 握手确认后才进入 Harness 模式。
4. 状态胶囊（输入岛左下角的只读元素）显示当前实际入口：`API` / `Web`（API 后端）、
   `Web` / `Desktop` / `TUI`（harness，判据 `settings.window === 'tui'`、`isEmbeddedShellSubject`、其余为 `Web`）。

### 7.5 状态

本节是**口径**，代码尚未改动。开工顺序建议：先在原生层做"主体 → CLI 启动器"的解析函数（纯函数加单测，
不改变现有行为），再动后台宿主的生命周期（必须连子进程一起收），最后才是前端的状态胶囊与端口简化。
