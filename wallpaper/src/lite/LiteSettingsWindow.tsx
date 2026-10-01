import { useEffect, useRef, useState, type ReactNode } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
// FREEZE(1A)：锁屏退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
// import type { AutostartStatus, LockScreenDiagnostics, TranslucentTbStatus } from '../native/runtime.ts'
// FREEZE(1B)：TranslucentTB 退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
// import type { AutostartStatus, TranslucentTbStatus } from '../native/runtime.ts'
import type { AutostartStatus } from '../native/runtime.ts'
import * as liteNative from './native.ts'
import { assetUrl, DEFAULT_LITE_SETTINGS, LITE_BACKGROUND_OPTIONS, LITE_PORTRAIT_OPTIONS, loadLiteSettings, saveLiteSettings } from './settings.ts'
import { t, useLanguage } from '../i18n/index.ts'
import { listenUntilDisposed } from '../runtime/lifecycle.ts'
import { autostartDetail, autostartRefusalNotice } from '../settings/autostartCopy.ts'
import { createAutostartQueue, type AutostartQueue } from '../settings/autostartQueue.ts'
import type { LiteSettings } from './types.ts'
import './LiteSettingsWindow.css'

function Toggle({ checked, disabled, label, onChange }: { checked: boolean; disabled?: boolean; label: string; onChange: (value: boolean) => void }) {
  return <button className={`lite-toggle ${checked ? 'is-on' : ''}`} type="button" role="switch" aria-label={label} aria-checked={checked} disabled={disabled} onClick={() => onChange(!checked)}><span /></button>
}

function SettingRow({ title, detail, children }: { title: string; detail?: string; children: ReactNode }) {
  return <div className="lite-setting-row"><div><strong>{title}</strong>{detail && <small>{detail}</small>}</div><div className="lite-setting-control">{children}</div></div>
}

export function LiteSettingsWindow() {
  // 这个窗口的 JSX 与提示条里都有词条，所以语言一变就要重渲染一次。
  useLanguage()
  const [settings, setSettings] = useState<LiteSettings>(DEFAULT_LITE_SETTINGS)
  const settingsRef = useRef(settings)
  const [notice, setNotice] = useState<string>()
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()
  // FREEZE(1B)：随系统集成冻结。
  // const [desktopFallbackStatus, setDesktopFallbackStatus] = useState<liteNative.DesktopWallpaperFallbackStatus>()
  // FREEZE(1B)：随系统集成冻结。
  // const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })
  const [customBackground, setCustomBackground] = useState<string>()
  const [customPortrait, setCustomPortrait] = useState<string>()
  const [autostartBusy, setAutostartBusy] = useState(false)
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   const [lockScreenBusy, setLockScreenBusy] = useState(false)
  // FREEZE(1B)：随系统集成冻结。
  // const [desktopFallbackBusy, setDesktopFallbackBusy] = useState(false)
  // FREEZE(1A)：锁屏退出后没人再用。
  // const lockOperationRef = useRef(false)
  const autostartOperationRef = useRef(false)
  // The state Windows reported, not the state that was requested: the row says
  // which path carries autostart and why, and Lite's page had no such state.
  const [autostartState, setAutostartState] = useState<AutostartStatus>({ enabled: false, source: 'none', reason: null })
  /**
   * What the switch stands for: the user's latest request while one is on the
   * wire, otherwise the state Windows last reported. See `createAutostartQueue`
   * for why a toggle is queued instead of dropped.
   */
  const latestAutostartRef = useRef(DEFAULT_LITE_SETTINGS.autostart)

  settingsRef.current = settings

  const commit = (next: LiteSettings) => {
    settingsRef.current = next
    setSettings(next)
    void saveLiteSettings(next).catch((error) => setNotice(t('lite.settings.save-failed', { error: String(error) })))
  }

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   const refreshDiagnostics = async () => {
  //     try {
  //       const diagnostics = await liteNative.lockScreenDiagnostics()
  //       setLockScreenDiagnostics(diagnostics)
  //       // A user may install Lite while the full edition's shared lock-screen
  //       // recovery point is still active. Mirror Windows' authoritative state
  //       // instead of presenting a misleading unchecked toggle.
  //       const current = settingsRef.current
  //       if (current.lockScreenEnabled !== diagnostics.managedImageActive) {
  //         const next = { ...current, lockScreenEnabled: diagnostics.managedImageActive }
  //         settingsRef.current = next
  //         setSettings(next)
  //         void saveLiteSettings(next).catch((error) => setNotice(`设置同步失败：${String(error)}`))
  //       }
  //     } catch (error) {
  //       setNotice(`锁屏检查失败：${String(error)}`)
  //     }
  //   }

  const refreshAutostart = async () => {
    try {
      const status: AutostartStatus = await liteNative.autostartStatus()
      const current = settingsRef.current
      setAutostartState(status)
      // A toggle is on the wire: its read-back is newer than this probe's.
      if (autostartOperationRef.current) return
      latestAutostartRef.current = status.enabled
      if (status.enabled !== current.autostart) commit({ ...current, autostart: status.enabled })
    } catch (error) {
      setNotice(t('lite.settings.autostart.read-failed', { error: String(error) }))
    }
  }

  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一块随之冻结。恢复办法：取消注释。
  // const refreshDesktopFallback = async () => {
  // try {
  // const status = await liteNative.desktopWallpaperFallbackStatus()
  // setDesktopFallbackStatus(status)
  // const current = settingsRef.current
  // // A successful native takeover is authoritative. If the settings file
  // // says enabled but no recovery point exists, fail closed and clear the
  // // stale preference instead of silently changing the desktop wallpaper.
  // if (status.managedActive && !current.desktopWallpaperFallback) {
  // commit({ ...current, desktopWallpaperFallback: true })
  // } else if (!status.backupExists && current.desktopWallpaperFallback) {
  // commit({ ...current, desktopWallpaperFallback: false })
  // }
  // } catch (error) {
  // setNotice(`读取登录过渡底图状态失败：${String(error)}`)
  // }
  // }

  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一块随之冻结。恢复办法：取消注释。
  // const refreshTranslucentTb = async () => {
  // try {
  // setTranslucentTb(await liteNative.translucentTbStatus())
  // } catch (error) {
  // setNotice(`读取 TranslucentTB 状态失败：${String(error)}`)
  // }
  // }

  const refreshCustomImages = async () => {
    try {
      const [background, portrait] = await Promise.all([
        liteNative.resolveImage('background'),
        liteNative.resolveImage('portrait'),
      ])
      setCustomBackground(background)
      setCustomPortrait(portrait)
    } catch (error) {
      setNotice(t('lite.settings.custom-image.read-failed', { error: String(error) }))
    }
  }

  useEffect(() => {
    void loadLiteSettings().then((loaded) => {
      settingsRef.current = loaded
      setSettings(loaded)
      // FREEZE(1A)：锁屏诊断随锁屏一起退出，这次挂载时的刷新随之冻结。
      // void refreshDiagnostics()
      void refreshAutostart()
      // FREEZE(1B)：随上面两者冻结。
      // void refreshDesktopFallback()
      // FREEZE(1B)：随上面两者冻结。
      // void refreshTranslucentTb()
      void refreshCustomImages()
    }).catch((error) => setNotice(t('lite.settings.load-failed', { error: String(error) })))
    if (!('__TAURI_INTERNALS__' in window)) return
    const current = getCurrentWindow()
    // The close listener is registered without awaiting the native call, so a
    // window that closes before it resolves releases the disposer immediately
    // instead of leaving a listener alive for the rest of the process.
    return listenUntilDisposed<{ preventDefault: () => void }>(
      async (emit) => current.onCloseRequested((event) => emit(event)),
      (event) => {
        event.preventDefault()
        void invoke('hide_settings_window').catch((error) => setNotice(String(error)))
      },
      { onError: (error) => setNotice(String(error)) },
    ).dispose
  }, [])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(undefined), 6500)
    return () => window.clearTimeout(timer)
  }, [notice])

  const autostartQueueRef = useRef<AutostartQueue>()
  const autostartQueue = () => {
    if (!autostartQueueRef.current) {
      autostartQueueRef.current = createAutostartQueue({
        send: (enabled) => liteNative.setAutostart(enabled),
        onBusy: (busy) => {
          autostartOperationRef.current = busy
          setAutostartBusy(busy)
        },
        onSettled: (status, requested) => {
          setAutostartState(status)
          // A newer toggle is already queued: its own read-back decides the
          // switch, and this older answer must not move it back.
          if (latestAutostartRef.current !== requested) return
          latestAutostartRef.current = status.enabled
          commit({ ...settingsRef.current, autostart: status.enabled })
          const refusal = autostartRefusalNotice(status, requested)
          if (refusal) setNotice(refusal)
        },
        onError: (error) => setNotice(t('lite.settings.autostart.update-failed', { error: String(error) })),
      })
    }
    return autostartQueueRef.current
  }

  const setAutostart = (enabled: boolean) => {
    latestAutostartRef.current = enabled
    // The switch follows the user at once; Windows' answer decides whether it
    // stays there. A second toggle is queued, not dropped.
    commit({ ...settingsRef.current, autostart: enabled })
    autostartQueue().request(enabled)
  }

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   const setLockScreen = async (enabled: boolean) => {
  //     if (lockOperationRef.current) return
  //     lockOperationRef.current = true
  //     setLockScreenBusy(true)
  //     try {
  //       const confirmation = await liteNative.setLockScreen(enabled)
  //       commit({ ...settingsRef.current, lockScreenEnabled: enabled })
  //       setNotice(confirmation)
  //       await refreshDiagnostics()
  //     } catch (error) {
  //       setNotice(`${enabled ? '接管锁屏图片' : '恢复原锁屏图片'}失败：${String(error)}`)
  //     } finally {
  //       lockOperationRef.current = false
  //       setLockScreenBusy(false)
  //     }
  //   }

  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30 决定），这一块随之冻结。恢复办法：取消注释。
  // const setDesktopFallback = async (enabled: boolean) => {
  // if (desktopFallbackBusy) return
  // setDesktopFallbackBusy(true)
  // try {
  // const confirmation = await liteNative.setDesktopWallpaperFallback(enabled)
  // commit({ ...settingsRef.current, desktopWallpaperFallback: enabled })
  // setNotice(confirmation)
  // await refreshDesktopFallback()
  // } catch (error) {
  // setNotice(`${enabled ? '启用登录过渡底图' : '关闭登录过渡底图'}失败：${String(error)}`)
  // } finally {
  // setDesktopFallbackBusy(false)
  // }
  // }

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   const openLockScreenSettings = async () => {
  //     try {
  //       await liteNative.openWindowsLockScreenSettings()
  //       setNotice('已打开 Windows 锁屏设置。')
  //     } catch (error) {
  //       setNotice(`无法打开 Windows 锁屏设置：${String(error)}`)
  //     }
  //   }

  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   const clearStaleLockScreenBackup = async () => {
  //     if (lockOperationRef.current) return
  //     if (!window.confirm('清理过期恢复点会永久删除已保存的原锁屏图片副本。Windows 当前锁屏图片不会被修改。确定继续吗？')) return
  //     lockOperationRef.current = true
  //     setLockScreenBusy(true)
  //     try {
  //       setNotice(await liteNative.clearStaleLockScreenBackup(true))
  //       await refreshDiagnostics()
  //     } catch (error) {
  //       setNotice(`清理旧锁屏恢复点失败：${String(error)}`)
  //     } finally {
  //       lockOperationRef.current = false
  //       setLockScreenBusy(false)
  //     }
  //   }

  const chooseCustomImage = async (slot: liteNative.LiteImageSlot) => {
    try {
      const path = await liteNative.chooseImage(slot)
      if (!path) return
      await liteNative.importImage(slot, path)
      if (slot === 'background') {
        const preview = await liteNative.resolveImage(slot)
        setCustomBackground(preview)
        commit({ ...settingsRef.current, background: 'custom' })
      } else {
        const preview = await liteNative.resolveImage(slot)
        setCustomPortrait(preview)
        commit({ ...settingsRef.current, portrait: 'custom' })
      }
      setNotice(slot === 'background' ? t('lite.settings.custom-image.background-imported') : t('lite.settings.custom-image.portrait-imported'))
    } catch (error) {
      setNotice(t('lite.settings.custom-image.import-failed', { error: String(error) }))
    }
  }

  return <main className="lite-settings-window">
    {notice && <div className="lite-notice" role="status"><span>{notice}</span><button type="button" aria-label={t('lite.settings.notice.close')} onClick={() => setNotice(undefined)}>×</button></div>}
    <header className="lite-titlebar">
      <div className="lite-titlebar-drag" onMouseDown={(event) => { if (event.button === 0) void invoke('start_settings_drag') }} />
      <div className="lite-brand"><span className="lite-brand-mark">DSH</span><div><strong>Wallpaper Lite</strong><small>{t('lite.settings.brand.tagline')}</small></div></div>
      <button type="button" className="lite-close" aria-label={t('lite.settings.window.close')} onClick={() => void invoke('hide_settings_window')}>×</button>
    </header>

    <div className="lite-content">
      <section className="lite-hero">
        <div><span className="lite-eyebrow">FIRST RELEASE</span><h1>{t('lite.settings.hero.title')}</h1><p>{t('lite.settings.hero.description')}</p></div>
        <div className="lite-hero-orbit" aria-hidden="true"><span /><span /><span /></div>
      </section>

      <section className="lite-card">
        {/* FREEZE(1A)：这张卡里的"锁屏"状态点随锁屏一起冻结（卡内还留着登录过渡底图与自动启动，所以标题保留）。 */}
        <div className="lite-card-heading"><div><span className="lite-kicker">01 · SYSTEM</span><h2>{t('lite.settings.system.title')}</h2></div></div>
        {/* FREEZE(1A)：锁屏退出，这一行随之冻结。<SettingRow title="接管 Windows 锁屏图片" detail={lockScreenBusy ? '正在应用系统设置，请稍候。' : '密码输入页仍由 Windows 原生处理。'}><Toggle label="接管 Windows 锁屏图片" checked={settings.lockScreenEnabled} disabled={lockScreenBusy} onChange={(value) => void setLockScreen(value)} /></SettingRow> */}
        {/* FREEZE(1B)：登录过渡底图随系统集成一起冻结。<SettingRow title="登录过渡底图" detail={desktopFallbackBusy ? '正在更新 Explorer 桌面底图。' : desktopFallbackStatus?.managedActive ? '已确认 Explorer 正在使用睡眠画面；重启后可减少解锁空档。' : desktopFallbackStatus?.warning ?? '让 Explorer 在应用启动前先显示睡眠画面，减少解锁后的原壁纸空档。'}><Toggle label="登录过渡底图" checked={settings.desktopWallpaperFallback} disabled={desktopFallbackBusy} onChange={(value) => void setDesktopFallback(value)} /></SettingRow> */}
        <SettingRow title={t('lite.settings.autostart.title')} detail={autostartBusy ? t('lite.settings.autostart.busy') : autostartDetail(autostartState)}><Toggle label={t('lite.settings.autostart.title')} checked={settings.autostart} disabled={autostartBusy} onChange={(value) => setAutostart(value)} /></SettingRow>
        {/* FREEZE(1A)：锁屏退出，这一行随之冻结。<div className="lite-actions"><button type="button" onClick={() => void openLockScreenSettings()}>打开 Windows 锁屏设置</button><button type="button" onClick={() => void refreshDiagnostics()}>刷新诊断</button></div> */}
        {/* FREEZE(1A)：锁屏退出，这一行随之冻结。{lockScreenDiagnostics && <div className="lite-diagnostics"><strong>{lockScreenDiagnostics.takeoverAvailable ? '锁屏接管可用' : '当前暂不可接管锁屏'}</strong>{lockScreenDiagnostics.warnings.slice(0, 2).map((warning) => <span key={warning}>{warning}</span>)}{desktopFallbackStatus?.backupExists && desktopFallbackStatus.warning && <span>{desktopFallbackStatus.warning}</span>}{lockScreenDiagnostics.staleBackup && <button type="button" className="lite-diagnostics-action" disabled={lockScreenBusy} onClick={() => void clearStaleLockScreenBackup()}>清理过期恢复点（删除原图副本）</button>}</div>} */}
      </section>

      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">02 · WAKE</span><h2>{t('lite.settings.wake.title')}</h2></div><span className="lite-card-caption">4 FRAME SEQUENCE</span></div>
        <SettingRow title={t('lite.settings.wake.enabled.title')} detail={t('lite.settings.wake.enabled.detail')}><Toggle label={t('lite.settings.wake.enabled.title')} checked={settings.animationsEnabled} onChange={(value) => commit({ ...settingsRef.current, animationsEnabled: value })} /></SettingRow>
        <SettingRow title={t('lite.settings.wake.every-unlock.title')} detail={t('lite.settings.wake.every-unlock.detail')}><Toggle label={t('lite.settings.wake.every-unlock.title')} checked={settings.playWakeOnEveryUnlock} onChange={(value) => commit({ ...settingsRef.current, playWakeOnEveryUnlock: value })} /></SettingRow>
        <SettingRow title={t('lite.settings.wake.skip.title')} detail={t('lite.settings.wake.skip.detail')}><Toggle label={t('lite.settings.wake.skip.title')} checked={settings.skipWakeAnimation} onChange={(value) => commit({ ...settingsRef.current, skipWakeAnimation: value })} /></SettingRow>
        <SettingRow title={t('lite.settings.wake.speed.title')} detail={`${settings.animationSpeed.toFixed(1)}×`}><input className="lite-range" type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(event) => commit({ ...settingsRef.current, animationSpeed: Number(event.target.value) })} /></SettingRow>
      </section>

      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">03 · SCENE</span><h2>{t('lite.settings.scene.title')}</h2></div><span className="lite-card-caption">BUILT-IN CATALOG</span></div>
        <div className="lite-choice-label">{t('lite.settings.scene.background')}</div>
        <div className="lite-option-grid lite-background-grid">{LITE_BACKGROUND_OPTIONS.map((option) => <button type="button" key={option.id} className={settings.background === option.id ? 'is-selected' : ''} onClick={() => commit({ ...settingsRef.current, background: option.id })}><img src={assetUrl(option.path)} alt="" draggable={false} /><span>{option.label}</span>{settings.background === option.id && <i>{t('lite.settings.scene.current')}</i>}</button>)}<button type="button" className={settings.background === 'custom' ? 'is-selected' : ''} onClick={() => void chooseCustomImage('background')}><img src={customBackground ?? assetUrl(LITE_BACKGROUND_OPTIONS[0].path)} alt="" draggable={false} /><span>{t('lite.settings.scene.custom-background')}</span>{settings.background === 'custom' && <i>{t('lite.settings.scene.current')}</i>}</button></div>
        <div className="lite-choice-label">{t('lite.settings.scene.portrait')}</div>
        <div className="lite-option-grid lite-portrait-grid">{LITE_PORTRAIT_OPTIONS.map((option) => <button type="button" key={option.id} className={settings.portrait === option.id ? 'is-selected' : ''} onClick={() => commit({ ...settingsRef.current, portrait: option.id })}><img src={assetUrl(option.path)} alt="" draggable={false} /><span>{option.label}</span>{settings.portrait === option.id && <i>{t('lite.settings.scene.current')}</i>}</button>)}<button type="button" className={settings.portrait === 'custom' ? 'is-selected' : ''} onClick={() => void chooseCustomImage('portrait')}><img src={customPortrait ?? assetUrl(LITE_PORTRAIT_OPTIONS[0].path)} alt="" draggable={false} /><span>{t('lite.settings.scene.custom-portrait')}</span>{settings.portrait === 'custom' && <i>{t('lite.settings.scene.current')}</i>}</button></div>
        <p className="lite-footnote">{t('lite.settings.scene.footnote')}</p>
      </section>

      {/* FREEZE(1B)：TranslucentTB 卡片随系统集成一起冻结。恢复办法：去掉这对注释，并恢复状态与刷新函数。
      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">04 · COMPATIBILITY</span><h2>TranslucentTB</h2></div><span className={`lite-pill ${translucentTb.running ? 'is-online' : ''}`}>{translucentTb.running ? '运行中' : translucentTb.installed ? '已安装' : '未检测到'}</span></div>
        <p className="lite-card-description">Lite 不修改任务栏，只提供与 TranslucentTB 的兼容入口。任务栏透明效果由 TranslucentTB 自己管理。</p>
        <div className="lite-actions"><button type="button" disabled={!translucentTb.installed || translucentTb.running} onClick={() => void liteNative.launchTranslucentTb().then(refreshTranslucentTb).catch((error) => setNotice(String(error)))}>启动 TranslucentTB</button><button type="button" onClick={() => void liteNative.openTranslucentTbInstall().catch((error) => setNotice(String(error)))}>前往 Microsoft Store</button><button type="button" onClick={() => void refreshTranslucentTb()}>刷新状态</button></div>
      </section>
      */}

      <footer className="lite-footer"><span>DSH Wallpaper Lite · Windows 11</span><span>{t('lite.settings.footer.autosave')}</span></footer>
    </div>
  </main>
}
