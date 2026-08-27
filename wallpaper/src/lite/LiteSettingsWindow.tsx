import { useEffect, useRef, useState, type ReactNode } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import type { AutostartStatus, LockScreenDiagnostics, TranslucentTbStatus } from '../native/runtime.ts'
import * as liteNative from './native.ts'
import { assetUrl, DEFAULT_LITE_SETTINGS, LITE_BACKGROUND_OPTIONS, LITE_PORTRAIT_OPTIONS, loadLiteSettings, saveLiteSettings } from './settings.ts'
import type { LiteSettings } from './types.ts'
import './LiteSettingsWindow.css'

function Toggle({ checked, disabled, label, onChange }: { checked: boolean; disabled?: boolean; label: string; onChange: (value: boolean) => void }) {
  return <button className={`lite-toggle ${checked ? 'is-on' : ''}`} type="button" role="switch" aria-label={label} aria-checked={checked} disabled={disabled} onClick={() => onChange(!checked)}><span /></button>
}

function SettingRow({ title, detail, children }: { title: string; detail?: string; children: ReactNode }) {
  return <div className="lite-setting-row"><div><strong>{title}</strong>{detail && <small>{detail}</small>}</div><div className="lite-setting-control">{children}</div></div>
}

export function LiteSettingsWindow() {
  const [settings, setSettings] = useState<LiteSettings>(DEFAULT_LITE_SETTINGS)
  const settingsRef = useRef(settings)
  const [notice, setNotice] = useState<string>()
  const [lockScreenDiagnostics, setLockScreenDiagnostics] = useState<LockScreenDiagnostics>()
  const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })
  const [customBackground, setCustomBackground] = useState<string>()
  const [customPortrait, setCustomPortrait] = useState<string>()
  const [autostartBusy, setAutostartBusy] = useState(false)
  const [lockScreenBusy, setLockScreenBusy] = useState(false)
  const lockOperationRef = useRef(false)
  const autostartOperationRef = useRef(false)

  settingsRef.current = settings

  const commit = (next: LiteSettings) => {
    settingsRef.current = next
    setSettings(next)
    void saveLiteSettings(next).catch((error) => setNotice(`设置保存失败：${String(error)}`))
  }

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

  const refreshAutostart = async () => {
    try {
      const status: AutostartStatus = await liteNative.autostartStatus()
      const current = settingsRef.current
      if (status.enabled !== current.autostart) commit({ ...current, autostart: status.enabled })
    } catch (error) {
      setNotice(`读取开机自启状态失败：${String(error)}`)
    }
  }

  const refreshTranslucentTb = async () => {
    try {
      setTranslucentTb(await liteNative.translucentTbStatus())
    } catch (error) {
      setNotice(`读取 TranslucentTB 状态失败：${String(error)}`)
    }
  }

  const refreshCustomImages = async () => {
    try {
      const [background, portrait] = await Promise.all([
        liteNative.resolveImage('background'),
        liteNative.resolveImage('portrait'),
      ])
      setCustomBackground(background)
      setCustomPortrait(portrait)
    } catch (error) {
      setNotice(`读取自定义素材失败：${String(error)}`)
    }
  }

  useEffect(() => {
    void loadLiteSettings().then((loaded) => {
      settingsRef.current = loaded
      setSettings(loaded)
      void refreshDiagnostics()
      void refreshAutostart()
      void refreshTranslucentTb()
      void refreshCustomImages()
    })
    if (!('__TAURI_INTERNALS__' in window)) return
    const current = getCurrentWindow()
    const closeListener = current.onCloseRequested((event) => {
      event.preventDefault()
      void invoke('hide_settings_window')
    })
    return () => { void closeListener.then((dispose) => dispose()) }
  }, [])

  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(undefined), 6500)
    return () => window.clearTimeout(timer)
  }, [notice])

  const setAutostart = async (enabled: boolean) => {
    if (autostartOperationRef.current) return
    autostartOperationRef.current = true
    setAutostartBusy(true)
    try {
      const status = await liteNative.setAutostart(enabled)
      commit({ ...settingsRef.current, autostart: status.enabled })
      if (status.enabled !== enabled) setNotice('Windows 没有接受这次开机自启变更，请检查系统启动应用权限。')
    } catch (error) {
      setNotice(`开机自启更新失败：${String(error)}`)
    } finally {
      autostartOperationRef.current = false
      setAutostartBusy(false)
    }
  }

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

  const openLockScreenSettings = async () => {
    try {
      await liteNative.openWindowsLockScreenSettings()
      setNotice('已打开 Windows 锁屏设置。')
    } catch (error) {
      setNotice(`无法打开 Windows 锁屏设置：${String(error)}`)
    }
  }

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
      setNotice(slot === 'background' ? '已导入自定义壁纸背景。' : '已导入自定义立绘。')
    } catch (error) {
      setNotice(`导入图片失败：${String(error)}`)
    }
  }

  return <main className="lite-settings-window">
    {notice && <div className="lite-notice" role="status"><span>{notice}</span><button type="button" aria-label="关闭通知" onClick={() => setNotice(undefined)}>×</button></div>}
    <header className="lite-titlebar">
      <div className="lite-titlebar-drag" onMouseDown={(event) => { if (event.button === 0) void invoke('start_settings_drag') }} />
      <div className="lite-brand"><span className="lite-brand-mark">DSH</span><div><strong>Wallpaper Lite</strong><small>轻量桌面壁纸</small></div></div>
      <button type="button" className="lite-close" aria-label="关闭设置" onClick={() => void invoke('hide_settings_window')}>×</button>
    </header>

    <div className="lite-content">
      <section className="lite-hero">
        <div><span className="lite-eyebrow">FIRST RELEASE</span><h1>让桌面安静地醒来</h1><p>只保留锁屏、苏醒动画、壁纸与立绘。Windows 密码页仍由系统负责。</p></div>
        <div className="lite-hero-orbit" aria-hidden="true"><span /><span /><span /></div>
      </section>

      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">01 · SYSTEM</span><h2>锁屏与启动</h2></div><span className={`lite-status-dot ${lockScreenDiagnostics?.managedImageActive ? 'is-active' : ''}`} /></div>
        <SettingRow title="接管 Windows 锁屏图片" detail={lockScreenBusy ? '正在应用系统设置，请稍候。' : '密码输入页仍由 Windows 原生处理。'}><Toggle label="接管 Windows 锁屏图片" checked={settings.lockScreenEnabled} disabled={lockScreenBusy} onChange={(value) => void setLockScreen(value)} /></SettingRow>
        <SettingRow title="登录后自动启动" detail={autostartBusy ? '正在更新启动任务。' : '使用当前用户的 Windows 启动任务。'}><Toggle label="登录后自动启动" checked={settings.autostart} disabled={autostartBusy} onChange={(value) => void setAutostart(value)} /></SettingRow>
        <div className="lite-actions"><button type="button" onClick={() => void openLockScreenSettings()}>打开 Windows 锁屏设置</button><button type="button" onClick={() => void refreshDiagnostics()}>刷新诊断</button></div>
        {lockScreenDiagnostics && <div className="lite-diagnostics"><strong>{lockScreenDiagnostics.takeoverAvailable ? '锁屏接管可用' : '当前暂不可接管锁屏'}</strong>{lockScreenDiagnostics.warnings.slice(0, 2).map((warning) => <span key={warning}>{warning}</span>)}</div>}
      </section>

      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">02 · WAKE</span><h2>苏醒动画</h2></div><span className="lite-card-caption">4 FRAME SEQUENCE</span></div>
        <SettingRow title="启用苏醒动画" detail="解锁后播放正式四帧素材。"><Toggle label="启用苏醒动画" checked={settings.animationsEnabled} onChange={(value) => commit({ ...settingsRef.current, animationsEnabled: value })} /></SettingRow>
        <SettingRow title="每次解锁播放" detail="关闭后只在应用启动时播放一次。"><Toggle label="每次解锁播放" checked={settings.playWakeOnEveryUnlock} onChange={(value) => commit({ ...settingsRef.current, playWakeOnEveryUnlock: value })} /></SettingRow>
        <SettingRow title="跳过动画" detail="直接进入静态壁纸与立绘。"><Toggle label="跳过动画" checked={settings.skipWakeAnimation} onChange={(value) => commit({ ...settingsRef.current, skipWakeAnimation: value })} /></SettingRow>
        <SettingRow title="动画速度" detail={`${settings.animationSpeed.toFixed(1)}×`}><input className="lite-range" type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(event) => commit({ ...settingsRef.current, animationSpeed: Number(event.target.value) })} /></SettingRow>
      </section>

      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">03 · SCENE</span><h2>壁纸与立绘</h2></div><span className="lite-card-caption">BUILT-IN CATALOG</span></div>
        <div className="lite-choice-label">壁纸背景</div>
        <div className="lite-option-grid lite-background-grid">{LITE_BACKGROUND_OPTIONS.map((option) => <button type="button" key={option.id} className={settings.background === option.id ? 'is-selected' : ''} onClick={() => commit({ ...settingsRef.current, background: option.id })}><img src={assetUrl(option.path)} alt="" draggable={false} /><span>{option.label}</span>{settings.background === option.id && <i>当前</i>}</button>)}<button type="button" className={settings.background === 'custom' ? 'is-selected' : ''} onClick={() => void chooseCustomImage('background')}><img src={customBackground ?? assetUrl(LITE_BACKGROUND_OPTIONS[0].path)} alt="" draggable={false} /><span>自定义背景 · 选择文件</span>{settings.background === 'custom' && <i>当前</i>}</button></div>
        <div className="lite-choice-label">右侧立绘</div>
        <div className="lite-option-grid lite-portrait-grid">{LITE_PORTRAIT_OPTIONS.map((option) => <button type="button" key={option.id} className={settings.portrait === option.id ? 'is-selected' : ''} onClick={() => commit({ ...settingsRef.current, portrait: option.id })}><img src={assetUrl(option.path)} alt="" draggable={false} /><span>{option.label}</span>{settings.portrait === option.id && <i>当前</i>}</button>)}<button type="button" className={settings.portrait === 'custom' ? 'is-selected' : ''} onClick={() => void chooseCustomImage('portrait')}><img src={customPortrait ?? assetUrl(LITE_PORTRAIT_OPTIONS[0].path)} alt="" draggable={false} /><span>自定义立绘 · 选择文件</span>{settings.portrait === 'custom' && <i>当前</i>}</button></div>
        <p className="lite-footnote">首发版默认使用正式内置素材，也可分别导入一张背景和一张立绘；主题包、插件和逐项替换会在完整版中提供。</p>
      </section>

      <section className="lite-card">
        <div className="lite-card-heading"><div><span className="lite-kicker">04 · COMPATIBILITY</span><h2>TranslucentTB</h2></div><span className={`lite-pill ${translucentTb.running ? 'is-online' : ''}`}>{translucentTb.running ? '运行中' : translucentTb.installed ? '已安装' : '未检测到'}</span></div>
        <p className="lite-card-description">Lite 不修改任务栏，只提供与 TranslucentTB 的兼容入口。任务栏透明效果由 TranslucentTB 自己管理。</p>
        <div className="lite-actions"><button type="button" disabled={!translucentTb.installed || translucentTb.running} onClick={() => void liteNative.launchTranslucentTb().then(refreshTranslucentTb).catch((error) => setNotice(String(error)))}>启动 TranslucentTB</button><button type="button" onClick={() => void liteNative.openTranslucentTbInstall().catch((error) => setNotice(String(error)))}>前往 Microsoft Store</button><button type="button" onClick={() => void refreshTranslucentTb()}>刷新状态</button></div>
      </section>

      <footer className="lite-footer"><span>DSH Wallpaper Lite · Windows 11</span><span>设置会自动保存</span></footer>
    </div>
  </main>
}
