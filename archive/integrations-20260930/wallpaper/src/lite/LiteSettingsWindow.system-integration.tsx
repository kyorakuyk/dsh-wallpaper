// 归档片段（B4，2026-10）：`wallpaper/src/lite/LiteSettingsWindow.tsx` 中系统集成（1B：TranslucentTB / 登录过渡底图）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/integrations-20260930/README.md` 的 B4 小节。

// ---- 原 6-7 行：import 区，锁屏的 import 注释（原 4-5）之后 ----
// FREEZE(1B)：TranslucentTB 退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
import type { AutostartStatus, TranslucentTbStatus } from '../native/runtime.ts'

// ---- 原 38-41 行：组件内 state，锁屏的 `lockScreenDiagnostics`（原 36-37）之后、`customBackground` 之前 ----
  // FREEZE(1B)：随系统集成冻结。
  const [desktopFallbackStatus, setDesktopFallbackStatus] = useState<liteNative.DesktopWallpaperFallbackStatus>()
  // FREEZE(1B)：随系统集成冻结。
  const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })

// ---- 原 47-48 行：锁屏的 `lockScreenBusy`（原 45-46）之后 ----
  // FREEZE(1B)：随系统集成冻结。
  const [desktopFallbackBusy, setDesktopFallbackBusy] = useState(false)

// ---- 原 104-121 行：`refreshAutostart` 之后 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一块随之冻结。恢复办法：取消注释。
  const refreshDesktopFallback = async () => {
    try {
      const status = await liteNative.desktopWallpaperFallbackStatus()
      setDesktopFallbackStatus(status)
      const current = settingsRef.current
      // A successful native takeover is authoritative. If the settings file
      // says enabled but no recovery point exists, fail closed and clear the
      // stale preference instead of silently changing the desktop wallpaper.
      if (status.managedActive && !current.desktopWallpaperFallback) {
        commit({ ...current, desktopWallpaperFallback: true })
      } else if (!status.backupExists && current.desktopWallpaperFallback) {
        commit({ ...current, desktopWallpaperFallback: false })
      }
    } catch (error) {
      setNotice(`读取登录过渡底图状态失败：${String(error)}`)
    }
  }

// ---- 原 123-130 行：紧接上一段，`refreshCustomImages` 之前 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一块随之冻结。恢复办法：取消注释。
  const refreshTranslucentTb = async () => {
    try {
      setTranslucentTb(await liteNative.translucentTbStatus())
    } catch (error) {
      setNotice(`读取 TranslucentTB 状态失败：${String(error)}`)
    }
  }

// ---- 原 152-155 行：挂载 effect 里 `void refreshAutostart()` 之后、`void refreshCustomImages()` 之前 ----
      // FREEZE(1B)：随上面两者冻结。
      void refreshDesktopFallback()
      // FREEZE(1B)：随上面两者冻结。
      void refreshTranslucentTb()

// ---- 原 230-244 行：`setAutostart` 与锁屏的 `setLockScreen`（原 212-228）之后 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一块随之冻结。恢复办法：取消注释。
  const setDesktopFallback = async (enabled: boolean) => {
    if (desktopFallbackBusy) return
    setDesktopFallbackBusy(true)
    try {
      const confirmation = await liteNative.setDesktopWallpaperFallback(enabled)
      commit({ ...settingsRef.current, desktopWallpaperFallback: enabled })
      setNotice(confirmation)
      await refreshDesktopFallback()
    } catch (error) {
      setNotice(`${enabled ? '启用登录过渡底图' : '关闭登录过渡底图'}失败：${String(error)}`)
    } finally {
      setDesktopFallbackBusy(false)
    }
  }

// ---- 原第 311 行：`01 · SYSTEM` 卡片内，卡片标题之后、自动启动行之前 ----
        {/* FREEZE(1B)：登录过渡底图随系统集成一起冻结。 */}
        <SettingRow title="登录过渡底图" detail={desktopFallbackBusy ? '正在更新 Explorer 桌面底图。' : desktopFallbackStatus?.managedActive ? '已确认 Explorer 正在使用睡眠画面；重启后可减少解锁空档。' : desktopFallbackStatus?.warning ?? '让 Explorer 在应用启动前先显示睡眠画面，减少解锁后的原壁纸空档。'}><Toggle label="登录过渡底图" checked={settings.desktopWallpaperFallback} disabled={desktopFallbackBusy} onChange={(value) => void setDesktopFallback(value)} /></SettingRow>

// ---- 原 334-340 行：`03 · SCENE` 卡片之后、`<footer className="lite-footer">` 之前 ----
      {/* FREEZE(1B)：TranslucentTB 卡片随系统集成一起冻结。恢复办法：去掉这对注释，并恢复状态与刷新函数。 */}
      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">04 · COMPATIBILITY</span><h2>TranslucentTB</h2></div><span className={`lite-pill ${translucentTb.running ? 'is-online' : ''}`}>{translucentTb.running ? '运行中' : translucentTb.installed ? '已安装' : '未检测到'}</span></div>
        <p className="lite-card-description">Lite 不修改任务栏，只提供与 TranslucentTB 的兼容入口。任务栏透明效果由 TranslucentTB 自己管理。</p>
        <div className="lite-actions"><button type="button" disabled={!translucentTb.installed || translucentTb.running} onClick={() => void liteNative.launchTranslucentTb().then(refreshTranslucentTb).catch((error) => setNotice(String(error)))}>启动 TranslucentTB</button><button type="button" onClick={() => void liteNative.openTranslucentTbInstall().catch((error) => setNotice(String(error)))}>前往 Microsoft Store</button><button type="button" onClick={() => void refreshTranslucentTb()}>刷新状态</button></div>
      </section>
