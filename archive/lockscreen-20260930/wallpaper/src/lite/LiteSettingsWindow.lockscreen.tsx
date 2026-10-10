// 归档片段（B4，2026-10）：`wallpaper/src/lite/LiteSettingsWindow.tsx` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原 4-5 行：import 区，`@tauri-apps/api/core` 之后。这行 import 同时带着 1B 的 `TranslucentTbStatus`；只恢复锁屏时去掉它 ----
// FREEZE(1A)：锁屏退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
import type { AutostartStatus, LockScreenDiagnostics, TranslucentTbStatus } from '../native/runtime.ts'

// ---- 原 36-37 行：组件内 state，`notice` 之后 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()

// ---- 原 45-46 行：`autostartBusy` 之后 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const [lockScreenBusy, setLockScreenBusy] = useState(false)

// ---- 原 49-50 行：`desktopFallbackBusy`（1B）之后、`autostartOperationRef` 之前 ----
  // FREEZE(1A)：锁屏退出后没人再用。
  const lockOperationRef = useRef(false)

// ---- 原 70-88 行：`commit` 之后、`refreshAutostart` 之前 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const refreshDiagnostics = async () => {
    try {
      const diagnostics = await liteNative.lockScreenDiagnostics()
      setLockScreenDiagnostics(diagnostics)
      // A user may install Lite while the full edition's shared lock-screen
      // recovery point is still active. Mirror Windows' authoritative state
      // instead of presenting a misleading unchecked toggle.
      const current = settingsRef.current
      if (current.lockScreenEnabled !== diagnostics.managedImageActive) {
        const next = { ...current, lockScreenEnabled: diagnostics.managedImageActive }
        settingsRef.current = next
        setSettings(next)
        void saveLiteSettings(next).catch((error) => setNotice(`设置同步失败：${String(error)}`))
      }
    } catch (error) {
      setNotice(`锁屏检查失败：${String(error)}`)
    }
  }

// ---- 原 149-150 行：挂载 effect 里 `setSettings(loaded)` 之后、`void refreshAutostart()` 之前 ----
      // FREEZE(1A)：锁屏诊断随锁屏一起退出，这次挂载时的刷新随之冻结。
      void refreshDiagnostics()

// ---- 原 212-228 行：`setAutostart` 之后 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const setLockScreen = async (enabled: boolean) => {
    if (lockOperationRef.current) return
    lockOperationRef.current = true
    setLockScreenBusy(true)
    try {
      const confirmation = await liteNative.setLockScreen(enabled)
      commit({ ...settingsRef.current, lockScreenEnabled: enabled })
      setNotice(confirmation)
      await refreshDiagnostics()
    } catch (error) {
      setNotice(`${enabled ? '接管锁屏图片' : '恢复原锁屏图片'}失败：${String(error)}`)
    } finally {
      lockOperationRef.current = false
      setLockScreenBusy(false)
    }
  }

// ---- 原 246-254 行：`setDesktopFallback`（1B）之后 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const openLockScreenSettings = async () => {
    try {
      await liteNative.openWindowsLockScreenSettings()
      setNotice('已打开 Windows 锁屏设置。')
    } catch (error) {
      setNotice(`无法打开 Windows 锁屏设置：${String(error)}`)
    }
  }

// ---- 原 256-271 行：紧接上一段，`chooseCustomImage` 之前。确认对话框是删除原锁屏图片副本前的硬性要求 ----
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  const clearStaleLockScreenBackup = async () => {
    if (lockOperationRef.current) return
    if (!window.confirm('清理过期恢复点会永久删除已保存的原锁屏图片副本。Windows 当前锁屏图片不会被修改。确定继续吗？')) return
    lockOperationRef.current = true
    setLockScreenBusy(true)
    try {
      setNotice(await liteNative.clearStaleLockScreenBackup(true))
      await refreshDiagnostics()
    } catch (error) {
      setNotice(`清理旧锁屏恢复点失败：${String(error)}`)
    } finally {
      lockOperationRef.current = false
      setLockScreenBusy(false)
    }
  }

// ---- 原第 308 行：`01 · SYSTEM` 卡片（`<section className="lite-card">`）的第一行，卡片标题之前 ----
        {/* FREEZE(1A)：这张卡里的"锁屏"状态点随锁屏一起冻结（卡内还留着登录过渡底图与自动启动，所以标题保留）。 */}

// ---- 原第 310 行：卡片标题之后、登录过渡底图行（1B，原 311）之前 ----
        {/* FREEZE(1A)：锁屏退出，这一行随之冻结。 */}
        <SettingRow title="接管 Windows 锁屏图片" detail={lockScreenBusy ? '正在应用系统设置，请稍候。' : '密码输入页仍由 Windows 原生处理。'}><Toggle label="接管 Windows 锁屏图片" checked={settings.lockScreenEnabled} disabled={lockScreenBusy} onChange={(value) => void setLockScreen(value)} /></SettingRow>

// ---- 原 313-314 行：自动启动行之后、`</section>` 之前。诊断块里引用了 1B 的 `desktopFallbackStatus`；只恢复锁屏时去掉那一段 ----
        {/* FREEZE(1A)：锁屏退出，这一行随之冻结。 */}
        <div className="lite-actions"><button type="button" onClick={() => void openLockScreenSettings()}>打开 Windows 锁屏设置</button><button type="button" onClick={() => void refreshDiagnostics()}>刷新诊断</button></div>
        {/* FREEZE(1A)：锁屏退出，这一行随之冻结。 */}
        {lockScreenDiagnostics && <div className="lite-diagnostics"><strong>{lockScreenDiagnostics.takeoverAvailable ? '锁屏接管可用' : '当前暂不可接管锁屏'}</strong>{lockScreenDiagnostics.warnings.slice(0, 2).map((warning) => <span key={warning}>{warning}</span>)}{desktopFallbackStatus?.backupExists && desktopFallbackStatus.warning && <span>{desktopFallbackStatus.warning}</span>}{lockScreenDiagnostics.staleBackup && <button type="button" className="lite-diagnostics-action" disabled={lockScreenBusy} onClick={() => void clearStaleLockScreenBackup()}>清理过期恢复点（删除原图副本）</button>}</div>}
