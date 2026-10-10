# 代码结构整理：拆大文件、隔离冻结代码、收拢状态

> 2026-10-05 起草。已确认冻结代码的处理原则：**保留源码与对应测试，只排除默认构建，不直接删除，也不只靠 Git tag 留档**；其余重构步骤仍待雪拍板。这份计划**不改任何用户可见行为**，只改代码的组织方式。
> 每一步都按"先出清单、再动手、做完给证据"的节奏走；每一步单独成一批提交，门禁全绿再进下一步。
> 本文是实施计划，不代表归档、构建隔离或后续重构已经完成。原有规模与计数为起草时记录，实施前按当前 commit 重新盘点。

## 零、为什么现在做

功能已经稳定（v0.3.x 已公开分发，CI 八步门禁在跑），但代码的重量集中在少数几个文件里，而且还在长：

- Rust：`windows_integration.rs` 5,881 行、`lib.rs` 5,299 行、`chat.rs` 4,749 行、`harness_launch.rs` 4,161 行、`crash_report.rs` 4,006 行。
- 前端：`App.tsx` 2,234 行（其中 `App` 组件本体约 1,600 行，29 个 `useState`、33 个 `useEffect`、23 个 `useRef`），`SettingsPanel.tsx` 1,214 行，`SettingsWindow.tsx` 67 KB。
- `artifacts/` 里十几个 `app-tsx-mine*.patch` 说明 `App.tsx` 已经是冲突热点。

这不是"代码写得差"：局部质量很高（纯函数大量导出可测、reducer 里收不变式、异步竞态用 epoch 防护、bridge 的令牌 ACL 与 `AsyncWorkTracker`）。问题在**宏观组织**：新东西总是追加进已有的大文件，于是每次改动的阅读成本、冲突概率和 agent 的上下文开销都在涨。

## 一、隔离冻结代码：保留源码，只退出默认构建（先做）

### 0. 已确认的保留原则与隔离方式

不再要求先决定"永久删除 / 临时关闭"。当前统一目标是：**冻结功能的源码、对应历史测试与恢复说明完整保留，但默认开发、发布构建与现役测试不引入冻结实现。** UI 不显示、函数无人调用或最终产物被摇掉，都不能单独证明源码已排除编译。

**默认方式：源码归档隔离。**

- 放在仓库根目录的 `archive/`，保留正常的 `.rs`、`.ts`、`.tsx` 文件，不压缩、不改成 `.bak`，也不只留在 Git 历史中。
- 锁屏沿用并补全 `archive/lockscreen-20260930/`；其它功能可按职责放入 `archive/frozen/system-integration/`、`archive/frozen/launch-ui/`、`archive/frozen/instance-ui/`、`archive/frozen/state-machine-v01/`。目录名与迁移清单实施前确认；完整文件保留原相对路径，混合文件只抽出冻结实现，不能复制整个现役大文件作为第二份实现。
- 每个归档单元的 README 记录原路径、来源 commit、归档原因、迁出的符号与测试、依赖、恢复接线和未验证范围。恢复接线包括模块声明、前端入口、命令注册、权限与测试入口；Git tag 只是额外的回退锚点，不替代工作树里的源码。README 同时注明：归档文件保留原来的相对 import，在 IDE 中显示为无法解析属预期现象；CI 的 `paths-ignore` 包含 `archive/**`，只改归档内容不会触发流水线。
- 归档目录不加入默认 workspace、生产入口、Tauri resources 或前端 `public/`。迁移前先把现役、共享与冻结实现分开；现役代码不能反向依赖归档代码。

**可选方式：需要持续维护的 Rust 功能用默认关闭的 Cargo feature。**

- 用模块级 `#[cfg(feature = "...")]` 隔离实现，并同步约束 import、类型、static、命令、handler 与测试入口；不加入 `default`、`full`、`lite` 特性或默认构建脚本的特性列表。
- 专属依赖按实际引用改为可选依赖，共享依赖保留。`if cfg!(...)` 是条件表达式，不是源码编译隔离的替代方案。
- 独立集成测试的 `#[path] mod` 也会引入实现；测试目标应归档，或用明确的 `required-features` 与同一功能特性绑定。不能只关 `lib.rs` 的声明。
- 只有确实需要持续验证恢复能力时，才增加显式 opt-in 的检查与测试矩阵；该矩阵不等于把功能重新加入默认产品构建。

**前端隔离约束：不把常量开关或 `exclude` 当作硬边界。**

- 当前构建是 `tsc -b && vite build`，类型检查先于 Vite 产物优化；`features.ts` 中的 `false` 常量不能保证相关源码不进入 TypeScript 程序。
- 当前 `tsconfig.json` 只 `include: ["src"]`，归档应留在生产 `src/` 之外，并断开现役 import、动态 import 与 reference。`tsconfig.exclude` 只是补充：被引用的归档文件仍可能重新进入编译。
- Vitest 的测试发现范围与 TypeScript 编译范围分别配置。默认测试只发现现役测试，归档测试单独保留；不能仅凭修改 tsconfig 就宣称测试隔离完成。
- 当前 `vite.config.ts` 的 `test` 段没有设置 `include`，Vitest 按默认规则扫描 `wallpaper/` 下所有 `*.spec.*` / `*.test.*`。因此归档测试只能放在 `wallpaper/` 之外（仓库根目录 `archive/`），不能放进 `wallpaper/tests/archive/` 之类的子目录；或者在迁移时给 Vitest 加一个明确的 `include`，只覆盖现役测试目录。

**恢复限制**：完全归档或默认关闭的源码不会持续接受默认编译器检查，接口可能随现役代码演进而失配。恢复前必须重新检查依赖、接线与安全前提，并实际运行启用侧门禁；不承诺"翻一个开关就一定恢复"。

### 1. 锁屏代码归档与构建隔离

延续 `release-scope-cleanup-plan.md` 第一节的"默认产品不再触碰锁屏"目标，但执行方式以本节的**源码保留、编译隔离**为准，不再按关键字零命中直接删除源码。

**当前需要处理的入口**

- `lib.rs` 的锁屏命令虽已注释，但 `mod lock_screen_backup;` 仍是无条件声明；`windows_integration.rs` 中的锁屏函数、互斥量与测试也未按冻结功能退出编译。
- `src-tauri/tests/lock_screen_backup.rs` 通过 `#[path = "../src/lock_screen_backup.rs"]` 独立引入实现。`cargo metadata` 已确认它是默认可发现的测试目标；只处理库入口不会让默认 `--all-targets` 测试退出这份源码。
- 当前查到的 `file_content_hash` 调用位于 `set_lock_screen`、`copy_sleep_image_without_overwrite` 与相关测试链中，不能预设它是现役素材去重的共享依赖。
- `sha2` 仍被现役 `appearance/importer.rs`、`appearance/exporter.rs` 使用，不能随锁屏隔离一并去掉。

**做法**

1. 先出迁移清单，把 `lock_screen_backup.rs`、`windows_integration.rs` 的锁屏实现、互斥量、专属 import、非 Windows 分支及对应测试补全到现有锁屏归档。`lib.rs` 与前端的冻结命令、适配器、类型及 UI 一并保留到所属归档单元，不再把长注释块散留在现役大文件里。
2. 归档完整后，从现役模块图断开相关声明与引用；独立的 `tests/lock_screen_backup.rs` 入口一起迁出默认测试目录。WorkerW、开机自启、桌面交互与共享类型仍留在现役模块中，不能把整个 `windows_integration.rs` 排除。
3. 核对 full / Lite 的 handler、`build.rs` 命令清单、capabilities 与权限生成入口，默认配置不注册或授权冻结命令；历史接线定义保留在归档中。不能只隐藏前端调用就认为原生入口已关闭。
4. `file_content_hash` 随实际所属功能处理：若无现役调用就一起归档；实施时若发现共享调用，再先抽到 full / Lite 都可用的中性模块，不为清理而强行放进 full 专属的 `appearance/`。保留现役需要的 `sha2`，修正其只描述锁屏用途的注释；其它依赖也按引用核对，不能整批去掉。
5. 更新归档 README，记录原位置、来源 commit 与恢复路径；在迁移前提交上打本地 tag 作为补充锚点，不能把恢复说明仅改成"从 tag 取回"。

**隔离前要确认的一件事**：可能使用过 MSIX 测试安装的机器是否仍处于锁屏接管态。当前接管逻辑要求包身份，但不能据此假设历史安装状态已经清理。若仍有接管态，先确认可用的恢复路径；本次源码隔离不自动恢复锁屏，也不清理、迁移或覆盖用户已有备份与托管图。

**验收**

- 迁出实现、历史测试与接线说明全部能在归档清单中对应，当前工作树可直接查阅源码，不只靠 tag。
- 默认 full / Lite 构建和现役测试不引入锁屏实现；独立测试入口也不再绕过库入口引入它。`cargo metadata` 只验证目标清单，模块依赖仍须另行检查。
- 默认 handler、命令清单与 capability 不暴露冻结功能；现役共享能力与持久化兼容性保持不变。
- 全套现役门禁绿且没有新增 dead_code 警告。装机验证默认运行不新增或改写锁屏备份、托管图及系统锁屏设置，已有用户备份不清理。
- 关键字扫描用于定位残余入口与引用，不再要求 `lock_screen` / `lockscreen` 或 `FREEZE` 全仓零命中；说明、兼容字段与边界测试中的命中逐项解释。

### 2. 其它冻结块：按实际职责保留，不以 UI 冻结判断整个模块无用

本轮优先处理以下三类；这是实施范围，不是全仓 `FREEZE` 的穷举，其它冻结块先另出清单：

| 冻结范围 | 保留与隔离方式 | 仍须留在现役模块中的能力 |
| --- | --- | --- |
| `FREEZE(1B)` 系统集成（透明任务栏、桌面回退等） | 归档对应实现、适配器、UI 与测试；需持续维护的 Rust 功能才用默认关闭的 feature | 开机自启，以及仍被桌面宿主等使用的共享能力 |
| 「启动参数」输入与启动调用中的参数传递 | 归档冻结控件、handler 与调用片段，不整体排除 `connect/launchArgs.ts` | `native/runtime.ts` 仍调用 `parseLaunchArgs`，`connect/endpoints.ts` 仍读取参数端口；解析与端点定位继续参与现役构建 |
| 「起别名」、实例下拉、每实例停止 UI | 分离并归档冻结控件与专属接线 | 仍被使用的标签逻辑、现役停止操作、实例归属过滤与持久化兼容字段；不得因控件退出而整批去掉 |

**测试处理**：先区分现役行为断言、冻结历史用例与源码形状断言。当前 `launchArgsAndInstances.spec.ts`、`ConversationBubble.spec.tsx` 等存在要求原文件保留 `FREEZE` 和历史注释块的断言。对应冻结块迁移时，把这种断言改为检查"归档内容完整、现役入口关闭"；不直接去掉测试来换取绿灯。尚未纳入本轮迁移范围的冻结块及其测试保持原样。

### 3. 前端遗留状态机：旧实现与测试一起归档

`scenes/stateMachine.ts` 里同时有两套状态机：现役的 `reduceRuntime`，以及 v0.1 留下的 `WallpaperStateMachine` / `nextState` / `TRANSITIONS`。后者在生产 `src/` 中没有调用方，只被 `tests/stateMachine.spec.ts` 引用。

- 把旧实现及其专属类型迁到 `archive/frozen/state-machine-v01/`，`tests/stateMachine.spec.ts` 一起保留并迁出默认测试发现范围；在 README 中记录原路径与 import 恢复方式。
- `reduceRuntime`、`RuntimeEvent`、`INITIAL_RUNTIME_STATE` 保持现役，保留 `runtimeState.spec.ts` 等现役覆盖。只剥离旧实现，不能把整个 `stateMachine.ts` 排除。
- 默认 TypeScript / Vite 与现役测试不再引入旧状态机。历史测试归档不是丢失测试，必须单独列明迁移数量和范围。

### 4. 本节统一验收与证据

- **保留证据**：迁移前后逐项核对源码、符号、对应历史测试与恢复说明；混合文件的提取边界必须有清单，不能漏掉注释中保留的实现。
- **隔离证据**：检查 Rust 模块与独立测试目标、TypeScript import / reference、Vite 入口与动态加载、Vitest 发现范围，以及默认 full / Lite 的构建特性。增加边界检查，防止现役代码重新引入 `archive/`；只看 UI 隐藏、关键字扫描或产物大小不算证明。
- **现役门禁**：沿用当前 CI 的 `pnpm typecheck`、`pnpm test`、`pnpm build`、`pnpm build:lite`、Rust full `--all-targets` 测试与 Lite 原生检查 / 测试、Lite bundle boundary，并检查 `git diff --check`。记录实际结果，不把本计划更新当成执行通过。
- **测试基线**：现役用例不得因整理而丢失；明确列出一起归档的历史用例，分别报告现役与归档测试数量。后续拆分步骤以本节完成后的现役测试基线验收。
- **启用侧证据**：选择默认关闭的 feature 时，若声称启用后能编译或恢复，必须运行对应 opt-in 门禁；未运行就标注未验证。归档默认不编译，不作持续兼容保证。

### 5. 执行结果（2026-10-11，分支 `refactor/freeze-isolation`）

迁移前锚点：tag `pre-freeze-isolation`（`3c92772`）。迁移清单见项目文档 `plans/code-structure-step1-inventory.md`；每个归档单元的 README 记录了原路径、迁出符号与测试、恢复接线与未验证范围。

| 批次 | 提交 | 内容 |
| --- | --- | --- |
| B1 | `bb20f1d` | v0.1 状态机 → `archive/frozen/state-machine-v01/` |
| B2 | `876d566`、`1eb7556` | 锁屏 Rust 实现、`lock_screen_backup` 模块及其独立测试目标 → `archive/lockscreen-20260930/` |
| B3 | `79ed691` | `lib.rs` 冻结命令注释、`build.rs` 9 条登记、9 个 permission toml → 两个归档 |
| B4 | `9b24ed3` | 前端锁屏与系统集成片段、孤儿 CSS → 两个归档 |
| B6 | `1d541bc` | 启动参数 / 别名 / 实例下拉 UI → `archive/frozen/launch-ui/`、`archive/frozen/instance-ui/` |
| B7 | `8395a1d`、`67b4342`、`51a7ea6` 及本地重新生成的 `gen/schemas` | CI 归档边界检查；archive 改动触发 CI；过时注释与 README 修正；权限 schema 去掉 9 条冻结命令 |

源码形状断言随对应批次在同一提交中改写为"现役入口关闭 + 归档完整"，未单独成批。

**现役门禁（CI，`1d541bc`）**

| 项 | 迁移前 | 迁移后 | 差值来源 |
| --- | --- | --- | --- |
| 前端 vitest | 551 passed + 5 skipped（556） | 548 passed（548） | 归档 4 条状态机 + 5 条 skip；新增 1 条边界断言 |
| bridge vitest | 96 passed + 1 skipped | 不变 | — |
| Rust full lib | 427 passed / 14 ignored | 404 / 14 | 归档 23 条（`lock_screen_backup` 17 + `windows_integration` 6） |
| Rust full 独立测试目标 | 含 `lock_screen_backup`（17） | 该目标移出 | 同上 |
| Lite lib | 144 passed / 2 ignored | 121 / 2 | 同上 23 条 |
| full lib 警告 | 101 | 45 | 锁屏 never-used 警告随实现移出，无新增 |
| Lite lib 警告 | 208 | 152 | 同上 |
| Lite bundle boundary | OK | OK | — |

**归档测试**：前端 9 条（状态机 4、launch-ui 2、instance-ui 3），Rust 23 条；归档测试不参与默认运行。

**未完成 / 未验证**：装机运行验证未单独执行（被移出的代码在迁移前已无现役调用方，运行行为按构造不变）；用户可见文案中的锁屏残留留待另批。

## 二、Harness 连接状态收成一处（收益最大）

### 现状

一条 Harness 连接的状态现在分散在六处：

- `runtime.harness`（`HarnessAvailability`，来自宿主）与 `runtime.harnessReasonCode`；
- `runtime.harnessProbing`（在 `RuntimeState` 里）**和** `App` 里另一个 `useState` 的 `harnessProbing`——同一个事实存了两份；
- `harnessStarting`、`harnessFailed`、`harnessBuffering` 三个布尔值，加上 `harnessLaunchPendingRef`、`harnessLaunchStartedAtRef` 两个 ref。

它们在 `App.tsx` 里有约 18 处 setter 调用，分布在多个 effect 里。四个布尔能组合出 16 种状态，其中大半没有意义（例如"正在启动"同时"已失败"），`App.tsx` 顶部不少纯函数（`isHarnessTransitioning`、`harnessLaunchOutcome`、`shouldReturnToHarness` 等）实际上是在给这些组合补判断。

### 做法

保留 `HarnessAvailability` 与"probing 和 availability 分开"这个已有的设计决定（理由见 `domain/types.ts` 的注释：probing 时滑槽、模型列表、会话都要继续按"还在"处理）。只把**本地的启动生命周期**收成一个 reducer：

```ts
type HarnessLaunch =
  | { kind: 'idle' }
  | { kind: 'buffering'; until: number }      // 切换主体后的最短黄灯
  | { kind: 'starting'; startedAt: number }
  | { kind: 'failed'; reason: Message }

interface HarnessConnection {
  availability: HarnessAvailability
  reasonCode?: string
  probing: boolean
  launch: HarnessLaunch
}
```

- 事件：`SUBJECT_SWITCHED`、`BUFFER_ELAPSED`、`LAUNCH_REQUESTED`、`LAUNCH_FAILED`、`STATUS_RECEIVED(availability, probing, reasonCode)`。
- 提示灯颜色、滑槽是否可点、要不要显示掉线通知，都从 `HarnessConnection` 用纯函数**派生**，不再单独存。
- 整体封装成 `features/harness/useHarnessConnection.ts`，`App` 只拿到 `{ connection, lamp, requestLaunch, switchSubject }`。
- 删掉 `App` 里重复的 `harnessProbing` state，只留一份。

### 验收

- reducer 的迁移表有逐行测试，覆盖现有 `backendSwitch.spec.ts`、`harnessStatus.spec.ts`、`appChatLifecycle.spec.ts` 里所有 Harness 相关用例，这些用例改为针对新 reducer 或保持原样通过。
- `App.tsx` 里不再出现 `setHarnessStarting` / `setHarnessFailed` / `setHarnessBuffering` / `setHarnessProbing`。
- 真机走一遍：主体在跑 / 不在跑 / 中途关掉主体 / 切换主体，四种情况下灯色与 v0.3.x 一致（录屏留证）。

## 三、拆 `App.tsx`

第二节做完之后再拆，因为 Harness 那一块是最纠缠的部分，先把它抽走，剩下的边界就清楚了。

按职责抽成自定义 hook，每个 hook 一个文件，放在对应的 `features/` 或 `runtime/` 下：

- `useChatAdapter`：适配器的创建、销毁、`chatAdapterLifecycleKey`、过期操作判定（`isCurrentChatOperation`）、流式缓冲。
- `useModelDirectory`：API 与 Harness 两侧的模型目录枚举、记住的选择、`modelOptions` / `modelLabels` / 禁用原因。
- `useDisplayLayout`：显示器列表、多屏背景、虚拟桌面边界、30 秒兜底轮询。
- `useAppearanceAssets`：素材解析与 `appearanceRefreshEpoch`。
- `useInteractionRegions`：热区发布、岛点击上报、右键菜单抑制。
- `useWorkspace`：表 / 里桌面切换、`workspaceEpoch`。

`App.tsx` 顶部那 600 行导出的纯函数跟着各自的 hook 搬走，测试改为从新位置导入。

**目标**：`App.tsx` 降到 500 行以内，只做组装与渲染；单个 hook 不超过 400 行。

**验收**：vitest 全绿且现役测试数不减（以第一节完成后的现役基线为准，归档历史测试单列）；`pnpm typecheck` 绿；真机冒烟（启动、唤醒、进出里桌面、三种后端各发一条消息、切换语言）。

`SettingsPanel.tsx` / `SettingsWindow.tsx` 按设置中心的六个页签拆成六个文件，同样的验收方式，可以放在这一步之后单独做。

## 四、拆 Rust 侧大文件

### 1. `lib.rs` 只做组装

现在 `lib.rs` 同时是：模块声明、72 个 `#[tauri::command]` 定义、Harness 状态缓存（`HARNESS_STATUS_CACHE` 等全局量）、应用启动与关闭、五组测试。并且 full 与 lite 各有一份 `generate_handler!` 列表（83 项与 12 项）要手工保持一致。

**做法**：

- 新建 `commands/` 目录，按领域分文件：`commands/chat.rs`、`commands/harness.rs`、`commands/appearance.rs`、`commands/desktop.rs`、`commands/settings.rs`、`commands/update.rs`、`commands/lite.rs`。命令函数原样搬过去，测试跟着搬。
- Harness 状态缓存与探测客户端搬进 `harness_status.rs`。
- 两份 `generate_handler!` 列表：抽一个宏或在 `commands/mod.rs` 里集中定义共享部分，lite 列表是 full 列表的子集这件事由一条测试或编译期断言保证。

**目标**：`lib.rs` 不超过 600 行。

### 2. `windows_integration.rs` 按职责拆

它现在装着：WorkerW 宿主与首帧挂接、交互热区（`INTERACTION_REGIONS`）、表/里桌面与双击监控、前台检测、窗口句柄登记、锁屏（第一节会保留源码并从默认构建隔离）。拆成 `desktop_host/` 目录下的 `workerw.rs`、`hit_region.rs`、`workspace.rs`、`foreground.rs`，全局 static 跟着各自的职责走；不把归档的锁屏实现重新引入现役模块。

### 3. `unsafe` 集中并写明前提

全项目约 160 个 `unsafe` 块（`windows_integration.rs` 104、`crash_report.rs` 28、`floating_ball.rs` 20、`harness_launch.rs` 10），目前没有一处写 `// SAFETY:` 说明。

**做法**：拆分时顺手建一个 `win32/` 模块，把反复出现的 Win32 调用（`FindWindowExW`、`SetParent`、`SetWindowRgn`、`CreateMutexW`、句柄的打开与关闭等）包成安全的薄封装，每个封装里的 `unsafe` 写一行 `// SAFETY:` 说明前提（句柄有效、缓冲区长度、调用线程）。业务代码不再直接写 `unsafe`。在 crate 根加 `#![deny(clippy::undocumented_unsafe_blocks)]`，CI 加一步 `cargo clippy`。

**验收**：`cargo test --all-targets` 与 lite 目标全绿；`grep -c "unsafe {" ` 在 `win32/` 之外为零或有逐条理由；真机冒烟覆盖 Explorer 重启后的重挂（`startup-diagnostic.log` 里出现 `event=reattach parent=WorkerW`）。

`chat.rs`、`harness_launch.rs`、`crash_report.rs` 的生产代码部分各在 2,500–3,500 行，暂不动，等前面几步的模式稳定后再看。

## 五、防止再长回去

拆完不加约束，几周后会回到原样。建议在 `docs/` 新增一份 `代码结构约定.md`（与 `写作与文案约定.md` 并列，同样写给实现者 agent 看），内容：

1. 单个源文件超过 1,500 行（测试除外）时，新功能不再追加，先提出拆分方案。
2. 新功能默认新建模块；只有改已有行为才动已有文件。
3. 冻结功能保留源码与对应测试，默认迁到仓库根目录 `archive/` 并断开现役入口；需要持续维护恢复能力的 Rust 功能才用默认关闭的编译特性。共享能力与兼容字段留在现役模块，不以 UI 冻结判断整个模块无用。不直接删除、不只靠 tag 留档，也不继续向现役大文件追加长注释实现块；恢复前重新检查兼容性并运行启用侧门禁。
4. 注释描述代码**现在**为什么这样，不写修改历史（"以前这里……踩过一次坑……"）。历史写进提交信息。
5. Rust 里新增的 `unsafe` 只能出现在 `win32/` 下，并带 `// SAFETY:`。

可选：CI 加一步行数检查脚本，对超过阈值的文件只报告不拦截，先观察一段时间。

## 六、顺序与粒度

1. 第一节（保留源码并隔离冻结代码）。先出归档 / 共享能力 / 测试入口清单，再实施迁移；无需先决定"永久 / 临时"，只有选择持续维护的默认关闭 feature 时才另定验证矩阵。
2. 第二节（Harness 连接状态）。
3. 第三节（拆 `App.tsx`，再拆设置中心）。
4. 第四节第 1 条（`lib.rs`）——纯机械搬运，可以和第三节并行。
5. 第四节第 2、3 条（`windows_integration.rs` 与 `unsafe`）。
6. 第五节的约定文件可以在第一步之前就写进去，让后续的实现按它走。

每一步：一个分支、若干提交、门禁全绿后合入；纯搬运的提交与改逻辑的提交分开，方便审查时只看后者。

## 七、已确认原则与仍待确认事项

**已确认**：冻结代码与对应历史测试完整保留，只退出默认构建；默认使用源码归档隔离，不直接删除，也不只留 Git tag。需要持续维护恢复能力的 Rust 功能可选默认关闭 feature，但不以开关替代隔离验证。

**仍待确认**：

- 哪些功能确实需要持续验证恢复能力，以及对应 feature 命名、opt-in 验证矩阵与是否增加可选的归档检查；不再要求在"永久删除 / 临时关闭"之间二选一。
- 第五节的行数阈值（建议 1,500）与是否在 CI 里加检查。
- ~~作者本机的 MSIX 测试安装是否还处于锁屏接管态~~：2026-10-10 作者确认**不处于**接管态，已据此进行锁屏源码隔离。
