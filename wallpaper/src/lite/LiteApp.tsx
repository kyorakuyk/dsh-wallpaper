import { useEffect, useMemo, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { SleepScene } from '../scenes/SleepScene.tsx'
import { WAKE_CURTAIN_OUT_MS, WAKE_ENTER_MS, WakeScene } from '../scenes/WakeScene.tsx'
import { reportNativeBootstrapReady } from '../native/bootstrapHandoff.ts'
import { liteRuntime } from './runtime.ts'
import { LiteIdleScene } from './LiteIdleScene.tsx'
import { assetUrl, DEFAULT_LITE_SETTINGS, LITE_BACKGROUND_OPTIONS, LITE_PORTRAIT_OPTIONS, loadLiteSettings, normalizeLiteSettings } from './settings.ts'
import type { LiteSettings } from './types.ts'
import * as liteNative from './native.ts'
import { litePersona } from './persona.ts'
import { listenUntilDisposed } from '../runtime/lifecycle.ts'
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
  const [nativeHandoffGeneration, setNativeHandoffGeneration] = useState<number>()
  /** 苏醒收尾这一幕是否还在演；时间到就摘掉，不留成常驻状态。 */
  const [wakeEnter, setWakeEnter] = useState(false)
  const settingsRef = useRef(settings)
  const settingsLoadRef = useRef<Promise<LiteSettings>>()
  const phaseRef = useRef(phase)
  const suppressNativeWakeRef = useRef(false)
  settingsRef.current = settings
  phaseRef.current = phase

  useEffect(() => {
    let disposed = false
    const loading = settingsLoadRef.current ?? (settingsLoadRef.current = loadLiteSettings())
    void loading.then((next) => {
      if (!disposed) setSettings(next)
    })
    return () => { disposed = true }
  }, [])

  useEffect(() => {
    if (!wakeEnter) return
    const timer = window.setTimeout(() => setWakeEnter(false), WAKE_ENTER_MS)
    return () => window.clearTimeout(timer)
  }, [wakeEnter])

  useEffect(() => {
    const loading = settingsLoadRef.current ?? (settingsLoadRef.current = loadLiteSettings())
    if (!('__TAURI_INTERNALS__' in window)) {
      let disposed = false
      void loading.then((next) => {
        if (!disposed) setPhase(next.animationsEnabled && !next.skipWakeAnimation ? 'waking' : 'idle')
      })
      return () => { disposed = true }
    }
    const applySnapshot = (snapshot: Awaited<ReturnType<typeof liteRuntime.snapshot>>) => {
      if (snapshot.phase === 'waking' && suppressNativeWakeRef.current) return
      setPhase(phaseFromSnapshot(snapshot.phase))
    }
    const listener = listenUntilDisposed(
      (onSnapshot) => liteRuntime.subscribe(onSnapshot),
      applySnapshot,
    )
    void liteRuntime.snapshot().then(async (snapshot) => {
      applySnapshot(snapshot)
      const loaded = await loading
      if (snapshot.phase !== 'booting') return
      const next = await liteRuntime.dispatch(
        'boot-ready',
        loaded.animationsEnabled && !loaded.skipWakeAnimation,
      )
      applySnapshot(next)
    }).catch(() => undefined)
    return () => listener.dispose()
  }, [])

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void listen<number>('native-handoff-generation', (event) => {
      setNativeHandoffGeneration((current) => Math.max(current ?? 0, event.payload))
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch((error) => console.warn('lite native hand-off generation listener failed', error))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return
    let disposed = false
    void liteNative.nativeBootstrapGeneration().then((generation) => {
      if (!disposed) setNativeHandoffGeneration((current) => Math.max(current ?? 0, generation))
    }).catch((error) => console.warn('lite native hand-off generation query failed', error))
    return () => { disposed = true }
  }, [phase])

  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return
    return listenUntilDisposed<LiteSettings>(
      (emit) => listen<LiteSettings>('settings-changed', (event) => emit(event.payload)),
      (payload) => setSettings(normalizeLiteSettings(payload)),
    ).dispose
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
    if (!('__TAURI_INTERNALS__' in window) || phase !== 'idle' || nativeHandoffGeneration === undefined) return
    const controller = new AbortController()
    void reportNativeBootstrapReady(nativeHandoffGeneration, liteNative, { signal: controller.signal })
      .then((released) => {
        if (!released && !controller.signal.aborted) console.warn('lite native hand-off remains covered until its host or renderer is ready')
      })
      .catch((error) => console.warn('lite native hand-off readiness check failed', error))
    return () => controller.abort()
  }, [nativeHandoffGeneration, phase])

  const background = LITE_BACKGROUND_OPTIONS.find((option) => option.id === settings.background) ?? LITE_BACKGROUND_OPTIONS[0]
  const portrait = LITE_PORTRAIT_OPTIONS.find((option) => option.id === settings.portrait) ?? LITE_PORTRAIT_OPTIONS[0]
  const backgroundUrl = settings.background === 'custom' ? customBackground : assetUrl(background.path)
  const portraitUrl = settings.portrait === 'custom' ? customPortrait : assetUrl(portrait.path)
  const persona = useMemo(() => litePersona(settings.portrait, portraitUrl), [settings.portrait, portraitUrl])

  const wakeDone = () => {
    // 与完整版同一套收尾：动画的最后一帧压黑，桌面在帷幕下面挂载，帷幕淡出即背景淡入，
    // 立绘随后从虚影里浮出。跳动画时不加这一幕。
    if (settingsRef.current.animationsEnabled && !settingsRef.current.skipWakeAnimation) setWakeEnter(true)
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
    let observedLockOrSuspend = false
    const listener = listenUntilDisposed<'locked' | 'unlocked' | 'suspend' | 'resume'>(
      async (emit) => {
        const { listen: listenEvent } = await import('@tauri-apps/api/event')
        return listenEvent<'locked' | 'unlocked' | 'suspend' | 'resume'>('system-session', (event) => emit(event.payload))
      },
      (payload) => {
        if (payload === 'locked' || payload === 'suspend') {
          observedLockOrSuspend = true
          suppressNativeWakeRef.current = false
          setPhase('locked')
          return
        }
        if (payload === 'resume' && !observedLockOrSuspend) return
        if (payload !== 'unlocked' && payload !== 'resume') return
        // Rust has already moved AppCore to waking for a real unlock. If the
        // user disabled the animation, explicitly settle it back to idle.
        unlockPreview(true)
        observedLockOrSuspend = false
      },
    )
    return () => listener.dispose()
  }, [])

  let scene
  if (phase === 'booting' || phase === 'locked') {
    scene = <SleepScene persona={persona} mode="system" quiet />
  } else if (phase === 'waking') {
    // 从熟睡那一帧开始播（与 App.tsx 同一处改动；旧的 1 = 睁眼，理由是锁屏已显示过 sleep.png）。
    scene = <WakeScene persona={persona} startIndex={0} handoffGeneration={nativeHandoffGeneration} enabled={settings.animationsEnabled && !settings.skipWakeAnimation} speed={settings.animationSpeed} onFirstWakeFrame={(generation) => reportNativeBootstrapReady(generation, liteNative, { verifySceneImages: false })} onWakeDone={wakeDone} />
  } else {
    scene = <LiteIdleScene persona={persona} backgroundUrl={backgroundUrl} />
  }

  return <div className={`wallpaper-root lite-wallpaper-root lite-phase-${phase}${wakeEnter ? ' wake-enter' : ''}`} data-edition="lite">{scene}{wakeEnter && <div className="wake-curtain-out" style={{ animationDuration: `${WAKE_CURTAIN_OUT_MS}ms` }} />}</div>
}
