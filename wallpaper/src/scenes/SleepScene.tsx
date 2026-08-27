/** 睡眠场景：静态画面，鲸鱼娘在床上呼呼大睡（全屏铺满） */

import { useEffect, useState } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import { placeholderPortrait } from '../ui/whale.ts'
import { wakeFrameSources } from './WakeScene.tsx'

export interface SleepSceneProps {
  persona: PersonaManifest
  /** 睡眠模式来源：system(系统锁定) / manual(应用内睡眠) */
  mode: 'system' | 'manual'
  /** Lite 首发版需要与 Windows 锁屏原图无缝衔接，不叠加装饰层。 */
  quiet?: boolean
}

export function SleepScene({ persona, mode, quiet = false }: SleepSceneProps) {
  // 优先用用户素材（睡眠静态图，全屏场景图），无素材回退程序占位布局
  const [sleepImg] = useState(() =>
    persona.assets.sleep ?? placeholderPortrait(persona.kind, 'sleep'),
  )
  const hasAsset = Boolean(persona.assets.sleep)

  // Decode the formal wake sequence while the desktop is still showing the
  // sleep frame. The first wake frame is the same sleep artwork, so the
  // unlock transition can paint immediately instead of waiting for four
  // network/decode tasks to start after the password is accepted.
  useEffect(() => {
    for (const source of wakeFrameSources(persona)) {
      const image = new Image()
      image.decoding = 'async'
      image.src = source
    }
  }, [persona])

  return (
    <div
      className="scene scene-sleep"
      style={{ ['--persona-primary' as string]: persona.theme.primary }}
    >
      {hasAsset ? (
        <>
          {/* 用户素材：整幅睡眠场景图 + 轻微暗化保证浮层可读 */}
          <img className="sleep-art" src={sleepImg} alt="睡着的鲸鱼娘" draggable={false} />
          {!quiet && <div className="sleep-art-veil" />}
          {!quiet && <div className="sleep-zzz">Z z z…</div>}
        </>
      ) : (
        <>
          {/* 占位布局：氛围背景 + 床 + 程序鲸鱼娘 */}
          <div className="sleep-bg">
            <div className="stars" />
            <div className="moon" />
          </div>
          <div className="bed">
            <div className="bed-mattress" />
            <div className="bed-frame" />
            <div className="pillow" />
            <div className="blanket" />
          </div>
          <img className="sleeping-whale" src={sleepImg} alt="睡着的鲸鱼娘" draggable={false} />
          <div className="sleep-zzz">Z z z…</div>
        </>
      )}
      {mode === 'manual' && !quiet && <div className="sleep-hint">按 Esc 或输入密码唤醒</div>}
    </div>
  )
}
