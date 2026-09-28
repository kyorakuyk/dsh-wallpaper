# 后台启动的隐藏：契约、实测数字、以及还没测到的那一次

> 2026-09-29。起因是用户报告三件事：(1) 右键壳的托盘图标，菜单一闪就没了（"客户端崩了"）；(2) 用户通过壁纸把客户端拉起来之后，窗口被反复按回去，"十几秒"都在对抗；(3) 启动时窗口那一帧能看见，约 80 毫秒。
>
> 本文件记录修完之后**这一场竞速的契约**、**这台机器上量得到的数字**、以及**必须由用户重启一次壳才能拿到的那一组**（时间线）。

## 0. 结论速览

| 问题 | 处理 | 依据 |
|---|---|---|
| 托盘菜单被吃掉 | 隐藏只作用于**无属主**的顶层窗口，并排除弹出菜单 `#32768`、对话框 `#32770`、工具提示、`Electron_*HostWindow`、`MSCTFIME UI`、`IME` | `client_window::is_hideable_top_level` / `is_popup_or_helper_class`（纯函数，有测试） |
| 窗口被反复按回去 | 契约改成"**一次启动只有一场隐藏、一次对抗**"：第一次藏住之后窗口再出现即归用户；用户明确要过的窗口连一次对抗都不发生 | `harness_launch::wait_for_shell` 的规则 1–3；`RevealIntent` |
| 一帧可见 | 1 毫秒 tick 只盯"壳已经建好、还没显示"的那一个句柄；Bridge 的 `/status` 作为"宿主就绪"的时钟 | `SHELL_HIDE_TICK`、`host_reported_ready`、`client_window::family_windows` |

## 1. 契约（写死在代码里的三条规则）

1. **一次启动只有一场隐藏，而且有绝对上限。** 这一场从壁纸提出启动请求开始，在"第一次藏住 **且** 家族连续 2 秒不在屏幕上"或 45 秒上限（`SHELL_HIDE_EPISODE`）到点时结束。之后这次启动不再藏任何窗口。新的后台启动才是新的一场；别的任何东西都不会重开一场。
2. **第一次藏住之后窗口再出现，这一场立刻结束。** 那是"有人要它"的证据（壳自己的托盘、`second-instance`、用户点了它），用户的显示赢。这条规则替代了老做法 —— 老做法是"只要它露头就再藏一次，直到连续 2 秒不在屏幕上"，而用户每次把窗口拿出来都会重置那 2 秒，于是隐藏越藏越久。
3. **用户明确要过的窗口连一次对抗都不会发生。** 落款在先，循环每一次藏之前都读它：
   - 设置里的「打开」与岛上的「桌面会话」图标 → `harness_launch::note_user_wants_shell`；
   - 循环**看见**壳的托盘菜单（`#32768`）出现在屏幕上 → `note_human_interaction`（循环自己既不弹菜单也不显示任何窗口，所以这是"有人手"的证据，不是循环自己的动作）；
   - 下一次**后台**启动 → `clear_user_wants_shell` 清掉这一款。

上限取 45 秒是因为冷启动"远不止十几秒"（热态实测见第 3 节，约 3.9 秒）。**防止对抗的是规则 2，不是这个上限** —— 上限只是"这一场什么时候彻底结束"。

## 2. 这台机器上量得到的数字

`cargo test --lib -- --ignored --nocapture this_machine_measures_the_hide_race`（release，2026-09-29，本机）：

| 量 | 中位 | 说明 |
|---|---|---|
| 一次 tick 检查（`IsWindowVisible`） | 亚微秒（200 次里最大 1.4 µs） | 这就是 1 毫秒 tick 之所以可能的原因 |
| tick 发现"窗口被显示出来了" | **1.5 ms** | 1 毫秒的 `Sleep` + 一次检查；`TimerResolution` 把它抬到 1 毫秒分辨率 |
| 按下隐藏（`hide_window`） | **8.0 ms** | 跨进程 `ShowWindow(SW_HIDE)`，调完即"不可见" |
| 一次全量家族枚举 | **30.6 ms** | 这台机器有 **538** 个顶层窗口；光枚举一遍 80 µs，Toolhelp 进程快照约 8 ms |
| Bridge `/status` 一次往返 | **0.8 ms** | 回环 HTTP GET（`Transfer-Encoding: chunked`） |

由这些数字推出的、一次后台启动应该看到的东西：

- **成功隐藏的窗口只有一次**（第一次藏住之后不再藏），所以**最多一次闪现**；
- 正常路径（认得出"还没显示的那个窗口"）：闪现 ≈ 1.5 ms 发现 + 8 ms 按下 ≈ **10 毫秒**，约一帧 60 Hz 的显示时间；
- 退化成枚举检测（认不出那个窗口）：闪现上界是密集枚举档 **50 ms**，且密集档只在"宿主就绪/端口应答"之后 10 秒内有；
- 用户按「打开」或点托盘：**0 次隐藏**（规则 3），或者最多被藏一次然后立刻归用户（规则 2）。

**零帧做不到。** 外部没有"别显示"的开关：壳的主进程在宿主发来 `ready` 之后自己调 `window.show()`（`app.asar!/lib/main.js:11569-11576`），壁纸能做的只是比它快。真正的零帧只能靠一个盖在它上面的不透明面（自绘/分层窗口），那有自己的代价，本项目不做。

## 3. 启动为什么慢：壳自己记下来的分阶段耗时

壳自己把启动阶段写在 `%APPDATA%\DSH Desktop\lifecycle-events\startup.jsonl`（最新一次运行，2026-09-27，热态，appVersion 2.0.4）：

| 阶段 | 耗时 |
|---|---|
| electron-ready | 88 ms |
| shell-environment | 184 ms |
| runtime-bootstrap（第一次） | 7 ms |
| profile-selection | 5 ms |
| profile-composition | 524 ms |
| runtime-bootstrap（第二次） | 314 ms |
| **host-boot** | **1831 ms** |
| renderer-startup | 862 ms |
| health-commit | 33 ms |
| **整次启动** | **3852 ms** |

对照同一文件里的时间戳，壳自己的日志（`%APPDATA%\DSH Desktop\logs\dsh-2026-09-27.log`）在 `19:29:22.708–22.713` 连着打了 10 条 workspace-registry 的过滤警告，而 `host-boot` 恰好在 `19:29:22.722` 完成 —— 也就是说那 1.8 秒里确实有插件在干活（工作区登记表要把每个 workspace 的会话头都对一遍）。

**为什么不显示是必然的、而且顺序是串的**（每一段都在等上一段）：

1. 壁纸请 Windows shell 启动别名 → Electron 主进程起来，解析 asar 里的 `lib/main.js`（11830 行），主窗口以 `show:false` 建好（`app.asar!/lib/main.js:11520-11521`、`:10600`）——**窗口句柄从这一刻就存在且不可见**，这就是 1 毫秒 tick 能盯住它的原因；
2. `reconcileBackend()`（`:10953`）先 `await navigateMain(applicationUrl)`（`:10955`），再做 `applyRelease()`（`:10957`、`:3471-3477`：读运行时元数据、迁移 profile 设置、建插件 profile、清 link 投影，全程持 profile 锁）；
3. 才 spawn 宿主子进程（`:3673-3691`）：**另一个 Electron 进程跑在 Node 模式**（`ELECTRON_RUN_AS_NODE=1`，`:3532`），入口 `dsh-desktop-host/lib/index.js`；
4. 宿主侧：模块加载（`index.js:1-10`，含 `dsh-app-boot`、`profile-boot`、`dsh-skill-office` 等静态导入；asar 里 `@deepseek-ai` 树一共 **1234 个 JS 文件、42.7 MB**，且 asar 里**没有任何 V8 code cache 条目**）→ `loadProfileDirectory` + `loadLayeredEnv`（`:221-224`）→ `runProfile({profile:"desktop", args:["--no-open","--port","19387"]})`（`:223-245`）；
5. webserver 先监听（`dsh/node_modules/@deepseek-ai/dsh-host-webserver/lib/index.js:297-302`），**其余插件随后装配** —— 这一条有本仓库自己的证据：`bridge/src/index.ts:1358-1362` 明确写着"一个 Web profile 组装 commands/presets/workspaces 可能比它的 HTTP 监听更慢"；
6. 本仓库的 Bridge 就是这些插件之一：它的 `apply()` 为令牌跑 `whoami.exe` + 四次 `icacls.exe`（`bridge/src/index.ts:44-46`、`:458`、`:531-532`、`:545`）。本机实测一次 `whoami.exe` 38 ms、一次 `icacls.exe` 9.4 ms（列表；授权那一遍同量级），所以这一段是 0.1–0.2 秒量级的串行开销，**每次启动都跑**（令牌文件不轮换：`ensureToken` 先 `restrictTokenFile` 再读，`:567-587`；实测令牌文件最后写入是 8 月 24 日，最后读取是 9 月 29 日）；
7. `await application`（`:323`）之后还要 `await ctx.plugin(office)`（`:326-330`）才发 `{type:"ready"}`（`:337-344`）；
8. 主进程收到 `ready` 之后**还有**几段 await：`authenticateWebHost(ready.url)`、`connectDesktopWelcome`、`analyticsEnabled()`（`:10833-10839`），然后 `openInitialWindow()`（`:10959`、`:11648`）→ `enterWorkspace()`（`:11569`）→ **又一次** `await navigateMain(applicationUrl)`（`:11572`）→ 才 `window.show()`（`:11574`）。

冷启动比热启动慢很多的部分是这一条链的"从磁盘读 + 解析 43 MB JS + 两个 Electron 进程重启 + 若干次本机 HTTP 往返"，而热态那一切都在页缓存里（上面那张表的 3.85 秒就是热态）。本节凡涉及耗时数字的都是实测；"冷启动更慢"这条是推断（机械盘/杀软/首次写 profile 都会加剧），本机没有冷启动的分阶段数据。

**必须写下的一条更正**：用户说的"十几秒"**不是启动耗时**，而是"窗口被反复按回去"的那一段时间 —— 那是老隐藏循环的设计缺陷（每显示一次就重置 2 秒静默期），第 1 节的规则 2 就是它的修法。

## 4. 时间线（仪表盘）与用户要怎样拿到那一组数

每次后台启动都会打一串带自带宽度的行，标签是 `[launch-timing]`，每行都带 `+Nms`（相对"壁纸提出启动请求"）。日志文件是
`%LOCALAPPDATA%\com.dsh.wallpaper\logs\dsh-wallpaper.log`（时间戳只到秒，所以毫秒必须由这些行自己带）：

| 行 | 含义 |
|---|---|
| `requested +0ms trigger=Automatic\|Slider\|Manual` | 壁纸向 Windows shell 提出启动请求 |
| `family-process … probe=100ms` | 壳的进程家族第一次出现（探测粒度 100 ms） |
| `family-windows count=… on_screen=…` | 家族第一次有了顶层窗口 |
| `port-answered …` | 19387 第一次应答 |
| `bridge-ready … poll=5ms` | **Bridge 自己报告宿主就绪**（`state == "bridge-ready"`） |
| `window-exists-hidden pid=… class=…` | 认出"已经建好、还没显示"的那个窗口 |
| `window-visible` / `hide-landed` / `visible-frame Nms` | 第一次看见它在屏幕上 / 藏掉它 / **那一帧有多长** |
| `tray-menu` / `reappeared` / `user-wants` / `settled` / `episode-over` | 这一场为什么结束 |

**要拿到实测数字，请做这一次（壁纸必须重启过一次，装上这一版）**：

1. 确认壳**没在跑**（托盘菜单里退出 `DeepSeek Harness`；壁纸自己不会去停它）；
2. 在设置里用「启动参数」或直接让开机自启那条路（滑槽也行）把官壳**以后台方式**拉起来 —— 也就是别用设置里的「打开」；
3. 等它起来（窗口应当不出现，或最多闪一下）；
4. 看日志：`%LOCALAPPDATA%\com.dsh.wallpaper\logs\dsh-wallpaper.log`，把这一行抓出来 ——
   `harness shell launched: aumid=com.deepseek.dsh trigger=… confirmed=… hidden=… visible_frame_ms=…`
   以及它前面那一串 `[launch-timing]` 行。
5. 把这两段贴回来，就能回答"Bridge 在壳显示窗口之前还是之后"这个问题，并且得到这台机器上的真实帧长。

## 5. Bridge 在 `window.show()` 之前还是之后（现在能答的部分 + 还欠的部分）

**已经能支持的**（代码与既有取证）：

- 壳显示窗口的条件是"宿主子进程发来 `ready`"（`app.asar!/lib/main.js:11569-11576`、`:3704-3707`、`:3633-3634`），而 `ready` 由宿主在 `await application` **之后**、`await ctx.plugin(office)` **之后**才发（`dsh-desktop-host/lib/index.js:323-344`）；
- 本仓库的 Bridge 是那个 application 里的一个插件，它的 `/status` 在 `['webServer']` 作用域挂上、并且要等令牌就绪（`bridge/src/index.ts:1363-1397`），因此 **`bridge-ready` 必然早于 `ready`，也就必然早于 `show()`**；实测这条路由在这台机器的 19387 上公开可读（200，`"state":"bridge-ready"`，无令牌）；
- 顺序上"端口应答"更早也更糊（webserver 先监听、插件后装配），所以两个信号都留着：谁先到用谁。

**还欠的部分**：上面是"代码顺序 + 路由语义"推出的结论，**不是这一次启动的实测时间差**。`[launch-timing]` 的 `bridge-ready`、`window-visible`、`hide-landed` 三行会在用户做完第 4 节那一次之后给出真正的毫秒差；在那之前，"Bridge 先于 show"只能说到"依据充分"，不能说成"已实测"。

## 6. 复现命令

```powershell
# 真机测量（自建一个离屏窗口，不动任何正在运行的程序）
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --release --lib -- `
  --ignored --nocapture this_machine_measures_the_hide_race

# 门禁
pnpm -C wallpaper typecheck
pnpm test
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --lib
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --no-default-features --features lite --lib
cargo check --manifest-path wallpaper/src-tauri/Cargo.toml --locked --no-default-features --features lite --bin dsh-wallpaper-lite
```
