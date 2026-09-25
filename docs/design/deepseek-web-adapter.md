# DeepSeek 网页适配器配置

DeepSeek 网页桥接通过 WebView2 的 DOM 操作工作。网页的 class、属性或路由发生小范围变化时，可以更新声明式配置文件，不必重新安装完整壁纸程序。

## 配置文件

仓库中的 [`wallpaper/src-tauri/config/deepseek-web-adapter.json`](../wallpaper/src-tauri/config/deepseek-web-adapter.json) 是内置默认配置，也是维护网页适配规则的基准文件。

运行中的用户 override 位于 Tauri 的应用配置目录，文件名为：

```text
deepseek-web-adapter.override.json
```

在设置中心的“连接 → DeepSeek 网页入口（实验）→ 网页适配器配置 → 打开配置”可以创建并打开它。Windows 的实际目录由 Tauri `app_config_dir()` 决定，MSIX 安装后可能位于包容器的 LocalCache 下；不要把 WebView2 的 Cookie 目录当成配置目录。

保存配置后，下一次网页状态读取、历史读取、导航、发送或停止操作会重新加载它。正在进行的发送回合会继续使用开始时已经验证过的那份配置。设置中心的“恢复默认”会移除 override 文件，使程序回到内置版本并能跟随之后的程序内置更新。

## 可以调整的内容

- `composerSelectors`：输入框或 contenteditable 节点
- `assistantSelectors`、`markdownSelectors`：助手消息和 Markdown 内容节点
- `messageSelectors`、`appShellSelectors`：历史消息和页面就绪提示
- `sendTokens`、`stopTokens`、`terminalTokens`、`loginTokens`：按钮或状态文字
- `assistantRoleTokens`、`userRoleTokens`：消息角色标记
- `conversationPathTemplate`：包含一个 `{id}` 的官方会话路径模板

`siteOrigin` 必须保持为 `https://chat.deepseek.com`。配置大小、字段数量、单项长度、控制字符和未知字段均有限制；解析或校验失败时，程序会保留 override 文件但使用内置默认配置，并在设置中心显示回退原因。

## 维护与发布边界

只改变选择器、标签或路由格式时，可以单独发布这个 JSON 文件，让用户替换 override 文件，不需要全量更新程序。推荐发布时同时给出配置版本、SHA-256 和变更说明。

当前版本支持本地 override 和内置回退；自动下载尚未启用。后续如果增加在线更新，必须使用固定 HTTPS 地址、内置公钥验证签名、原子替换、保留上一份可用版本，并且不能在启动关键路径同步下载。不能把远程 JSON 直接当 JavaScript 执行。

如果 DeepSeek 改变了流式完成语义、登录流程、反爬策略或需要新增 DOM 行为，配置文件无法覆盖，仍然需要更新适配器引擎。
