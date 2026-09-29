# DSH 的 profile、端口与会话归属（取证）

取证目标：把"端口 ↔ profile ↔ 对话"三者的关系钉死，供"并行启动多个实例 + 每个 checkout 一个别名 + 更新检查"这三个设计决定使用。
只写读到的证据；读不到的地方单列在最后一节。

## 0. 取证对象与版本（先说清楚，否则行号会被误用）

| 对象 | 位置 | 版本 |
| --- | --- | --- |
| 本机源码检出 | `D:\Family\DeepSeekHarness\deepseek-harness` | `apps/cli/package.json` 写 `0.1.0-rc.5` |
| 已安装 CLI（真正在跑的那份） | `C:\Users\rnfmabj\AppData\Roaming\npm\node_modules\@deepseek-ai\dsh` | `0.2.0-rc.1` |
| 官方桌面壳内的 dsh 运行时 | `D:\Family\dsh-official\resources\app.asar`（`dsh/package.json` 写 `@deepseek-ai/dsh-desktop-runtime` `0.2.0-rc.1`） | `0.2.0-rc.1`，构建 commit `62962ee48ef60ed1f97c10fffceb294eb3adf719` |
| 用户数据根 | `C:\Users\rnfmabj\.dsh` | 只读查看 |

**两者不同版本**：检出是 0.1.0-rc.5，本机实际运行的是 0.2.0-rc.1。凡涉及"现在会怎样"的结论，优先引用 0.2.0-rc.1 的编译产物（`node_modules/@deepseek-ai/*/lib/*.js`、`*/cordis.patch.yml`）或壳内解出的文件；引用检出源码时会写明"检出 0.1.0-rc.5"。我逐条核对过的关键行在两版里一致（已知的位置差异只有一处：`storage-json` 的 `root` 行在检出里位于 web-app 的 patch、在 0.2.0-rc.1 里位于 base 的 patch，值相同）；无法逐行核对的壳内实现见第 6 节第 5 条。

`app.asar` 用 PowerShell 打不开成目录（`Test-Path` 返回 False）。我写了两支只读脚本，按 asar 头部的 JSON 索引直接抽文件/扫描字节（脚本放在 `%TEMP%\dsh-evidence-scan\`，不在本仓库内）：
- `asar-index.mjs <asar> list|dump|at <参数>`：列条目、按路径导出、按字节偏移定位条目；
- `asar-scan.mjs <asar> <路径过滤> <字节串> [每文件最多几处]`：在指定路径前缀的 js/json/yml 文本里找字节串并打印上下文。

由此确认：`19387` 在整个 118 MB 的 asar 里**只出现一次**（条目 `dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js`，文件内偏移 9727）。

## 1. 端口与 profile 的关系

**结论：两者没有任何绑定。端口是 web 表层的本次调用参数，profile 是启动器的参数，互不知道对方的存在。**

启动器只管自己的 flag，后面的一律交给被启动的树，由 app 插件自己解析：`apps/cli/src/args.ts:1-16` 的模块注释 + `parseDshArgs`（检出 0.1.0-rc.5）；对应 `dsh-cmdline` 的注入机制 `packages/boot/cmdline/src/index.ts:2-16`。Windows 上真正在跑的那份进程命令行是
`"node" "...\@deepseek-ai\dsh\lib\bin.js" --profile web --no-open`（`Get-CimInstance Win32_Process` 实读，PID 4980）。

端口的决定链（0.2.0-rc.1，已安装的编译产物）：

1. `dsh --profile web --port N` 由 `web-startup` 插件解析成 `webStartup` 服务：`...\@deepseek-ai\dsh-web-app\lib\startup.js:22`（flag 表：`--host`、`--no-open`、`--port`、`--trusted-host`），`:38-48`（非数字 `--port` 直接报错退出；`--host 0.0.0.0` 被明确拒绝）。
2. webserver 行读这个服务，**默认值就写在 bundle 的 patch 行里**：
   `...\@deepseek-ai\dsh-web-app\cordis.patch.yml:173-174` → `host: !!js ctx.webStartup.host ?? '127.0.0.1'` / `port: !!js ctx.webStartup.port ?? 3080`。
   也就是说 3080 是 bundle 层的一个字面量，不是环境变量、也和 profile 无关；任何更后的 patch 层（profile 的 `cordis.patch.yml`、`--patch`）都可以整行替换它。
3. `--port 0` 让操作系统分配：`packages/host/webserver/src/index.ts:48-49`（"zero requests an OS-assigned port"）、`:78-81`（`get port()` 返回实际绑定值）、`:218-221`（`listen` 回调里取 `server.address().port`）。
4. 端口被占：`listen` 失败会**拒绝该行（fiber）的初始化**，"the boot process reports the failed fiber"（同上 `:56-57`）。没有"自动换端口"或"检测到已有实例就退让"的逻辑。

19387 与 43120 的来历：

- 官方桌面壳里 bundled 的 `@deepseek-ai/dsh-desktop-host` 调 `runProfile({ environment: loadLayeredEnv("dsh"), profile: "desktop", resolvedProfile: {...}, patchFiles: [], args: ["--no-open", "--port", "19387"], ... })`（`app.asar` → `dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js:223-245`）。所以"壳跑 `desktop` profile、监听 19387"是**这份文件写死的**，不是 DSH 的规则。
- 同一个文件 `:337-344` 把真实 URL 报给 Electron 主进程：`ctx.connection.authenticatedUrl(\`http://127.0.0.1:${String(ctx.webServer.port)}\`)`，以 `{ type: "ready", url, injections }` 发出。**端口不是壳的契约**：壳自己拿的是运行时端口。而 profile 的 patch 层可以把 webserver 行的 `config` 整行换掉（于是那行的 `port` 不再来自 `ctx.webStartup.port ?? 3080`，`--port` flag 对它就失效）——这条我没有实测，只从 patch 语义与行配置读出（见第 6 节第 4 条）。
- 43120 只出现在壁纸仓库自己的默认候选表里（`wallpaper\src\connect\endpoints.ts:79-96` 等），DSH 侧没有这个值。

**同一 profile、不同端口：什么共享、什么独立**

实测（隔离规则下跑的探针，脚本 `%TEMP%\dsh-evidence-scan\probe-isolation.ps1`）：`DSH_HOME` 指向临时目录，`--profile web`，同 cwd，端口 3199 / 3198 / 3197：

```
A1 (home-a, port 3199) listening: True
A2 (home-a, port 3198) listening: True
A1 still listening after A2 boot: True
B1 (home-b, port 3197) listening: True
stdout（只取 URL，不打印 token）: http://127.0.0.1:3199 / :3198 / :3197
```

同 DSH_HOME 的两个进程同时监听、同 profile 名、同 cwd，谁也没拦谁。三个进程跑完即停（`Stop-Process`，含子进程），用户的实例没被碰。

- **独立**：进程内存里的一切 —— 已挂载（attached）的会话与 agent handle、SSE 订阅、后台任务、审批等待、`ctx.*` 服务与插件实例、每个进程自己的 token（web 根 URL 的进程 token）。清单页的"运行中"状态也是各进程自己的（`api-proxy.ts:1731-1739` 从 `ctx.agents` 取）。
- **共享**：所有以 DSH_HOME 为根的磁盘状态 —— profile 目录（插件清单、`cordis.patch.yml`、pnpm 依赖树）、`sessions/`、`storages/`、`settings.yaml`、`.credentials.yaml`、`.env`、`.anonymous-user-id`、`attachments/`、`.agent-presets/`、`logs/`，以及插件自己写进 home 的固定路径（见第 3 节）。共享方式不是"加锁的共享"，而是"各写各的"：见第 3 节的两处明写。

## 2. profile 与会话归属

**结论：会话按 DSH_HOME 归属，不按 profile、也不按端口。**

证据链（每一环都能独立核对）：

1. 会话日志根只有一处配置：`- id: session-persistence-jsonl / config: root: !!js dshHomePath('sessions')`（0.2.0-rc.1 已安装 `...\@deepseek-ai\dsh-base\cordis.patch.yml:130-133`；检出同址 `packages/bundle/base/cordis.patch.yml:98-101`）。`dshHomePath()` = `resolveDshHome() + 段`（`packages/util/home-paths/src/index.ts:98-100`）。没有任何一层把它按 profile 分开。
2. 磁盘布局由 cwd 与会话 id 决定，没有 profile 维度：`sessions/<projectKey(cwd)>/<encodeSegment(id)>/session.jsonl.zstd`（`packages/session/session-persistence-jsonl/src/format.ts:147-208`）。用户目录里能直接看到 `--C-Users-rnfmabj--`、`--D-Family-DeepSeekHarness-plugins-dsh-wallpaper--`、`_no-cwd` 这类目录名（`Get-ChildItem ~\.dsh\sessions`，11 个）。
3. 会话 header 的字段是**穷举的**且不含 profile：`version / id / createdAt / cwd / parentSession / seedLength / origin / delegationDepth / agentPreset`（`format.ts:33-44`）。也就是说"这条会话属于哪个 profile"这件事在数据里根本不存在。
4. `list()` 遍历 root 下**所有** project 目录并返回所有 header（`packages/session/session-persistence-jsonl/src/index.ts:446-504`）。
5. 侧栏的数据来源就是它：`session.list` → `listVisibleSessionSummaries()`，注释写明"Attached sessions come from memory; servable cold sessions merge from persistence"（`packages/host/apiproxy/src/api-proxy.ts:1724-1745`、`:2036-2037`）。冷会话只过滤 `meta.cwd !== undefined`，没有 profile 过滤。
6. 工作区分组与"归档"集合来自 `storages/workspace.json`（web 层插入 `- id: workspace`，检出 `packages/bundle/web-app/cordis.patch.yml:73-74`；storage-json 的 `root: !!js dshHomePath('storages')`，0.2.0-rc.1 已安装 base `cordis.patch.yml:168-171`）。归档集是 registry-global（`packages/workspace/workspace/src/index.ts:228-253`），同样是 home 级。

所以：**A 实例跑 `desktop`、B 实例跑 `web`、两者同一个 DSH_HOME 与同一个工作区 W，它们的会话列表是同一份**（同一条条的 id、标题、归档状态），差别只在"哪些是 attached/运行中"和"本实例挂了哪些工具"。

**能不能换个 profile resume 同一条会话：能，但会按那个实例的 roster 重建组合。**

- resume 时 preset 由日志重建：`resolveSessionPreset()` 从 header 取创建时的值，再被日志里最后一条 `agent-preset/selected` 覆盖（`packages/preset/agent-presets/src/session.ts:38-53`）；api-proxy 在开会话、fork、查历史时都走它（`api-proxy.ts:548`、`:1274`、`:1655`、`:1706`、`:2425`）。
- 解析不出去就失败：`UnknownPresetError` / 已发现但坏掉的 `PresetMountError`（`packages/preset/agent-presets/src/index.ts:213-239`）。
- preset 的 roster 是**安装侧 + DSH_HOME**，不是 profile 侧：随包发布的 `presets/*.patch.yml`（0.2.0-rc.1 的 `dsh-web-app\presets\`：`cordis/minimal/ptc/standard`）+ 用户层 `$DSH_HOME/.agent-presets`（`packages/preset/agent-presets/src/index.ts:131-134`）。因此同一台机器上 `desktop` 与 `web` 的 preset roster 通常一致，换 profile resume 不会因为 preset 缺席而失败。
- 真正会变的是**工具与提示词**：profile 决定 host 平面挂了哪些插件与行（壁纸桥、memento、process-folding 等都写在各自的 `profiles/<name>/package.json` 的 `dsh.profile.bundles` 里，实读用户目录：`desktop` = base + web-app + `dsh-memento` + `dsh-wallpaper-bridge`；`web` = base + web-app + `dsh-process-folding` + `dsh-wallpaper-bridge`）。在 `web` 里 resume 桌面会话，历史照旧，但桥给桌面会话注入的入口简报与 preset 不会被那个 profile 重新注入（那是桥自己的行为，见 `bridge/src/index.ts:351-367`）。
- 还有一个容易忽略的坑：**profile 名本身是被产品使用的语义**。随包组合里有按名字启停的行，例如 `desktopPlatform: !!js "ctx.get('profileContext')?.name === 'desktop' && ..."`（0.2.0-rc.1 base `cordis.patch.yml:115`）与 web-app 里三处 `disabled: !!js "ctx.get('profileContext')?.name !== 'desktop'"`（`dsh-web-app\cordis.patch.yml:47,59,280`）。把官方壳的 profile 改名会让 desktop 专用行集体消失。反过来，profile 名与目录对工具可见：每次 shell 工具调用都会拿到 `DSH_HOME` / `DSH_SHELL` / `DSH_SESSION_ID` / `DSH_PROFILE` / `DSH_PROFILE_DIR`（0.2.0-rc.1 `...\dsh-shell-env\lib\index.js:84-94`），所以"给每个 checkout 一个别名"如果是**新 profile 名**，模型侧也能感知。
- 顺带纠正一个仓库内的误认：`~/.dsh/visible-sessions.json` **不是 DSH 写的**。它的写入点在壁纸桥自己：`bridge/src/index.ts:1106-1125`（"临时诊断（拿到答案就删）"），与 token 同目录。DSH 源码里搜不到 `visible-sessions` 这个字符串。

## 3. 两个实例同时跑，哪些路径会撞车

| 路径 | 谁写 | 同 DSH_HOME 两个实例的行为 |
| --- | --- | --- |
| `profiles/<name>/`（package.json、cordis.patch.yml、pnpm-workspace.yaml、node_modules、`.plugin-manager/`） | 启动器 + 插件管理器 | 按**名字**共享同一份。同名 profile 的两个实例读同一份插件集与同一个 patch 文件；一方的 `dsh plugin --profile <name> add` 改的是另一方下次启动会读到的东西 |
| `profiles/node_modules/`（扁平回退链接） | 每次启动 `healProfilesModuleFallback` | 每次启动都按**当前安装**重指链接（`packages/boot/app-boot/src/profile.ts:204-255`），并发用 EEXIST 容忍（`:189-202` 明确写"Concurrent launches heal the same fallback"）。不同 checkout / 不同版本的两次启动会互相重指这份共享回退表 |
| `sessions/<project>/<id>/session.jsonl.zstd` | 每个写入事件的进程 | 共享文件。追加是 `open(path,'a')` 后整批写 + `fsync`，失败时**回滚到写前字节数**（`session-persistence-jsonl/src/index.ts:646-689`）；全程没有跨进程锁。两个进程同时 resume 同一条会话并发消息，就是两个写者交替 append 同一个文件，且一方的回滚会 `truncate` 掉另一方刚写进去的字节 |
| `storages/workspace.json`、`storages/session_projcache.json`、`storages/session_projcache/sessions/<id>.json` | storage-json 后端 / 投影缓存 | 共享且**内存权威、打开后不重读**：`unit.ts:1-7`（"The in-memory state is authoritative"）、`:24-45`（只在 open 时 `readFile` 一次）。提交是整文件原子替换，设计前提写得很直白："a unit file has exactly one writer per process and last-write-wins is correct"（`storage-json/src/atomic.ts:1-12`）。两个实例各自持有 workspace.json 的一份内存状态并整文件覆盖，互相看不到对方新增的工作区/会话归属，后写的覆盖先写的 |
| `settings.yaml` | `@deepseek-ai/dsh-settings-file` | 共享（`<harness home>/settings.yaml`，`dshHome?: 默认 $DSH_HOME`，`settings-file/src/index.ts:22-25,50-56`）。写侧有 `withFileLock`（`<file>.lock`，`wx` 创建、20ms 起指数退避、2s 超时**抛错而不是抢锁**：`packages/util/atomic-write/src/index.ts:71-118`），所以不会互相踩字节，但"哪个实例最后写"决定两个实例下次读到的默认模型等设置 |
| `.credentials.yaml` / `.env` / `.anonymous-user-id` | credentials-local / 启动器 / 身份 | 共享。全新 DSH_HOME 第一次启动就会建出 `.credentials.yaml`（实测 home-a/home-b 顶层都只有 `profiles`、`storages`、`.credentials.yaml`） |
| `attachments/v1/` | attachment-local | 共享（`resolve(join(resolveDshHome(config.dshHome), 'attachments', 'v1'))`，`packages/attachment/attachment-local/src/index.ts:53`） |
| `logs/startup-<时间>-<uuid>.log` | 启动失败诊断（只有启动审计失败才写） | 目录 `$DSH_HOME/logs`、文件名带随机 UUID、`flag: 'wx'`，不会撞（0.2.0-rc.1 `lib/bin.js:157-193`） |
| `wallpaper/bridge-token` | 壁纸桥 | **共享同一个 token，且是有意为之**：`defaultTokenRoot()` = `$DSH_HOME` 或 `~/.dsh`（`bridge/src/index.ts:242-244`），路径 `<tokenRoot>/wallpaper/bridge-token`（`:249-260`），`ensureToken` 优先复用已存在的 token，代码注释写明了原因："Rotating it on every startup breaks an already-running bridge when a second DSH launch races for port 3080"（`:577-598`）。用户的两份 profile（`desktop` 与 `web`）装的是同一个 `dsh-wallpaper-bridge`（`file:D:/Family/DeepSeekHarness/plugins/dsh-wallpaper/bridge`），而 `bridge/cordis.patch.yml` 只 `insert` 了 id、**没有给任何 config**，所以两个实例的桥解析到同一个 token 文件、同一个桌面工作区路径（`%LOCALAPPDATA%\com.dsh.wallpaper\桌面会话`，`bridge/src/index.ts:282-286`）。 |
| `wallpaper/visible-sessions.json` | 壁纸桥（临时诊断） | 同目录、固定文件名、`writeFile` 直接覆盖（`bridge/src/index.ts:1114-1125`），两个实例是后写覆盖先写。只是取证文件，不影响主流程 |
| `dsh-memento/memory.db`（+`-wal`/`-shm`） | `dsh-memento` 插件（只有装了它的 profile，例如用户目录里的 `desktop`） | 插件自己的固定路径，同一 DSH_HOME 里装了它的两个实例会共用同一个 SQLite 库（用户目录实读：212992 / 20632 / 32768 字节） |

**"两个并发实例的插件会不会为了固定路径打架"——就壁纸桥而言：不会为 token 打架（复用同一个 token 是它写明的设计），只会抢那个诊断文件。真正的风险在别处：同一 DSH_HOME 下两个实例的 `sessions/` 与 `storages/` 是无锁共享，见上表。**

## 4. `DSH_HOME`

**结论：它是唯一且受支持的用户数据根；要"完全隔离的实例"，设它就是正确做法。**

- 解析优先级：显式配置 > `$DSH_HOME` > `~/.dsh`；空串或纯空白视为未设置（`packages/util/home-paths/src/index.ts:56-91`，官方壳内 `app.asar` → `lib/main.js:56-59` 是同一份实现）。
- 它 scope 的东西（全部实测或读源码）：

  - `profiles/<name>`（`resolveProfileDir` = `join(home, 'profiles', name)`，0.2.0-rc.1 已安装 app-boot `lib/index.js:524-527`）
  - `sessions/`（base `cordis.patch.yml:130-133`）
  - `storages/`（base `cordis.patch.yml:168-171`）
  - `settings.yaml`（`settings-file` 默认）
  - `.credentials.yaml`（credentials-local：`$DSH_HOME/.credentials.yaml` 优先于 `$DSH_HOME/.env`，`packages/credentials/credentials-local/src/index.ts:1-10`）
  - `attachments/v1`、`.anonymous-user-id`、`.agent-presets`、`profiles/node_modules`、`logs/`
  - 用户 `.dsh` 顶层实读目录：`attachments cache costs dsh-memento dsh-runtimes llm-deepseek logs profiles sessions storages wallpaper workspace` + `.anonymous-user-id .credentials.yaml* settings.yaml.imported visible-sessions.json`

- **`DSH_HOME` 不能被 `.env` 改**：`DSH_` 前缀属于 bootstrap-only，只允许由启动环境提供，写进 `.env` 会直接让启动失败并给出"export 它"的提示（0.2.0-rc.1 app-boot `lib/index.js:3358-3388`、`:3410-3416`）。环境层本身的顺序是：继承的进程环境 > 调用目录的 `.env` > `$DSH_HOME/.env`，且已存在的继承值不会被覆盖（`loadLayeredEnv`，同文件 `:3422-3458`）。
- 全新 DSH_HOME 的实测结果（探针，`--profile web`，端口 3199）：
  `home-a` 顶层 = `profiles`、`storages`、`.credentials.yaml`；`profiles/` 下自动出现 `web`（来自 shipped 模板 `PROFILE_TEMPLATES.web`，已安装 app-boot `lib/index.js:529-535`）；`storages/workspace.json` 已生成；`sessions/` 尚未创建（第一次落盘才建，`Config.root` 注释：`session-persistence-jsonl/src/index.ts:60-68`）。`home-b` 同样独立。也就是说：换 DSH_HOME 会同时换掉 profile、会话、凭据、设置、附件 —— 是一个"整机替身"，不是"只换对话"。
- 官方桌面壳吃的是**同一个** DSH_HOME：壳的 `resolveDesktopPaths(dshHome = resolveDshHome())` 得出 `profile = <home>/profiles/desktop`（`app.asar` → `lib/main.js:68-73`），子进程环境由 `desktopNodeEnvironment` 原样透传只加 `ELECTRON_RUN_AS_NODE`（`:3529-3537`），spawn 时 `cwd = projectDir`、`env = desktopNodeEnvironment(...)`（`:3674-3691`）。所以给壳设 `DSH_HOME` 会把 `desktop` profile 一起搬走。
- 但壳**不是**完全在 DSH_HOME 里：Electron 自己的 userData（`app.getPath("userData")`：快捷键 `keybindings.json`、`background-close-confirmed` 标记、更新日志）不在 DSH_HOME 下（`lib/main.js:9857-9918`、`:11451`）。这一点在本机可以直接看到：正在运行的壳进程带的参数是 `--user-data-dir="C:\Users\rnfmabj\AppData\Roaming\@deepseek-ai\dsh-desktop"`（`Get-CimInstance Win32_Process` 实读，PID 44960 及其子进程），与 `~\.dsh` 无关。
- 只想隔离"会话历史"而不隔离凭据/设置时，还有一条更窄的路：在 profile 的 patch 层覆盖 `session-persistence-jsonl.root`（以及需要时 `storage-json.root`）—— 因为这两行就是普通行，profile 的 `cordis.patch.yml` 在 bundle 层之后应用（`packages/boot/app-boot/src/profile.ts:5-21` 的层序说明）。凭据、设置、附件仍是 home 级。

## 5. 建"多实例 + 每 checkout 一个别名"时容易摔的其它事

- **官方桌面壳有进程级单实例锁**：`application.requestSingleInstanceLock()` 失败就 `application.quit()`，成功则监听 `second-instance` 把已有窗口拉到前台（`app.asar` → `lib/main.js:6808-6823`）。所以"并行开两个官方壳"不成立，多开只会聚焦已有窗口（壁纸仓库自己的目录缓存里也记了 `singleInstance: true`，`wallpaper\src-tauri\src\harness_catalog.rs:156`）。要多实例只能走 CLI/web 这条线（或给壳换 userData）。
- **壳另有一把 profile 级锁文件**：`<home>/profiles/desktop/lock`，`openSync(...,'wx',0o600)` 创建、里面写 PID、`finally` 删除、已存在时用 `process.kill(pid, 0)` 判断陈旧（`lib/main.js:3479-3513`）。它只包住壳自己的 profile 维护动作（`disableAllPlugins` / `applyRelease`），**CLI 的 `dsh plugin --profile ...` 不走它**；CLI 侧 profile `package.json` 的写入走 `withFileLock`（`dsh-config-editor\lib\index.js:72`，锁实现见 `packages/util/atomic-write/src/index.ts:71-118`；注意超时是抛错，不是抢锁）。
- **CLI/web 侧没有单实例锁、没有 profile 锁**：实测两个进程同 DSH_HOME、同 profile 名、同 cwd 各占一个端口并存（第 1 节输出）。启动器层面唯一被"考虑过并发"的地方是 `profiles/node_modules` 的回退链接（EEXIST 容忍）。
- **端口冲突的失败形态是启动失败**，不是降级或重试（第 1 节第 4 点）。给每个实例分配固定端口时，要么自己先探测占用，要么用 `--port 0` 并读回实际端口（`--port 0` 是官方支持的，见 flag 说明）。
- **"profile 名"不是"checkout 隔离"**：profile 按名字存在于 DSH_HOME 下，与 dsh 安装位置无关。两个 checkout（例如 0.1.0-rc.5 的检出与 0.2.0-rc.1 的安装）用同一个 profile 名，会共用同一份 `package.json` / `cordis.patch.yml` / 依赖树，并且每次启动都会把共享的 `profiles/node_modules` 回退表重指到当前安装（第 3 节第二行）。要做"每个 checkout 一个别名"，要么一 checkout 一个 profile 名，要么一 checkout 一个 DSH_HOME；只换端口什么也隔离不了。
- **设置面是 home 级**：`settings.yaml` 一份（模型选择、插件设置都在里面；该文件被监视并热重载，设置行支持 `!!js` 表达式）。两个实例的用户可见设置会互相影响。
- **profile 名是运行期语义**（第 2 节末）：`profileContext.name` 已经被随包组合用来开关 `desktop` 专用行；同时 `DSH_PROFILE` / `DSH_PROFILE_DIR` 会出现在每个 shell 工具调用的环境里。
- **版本错位这一条对"更新检查"直接相关**：壳内运行时与已安装 CLI 同为 `0.2.0-rc.1`，但本机源码检出是 `0.1.0-rc.5`；壳内 `dsh-desktop-host` 调 `runProfile` 时传了 `resolvedProfile` 与 `packageManager` 两个参数（`app.asar` → `dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js:223-245`），而检出的 `RunProfileOptions` 只有 `environment/profile/patchFiles/args`（`apps/cli/src/profile-boot.ts:173-183`）—— 说明壳里那份 profile-boot 比检出新。做版本/兼容性判断时，不能拿检出当"当前实现"。

## 6. 没能确定的部分（不要当成结论用）

1. **"两个 profile 的侧栏确实列出同一条会话"没有用 HTTP 实测**。第 2 节的结论来自三处源码闭合（会话根只有一处配置、header 无 profile 字段、`list()` 全量遍历 + 侧栏数据来源就是它）。要实测需要驱动 `/api/session.list`（或创建一条会话），在当前授权范围内我没有做（创建会话可以只空白创建，但空白会话是否立刻落盘、以及是否值得为它引入一次 LLM 调用外的副作用，我没有把握，所以没做）。
2. **两个进程同时 append 同一条会话日志的实际损坏形态没有实测**。读到的是"没有跨进程锁 + 回滚会 `truncate` 到写前字节数"，方向明确，但"会读到什么、能不能自愈"没有测。
3. **两个实例同时写 `storages/` 的丢更新没有实测**。读到的是"内存权威 + 不重读 + 每文件单写者"的设计前提；实际丢多少、会不会自愈没有测。用户目录里 `storages/session_projcache.json` 与 `session_projcache/sessions/*.json` 并存，说明投影缓存本身是 fail-soft 设计，但 workspace.json 没有这层说明。
4. **官方壳能否通过 patch 改端口没有实测**。读到的是端口值来自 `webStartup` 与行配置两处，而 patch 会**整行替换**目标行的 `config`（检出 `packages/bundle/web-app/cordis.patch.yml:5-6`："A patch replaces the targeted row's whole `config`, so each row below restates every key it owns"）。真机上改壳端口的后果（含壳内 `ready` 上报、壁纸侧的钉住逻辑）没有验证。
5. **壳内那份 app-boot 的每一行没有逐行核对**。我只抽了 `dsh-desktop-host`、`lib/main.js`、`package.json`、`dsh/package.json` 等条目并用字节扫描定位了 `19387`；壳内 `@deepseek-ai/dsh-app-boot` 的完整实现没有全部导出阅读。凡涉及壳内行为且与检出不一致的地方，本文件按"读到的片段"陈述。
