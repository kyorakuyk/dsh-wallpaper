# 归档：锁屏相关（2026-09-30）

用户决定**壁纸不再触碰锁屏**：现在做的和用户自己在 Windows 设置里换一张没有区别，却多出风险，
而且不能回滚。所以整块退出——整文件进这里归档，代码块在原处按 FREEZE 注释。

| 归档文件 | 原位置 |
| --- | --- |
| wallpaper/src-tauri/src/bin/lockscreen_probe.rs | 独立的锁屏诊断探针（cargo feature `lockscreen-probe`） |
| scripts/build-lockscreen-probe.ps1 | 构建并安装那个探针包的脚本 |
| packaging/msix/LockScreenProbe.AppxManifest.xml | 探针的 MSIX 清单 |

恢复办法：把文件放回原位，并取消 `wallpaper/src-tauri/Cargo.toml` 里那段注释
（`lockscreen-probe` feature 与 `[[bin]]`）。

当前状态（2026-10）：锁屏的 Rust 实现已迁出到这里（见下一节 B2）；`lib.rs` 里注释掉的锁屏命令、
handler 注释行、`build.rs` 与 permissions 的接线待 B3 处理，前端部分待 B4。最初按 FREEZE 注释
的代码块清单见 `docs/plans/release-scope-cleanup-plan.md` 第一节。

## 2026-10 第二批归档（B2）：Rust 锁屏实现

来源 commit `3c92772`（迁移前打了 tag `pre-freeze-isolation`）；下列行号均指该 commit。
这一批把锁屏的 Rust 实现移出默认构建。开机自启、`has_package_identity`、首帧素材
`LockScreenSleep.png`（`native_bootstrap.rs` 的 `find_asset`）属于共享/现役能力，仍留在原处。

| 归档文件 | 原位置 |
| --- | --- |
| wallpaper/src-tauri/src/lock_screen_backup.rs | 同路径整文件（`git mv`），含 `file_content_hash` 与 17 条单元测试 |
| wallpaper/src-tauri/tests/lock_screen_backup.rs | 同路径整文件（`git mv`），Cargo 自动发现的独立测试目标（`#[path]` 引入上面的模块） |
| wallpaper/src-tauri/src/windows_integration/lock_screen.rs | 新文件：从 `wallpaper/src-tauri/src/windows_integration.rs` 逐字抽出的锁屏段（按原顺序，含注释与 `#[cfg]`） |

`windows_integration.rs` 中被移出的符号（按区段，原行号）：

- 283-335：`LOCK_SCREEN_TRANSACTION`、`LOCK_SCREEN_TRANSACTION_MUTEX_NAME`、`CrossProcessLockScreenTransaction`（含 impl 与 `Drop`）
- 3355-3373：`lock_screen_config_dir`
- 3375-3638：`set_lock_screen`（Windows）
- 3640-3671：`clear_stale_lock_screen_backup`
- 3673-3758：`restore_precondition_is_satisfied`、`finish_lock_screen_takeover_attempt`、`abort_lock_screen_takeover_before_set`、`finish_verified_lock_screen_restore`
- 4711-4724：`can_attempt_lock_screen_takeover`、`bundled_sleep_resource_candidates`
- 4726-4754：`current_package_install_root`
- 4756-4846：`bundled_sleep_image_path`
- 4848-4938：`managed_image_path_for_state`、`copy_sleep_image_without_overwrite`、`remove_backup_after_restore`
- 4940-5015：`LockScreenDiagnostics`、`lock_screen_diagnostics`（Windows）
- 5033-5068：非 Windows 桩 `set_lock_screen`、`LockScreenDiagnostics`、`lock_screen_diagnostics`

被移出的测试：`lock_screen_backup.rs` 内 17 条（它们原本经 `lib.rs` 的 `mod lock_screen_backup;`
编进 lib 测试，又经独立测试目标再跑一遍）；`windows_integration.rs` 测试模块里 6 条，现放在
`lock_screen.rs` 末尾的 `#[cfg(test)] mod tests`：

- `packaged_builds_are_always_eligible_for_lock_screen_takeover`（原 5190-5194）
- `unpackaged_lock_screen_takeover_is_never_eligible`（原 5196-5199）
- `packaged_sleep_resource_uses_tauris_verified_windows_path_first`（原 5487-5493）
- `v2_backup_uses_its_manifest_managed_filename`（原 5495-5518）
- `managed_sleep_copy_never_overwrites_an_existing_path`（原 5520-5541）
- `restore_precondition_requires_the_current_managed_image`（原 5543-5558）

从现役删除的 import（都只被锁屏段使用；`lock_screen.rs` 顶部原样保留了这些 `use`）：

- `wallpaper/src-tauri/src/lib.rs:30` `mod lock_screen_backup;`
- `windows_integration.rs:19-27` `use crate::lock_screen_backup::{…}` 整块
- `windows::Storage::StorageFile`
- `windows::System::UserProfile::{LockScreen, UserProfilePersonalizationSettings}`
- `windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0}`
- `windows::Win32::Storage::Packaging::Appx::GetCurrentPackagePath`（现役里只剩 `current_run_entry_command` 的一句注释提到它）
- `windows::Win32::System::Threading::{ReleaseMutex, WaitForSingleObject}`（`CreateMutexW` 仍被壁纸宿主互斥锁使用，保留）
- 测试模块的 `use tempfile::tempdir;`

### 恢复接线

1. 把两个整文件移回 `wallpaper/src-tauri/src/lock_screen_backup.rs` 与 `wallpaper/src-tauri/tests/lock_screen_backup.rs`，
   并在 `lib.rs` 重新加 `mod lock_screen_backup;`。
2. 把 `lock_screen.rs` 的内容按原行号并回 `windows_integration.rs`；或把它放到
   `src/windows_integration/lock_screen.rs`，在 `windows_integration.rs` 里 `mod lock_screen;`，
   并调整 `use`（子模块需 `use super::*;` 或显式引入 `has_package_identity` 等父模块项，
   被调用的项改成 `pub(super)`/`pub(crate)`，`lib.rs` 的调用路径相应改为 `windows_integration::lock_screen::…`）。
3. 恢复上面列出的被删 import（文件顶部已原样写出），测试里的 `tempfile::tempdir` 同样恢复。
4. `lib.rs` 里注释掉的锁屏命令与 handler 注释行、`build.rs` 的命令名单、`permissions/` 下的锁屏权限：见 B3。

### 未验证范围

- 归档代码不再被编译检查；恢复前须重新编译并运行
  `cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --all-targets`（以及 Lite 的 `--no-default-features --features lite`）。
- IDE 中归档文件的 import 无法解析属预期（它们不在任何 crate 的模块树里）。
- CI 的 `paths-ignore` 含 `archive/**`，只改动这里不会触发 CI。
