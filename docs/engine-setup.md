# 壁纸引擎接入指南（M6）

把 `wallpaper/dist/` 构建产物变成**桌面交互壁纸**的方式。

## 前置

```bash
pnpm build   # 产物在 wallpaper/dist/（index.html + assets/）
```

> 产物使用相对路径（`base: './'`），可被任意静态服务器或壁纸引擎直接加载。

## 方式 A：开源网页壁纸引擎（推荐）

以下引擎支持把**网页**作为交互壁纸渲染在桌面背景层（WorkerW），鼠标可直接交互：

| 引擎 | 平台 | 说明 |
|---|---|---|
| [Sucrose](https://github.com/Taiizor/Sucrose) | Windows | 支持网页壁纸 + 交互；开源 |
| [octos](https://github.com/underpig1/octos) | Windows | 创建/分享 Web 交互壁纸；Microsoft Store 可装 |
| [NoisyWinds/Wallpaper](https://github.com/NoisyWinds/Wallpaper) | Windows | HTML5 动态壁纸（可 hover 交互） |

**步骤**（以 octos 为例）：
1. 本地起一个静态服务器指向 `wallpaper/dist/`：
   ```bash
   pnpm preview   # 默认 http://127.0.0.1:4173
   ```
2. 在 octos/Sucrose 中新建"网页壁纸"，URL 填 `http://127.0.0.1:4173/`
3. 应用壁纸 → 桌面背景即鲸鱼娘壁纸，可点击交互

> 长期运行建议：把静态服务器做成自启服务，或用 `python -m http.server` / `npx serve` 等常驻进程。

## 方式 B：Tauri 壳（自研 WorkerW 宿主，P2）

需要"真壁纸层 + 穿透控制 + 托盘 + 系统锁屏联动"时，用 Tauri 2 做壳：

- 单一无边框 `background` WebView 注入 WorkerW，承载画面与桌面内热区；非热区由原生命中测试交还 Explorer
- 通过 tauri 事件把 SessionSwitch（锁屏/解锁）转发给前端状态机
- 提供托盘菜单（切换形态/设置/退出）
- 注册表 Run 键实现开机自启（含可选的自启 DSH 后端）

架构上**只换外壳**：`wallpaper/` 前端不变，Tauri 壳负责窗口/系统事件/托盘。

## 注意事项

- **登录态**：网页实验入口只会在默认浏览器打开 DeepSeek 官方页面；当前没有应用内网页登录态、Cookie 读取或 DOM 消息桥接。DSH 会话通过本地 bridge `http://127.0.0.1:3080` 连接。
- **穿透**：网页壁纸引擎的鼠标穿透策略各异；需要精细控制时走 Tauri 壳
- **3080 探测**：只在 `/api/wallpaper/v1/status` 的版本、鉴权状态与新建会话所需能力集完整匹配时启用 Harness；端口可达本身不会启用。`resume` 仅在 DSH 安装会话持久化服务时出现，不影响新建会话。
