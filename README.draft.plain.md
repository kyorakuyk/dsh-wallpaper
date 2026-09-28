# DSH Wallpaper · 桌面是入口，窗口才是工作台

> Windows 桌面壁纸层 + 鲸鱼娘立绘 + 三个可切换的聊天后端
> Tauri 2 · React · Rust/Win32 ｜ **不修改 DSH 官方 Web UI**

<!-- 配图待补（先留成注释，免得留一条断链）：
     ![待机界面](docs/media/hero.png)     待机全景：桌面 + 立绘
     ![输入岛](docs/media/island.png)     输入岛：状态灯 / 模型选择 / 用量与费用
     ![悬浮球](docs/media/ball.png)       悬浮球与里桌面
     ![设置中心](docs/media/settings.png) 设置中心六个页签
-->

AI 的摩擦往往不在"用起来难"，而在入口不在手边：先打开浏览器或桌面端、找回上次的会话，用完关掉，它又消失了。

这里把入口搬到桌面上——开机就在、不抢焦点、一眼能看到状态、一句话就能发起。需要认真干活时，把它拉到面前：已经开着就唤到前台，没开就启动它，网页型的用浏览器打开。复杂的事仍旧在它自己的窗口里做。

## 它长什么样

| 入口 | 样子 | 什么时候用 |
|---|---|---|
| 待机立绘 | 桌面右侧的鲸鱼娘，随模型分层在幼年/成年之间切换 | 点一下开始对话，或只是看一眼 |
| 输入岛 | 中央玻璃悬浮面板；也可以收成任务栏上的胶囊 | 平时说话的地方（两种布局可选） |
| 悬浮球 | 桌面右下角的小圆钮，内嵌"消息"图标 | 鼠标靠近时滑出，点一下进里桌面 |
| 托盘菜单 | 系统托盘 | 换后端、开设置、换背景、退出 |

- **里桌面**：点悬浮球进入的一层"桌面工作区"，输入岛就位、按 `X` 退出。表桌面上壁纸收不到鼠标消息（画面挂在 Explorer 图标层之下），所以折叠态的入口必须住在独立的小窗口里——悬浮球和任务栏胶囊就是这么来的。
- **表桌面**：桌面本来的样子；壁纸只画画面，非热区输入原样交还 Explorer，框选图标、右键菜单照旧。

## 能力一览

| 模块 | 状态 | 说明 |
|---|---|---|
| 叙事状态机 | ✅ | 睡眠 → 苏醒（四帧动画）→ 待机 → 会话 |
| 形态与立绘 | ✅ | 内置 4 个形态：蓝/黑 × 幼/成年，随模型分层切换；成年态整体放大 1.15，两档站在一起头一样大 |
| 聊天三后端 | ✅ | DeepSeek 网页入口（实验）、DeepSeek API（付费）、DeepSeek Harness |
| 聊天模式开关 | ✅ | 设置中心一键切换，正在运行的壁纸立即生效 |
| 输入岛 | ✅ | 会话窗本体：状态灯、模型选择、用量与费用、历史轨道、收起/展开 |
| 悬浮球与里桌面 | ✅ | 圆形无边框窗口，独立进岛路径；点 `X` 退出里桌面 |
| 任务栏胶囊 | ✅ | 折叠态另一种布局，贴着任务栏展开 |
| 设置中心 | ✅ | 六个页签：常规 / 连接 / 外观 / 形态 / 历史 / 系统，改动即时保存 |
| 历史与费用 | ✅ | 本地 API 会话记录可查看、删除、清空；配置价格后估算本轮与会话费用 |
| 外观素材库 | ✅ | 导入图片 → 指定用途（桌面背景 / 四个立绘槽位），不写代码换装 |
| 多屏 | 🧪 | 逐屏背景与苏醒帧、立绘与会话窗的目标屏幕选择 |
| 锁屏接管 | 🧪 | 安全备份/恢复 + MSIX 打包验证已完成；已安装包的 `Win+L` 人工验收待做 |
| 开机自启 | ✅ | MSIX StartupTask 优先，兼容 `HKCU\...\Run` |
| DSH 会话桥 | 🧪 | `bridge/` 插件：loopback REST/SSE + bearer token |
| DSH Wallpaper Lite | ✅ | 独立前端入口、独立 feature 与 MSIX 身份，只做锁屏接管 + 静态壁纸 |

## 三个聊天后端

| 后端 | 怎么工作 | 什么时候选它 |
|---|---|---|
| DeepSeek 网页入口（实验） | 应用内独立 WebView2 打开 chat.deepseek.com，按 DOM 适配器收发 | 想用网页版账号、不走 API 计费 |
| DeepSeek API（付费） | 官方 API 流式对话；Key 只存在 Windows 凭据管理器 | 要稳定、要费用统计 |
| DeepSeek Harness | 接兼容的 Wallpaper Bridge，与 DSH 同一份会话上下文 | 已经在用 DSH |

- 在「设置中心 → 连接 → 聊天模式」里选 Web 还是 API，**正在运行的壁纸立刻切换**；输入岛上的开关在 Harness 与聊天后端之间快切，切回来时回到你选的那个后端。
- 网页桥接失败**不会**自动切到付费 API。
- Harness 那盏灯：绿色=已连接，黄色呼吸=正在连/断线重连中，熄灭=确认不在了（要连续两次确认才判死）。

## 上手

```powershell
pnpm install
pnpm dev            # 浏览器预览 http://127.0.0.1:5187
pnpm desktop:dev    # 桌面应用：壁纸可独立启动，Harness 模式需要兼容的 Bridge
```

- 快捷键：`Alt+W` 睡眠、`Esc` 睡眠中唤醒 / 关闭设置（可在设置里改休眠键）
- 常用操作：点立绘开对话；鼠标移到桌面右下角唤出悬浮球；托盘菜单打开设置
- 安装包：本机自签 MSIX 用 `pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-local-msix.ps1`（默认跑检查 → 构建 → 递增版本 → 签名 → 安装 → 启动，并保留上一版供回退）；Lite 版加 `-Edition lite`
- 网页版账号登录：设置中心 →「打开应用内页面」，登录态由独立 WebView2 配置目录保存，本应用不读取也不复制 Cookie

## 结构

```
dsh-wallpaper/
├── wallpaper/               # 壁纸前端 + Tauri 壳
│   ├── src/
│   │   ├── scenes/          # 状态机与场景（Sleep / Wake / Idle / Chat）
│   │   ├── features/chat/   # 输入岛与会话窗（含历史轨道）
│   │   ├── floating/        # 悬浮球窗口
│   │   ├── persona/         # 形态注册表与立绘比例适配
│   │   ├── appearance/      # 背景、主题与素材库
│   │   ├── connect/         # 端点扫描、主体识别、Harness 状态
│   │   ├── chat/            # 会话适配器（网页 / API / Harness）
│   │   ├── settings/        # 设置持久化与设置中心
│   │   ├── native/          # Tauri invoke 绑定（runtime.ts）
│   │   ├── runtime/         # 交互热区发布、显示器布局
│   │   └── ui/ widgets/     # 气泡、玻璃、图标等基础件
│   ├── public/personas/     # 立绘与背景资源
│   └── src-tauri/           # Rust 壳：WorkerW 宿主 / 悬浮球 / 设置窗 / 托盘 / 锁屏 / 自启 / 聊天
├── bridge/                  # DSH 会话桥插件
├── assets/personas/         # 立绘源图与历史留档
├── scripts/                 # 素材处理与本地发布（20 个）
└── docs/                    # plans / evidence / design / guides（见 docs/README.md）
```

几条关键实现：

- **单一宿主**：画面与桌面交互热区共用一个 `background` WebView，注入 WorkerW；设置窗是全应用唯一的独立窗口。
- **启动首帧**：原生先用随包睡眠图铺一帧；Explorer 还没给出 WorkerW 时在限定时间内重试并暂挂 Progman，宿主恢复后再把首帧层重新挂回去。
- **无静默替换**：只探测用户在设置里选定的那个主体（客户端或源码目录）的端口；别的客户端更能用也不会被悄悄换上。
- **拉起即完成**：把配置的主体唤到前台，系统拒绝前台切换也算"已到达"（用户点一下就行）；没有可拉起的窗口才开浏览器。
- **凭据边界**：API Key 只写入当前 Windows 用户的凭据管理器，聊天请求在原生层读取；界面能看到的只有脱敏形态。

## 开发与门禁

```powershell
pnpm typecheck      # 前端 + Bridge 类型检查
pnpm test           # 前端单测 + Bridge 测试
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --all-targets
pnpm build          # 前端产物
pnpm build:lite     # Lite 前端产物
cargo check --manifest-path wallpaper/src-tauri/Cargo.toml --locked --no-default-features --features lite --bin dsh-wallpaper-lite
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-lite-bundle.ps1
```

CI（`.github/workflows/ci.yml`）在 `master`、`codex/**` 的推送与面向 `master` 的 PR 上跑上面这一整套；`package.yml` 负责 Lite 首发产物（手动触发或推 `v*` 标签）。

## 当前状态

本机自测：**0.2.0.134**（2026-09-27）。带日期的真机验收记录（含未通过项）在 [`docs/evidence/`](docs/evidence/)；已完成的施工计划在 [`docs/plans/`](docs/plans/)。

## 素材与版权

- 立绘、动画帧、背景均为 AI 生成或用户自备；代码 MIT
- 四张内置立绘的源图在 `assets/personas/`：`蓝幼.png`、`蓝熟.png`、`黑红幼.png`、`黑红熟.png`。`python scripts/remove-bg.py` 会生成运行时的 `wallpaper/public/personas/portrait-*.png`（已有透明通道的图原样保留）
- 单用户换装走「设置中心 → 外观 → 素材库」；改项目默认图则替换源图后重新生成并打包

## 去哪看

| 想看什么 | 去哪 |
|---|---|
| 全部文档的分类规则 | [`docs/README.md`](docs/README.md) |
| 施工文档（需求、顺序、验收条件） | [`docs/plans/`](docs/plans/) |
| 真机取证与结论记录 | [`docs/evidence/`](docs/evidence/) |
| 机制与设计说明 | [`docs/design/`](docs/design/) |
| 操作与专题（Lite 发布、锁屏、多屏…） | [`docs/guides/`](docs/guides/) |
| DSH 会话桥协议与版本矩阵 | [`bridge/README.md`](bridge/README.md) |
| 网页适配器配置 | [`docs/design/deepseek-web-adapter.md`](docs/design/deepseek-web-adapter.md) |
| 网页型客户端打开方式与会话策略 | [`docs/design/harness-subject-and-ui-design.md`](docs/design/harness-subject-and-ui-design.md) |

## 许可

MIT（项目代码）；内置素材为 AI 生成或用户自备，用户自备素材版权归其所有。
