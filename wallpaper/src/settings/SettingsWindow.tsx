import { useEffect, useState } from 'react'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { appCoreClient } from '../runtime/appCoreClient.ts'
import { nativeRuntime, type TranslucentTbStatus } from '../native/runtime.ts'
import { loadSettings, saveSettings, type WallpaperSettings } from './store.ts'
import { SettingsPanel } from './SettingsPanel.tsx'
import { chooseAppearanceImportPaths, nativeAppearance } from '../native/appearance.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
import './SettingsWindow.css'


export function SettingsWindow() {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [harness, setHarness] = useState<'offline' | 'web-only' | 'bridge-ready'>('offline')
  const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })
  const [notice, setNotice] = useState<string>()
  const [appearanceAssets, setAppearanceAssets] = useState<AppearanceAssetSummary[]>([])
  const [appearanceOverrides, setAppearanceOverrides] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [appearanceBusy, setAppearanceBusy] = useState(false)

  const refreshAppearance = () => void Promise.all([nativeAppearance.getState(), nativeAppearance.listAssets()])
    .then(([snapshot, assets]) => { setAppearanceOverrides(snapshot.overrides); setAppearanceAssets(assets) })
    .catch((error) => setNotice(`素材库读取失败：${String(error)}`))

  const refreshTranslucentTb = () => void nativeRuntime.translucentTbStatus().then(setTranslucentTb).catch((error) => setNotice(String(error)))
  useEffect(() => {
    void appCoreClient.snapshot().then((snapshot) => setHarness(snapshot.harness))
    refreshTranslucentTb()
    refreshAppearance()
    const current = getCurrentWindow()
    const unlisten = current.onCloseRequested((event) => {
      event.preventDefault()
      void invoke('hide_settings_window')
    })
    return () => { void unlisten.then((dispose) => dispose()) }
  }, [])

  const change = (next: WallpaperSettings) => {
    const previous = settings
    setSettings(next)
    saveSettings(next)
    // Every Tauri WebView owns an isolated browser storage partition. Carry
    // the new value with the event so the background WebView never rereads its
    // own stale localStorage copy.
    void emit('settings-changed', next)
    if (next.lockScreenEnabled !== previous.lockScreenEnabled) {
      void nativeRuntime.setLockScreen(next.lockScreenEnabled).catch((error) => {
        // Do not leave a setting enabled when Windows rejected the operation.
        const reverted = { ...next, lockScreenEnabled: previous.lockScreenEnabled }
        setSettings(reverted)
        saveSettings(reverted)
        void emit('settings-changed', reverted)
        setNotice(String(error))
      })
    }
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
    try { setAppearanceOverrides((await nativeAppearance.setOverride(slot, assetId)).overrides); void emit('appearance-changed') } catch (error) { setNotice(`应用素材失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }
  const clearAppearance = async (slot: AppearanceSlot) => {
    setAppearanceBusy(true)
    try { setAppearanceOverrides((await nativeAppearance.clearOverride(slot)).overrides); void emit('appearance-changed') } catch (error) { setNotice(`恢复默认失败：${String(error)}`) } finally { setAppearanceBusy(false) }
  }

  return <main className="settings-window">
    {notice && <div className="settings-window__notice">{notice}<button onClick={() => setNotice(undefined)}>×</button></div>}
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
      onRequestDeepSeekLogin={() => void nativeRuntime.requestDeepSeekLogin()}
      onConfigureApiKey={() => {
        const key = window.prompt('输入 DeepSeek API Key。密钥只会写入 Windows 凭据管理器。')
        if (key) void nativeRuntime.saveApiKey(key).catch((error) => setNotice(String(error)))
      }}
      onClose={close}
    />
  </main>
}
