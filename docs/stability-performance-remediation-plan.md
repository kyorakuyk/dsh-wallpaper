# DSH Wallpaper 稳定性与性能修复计划

> 目标读者：负责继续修改代码的 DeepSeek / 其他编码 Agent  
> 编写日期：2026-09-22  
> 当前基线：`codex/startup-bootstrap-retry` / `af32864`  
> 本文是实施计划，不代表相关修复已经完成。

## 0. 开始前的仓库边界

当前可用仓库是：

```text
D:\Family\DeepSeekHarness\plugins\dsh-wallpaper-rebuild
```

不要在下面这个目录执行 Git 修改、提交、清理或恢复命令：

```text
D:\Family\DeepSeekHarness\plugins\dsh-wallpaper
```

它仍是 `.git` 已损坏的旧工作区，目前因为 WorkBuddy 后台进程持有目录句柄而尚未改名。

开始修改前必须确认：

```powershell
git -c gc.auto=0 -c gc.autoDetach=false rev-parse --show-toplevel
git -c gc.auto=0 -c gc.autoDetach=false status --short --branch
git -c gc.auto=0 -c gc.autoDetach=false log -2 --oneline
```

期望：

- 根目录是 `dsh-wallpaper-rebuild`；
- 分支是 `codex/startup-bootstrap-retry`；
- HEAD 是 `af32864`，或是以它为祖先的后续修复提交；
- 工作树没有不明改动；
- 本地配置中的 `gc.auto=0`、`gc.autoDetach=false` 不得删除；
- 不得把 `.workbuddy/`、证书私钥、`node_modules/`、`target/` 或临时输出加入版本库；
- 未经用户明确要求，不执行 `push`、`reset --hard`、历史重写或目录删除。

## 1. 总目标与非目标

### 1.1 总目标

本轮优先解决：

1. 用户停止 Harness/API 回答后，发送键立即释放，晚到事件不能让界面重新进入生成状态；
2. 设置中心先显示 UI，再按页面懒加载系统探测，避免打开时卡顿和集中拉起子进程；
3. 所有持久化设置和跨 WebView 设置事件经过同一套运行时校验，坏数据不能造成白屏；
4. 长期运行时，会话记录、网络流、事件监听、Bridge 会话和轮询都有明确上限及清理路径；
5. 保持 DeepSeek Web、Harness、多屏、锁屏和 Lite 版已有行为不回退。

### 1.2 非目标

- 不重做视觉设计；
- 不恢复 CapsuleBubble/“灵动胶囊”方案；
- 不更换 Tauri/WebView2 技术栈；
- 不借机重构全部聊天架构；
- 不修改 DeepSeek 网页 DOM 配置，除非现有测试证明本轮改动影响了它；
- 不以删掉历史、截断当前回答正文的方式换取性能；
- 不在本轮自动打包、签名、安装或推送，除非用户另行要求。

## 2. 实施顺序

| 批次 | 项目 | 优先级 | 建议提交 |
|---|---|---:|---|
| A | 原生聊天停止生命周期 | P0 | `fix: isolate cancelled native chat turns` |
| A | 设置中心首帧与探测懒加载 | P0 | `perf: defer settings system probes` |
| A | 设置归一化与跨 WebView 防御 | P0 | `fix: normalize settings at every boundary` |
| B | 异步监听清理 | P1 | `fix: dispose deferred renderer listeners` |
| B | API 会话记录裁剪与删除 | P1 | `fix: bound durable api conversation history` |
| B | API/Harness 流空闲看门狗 | P1 | `fix: bound idle chat streams` |
| C | Bridge 会话、订阅者与历史上限 | P2 | `fix: bound wallpaper bridge resources` |
| C | 流式渲染和历史列表 | P2 | `perf: batch streaming chat renders` |
| C | 多屏轮询与退出清理 | P2 | `perf: reduce resident polling and clean native exit` |

每个批次单独提交。一个批次未通过测试时，不开始下一批。

## 3. 批次 A：必须优先完成

### A1. 修复原生聊天停止生命周期

#### 当前问题

文件：`wallpaper/src/chat/nativeAdapter.ts`

当前 `stop()` 只有：

```ts
async stop(): Promise<void> { await nativeRuntime.cancelChat(this.mode) }
```

界面层会先把活动状态改为 `idle`，但适配器仍保留事件作用域和历史对账定时器。取消后晚到的 delta、assistant message 或 status 仍可能被接受，从而让发送键重新进入忙碌状态。

#### 关键设计约束

不能简单照搬 `DeepSeekWebAdapter.stop()` 并直接清空现有 `requestId`。

原因：`NativeChatAdapter` 中 Harness 的 `requestId` 当前兼任 SSE 连接作用域 ID，而 DeepSeek API 的 `requestId` 是每一轮请求 ID。两者语义不同。直接清空会导致 Harness 后续发送继续复用适配器时无法接收合法事件。

#### 修改方案

1. 拆分状态：
   - `harnessConnectionId`：Harness 连接生命周期内稳定；
   - `apiRequestId`：每次 API send 新建；
   - `turnGeneration` 或 `activeTurnToken`：每次 send 新建，用于本地晚到事件隔离；
   - `turnActive`：停止后立即设为 `false`。
2. `acceptsScopedChatEvent` 不再混用 API 请求 ID 和 Harness 连接 ID。
3. Harness `stop()` 必须按以下顺序执行：
   - 先使当前 turn 失效；
   - 停止并清空 `reconcileTimer`；
   - 阻止已在途的 `reconcileHistory()` 发布消息；
   - 立即向 UI 发出 `idle`；
   - 再等待原生取消；
   - 必要时重新建立新的 Harness 事件作用域，但保留同一个 DSH session ID。
4. API `stop()`：
   - 清除当前 API request ID；
   - 立即发出 `idle`；
   - 晚到 API 事件必须被 request ID 闸门拒绝。
5. 下一次 `send()` 必须建立新的 turn 身份，并能正常接收新事件。
6. 取消失败只能显示可恢复错误，不能把活动状态重新改成 streaming。

#### 涉及文件

- `wallpaper/src/chat/nativeAdapter.ts`
- `wallpaper/src/native/runtime.ts`（仅在需要拆分 IPC 字段时）
- `wallpaper/src-tauri/src/chat.rs`（仅在必须增加每轮 Harness 标识时）
- `wallpaper/src/App.tsx`
- 新增 `wallpaper/tests/nativeAdapter.spec.ts`
- 必要时扩充 `wallpaper/tests/appChatLifecycle.spec.ts`

#### 必须增加的测试

1. `stop()` 在原生 Promise 未完成前就发布 `idle`；
2. `stop()` 清理历史对账定时器；
3. 停止后的 late delta、late assistant message 不进入 UI；
4. 停止后马上再次发送，旧 turn 事件不能污染新 turn；
5. 新 turn 的合法事件仍能到达；
6. API 和 Harness 两种模式分别测试；
7. 原生取消失败时 UI 保持 idle，并显示错误；
8. 使用 fake timers 验证取消后不会继续调用 `harnessHistory()`。

#### 验收标准

- 点“停止生成”后 100 ms 内发送键恢复；
- 取消接口延迟或失败时，按钮仍不会重新锁死；
- 旧回答不再继续追加；
- 下一条消息可以立即发送并正常完成；
- DeepSeek Web 现有停止逻辑不回退。

### A2. 设置中心先绘制，再按页面加载探测

#### 当前问题

文件：`wallpaper/src/settings/SettingsWindow.tsx`

设置窗口挂载时同时触发：

- AppCore 快照；
- TranslucentTB 探测；
- DSH 路径扫描；
- 托管 DSH 状态；
- 自启状态；
- 锁屏诊断；
- 显示器枚举；
- DeepSeek Web 配置状态。

这些调用虽然部分已放入 Rust blocking worker，但仍会在窗口打开瞬间造成磁盘、PowerShell、Win32 和 IPC 峰值。`scanDsh(false)` 仍可能遍历用户目录，不应成为打开设置的默认动作。

#### 修改方案

1. 把 SettingsPanel 当前内部的 `page` 状态提升到 `SettingsWindow`，或提供严格类型的 `onPageActivated(page)`。
2. 首帧只加载：
   - 本地设置；
   - AppCore 快照；
   - 当前页真正需要的最小数据。
3. 页面级懒加载：
   - “集成”：TranslucentTB、DSH 托管状态、Web 适配器配置；
   - “系统”：锁屏诊断、自启状态；
   - “显示”：显示器枚举；
   - “外观”：已有素材库懒加载继续保留。
4. 删除窗口打开时的自动 DSH 目录扫描：
   - 已配置路径只做一次便宜的存在性检查；
   - 全面扫描只由“扫描 DSH”按钮触发；
   - 扫描期间保留 busy 状态、禁止重复点击。
5. 对每种探测增加 `loaded` 与 `inFlight` 标记，避免反复切页重复启动同一任务。
6. 低优先级探测放到首帧后的双 `requestAnimationFrame` 或 `requestIdleCallback`；必须提供不支持 `requestIdleCallback` 的回退。
7. 不允许通过延迟显示整个设置窗口来掩盖耗时。

#### 涉及文件

- `wallpaper/src/settings/SettingsWindow.tsx`
- `wallpaper/src/settings/SettingsPanel.tsx`
- `wallpaper/src/settings/SettingsPanel.css`（仅在需要 skeleton/加载状态时）
- `wallpaper/tests/nativeChatBoundary.spec.ts`
- 建议新增 `wallpaper/tests/SettingsWindow.spec.tsx`

#### 必须增加的测试

1. 初次渲染不调用 `scanDshPaths`；
2. 未打开“系统”页时不调用锁屏诊断；
3. 第一次打开对应页面时只调用一次；
4. 同一页面快速切换不会并发执行重复探测；
5. DSH 扫描按钮 busy 时不能再次调用；
6. 探测失败只显示 notice，不影响窗口继续操作；
7. 关闭窗口后，晚到结果不会更新已卸载组件。

#### 验收标准

- 点击设置后先出现完整窗口骨架，不再先黑框再等待内容；
- 未进入 DSH 页面时不发生目录扫描；
- 未进入系统页面时不启动锁屏/自启探测；
- 设置窗口打开、关闭、再次打开不产生重复监听或重复任务。

### A3. 所有设置边界统一归一化

#### 当前问题

文件：

- `wallpaper/src/settings/store.ts`
- `wallpaper/src/App.tsx`
- `wallpaper/src/settings/SettingsWindow.tsx`

现有 `migrate()` 使用了 `...value`，只严格处理部分字段。以下异常值仍可能穿过：

- 非法 `defaultBackend`；
- 字符串形式的 `animationSpeed`；
- 含 `null` 或错误结构的 `modelTierRules`；
- 非对象 `bubbleOverrides`；
- 非有限数字、过长字符串和未知枚举；
- `settings-changed` 事件中的未归一化载荷。

`App.tsx` 当前直接 `setSettings(event.payload)`，绕开了本地加载时的迁移逻辑。

#### 修改方案

1. 将 `migrate()` 重构为公开的纯函数：

```ts
export function normalizeSettings(raw: unknown): WallpaperSettings
```

2. 不再展开未知对象；从 `DEFAULT_SETTINGS` 开始逐字段重建。
3. 为以下字段建立显式验证器：
   - Backend、conversation policy、interaction layout、animation intensity；
   - 有限数字和范围；
   - bool；
   - 受限长度字符串；
   - `ModelTierRule` 每个元素的 backend/match/pattern/tier；
   - `bubbleOverrides` 的 key/value；
   - `multiScreen`、API、DSH 子对象。
4. 正则规则只存储字符串，不在迁移阶段执行不受控正则；运行时已有匹配逻辑应捕获非法表达式。
5. `loadSettings()`、`saveSettings()`、`SettingsWindow.commitSettings()`、`settings-changed` 接收端全部调用同一函数。
6. 跨 WebView 接收到未来版本设置时：
   - 保留已识别字段；
   - 忽略未知字段；
   - 不把错误数据写回覆盖原存储，除非用户实际修改设置。

#### 涉及文件

- `wallpaper/src/settings/store.ts`
- `wallpaper/src/App.tsx`
- `wallpaper/src/settings/SettingsWindow.tsx`
- 新增或扩充 `wallpaper/tests/settingsStore.spec.ts`
- 扩充 `wallpaper/tests/multiScreenSettings.spec.ts`

#### 必须增加的测试

至少覆盖：

- `animationSpeed: "fast"`；
- `modelTierRules: [null, {}, validRule]`；
- `bubbleOverrides: "bad"`；
- `defaultBackend: "unknown"`；
- `NaN`、`Infinity`、超界数值；
- 非对象 `multiScreen`；
- 超长 display ID；
- `settings-changed` 事件收到坏载荷时仍能渲染；
- 完整合法设置 round-trip 不丢字段。

#### 验收标准

- 手工向 localStorage 写入坏设置后，设置中心和桌面均能打开；
- `animationSpeed.toFixed()` 等调用不再可能收到字符串；
- Lite 版设置归一化行为保持不变。

## 4. 批次 B：长期稳定性

### B1. 修复异步监听的清理竞态

#### 当前问题

`App.tsx` 多处使用：

```ts
let dispose = () => undefined
void listen(...).then((unlisten) => { dispose = unlisten })
return () => dispose()
```

如果组件先卸载、Promise 后返回，真正的 disposer 永远不会执行。

#### 修改方案

1. 提取小型工具或 hook，维护 `disposed` 标记；
2. Promise 返回时如果已经卸载，立即调用刚拿到的 disposer；
3. disposer 必须最多调用一次；
4. `appCoreClient.subscribe`、`settings-changed`、`appearance-changed`、显示器事件等统一使用；
5. Promise rejection 必须被捕获，不能产生 unhandled rejection。

#### 测试

- 使用可控 Promise，让组件先 unmount 再 resolve；
- 断言 disposer 被调用一次；
- 正常挂载/卸载也只调用一次；
- rejection 被转换为可控错误或被安全忽略。

### B2. API 会话记录必须可裁剪、可删除、可降级

#### 当前问题

- DPAPI 明文档案硬上限为 16 MB；
- 会话和消息会长期增长；
- 没有删除会话入口；
- 超限保存失败后，后续每轮仍会复制、合并、序列化完整档案；
- `api_history` 会克隆整个会话返回前端。

#### 修改方案

1. 定义低于硬上限的目标预算，例如 12 MB；16 MB 继续作为拒绝损坏/异常档案的硬上限。
2. 保存前按稳定规则裁剪：
   - 优先保留当前会话；
   - 每个会话保留最近消息；
   - 全局按最后更新时间淘汰最旧会话；
   - 不切断单条消息正文；
   - 不改变保留下来的消息 ID 和时间戳。
3. 给 `ApiConversation` 增加或计算 `updated_at`。
4. 增加删除命令：
   - 删除一个会话；
   - 可选“清空全部 API 历史”；
   - 命令必须在进程内锁和跨进程锁下原子更新。
5. 首次保存因体积仍失败时：
   - 本次进程停止重复持久化尝试；
   - 保留内存对话；
   - UI 显示一次明确、可操作的警告；
   - 不覆盖原加密档案。
6. `api_history` 返回值增加消息数和字节上限，支持“最近 N 条/加载更早”参数。

#### 涉及文件

- `wallpaper/src-tauri/src/api_persistence.rs`
- `wallpaper/src-tauri/src/chat.rs`
- `wallpaper/src-tauri/src/lib.rs`
- 对应 capability/permission 文件和生成 schema
- `wallpaper/src/native/runtime.ts`
- 设置或历史 UI
- Rust 持久化与命令测试

#### 测试

- 超过目标预算会删除最旧数据而不是永久失效；
- 当前会话优先保留；
- 单条超大消息不被切半；
- 删除在两个模拟进程视图合并时不会复活；
- 未来 schema、损坏密文仍然 fail-closed；
- DPAPI 测试继续证明磁盘上没有明文。

### B3. 给长连接增加“空闲超时”而不是只靠 24 小时总超时

#### 当前问题

API 和 Harness SSE 客户端的总 timeout 为 24 小时。连接建立后若网络半断开且不再产生数据，发送槽可能长时间被占用。

#### 修改方案

1. 保留较长总生命周期，但对每次读取使用独立空闲期限；
2. API 建议 120 秒无数据超时；
3. Harness Bridge 每 15 秒有 heartbeat，可使用 45～60 秒无数据超时；
4. 每收到一个 chunk/heartbeat 重置空闲期限；
5. 用户取消必须优先于 timeout；
6. timeout 后发出稳定错误码，并可靠释放请求槽；
7. 不把模型正常的短暂停顿误判为失败。

#### 测试

- 持续收到 chunk 时可运行超过总空闲期限；
- 完全无数据时按预期超时；
- heartbeat 可保持 Harness 连接；
- cancel 与 timeout 同时发生时只清理一次；
- timeout 后下一轮可发送。

## 5. 批次 C：资源和渲染优化

### C1. Bridge 的会话、SSE 客户端和历史必须有上限

#### 修改方案

1. 定义并测试：
   - 最大 live wallpaper sessions；
   - 每个 session 最大 SSE clients；
   - 最大 pending creations；
   - history 最大消息数和 UTF-8 字节数。
2. 默认桌面行为继续复用同一个 workspace-owned session，不能因为重连产生冗余会话。
3. 空闲且无客户端的 live handle 在 TTL 后 `dispose()`；不要删除 DSH 的耐久历史。
4. 达到上限时返回稳定的 429/409，不继续分配资源。
5. `historyOf()` 在 JSON 序列化前裁剪，而不是让 Rust 客户端读取到 4 MB 后才拒绝。
6. 慢 SSE 客户端继续执行现有 fail-fast 断开策略。

#### 测试

- 重复连接同一 session 不增加 live 数量；
- 外来 session 仍被 workspace ownership 拒绝；
- 超额订阅者被拒绝；
- TTL 只释放空闲 handle；
- history 截取保持消息顺序且不切半正文。

### C2. 批处理流式文本与历史滚动

#### 当前问题

- 每个 delta 都执行一次 React state 更新；
- 展开历史时，每个 delta 都读取 `scrollHeight` 并 `scrollTo`；
- 全部历史消息一次性进入 DOM。

#### 修改方案

1. delta 先写入 ref 缓冲区，每个 animation frame 最多提交一次 state；
2. 自动滚动也合并到同一个或下一个 animation frame；
3. 用户离开底部后停止自动滚动；
4. 默认只渲染最近 100 条，提供“加载更早”；
5. 只减少 DOM，不删除内存/持久化历史；
6. 当前正在生成的完整正文不能截断。

#### 测试

- 一帧内多个 delta 合并且顺序不变；
- 用户上滚后不会被拉回底部；
- 加载更早不会重复消息；
- 最终 assistant message 与 streaming buffer 合并后正文完整。

### C3. 降低常驻轮询，补齐退出清理

#### 修改方案

1. `desktopLayoutMetrics` 不再无条件每秒 IPC：
   - 优先响应 `display-changed`、resize、设置变化；
   - 仅在会话层展开或任务栏布局需要时短期轮询；
   - 保留低频兜底（例如 30 秒），而不是 1 秒。
2. DSH 启动检测的 1 秒轮询只在 `harnessStarting=true` 时存在，并在成功/超时后立即销毁。
3. Settings 显示器列表只在显示设置页激活时轮询，或改为原生事件推送。
4. Tauri `.run(|_, _| {})` 改为显式处理退出生命周期：
   - 正常退出时恢复桌面图标；
   - 销毁原生 bootstrap 窗口并恢复进程优先级；
   - 停止本进程启动并持有的 DSH child；
   - 清理会话通知/子类资源；
   - 不把“隐藏到托盘”误判为退出。
5. 给 native bootstrap 增加兜底看门狗，前端永远未发送 ready 时也不能永久残留。

#### 验收

- 空闲桌面不再每秒进行布局 IPC；
- 显示器热插拔仍能更新；
- 托盘退出后无残留 bootstrap 窗口、隐藏图标状态或子进程；
- 普通隐藏设置窗口不会触发全局清理。

## 6. 每个批次的验证命令

在 `dsh-wallpaper-rebuild` 根目录运行：

```powershell
pnpm typecheck
pnpm test
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --all-targets
pnpm build
pnpm build:lite
git -c gc.auto=0 -c gc.autoDetach=false diff --check
git -c gc.auto=0 -c gc.autoDetach=false status --short
```

当前基线计数：

- Wallpaper：29 个测试文件 / 135 个测试；
- Bridge：2 个测试文件 / 22 个测试；
- Rust lib：86 个单元测试，另有集成测试；
- 完整版与 Lite 构建均成功。

新测试数量应增加，不能通过删除或跳过原测试维持绿色。

## 7. 真机验收清单

自动测试通过后，在 Windows 11 真机执行：

### 聊天停止

1. Harness 开始生成长回复；
2. 点击停止；
3. 发送键立即恢复；
4. 等待 10 秒，旧文本不再增长；
5. 立即发送下一条消息，可正常收到完整回复；
6. DeepSeek API 与 Web 各重复一次。

### 设置中心

1. 冷启动后首次打开设置；
2. 窗口立即显示，不出现长时间黑框；
3. 不进入集成页时，不扫描 DSH、不启动 TranslucentTB PowerShell 探测；
4. 点击“扫描 DSH”后有 busy 和结果反馈；
5. 快速开关设置窗口，不出现重复 notice 或持续后台任务。

### 坏设置恢复

1. 在开发环境写入非法 localStorage 设置；
2. 重载背景和设置窗口；
3. 两者都不白屏；
4. 非法字段回退默认值，合法字段保留。

### 多屏与退出

1. 两个不同缩放比例的屏幕分别显示背景和立绘；
2. 热插拔或切换主屏后布局更新；
3. 空闲时观察 CPU/GPU/磁盘，无每秒明显尖峰；
4. 托盘退出后桌面图标和 WorkerW 状态恢复。

## 8. 提交与交付要求

DeepSeek 完成每个批次后必须交付：

1. 修改摘要；
2. 根因和状态机说明；
3. 新增测试名称与通过数量；
4. 所有验证命令的实际结果；
5. 尚未完成的真机验收项目；
6. `git status --short` 输出；
7. 对应本地提交 SHA。

不得只报告“构建成功”。涉及发送键、设置启动和多屏的修改，必须分别说明自动测试与真机验证边界。
