# dsh-wallpaper · 鲸鱼娘交互壁纸框架

DeepSeek Harness 生态的**交互式桌面壁纸框架**：以鲸鱼娘为拟人形象，提供「睡眠 → 苏醒 → 待机 → 会话」的完整叙事体验，支持形态切换（蓝↔黑、幼↔成年）、深海背景切换、素材定制与 DSH 后端感知。

> 独立 Tauri 桌面应用，不修改 DSH 官方 Web UI；同时保留浏览器预览模式。

## 首发产品：DSH Wallpaper Lite

Lite 与完整版共用本仓库和 Windows 原生壁纸核心，但采用独立的前端入口、Rust feature、Tauri 配置和 MSIX 身份。首发包只提供 Windows 锁屏图片接管、正式四帧苏醒动画、静态壁纸、立绘、开机自启、可选登录过渡底图以及 TranslucentTB 兼容入口；不包含聊天、DeepSeek/DSH 连接、会话、表里桌面或插件。

```powershell
pnpm build:lite
pnpm desktop:build:lite
```

锁屏接管的正式验收和 MSIX 安装请在另一台 Windows 11 测试机执行；完整范围与发布命令见 [`docs/lite-release.md`](<C:/DeepSeekHarness/plugins/dsh-wallpaper/docs/lite-release.md>)。

## 功能总览

| 模块 | 状态 | 说明 |
|---|---|---|
| 🐋 **叙事状态机** | ✅ | Sleep → Waking → Idle → Chat 全链路 |
| 🌅 **苏醒帧动画** | ✅ | variant-anima 4 帧序列（睡脸→睁眼→坐起→慵懒打哈欠），1920×1024 |
| 🎨 **形态系统** | ✅ | 蓝/黑 × 幼/成年 四形态，立绘即时切换（useMemo），气泡跟随立绘 |
| 🌊 **深海背景** | ✅ | 3 款深海室内插画 + 默认渐变，设置面板切换，持久化 |
| 💬 **聊天后端** | ✅ | DeepSeek API（流式）+ Harness（bridge 会话）；DeepSeek 网页入口仍为实验能力 |
| 🔌 **DSH 会话桥** | ✅ | `bridge/` 独立插件：loopback REST/SSE + bearer token 鉴权 |
| 🖥️ **Tauri 壳** | ✅ | 统一 WorkerW 背景宿主（画面与桌面内交互热区）+ 独立设置窗口、托盘、开机自启 |
| 🔐 **锁屏接管** | 🧪 | 安全备份/恢复与无副作用 MSIX 打包验证已完成；已安装 MSIX 的 `Win+L` 人工验收待做 |
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
💬 会话    DeepSeek API 或 Harness 可在壁纸中对话；网页模式仅打开官方网页入口，不提供消息桥接
   ▼
🖥️ Harness  兼容的 DSH Wallpaper Bridge 就绪后，立绘切换为黑红主题，并通过其会话接口通信（默认 loopback `http://127.0.0.1:3080`）
```

## 快速开始

```bash
# 安装依赖
pnpm install

# 浏览器预览（http://127.0.0.1:5177）
pnpm dev

# 前端与 Bridge 自动化测试
pnpm test

# 桌面应用
pnpm desktop:dev     # 壁纸可独立启动；Harness 模式需要兼容的 Wallpaper Bridge
pnpm desktop:build   # 构建安装包
```

**Lite 首发安装包**：由 GitHub Actions 远程生成 `dsh-wallpaper-lite_0.1.1_x64-setup.exe`；开发机不执行打包或安装，下载入口见 [`docs/lite-release.md`](<C:/DeepSeekHarness/plugins/dsh-wallpaper/docs/lite-release.md>)。

**交互**：
- `Alt+W`：进入睡眠模式
- `Esc`：睡眠中唤醒 / 关闭设置
- 点击右侧立绘：打开会话窗
- 右下角圆点 / 托盘图标：打开设置面板
- 兼容的 Wallpaper Bridge 连续就绪时，待机界面出现「切换到 Harness」询问条；仅 3080 根页面可访问时只显示诊断，不可切换

## GitHub Actions

- `.github/workflows/ci.yml`：在 `master`、`codex/**` 的推送和面向 `master` 的 PR 上运行 Windows x64 类型检查、前端/Bridge 测试、Rust 全目标测试与前端构建。
- `.github/workflows/package.yml`：支持手动运行或推送 `v*` 标签时构建 Lite 首发 Windows 产物（NSIS 安装器、临时测试证书签名的锁屏 MSIX、公开 `.cer` 与自动导入证书的测试引导安装器）；完整版仍可单独运行本地工程命令检查，不会混入首发 Release。所有 Actions 工件保留 14 天。
- Lite CI 会在临时 Windows runner 上生成一次性测试证书并签名 MSIX，只上传公开 `.cer`，不上传私钥 `.pfx`，也不会自动安装或修改锁屏。正式 MSIX 签名仍应配置受信任发行证书和独立正式清单，证书不得提交到仓库。

## 形态系统（persona）

形态由 `assets/personas/<id>/` 目录 + `manifest.json` 定义。内置 4 个形态（均有专属透明立绘）：

| id | 名称 | 后端 | 年龄段 |
|---|---|---|---|
| `blue-child` | 蓝色幼年鲸鱼娘 | DeepSeek 蓝色主题 | 幼 |
| `black-adult` | 黑红成年鲸鱼娘 | DSH Wallpaper Bridge | 成年 |
| `blue-adult` | 蓝色成年鲸鱼娘 | DeepSeek 蓝色主题 | 成年 |
| `black-child` | 黑红幼年鲸鱼娘 | DSH Wallpaper Bridge | 幼 |

### manifest.json 规范（用户定制）

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
│   │   ├── persona/            # 形态注册表 + manifest
│   │   ├── chat/               # 聊天适配器（mock/native/deepseekWeb）
│   │   ├── connect/            # 3080 探测 + harness 状态
│   │   ├── domain/             # 类型 + 模型分层
│   │   ├── native/             # Tauri invoke 桥（runtime.ts）
│   │   ├── settings/           # 设置持久化与面板
│   │   └── ui/                 # 气泡 + 程序化鲸鱼娘
│   └── src-tauri/              # Rust 壳（WorkerW 宿主/设置窗口/托盘/锁屏/自启/聊天）
├── bridge/                     # DSH 会话桥插件（loopback REST/SSE）
├── assets/personas/            # 全部素材（立绘/动画帧/背景/留档）
├── scripts/                    # 素材生成与处理（11 个脚本）
└── docs/                       # 壁纸引擎接入指南
```

### 关键机制

- **状态机**：`scenes/stateMachine.ts` 纯函数转移表，可测试
- **Harness 探测**：只将版本与能力兼容的 Wallpaper Bridge 判为可用；3080 根页面可访问但缺少 Bridge 时仅显示诊断状态
- **形态联动**：`autoSwitchHarness` 开启时，兼容 Bridge 连续就绪后才切换黑红形态；失去兼容 Bridge 后可手动切回（可关闭）
- **Tauri 壳**：单一 `background` WebView 注入 WorkerW（画面和桌面内交互热区共用宿主）；`settings` 是唯一独立应用窗口。非热区输入通过原生命中测试交还 Explorer；另有 WTS 锁定/解锁、托盘菜单、reg 自启和 keyring 凭据。
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

## 素材与版权

- 立绘/动画帧/背景均为 **AI 生成或用户自备**，代码 MIT 许可
- `assets/personas/abolished/` 保留全部历史迭代版本（留档）
- 素材替换：`assets/personas/<id>/` 放图 + 写 manifest，或跑 `scripts/remove-bg.py` 抠白底

## 路线图

- [x] M1-M5：骨架 / 状态机 / 待机气泡 / API 与 Harness 后端 / 设置面板
- [x] M6：Tauri 壳（统一 WorkerW 宿主、设置窗口、托盘、自启）
- [🧪] DeepSeek 网页 DOM 消息桥接首版（应用内持久 WebView2；仅 DOM 交互，不读取或复制 Cookie；页面改版时安全降级；Win11 真机通讯验收待做）
- [ ] 锁屏接管真机验收（已安装 MSIX + `Win+L`；现有实现不会在未封装开发版中接管系统锁屏）
- [x] 苏醒帧动画序列（variant-anima，人设修正：有腿+尾巴装饰）
- [x] 深海室内背景切换
- [x] DSH 会话桥（bridge/）+ 聊天双通道
- [ ] 真机安装联调（bridge 挂载 + 会话打通）
- [ ] 素材导入 UI + 用户形态扫描

## 许可

MIT（项目代码）；内置素材为 AI 生成或用户自备，用户自备素材版权归其所有。
