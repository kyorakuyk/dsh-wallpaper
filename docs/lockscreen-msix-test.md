# 锁屏接管：MSIX 安装包验收

锁屏图片接管是 Windows 的用户级设置。本项目将 **MSIX 包身份** 作为锁屏能力的正式支持与验收前提：无包身份的 `tauri dev`、直接运行的 EXE 与常规 NSIS 安装在不同 Windows 11 环境中的行为不一致，因此不在本项目的锁屏支持矩阵内。

本文件说明的是开发/测试路径，不是面向用户的发布方式。自签名证书仅可用于本机测试，绝对不能随正式版本分发。

正式路径只使用 `UserProfilePersonalizationSettings::IsSupported()` 与 `TrySetLockScreenImageAsync` 的返回值/HRESULT 作为运行时门控。它不需要、也不调用 `RequestAccessAsync`（后者属于后台任务授权，不是锁屏图片 API）；如果 `TrySet…` 返回 `false` 或系统策略拒绝，应用会失败关闭并保留恢复点，不会静默改走旧接口。

## 先决条件

- Windows 11 x64（MSIX 清单 `Windows.Desktop` 最低版本为 `10.0.22000.0`），且已安装 Windows 10/11 SDK（包含 `MakeAppx.exe` 与 `SignTool.exe`）。
- `pnpm`、Rust/Cargo 能正常构建项目。

纯布局打包不需要已安装 WebView2 或 VCLibs，因此可在干净构建机上安全验证包内容。只有显式传入 `-InstallPackage` 进入本机安装验收时，脚本才会检查 Microsoft Edge WebView2 Evergreen Runtime 与 x64 `Microsoft.VCLibs.140.00.UWPDesktop`；缺少时会在安装前停止。

## 最安全的默认操作：只生成未签名包

下面命令会构建应用、重建本仓库受控的 `artifacts\msix-test\package-root`、打包并验证目录布局，但**不会**：

- 创建或安装证书；
- 安装/注册 MSIX；
- 修改锁屏或任何系统设置。

```powershell
.\scripts\build-msix-test.ps1
```

产物位于 `artifacts\msix-test\dsh-wallpaper-lockscreen-test.msix`。未签名 MSIX 不可安装，这是预期行为。

Rust 二进制会以锁定依赖固定构建为 `x86_64-pc-windows-msvc`，并显式启用 Tauri `custom-protocol`（避免程序误访问已停止的 Vite `devUrl`），输出到独立的 `artifacts\msix-test\cargo-target\x86_64-pc-windows-msvc\`；脚本会在打包前读取 PE Machine 字段，拒绝任何非 x64 EXE 或 `WebView2Loader.dll`，不会写入或占用开发实例使用的 `wallpaper\src-tauri\target\`。

如果已在本机通过本脚本完成相同 profile 的隔离构建，只验证打包布局：

```powershell
.\scripts\build-msix-test.ps1 -SkipBuild
```

`-SkipBuild` 不会回退读取常规开发 target；若隔离目录中没有对应程序，请去掉该参数先构建一次。

脚本会明确检查以下包内文件：

```text
dsh-wallpaper.exe
WebView2Loader.dll
dist/index.html
_up_/public/personas/wake-frames/variant-anima/sleep.png
Assets/*.png
AppxManifest.xml
```

这里的 `_up_/public/.../sleep.png` 与 Tauri 当前 Windows bundle 的 `BaseDirectory::Resource` 路径规则一致：资源目录就是 EXE 所在目录，Tauri 保留 `bundle.resources` 源文件的相对路径。壁纸前端本身仍从 `dist/` 加载。`WebView2Loader.dll` 不再从 Cargo 缓存中按“第一个 x64 文件”猜选；脚本以 `cargo metadata --locked` 解析唯一的 `webview2-com-sys` 包，再复制该包相邻的 x64 loader，并验证其 PE 架构。

## 显式创建本机测试证书并安装

以下命令会产生有副作用：在**当前用户**的 `My` 证书存储创建可导出的代码签名测试证书并导出 PFX/CER；为安装自签名 MSIX，则将 CER 加入**本机**的 `TrustedPeople`，然后注册当前用户的 MSIX。

Windows 的 AppX 部署服务不会把 `CurrentUser` 证书存储当作自签名发布者的机器级信任；因此安装路径必须显式传入 `-InstallMachineCertificate`，并在管理员 PowerShell 中把这张专用测试发布者证书放入 `LocalMachine\TrustedPeople`。这会信任该测试发布者签名的包，故它只能用于本机测试，验收结束后应移除。未使用该开关时，`-InstallCertificate` 只会导入 `CurrentUser\TrustedPeople` 供签名验证，脚本拒绝继续安装。

```powershell
.\scripts\build-msix-test.ps1 `
  -CreateTestCertificate `
  -InstallCertificate `
  -InstallPackage
```

脚本会交互要求一个 PFX 密码；不要把密码写入命令历史、脚本或 Git。所有测试证书文件都留在 `artifacts\msix-test\`，并应保持 Git 忽略。

脚本刻意区分两个阶段：`SignTool sign` 成功只说明签名已经写入包；Windows 是否信任该证书由 `SignTool verify /pa` 决定。新建的自签名证书在导入信任存储前出现“不受信任根”是正常情况，因此脚本只会在你显式要求 `-InstallCertificate` 或 `-InstallPackage` 时、并且在安装前运行 `/pa` 验证。若该验证失败，脚本不会安装软件包、不会自动扩大到机器级证书存储，也不会删除已签名包或证书材料，以便你按受控流程诊断和清理。

如需通过自动化提供密码，先在当前 PowerShell 会话内创建 `SecureString`：

以**管理员 PowerShell**执行：

```powershell
$password = Read-Host 'PFX password' -AsSecureString
.\scripts\build-msix-test.ps1 `
  -CreateTestCertificate `
  -CertificatePassword $password `
  -InstallCertificate `
  -InstallMachineCertificate `
  -InstallPackage
```

也可复用自己保管的**测试证书**；脚本不会导入外部 PFX 的私钥，只会在显式指定 `-InstallCertificate` 时导入同路径 `.cer` 公钥。此测试清单的 `Publisher` 固定为 `CN=DSH Wallpaper Test`，脚本会只读打开 PFX，并在签名之前检查其中唯一带私钥的终端证书 `Subject` 是否与它完全一致；不一致会停止，避免生成“签名成功但 Windows 无法安装”的包。

`-Release` 仅改用 Rust release profile，**不会**把这套测试清单变成可发布清单。组织/商店发行证书通常有不同的 Subject，必须使用由发布流程维护、Publisher 与证书匹配的独立清单；不要试图用本脚本的测试清单签正式包。

```powershell
$password = Read-Host 'PFX password' -AsSecureString
.\scripts\build-msix-test.ps1 -Release `
  -CertificatePath C:\safe\dsh-wallpaper-test.pfx `
  -CertificatePassword $password `
  -InstallCertificate `
  -InstallPackage
```

`-WhatIf` 可预览创建测试证书、安装证书和安装软件包等系统级副作用；它仍会构建并重建仓库内已忽略的 `artifacts\msix-test\` 打包产物，以便 MakeAppx 从当前输入验证布局，但不会创建 PFX/CER、写入证书存储或注册软件包：

```powershell
.\scripts\build-msix-test.ps1 -CreateTestCertificate -InstallCertificate -InstallPackage -WhatIf
```

## 验收清单

1. 从开始菜单启动已安装的 **DSH Wallpaper (Lock Screen Test)**，不要直接运行构建目录的 EXE。
2. 打开“设置 → 系统 → Windows 集成”。诊断必须确认当前进程有包身份；若显示“无包身份”，停止验收并检查是否从正确安装入口启动。
3. “接管前检查”必须显示：Windows 允许尝试、睡眠图已准备，并且当前锁屏为可私有备份的静态本地图片。
4. 如果当前锁屏来自 Windows Spotlight 或其他动态来源，应用必须拒绝接管，不能覆盖后再声称可恢复。
5. 启用“接管锁屏图片”，按 `Win+L`，确认显示熟睡画面；密码输入仍完全由 Windows 安全桌面处理。
6. 解锁后确认苏醒动画开始；回到设置点击“恢复原锁屏图片”。
7. 恢复前若故意在 Windows 设置中换了一张锁屏图，应用必须停止且保留恢复点，绝不能覆盖这张新图；随后重新启用接管，再执行正常恢复。
8. 再按 `Win+L`，确认恢复的是原静态图片；诊断不应再显示有效接管状态或未清理的有效备份。
9. 启动一次普通应用、回到桌面、再锁定/解锁，确认 WorkerW 桌面宿主与锁屏接管互不干扰。

## 清理测试环境

在卸载 MSIX 前，先通过应用的“恢复原锁屏图片”完成恢复。随后可以删除当前用户安装的测试包与测试证书：

```powershell
# 查看（不修改）
Get-AppxPackage -Name com.dsh.wallpaper
Get-ChildItem Cert:\LocalMachine\TrustedPeople |
  Where-Object Subject -eq 'CN=DSH Wallpaper Test'

# 删除测试包
Get-AppxPackage -Name com.dsh.wallpaper | Remove-AppxPackage

# 删除仅供测试的信任证书（确认 Subject / Thumbprint 后再执行）
Get-ChildItem Cert:\LocalMachine\TrustedPeople |
  Where-Object Subject -eq 'CN=DSH Wallpaper Test' |
  Remove-Item
```

可保留或安全删除 `artifacts\msix-test\` 中的 PFX/CER；PFX 含私钥，不能上传、发送或提交。

## 发布边界

- 常规 NSIS 包继续提供桌面壁纸功能；锁屏页面必须依据真实包身份提示“需要 MSIX 包身份”，不得因 debug/release 或安装方式猜测结果。
- 面向用户的 MSIX 必须由 Microsoft Store 或组织持有的受信任发行证书签名；不得分发本脚本生成的自签名证书。
- MSIX 测试包当前只面向 Windows 11 x64；发布版如需 x86/ARM64，必须分别构建、提供对应图标/运行时依赖并单独验收。
- 本实现只接管 `LockScreen::OriginalImageFile` 能返回、并可读取且可私有复制的本地静态锁屏图。Microsoft 文档说明该属性只检索文件（若图片通过 stream 设置则返回 `E_FILE_NOT_FOUND`）；因此任何非 `file:` URI、不可读文件或查询失败都保持失败关闭。Windows Spotlight 是常见的此类不可可靠恢复来源，但不是通过 URI 文案作推断。

## 实现依据

- `windows-rs 0.61` 应通过 `Win32::Storage::Packaging::Appx::GetCurrentPackageFullName` 检测当前进程包身份；`APPMODEL_ERROR_NO_PACKAGE` 表示无包身份。所需 feature 为 `Win32_Storage_Packaging_Appx`。
- Microsoft：[GetCurrentPackageFullName](https://learn.microsoft.com/windows/win32/api/appmodel/nf-appmodel-getcurrentpackagefullname)、[LockScreen.OriginalImageFile（只检索文件）](https://learn.microsoft.com/uwp/api/windows.system.userprofile.lockscreen.originalimagefile)、[TrySetLockScreenImageAsync（返回 `true` 才代表成功，重设时须使用不同文件名）](https://learn.microsoft.com/uwp/api/windows.system.userprofile.userprofilepersonalizationsettings.trysetlockscreenimageasync)、[UserProfilePersonalizationSettings 方法表](https://learn.microsoft.com/uwp/api/windows.system.userprofile.userprofilepersonalizationsettings)、[BackgroundExecutionManager.RequestAccessAsync（与锁屏图片无关）](https://learn.microsoft.com/uwp/api/windows.applicationmodel.background.backgroundexecutionmanager.requestaccessasync)、[命令行打包 MSIX](https://learn.microsoft.com/windows/msix/package/manual-packaging-root)、[MakeAppx `/h SHA256`](https://learn.microsoft.com/windows/msix/package/create-app-package-with-makeappx-tool)、[SignTool 签名与 `/pa` 验证](https://learn.microsoft.com/windows/win32/seccrypto/signtool)、[创建包签名证书](https://learn.microsoft.com/windows/msix/package/create-certificate-package-signing)、[为包签名证书建立信任](https://learn.microsoft.com/windows/msix/package/create-certificate-package-signing#install-the-certificate)、[桌面应用 MSIX 清单](https://learn.microsoft.com/windows/msix/desktop/desktop-to-uwp-manual-conversion)。
