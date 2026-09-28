# 构建与发布的耗时：实测、改动与可复用的方法

一份留档：本地发布脚本与远端 CI 的耗时**实测数据**、改动前后的对照、以及下次遇到"构建慢"时该怎么做。
不写猜测：本文所有数字都来自本机实测，未实测的部分明确标注为"未验证"。

相关提交：`82cbdfc`（本地发布并行）、`661a710`（CI 依赖缓存）。

---

## 结论速览

| 目标 | 优化前 | 优化后 | 手段 |
|---|---|---|---|
| 本地发布一次（`publish-local-msix.ps1`，默认跑全套检查） | **103.5s** | **65.5s** | 检查改为与 release 构建**并行** |
| 远端 CI 的 Rust 依赖编译（`ci.yml` / `package.yml`） | 每个 lock 版本只可能命中一次快照；那一次失败则**长期零缓存** | 依赖常驻，失败也存 | 手写 `actions/cache` → `Swatinem/rust-cache` |
| 打包任务（`package.yml`） | release 编译**两遍** | 未改（已确认，见下文） | 待做：两条构建共用一个 target 目录 |

---

## 一、怎么量（可复用）

本地发布脚本在每个阶段开始**前**打印 `==> 阶段名`，所以逐行打时间戳就能分解：

```powershell
$sw = [System.Diagnostics.Stopwatch]::StartNew()
pwsh -NoProfile -File .\scripts\publish-local-msix.ps1 -PackageVersion 0.2.0.N 2>&1 |
  ForEach-Object { "{0,7:N1}s  {1}" -f $sw.Elapsed.TotalSeconds, $_ } |
  Tee-Object -FilePath "$env:TEMP\publish.log"
```

**归属规则（我第一次就读错的地方）**：打点在校验点之前，所以时间戳 `T(n)` 与 `T(n-1)` 之间的区间属于
**第 n-1 个阶段**，不是第 n 个。按错的算法读，会把"构建"的 55s 归给"签名"。

单项对照实验：把可疑步骤单独拿出来量一次，例如怀疑签名慢：

```powershell
Copy-Item artifacts\msix-test\dsh-wallpaper-lockscreen-test.msix $env:TEMP\probe.msix
Measure-Command { & $signtool sign /fd SHA256 /sha1 <CER 里的指纹> /s My $env:TEMP\probe.msix }
```

---

## 二、本地发布（`scripts/publish-local-msix.ps1`）

### 优化前：103.5s，两半严格串行

| 阶段 | 耗时 | 说明 |
|---|---|---|
| 启动（版本、证书、回滚点解析） | 1.3s | |
| TypeScript 类型检查 | 4.9s | |
| 前端 + Bridge 测试 | 11.1s | |
| Rust 测试（`--all-targets`，debug） | 27.5s | 调试 target 目录是暖的，所以主要是链接与运行 |
| **构建 full Release 布局** | **55.3s** | 其中 **crate 自身的 release 编译 48.0s**、MakeAppx 约 1s、资源拷贝约 5s |
| 签名（signtool） | **0.4s** | 同一份包再签一次：0.3s |
| 信任链验证 + 安装 + 启动 | 3.0s | |

关键事实：release profile 只设了 `strip`（没有 LTO、没有 `codegen-units = 1`），依赖已全部缓存在隔离
target 目录里（`artifacts/msix-test/cargo-target`，约 3.3 GB），所以那 48s 就是**这个上万行 Tauri
crate 自身的代码生成**——它随每次改动而重编，是本地"每次都要等"的真正来源。

### 两次判断错误（留在这里提醒）

1. 先入为主地认为"签名一个 65 MB 的包一定很慢" ✗。实测 0.4s。
2. 按错区间归属读分段表，把构建时间算成了签名时间 ✗。改对后才发现大头是 crate 编译。

**教训**：不要按"看起来最重的那一步"下判断，也不要相信一张没有核对归属的表。

### 优化方式：检查与构建并行

检查用**默认** target 目录 + node；构建用**隔离**的 `artifacts/msix-test/cargo-target`——两者不共享资源，
因此可以同时进行。改动把三项（Lite 版为五项）检查放进后台作业，构建照常进行，**在安装之前**结算检查结果：

- 失败即中止，不会安装任何东西；
- 日志里仍能看到每个阶段的完整输出（暂存在作业结果里，构建结束后一次性打印）。

**代价（写在改动处，也写在这里）**：检查**失败**时，构建已经跑完才发现（改前是构建前就停），失败那次多花
构建那几十秒；检查**通过**时——日常的绝大多数情况——省掉整段检查时间。

### 优化后：65.5s

| 指标 | 值 |
|---|---|
| 总时长 | **65.5s**（-37%） |
| 并行检查的结算点 | 62.1s（三项检查全部完成） |
| 检查是否少跑 | **没有**：Rust 211 项、前端、bridge 全部执行并通过 |
| 新的关键路径 | 构建那 55s（48s 是 crate 编译） |

### 还没做的（未验证，不要当成已完成）

1. **release 增量编译**（`CARGO_INCREMENTAL=1`）：理论收益是"少量改动时重建 48s → 20-30s"，代价是 target
   目录膨胀；只在源文件少量改动时有效，**尚未测量**。
2. **开发注册（loose registration）**：可省掉 MakeAppx + 签名 + 安装，且改完代码重建即生效、无需重新注册。
   前提是系统开启 Developer Mode，本机 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock\
   AllowDevelopmentWithoutDevLicense` **不存在**（= 关闭）⇒ 需要用户明确同意才能开。

---

## 三、远端 CI（`.github/workflows/`）

### 优化前的问题（`ci.yml` 与 `package.yml` 都是同一个写法）

```yaml
key: ${{ runner.os }}-cargo-${{ hashFiles('.../Cargo.lock') }}
```

- key 只由 lock 文件决定，而 **`actions/cache` 对已存在的 key 永不重写** ⇒ 每个 lock 版本只会在
  **第一次成功**的那一轮存一次；那一轮失败或被取消，之后每一轮都从零编译，直到 lock 再变。
- 缓存内容还包含**本仓库自己的产物**与增量产物：恢复与上传都更慢，而自己的 crate 每轮都变、缓存它没有收益。

### 优化方式

- 两个工作流都改用 `Swatinem/rust-cache@v2`：只缓存**依赖**（key 含 lock 哈希，lock 不变时命中正是对的），
  `cache-on-failure: true` 让失败的一轮也能存，保存前清理增量产物与一周前的旧产物。
- `package.yml` 里 MSIX 辅助脚本用的**另一个** target 目录（`artifacts/msix-test/cargo-target`）该 action
  无法知晓，保留手写缓存，但 key 加 `github.run_id`（这样每轮都能存）、`restore-keys` 用 lock 哈希前缀
  （这样下轮能命中）。

### 效果

**未实测**（本文写作时尚未跑过新 CI）。下一轮 CI 的耗时是对它的唯一证明。

### 已确认但未改的一处浪费

`package.yml` 把 release 编译做了两遍：`pnpm desktop:build:lite` 编到
`wallpaper/src-tauri/target/release`，而 `build-msix-test.ps1` 用 `--target x86_64-pc-windows-msvc` 编到
`artifacts/msix-test/cargo-target`。两者编的是同一个 lite 二进制。让它们共用 target 目录（给 Tauri 也传
`--target`，工作流里已安装该 target）可省掉一整轮 release 编译（Windows 上通常 5-8 分钟），代价是 NSIS
产物路径会移动，而收集产物那一步正按旧路径找——属于"必须本地真跑一遍确认路径"的改动。

---

## 四、测量过程中发现的两个坑（与耗时无关）

1. **回滚包与随手构建共用同一个固定路径**。量分段时直接跑了一次 `build-msix-test.ps1`（不带版本清单），
   它把 `artifacts/msix-test/dsh-wallpaper-lockscreen-test.msix` 覆盖成了清单里的 `0.2.0.74`，而当时装的是
   `0.2.0.199`。发布脚本在 1.5s 就停止并拒绝继续（"为避免丢失回滚点而停止"——它做得对）。建议加固：
   回滚包按版本命名（`dsh-wallpaper-<版本>.msix`），固定路径只当草稿。
2. **测试证书在 `CurrentUser\My` 里攒了 12 张同名 `CN=DSH Wallpaper Test`**（历次 `-CreateTestCertificate`
   的产物）。修复上面那条时我按"第一个匹配"签错了证书，被脚本的"签名者必须等于项目 CER 指纹"拦下。
   正确判据是**项目 CER 里的指纹**，不是主题名。

---

## 五、下次"构建慢"的检查清单

1. **逐阶段打点**，别整体计时；注意打点位置与区间归属。
2. **拿单项做对照实验**（同一份产物再签一次、同一命令再跑一次），区分"这项工作贵"与"首次读盘/杀软扫描贵"。
3. **缓存三问**：key 由什么决定？命中之后还会不会重存？失败的那一轮存不存？
4. **不要因为"检查慢"就默认跳过检查**：先量它占多少。这次检查只占 43.5s/103.5s，而且与构建完全可并行。
5. **未经测量的优化不写进代码**（增量编译这条就没写），但要把候选、前提与风险记下来。
