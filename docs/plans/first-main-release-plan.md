# 完整版首次正式发布计划

状态：**待施工**（本文只做调研与决策，未改任何代码、未构建、未安装、未提交）。写于本次调研。

本文回答一个问题：要让**完整版**（区别于现在本机构建的 `dsh-wallpaper-lockscreen-test.msix`，也区别于当前 CI 只出 Lite 的公开 Release）可以正式发布，需要补齐什么。

每一条事实都标注了取证方式：`文件:行`、命令输出，或外部 URL。外部结论标注置信度。

---

## 1. 目标与范围

### 1.1 本文覆盖什么

- 完整版（`edition = full`，`com.dsh.wallpaper`）的**第一次正式对外发布**。
- 一个渠道：GitHub Releases（见第 5 节）。
- 一种包形态的**选定**（见第 3 节）——这是本文的核心决策。

### 1.2 本文不覆盖什么

- Lite 版的首次发布。Lite 与完整版是两个身份（`com.dsh.wallpaper.lite` / `com.dsh.wallpaper`），两条版本线互不相干（`packaging/msix/AppxManifest-Lite.xml:10` 的 `Version="0.1.2.0"` 对 `packaging/msix/AppxManifest.xml:10` 的 `Version="0.2.0.74"`，见第 2.3 节）。
- 更新检测功能的实现。它有自己的计划书：`docs/plans/wallpaper-update-detection-plan.md`（状态：待施工）。
- 商店上架运营、定价、法务主体。

### 1.3 「正式发布」在这份文档里的定义

发布物同时满足：包形态已选定并有受信任签名；版本号与标签按约定（第 2.3、5.2 节）落地；第 6 节清单全部完成；第 7 节验收条件全部通过。

---

## 2. 现状

### 2.1 本机现在有什么

| 事实 | 取证 |
| --- | --- |
| 完整版 MSIX **已安装**在本机，版本 `0.2.0.205` | `Get-AppxPackage -Name 'com.dsh.wallpaper'` 输出：`Name=com.dsh.wallpaper, Version=0.2.0.205, PackageFullName=com.dsh.wallpaper_0.2.0.205_x64__pdxj8y3r6rm5g` |
| 该安装的签名者不是受信任发行者 | 同一命令返回 `SignatureKind = Developer`（即本机开发态注册） |
| 安装目录里的 exe 无 Authenticode 签名 | `Get-AuthenticodeSignature <InstallLocation>\dsh-wallpaper.exe` 返回 `Status = NotSigned` |
| **Lite 从未在本机安装过** | `Get-AppxPackage | Where-Object { $_.Name -like '*dsh*' -or $_.Name -like '*wallpaper*' }` 只返回 `com.dsh.wallpaper 0.2.0.205` 一行，没有 `com.dsh.wallpaper.lite` |
| 「已安装」≠「已按文档验收」 | README.md:30 与 docs/plans/v0.3-product-rewrite-plan.md:80 都写明已安装 MSIX 的 `Win+L` 人工验收**尚待执行** |

本机 `Cert:\LocalMachine\TrustedPeople` 里累积了 **11 张** `Subject = CN=DSH Wallpaper Test` 的自签证书（`Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Subject -eq 'CN=DSH Wallpaper Test'`，指纹如 `F83AA64A…`、`C799677D…`，到期日集中在 2027-08/2027-09）。这是反复本地发布留下的堆积，本身就是一条待清理项（第 6 节）。

### 2.2 今天的测试包是怎么签的、怎么被信任的

- 签名主体是脚本生成的自签证书，`Subject = CN=DSH Wallpaper Test`：
  - `packaging/msix/AppxManifest.xml:10` → `Publisher="CN=DSH Wallpaper Test"`
  - `packaging/msix/AppxManifest-Lite.xml:10` → 同一个 `Publisher`
  - `docs/guides/lite-release.md:44` 写明「正式发布必须使用受信任发行证书替换测试清单中的 `CN=DSH Wallpaper Test`，并把同一 Publisher 写入 `packaging/msix/AppxManifest-Lite.xml`」
- 信任是靠一个引导器把公开 `.cer` 导入「本地计算机 / 受信任的人」：
  - `docs/guides/lite-release.md:52`：引导器「会请求管理员权限，校验 MSIX 的签名者、清单 Publisher 与内置 CER 完全匹配，将 CER 导入"本地计算机 / 受信任的人"，再为当前用户注册 MSIX。它不会安装 PFX 私钥，也不会写入受信任的根证书存储。」
  - 该引导器由 `scripts/build-msix-test-bootstrapper.ps1` 生成（IExpress 封装）。
- CI 每次运行**现场生成**一次性证书，只上传公开 `.cer`，私钥 `.pfx` 当场删除：`.github/workflows/package.yml:85-92`（生成随机密码）与 `:105-108`（挑出 `.cer`、`Remove-Item` 掉 `.pfx`）。
- 产物名与显示名：

| 名称 | 值 | 依据 |
| --- | --- | --- |
| 完整版 MSIX 文件名 | `dsh-wallpaper-lockscreen-test.msix` | `scripts/build-msix-test.ps1:417` |
| Lite MSIX 文件名 | `dsh-wallpaper-lite-lockscreen-test.msix` | 同上 |
| 完整版清单显示名 | `DSH Wallpaper (Lock Screen Test)` | `packaging/msix/AppxManifest.xml:12` |
| 完整版应用显示名 | `DSH Wallpaper` | `packaging/msix/AppxManifest.xml:35` |
| 完整版包名 | `com.dsh.wallpaper` | `packaging/msix/AppxManifest.xml:10` |
| Lite 包名 | `com.dsh.wallpaper.lite` | `packaging/msix/AppxManifest-Lite.xml:10` |

「Lock Screen Test」直接写进了正式清单的显示名里。正式发布前必须改掉（第 6 节）。

### 2.3 版本号现状：四套数字，互不同步

| 位置 | 当前值 | 依据 |
| --- | --- | --- |
| `package.json` | `0.1.0` | `package.json` 的 `"version"` |
| `wallpaper/src-tauri/tauri.conf.json` | `0.2.0` | 该文件第 4 行 |
| `wallpaper/src-tauri/Cargo.toml` | `0.2.0` | 该文件第 3 行 |
| `packaging/msix/AppxManifest.xml`（完整版） | `0.2.0.74` | 该文件第 10 行 |
| `packaging/msix/AppxManifest-Lite.xml`（Lite） | `0.1.2.0` | 该文件第 10 行 |
| 本机已安装完整版 | `0.2.0.205` | `Get-AppxPackage`；注意它**不等于**仓库里清单写的 `0.2.0.74` |

本地发布脚本每次把清单版本往上顶一格以高于已安装版本，所以「仓库清单版本」和「本机已安装版本」本来就会漂移：

- `scripts/publish-local-msix.ps1:109-113`：若源版本不高于已安装版本，就把第四段 revision 加一（`revision = max(0, installed.Revision) + 1`，上限 65535）。
- `scripts/publish-local-msix.ps1:57-58`：MSIX 版本必须是四段数字 `^\d+\.\d+\.\d+\.\d+$`。
- `artifacts/` 下的发布日志到 `publish-0.2.0.99.log` 为止，而本机是 `.205`——说明有相当一部分发布没有留日志，也说明版本号已被当作本地计数器频繁消耗。

`artifacts/` 里另有 `publish-0.2.0.100.log` 到 `publish-0.2.0.111.log` 等一批 12 万字节级日志，以及一批补丁与截图文件（`artifacts/app-tsx-mine*.patch`、`artifacts/island-*.png` 等），属于开发过程残留。

### 2.4 CI 今天到底产出什么

`.github/workflows/package.yml` 是唯一的打包工作流。

- 触发：`workflow_dispatch` 手动，或推送 `v*` 标签（第 4-7 行）。
- **矩阵只有 lite**：

```yaml
      matrix:
        # The first public package is intentionally Lite-only.  The full
        # edition remains in the repository for later work, but must not be
        # accidentally shipped with this release.
        edition: [lite]
```

（`.github/workflows/package.yml:21-27`）

- 产出四类工件（均 `retention-days: 14`）：Lite NSIS 安装器（`:121-127`）、Lite 自签 MSIX（`:129-135`）、公开 `.cer`（`:137-143`）、Lite MSIX 测试引导器（`:145-151`）。
- 只有带 `v` 前缀的标签才会建 Release，且只下载 `dsh-wallpaper-lite-*` 模式的工件：

```yaml
        pattern: dsh-wallpaper-lite-*
```

（`.github/workflows/package.yml:167-172`）

**在 `package.yml` 单独看来**：推一个 `v*` 标签，Release 里不会出现任何完整版产物，完整版连构建都不会发生。

`ci.yml` 是纯校验工作流（`pnpm typecheck`、`pnpm test`、`cargo test`、`pnpm build`、Lite 前端/Rust 检查、`scripts/verify-lite-bundle.ps1`），不产出可发布包（`.github/workflows/ci.yml:66-88`）。

### 2.4.1 完整版工作流已经出现（本次调研期间新增）

`.github/workflows/package-full.yml` 是本次调研进行中被**另一个 agent 新建**的（出现时该文件仍为未跟踪状态：`git status --short` 显示 `?? .github/workflows/package-full.yml`）。它改变了第 2.4 节的结论，因此单列一节。

- 触发：`workflow_dispatch`（可填 `package_version` 输入）或推送 `v*` 标签（`:21-30`）。
- 版本来源三分支（`:96-143`）：
  1. 手动运行且填了 `package_version` → 用它；必须 `^\d+\.\d+\.\d+\.\d+$` 且每段 ≤ 65535（`:104-112`）；
  2. `v*` 标签 → 去掉 `v`，不足四段补 0；**不像版本号的 `v*` 标签直接失败**（`:126-128`）；
  3. 手动运行且没填 → `<Cargo.toml 的 major>.<minor>.<运行号>.0`（`:133-142`），注释说明第一次运行即 `0.2.1.0`，且**刻意高于当前在用的 `0.2.0.205`**（`:17-19`）。
- 临时清单替换 `Identity Version`，仓库文件不动（`:149-158`），与 `publish-local-msix.ps1` 的做法一致（`:94-95` 注释）。
- 签名方式与 Lite 完全一致：runner 上现生成一次性自签证书，`-Edition full -Release -CreateTestCertificate`，上传前删 `.pfx`，只留公开 `.cer`（`:160-174`、`:222-223`）。
- **上传前有一道校验**（`:176-217`）：解包核对 `Identity Name` / `Identity Version` / `Publisher`，并核对 `.cer` 指纹与 MSIX 签名证书指纹一致，不一致就抛错。这正好挡住了「版本写错却照常上传」。
- 产物名：`dsh-wallpaper-full-msix-test-signed`、`dsh-wallpaper-full-msix-test-certificate`（`:231`、`:239`），保留 14 天。
- Release 任务：`gh release view` 判断存在性，不存在才 `gh release create --verify-tag`，然后 `gh release upload … --clobber`；**只上传 `.msix`，不上传 `.cer`**，理由写在 `:262-263`——Lite 的 Release 资产里已经有一个同名的 `dsh-wallpaper-test.cer`，不碰它就不会互相覆盖。

这解决了两件事，也留下两个新问题（都进了第 6/8 节）：

- 解决：完整版现在有远端构建与下载入口，且两个工作流被设计成**往同一枚 Release 里并排放资产**（`:5-10` 注释说明分工与幂等），所以第 8.4 节原来设想的「两条版本线混在一个 release 流」已经是既定设计，问题变成「必须确保 `releases/latest` 解析正确」。
- 新问题一：版本解析只与**清单里的** `0.2.0.74` 比对，比对不上时仅 `Write-Warning`（`:145-147`）。**它完全不检查本机/已发布过的更高版本**，而注释自己承认 `0.2.0.205` 是「当前在用」的版本（`:17-19`）。若标签或输入算出的版本低于用户已装版本，MSIX 会拒绝升级。
- 新问题二：`.cer` 只进 Actions artifact（保留 14 天），**不进 Release**。全新安装的测试用户在 14 天后将拿不到与本轮 MSIX 配对的公开证书（第 6 节 D14）。

### 2.5 已有的发布机械

`scripts/` 下与发布直接相关的：

| 脚本 | 作用 | 关键依据 |
| --- | --- | --- |
| `build-msix-test.ps1` | 组装并（可选）签名 MSIX；`-Edition` 默认 **full**，`ValidateSet('full','lite')` | 该文件第 22-23 行 |
| `build-msix-test-bootstrapper.ps1` | 把 MSIX + CER 封成 IExpress 引导安装器；只封 CER，不封 PFX | `docs/guides/lite-release.md:69` |
| `install-msix-test.ps1` | 测试机安装 | 文件名 |
| `publish-local-msix.ps1` | 本机整套发布（构建+签名+安装+回滚点） | 该文件第 21-24 行的 `param` |
| `build-lockscreen-probe.ps1` | 构建最小锁屏诊断包 | `scripts/build-lockscreen-probe.ps1:12,38` |
| `verify-lite-bundle.ps1` | Lite 产物边界门禁 | 该文件第 9-44 行 |
| `probe-bridge-readiness.ps1` | Bridge 就绪探测 | 文件名 |

`scripts/` 下另有 **11 个素材生成脚本**（`generate-wake-frames*.py`、`generate-sleep-scene.py`、`fix-frame4*.py`、`generate-*bg*.py`、`remove-bg.py` 等）。它们是开发工具，不是发布机械。

NSIS 侧的 `resources` 只显式带一个文件，安装模式是当前用户：

- `wallpaper/src-tauri/tauri.conf.json:22` → `"targets": ["nsis"]`
- `wallpaper/src-tauri/tauri.conf.json:26` → `"resources": ["../public/personas/wake-frames/variant-anima/sleep.png"]`
- `wallpaper/src-tauri/tauri.conf.json:27` → `"nsis": { "installMode": "currentUser" }`

**`installMode: currentUser` 是重要前提**：它意味着 NSIS 安装器不提权。第 3.3 节的 `PersonalizationCSP` 路线要求写 `HKLM`，与该前提直接冲突。

### 2.6 仓库里的标签

```
$ git tag --list
checkpoint-2026-09-28
checkpoint-2026-09-28-lw
v0.1.0-lite
v0.1.0-lite.1
```

`checkpoint-*` 这类标签无法比较大小——这正是 `docs/plans/wallpaper-update-detection-plan.md:38` 所说「现存标签是 `checkpoint-2026-09-28` 这类，无法比较大小 ⇒ 先要约定」。现有的两个 `v0.1.0-lite*` 标签带 `-lite` 后缀，**不匹配**该计划书约定的 `^v\d+(\.\d+){0,3}$`（同文件第 48 行），因此按计划书的解析规则会被当作「没有新版本」。这两条标签属于已发布的历史，本文不改动它们，但新约定必须明确 Lite 与完整版如何共用 `v*` 标签空间。

---

## 3. 核心决策：打包形态

### 3.1 决定性事实（先说结论）

**锁屏接管在本仓库里被硬绑定在 MSIX 包身份上，且没有等价回落；同时它并不是「唯一未打包就无法工作的东西」。**

代码依据（`wallpaper/src-tauri/src/windows_integration.rs:2798-2806`）：

```rust
    // Both takeover *and* restoration call the system setter.  Do not let an
    // unpackaged dev/NSIS process mutate the lock screen just because it finds
    // a recovery manifest left by an earlier run. The supported surface is an
    // MSIX-identified process only; preserving the manifest is safer than an
    // unverified write from a different installation context.
    let has_package_identity = has_package_identity()?;
    if !can_attempt_lock_screen_takeover(has_package_identity) {
        return Err("当前进程没有 MSIX 包身份。为确保锁屏接管和恢复可验证，常规桌面版不会修改系统锁屏；现有恢复点已保留。请安装 MSIX 包后重试。".into());
    }
```

这个守卫是**无条件**的：`can_attempt_lock_screen_takeover` 只转述了它的入参，没有任何编译期开关或特性门（`wallpaper/src-tauri/src/windows_integration.rs:3925-3928`）：

```rust
#[cfg(windows)]
fn can_attempt_lock_screen_takeover(has_package_identity: bool) -> bool {
    has_package_identity
}
```

工程测试把这一点钉住了（同文件 `:4293-4298`）：`assert!(can_attempt_lock_screen_takeover(true)); assert!(!can_attempt_lock_screen_takeover(false));`

包身份来自 `GetCurrentPackageFullName`，`APPMODEL_ERROR_NO_PACKAGE` 表示无身份（同文件 `:3179-3192`）。

**这不是「代码留下的模糊地带」，而是项目自己写下来的决定**，三处一致：

1. `docs/guides/lockscreen-msix-test.md:146`：「常规 NSIS 包继续提供桌面壁纸功能；锁屏页面必须依据真实包身份提示"需要 MSIX 包身份"，不得因 debug/release 或安装方式猜测结果。」
2. `docs/plans/v0.3-product-rewrite-plan.md:78`：「只允许具有真实 MSIX 包身份的构建调用系统锁屏 API；无包身份的 `tauri dev` / NSIS 桌面程序不会接管或恢复系统锁屏。」
3. `docs/guides/lite-release.md:54`：「锁屏接管只在 MSIX 包内启用，NSIS 安装器用于检查普通壁纸、自启、动画、立绘和 TranslucentTB 兼容入口。」

### 3.2 关键的回落路径只覆盖桌面壁纸，不覆盖锁屏

必须说清楚，因为两者极易混淆：

- `wallpaper/src-tauri/src/desktop_fallback.rs` 的「登录过渡底图」用 `IDesktopWallpaper::SetWallpaper`，旧接口回落 `SystemParametersInfoW(SPI_SETDESKWALLPAPER)`（该文件 `:372-379`、`:354-369`）。
- **它设的是桌面壁纸，不是锁屏图片。** 该文件的常量与函数名都明确：`SLEEP_ASSET` / `bundled_sleep_source` / `set_fallback`（`desktop_fallback.rs:48,341`）。
- **桌面壁纸 API 不需要包身份**，因此 Lite 的这个功能在 NSIS 里能正常工作。
- `lock_screen_diagnostics` 会把这个事实直接报给 UI：`takeover_available = supported && can_attempt_lock_screen_takeover(package_identity)`（`windows_integration.rs:4194-4196`），前端接口里也有 `packageIdentity` 与 `takeoverAvailable` 两个独立字段（`wallpaper/src/native/runtime.ts:25`）。

**所以「NSIS 安装的完整版」在该形态下能做的桌面相关功能是完整的，丢掉的恰好是产品叙事里最独特的那一项。**

### 3.3 三种形态的对比

| | A. 纯 MSIX（完整包） | B. 纯 NSIS（无身份） | C. NSIS + 稀疏包（packaging with external location） |
| --- | --- | --- | --- |
| **锁屏接管** | 可用 | **不可用**（代码硬拒绝，见 3.1） | 预期可用——但**未验证**，见 3.5 |
| 桌面壁纸/登录过渡底图 | 可用 | 可用 | 可用 |
| 开机自启 | StartupTask，或 `explorer.exe shell:AppsFolder\<family>!Wallpaper` 别名（`windows_integration.rs:3229-3232`） | 写自己的 exe 路径（同文件 `:3249-3254`） | 需决定走哪条；有身份时可走别名 |
| `HKCU\...\Run` 写入 | 需 `virtualization:ExcludedKeys` 窄口子（`packaging/msix/AppxManifest.xml:21-25`），代码注释解释了不加会怎样 | 直接写，本来就是真实键 | 取决于注册方式 |
| 安装模型 | Windows 部署；**包不能自我替换**（`wallpaper-update-detection-plan.md:35`） | 传统安装器，自己管升级 | 安装器 + 稀疏包注册；升级两者都要动 |
| 安装是否需要提权 | 否（当前用户注册） | 当前用户即可（`tauri.conf.json:27`） | 注册身份包通常需管理员 |
| 用户可见的信任门槛 | 见第 4 节 | SmartScreen；自签等于无签名 | 两者叠加 |
| 仓库改动量 | 几乎为零（现有 `AppxManifest.xml` 就是完整版清单） | 中：需接受永久失去锁屏接管，且要在 UI 明确提示 | 大：新增稀疏包清单、安装器里加注册/注销步骤、`has_package_identity` 的语义要重新验证 |
| Store 上架 | 可行（MSIX 提交，Store 重新签名） | 只能走 MSI/EXE 提交且必须自备受信任签名、且必须静默安装（Store Policy 10.2.9） | 稀疏包本身不可直接提交 Store |

### 3.4 形态 A 与产品定位一致

`docs/guides/lite-release.md:1` 把 Lite 定义为「首发的轻量产品目标」；`README.md:7-16` 同样把首发定位在 Lite。完整版的差异化能力（聊天、Harness、表里桌面、主题、更新检查）都在 MSIX 之外也能跑，唯一被身份卡住的是锁屏接管。既然锁屏接管被明确列为 Lite 的首发范围（`docs/guides/lite-release.md:7`），那么完整版把同一能力保留下来，是维持两个版本能力关系一致的最省事做法。

### 3.5 形态 C 的真实状态：能解决，但没验证过

`packaging with external location` 确实是微软支持的、用现有安装器获得包身份的正规路线：MSIX 文档说「Starting in Windows 10, version 2004, you can grant package identity to an app simply by building and registering a *package with external location* with your app … allows you to register a simple identity package in your existing installer without changing how or where you install your application」，并明确「If you have an existing app with its own installer (WiX, NSIS, InstallShield) and want to add Windows features that require package identity - without replacing your installer with MSIX, use packaging with external location」。（[Grant package identity by packaging with external location](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps-overview)、[Choose a distribution path](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path)，置信度：高——文档原文且是当前版本）

但**必须如实标注三点不确定**：

1. 该文档没有列举「哪些 API 在稀疏包身份下可用」。`TrySetLockScreenImageAsync` 是否在稀疏包身份下通过，微软文档**没有正面说明**，本仓库也从未测过（`artifacts/` 下有 `lockscreen-probe` 相关记录，但探针清单是**完整 MSIX**：`scripts/build-lockscreen-probe.ps1:12` 使用 `packaging\msix\LockScreenProbe.AppxManifest.xml`，不是稀疏包）。
2. 代码用的是 `GetCurrentPackageFullName` 判身份，它读的是进程的包身份。稀疏包应该能让它为真，但**未实测**。
3. 稀疏包本身的签名、以及「注册稀疏包是否需要管理员」，本仓库没有任何现成脚本。

**因此形态 C 是一个需要先做技术验证的选项，不是可以直接排期的选项。** 若要选 C，第一步必须是在一台干净的 Windows 11 上跑通「NSIS 安装 → 注册稀疏包 → `has_package_identity()` 为真 → 锁屏接管成功 → 恢复成功」，再谈发布。

### 3.6 建议

**选 A：完整版以 MSIX 形态首发。** 理由：它是今天唯一被验证过的形态（本机 `0.2.0.205` 就是它），零额外工程风险，直接保住锁屏接管，并且顺带解决签名问题（第 4 节：Store 路线重新签名，不需要自购证书）。

把 C 记为「后续可选优化」，只在出现明确需求（例如必须支持 `.exe` 安装器、或必须绕开 App Installer）时再立验证任务。

B 只有在用户愿意接受「完整版没有锁屏接管」时才成立。本文不推荐，因为那会让完整版在核心叙事上弱于 Lite。

---

## 4. 签名与费用

### 4.1 外部事实（微软自己的口径）

微软的 [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) 页面给出：

| 证书类型 | 首次下载的 SmartScreen 行为 |
| --- | --- |
| Microsoft Store | 无警告——由微软证书覆盖 |
| 有效证书（OV/EV） | 有警告——在积累信誉前显示为「无法识别的应用」；会显示已验证的发布者名称 |
| 无签名 | 有警告——「Windows protected your PC」；用户须选「仍要运行」 |
| 自签证书 | 有警告——**与无签名相同** |

同页明确写着：

> EV certificates no longer bypass SmartScreen. Years ago, signing files with an Extended Validation (EV) code signing certificate would result in positive SmartScreen reputation by default, but this behavior no longer exists. … Paying a premium for EV solely to avoid SmartScreen warnings is no longer justified.

以及：

> The simplest way to avoid SmartScreen warnings is to publish through the Microsoft Store. Store-distributed apps are signed by a Microsoft certificate and are never subject to SmartScreen download warnings.

[Code signing options for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)（最后更新 2026-08-29）给出可比的费用与可用性：

| 选项 | 费用 | 可用性 | SmartScreen | 可上 Store |
| --- | --- | --- | --- | --- |
| Store（MSIX），Store 重新签名 | 免费 | 全球 | 无警告 | 是 |
| Store（MSI/EXE 安装器），发布者自签 | 需链到 Microsoft Trusted Root Program 的证书 | 全球 | 安装时无 SmartScreen（仍可能有 UAC） | 是 |
| Azure Artifact Signing（原 Trusted Signing） | 约 $9.99/月 | 组织：美国、加拿大、EU、英国；**个人开发者：仅美国与加拿大** | 有警告，信誉靠累积 | 否 |
| OV 证书 | $150–300/年 | 全球 | 同上 | 否 |
| EV 证书 | $400+/年 | 全球 | 自 2024 起与 OV 相同，**不再即时绕过** | 否 |
| 自签证书 | 免费 | — | 阻断公众安装 | 否 |
| 无签名 | 免费 | — | 强阻断 | 否 |

补充事实（同一页面）：OV「As of June 2023, the CA/Browser Forum requires private keys for OV certificates to be stored on a hardware security module (HSM) or hardware token」，即 2023 年 6 月起 OV 私钥必须落在硬件令牌或云 HSM 上。

地域限制另有一处更宽的表述（[Artifact Signing quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)）：

> Public Trust certificates are available to organizations in the United States, Canada, the European Union, the United Kingdom, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway, and Israel. Individual developers must be located in the United States or Canada.

**两处来源的国家清单不完全一致**（`code-signing-options` 只列「USA, Canada, EU, UK」，`quickstart` 还列了澳、新、日、韩、新、瑞士、挪、以），但**在「个人开发者仅限美国或加拿大」这一点上完全一致**。置信度：高。

同一 quickstart 还说明个人身份验证的硬性前置：

> For Public Trust individual identity validation, Artifact Signing automatically sources identity details from the Azure billing account associated with the subscription used to create the Artifact Signing resource. The billing account must have an Account Type of Individual. The legal name and sold-to address on the billing account must match the information on the government-issued ID used for identity validation.

且身份验证处理时间为「1 to 20 business days（可能要更久）」。（同上，置信度：高）

### 4.2 Microsoft Store 开发者账号：现在是免费的

[Free developer registration for individual developers](https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer) 写明：

> **No registration fee** | The $19 registration fee is waived in the new flow.

[Steps to open a developer account](https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account?tabs=individual) 更进一步：

> With the new onboarding experience, there are **no registration fees** for either account type, so you can create your developer account and start publishing at no cost.

（两页一致，后者更宽：两种账号类型都免费。最后更新 2026-05-07 / 2026-04-18。置信度：高）

但同一个「Steps to open a developer account」页面给出了值得警惕的账号类型定义：

> **Independent developers** whose distribution of apps through the Store is **not in relation to their business, trade, or profession**

若一个应用与本人的职业、行业或业务相关，按这一定义应当使用 Company 账号；同页也说明「Changing a developer account from Individual to Company is **not** supported in Partner Center」。Store Policy 10.14 同样规定 Company 账号适用于「organizations, businesses, and any person acting in relation to their trade or profession」，并且「if a reasonable consumer would interpret your application or publisher name to be that of a business entity」时公司账号是**必需**的。（[Microsoft Store Policies v7.20](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies)，发布日 2026-09-15，生效日 2026-10-22。置信度：高）

### 4.3 一个必须正视的地理约束

**本节所有结论对「个人开发者在中国大陆」这一情形的影响**：

- Azure Artifact Signing：**个人不可用**（仅美/加）。
- OV/EV：全球可买，但需要 CA 完成主体验证；个人能否购买取决于具体 CA 的政策，本文**未能从微软或 CA 官方页面确认一家给出「中国大陆个人可签发」的明确承诺**——见第 9 节未确定项。
- Microsoft Store：全球可用、免费，且**MSIX 提交由 Store 重新签名**，因此完全不需要自购证书。这是唯一同时满足「免费 + 无 SmartScreen 警告 + 保住锁屏接管」的路径。

### 4.4 每个选项对仓库的具体改动

| 选项 | 仓库要改什么 |
| --- | --- |
| **Store 上架 MSIX** | `packaging/msix/AppxManifest.xml:10` 的 `Publisher` **保持与 Store 账号绑定的一致值**（由 Partner Center 给出）；`:12` 的显示名去掉 `(Lock Screen Test)`；`:13` 的 `PublisherDisplayName` 同步；版本号按第 5.2 节约定；CI 从「自签测试包」改为「产出可提交的 MSIX」（不需自签，即 `package-full.yml` 的 `-CreateTestCertificate` 需要去掉）。不需要自购证书。 |
| **自购 OV/EV + 自行分发** | 同上改 `Publisher`，且必须是证书里的**实际主体字符串**；`publish-local-msix.ps1` 的签名参数改指向真实证书/云 HSM；第 6 节里的「证书信任引导」要么废弃、要么改为「用户自行确认发布者」。 |
| **Azure Artifact Signing** | 除上者外，还需满足 4.1 的地域与 Azure 计费账号类型前置；CI 侧改为接入云签名（无需硬件令牌）。 |
| **继续自签** | 不改仓库，但**这不是发布选项**：微软文档明确自签与无签名在 SmartScreen 上同档。 |
| **形态 C（稀疏包）** | 在以上任一项之外，额外新增稀疏包清单、安装器内的注册/注销步骤，以及第 3.5 节的技术验证。 |

**注意：`Publisher` 字符串一变，包族名（package family name）就变。** 今天本机的包族名是 `com.dsh.wallpaper_pdxj8y3r6rm5g`，最后 13 位是 Publisher 的哈希。改名的影响面：

- `windows_integration.rs:4320-4384` 的两个测试把 `com.dsh.wallpaper_pdxj8y3r6rm5g` **硬编码**在断言里（`:4366`、`:4376`、`:4389`），换 Publisher 后这几处必须更新。
- 已安装用户的开机自启项里记的是旧别名 `explorer.exe shell:AppsFolder\<旧族名>!Wallpaper`；代码有迁移逻辑（`migrate_legacy_autostart`，`README.md:113` 提到），但 README.md:136 同时记录了「壁纸自启项指向已安装 `0.2.0.71`」这类历史值，说明该迁移路径被真实使用过、也说明这是易错区。**发布前应专门回归一次「旧族名 → 新族名」的自启迁移。**

---

## 5. 发布渠道与更新链路

### 5.1 渠道

GitHub Releases 是唯一既定更新源。`docs/plans/wallpaper-update-detection-plan.md:7` 记录用户已确认的第一条规矩：

> 更新源就是 GitHub Releases；

且这是「施工时按原样执行，不要重新设计」的四条规矩之一。

### 5.2 标签与资产约定（计划书里已写下的）

`docs/plans/wallpaper-update-detection-plan.md:46-48`：

> - 标签格式：`v<四段版本>`，与 MSIX 版本一一对应，例如 `v0.2.0.203`；
> - release 至少带一个资产：`dsh-wallpaper-lockscreen-test.msix`（沿用现在 CI 的产物名）；证书 `.cer` 不必随 release 走——更新场景下证书早就被信任了；
> - 版本比较：解析标签去掉前缀 `v`，按四段数字比较；只接受 `^v\d+(\.\d+){0,3}$`，解析不出的 release 一律按"没有新版本"处理（宁可漏报，不可误报）。

同文件 `:50-62` 还有一条**必须先于形态决策解决**的约束（用户 2026-09-29 的决定）：

> 用户 2026-09-29 明确定过：**正式版的包先做占位**，第一版正式发布"可能是 NSIS"。所以这套功能**不允许把 MSIX 当成唯一形态**

以及按安装形态分派版本读取的三分支：MSIX 用 `Package.Current.Id.Version`；NSIS 读 `HKCU\...\Uninstall\<app>_is1` 的 `DisplayVersion`，读不到再退到主程序 exe 的字符串版本；两者都读不到就不检查（同文件 `:55-58`）。

**这条 2026-09-29 的决定与第 3 节的结论直接冲突**：如果形态定为纯 NSIS，锁屏接管就会永久失效。因此第 3 节的形态决策一旦落定，应当回过头更新这份计划书（把「先做占位」改成按已定形态收敛），否则两条文档会互相矛盾。

### 5.3 更新检查会读什么

- 端点：`GET https://api.github.com/repos/<owner>/<repo>/releases/latest`，匿名可用（每小时 60 次），**必须带 User-Agent**，否则 403（`wallpaper-update-detection-plan.md:40`）。
- 只有完整版具备该能力：`reqwest` 是 `full` 专属依赖（同文件 `:39`）。因此**更新链路天然属于完整版**，Lite 只给提示（同文件 `:112-114`）。
- 资产后缀白名单 `.msix` / `.exe`，不写死文件名（同文件 `:62`）。
- 已知失败模式：「机器上没信任签发证书时，App Installer 会拒绝」（同文件 `:109`）。走 Store 路线时这条风险自然消失。

### 5.4 CI 的现状与仍需改动的部分

**已经由 `.github/workflows/package-full.yml` 完成的（见 2.4.1）**：完整版的远端构建、临时清单版本替换、上传前版本/发布者/证书三方核对、以及往同一枚标签 Release 幂等补传 `.msix`。第 2.4.1 节列出的两个遗留问题（版本只与清单比对、`.cer` 不进 Release）分别记为 D14、D15。

**仍然需要改动的**：

1. 签名步骤：`package-full.yml:160-174` 目前是「每轮一次性的自签测试证书」。正式发布必须按第 4 节选定的方案替换——走 Store 则去掉 `-CreateTestCertificate`，走自购证书则改为指向真实证书/云 HSM。
2. 正式清单改名与 `Publisher` 定稿（D2、D3）：这两个值现在同时出现在 `package.yml`、`package-full.yml`、`package-full.yml:192-205` 的核对逻辑与 `windows_integration.rs` 的硬编码测试里，改一处必须同步全部。
3. `.github/workflows/package.yml:27` 的 `edition: [lite]` 与 `:24-26` 的注释（「must not be accidentally shipped with this release」）在形态定稿后需要重新表述，否则它会与新出现的完整版工作流在意图上互相矛盾——一个说「完整版不得随本次发布出去」，另一个说「标签上把完整版资产补进同一枚 Release」。
4. `releases/latest` 的解析正确性：现在两个工作流**故意**往同一枚 Release 里放资产（`package-full.yml:5-10`），而更新检查读的是 `releases/latest`（5.3）。因此必须保证：任一时刻的「最新 release」对应的标签就是完整版应当比较的版本。**Lite 若用 `v0.1.x` 标签发新 Release，会让完整版读到更低版本号而漏报更新。** 这一条要么靠「Lite 与完整版不同时发」的流程纪律，要么靠更新检查改为按资产名过滤。见第 8.4 节。

---

## 6. 首次发布前必须补齐的清单

每条给出「依据」与「完成意味着什么」。

### D1. 形态决策落地（阻塞项）

- 依据：第 3 节；`wallpaper/src-tauri/src/windows_integration.rs:2798-2806`。
- 完成意味着：`docs/plans/first-main-release-plan.md` 第 3.6 节被一份明确的施工任务取代，且 `docs/plans/wallpaper-update-detection-plan.md:52` 的「先做占位」表述被同步收敛。

### D2. 正式清单改名

- 依据：`packaging/msix/AppxManifest.xml:12` 当前为 `DSH Wallpaper (Lock Screen Test)`，`:13` 为 `DSH Wallpaper Test`。
- 完成意味着：完整版清单的 `DisplayName` / `PublisherDisplayName` 都不再含 `Test`；`Description`（`:14`，现为「Test package for DSH Wallpaper lock-screen integration.」）重写为正式描述。

### D3. 签名与 `Publisher` 定稿

- 依据：4.1–4.4；`packaging/msix/AppxManifest.xml:10`。
- 完成意味着：`Publisher` 与最终证书/Store 账号主体逐字符一致；`windows_integration.rs:4366,4376,4389` 里硬编码的旧族名已更新；旧族名→新族名的自启迁移已在真机回归。

### D4. 锁屏接管的 `Win+L` 人工验收

- 依据：`README.md:30`「已安装 MSIX 的 `Win+L` 人工验收待做」；`docs/plans/v0.3-product-rewrite-plan.md:80`「**尚未完成正式交付验收：**已安装 MSIX + `Win+L` 人工验收仍待执行。因此不能声称锁屏接管已经正式验收或可在所有 Windows 11 环境中交付。」
- 完成意味着：按 `docs/guides/lockscreen-msix-test.md:111-121` 的 9 条验收清单在**新签名的正式包**上逐条通过（注：该清单第 113 条目前要求从开始菜单启动「DSH Wallpaper (Lock Screen Test)」，改名后需同步更新文档）。

### D5. Lite 从未被安装或验证（本机）

- 依据：第 2.1 节命令输出——本机只有 `com.dsh.wallpaper 0.2.0.205`，没有 `com.dsh.wallpaper.lite`。`artifacts/` 下也没有 Lite 的安装或验收记录。
- 完成意味着：在一台**非开发机**并且在同一台机上**也**安装并跑通 Lite，或明确写下「Lite 与完整版共用原生核心，Lite 验收由完整版验收覆盖，差异面（前端入口、feature、权限清单）由 `scripts/verify-lite-bundle.ps1` 与 `ci.yml` 覆盖」这一替代论证并接受其风险。
- 注意：`docs/guides/lite-release.md:38` 已经写下「不要在开发机上安装或验收」，而本机恰恰是开发机，且 `com.dsh.wallpaper` 是本机开发态注册（`SignatureKind = Developer`）。也就是说**今天的「已安装」状态本身就不构成验收证据**。

### D6. 从未在干净机器上安装过

- 依据：`docs/guides/lite-release.md:38` 与 `:56` 描述了测试机的前置（Windows 11 x64、Windows SDK、WebView2 Runtime、`Microsoft.VCLibs.140.00.UWPDesktop`），但仓库里没有任何一次干净机安装的记录。清单第 30 行声明了对 `Microsoft.VCLibs.140.00.UWPDesktop` 的依赖（`packaging/msix/AppxManifest.xml:30`），该依赖**不随包携带**。
- 完成意味着：在一台从未装过本产品的 Windows 11 x64 上完成「装依赖 → 装包 → 首次启动 → 锁屏接管 → 恢复 → 卸载」，并留下记录；同时确认缺 `VCLibs` 时的失败模式有可读提示而不是静默失败。

### D7. README 与实现不一致

- **`Alt+W` 睡眠快捷键已不存在，但 README 仍在宣传。**
  - `README.md:70`：「`Alt+W`：进入睡眠模式」。
  - 全仓库对 `Alt+W` / `sleepHotkey` 的搜索只命中 **README.md:70、README.draft.md:204、README.draft.plain.md:69** 三处文档，**没有任何代码或配置**实现它。唯一涉及该功能的代码痕迹是一条注释，且用的是过去式：`wallpaper/tests/settingsStore.spec.ts:194`「`profile` carries the same short-string limit the deleted sleep-hotkey…」——即已被删除。
  - 同时 `README.md:140` 的验收表仍把 `sleepHotkey` 列为「既有配置保留」。
  - 完成意味着：`README.md` 删除或改写 `Alt+W` 与 `sleepHotkey` 的表述，并声明那个配置键的现状（保留字段、但功能已移除）。
- **README.draft.md 与 README.draft.plain.md 仍在仓库根目录。** 完成意味着：删除、或移入 `docs/` 并标注为草案，避免对外仓把它们当成当前说明。
- **`README.md:16`、`:67` 与 `:146-150` 的链接仍是旧路径。** `README.md:16` 写 `docs/lite-release.md`、`:67` 同、`:128` 写 `bridge/README.md`。实际文件位于 `docs/guides/lite-release.md`（`docs/README.md:59`）。完成意味着：修链接或按 `docs/README.md:67` 的约定「历史文件里的旧路径不要改写」明确例外范围。
- **`README.md:223` 声称 `scripts/` 是「素材生成与处理（17 个脚本）」。** 实测 `scripts/` 下共 **19** 个文件（其中 11 个 `.py` 素材脚本，其余为发布/探测脚本）。完成意味着：数字与职责描述都对上。

### D8. `dst` / `pn` 装载器：**无法在本仓库证实**

- 依据：对全仓库 `*.md` 搜索 `\bdst\b|\bnp\b` 无任何命中；`Get-ChildItem -Recurse -Directory` 中不存在名为 `dst`、`pn`、`shims` 的目录（除 `wallpaper/src-tauri/src/bin`，那是 Rust bin 目录，与装载器无关）；`package.json` 的 `scripts` 里也没有对应命令。
- 结论：**该项在本仓库中不存在**（不是「未验证」，是「找不到」）。完成意味着：用户澄清所指（可能是别的仓库、或已随某次重构移除），然后从本清单中删除或补入正确的依据。

### D9. 手工睡眠残留

- 依据（逐个实测）：
  - `README.md:70` 的 `Alt+W`：见 D7，已移除。
  - `README.md:71` 的 `Esc`：仍作为「睡眠中唤醒 / 关闭设置」被宣传；本轮未逐行核对实现，属**未核实项**，应从 README 的对外表述里核实后再保留。
  - 睡眠/苏醒素材存在两代并存：`wallpaper/public/personas/wake-frames/`（`frame-1-sleep.jpg`、`frame-2-eyes.jpg`、`frame-3-situp.jpg`、`frame-4-yawn.jpg`）为旧一套；`wake-frames/variant-anima/`（`sleep.png`、`frame-2-eyes.png`、`frame-3-yawn.webp`、`frame-4-awake.webp`）为实装一套。`wallpaper/src/persona/registry.ts:25-26` 用新一套的 `sleep`，但 `wake` 仍指向 `personas/wake.jpg`（旧图，文件仍存在）。
  - `wallpaper/public/personas/wake-frames/` 下另有 `variant-v4/`，共 **8** 个文件，其中 4 个是候选帧（`frame-4-yawn-cand1.jpg`、`cand2`、`cand3`、`frame-4-yawn-legs-v1.jpg`，合计约 7.3 MB），另 4 个（`frame-1-sleep.jpg`、`frame-2-eyes.jpg`、`frame-3-situp.jpg`、`frame-4-yawn.jpg`）是这一代的正式命名帧。
- 完成意味着：决定 `variant-v4/` 与 `wake-frames/*.jpg` 的去留；若保留为开发素材，移出 `wallpaper/public/`，因为该目录会被打包进前端产物。**已确认的正面事实**：候选帧目前**没有**进入 Lite 产物——`wallpaper/dist-lite/` 实测 15 个文件、21.2 MB，`personas/` 下只有四张立绘、三张深海背景和 `variant-anima` 的四个文件，无任何 `cand`/`legs`/`v4`。`scripts/verify-lite-bundle.ps1:24-27` 也把 `variant-anima` 的四个文件列为**必需**、`wake-frames/*.jpg` 列入禁止串。

### D10. 没有 LICENSE 文件：**已证实为真**

- 依据：仓库根目录只有 `.gitignore`、`package.json`、`pnpm-lock.yaml`、`pnpm-workspace.yaml`、`README.md`、`README.draft.md`、`README.draft.plain.md` 与一个 `_tmp_43272_…` 临时目录。`git log --all --diff-filter=A --name-only -- 'LICENSE*' 'COPYING*'` **无输出**，即从未在历史中新增过。
- 但同时 `package.json` 声明了 `"license": "MIT"`，`README.md:285` 也写「MIT（项目代码）；内置素材为 AI 生成或用户自备，用户自备素材版权归其所有」。
- 这是**声明与文件不一致**，对正式发布是硬伤（GitHub 的许可识别、下游使用者、以及 Store 提交时对第三方权利的要求都依赖它）。Store Policy 11.2 明确要求所有内容「either originally created by the application provider, appropriately licensed from the third-party rights holder, used as permitted by the rights holder, or used as otherwise permitted by law」。
- 完成意味着：新增 `LICENSE`（MIT 全文 + 版权人），并单独写明内置素材的授权范围（因为 README 已声明素材与代码不同源）。

### D11. 面向最终用户的崩溃与日志上报路径：**已证实不存在**

- 应用**有**本地日志：`wallpaper/src-tauri/src/lib.rs:4938-4942` 装了 `tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build()`。日志点非常密集（全仓库 `log::` 调用约 188 处）。
- 但**没有任何上报或崩溃收集**：`wallpaper/src-tauri/Cargo.toml` 中搜索 `sentry|crash|minidump|breakpad` **无命中**；`lib.rs` 中也没有 `panic::set_hook`（唯一的 `panic!` 命中在 `lib.rs:4239`，是测试断言）。
- `lib.rs:4935-4937` 的注释还说明了日志是**受容量约束**的：「obscuring real startup failures in the small rotating log」。
- 完成意味着：至少要有一条**用户可执行**的收集路径。最小可接受形态是设置中心一个「打开日志目录」按钮（Tauri log 插件默认写到应用日志目录），并在 README/发布说明中写明日志位置与如何反馈；是否接入崩溃上报（如 `sentry`/`minidump`）是可选的第二步，但必须显式决策，不能默认留空。

### D12. 锁屏接管恢复点的用户安全边界需在发布文档中写明

- 依据：`docs/guides/lite-release.md:22` 说明「清理过期恢复点会永久删除保存的原图副本，设置中心会在执行前再次确认；卸载前应先在设置中心关闭该开关」，且 `windows_integration.rs:2828-2844` 有一整套拒绝覆盖的保护逻辑（外部改图、备份缺失、备份不完整各自有专门错误文案）。
- 完成意味着：发布说明里有一条「卸载前先恢复原锁屏图片」的醒目提示，以及「Windows Spotlight 等动态来源会被拒绝接管」的限制说明（`docs/guides/lockscreen-msix-test.md:149`）。

### D13. 测试许可与义务自检

- 依据：Store Policy（[v7.20](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies)）10.5.1 明确「Product types that inherently have access to Personal Information must always have privacy policies. These include, but are not limited to, Desktop Bridge and Win32 products.」——本产品是 Desktop Bridge 形态的打包 Win32 应用，因此**上 Store 就必须有隐私政策 URL**。
- 同理 10.2.7 要求「clearly communicate and enable a user's ability to cleanly uninstall and remove your product from their device」。
- 完成意味着：若走 Store 路线，隐私政策已托管并有 URL；卸载路径已在文档与设置中心中可见。

### D14. 版本解析必须与「已发布/已安装的最高版本」比对

- 依据：`.github/workflows/package-full.yml:145-147` 只在算出的小于**仓库清单里的** `0.2.0.74` 时打一条 `Write-Warning`，不做任何拒绝；而同文件 `:17-19` 的注释自己承认 `0.2.0.205` 是「当前在用的」版本，本机实测的已安装版本也正是 `0.2.0.205`（第 2.1 节）。MSIX 不接受低于已安装版本的升级。
- 后果：一个 `v0.2.0.100` 这样的标签会顺利构建、顺利上传、顺利发布，但没有任何用户能装上它——而且 CI 是绿的。这属于「发布看起来成功、实际不可用」的静默失败。
- 完成意味着：把「已发布过的最高版本」变成发布流程里一个可查的事实（例如以已有 `v*` 标签的最大四段版本为准），并在算出的版本不高于它时让这一轮**失败**而不是警告。

### D15. 公开证书 `.cer` 不进 Release，14 天后无法配对

- 依据：`package-full.yml:236-242` 把 `.cer` 只上传为 Actions artifact（`retention-days: 14`），并在 `:262-263` 说明有意不上传到 Release（理由是 Lite 的资产里已有同名 `dsh-wallpaper-test.cer`，不碰它）。
- 影响：`wallpaper-update-detection-plan.md:47` 说「证书 `.cer` 不必随 release 走——更新场景下证书早就被信任了」，这对**升级**成立，但**全新安装的测试用户不成立**。首次安装必须同时拿到与本轮 MSIX 配对的公开证书（`docs/guides/lite-release.md:52` 的引导器逻辑要求「MSIX 的签名者、清单 Publisher 与内置 CER 完全匹配」）。
- 完成意味着：明确一个「首次安装怎么拿证书」的持久入口（改用受信任发行证书后此问题自然消失；若仍用自签测试包，则要么延长 artifact 保留期，要么给 `.cer` 一个不与 Lite 冲突的名字后放进 Release）。

---

## 7. 验收条件

发布本身通过的判据（每条对应可观测证据）：

1. 一个 `v<四段版本>` 标签触发的 Release 里，同时存在完整版与 Lite 的约定资产，且**完整版资产的版本号等于标签里的四段版本**（不是清单里的旧值）。`package-full.yml:192-205` 已经在上传前核对这一点，验收时抽查该轮的 CI 日志即可。
2. 完整版包在**一台从未安装过本产品的 Windows 11 x64** 上安装成功，且安装过程无 SmartScreen 阻断（走 Store 路线时）或出现的是**显示已验证发布者名**的警告（走自签/自购证书路线时）。
3. 全新机器上完成一次完整往返：启用锁屏接管 → `Win+L` 看到熟睡画面 → 解锁 → 恢复原锁屏 → `LockScreen::OriginalImageFile` 回读确认已恢复。依据 `docs/guides/lockscreen-msix-test.md:111-121`。
4. 在 Windows 设置里故意先换一张锁屏图，再点恢复：应用必须**停止并保留恢复点**，不得覆盖（同清单第 119 条）。
5. 开机自启在**升级后**仍然生效，且自启项指向的别名与当前包族名一致（回归 `migrate_legacy_autostart`）。
6. 卸载后不残留：无有效接管恢复点、无遗留自启项、`Get-AppxPackage` 不再列出该包。
7. `pnpm typecheck`、`pnpm test`、`cargo test`、`pnpm build`、`pnpm build:lite`、`scripts/verify-lite-bundle.ps1` 全绿（对齐 `.github/workflows/ci.yml:66-88`）。
8. 第 6 节 D1–D15 全部关闭，或有用户明确签字的「接受该风险」记录。
9. 发布说明包含：已知限制（锁屏只支持可读的本地静态原图、Spotlight 会被拒绝）、卸载前先恢复的提示、以及日志位置与反馈途径（D11）。
10. 用发布出来的资产实测一次**升级**：从任一较早版本升到本轮版本能成功安装，且自启项未断（对应第 5 条与 D14）。

---

## 8. 待决定

每条给出建议与理由。

### 8.1 完整版首发走 MSIX，还是接受失去锁屏接管的 NSIS？

**建议：MSIX。** 理由：锁屏接管在代码里是硬绑包身份的能力（3.1），NSIS 形态下它不是「弱化」而是「完全没有」，且代码明确拒绝执行；同时 MSIX 是唯一被本机验证过的形态，也顺带解决免费签名问题（Store 重新签名）。若选 NSIS，请同时确认愿意让完整版在核心叙事上弱于 Lite。

### 8.2 走 Microsoft Store，还是自购证书自行分发？

**建议：走 Store。** 理由：这是唯一「零证书费用 + 无 SmartScreen 警告 + 保住锁屏接管」三者同时成立的路径（4.1、4.3）。自购 OV（$150–300/年，且 2023-06 起私钥必须在硬件令牌/云 HSM 上）或 EV（$400+/年）都**仍会**遇到新文件的 SmartScreen 警告，EV 的溢价已不再换来即时信任。Azure Artifact Signing 更便宜，但**个人开发者仅限美国或加拿大**，若你在中国大陆则不适用。

需要你确认的连带项：Store 账号类型会是 Individual 还是 Company。若该应用与你的职业/业务相关，微软的措辞指向 Company（4.2），而 Individual→Company **不可转换**，选错要重开账号。

### 8.3 版本线：完整版首发落在哪个四段版本？

**建议：`v0.3.0.0`。** 理由：`docs/plans/v0.3-product-rewrite-plan.md:392-401` 用整节定义了「首版发布必须满足」的条件，即项目自己的计划就是把 v0.3 当作首个发布里程碑；而 Lite 已经占用了 `v0.1.0-lite*` 标签（2.6），完整版另起 `0.3` 线可以让两条线在 `releases/latest` 的竞争中语义清晰。

这条路与 `package-full.yml` 现有解析完全兼容：`v0.3.0.0` 会被 `:123-132` 的分支解析为四段版本 `0.3.0.0`，并高于已安装的 `0.2.0.205`。

但请一并决定两个**本文无法替你判断**的问题：

1. 有没有真实用户停在比首发版本更高的 `0.2.0.x` 上？（本机就是 `0.2.0.205`，见 2.1）MSIX 不接受降级，见 D14。
2. 一旦定了 `0.3.x`，`package-full.yml:133-142` 的「手动运行未填版本」分支仍会从 `Cargo.toml` 的 `0.2` 起算（`0.2.<运行号>.0`），产生 `0.2.x` 与 `0.3.x` 交叉的版本号。要决定是同步抬高 `Cargo.toml`/`tauri.conf.json` 的版本到 `0.3`，还是禁用该分支。

### 8.4 Lite 与完整版共用一个 Release：现在这是既定设计，需要决定怎么保住 `releases/latest`

现状已经变了：`package-full.yml:5-10` 明确写「本工作流只构建完整版 MSIX，标签上把资产**补**进同一枚 Release」，并做了幂等处理，所以「分开两个 Release」已不是默认选项。剩下的是**必须解决**的正确性问题：

- 更新检查读 `releases/latest`（5.3），它只返回最新一枚 Release。
- Lite 的标签线是 `v0.1.x`，完整版是 `v0.2.x`/`v0.3.x`。谁后发，谁就成为 `latest`。
- 于是「Lite 发完新版本后，完整版读到 `0.1.x`，判定没有新版本」是必然发生的，不是概率问题。

**建议：让更新检查不依赖 `releases/latest`，改为列出 releases 后按「标签可解析 + 存在匹配本形态的资产」筛选取最大版本。** 理由：这是唯一不依赖人工发布纪律的做法，而且 `wallpaper-update-detection-plan.md:62` 已经定了「用后缀白名单判断可安装资产，不要写死文件名」的原则，按资产筛选与它一致。代价是要修改那份计划书里「读 `releases/latest`」的设计（`:40`）。

**次选：约定两条线永不在同一时间段发 Release，且每次只推一个标签。** 这不需要改代码，但把一个正确性问题变成了流程纪律问题，一旦破例就会静默漏报更新。

### 8.5 是否要在首发前验证形态 C（稀疏包）？

**建议：不在首发关键路径上，但值得单独排一个技术验证任务。** 理由：它能把「NSIS 安装器」与「锁屏接管」两个原本互斥的诉求同时满足（3.5），这对分发体验有明显价值；但该路线在本仓库零验证，且注册稀疏包通常需要管理员权限，会改变当前 `installMode: currentUser` 的无提权安装体验（`tauri.conf.json:27`）。

### 8.6 现在还缺一次性证实的项（见第 9 节），要不要先补？

**建议：补 D8（`dst`/`pn` 装载器）与 D9 的 `Esc` 一项。** 理由：这两项是清单里唯一「我无法从仓库证实或推翻」的条目。在它们有结论之前，第 6 节的清单不能被当作完整。

### 8.7 自签测试包要不要继续发布？

**建议：在拿到受信任证书之前继续，但必须同时解决 D15（证书配对的持久入口）。** 理由：现在的自签包是唯一能真机验证锁屏接管的载体（`SignatureKind = Developer` 的本机安装不算验收，见 D5）。但「`.cer` 只在 Actions artifact 里保留 14 天」意味着这条验证链路有时效性，`package-full.yml:174` 的下载入口会在两周后变成不可用。

---

## 9. 未能建立结论的事项

如实列出，避免把不确定当成已确定。

1. **微软文档没有正面写明 `TrySetLockScreenImageAsync` 需要包身份。** 官方 API 参考页的「Windows requirements」只列 Device family 与 API contract，**没有**「需打包」字样（[UserProfilePersonalizationSettings](https://learn.microsoft.com/en-us/uwp/api/windows.system.userprofile.userprofilepersonalizationsettings)、[TrySetLockScreenImageAsync](https://learn.microsoft.com/en-us/uwp/api/windows.system.userprofile.userprofilepersonalizationsettings.trysetlockscreenimageasync)，置信度：高——即「文档确实没写」这一点是确定的）。官方示例使用 `ms-appx:///Local/…` 这种**仅打包态可用**的 URI 方案，是间接信号但不是明文要求。
   - 因此第 3.1 节的结论**主要建立在仓库自身的代码与工程判断上**（守卫、测试、三处文档一致），外部文档只能提供间接支持，**不能提供正面证实**。
   - 反过来说：这也是形态 C 值得验证的原因——如果稀疏包身份能通过，说明真正的门槛是「包身份」而不是「完整 MSIX 部署」，与代码的 `has_package_identity` 判断一致。
2. **`PersonalizationCSP` 路线未采用，且证据强度低。** 唯一的来源是一条微软 Q&A 回答（[lockscreen customized image push via GPO](https://learn.microsoft.com/en-us/answers/questions/2279936/lockscreen-cusotomized-image-push-via-gpo-for-wind)），它描述的是**组策略/注册表推送**：写 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\PersonalizationCSP` 下的 `LockScreenImagePath`/`LockScreenImageStatus`/`LockScreenImageUrl`。该回答称标准方法「Works Only for Enterprise/Education」，并把注册表路线列为 Windows Pro 的替代方案。**这不是应用级 API，而是管理员策略机制**；写 `HKLM` 需要提权，与当前 `installMode: currentUser`（`tauri.conf.json:27`）冲突；且有「some Windows Pro versions may reset the lock screen after updates」的警告。
   - 置信度：**低**（社区回答而非官方规范；未在微软正式功能文档中找到该键的当前支持状态）。
   - 更重要的是：**即便它可用，也不该用作产品的默认路径**——Store Policy 10.2.8 要求「You are required to use supported methods and must obtain user consent to change any user's Windows settings, preferences, settings UI, or modify the user's Windows experience in any way. Unsupported methods include but are not limited to use of accessibility APIs or undocumented or unsupported APIs in unsupported ways.」用未文档化的注册表策略去改用户锁屏，正是这条政策要防的模式。
3. **个人开发者在中国大陆购买 OV/EV 证书的可行性与价格，未证实。** 微软页面只给全球区间 `$150–300/年`（OV）与 `$400+/年`（EV），并说明 CA 会做主体验证；本文没有从任何 CA 官方页面确认「中国大陆个人主体可签发」的明确承诺，`$` 区间也未含中国区渠道报价。
4. **形态 C（稀疏包）能否让锁屏 API 通过，纯属推断。** 依据是「该机制提供包身份」+「代码按包身份判断」两条推理，没有任何实测（3.5）。
5. **`dst` / `pn` 装载器在本仓库中不存在**（D8）。不是「没验证」，是「搜不到」。可能指别的仓库或已被移除。
6. **`README.md:71` 的 `Esc` 行为未逐行核实。** 本轮只确认了 `Alt+W` 已移除（D7），没有对 `Esc` 的实现做同样的逐行核对。
7. **完整版从未在干净机器上安装过**——这一点不是「未证实」而是「没有记录」，即 D6 描述的状态本身就是结论。
8. **`0.2.0.205` 这个已安装版本对应的发布日志缺失。** `artifacts/` 里的 `publish-*.log` 只到 `0.2.0.111`（实测 `Get-ChildItem artifacts -Filter 'publish-0.2.0.2*.log'` 无输出），说明 `.205` 那一轮的来源除了命令输出之外没有留下记录；本文无法重建它的构建时间与签名证书指纹。

---

## 附：本次调研未做的事

按任务边界，本文件撰写期间**没有**构建、没有安装、没有提交、没有运行打包脚本、没有启动或留下任何进程。所有命令都是只读查询（`git log`/`git tag`/`Get-AppxPackage`/`Get-ChildItem`/`Get-AuthenticodeSignature`/`Select-String`）。

撰写期间工作区中出现了**另一个 agent 的新增文件** `.github/workflows/package-full.yml`（`git status --short` 中为未跟踪）。本文只读取它并在第 2.4.1 节据实描述，没有改动它。除本文件自身外，未创建或修改任何文件。
