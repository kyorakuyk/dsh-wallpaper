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
import { WAKE_FRAME_DURATIONS, wakeFrameSources } from './WakeScene.tsx'
import './MultiScreenWakeScene.css'

export interface MultiScreenWakeSceneProps {
  displays: readonly DesktopDisplayInfo[]
  persona: PersonaManifest
  onWakeDone: () => void
  onFirstWakeFrame?: () => void
  startIndex?: number
  enabled?: boolean
  speed?: number
}

export function MultiScreenWakeScene({
  displays,
  persona,
  onWakeDone,
  onFirstWakeFrame,
  startIndex = 0,
  enabled = true,
  speed = 1,
}: MultiScreenWakeSceneProps) {
  const frames = wakeFrameSources(persona)
  const initialIndex = Math.min(Math.max(0, startIndex), Math.max(0, frames.length - 1))
  const [index, setIndex] = useState(initialIndex)
  const frameIndexRef = useRef(initialIndex)
  const onWakeDoneRef = useRef(onWakeDone)
  const onFirstWakeFrameRef = useRef(onFirstWakeFrame)
  const firstWakeFrameReportedRef = useRef(false)
  onWakeDoneRef.current = onWakeDone
  onFirstWakeFrameRef.current = onFirstWakeFrame

  const virtualBounds = useMemo(() => virtualDesktopBounds(displays), [displays])
  const image = useMemo(
    () => persona.assets.wake ?? placeholderPortrait(persona.kind, 'wake'),
    [persona.assets.wake, persona.kind],
  )
  const hasFrames = frames.length > 1

  useEffect(() => {
    if (!hasFrames || index < 1 || firstWakeFrameReportedRef.current) return
    firstWakeFrameReportedRef.current = true
    let first = 0
    let second = 0
    first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => onFirstWakeFrameRef.current?.())
    })
    return () => {
      cancelAnimationFrame(first)
      cancelAnimationFrame(second)
    }
  }, [hasFrames, index])

  useEffect(() => {
    if (!enabled) {
      const immediate = setTimeout(() => onWakeDoneRef.current(), 0)
      return () => clearTimeout(immediate)
    }
    if (!hasFrames) {
      const timer = setTimeout(() => onWakeDoneRef.current(), 2400)
      return () => clearTimeout(timer)
    }
    const duration = WAKE_FRAME_DURATIONS[Math.min(frameIndexRef.current, WAKE_FRAME_DURATIONS.length - 1)] / Math.max(0.25, speed)
    const timer = setTimeout(() => {
      if (frameIndexRef.current >= frames.length - 1) {
        onWakeDoneRef.current()
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
      {hasFrames ? frames.map((frame, frameNumber) => <img
        key={frame}
        className={`multi-screen-wake-frame ${frameNumber === index ? 'active' : ''}`}
        src={frame}
        alt={`苏醒 ${frameNumber + 1}`}
        draggable={false}
      />) : <img className={`multi-screen-wake-art multi-screen-wake-art-${Math.min(index, 3)}`} src={image} alt="苏醒的鲸鱼娘" draggable={false} />}
    </div>) : <div className="multi-screen-wake-surface multi-screen-wake-surface--virtual">
      {hasFrames ? frames.map((frame, frameNumber) => <img
        key={frame}
        className={`multi-screen-wake-frame ${frameNumber === index ? 'active' : ''}`}
        src={frame}
        alt={`苏醒 ${frameNumber + 1}`}
        draggable={false}
      />) : <img className={`multi-screen-wake-art multi-screen-wake-art-${Math.min(index, 3)}`} src={image} alt="苏醒的鲸鱼娘" draggable={false} />}
    </div>}
  </div>
}
