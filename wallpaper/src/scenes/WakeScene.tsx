/** 苏醒场景：帧序列动画（睡脸→睁眼→起身→打哈欠），构图连贯 */

import { useEffect, useRef, useState } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import { placeholderPortrait } from '../ui/whale.ts'
import { assetUrl } from '../runtime/assets.ts'

export interface WakeSceneProps {
  persona: PersonaManifest
  /** 动画播完回调 → 状态机 'wakeDone' */
  onWakeDone: () => void
  /** Called after the first non-sleep frame has had a browser paint opportunity. */
  onFirstWakeFrame?: () => void
  /** The lock screen already showed sleep.png, so unlocks may begin at frame 2. */
  startIndex?: number
  enabled?: boolean
  speed?: number
}

/** 帧序列（按播放顺序）。若用户素材提供 animations.wake.frames 则优先使用。 */
export const DEFAULT_WAKE_FRAMES = [
  assetUrl('personas/wake-frames/variant-anima/sleep.png'),
  assetUrl('personas/wake-frames/variant-anima/frame-2-eyes.png'),
  assetUrl('personas/wake-frames/variant-anima/frame-3-situp.png'),
  assetUrl('personas/wake-frames/variant-anima/frame-4-yawn.png'),
]

/** 每帧停留时长（ms）：睡脸稍久，中间过渡稍快 */
export const WAKE_FRAME_DURATIONS = [2200, 1400, 1600, 2000]

export function wakeFrameSources(persona: PersonaManifest): string[] {
  return persona.animations?.wake?.frames?.length
    ? persona.animations.wake.frames
    : DEFAULT_WAKE_FRAMES
}

export function WakeScene({ persona, onWakeDone, onFirstWakeFrame, startIndex = 0, enabled = true, speed = 1 }: WakeSceneProps) {
  const frames = wakeFrameSources(persona)
  const initialIndex = Math.min(Math.max(0, startIndex), Math.max(0, frames.length - 1))
  const [index, setIndex] = useState(initialIndex)
  const [img] = useState(() =>
    persona.assets.wake ?? placeholderPortrait(persona.kind, 'wake'),
  )
  const hasFrames = frames.length > 1
  const frameIndexRef = useRef(initialIndex)
  const onWakeDoneRef = useRef(onWakeDone)
  const onFirstWakeFrameRef = useRef(onFirstWakeFrame)
  const firstWakeFrameReportedRef = useRef(false)
  onWakeDoneRef.current = onWakeDone
  onFirstWakeFrameRef.current = onFirstWakeFrame

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
      // 无帧序列：单图渐显模式，播完回调
      const t = setTimeout(() => onWakeDoneRef.current(), 2400)
      return () => clearTimeout(t)
    }
    // 帧序列模式：按 FRAME_DURATIONS 逐帧切换，播完回调
    const duration = WAKE_FRAME_DURATIONS[Math.min(frameIndexRef.current, WAKE_FRAME_DURATIONS.length - 1)] / Math.max(0.25, speed)
    const t = setTimeout(() => {
      if (frameIndexRef.current >= frames.length - 1) {
        onWakeDoneRef.current()
      } else {
        frameIndexRef.current += 1
        setIndex(frameIndexRef.current)
      }
    }, duration)
    return () => clearTimeout(t)
  }, [enabled, index, frames, hasFrames, speed])

  return (
    <div
      className="scene scene-wake"
      style={{ ['--persona-primary' as string]: persona.theme.primary }}
    >
      {hasFrames ? (
        <>
          {/* 帧序列动画：每帧淡入切换 */}
          {frames.map((f, i) => (
            <img
              key={f}
              className={`wake-frame ${i === index ? 'active' : ''}`}
              src={f}
              alt={`苏醒 ${i + 1}`}
              draggable={false}
            />
          ))}
        </>
      ) : (
        <>
          <img
            className={`wake-art wake-art-${Math.min(index, 3)}`}
            src={img}
            alt="苏醒的鲸鱼娘"
            draggable={false}
          />
        </>
      )}
    </div>
  )
}
