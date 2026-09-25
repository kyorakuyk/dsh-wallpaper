# 壁纸引擎接入指南（M6）

把 `wallpaper/dist/` 构建产物变成**桌面交互壁纸**的方式。

## 前置

```bash
pnpm build   # 产物在 wallpaper/dist/（index.html + assets/）
```

> 产物使用根绝对路径（`base: '/'`），必须从站点根路径通过静态服务器提供；不要直接双击 `index.html`。`pnpm preview` 会提供可供网页壁纸引擎访问的本地 URL。

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

## 方式 B：Tauri 壳（当前主产品，自研 WorkerW 宿主）

需要"真壁纸层 + 穿透控制 + 托盘 + 系统锁屏联动"时，用 Tauri 2 做壳：

- 单一无边框 `background` WebView 注入 WorkerW，承载画面与桌面内热区；非热区由原生命中测试交还 Explorer
- 通过 tauri 事件把 SessionSwitch（锁屏/解锁）转发给前端状态机
- 提供托盘菜单（切换形态/设置/退出）
- MSIX 正式包使用固定 TaskId 的 Windows StartupTask 实现开机自启；旧版、开发版和不带该扩展的安装包回退到当前用户 Run 项。更新时会读取系统真实状态，并在可行时把旧项迁移到 StartupTask。
- 进程入口会在 Tauri/WebView2 初始化前尝试加载一张只读 `sleep.png` 原生首帧并挂到 WorkerW 下方；若 Explorer 尚未创建独立 WorkerW，会在限定窗口内重试并暂挂 Progman，随后由后台宿主恢复时重新挂载并同步尺寸。背景 WebView 完成两帧绘制后释放；资源和桌面宿主均不可用时仍安全跳过，不接管安全桌面。启动诊断写入进程的 `%LOCALAPPDATA%\DSHWallpaper\startup-diagnostic.log`，MSIX 的包容器重定向路径以 README 说明为准。

架构上**只换外壳**：`wallpaper/` 前端不变，Tauri 壳负责窗口/系统事件/托盘。

## 注意事项

- **登录态**：网页实验入口使用应用内持久 WebView2 打开 DeepSeek 官方页面；Cookie 仅由 WebView2 保存，本应用不读取、复制或记录。DOM 特征无法识别时会安全提示适配器需要更新。DSH 会话通过本地 bridge `http://127.0.0.1:3080` 连接。
- **穿透**：网页壁纸引擎的鼠标穿透策略各异；需要精细控制时走 Tauri 壳
- **3080 探测**：只在 `/api/wallpaper/v1/status` 的版本、鉴权状态与新建会话所需能力集完整匹配时启用 Harness；端口可达本身不会启用。`resume` 仅在 DSH 安装会话持久化服务时出现，不影响新建会话。
