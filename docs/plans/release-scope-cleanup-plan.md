# 发布范围收敛：锁屏退出、素材清理、自动装桥

> 2026-09-30 用户拍板。这三件事都会改变"发出去的包里面有什么"，所以**发布排它们之后**。
> 每件都按"先出清单、再动手、做完给证据"的节奏走。

## 一、不再触碰锁屏（用户：A 彻底不碰）

> 2026-10-10 补充：目标不变，**执行方式与验收以 `code-structure-refactor-plan.md` 第一节为准**——锁屏源码与测试保留到 `archive/`，只退出默认构建，不再按关键字零命中删除源码。下面的“谁在碰”清单仍可作为迁移清单的起点；验收中的关键字零命中一条由那份计划的隔离证据替代。

**理由（用户原话的意思）**：壁纸现在做的和用户手动去 Windows 设置里换锁屏**没有区别**——都是替换掉一个槽位；
但它多了风险，而且**不能回滚**。既然收益等于零、风险大于零，就整体退出。

**谁在碰（2026-09-30 实测的清单）**

| 位置 | 现在做什么 |
| --- | --- |
| `wallpaper/src-tauri/src/lock_screen_backup.rs` | 备份/恢复用户的锁屏设置（`%APPDATA%\com.dsh.wallpaper\lock-screen*`、`lock-screen-backup.json`） |
| `wallpaper/src-tauri/src/windows_integration.rs` | 写入锁屏相关系统状态 |
| `wallpaper/src-tauri/src/desktop_repair.rs`、`desktop_fallback.rs`、`native_bootstrap.rs` | 启动与修复路径里的锁屏动作 |
| `wallpaper/src-tauri/src/appearance/{repository,types}.rs` | 外观素材里的锁屏槽位 |
| `wallpaper/src-tauri/src/bin/lockscreen_probe.rs` + `scripts/build-lockscreen-probe.ps1` | 独立探针程序（取证用） |
| `packaging/msix/AppxManifest{,-Lite}.xml`、`LockScreenProbe.AppxManifest.xml`、`scripts/build-msix-test.ps1`、`scripts/publish-local-msix.ps1` | 打包与发布链里的锁屏部分；测试包名是 `dsh-wallpaper-lockscreen-test.msix` |

**验收标准**

* `grep -ri lockscreen wallpaper/src-tauri/src packaging scripts` 只剩与本次退出相关的历史说明（或零命中）；
* 装机后**不产生** `%APPDATA%\com.dsh.wallpaper\lock-screen*`；
* 用户的锁屏设置在任何操作前后**逐字段不变**（可读注册表/`IDesktopWallpaper` 前后对比取证）；
* MSIX 里不再携带锁屏素材；测试包名不再叫 lockscreen（改名会牵动 `publish-local-msix.ps1` 的版本守卫，单独一步做）；
* 全套门禁照旧（typecheck / vitest / cargo / 构建 / lite 目标）。

**顺序**：先出"要删的文件与代码块"清单 → 用户确认 → 动手。**不删 `appearance` 的素材槽位**这类与锁屏无关的部分，只切掉锁屏那一条。

## 二、删掉没用到的素材（用户：让目录里只剩看得见的东西）

**动机**：用户打开目录看项目结构时，不该被"从来没用过的东西"搞晕。
**可见的应该只有：四张立绘 + 四张帧动画**（用户 2026-09-30）。目标是**当前树清爽**（不是仓库体积——删文件不会缩小历史，改历史要另作评估）。

**现状（已跟踪体量）**：`assets/` 44 个文件 81.7 MB、`wallpaper/public/` 26 个文件 44.3 MB、`docs/media/` 2 个文件 5.5 MB。
`wallpaper/dist-lite/` 与 `packaging/msix/Assets` 已被 .gitignore，不在仓库里，无需处理。

**判据**：素材引用集中在官方目录表白名单（`officialCatalog.ts`、`official.rs`，经 `persona.ts`/`registry.ts`/`types.ts`/`WakeScene.tsx` 读取）。
"没用到" = 不在白名单、也不被文档与脚本引用。

**验收标准**

* 输出一张表：每个**候选删除**文件的路径、体积、最后修改时间，以及"为什么判定没用到"（引用检索零命中 / 被 `abolished` 之类命名标记为废弃）；
* 用户划掉要留的之后才删；
* 删完：应用构建、lite 构建、cargo 测试、前端测试全绿；跑一次应用确认四张立绘与四张帧动画都**仍能正常显示**（这是这次唯一真正的验收点）；
* 若某文件是"变体/备用"而非废弃，默认**保留并标注**，不自作主张删。

## 三、扫描并选中 harness 后自动装桥（用户：按我提的三条政策）

**现状（2026-09-30 实测）**：壁纸**从来不装桥**——整个 `wallpaper/src`、`src-tauri/src`、`packaging`、`scripts` 里没有任何安装动作，只有三处报错文案让用户自己去装（`chat.rs:2487/2547/2612`）。
所以新用户装完壁纸切到 harness 会发现聊不了。

**落点**：扫描在 `SettingsWindow.tsx:275`（`scanHarnessTargets`），选中主体在 `:936`（`onSelectSubject`）；原生侧新增一个"确保这个主体的档案里有桥"的命令。

**政策（用户已定）**

1. **装进哪些档案**：CLI/源码树主体 ⇒ 设置里的 `profile`（默认 `web`）；**壳主体 ⇒ 两个都要装**——
   客户端在跑时壁纸连的是它的 `desktop` 档案宿主，客户端不在跑时才用我们起的 `web` 档案宿主。
   少装哪一份，就会在对应的那半段路上"灯亮着但建不了会话"。
2. **触发时机**：**每次启动都检查，缺桥就补**（用户选"后者"）。仍然遵守一条底线：
   只在"确实缺"或"版本对不上"时才动包管理，而且是**可观察**的——状态栏里给结果，不静默。
3. **失败呈现**：把 `dsh plugin` / `pnpm` 的 stderr 原样收成一条可读状态（版本豁免警告、CLI 不在 PATH、
   主体没在跑、档案不可写……各自说清楚），**绝不假装成功**。

**要用哪个 CLI 装**（2026-09-30 实测的坑）：全局 CLI **拒绝 `desktop` 档案**
（`profile "desktop" is managed exclusively by the Electron application`）；官壳自带的 CLI 才有
`manageDesktopProfile`。所以：

* `web` / `dsh-tui` 等 ⇒ 全局 CLI 或该主体的 CLI；
* `desktop` ⇒ **只能用官壳自带的 CLI**：`<壳安装目录>\resources\runtime\cli\bin\dsh.cmd plugin --profile desktop add …`。

**版本**：钉 `dsh-wallpaper-bridge@0.1.5`（与三个档案现状一致；换版本时这里要跟着改，
并且与"壁纸校验宿主回报的 build"那件事共用同一个常量）。

**验收标准**

* 在三个档案里各删掉桥（备份后）→ 选一次主体 → 桥按政策装回、版本正确；
* 壳主体：故意只留 `web` 的桥，确认 `desktop` 那份会被补上；
* 断开网络、或让 `dsh` 不在 PATH：状态栏出现**可读**的失败原因，且**不**留下半个安装；
* 不产生"每次启动都跑一遍包管理"的噪音（有桥就不动，日志里只有一次判定）。
