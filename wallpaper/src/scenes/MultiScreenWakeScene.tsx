/**
 * Wake animation rendered independently inside every physical display's
 * rectangle. The background host is still one WorkerW/WebView; duplicating
 * only the four bitmap layers prevents a wide virtual desktop from stretching
 * one animation across monitors with different aspect ratios.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import type { DesktopDisplayInfo } from '../native/runtime.ts'
import { displayCssRect, virtualDesktopBounds } from '../runtime/displayLayout.ts'
import { placeholderPortrait } from '../ui/whale.ts'
import { WAKE_CURTAIN_IN_MS, WAKE_FRAME_DURATIONS, useWakeCurtain, useWakeFramePreload, wakeFrameSources } from './WakeScene.tsx'
import { decodeImageSource } from '../native/bootstrapHandoff.ts'
import './MultiScreenWakeScene.css'

export interface MultiScreenWakeSceneProps {
  displays: readonly DesktopDisplayInfo[]
  persona: PersonaManifest
  onWakeDone: () => void
  onFirstWakeFrame?: (generation: number) => void | boolean | Promise<void | boolean>
  handoffGeneration?: number
  startIndex?: number
  enabled?: boolean
  speed?: number
}

export function MultiScreenWakeScene({
  displays,
  persona,
  onWakeDone,
  onFirstWakeFrame,
  handoffGeneration,
  startIndex = 0,
  enabled = true,
  speed = 1,
}: MultiScreenWakeSceneProps) {
  const frames = wakeFrameSources(persona)
  const initialIndex = Math.min(Math.max(0, startIndex), Math.max(0, frames.length - 1))
  const [index, setIndex] = useState(initialIndex)
  const [fallbackToStatic, setFallbackToStatic] = useState(false)
  const frameIndexRef = useRef(initialIndex)
  const onWakeDoneRef = useRef(onWakeDone)
  const onFirstWakeFrameRef = useRef(onFirstWakeFrame)
  const reportedGenerationRef = useRef<number>()
  onWakeDoneRef.current = onWakeDone
  const { curtain, dropCurtain } = useWakeCurtain(onWakeDone)
  onFirstWakeFrameRef.current = onFirstWakeFrame

  const virtualBounds = useMemo(() => virtualDesktopBounds(displays), [displays])
  const image = useMemo(
    () => persona.assets.wake ?? placeholderPortrait(persona.kind, 'wake'),
    [persona.assets.wake, persona.kind],
  )
  const hasFrames = frames.length > 1
  useWakeFramePreload(frames, enabled && hasFrames)

  useEffect(() => {
    if (!hasFrames || index < 1 || handoffGeneration === undefined || reportedGenerationRef.current === handoffGeneration) return
    const generation = handoffGeneration
    const source = fallbackToStatic ? image : frames[index]
    if (!source) return
    let cancelled = false
    let first = 0
    let second = 0
    void decodeImageSource(source).then((ready) => {
      if (cancelled) return
      if (!ready) {
        if (!fallbackToStatic) setFallbackToStatic(true)
        return
      }
      first = requestAnimationFrame(() => {
        second = requestAnimationFrame(() => {
          if (cancelled) return
          void Promise.resolve(onFirstWakeFrameRef.current?.(generation)).then((released) => {
            if (!cancelled && released !== false) reportedGenerationRef.current = generation
          }).catch((error) => console.warn('multi-screen wake frame hand-off callback failed', error))
        })
      })
    })
    return () => {
      cancelled = true
      cancelAnimationFrame(first)
      cancelAnimationFrame(second)
    }
  }, [fallbackToStatic, frames, handoffGeneration, hasFrames, image, index])

  useEffect(() => {
    if (!enabled) {
      const immediate = setTimeout(() => onWakeDoneRef.current(), 0)
      return () => clearTimeout(immediate)
    }
    if (!hasFrames) {
      const timer = setTimeout(dropCurtain, 2400)
      return () => clearTimeout(timer)
    }
    const duration = WAKE_FRAME_DURATIONS[Math.min(frameIndexRef.current, WAKE_FRAME_DURATIONS.length - 1)] / Math.max(0.25, speed)
    const timer = setTimeout(() => {
      if (frameIndexRef.current >= frames.length - 1) {
        dropCurtain()
      } else {
        frameIndexRef.current += 1
        setIndex(frameIndexRef.current)
      }
    }, duration)
    return () => clearTimeout(timer)
  }, [enabled, frames, hasFrames, index, speed])

  const targetDisplays = displays.length > 0 ? displays : undefined
  return <div className="scene multi-screen-wake-scene">
    {targetDisplays ? targetDisplays.map((display) => <div
      key={display.id}
      className="multi-screen-wake-surface"
      style={displayCssRect(display, virtualBounds)}
      data-display-id={display.id}
    >
      {hasFrames && !fallbackToStatic ? frames.map((frame, frameNumber) => <img
        key={frame}
        className={`multi-screen-wake-frame ${frameNumber === index ? 'active' : ''}`}
        src={frame}
        alt={`苏醒 ${frameNumber + 1}`}
        draggable={false}
      />) : <img className={`multi-screen-wake-art multi-screen-wake-art-${Math.min(index, 3)}`} src={image} alt="苏醒的鲸鱼娘" draggable={false} />}
    </div>) : <div className="multi-screen-wake-surface multi-screen-wake-surface--virtual">
      {hasFrames && !fallbackToStatic ? frames.map((frame, frameNumber) => <img
        key={frame}
        className={`multi-screen-wake-frame ${frameNumber === index ? 'active' : ''}`}
        src={frame}
        alt={`苏醒 ${frameNumber + 1}`}
        draggable={false}
      />) : <img className={`multi-screen-wake-art multi-screen-wake-art-${Math.min(index, 3)}`} src={image} alt="苏醒的鲸鱼娘" draggable={false} />}
    </div>}
    {curtain && <div className="wake-curtain" style={{ animationDuration: `${WAKE_CURTAIN_IN_MS}ms` }} />}
  </div>
}
