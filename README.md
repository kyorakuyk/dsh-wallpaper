# dsh-wallpaper · 鲸鱼娘交互壁纸框架

DeepSeek Harness 生态的**交互式桌面壁纸框架**：以鲸鱼娘为拟人形象，提供「睡眠 → 苏醒 → 待机 → 会话」的完整叙事体验，支持形态切换（蓝↔黑、幼↔成年）、素材定制与 DSH 后端感知。

> 独立 Tauri 桌面应用，不修改 DSH 官方 Web UI；同时保留浏览器预览模式。

## 当前实现状态

- Windows 11 WorkerW 背景窗 + 透明交互窗；WTS 锁定、解锁、休眠恢复事件已接入。
- 当前用户锁屏图设置、静态原图备份/恢复、开机自启、托盘菜单已接入。
- DeepSeek API 由 Rust 层流式请求，API Key 只写入 Windows 凭据管理器。
- `bridge/` 是可独立安装的 DSH 插件，提供 `/api/wallpaper/v1` REST/SSE 协议；bearer token 只由原生壳读取。
- DeepSeek 免费网页模式目前以“实验性、未验证即停用”方式交付：使用持久化官方 WebView2 登录窗口，但在 DOM 特征 fixture 完成前不会自动操作网页，也绝不会自动切到付费 API。

Debug 安装包：`wallpaper/src-tauri/target/debug/bundle/nsis/dsh-wallpaper_0.2.0_x64-setup.exe`。

## 体验流程

```
😴 睡眠    锁屏/睡眠模式：萝莉鲸鱼娘在床上呼呼大睡（静态画面）
   │ 解锁（Esc / 系统解锁）
   ▼
🌅 苏醒    动画：睁眼 → 伸懒腰 → 坐起 → 起床（约 5s）
   │ 动画播完
   ▼
☀️ 待机    背景插画 + 右侧立绘 + 气泡「早上好！今天要做什么呢？」
   │ 单击立绘 / 快捷键
   ▼
💬 会话    默认 DeepSeek 网页版（新窗口）
   │ 检测到 3080（DSH web）上线 → 气泡询问
   ▼
🖥️ Harness  立绘切换为黑红主题 + 会话窗指向 http://127.0.0.1:3080
```

## 快速开始

```bash
# 安装依赖
pnpm install

# 开发预览（浏览器打开 http://127.0.0.1:5177）
pnpm dev

# 生产构建（产物在 wallpaper/dist/，可交给壁纸引擎）
pnpm build

# 测试
pnpm test

# Windows 桌面开发 / 安装包
pnpm desktop:dev
pnpm desktop:build
```

**交互**：
- `Alt+W`：进入睡眠模式
- `Esc`：睡眠中唤醒 / 关闭设置
- 点击右侧立绘：打开会话窗
- 右下角圆点：打开设置面板
- 3080 在线时待机界面出现「切换到 Harness」询问条

## 形态系统（persona）

形态由 `assets/personas/<id>/` 目录 + `manifest.json` 定义。内置 4 个程序化占位形态：

| id | 名称 | 后端 | 年龄段 |
|---|---|---|---|
| `blue-child` | 蓝色萝莉鲸鱼娘 | deepseek.com 网页 | 幼 |
| `black-adult` | 黑红成年鲸鱼娘 | DSH (3080) | 成年 |
| `blue-adult` | 蓝色成年鲸鱼娘 | deepseek.com 网页 | 成年 |
| `black-child` | 黑红萝莉鲸鱼娘 | DSH (3080) | 幼 |

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
    "sleep": "sleep.png"             // 睡眠静态图（可选，缺省用程序占位）
  }
  // 帧动画（可选）：
  // "animations": { "wake": { "frames": ["w1.png","w2.png"], "fps": 12 } }
}
```

**替换立绘**：往 `assets/personas/<id>/` 放入你的图片并写 manifest 即可——无需改代码。

## 架构

```
wallpaper/src/
├── scenes/       状态机 + 场景组件（Sleep / Wake / Idle / Chat）
├── persona/      形态注册表 + manifest 类型
├── connect/      3080 探测（DSH web 在线检测，去抖）
├── settings/     设置持久化（localStorage）与面板
└── ui/           气泡组件 + 程序化鲸鱼娘绘制（Canvas）
```

- **状态机**：`scenes/stateMachine.ts`，纯函数转移表，可测试
- **3080 探测**：`connect/probe.ts`，HTTP 轮询 + 连续 settle 次去抖
- **形态联动**：`autoSwitchPersona` 开启时，3080 上线→黑红形态，下线→蓝色形态（可关闭手动控制）

## 路线图

- [x] M1-M5：骨架 / 状态机 / 待机气泡 / 双后端 / 设置面板
- [ ] M6：壁纸引擎接入（见 `docs/engine-setup.md`）
- [ ] 系统锁屏联动（SessionSwitch）：需桌面壳（Tauri）
- [ ] 开机自启 DSH 后端（写注册表）：需桌面壳
- [ ] 素材导入 UI + 用户形态扫描

## 许可

MIT（本项目代码）；内置占位素材为程序生成，无版权问题。用户自备素材版权归其所有。
