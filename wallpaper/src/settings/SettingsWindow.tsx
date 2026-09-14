import { useEffect, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { appCoreClient } from '../runtime/appCoreClient.ts'
import { nativeRuntime, type AutostartStatus, type DesktopDisplayInfo, type LockScreenDiagnostics, type ManagedDshStatus, type TranslucentTbStatus } from '../native/runtime.ts'
import { loadSettings, saveSettings, type WallpaperSettings } from './store.ts'
import { SettingsPanel } from './SettingsPanel.tsx'
import { chooseAppearanceImportPaths, nativeAppearance } from '../native/appearance.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
import './SettingsWindow.css'


export function SettingsWindow() {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [harness, setHarness] = useState<'offline' | 'web-only' | 'bridge-ready'>('offline')
  const [interactionEnabled, setInteractionEnabled] = useState(true)
  const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })
  const [dshCandidates, setDshCandidates] = useState<Array<{ rootPath: string; source: string }>>([])
  const [managedDsh, setManagedDsh] = useState<ManagedDshStatus>({ managed: false, running: false })
  const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()
  const [desktopDisplays, setDesktopDisplays] = useState<DesktopDisplayInfo[]>([])
  const [lockScreenBusy, setLockScreenBusy] = useState(false)
  const [autostartBusy, setAutostartBusy] = useState(false)
  const [notice, setNotice] = useState<string>()
  const [appearanceAssets, setAppearanceAssets] = useState<AppearanceAssetSummary[]>([])
  const [appearanceOverrides, setAppearanceOverrides] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [appearanceBusy, setAppearanceBusy] = useState(false)
  // A state update does not become visible to an async callback until React
  // renders again. Keep the last committed settings here so a successful
  // lock-screen request never overwrites unrelated settings changed while it
  // was in flight.
  const settingsRef = useRef(settings)
  const lockScreenOperationRef = useRef(false)
  const autostartOperationRef = useRef(false)
  const lockScreenDiagnosticsRequestRef = useRef(0)

  const commitSettings = (next: WallpaperSettings) => {
    settingsRef.current = next
    setSettings(next)
    saveSettings(next)
    // Every Tauri WebView owns an isolated browser storage partition. Route
    // settings through Rust so this renderer cannot emit to arbitrary Tauri
    // event targets; Rust delivers the snapshot only to the background host.
    void invoke('publish_settings', { settings: next })
      .catch((error) => setNotice(`设置同步失败：${String(error)}`))
  }

  useEffect(() => {
    if (!notice) return
    // Notices are transient feedback, not a second persistent UI layer.
    // Failures stay long enough to read; every notice still has a keyboard-
    // and pointer-accessible dismissal control below.
    const delay = /失败|错误|拒绝|无法|不允许/.test(notice) ? 8200 : 4200
    const timer = window.setTimeout(() => setNotice(undefined), delay)
    return () => window.clearTimeout(timer)
  }, [notice])

  const refreshAppearance = () => void Promise.all([nativeAppearance.getState(), nativeAppearance.listAssets()])
    .then(([snapshot, assets]) => { setAppearanceOverrides(snapshot.overrides); setAppearanceAssets(assets) })
    .catch((error) => setNotice(`素材库读取失败：${String(error)}`))

  const refreshTranslucentTb = () => void nativeRuntime.translucentTbStatus().then(setTranslucentTb).catch((error) => setNotice(String(error)))
  const scanDsh = () => void nativeRuntime.scanDshPaths().then(setDshCandidates).catch((error) => setNotice(String(error)))
  const refreshManagedDsh = () => void nativeRuntime.managedDshStatus().then(setManagedDsh).catch((error) => setNotice(String(error)))
  const refreshLockScreenDiagnostics = async () => {
    const request = ++lockScreenDiagnosticsRequestRef.current
    try {
      const diagnostics = await nativeRuntime.lockScreenDiagnostics()
      // An older initial/refresh request may finish after a successful
      // takeover or restore. Never let it replace the newer system result.
      if (request === lockScreenDiagnosticsRequestRef.current) setLockScreenDiagnostics(diagnostics)
    } catch (error) {
      if (request === lockScreenDiagnosticsRequestRef.current) setNotice(`锁屏检查失败：${String(error)}`)
    }
  }
  const refreshDesktopDisplays = async () => {
    try {
      setDesktopDisplays(await nativeRuntime.desktopDisplays())
    } catch (error) {
      setNotice(`显示器列表读取失败：${String(error)}`)
    }
  }
  const refreshAutostartStatus = async () => {
    if (!nativeRuntime.isNative) return
    try {
      const status: AutostartStatus = await nativeRuntime.autostartStatus()
      const current = settingsRef.current
      if (current.autostart !== status.enabled) {
        const next = { ...current, autostart: status.enabled }
        settingsRef.current = next
        setSettings(next)
        saveSettings(next)
        void invoke('publish_settings', { settings: next }).catch((error) => setNotice(`设置同步失败：${String(error)}`))
      }
      if (status.source === 'disabled-by-user') setNotice('Windows 已禁用 DSH Wallpaper 开机启动，请在系统设置中允许。')
      if (status.source === 'disabled-by-policy') setNotice('Windows 策略禁止 DSH Wallpaper 开机启动。')
    } catch (error) {
      setNotice(`读取开机自启状态失败：${String(error)}`)
    }
  }
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
  useEffect(() => {
    void appCoreClient.snapshot().then((snapshot) => { setHarness(snapshot.harness); setInteractionEnabled(snapshot.interaction.enabled) })
    refreshTranslucentTb()
    scanDsh()
    refreshManagedDsh()
    void refreshAutostartStatus()
    refreshLockScreenDiagnostics()
    void refreshDesktopDisplays()
    refreshAppearance()
    const current = getCurrentWindow()
    const unlisten = current.onCloseRequested((event) => {
      event.preventDefault()
      void invoke('hide_settings_window')
    })
    // The background surface receives the native display-change event. The
    // settings window intentionally does not subscribe to renderer events;
    // while it is open, a light polling refresh keeps its monitor list current
    // without widening the settings-to-background event boundary.
    const displayTimer = window.setInterval(() => { void refreshDesktopDisplays() }, 5000)
    return () => {
      void unlisten.then((dispose) => dispose())
      window.clearInterval(displayTimer)
    }
  }, [])

  const change = (next: WallpaperSettings) => {
    const previous = settingsRef.current
    const autostartChanged = next.autostart !== previous.autostart
    // System lock-screen ownership is deliberately excluded from the normal
    // immediate-save path. The dedicated async operation above is the only
    // place allowed to persist or broadcast a change to this field.
    const normalNext = next.lockScreenEnabled === previous.lockScreenEnabled
      ? next
      : { ...next, lockScreenEnabled: previous.lockScreenEnabled }
    if (autostartChanged) {
      if (autostartOperationRef.current) return
      autostartOperationRef.current = true
      setAutostartBusy(true)
      void nativeRuntime.setAutostart(next.autostart)
        .then((status) => {
          const current = settingsRef.current
          commitSettings({ ...current, autostart: status.enabled })
          if (status.enabled !== next.autostart) setNotice('Windows 没有接受这次开机自启变更，请检查系统启动应用权限。')
        })
        .catch((error) => setNotice(`开机自启更新失败：${String(error)}`))
        .finally(() => {
          autostartOperationRef.current = false
          setAutostartBusy(false)
        })
      return
    }
    commitSettings(normalNext)
  }

  const close = () => void invoke('hide_settings_window')
  const importAppearance = async () => {
    setAppearanceBusy(true)
    try {
      const paths = await chooseAppearanceImportPaths()
      if (paths.length === 0) return
      await nativeAppearance.importPaths(paths)
      refreshAppearance()
    } catch (error) { setNotice(`导入失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const classifyAppearance = async (assetId: string, slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { await nativeAppearance.classifyAsset(assetId, [slot]); refreshAppearance() } catch (error) { setNotice(`素材分类失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const selectAppearance = async (slot: AppearanceSlot, assetId: string) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.setOverride(slot, assetId)).overrides); void invoke('notify_appearance_changed').catch((error) => setNotice(`外观同步失败：${String(error)}`)) } catch (error) { setNotice(`应用素材失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const clearAppearance = async (slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.clearOverride(slot)).overrides); void invoke('notify_appearance_changed').catch((error) => setNotice(`外观同步失败：${String(error)}`)) } catch (error) { setNotice(`恢复默认失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }

  return <main className="settings-window">
    {notice && <div className="settings-window__notice" role="status"><span>{notice}</span><button type="button" aria-label="关闭通知" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); setNotice(undefined) }}>×</button></div>}
    <SettingsPanel
      settings={settings}
      harnessStatus={harness}
      translucentTb={translucentTb}
      dshCandidates={dshCandidates}
      onScanDsh={scanDsh}
      onAdoptDsh={(rootPath) => change({ ...settingsRef.current, dshLaunch: { ...settingsRef.current.dshLaunch, rootPath } })}
      onLaunchDsh={() => { const dsh = settingsRef.current.dshLaunch; if (!dsh.rootPath) return; void nativeRuntime.launchDsh(dsh.rootPath, dsh.profile, dsh.command).then((pid) => { setNotice(`已启动 DSH（PID ${pid}），等待 Bridge 就绪后可在桌面切换。`); refreshManagedDsh() }).catch((error) => setNotice(String(error))) }}
      managedDsh={managedDsh}
      onRefreshManagedDsh={refreshManagedDsh}
      onStopManagedDsh={() => void nativeRuntime.stopManagedDsh().then(() => { setNotice('已停止本应用启动的 DSH。'); refreshManagedDsh() }).catch((error) => setNotice(String(error)))}
      onChange={change}
      onRefreshTranslucentTb={refreshTranslucentTb}
      onLaunchTranslucentTb={() => void nativeRuntime.launchTranslucentTb().then(refreshTranslucentTb).catch((error) => setNotice(String(error)))}
      onInstallTranslucentTb={() => void nativeRuntime.openTranslucentTbInstall().catch((error) => setNotice(String(error)))}
      appearanceAssets={appearanceAssets}
      appearanceOverrides={appearanceOverrides}
      appearanceBusy={appearanceBusy}
      onImportAppearance={() => { void importAppearance() }}
      onClassifyAppearance={(assetId, slot) => { void classifyAppearance(assetId, slot) }}
      onSelectAppearance={(slot, assetId) => { void selectAppearance(slot, assetId) }}
      onClearAppearance={(slot) => { void clearAppearance(slot) }}
      lockScreenDiagnostics={lockScreenDiagnostics}
      onRefreshLockScreenDiagnostics={refreshLockScreenDiagnostics}
      onRestoreLockScreen={() => { void openWindowsLockScreenSettings() }}
      onClearStaleLockScreenBackup={() => { void clearStaleLockScreenBackup() }}
      onSetLockScreenEnabled={(enabled) => { void setLockScreenEnabled(enabled) }}
      lockScreenBusy={lockScreenBusy}
      autostartBusy={autostartBusy}
      desktopDisplays={desktopDisplays}
      onRefreshDesktopDisplays={refreshDesktopDisplays}
      onRequestDeepSeekLogin={() => void nativeRuntime.requestDeepSeekLogin().catch((error) => setNotice(`无法打开 DeepSeek 应用内页面：${String(error)}`))}
      onConfigureApiKey={() => void nativeRuntime.promptForApiKeyCredential()
        .then((saved) => { if (saved) setNotice('DeepSeek API Key 已更新到 Windows 凭据管理器。') })
        .catch((error) => setNotice(String(error)))}
      interactionEnabled={interactionEnabled}
      onSetInteractionEnabled={(enabled) => void appCoreClient.setInteractionEnabled(enabled).then((snapshot) => setInteractionEnabled(snapshot.interaction.enabled)).catch((error) => setNotice(String(error)))}
      onClose={close}
    />
  </main>
}
