# DSH Wallpaper Lite 首发版

Lite 是首发的轻量产品目标，与完整版共用一个仓库和 Windows 原生壁纸核心，但使用独立的前端入口、Cargo feature、Tauri 配置和 MSIX 身份。

## 首发范围

- Windows 锁屏图片接管与安全恢复
- 登录后睡眠画面到正式四帧苏醒动画
- 静态桌面背景与右侧立绘
- 设置中心可导入一张自定义背景和一张自定义立绘（PNG/JPEG/WebP，单张不超过 32 MB）
- 开机自启
- 可选的 Explorer 登录过渡底图（将普通桌面暂时对齐到睡眠画面，减少解锁后的原壁纸空档）
- TranslucentTB 状态、启动和商店入口
- 精简设置中心

Lite 不包含 DeepSeek Web/API、Harness、3080 探测、聊天、历史、Token/费用、表里桌面、组件插件、主题包和交互输入岛。密码页仍由 Windows 安全桌面处理，Lite 不读取或注入密码。

## 登录过渡底图（可选）

设置中心的“登录过渡底图”会把 Explorer 普通桌面壁纸切换为内置 `sleep.png`，让用户输入密码后到应用原生首帧接管之间不再闪出用户原壁纸。它不是 Shell 替换，也不参与密码页；应用只保存当前静态壁纸的恢复点，遇到 Spotlight、幻灯片、动态壁纸或检测到用户已改过壁纸时会拒绝覆盖。

该开关默认关闭。启用前请确认当前桌面使用的是可读的静态图片；关闭时应用只在当前壁纸仍是它管理的睡眠图时恢复原图。若外部程序已经更换壁纸，恢复点会保留并显示冲突提示，不会强行覆盖。卸载前应先在设置中心关闭该开关。

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

`.github/workflows/package.yml` 的首发工作流只构建 Lite：普通 NSIS 安装器、带临时测试证书签名的 MSIX、公开 `.cer`，以及把 MSIX/CER/安装脚本封装在一起的 `dsh-wallpaper-lite-msix-test-setup.exe`。完整版仍可通过本地开发命令和 `scripts/build-msix-test.ps1 -Edition full` 做工程检查，但不会混入 Lite 首发 Release。`.github/workflows/ci.yml` 会分别检查 Lite 前端、Lite Rust feature、禁止内容边界和 Lite 权限清单。

### 另一台电脑安装 Lite 测试包

优先从 GitHub Release 下载 `dsh-wallpaper-lite-msix-test-setup.exe`，并确认它来自你信任的那次发布。该引导器会在明确提示后请求管理员权限，校验 MSIX 的签名者、清单 Publisher 与内置 CER 完全匹配，将 CER 导入“本地计算机 / 受信任的人”，再为当前用户注册 MSIX。它不会安装 PFX 私钥，也不会写入受信任的根证书存储。

这是测试引导器，不是正式发行安装器：引导器本身在未配置正式发行证书时也可能触发 SmartScreen 或显示未知发布者；只有在确认来源和证书指纹后才应继续。不要安装或传播 CI 中被删除的 `.pfx` 私钥。若不使用引导器，也可以从同一次 Release 下载 `.msix` 与 `.cer`，手动完成相同的信任导入流程。该证书只用于本次测试，正式发布必须替换为受信任发行证书并同步更新清单 Publisher。锁屏接管只在 MSIX 包内启用，NSIS 安装器用于检查普通壁纸、自启、动画、立绘和 TranslucentTB 兼容入口。

测试机还需要 Windows 11 x64、官方 WebView2 Runtime 和 `Microsoft.VCLibs.140.00.UWPDesktop`；这些是 MSIX 的系统依赖，不会被 Lite 安装器捆绑。安装完成后从开始菜单启动 **DSH Wallpaper Lite**，再在设置中心开启锁屏图片接管。若 Windows 拒绝测试证书或依赖包，先不要反复安装，记录错误并保留原锁屏图片。

### 本地生成测试引导器

在已经生成并签名的 MSIX 与匹配 CER 旁运行：

```powershell
.\scripts\build-msix-test-bootstrapper.ps1 `
  -MsixPath .\artifacts\msix-test\dsh-wallpaper-lite-lockscreen-test.msix `
  -CertificatePath .\artifacts\msix-test\dsh-wallpaper-test.cer `
  -OutputPath .\artifacts\msix-test\dsh-wallpaper-lite-msix-test-setup.exe
```

构建器会在 IExpress 封装前拒绝 Publisher、签名证书和 CER 不一致的组合；它只封装公开 CER，不会把 PFX 放进 EXE。IExpress 的命令行模式要求构建路径不要包含空格。

GitHub 的手动 `workflow_dispatch` 入口要求工作流文件已经存在于默认分支。当前开发分支的 CI 已通过；合并到默认分支或由发布者推送版本标签后，Package workflow 才会生成远端安装包工件。
