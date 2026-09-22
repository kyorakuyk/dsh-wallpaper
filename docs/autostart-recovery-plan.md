# 开机自启缺口 — 修复方案

> 来源：2026-09-22 现场排查。现象为「开机不自启」，实测确认**两条自启路径均未注册，且代码无自愈路径**。
> 本文档给实施方（Luna）使用。作者（砚）未改动任何代码。

---

## 一、现象

设置中心「开机自启」显示**未开启**；开机后 DSH Wallpaper 不启动。此问题**反复出现**（用户描述为"又不自启了"），说明非偶发。

## 二、本机已核实的事实

排查机：Windows 11 家庭版 build 26200，双显示器 4480×1600。

| 检查项 | 结果 |
|---|---|
| 已安装包 | `com.dsh.wallpaper_0.2.0.71_x64__pdxj8y3r6rm5g`，Status=Ok，`SignatureKind=Developer`，安装于 2026-09-17 12:09:49 |
| 已安装 manifest 中的 StartupTask 声明 | ✅ 存在（`contains 'StartupTask': True`） |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `dsh-wallpaper` | ❌ **不存在** |
| `HKCU\…\AppModel\SystemAppData\com.dsh.wallpaper_pdxj8y3r6rm5g\DshWallpaperStartup` | ❌ **不存在** |
| `HKLM\…\AppModel\SystemAppData\…\DshWallpaperStartup` | ❌ 不存在 |
| 策略 `EnableFullTrustStartupTasks` / `EnableUwpStartupTasks` | ✅ 均为 2 |
| 策略 `SupportFullTrustStartupTasks` / `SupportUwpStartupTasks` | ✅ 均为 1 |
| 任务计划 / 启动文件夹 | 无 dsh 相关项 |
| 对照组：同机其他包的 StartupTask 键 | Intel Arc Software → `IntelGraphicsSoftwareStartup`；Intel Graphics → `GCPStartupId`；OfficeHub → `WebViewHostStartupId`；Todos → `ToDoStartupId`。**机制在本机可用，且这些应用都有注册键** |
| 安全软件 | 火绒运行中（`HipsDaemon` / `HipsTray` / `wsctrlsvc`） |
| `startup-diagnostic.log` | 位于 `%LOCALAPPDATA%\Packages\com.dsh.wallpaper_pdxj8y3r6rm5g\LocalCache\Local\DSHWallpaper\`，仅 7 行，最后写入 2026-09-18 02:00:00 |

**关键推论**：日志位于 MSIX 包容器内 → 近期运行的一直是**有包身份**的 MSIX 版，因此自启必然走 `has_package_identity() == true` 分支。

## 三、根因

### 代码链

1. `wallpaper/src-tauri/src/lib.rs:1043-1049`：

```rust
if windows_integration::set_startup_task(enabled)?.is_some() {
    let _ = windows_integration::remove_legacy_run_entry();  // ← 主动删除 Run 键
    return windows_integration::autostart_status();
}
```

   一旦在 MSIX 环境下成功启用过 StartupTask，代码**主动删除 Run 键**，此后自启 100% 依赖 StartupTask。

2. `packaging/msix/AppxManifest.xml:26` 中 `Enabled="false"` —— MSIX StartupTask **默认关闭**，必须由应用运行时经 `RequestEnableAsync()` 启用。状态可被外部重置（包重装/更新、系统或用户禁用、安全软件干预）。

3. **重置后没有任何代码会自动重新启用**：

   - `windows_integration.rs:2362-2375` `migrate_legacy_autostart()` **只在 `status.source == "run"` 时才动作**。Run 键已被删除 → 该条件永不成立 → 函数直接 `return Ok(Some(status))`，什么都不做。
   - `windows_integration.rs:2333-2339` `autostart_status()` 的 `Disabled` 分支也**只在 Run 键存在时**才返回 `enabled: true`。

**结论**：「Run 键已删 + StartupTask 被重置」= 永久失去自启，且无自愈路径。这是设计缺口，会周期性复发。

代码注释（`windows_integration.rs:2328-2332`）已承认相邻风险（"新包看起来静默关掉了自启"），但只防住了 Run → StartupTask 的迁移，未覆盖本组合。

### 附带脆弱点：读写不对称

- **写** Run 键：`lib.rs:1054-1077`，走 `Command::new("reg")` 调外部 `reg.exe`。
- **读** Run 键：`windows_integration.rs:2253`，原生 `RegQueryValueExW`。
- **删** Run 键：`windows_integration.rs:2297`，原生 `RegDeleteValueW`。

写路径依赖外部进程能被创建。火绒 HIPS 对「程序 → 拉起 reg.exe → 写 HKCU Run」是典型拦截对象（本次排查环境中 `reg.exe` 亦被安全策略拦截，改用 PowerShell 读注册表完成取证）。

---

## 四、修复方案

### 必做 1 — Run 键写入改用原生 API

**目标**：去掉对 `reg.exe` 的依赖，使读 / 写 / 删三条路径对称，同时消除 HIPS 通过进程创建链识别可疑行为的可能。

**改动**：
- 在 `windows_integration.rs` 新增 `pub(crate) fn set_legacy_run_entry(enabled: bool) -> Result<(), String>`；`enabled=false` 时直接复用现有 `remove_legacy_run_entry()`。
- 用 `RegCreateKeyExW` + `RegSetValueExW` 写入，类型 `REG_SZ`，名字 `dsh-wallpaper`。
- 值必须为 **UTF-16 且以 NUL 结尾**：`exe.as_os_str().encode_wide().collect::<Vec<u16>>()` 后 `push(0)`；`cbData` 按字节计（`len * 2`）。
- `lib.rs:1050-1080` 整段 `Command::new("reg")` 替换为对该函数的调用。
- 顺带移除 `use std::process::Command;` 与 `creation_flags(0x08000000)` —— 不再需要隐藏控制台窗口。

**所需 crate feature**：`Win32_System_Registry`（`Cargo.toml:75` 已启用）。需补充的 import：`RegCreateKeyExW`、`RegSetValueExW`、`REG_SZ`、`REG_OPTION_NON_VOLATILE`、`REG_CREATE_KEY_DISPOSITION`。

**错误处理**：保持现有中文错误文案风格；返回 `Result<(), String>`，失败时给出 Windows 错误码，便于用户截图反馈。

> 示意骨架（实施方按项目现有风格调整）：
> ```rust
> #[cfg(windows)]
> pub(crate) fn set_legacy_run_entry(enabled: bool) -> Result<(), String> {
>     if !enabled {
>         return remove_legacy_run_entry();
>     }
>     let exe = std::env::current_exe().map_err(|e| format!("无法获取可执行文件路径：{e}"))?;
>     let mut value: Vec<u16> = exe.as_os_str().encode_wide().collect();
>     value.push(0);
>     let mut key = HKEY::default();
>     let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
>     let status = unsafe {
>         RegCreateKeyExW(
>             HKEY_CURRENT_USER,
>             w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
>             None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE,
>             None, &mut key, &mut disposition,
>         )
>     };
>     if status != ERROR_SUCCESS {
>         return Err(format!("无法打开当前用户开机启动项（错误码 {}）。", status.0));
>     }
>     let bytes = core::slice::from_raw_parts(
>         value.as_ptr() as *const u8,
>         value.len() * core::mem::size_of::<u16>(),
>     );
>     let set_status = unsafe {
>         RegSetValueExW(key, w!("dsh-wallpaper"), None, REG_SZ, Some(bytes))
>     };
>     let _ = unsafe { RegCloseKey(key) };
>     if set_status != ERROR_SUCCESS {
>         return Err(format!("无法写入 DSH Wallpaper 开机启动项（错误码 {}）。", set_status.0));
>     }
>     Ok(())
> }
> ```

---

### 必做 2 — 增加自愈。两个方案，择一

#### 方案 2a（推荐，语义正确）：持久化用户意图 + 启动自愈

**前提问题**：当前**没有任何地方持久化"用户想要开机自启"这个意图**。设置中心显示的 `enabled` 直接来自 `autostart_status()`（系统实际状态）。因此代码无法区分「用户从未开启」与「开启过但被重置」——这是自愈无法实现的根本障碍。

**改动**：

1. **新增意图标记**。最省事的做法是在原生层直接读写，前端零改动：
   注册表 `HKCU\Software\DSHWallpaper` → `AutostartPreferred`（`REG_DWORD`，1/0）。
   写入时机：`set_autostart_blocking` 每次被用户显式调用时（无论是开还是关）都写一次。
   > 若项目已有设置持久化机制（前端 settings 或 Rust 侧配置），优先复用它，避免新增存储位置。**此项请实施方先确认现有方案再定。**

2. **扩展 `migrate_legacy_autostart()`**（`windows_integration.rs:2362`），在现有 `source == "run"` 分支之外增加：

   ```
   若 有包身份 且 AutostartPreferred == 1：
       读 StartupTask 状态
       若 state == Disabled                        → 尝试 set_startup_task(true)
                                                    → 成功/失败均写日志，不阻断启动
       若 state == DisabledByUser                  → 不动（尊重用户的明确拒绝）
       若 state == DisabledByPolicy                → 不动，并记录
   ```

3. **同时保留一条兜底**：若 `StartupTask::GetAsync` 返回 `Err`（`windows_integration.rs:2194` 的 `return Ok(None)` 分支）而导致退回 Run 键路径，此时属于"StartupTask 不可用"，**不应删除已存在的 Run 键**（现有逻辑在 `Some(..)` 时才删，此路径不触发，保持即可）。

**边界（务必遵守）**：`DisabledByUser` **绝不可自动改写**——那是用户的明确拒绝，自动启用属于违背用户意愿。只对 `Disabled`（从未启用 / 被重置）做自愈。

#### 方案 2b（最小改动，接受一点冗余）：不再删除 Run 键

保留双路径。代价是每次登录可能触发两次启动尝试，由单实例守卫挡下第二次。

> `lib.rs:1044-1046` 的注释说明了删除 Run 键的动机："so the single-instance guard does not needlessly process a second launch attempt"。即作者**有意**删除以避免多余的一次启动尝试。若采用 2b，需接受这点开销（对常驻壁纸进程而言是毫秒级），并确认单实例守卫不会因此产生用户可见的副作用。
>
> 注意：本项目已 vendor 并 patch 了 `tauri-plugin-single-instance` 修复上游竞态（`Cargo.toml:91-96`），采用 2b 前建议针对该场景补一条测试。

**推荐 2a**：它保留了原设计的意图（单一权威路径），只是把缺失的恢复能力补上。

---

### 建议 3 — 设置中心区分 `source` 并给出指引

`AutostartStatus.source` 已经能区分 `startup-task` / `disabled-by-user` / `disabled-by-policy` / `run` / `none`，但 UI 只呈现了布尔值。

建议至少对 `disabled-by-user` 显示明确的指引文案（例如"Windows 已禁用本应用的启动任务，请在「设置 → 应用 → 启动」中手动允许"）。否则用户会看到开关是关的却不知为何关不掉/开不上，也无法自助恢复。

`disabled-by-policy` 同理，提示"系统策略禁止"，避免被误认为应用缺陷。

---

## 五、验收标准

改完后需在真机（已安装 MSIX）通过以下全部检查：

1. 设置中心开启自启后，注册表出现
   `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData\com.dsh.wallpaper_pdxj8y3r6rm5g\DshWallpaperStartup`，其 `State` 为启用值。
2. 手动删除该注册表项（模拟被重置）后重启应用 → **该键被自动重建**（方案 2a），且 `startup-diagnostic.log` 有对应记录。
3. 重启电脑 → 应用自动启动。
4. 在「设置 → 应用 → 启动」中关闭 DSH Wallpaper → 重启应用后**不会被自动改回**（`DisabledByUser` 边界）。
5. 关闭自启后，Run 键与 StartupTask 均被清理，重启不启动。
6. `cargo test --manifest-path wallpaper/src-tauri/Cargo.toml` 全绿；建议为新函数补单元测试（可参照 `windows_integration.rs:2940` 附近 `hit_test_uses_half_open_rectangles` 的测试风格）。
7. 无包身份（开发版 / NSIS 版）路径回归：仍能通过 Run 键正常自启。

## 六、不应做的事

- **不要**把自启注册改成"每次启动都无条件重写"——会覆盖用户手动关闭的意愿，且每次启动都写注册表会引发 HIPS 频繁告警。
- **不要**自动改写 `DisabledByUser` / `DisabledByPolicy` 状态。
- **不要**改用任务计划程序（`schtasks`）绕过 MSIX StartupTask——MSIX 包身份下这属于非预期路径，且会引入新的安全软件告警面。
- **不要**为了"看起来修好了"而在 UI 上把 `source` 一律显示为已启用。

## 七、相关代码位置索引

| 位置 | 内容 |
|---|---|
| `wallpaper/src-tauri/src/lib.rs:1039-1083` | `set_autostart_blocking`（含 `reg.exe` 调用） |
| `wallpaper/src-tauri/src/lib.rs:1086-1104` | `set_autostart` / `autostart_status` command 层 |
| `wallpaper/src-tauri/src/lib.rs:2217-2231` | 启动时的 `migrate_legacy_autostart` 调用点 |
| `wallpaper/src-tauri/src/windows_integration.rs:2166-2179` | `has_package_identity`（`GetCurrentPackageFullName`） |
| `wallpaper/src-tauri/src/windows_integration.rs:2185-2217` | `set_startup_task`（`RequestEnableAsync` 与状态分支） |
| `wallpaper/src-tauri/src/windows_integration.rs:2223-2228` | `AutostartStatus` 结构（`enabled` + `source`） |
| `wallpaper/src-tauri/src/windows_integration.rs:2231-2274` | `legacy_run_entry_present`（原生读） |
| `wallpaper/src-tauri/src/windows_integration.rs:2277-2308` | `remove_legacy_run_entry`（原生删） |
| `wallpaper/src-tauri/src/windows_integration.rs:2315-2356` | `autostart_status` |
| `wallpaper/src-tauri/src/windows_integration.rs:2362-2375` | `migrate_legacy_autostart`（自愈应扩展在此） |
| `packaging/msix/AppxManifest.xml:25-27` | StartupTask 声明（`Enabled="false"`） |
| `packaging/msix/AppxManifest-Lite.xml:25-27` | Lite 版同款声明 |

## 八、当前分支提示

排查时工作区位于 `codex/startup-bootstrap-retry` 分支（含 9 个未合入 master 的提交，其中 `eaec264 fix: recover native startup handoff` 与 `e532ff0 chore: bump lockscreen test package` 均改动过 `packaging/msix/AppxManifest.xml`）。实施前请确认基线分支，避免与在途工作冲突。
