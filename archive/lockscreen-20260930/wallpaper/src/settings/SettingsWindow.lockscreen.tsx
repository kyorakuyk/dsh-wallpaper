// 归档片段（B4，2026-10）：`wallpaper/src/settings/SettingsWindow.tsx` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原 7-8 行：import 区，`appCoreClient` 之后。这行 import 同时带着 1B 的 `TranslucentTbStatus`；只恢复锁屏时去掉它 ----
// FREEZE(1A)：锁屏退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项，所以整行注释、旁边写出不含它的版本）。
import { nativeRuntime, type AutostartStatus, type ApiConversationListing, type ApiKeyStatus, type DeepSeekWebAdapterConfigStatus, type DesktopDisplayInfo, type DesktopWorkspaceStatus, type HarnessEndpointScan, type HarnessTarget, type LockScreenDiagnostics, type ManagedDshStatus, type TranslucentTbStatus } from '../native/runtime.ts'

// ---- 原 147-148 行：`managedDshBusy` 之后、`desktopDisplays` 之前 ----
  // FREEZE(1A)：锁屏退出后没人再用（见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()

// ---- 原 150-151 行：`desktopDisplays` 之后、`autostartBusy` 之前 ----
  // FREEZE(1A)：锁屏退出后没人再用（见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const [lockScreenBusy, setLockScreenBusy] = useState(false)

// ---- 原 215-216 行：`settingsRef` 之后、`autostartOperationRef` 之前 ----
  // FREEZE(1A)：锁屏退出后没人再用（见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const lockScreenOperationRef = useRef(false)

// ---- 原 237-238 行：`dshScanOperationRef` 之后、`apiHistoryOperationRef` 之前 ----
  // FREEZE(1A)：锁屏退出后没人再用（见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const lockScreenDiagnosticsRequestRef = useRef(0)

// ---- 原 375-387 行：`harnessTargetCatalog` 的 effect 之后、`refreshDesktopDisplays` 之前 ----
  // FREEZE(1A)：锁屏退出后没人再用（见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const refreshLockScreenDiagnostics = async () => {
    const request = ++lockScreenDiagnosticsRequestRef.current
    try {
      const diagnostics = await nativeRuntime.lockScreenDiagnostics()
      if (!mountedRef.current) return
      // An older initial/refresh request may finish after a successful
      // takeover or restore. Never let it replace the newer system result.
      if (request === lockScreenDiagnosticsRequestRef.current) setLockScreenDiagnostics(diagnostics)
    } catch (error) {
      if (request === lockScreenDiagnosticsRequestRef.current) setNotice(`锁屏检查失败：${String(error)}`)
    }
  }

// ---- 原 604-606 行：`probeRunners` 内，`deepseekWebAdapterConfig` 之后、`autostartStatus` 之前（与 settingsProbes 片段里的 `lockScreenDiagnostics` 探针一起恢复） ----
    // FREEZE(1A)：锁屏退出，这一条探针不再注册（见 docs/plans/release-scope-cleanup-plan.md 第一节）。
    // 恢复办法：取消注释即可 —— refreshLockScreenDiagnostics 仍然存在，只是暂时没人调用它。
    lockScreenDiagnostics: refreshLockScreenDiagnostics,

// ---- 原 835-881 行：`clearApiHistory` 之后、首帧探针注释（"First paint owns no probe at all"）之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：把下面这段还原。
  const setLockScreenEnabled = async (enabled: boolean, force = false) => {
    // `lockScreenBusy` only changes after a render. The ref closes the small
    // double-click / keyboard activation window before that render occurs.
    if (lockScreenOperationRef.current || (!force && settingsRef.current.lockScreenEnabled === enabled)) return
    lockScreenOperationRef.current = true
    setLockScreenBusy(true)
    try {
      const confirmation = await nativeRuntime.setLockScreen(enabled)
      // Do not optimistically persist or broadcast the setting: Windows is
      // authoritative here. Only record the requested state after its native
      // setter succeeds.
      commitSettings({ ...settingsRef.current, lockScreenEnabled: enabled })
      setNotice(confirmation)
      await refreshLockScreenDiagnostics()
    } catch (error) {
      // Keep the previously committed setting visible and persisted. This is
      // especially important when the MSIX identity gate rejects takeover.
      setNotice(`${enabled ? '接管锁屏图片' : '恢复原锁屏图片'}失败：${String(error)}`)
    } finally {
      lockScreenOperationRef.current = false
      setLockScreenBusy(false)
    }
  }
  const openWindowsLockScreenSettings = async () => {
    try {
      await nativeRuntime.openWindowsLockScreenSettings()
      setNotice('已打开 Windows 锁屏设置；请在系统设置中选择要恢复的图片。')
    } catch (error) {
      setNotice(`无法打开 Windows 锁屏设置：${String(error)}`)
    }
  }
  const clearStaleLockScreenBackup = async () => {
    if (lockScreenOperationRef.current) return
    if (!window.confirm('清理旧锁屏恢复点会永久删除已保存的原锁屏图片副本。Windows 当前锁屏图片不会被修改。确定继续吗？')) return
    lockScreenOperationRef.current = true
    setLockScreenBusy(true)
    try {
      setNotice(await nativeRuntime.clearStaleLockScreenBackup(true))
      await refreshLockScreenDiagnostics()
    } catch (error) {
      setNotice(`清理旧恢复点失败：${String(error)}`)
    } finally {
      lockScreenOperationRef.current = false
      setLockScreenBusy(false)
    }
  }

// ---- 原 979-980 行与 982-988 行：`change` 内。恢复后把现役的 `const normalNext = next`（原 989 行）换回下面的合并 ----
    // FREEZE(1A)：只为上面那段已冻结的合并而生，随之冻结。
    const previous = settingsRef.current

// ---- （同上，原 982-988 行；原 981 行 `const autostartChanged = …` 在两段之间，仍在现役） ----
    // FREEZE(1A)：锁屏退出，字段已不存在，合并回归"直接用 next"。恢复办法：还原下面被注释的六行。
    // System lock-screen ownership is deliberately excluded from the normal
    // immediate-save path. The dedicated async operation above is the only
    // place allowed to persist or broadcast a change to this field.
    const normalNext = next.lockScreenEnabled === previous.lockScreenEnabled
      ? next
      : { ...next, lockScreenEnabled: previous.lockScreenEnabled }

// ---- 原 1157-1164 行：`<SettingsPanel>` 的 props，`onClearAppearance` 之后、`autostartBusy` 之前 ----
      /* FREEZE(1A)：锁屏退出，以下 6 个 prop 随之冻结。恢复办法：去掉这对块注释即可。 */
      lockScreenDiagnostics={lockScreenDiagnostics}
      onRefreshLockScreenDiagnostics={refreshLockScreenDiagnostics}
      onRestoreLockScreen={() => { void openWindowsLockScreenSettings() }}
      onClearStaleLockScreenBackup={() => { void clearStaleLockScreenBackup() }}
      onSetLockScreenEnabled={(enabled) => { void setLockScreenEnabled(enabled) }}
      lockScreenBusy={lockScreenBusy}
