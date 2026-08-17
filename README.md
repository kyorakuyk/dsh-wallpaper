# dsh-wallpaper · 鲸鱼娘交互壁纸框架

DeepSeek Harness 生态的**交互式桌面壁纸框架**：以鲸鱼娘为拟人形象，提供「睡眠 → 苏醒 → 待机 → 会话」的完整叙事体验，支持形态切换（蓝↔黑、幼↔成年）、深海背景切换、素材定制与 DSH 后端感知。

> 独立 Tauri 桌面应用，不修改 DSH 官方 Web UI；同时保留浏览器预览模式。

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
🖥️ Harness  立绘切换为黑红主题 + 会话走 DSH bridge（http://127.0.0.1:3080）
```

## 快速开始

```bash
# 安装依赖
pnpm install

# 浏览器预览（http://127.0.0.1:5177）
pnpm dev

# 测试（前端 14 + bridge 3，共 17 项）
pnpm test

# 桌面应用
pnpm desktop:dev     # 开发（需要 dsh web 在 3080 运行）
pnpm desktop:build   # 构建安装包
```

**Release 安装包**：`wallpaper/src-tauri/target/release/bundle/nsis/dsh-wallpaper_0.2.0_x64-setup.exe`（32.5 MB）

**交互**：
- `Alt+W`：进入睡眠模式
- `Esc`：睡眠中唤醒 / 关闭设置
- 点击右侧立绘：打开会话窗
- 右下角圆点 / 托盘图标：打开设置面板
- 3080 在线时待机界面出现「切换到 Harness」询问条

## 形态系统（persona）

形态由 `assets/personas/<id>/` 目录 + `manifest.json` 定义。内置 4 个形态（均有专属透明立绘）：

| id | 名称 | 后端 | 年龄段 |
|---|---|---|---|
| `blue-child` | 蓝色幼年鲸鱼娘 | DeepSeek 蓝色主题 | 幼 |
| `black-adult` | 黑红成年鲸鱼娘 | DSH (3080) | 成年 |
| `blue-adult` | 蓝色成年鲸鱼娘 | DeepSeek 蓝色主题 | 成年 |
| `black-child` | 黑红幼年鲸鱼娘 | DSH (3080) | 幼 |

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
- **3080 探测**：`connect/probe.ts` HTTP 轮询 + settle 去抖
- **形态联动**：`autoSwitchPersona` 开启时 3080 上线→黑红、下线→蓝色（可关闭）
- **Tauri 壳**：单一 `background` WebView 注入 WorkerW（画面和桌面内交互热区共用宿主）；`settings` 是唯一独立应用窗口。非热区输入通过原生命中测试交还 Explorer；另有 WTS 锁定/解锁、托盘菜单、reg 自启和 keyring 凭据。
- **bridge 协议**：`/api/wallpaper/v1` 版本化 REST/SSE，status 公开、会话路由 bearer token 鉴权（token 存 `$DSH_HOME/wallpaper/bridge-token`，仅原生壳读取）

## 测试

```bash
# 前端（状态机/探测/surface/modelTier/runtimeState/conversationPolicy）
pnpm -C wallpaper exec vitest run   # 14 项

# bridge（协议编解码）
pnpm -C bridge exec vitest run      # 3 项
```

## 素材与版权

- 立绘/动画帧/背景均为 **AI 生成或用户自备**，代码 MIT 许可
- `assets/personas/abolished/` 保留全部历史迭代版本（留档）
- 素材替换：`assets/personas/<id>/` 放图 + 写 manifest，或跑 `scripts/remove-bg.py` 抠白底

## 路线图

- [x] M1-M5：骨架 / 状态机 / 待机气泡 / API 与 Harness 后端 / 设置面板
- [x] M6：Tauri 壳（统一 WorkerW 宿主、设置窗口、托盘、自启）
- [ ] DeepSeek 网页 DOM 消息桥接（当前实验入口只会用默认浏览器打开官方页面；本应用不读取 Cookie，不能使用或保存该页面的登录状态）
- [ ] 锁屏接管真机验收（已安装 MSIX + `Win+L`；现有实现不会在未封装开发版中接管系统锁屏）
- [x] 苏醒帧动画序列（variant-anima，人设修正：有腿+尾巴装饰）
- [x] 深海室内背景切换
- [x] DSH 会话桥（bridge/）+ 聊天双通道
- [ ] 真机安装联调（bridge 挂载 + 会话打通）
- [ ] 素材导入 UI + 用户形态扫描

## 许可

MIT（项目代码）；内置素材为 AI 生成或用户自备，用户自备素材版权归其所有。
