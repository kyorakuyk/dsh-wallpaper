import { useEffect, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { appCoreClient } from '../runtime/appCoreClient.ts'
import { nativeRuntime, type LockScreenDiagnostics, type TranslucentTbStatus } from '../native/runtime.ts'
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
  const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()
  const [lockScreenBusy, setLockScreenBusy] = useState(false)
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
    lockScreenOperationRef.current = true
    setLockScreenBusy(true)
    try {
      setNotice(await nativeRuntime.clearStaleLockScreenBackup())
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
    refreshLockScreenDiagnostics()
    refreshAppearance()
    const current = getCurrentWindow()
    const unlisten = current.onCloseRequested((event) => {
      event.preventDefault()
      void invoke('hide_settings_window')
    })
    return () => { void unlisten.then((dispose) => dispose()) }
  }, [])

  const change = (next: WallpaperSettings) => {
    const previous = settingsRef.current
    // System lock-screen ownership is deliberately excluded from the normal
    // immediate-save path. The dedicated async operation above is the only
    // place allowed to persist or broadcast a change to this field.
    const normalNext = next.lockScreenEnabled === previous.lockScreenEnabled
      ? next
      : { ...next, lockScreenEnabled: previous.lockScreenEnabled }
    commitSettings(normalNext)
    if (next.autostart !== previous.autostart) void nativeRuntime.setAutostart(next.autostart).catch((error) => setNotice(String(error)))
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
      onRequestDeepSeekLogin={() => void nativeRuntime.requestDeepSeekLogin()}
      onConfigureApiKey={() => void nativeRuntime.promptForApiKeyCredential()
        .then((saved) => { if (saved) setNotice('DeepSeek API Key 已更新到 Windows 凭据管理器。') })
        .catch((error) => setNotice(String(error)))}
      interactionEnabled={interactionEnabled}
      onSetInteractionEnabled={(enabled) => void appCoreClient.setInteractionEnabled(enabled).then((snapshot) => setInteractionEnabled(snapshot.interaction.enabled)).catch((error) => setNotice(String(error)))}
      onClose={close}
    />
  </main>
}
