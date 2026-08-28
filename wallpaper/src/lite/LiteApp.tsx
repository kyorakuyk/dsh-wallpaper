import { useEffect, useMemo, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { SleepScene } from '../scenes/SleepScene.tsx'
import { WakeScene } from '../scenes/WakeScene.tsx'
import { releaseNativeBootstrap } from './native.ts'
import { liteRuntime } from './runtime.ts'
import { LiteIdleScene } from './LiteIdleScene.tsx'
import { assetUrl, DEFAULT_LITE_SETTINGS, LITE_BACKGROUND_OPTIONS, LITE_PORTRAIT_OPTIONS, loadLiteSettings, normalizeLiteSettings } from './settings.ts'
import type { LiteSettings } from './types.ts'
import * as liteNative from './native.ts'
import { litePersona } from './persona.ts'
import './LiteScene.css'

type LitePhase = 'booting' | 'locked' | 'waking' | 'idle'

function phaseFromSnapshot(phase: string): LitePhase {
  if (phase === 'locked') return 'locked'
  if (phase === 'waking') return 'waking'
  if (phase === 'booting') return 'booting'
  return 'idle'
}

export function LiteApp() {
  const [settings, setSettings] = useState<LiteSettings>(DEFAULT_LITE_SETTINGS)
  const [phase, setPhase] = useState<LitePhase>('booting')
  const [customBackground, setCustomBackground] = useState<string>()
  const [customPortrait, setCustomPortrait] = useState<string>()
  const settingsRef = useRef(settings)
  const phaseRef = useRef(phase)
  const suppressNativeWakeRef = useRef(false)
  settingsRef.current = settings
  phaseRef.current = phase

  useEffect(() => {
    let disposed = false
    void loadLiteSettings().then((next) => {
      if (!disposed) setSettings(next)
    })
    return () => { disposed = true }
  }, [])

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) {
      const timer = window.setTimeout(() => setPhase(settingsRef.current.animationsEnabled && !settingsRef.current.skipWakeAnimation ? 'waking' : 'idle'), 120)
      return () => window.clearTimeout(timer)
    }
    let unsubscribe: () => void = () => undefined
    let disposed = false
    const applySnapshot = (snapshot: Awaited<ReturnType<typeof liteRuntime.snapshot>>) => {
      if (snapshot.phase === 'waking' && suppressNativeWakeRef.current) return
      if (!disposed) setPhase(phaseFromSnapshot(snapshot.phase))
    }
    void Promise.all([
      liteRuntime.snapshot().then(applySnapshot),
      liteRuntime.subscribe(applySnapshot).then((dispose) => { unsubscribe = dispose }),
    ])
    const timer = window.setTimeout(() => {
      void liteRuntime.dispatch('boot-ready', settingsRef.current.animationsEnabled && !settingsRef.current.skipWakeAnimation)
    }, 120)
    return () => {
      disposed = true
      window.clearTimeout(timer)
      unsubscribe()
    }
  }, [])

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return
    let disposed = false
    let dispose: () => void = () => undefined
    void listen<LiteSettings>('settings-changed', (event) => {
      if (!disposed) setSettings(normalizeLiteSettings(event.payload))
    }).then((unlisten) => { dispose = unlisten })
    return () => {
      disposed = true
      dispose()
    }
  }, [])

  useEffect(() => {
    let disposed = false
    const refresh = async () => {
      const [background, portrait] = await Promise.all([
        settings.background === 'custom' ? liteNative.resolveImage('background') : Promise.resolve(undefined),
        settings.portrait === 'custom' ? liteNative.resolveImage('portrait') : Promise.resolve(undefined),
      ])
      if (!disposed) {
        setCustomBackground(background)
        setCustomPortrait(portrait)
      }
    }
    void refresh()
    return () => { disposed = true }
  }, [settings.background, settings.portrait])

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return
    let first = 0
    let second = 0
    first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => { void releaseNativeBootstrap() })
    })
    return () => {
      cancelAnimationFrame(first)
      cancelAnimationFrame(second)
    }
  }, [])

  const background = LITE_BACKGROUND_OPTIONS.find((option) => option.id === settings.background) ?? LITE_BACKGROUND_OPTIONS[0]
  const portrait = LITE_PORTRAIT_OPTIONS.find((option) => option.id === settings.portrait) ?? LITE_PORTRAIT_OPTIONS[0]
  const backgroundUrl = settings.background === 'custom' ? customBackground : assetUrl(background.path)
  const portraitUrl = settings.portrait === 'custom' ? customPortrait : assetUrl(portrait.path)
  const persona = useMemo(() => litePersona(settings.portrait, portraitUrl), [settings.portrait, portraitUrl])

  const wakeDone = () => {
    setPhase('idle')
    if ('__TAURI_INTERNALS__' in window) void liteRuntime.dispatch('wake-done')
  }

  const unlockPreview = (everyUnlock = true) => {
    const playWake = settingsRef.current.animationsEnabled
      && !settingsRef.current.skipWakeAnimation
      && (everyUnlock ? settingsRef.current.playWakeOnEveryUnlock : true)
    setPhase(playWake ? 'waking' : 'idle')
    if ('__TAURI_INTERNALS__' in window) {
      suppressNativeWakeRef.current = !playWake
      void liteRuntime.dispatch('unlock', playWake).then((snapshot) => {
        suppressNativeWakeRef.current = false
        setPhase(phaseFromSnapshot(snapshot.phase))
      }).catch(() => {
        suppressNativeWakeRef.current = false
      })
    }
  }

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) {
      const onKey = (event: KeyboardEvent) => {
        if (event.key === 'Escape' && phaseRef.current === 'locked') unlockPreview()
      }
      window.addEventListener('keydown', onKey)
      return () => window.removeEventListener('keydown', onKey)
    }
    let disposed = false
    let dispose: () => void = () => undefined
    let observedLockOrSuspend = false
    void listen<'locked' | 'unlocked' | 'suspend' | 'resume'>('system-session', (event) => {
      if (disposed) return
      if (event.payload === 'locked' || event.payload === 'suspend') {
        observedLockOrSuspend = true
        suppressNativeWakeRef.current = false
        setPhase('locked')
        return
      }
      if (event.payload === 'resume' && !observedLockOrSuspend) return
      if (event.payload !== 'unlocked' && event.payload !== 'resume') return
      // Rust has already moved AppCore to waking for a real unlock. If the
      // user disabled the animation, explicitly settle it back to idle.
      unlockPreview(true)
      observedLockOrSuspend = false
    }).then((unlisten) => { dispose = unlisten })
    return () => {
      disposed = true
      dispose()
    }
  }, [])

  let scene
  if (phase === 'booting' || phase === 'locked') {
    scene = <SleepScene persona={persona} mode="system" quiet />
  } else if (phase === 'waking') {
    scene = <WakeScene persona={persona} enabled={settings.animationsEnabled && !settings.skipWakeAnimation} speed={settings.animationSpeed} onWakeDone={wakeDone} />
  } else {
    scene = <LiteIdleScene persona={persona} backgroundUrl={backgroundUrl} />
  }

  return <div className={`wallpaper-root lite-wallpaper-root lite-phase-${phase}`} data-edition="lite">{scene}</div>
}
