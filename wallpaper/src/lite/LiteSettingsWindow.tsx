import { useEffect, useRef, useState, type ReactNode } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import type { AutostartStatus } from '../native/runtime.ts'
import * as liteNative from './native.ts'
import { assetUrl, DEFAULT_LITE_SETTINGS, LITE_BACKGROUND_OPTIONS, LITE_PORTRAIT_OPTIONS, loadLiteSettings, saveLiteSettings } from './settings.ts'
import { formatSentence, msg, sentenceOf, t, useLanguage, type Sentence } from '../i18n/index.ts'
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
  /**
   * 通知条上那句话，**没求值**：我们自己写的是 `Message`（渲染期求值 → 切语言会重译），
   * 原生抛出来的那几句是自由文本（`string`，本批不翻译）。
   */
  const [notice, setNotice] = useState<Sentence>()
  const [customBackground, setCustomBackground] = useState<string>()
  const [customPortrait, setCustomPortrait] = useState<string>()
  const [autostartBusy, setAutostartBusy] = useState(false)
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
    void saveLiteSettings(next).catch((error) => setNotice(msg('lite.settings.save-failed', { error: sentenceOf(error) })))
  }

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
      setNotice(msg('lite.settings.autostart.read-failed', { error: sentenceOf(error) }))
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
      setNotice(msg('lite.settings.custom-image.read-failed', { error: sentenceOf(error) }))
    }
  }

  useEffect(() => {
    void loadLiteSettings().then((loaded) => {
      settingsRef.current = loaded
      setSettings(loaded)
      void refreshAutostart()
      void refreshCustomImages()
    }).catch((error) => setNotice(msg('lite.settings.load-failed', { error: sentenceOf(error) })))
    if (!('__TAURI_INTERNALS__' in window)) return
    const current = getCurrentWindow()
    // The close listener is registered without awaiting the native call, so a
    // window that closes before it resolves releases the disposer immediately
    // instead of leaving a listener alive for the rest of the process.
    return listenUntilDisposed<{ preventDefault: () => void }>(
      async (emit) => current.onCloseRequested((event) => emit(event)),
      (event) => {
        event.preventDefault()
        void invoke('hide_settings_window').catch((error) => setNotice(sentenceOf(error)))
      },
      { onError: (error) => setNotice(sentenceOf(error)) },
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
        onError: (error) => setNotice(msg('lite.settings.autostart.update-failed', { error: sentenceOf(error) })),
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
      setNotice(msg('lite.settings.custom-image.import-failed', { error: sentenceOf(error) }))
    }
  }

  return <main className="lite-settings-window">
    {notice && <div className="lite-notice" role="status"><span>{formatSentence(notice)}</span><button type="button" aria-label={t('lite.settings.notice.close')} onClick={() => setNotice(undefined)}>×</button></div>}
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
        <div className="lite-card-heading"><div><span className="lite-kicker">01 · SYSTEM</span><h2>{t('lite.settings.system.title')}</h2></div></div>
        <SettingRow title={t('lite.settings.autostart.title')} detail={autostartBusy ? t('lite.settings.autostart.busy') : autostartDetail(autostartState)}><Toggle label={t('lite.settings.autostart.title')} checked={settings.autostart} disabled={autostartBusy} onChange={(value) => setAutostart(value)} /></SettingRow>
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

      <footer className="lite-footer"><span>DSH Wallpaper Lite · Windows 11</span><span>{t('lite.settings.footer.autosave')}</span></footer>
    </div>
  </main>
}
