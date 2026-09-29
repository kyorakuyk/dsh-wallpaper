/** 苏醒场景：帧序列动画（睡脸→睁眼→打哈欠→醒来），构图连贯 */

import { useEffect, useRef, useState } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import { placeholderPortrait } from '../ui/whale.ts'
import { assetUrl } from '../runtime/assets.ts'
import { decodeImageSource } from '../native/bootstrapHandoff.ts'
import './wakeTransition.css'

export interface WakeSceneProps {
  persona: PersonaManifest
  /** 动画播完回调 → 状态机 'wakeDone' */
  onWakeDone: () => void
  /** Called after the first non-sleep frame has had a browser paint opportunity. */
  onFirstWakeFrame?: (generation: number) => void | boolean | Promise<void | boolean>
  handoffGeneration?: number
  /** The lock screen already showed sleep.png, so unlocks may begin at frame 2. */
  startIndex?: number
  enabled?: boolean
  speed?: number
}

/** 帧序列（按播放顺序）。若用户素材提供 animations.wake.frames 则优先使用。 */
export const DEFAULT_WAKE_FRAMES = [
  // 前两帧保持 PNG：它们同时被原生层解码（启动遮盖层与锁屏图走 WIC/Windows），
  // 换成 WebP 会把这条链路押在系统是否装了 WebP 解码器上。
  assetUrl('personas/wake-frames/variant-anima/sleep.png'),
  assetUrl('personas/wake-frames/variant-anima/frame-2-eyes.png'),
  // 后两帧只由 WebView 解码：同样画面存 PNG 要 3 MB 以上，WebP 只要 0.3 MB。
  assetUrl('personas/wake-frames/variant-anima/frame-3-yawn.webp'),
  assetUrl('personas/wake-frames/variant-anima/frame-4-awake.webp'),
]

/** 每帧停留时长（ms）：睡脸稍久，中间过渡稍快 */
export const WAKE_FRAME_DURATIONS = [2200, 1400, 1600, 2000]

/** 帷幕压下（画面渐黑）用时。 */
export const WAKE_CURTAIN_IN_MS = 420
/** 桌面从黑里淡出用时；与 styles.css 的 `.wake-curtain-out` 是同一个节奏。 */
export const WAKE_CURTAIN_OUT_MS = 720
/**
 * 这一幕的总时长：帷幕淡出 + 立绘从虚影浮出（styles.css 里是 700ms 延迟 + 1100ms 动画）。
 * 调用方用它决定 `.wake-enter` 这类只在收尾期间存在的状态活多久。
 */
export const WAKE_ENTER_MS = 1800

/**
 * 苏醒收尾的帷幕。
 *
 * 动画最后一帧要先压黑，**再**把画面交给桌面：交接如果发生在画面还亮着的时候，桌面就是
 * 从苏醒帧上直接跳出来的，谈不上"淡入"。所以序列播完后调用方只落帷幕，等 `WAKE_CURTAIN_IN_MS`
 * 才回调 `onDone`，那段时间正好是屏幕变黑。
 */
export function useWakeCurtain(onDone: () => void) {
  const [curtain, setCurtain] = useState(false)
  const onDoneRef = useRef(onDone)
  onDoneRef.current = onDone

  useEffect(() => {
    if (!curtain) return
    const timer = setTimeout(() => onDoneRef.current(), WAKE_CURTAIN_IN_MS)
    return () => clearTimeout(timer)
  }, [curtain])

  return { curtain, dropCurtain: () => setCurtain(true) }
}

export function wakeFrameSources(persona: PersonaManifest): string[] {
  return persona.animations?.wake?.frames?.length
    ? persona.animations.wake.frames
    : DEFAULT_WAKE_FRAMES
}

/** Warm subsequent frames without delaying the first visible wake frame. */
export function useWakeFramePreload(frames: readonly string[], enabled: boolean): void {
  useEffect(() => {
    if (!enabled || frames.length <= 1) return
    let cancelled = false
    void (async () => {
      // Sequential loading keeps the unlock path from starting four decodes at
      // once, while the timeout prevents one broken custom frame from stalling
      // the animation forever.
      for (const frame of frames.slice(1)) {
        if (cancelled) return
        await decodeImageSource(frame)
      }
    })()
    return () => { cancelled = true }
  }, [enabled, frames])
}

export function WakeScene({ persona, onWakeDone, onFirstWakeFrame, handoffGeneration, startIndex = 0, enabled = true, speed = 1 }: WakeSceneProps) {
  const frames = wakeFrameSources(persona)
  const initialIndex = Math.min(Math.max(0, startIndex), Math.max(0, frames.length - 1))
  const [index, setIndex] = useState(initialIndex)
  const [img] = useState(() =>
    persona.assets.wake ?? placeholderPortrait(persona.kind, 'wake'),
  )
  const [fallbackToStatic, setFallbackToStatic] = useState(false)
  const hasFrames = frames.length > 1
  const frameIndexRef = useRef(initialIndex)
  const onWakeDoneRef = useRef(onWakeDone)
  const onFirstWakeFrameRef = useRef(onFirstWakeFrame)
  const reportedGenerationRef = useRef<number>()
  onWakeDoneRef.current = onWakeDone
  onFirstWakeFrameRef.current = onFirstWakeFrame
  const { curtain, dropCurtain } = useWakeCurtain(onWakeDone)

  useWakeFramePreload(frames, enabled && hasFrames)

  useEffect(() => {
    if (!hasFrames || index < 1 || handoffGeneration === undefined || reportedGenerationRef.current === handoffGeneration) return
    const generation = handoffGeneration
    const source = fallbackToStatic ? img : frames[index]
    if (!source) return
    let cancelled = false
    let first = 0
    let second = 0
    void decodeImageSource(source).then(async (ready) => {
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
          }).catch((error) => console.warn('wake frame hand-off callback failed', error))
        })
      })
    })
    return () => {
      cancelled = true
      cancelAnimationFrame(first)
      cancelAnimationFrame(second)
    }
  }, [fallbackToStatic, frames, handoffGeneration, hasFrames, img, index])

  useEffect(() => {
    if (!enabled) {
      const immediate = setTimeout(() => onWakeDoneRef.current(), 0)
      return () => clearTimeout(immediate)
    }
    if (!hasFrames) {
      // 无帧序列：单图渐显模式，播完落帷幕
      const t = setTimeout(dropCurtain, 2400)
      return () => clearTimeout(t)
    }
    // 帧序列模式：按 FRAME_DURATIONS 逐帧切换，最后一帧之后落帷幕
    const duration = WAKE_FRAME_DURATIONS[Math.min(frameIndexRef.current, WAKE_FRAME_DURATIONS.length - 1)] / Math.max(0.25, speed)
    const t = setTimeout(() => {
      if (frameIndexRef.current >= frames.length - 1) {
        dropCurtain()
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
      {hasFrames && !fallbackToStatic ? (
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
      {curtain && <div className="wake-curtain" style={{ animationDuration: `${WAKE_CURTAIN_IN_MS}ms` }} />}
    </div>
  )
}
