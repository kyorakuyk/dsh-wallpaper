# 开机壁纸预渲染与 WorkerW 交接实施计划

> 给下游编码 Agent 的任务书。目标是在 Windows 登录后尽早显示项目画面，并让原生首帧、WebView 动画及 WorkerW 挂载之间尽可能无跳变。本文是待实施方案，不代表相关功能已完成。

## 0. 任务边界与交付方式

在 `D:\Family\DeepSeekHarness\plugins\dsh-wallpaper` 工作。开始前检查 `git status --short --branch`、当前 HEAD、仓库内 `AGENTS.md`（若存在）和在途改动。保留用户已有修改；不要在损坏的 `.git-corrupt-20260922` 或旧的 `dsh-wallpaper-rebuild` 副本工作。

按下面的阶段顺序实施，逐阶段给出改动、测试结果和实测证据。前一阶段无法证明可用时，停在该阶段说明原因，不要继续增加窗口、计时器或后台服务来掩盖问题。不要自动 push、打包签名、安装 MSIX 或修改真实用户的系统壁纸；这些不属于本任务的自动实施步骤。

目标场景是 **Explorer 普通桌面**。不接管 Windows 密码页，不创建 Credential Provider，不使用 SYSTEM 服务，不写 `NoLockScreen` 等机器级策略。

## 1. 当前实现与待解决的问题

现在的链路如下：

1. `wallpaper/src-tauri/src/main.rs` 在 Tauri 初始化前调用 `prepare_native_bootstrap()`。
2. `native_bootstrap.rs::prepare()` 查找 WorkerW；找不到独立 WorkerW 时，`request_wallpaper_worker()` 可返回 Progman。它加载 `sleep.png`，创建原生 HWND，按显示器分别 `cover` 绘制。
3. Tauri `setup()` 调用 `windows_integration::start_wallpaper_host()`。`create_background_window()` 创建隐藏的单个 `background` WebView，`attach_to_workerw()` 随即将其改为桌面子窗口、调整尺寸并 `ShowWindow`。以后 2 秒看门狗负责恢复/从 Progman 升级到 WorkerW。
4. 前端在 idle 的两次 `requestAnimationFrame` 后，或苏醒动画首次切到非睡眠帧后，调用 `release_native_bootstrap()`。该命令只隐藏原生层，以便以后锁定/解锁复用。

当前缺口：

- WebView 在附着时并未保证页面资源、目标尺寸和第一帧已就绪；
- `requestAnimationFrame` 表示 JS 获得绘制机会，不证明 DWM 已显示匹配画面；
- 原生层初始使用 `HWND_BOTTOM`，WebView 随后显示。必须实测并确保两者在同一宿主中保持“桌面图标 > 原生遮盖层 > WebView > 系统壁纸”的有效可见顺序；不能从常量名称推断层级一定正确；
- WorkerW 出现时，WebView 与原生层由不同代码路径重新挂载，交接期间可能短暂露出错误画面；
- 程序启动前仍由 Windows 绘制系统静态壁纸。当前完整版没有“登录过渡底图”设置，Lite 的现有实现则给所有显示器设同一张图，并且只记原壁纸路径。

真实用户机器的此前诊断显示：系统桌面壁纸路径为华硕 `asus.jpg`，而原生首帧为项目睡眠图；原生首帧在**进程启动后**约 1.6 秒准备好，常先挂在 Progman。这些数字不包含 Windows 登录到进程启动的等待时间。实施时要重新采集最新数据，不要把旧日志当成新测试结果。

## 2. 架构目标

按时间划分三个阶段，各阶段尽可能保持同一图像、逐屏裁切和亮度：

```text
进程未启动              进程已启动，WorkerW 未就绪             WorkerW 已就绪
Windows 静态底图   ->   Progman 下的原生遮盖层和 WebView   ->   同一个 WebView HWND 重新挂载
                         原生层遮住预热中的 WebView             确认可见后释放原生层
```

“预渲染”优先使用**现有单个 `background` WebView**。在 Progman 宿主中让它完成初始化，由原生层遮盖；WorkerW 可用后移动同一个 HWND。不要逐帧截取 WebView 再复制到另一个窗口，也不要增加第二个 WebView、全屏置顶窗或常驻 GPU 复制循环。

如果 Progman 也不存在，普通用户进程此时没有可靠的桌面宿主：等待/重试，并依靠 Windows 静态底图维持画面；不要让临时顶层窗盖住图标或其他应用。

## 3. 阶段 A：建立可比较的启动证据

### 修改范围

- `wallpaper/src-tauri/src/native_bootstrap.rs`
- `wallpaper/src-tauri/src/windows_integration.rs`
- `wallpaper/src-tauri/src/lib.rs`
- `wallpaper/src/App.tsx`、`WakeScene.tsx`、`MultiScreenWakeScene.tsx`（仅埋点）

### 实施

新增一次启动的 `generation`/`startupId` 和单调时间戳，记录以下事件：进程入口、原生层显示、选中的父窗口类名、WebView 创建、首次附着、真实客户区尺寸、页面资源就绪、前端申请交接、WorkerW 出现、两层重新挂载、原生层隐藏及看门狗兜底。只记录类型、尺寸、耗时和失败代码，不记录聊天文本、Cookie、完整私有路径或像素内容。

在原生层和 WebView 同时存在时，记录两者实际 `GetParent`、客户区、是否可见与相对于图标层的 Z 序。补上一个只读诊断入口或日志记录即可，不要为了测试而改变显示层级。

### 阶段完成条件

能从同一份日志重建一次登录后启动的时间线，并区分：

- Windows 静态壁纸 → 原生层的空档；
- 原生层 → WebView 的交接；
- Progman → WorkerW 的重新挂载；
- WebView 资源加载与实际几何就绪。

## 4. 阶段 B：把渲染就绪与可见交接分离

### 建议状态机

给一次 WebView 窗口生命周期定义明确状态：

```text
NativeCoverVisible -> WebviewCreated -> AttachedToProgmanOrWorker
                   -> ScenePrepared -> RevealEligible -> NativeCoverHidden

AttachedToProgmanOrWorker -> Reparenting -> AttachedToWorker -> ScenePrepared
```

每次窗口重建或父窗口/尺寸变更都递增 `generation`。旧 WebView、旧显示器布局或旧帧发出的 `ready` 事件必须被拒绝。解锁苏醒也应产生新的交接代际，不能让上一轮启动的回调释放当前遮盖层。

### 修改建议

1. 在 `windows_integration.rs` 保留现有单 WebView。Progman 可用时先附着到 Progman 并设置最终虚拟桌面的物理尺寸；WorkerW 已可用时直接附着它。WebView 在原生层遮盖下保持可见，使 WebView2 有机会真正合成；不要把 `.visible(false)` 当成“已经画好第一帧”。
2. 增加 `place_cover_above_webview_below_icons()` 之类的原生辅助逻辑。每次首次附着、重挂载和显示器尺寸变化后检查 Z 序。必须在真实 Win11 上证明桌面图标、框选与右键仍可用，普通前台应用不会被盖住。
3. WebView 页面只在目标帧图片加载且 `decode()` 成功、场景处于当前 `generation`、显示器布局已应用后报告 `ScenePrepared`。失败或 1200 ms 超时不是“加载成功”：进入显式降级路径，让原生睡眠帧继续显示或在明确错误后安全释放。
4. `ScenePrepared` 只是**候选**信号。Rust 再核对 WebView HWND 存在、父窗口为预期 Progman/WorkerW、客户区与目标尺寸匹配、原生层仍有效，才允许交接。
5. `requestAnimationFrame` 可以作为页面提交的一环，不能单独作为“画面已呈现”的证明。按原生遮盖层保持、窗口状态校验和可见帧试验制定可行的 reveal 时机。若尝试 `DwmFlush` 或 WebView2 回调，要在文档中说明它们各自保证了什么，不能宣称它们证明屏幕像素绝对一致。
6. 交接成功后隐藏原生层；失败时保留现有遮盖层并走有界恢复路径。不得使用无限重试、每帧 SetWindowPos 或 busy wait。

### 当前代码中的特殊竞态

- `WakeScene.tsx` 和 `MultiScreenWakeScene.tsx` 在安排双 rAF **之前**将 `firstWakeFrameReportedRef` 设为 true。若组件/帧在回调执行前切换并取消 rAF，可能永不再报告首帧。改交接协议时一并解决，且只在同一代际实际完成报告后置位。
- `useWakeFramePreload()` 的 `onerror` 与超时目前都 resolve，不能据此证明图片可显示。要分别返回成功、失败与超时结果。
- 现有 20 秒原生层看门狗是最后兜底。保持其幂等销毁与日志，不允许新状态机绕开或无限延长它。

### 完成条件

无动画、正常苏醒动画、素材加载失败三种路径均有确定终态。按钮和桌面图标照常工作，WebView 出错不会留下永久遮盖层，也不会突然展示一个白色/黑色 WebView。

## 5. 阶段 C：WorkerW 晚到时移动同一实时画面

在现有 `recover_wallpaper_host()` 和 `native_bootstrap::reattach_to_workerw()` 基础上调整交接顺序。进入 `Reparenting` 时暂停遮盖层释放；在 WorkerW 可见后把遮盖层、WebView 都移到目标宿主，核对各自父窗口和客户区，再恢复可见交接。顺序必须在 Win11 实测确定，不能只按 `SetParent` 返回值判断成功；现有实现已经使用 `GetParent` 复核，保持这一点。

必须避免以下结果：

- Progman 旧位置先露出系统原壁纸；
- WorkerW 新位置先露出空白 WebView；
- 原生层在 WebView 下方，不能遮盖预热阶段；
- 遮盖层高于桌面图标，吞掉点击；
- 双屏只把一个 4480×1600 画面拉伸；
- Tauri 重新创建 WebView 后，旧 `ready` 事件隐藏新的原生遮盖层。

如果目标机器长期没有独立 WorkerW，Progman 是合法的最终降级宿主；不要把“必须等到 WorkerW”写成永不完成的状态。

## 6. 阶段 D：进程启动前的 Windows 静态底图（独立工作包）

阶段 B/C 只能修**进程启动后**。若验收目标还包括登录后、程序尚未启动时也看不到原系统壁纸，须另做完整版的**可选**静态底图功能：

1. 读取并私有复制每块显示器当前的静态壁纸，建立原子恢复清单。原始文件被用户移动后仍应可恢复；损坏、动态壁纸、幻灯片或来源不明时拒绝接管。
2. 按显示器 ID 生成与原生睡眠层一致的预制图，以 `IDesktopWallpaper::SetWallpaper(monitorID, path)` 分屏设置。当前 Lite 代码传 `NULL`，会给所有屏幕设置同一张图，不可直接复用到完整版多屏。
3. 只有用户在完整版设置中明确打开选项才修改 Windows 壁纸。关闭选项或卸载前先确认 Windows 仍在使用我们管理的图，再逐屏恢复；用户后来改了壁纸时不覆盖。
4. 图片、显示器布局和设置变化应更新预制图与清单，但不能在每帧或每次 WebView 重绘时调用 Windows 壁纸接口。
5. 此工作包单独提交、单独真机验收。不要为了完成 B/C 偷偷改变系统壁纸或写注册表。

如果本轮只完成 A/B/C，交付时明确说“进程启动前仍显示用户原系统壁纸”，不能称整个冷启动已经无缝。

## 7. 测试与验收

### 自动测试

新增有行为价值的测试：

- 状态机在 `ready`、重建、重挂载、超时、解锁交错时拒绝旧代际释放；
- 图片解码失败/超时不报告成功；
- 多屏负坐标和混合 DPI 下的逐屏区域、`cover` 几何保持正确；
- Progman 长期降级可以完成交接；
- WorkerW 晚到后仍使用同一个 `background` WebView，不新建第二个常驻 renderer；
- 看门狗、退出清理和原生层反复 hide/show/destroy 幂等。

不要写只搜索注释关键字的测试代替窗口行为测试。桌面层级、DWM 首帧必须真机验证。

每个阶段至少运行：

```powershell
pnpm typecheck
pnpm test
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --all-targets
pnpm build
pnpm build:lite
git -c gc.auto=0 -c gc.autoDetach=false diff --check
```

### Windows 真机矩阵

用当前源码构建/运行的版本，在固定设备上录像或逐帧采样：

| 场景 | 应观察的结果 |
|---|---|
| 冷登录：WorkerW 已在 | 无白/黑帧，图标和交互正常；记录每个阶段耗时 |
| 冷登录：WorkerW 晚到，仅 Progman | 画面先在 Progman，后移到 WorkerW；移动瞬间不闪旧壁纸 |
| Explorer 重启 | 背景恢复，不能盖住前台应用或桌面图标 |
| 锁屏→解锁，含快速重复锁/解锁 | 只释放当前代际原生层，不出现旧回调串扰 |
| 两屏：左侧负坐标、不同缩放/比例 | 每屏独立裁切，无遮盖层错位或跨屏拉伸 |
| 缺图、WebView2 创建失败、前端未报告 ready | 有界等待、可恢复提示和安全兜底 |
| 全屏游戏/普通应用在前台 | 壁纸窗口不置顶、不抢焦点、不吞输入 |

用 `startup-diagnostic.log` 和录像逐项对照。对比静态底图、原生层、WebView 首帧时记录物理坐标位移和可见亮度差；不要仅凭“感觉差不多”宣称无缝。视频/截图应遮蔽桌面中的个人数据。

## 8. 下游 Agent 的最终交付格式

请逐阶段报告：修改文件、状态转换、新增/改变的窗口层级、失败兜底、测试结果、真机设备与录像/日志证据、尚未覆盖的场景、`git status --short` 和本地提交 SHA。指出阶段 D 是否完成。若无法取得可靠的 WebView 可见首帧信号或无法保持图标层正常，说明失败证据并保留可用的原生层+Progman方案，不要把实验标记为已交付。
