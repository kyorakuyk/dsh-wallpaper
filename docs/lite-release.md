# DSH Wallpaper Lite 首发版

Lite 是首发的轻量产品目标，与完整版共用一个仓库和 Windows 原生壁纸核心，但使用独立的前端入口、Cargo feature、Tauri 配置和 MSIX 身份。

## 首发范围

- Windows 锁屏图片接管与安全恢复
- 登录后睡眠画面到正式四帧苏醒动画
- 静态桌面背景与右侧立绘
- 设置中心可导入一张自定义背景和一张自定义立绘（PNG/JPEG/WebP，单张不超过 32 MB）
- 开机自启
- TranslucentTB 状态、启动和商店入口
- 精简设置中心

Lite 不包含 DeepSeek Web/API、Harness、3080 探测、聊天、历史、Token/费用、表里桌面、组件插件、主题包和交互输入岛。密码页仍由 Windows 安全桌面处理，Lite 不读取或注入密码。

## 本地只做代码验证

```powershell
pnpm typecheck
pnpm -C wallpaper build:lite
cargo check --manifest-path wallpaper/src-tauri/Cargo.toml --locked --no-default-features --features lite --bin dsh-wallpaper-lite
```

Lite 前端产物位于 `wallpaper/dist-lite/`，该目录已加入 Git 忽略。构建完成后应只包含正式四帧、内置背景和四套立绘，不包含候选图。

Lite 的普通设置保存在自己的应用目录；锁屏恢复点使用用户级共享目录，以便 Lite 与完整版切换时仍能安全恢复原锁屏图片。两个版本还共享一个 Windows 桌面宿主互斥锁，不会同时运行。

## 另一台 Windows 电脑生成 MSIX

不要在开发机上安装或验收。发布/测试机准备好 Windows 11 x64、Windows SDK、WebView2 Runtime、Microsoft.VCLibs.140.00.UWPDesktop 后执行：

```powershell
.\scripts\build-msix-test.ps1 -Edition lite -Release
```

脚本只在显式传入证书和安装参数时执行签名、证书导入或安装。正式发布必须使用受信任发行证书替换测试清单中的 `CN=DSH Wallpaper Test`，并把同一 Publisher 写入 `packaging/msix/AppxManifest-Lite.xml`。

## CI

`.github/workflows/package.yml` 使用 `full/lite` 矩阵构建 NSIS 和未签名 MSIX 测试产物；`.github/workflows/ci.yml` 会分别检查 Lite 前端、Lite Rust feature 和禁止内容边界。

GitHub 的手动 `workflow_dispatch` 入口要求工作流文件已经存在于默认分支。当前开发分支的 CI 已通过；合并到默认分支或由发布者推送版本标签后，Package workflow 才会生成远端安装包工件。
