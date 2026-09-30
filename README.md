# dsh-wallpaper · 鲸鱼娘交互壁纸框架

DeepSeek Harness 生态的**交互式桌面壁纸框架**：以鲸鱼娘为拟人形象，提供「睡眠 → 苏醒 → 待机 → 会话」的完整叙事体验，支持形态切换（蓝↔黑、幼↔成年）、深海背景切换、素材定制与 DSH 后端感知。

> 独立 Tauri 桌面应用，不修改 DSH 官方 Web UI；同时保留浏览器预览模式。

## 安装（完整版）

前置：Windows 11（WebView2 由系统自带）。到 [Releases](https://github.com/kyorakuyk/dsh-wallpaper/releases) 下载 `dsh-wallpaper_<版本>_x64-setup.exe` 双击安装 —— **当前用户、不请求管理员、不需要证书**，卸载走「应用和功能」。安装器未签名，第一次运行会有一次 SmartScreen 提示（更多信息 → 仍要运行）。

自己构建：

```powershell
pnpm desktop:build                                  # 产物在 wallpaper/src-tauri/target/release/bundle/nsis/
pwsh -File .\scripts\publish-local-nsis.ps1 -Install # 先跑四道门禁，再构建、报出 SHA-256、静默安装
```

**Lite 版**仍在仓库里（独立前端入口与 Rust feature：`pnpm build:lite` / `pnpm desktop:build:lite`），但它不再是首发包。MSIX 那条路保留给将来的商店上架（需要证书），相关文档见 [`docs/lockscreen-msix-test.md`](docs/lockscreen-msix-test.md) —— 文件名是历史遗留，包本身早已与锁屏无关。

## 功能总览

| 模块 | 状态 | 说明 |
|---|---|---|
| 🐋 **叙事状态机** | ✅ | Sleep → Waking → Idle → Chat 全链路 |
| 🌅 **苏醒帧动画** | ✅ | variant-anima 4 帧序列（睡脸→睁眼→坐起→慵懒打哈欠），1920×1024 |
| 🎨 **形态系统** | ✅ | 蓝/黑 × 幼/成年 四形态，立绘即时切换（useMemo），气泡跟随立绘 |
| 🌊 **深海背景** | ✅ | 3 款深海室内插画 + 默认渐变，设置面板切换，持久化 |
| 💬 **聊天后端** | ✅ | DeepSeek API（流式）+ Harness（Bridge 会话）+ DeepSeek 网页 DOM 桥接（实验能力） |
| 🔌 **DSH 会话桥** | 🧪 | `bridge/` 独立插件：loopback REST/SSE + bearer token 鉴权；会话恢复限定在桌面工作区；真机安装已在两台机器上跑通（v0.3.0 / v0.3.1） |
| 🖥️ **Tauri 壳** | ✅ | 统一 WorkerW 背景宿主（画面与桌面内交互热区）+ 独立设置窗口、托盘、开机自启 |
| 🖥️ **完整版多屏** | 🧪 | 显示器枚举、逐屏背景与苏醒帧、会话窗/立绘目标屏幕选择；Lite 首发仍为单主屏 |
| 🔌 **桥的自动对齐** | ✅ | 启动与选中主体时把桥对齐到钉住的版本；版本相符时**不跑包管理、不动你的档案**；遇到"版本豁免"提示只转达，不代你确认 |
| 🔒 **凭据安全** | ✅ | DeepSeek API Key 只存 Windows 凭据管理器（keyring） |

## 体验流程

```
😴 睡眠    睡眠模式：鲸鱼娘在深海室内酣睡（静态画面，可换深海背景）
   │ 解锁（Esc / 系统解锁）
   ▼
🌅 苏醒    帧动画：睡脸 → 睁眼 → 坐起 → 手遮嘴轻打哈欠（约 7s）
   │ 动画播完
   ▼
☀️ 待机    深海背景 + 右侧立绘 + 气泡「早上好！今天要做什么呢？」
   │ 单击立绘 / 快捷键
   ▼
💬 会话    DeepSeek API、Harness 或 DeepSeek 网页 DOM 桥接可在壁纸中对话；网页模式使用独立持久 WebView2，并按会话策略固定/恢复 DeepSeek 会话
   ▼
🖥️ Harness  兼容的 DSH Wallpaper Bridge 就绪后，立绘切换为黑红主题，并通过其会话接口通信（默认 loopback `http://127.0.0.1:3080`）
```

## 快速开始

```bash
# 安装依赖
pnpm install

# 浏览器预览（http://127.0.0.1:5187）
pnpm dev

# 前端与 Bridge 自动化测试
pnpm test

# 桌面应用
pnpm desktop:dev     # 壁纸可独立启动；Harness 模式需要兼容的 Wallpaper Bridge
pnpm desktop:build   # 构建安装包
```

**安装包**：见上面的「安装（完整版）」；Lite 的构建与打包命令在 [`docs/lite-release.md`](docs/lite-release.md)。

**交互**：
- `Alt+W`：进入睡眠模式
- `Esc`：睡眠中唤醒 / 关闭设置
- 点击右侧立绘：打开会话窗
- 右下角圆点 / 托盘图标：打开设置面板
- 兼容的 Wallpaper Bridge 连续就绪时，待机界面出现「切换到 Harness」询问条；仅 3080 根页面可访问时只显示诊断，不可切换

## DSH 连接与状态诊断

壁纸只通过本机 loopback 上的 Wallpaper Bridge 访问 DSH。探测结果分六态，界面文案与
reason 码由 `wallpaper/src/connect/harnessLabels.ts` 统一定义，只有 `bridge-ready`
允许发送消息，任何状态都不会自动切到付费的 DeepSeek API：

| 状态 | 含义 | 用户可做的事 |
| --- | --- | --- |
| `offline` | 3080 上没有可识别的服务 | 按需启动 DSH |
| `web-only` | 有 HTTP 服务但不是 Wallpaper Bridge | 安装/更新 Bridge 插件 |
| `bridge-loading` | Bridge 已挂载，会话服务仍在装载 | 等待 |
| `bridge-auth-unavailable` | Bridge 已挂载但本机令牌不可用 | 重启壁纸应用；检查令牌目录权限 |
| `bridge-incompatible` | 协议版本、能力集合或宿主形状不匹配 | 更新 DSH 或 Bridge |
| `bridge-ready` | 所需路由全部挂载且令牌可用 | 无 |

`bridge-ready` 的判定依据是 Bridge 实际注册了哪些路由，而不是它声明"支持"什么，
因此它不会在 `POST /sessions` 还会 404 的时候出现。

### 「随壁纸启动 DSH」

设置中心「DeepSeek Harness 启动」卡片提供该开关，默认关闭，每个壁纸进程最多发起一次：

- 它只在**壁纸启动时**触发。**登录后自动生效还需要壁纸自身开机自启**；壁纸自启未开启、
  被用户在任务管理器中禁用或被策略禁用时，卡片会明确说明，并且不会改动你的系统自启设置。
- 已在 3080 运行的外部 DSH 不会被接管、重启或停止，只会转入 Bridge 状态探测。
- 无人值守时不会执行你填写的自定义「启动命令」，除非你在同一张卡片中明确允许；
  手动「启动」按钮始终使用该命令。
- 根目录无效、找不到 Node/pnpm、profile 非法、进程启动失败分别给出不同原因，不做无限重试。
- **修正配置后如何立刻恢复**：自动启动每个壁纸进程只尝试一次（失败也记入，避免每秒重启），
  所以改好根目录后**当次不会自动重试**。请直接用卡片上的「**启动**」按钮 —— 它走的是另一条
  路径（`launch_dsh`），不受那条一次性记录约束，命令与校验完全相同。下一次壁纸启动时自动
  路径才会重试。

> **自启项归属提醒**：自启优先使用包的 StartupTask，未打包的开发版才回退到
> `HKCU\...\Run`。回退路径写入的是**当前进程自己的 exe 路径**，所以在开发版里切换
> 「开机自启」会把 Run 项从已安装包改指向 `target\debug\dsh-wallpaper.exe`。
> 验证「登录后自动启动 DSH」时请使用**签名测试包**，不要用开发版改自启项 ——
> 否则登录时启动的是开发版，验收结论不成立。安装包升级时 `migrate_legacy_autostart`
> 会把 Run 项迁移为 StartupTask；若 Windows 拒绝，旧项会被保留，因此升级不会造成自启空档。

### 更新已安装的 Bridge

DSH profile 里的 Bridge 是**拷贝**而非指向本仓库的链接（desktop profile 使用 pnpm 的
hoisted 模式），因此改完 `bridge/src` 后必须刷新 profile，运行中的 DSH 才会加载新代码：

```powershell
dsh plugin --profile desktop install   # 换成你实际使用的 profile
```

刷新后请从 `/api/wallpaper/v1/status` 的 `bridgeVersion` / `bridgeBuild` 确认已生效，
而不要只看本地构建输出。壁纸会在**启动时与选中主体时**各对齐一次桥：只跑官方那条 `dsh plugin … add` 命令，版本相符就什么都不做；它不会下载代码、不会替你确认版本豁免，也不会改写你档案以外的任何东西。
版本矩阵、协议版本与包版本的区别、宿主适配层边界见
[`bridge/README.md`](bridge/README.md)。

### 当前真机验收状态

2026-09-25 实测。**已通过的项目也有实测记录，未通过的如实标注。**

| 环节 | 状态 |
| --- | --- |
| 壁纸自启项 | ✅ `HKCU\...\Run` 的 `dsh-wallpaper` 存在，指向已安装 `0.2.0.71` |
| **profile 内 Bridge 已刷新** | ✅ 已按文档更新:`remove` + `add` 后 profile 内 `lib/index.js` 摘要 **等于源码构建**(普通 `install` 不生效,因为 lockfile 对 `type: directory` 依赖无完整性校验,pnpm 认为已满足)。profile 的依赖集合与版本未变,只有 peer 范围随 Bridge 声明从 rc.6 落到 rc.5;改动前已备份 `package.json`/`pnpm-lock.yaml` |
| **真实链路(创建→消息→回复→取消)** | ✅ 用真实 profile 启动的 DSH 上实测:未鉴权 401;`POST /sessions` 201;`POST /messages` 202;约 5 秒后 `GET /history` 返回 2 条 —— **`[user] say PONG` / `[assistant] PONG`**;`POST /cancel` 202。这需要你 profile 里的真实凭据,是本轮唯一能证明"模型真的回了"的证据 |
| **`/status` 新契约** | ✅ 真实 profile 的实时响应含 `state: bridge-ready`、`reasonCode: ready`、`bridgeBuild`、`authoredAgainst: ^0.1.0-rc.5`,能力集含 `control`/`sessions`/`resume` 等 |
| 设置迁移已在真实环境生效 | ✅ 你机器上的设置文档已是 `version: 10`(`defaultBackend`/`autostart`/`sleepHotkey` 等既有配置保留),说明存储迁移在真实使用中跑通 |
| 已安装包是否含本功能 | ❌ `0.2.0.71`(构建于 2026-09-17)**不含**「随壁纸启动 DSH」,也不含本轮 Bridge 就绪/适配层改动;二进制字面量搜索确认相关标识全不存在 |
| 开发版是否含本功能 | ✅ `target\debug\dsh-wallpaper.exe` 含 `autostart_managed_dsh`、`managed_dsh_autostart_status`,前端产物含设置项与卡片文案 |
| **单飞(重启两次各一个受管进程)** | ⏳ **未实测**。「每个进程最多一次」有原生记录 + 渲染层模块标记 + 回归测试,但"重启壁纸两次数进程数"需要从设置中心打开开关后重启应用,本轮未执行 |
| **登录后自动启动** | ❌ **尚未成立**,必须先出含本功能的签名测试包并安装,再按「安装包版本 / 实际 exe 路径 / Windows 启动来源 / 壁纸与 DSH 启动时间 / Bridge ready 时间 / 关机重登后进程数」逐项记录 |

## GitHub Actions

- `.github/workflows/ci.yml`：在 `master`、`codex/**` 的推送和面向 `master` 的 PR 上运行 Windows x64 类型检查、前端/Bridge 测试、Rust 全目标测试与前端构建。
- `.github/workflows/package.yml`：支持手动运行或推送 `v*` 标签时构建 Lite 首发 Windows 产物（NSIS 安装器、临时测试证书签名的锁屏 MSIX、公开 `.cer` 与自动导入证书的测试引导安装器）；完整版仍可单独运行本地工程命令检查，不会混入首发 Release。所有 Actions 工件保留 14 天。
- Lite CI 会在临时 Windows runner 上生成一次性测试证书并签名 MSIX，只上传公开 `.cer`，不上传私钥 `.pfx`，也不会自动安装或修改锁屏。正式 MSIX 签名仍应配置受信任发行证书和独立正式清单，证书不得提交到仓库。

## 形态系统（persona）

当前内置形态由 `wallpaper/src/persona/registry.ts` 注册表定义，素材保存在 `assets/personas/` 与 `wallpaper/public/personas/`；用户目录扫描和 `manifest.json` 驱动的自定义形态仍是后续能力。内置 4 个形态（均有专属透明立绘）：

| id | 名称 | 后端 | 年龄段 |
|---|---|---|---|
| `blue-child` | 蓝色幼年鲸鱼娘 | DeepSeek 蓝色主题 | 幼 |
| `black-adult` | 黑红成年鲸鱼娘 | DSH Wallpaper Bridge | 成年 |
| `blue-adult` | 蓝色成年鲸鱼娘 | DeepSeek 蓝色主题 | 成年 |
| `black-child` | 黑红幼年鲸鱼娘 | DSH Wallpaper Bridge | 幼 |

### manifest.json 草案（尚未由运行时扫描）

```jsonc
{
  "id": "my-persona",
  "name": "我的鲸鱼娘",
  "theme": {
    "primary": "#e03050",        // 主题主色（气泡/边框/氛围）
    "accent": "#ff6b81",         // 辅色
    "glow": "rgba(224,48,80,0.2)" // 氛围光
  },
  "age": "adult",                // child | adult
  "kind": "black",               // blue=网页后端 | black=DSH 后端
  "bubbles": {
    "morning": "早上好！今天要做什么呢？",
    "done": "搞定啦～",
    "harnessOnline": "检测到 Harness，切换形态？",
    "harnessOffline": "Harness 已下线。",
    "chatOpen": "想聊点什么呀？"
  },
  "assets": {
    "portrait": "portrait.png",      // 立绘（透明底 PNG，右侧站立）
    "illustration": "bg.jpg",        // 背景插画（可选）
    "sleep": "sleep.png"             // 睡眠静态图
  }
  // 帧动画（可选）：
  // "animations": { "wake": { "frames": ["w1.png","w2.png"], "fps": 12 } }
}
```

**替换立绘**：往 `assets/personas/<id>/` 放入图片并写 manifest 即可——无需改代码。

## 深海背景

`wallpaper/src/personas/deepsea-bg/` 提供 3 款可切换背景，设置面板选择后持久化：

| id | 名称 | 设计 |
|---|---|---|
| `default` | 主题渐变 | 跟随形态主题色 |
| `deepsea-1` | 深海工作室 | 海底小屋 + 鱼影投墙 + 窗外鱼群珊瑚 |
| `deepsea-2` | 深海穹顶舱 | 玻璃穹顶 + 水母 + 暖灯沙发 |
| `deepsea-3` | 深海书房 | 落地玻璃窗 + 海底阳光柱 + 木质书架 |

## 架构

```
dsh-wallpaper/
├── wallpaper/                  # 壁纸前端 + Tauri 壳
│   ├── src/
│   │   ├── scenes/             # 状态机 + 场景（Sleep/Wake/Idle/Chat）
│   │   ├── persona/            # 当前内置形态注册表（用户扫描待实现）
│   │   ├── chat/               # 聊天适配器（mock/native/deepseekWeb）
│   │   ├── connect/            # 3080 探测 + harness 状态
│   │   ├── domain/             # 类型 + 模型分层
│   │   ├── native/             # Tauri invoke 桥（runtime.ts）
│   │   ├── settings/           # 设置持久化与面板
│   │   └── ui/                 # 气泡 + 程序化鲸鱼娘
│   └── src-tauri/              # Rust 壳（WorkerW 宿主/设置窗口/托盘/锁屏/自启/聊天）
├── bridge/                     # DSH 会话桥插件（loopback REST/SSE）
├── assets/personas/            # 全部素材（立绘/动画帧/背景/留档）
├── scripts/                    # 素材生成与处理（17 个脚本）
└── docs/                       # 壁纸引擎接入指南
```

### 关键机制

- **状态机**：`scenes/stateMachine.ts` 纯函数转移表，可测试
- **Harness 探测**：只将版本与能力兼容的 Wallpaper Bridge 判为可用；3080 根页面可访问但缺少 Bridge 时仅显示诊断状态
- **形态联动**：`autoSwitchHarness` 开启时，兼容 Bridge 连续就绪后才切换黑红形态；失去兼容 Bridge 后可手动切回（可关闭）
- **Tauri 壳**：单一 `background` WebView 注入 WorkerW（画面和桌面内交互热区共用宿主）；`settings` 是唯一独立应用窗口。非热区输入通过原生命中测试交还 Explorer；另有 WTS 锁定/解锁、托盘菜单、MSIX StartupTask 优先且兼容 Run 键的自启和 keyring 凭据。
- **启动首帧**：原生首帧先使用随包睡眠图；Explorer 尚未提供 WorkerW 时在限定时间内重试并暂挂 Progman，后台宿主恢复时同步把首帧层重新挂到 WorkerW。原生层按当前每块显示器的物理区域独立裁切绘制，前端苏醒帧也按显示器区域分别播放。启动阶段只记录父窗口类型、尺寸、显示器数量、阶段和耗时；诊断文件位于进程的 `%LOCALAPPDATA%\DSHWallpaper\startup-diagnostic.log`。MSIX 运行时 Windows 通常会把 `%LOCALAPPDATA%` 重定向到包容器，实际路径形如 `%LOCALAPPDATA%\Packages\<package-family>\LocalCache\Local\DSHWallpaper\startup-diagnostic.log`，可用 `rg --files $env:LOCALAPPDATA -g startup-diagnostic.log` 定位。
- **DeepSeek 网页会话**：应用内 WebView2 使用独立持久数据目录保存网页登录态，但会话指针单独由壁纸保存。默认“恢复最近会话”会导航到保存的 `https://chat.deepseek.com/a/chat/s/<conversation-id>`，历史读取和发送前都会确认页面已处于该会话；只有策略判定需要新建时，才会回到根路由创建新会话。应用不读取或复制 Cookie，也不会因 WebView 重载自动创建新的重复会话。
- **DeepSeek 网页适配配置**：选择器、角色标签和会话路由模板来自 `wallpaper/src-tauri/config/deepseek-web-adapter.json`。设置中心可打开应用配置目录中的 `deepseek-web-adapter.override.json`；保存后下一次网页操作读取新配置，配置无效时自动回退内置版本。该配置只允许声明式选择器和标签，不允许 JavaScript、Cookie、凭据或任意域名；配置维护说明见 [`docs/deepseek-web-adapter.md`](docs/deepseek-web-adapter.md)。
- **bridge 协议**：`/api/wallpaper/v1` 版本化 REST/SSE，status 公开、会话路由 bearer token 鉴权（token 存 `$DSH_HOME/wallpaper/bridge-token`，仅原生壳读取）

## 测试

```bash
# 前端（状态机、探测、surface、model tier、runtime、会话策略等）
pnpm -C wallpaper exec vitest run

# Bridge（协议、路由、token 边界）
pnpm -C bridge test

# 原生层（权限边界、锁屏、Harness/API 状态）
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml
```

## 本机一键发布

在 PowerShell 7 中运行：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-local-msix.ps1
```

脚本默认运行类型检查、前端/Bridge 测试和 Rust 测试，然后构建 Release MSIX、自动递增清单修订号、签名、验证、安装并启动；版本号只写入本次构建用的临时清单，不修改项目清单。Lite 发布还会运行 Lite 原生检查/测试和与 CI 共用的 Lite 产物边界检查。签名从当前用户证书库按指纹使用私钥，不读取或传递 PFX 密码。MSIX 只安装给当前用户，不需要管理员权限；只有本机尚未信任匹配的公开 CER 时，才会为导入 `LocalMachine\TrustedPeople` 单独请求 UAC。拒绝 UAC 会在安装前停止。升级默认关闭正在运行的壁纸，`-NoForceApplicationShutdown` 可要求脚本发现旧进程时停止并提示手动退出。脚本会保留上一版签名包供人工回退；若新版本已安装但启动验证失败，不会自动降级。`-PlanOnly` 只显示计划，`-SkipChecks` 跳过类型检查和测试（Lite 产物边界、签名与安装完整性校验仍执行），`-NoLaunch` 安装后不启动。

首次使用前需要匹配 MSIX `Publisher` 的公开 CER，以及 `Cert:\CurrentUser\My` 中对应的带私钥签名证书；脚本不会创建或导出 PFX。构建 Lite 包可传入 `-Edition lite`，安装前可用 `-PlanOnly` 查看版本递增、签名证书、UAC 需求和回滚包状态。

## 素材与版权

- **代码以 MIT 授权**；立绘、动画帧与背景**不与代码同许可**（CC BY-NC-SA 4.0，见 [LICENSE-ASSETS.md](LICENSE-ASSETS.md)）
- `assets/personas/abolished/` 保留全部历史迭代版本（留档）
- 四张默认立绘的源图位于 `assets/personas/`：`蓝幼.png`、`蓝熟.png`、`黑红幼.png`、`黑红熟.png`。运行 `python scripts/remove-bg.py` 会将它们分别准备到 `wallpaper/public/personas/portrait-blue-child.png`、`portrait-blue-adult.png`、`portrait-black-child.png`、`portrait-black-adult.png`；已有透明通道会原样保留，白底图才会抠图。
- 单用户换装可在「设置中心 → 外观 → 素材库」导入图片并分配到 DeepSeek Flash/Pro 或 Harness Flash/Pro 槽位；要改项目默认图则替换上述源图并重新生成运行时资源，再构建安装包。

## 路线图

- [x] M1-M5：骨架 / 状态机 / 待机气泡 / API 与 Harness 后端 / 设置面板
- [x] M6：Tauri 壳（统一 WorkerW 宿主、设置窗口、托盘、自启）
- [🧪] DeepSeek 网页 DOM 消息桥接首版（应用内持久 WebView2；仅 DOM 交互，不读取或复制 Cookie；页面改版时安全降级；回复解析、请求终态和旧事件隔离已有自动化覆盖；Win11 真实通讯验收待做）
- [ ] 锁屏接管真机验收（已安装 MSIX + `Win+L`；现有实现不会在未封装开发版中接管系统锁屏）
- [x] 苏醒帧动画序列（variant-anima，人设修正：有腿+尾巴装饰）
- [x] 深海室内背景切换
- [x] DSH 会话桥（bridge/）+ 聊天双通道
- [🧪] 完整版多屏分层（逐屏背景/苏醒帧与交互目标屏幕；Windows 11 多显示器真机验收待做）
- [ ] 真机安装联调（bridge 挂载 + 会话打通）
- [ ] 素材导入 UI + 用户形态扫描

## 许可

**代码**：MIT（见 [LICENSE](LICENSE)）。

**美术素材**（立绘、动画帧、背景）：CC BY-NC-SA 4.0 —— 原型来自 ZipZipPipe 与上善无形，本项目的立绘是其衍生，因此沿用该许可：**署名 + 非商业 + 相同方式共享**；用户自备素材的版权仍归其作者。详见 [LICENSE-ASSETS.md](LICENSE-ASSETS.md)。

两者分开授权：**MIT 只覆盖代码**——再用到美术素材时，按 CC BY-NC-SA 4.0 的署名与非商业要求来。
