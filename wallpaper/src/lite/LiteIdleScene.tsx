import { useMemo } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import { placeholderPortrait } from '../ui/whale.ts'
import { usePortraitEnvironmentBlend } from '../scenes/usePortraitEnvironmentBlend.ts'

export interface LiteIdleSceneProps {
  persona: PersonaManifest
  backgroundUrl?: string
}

/** 纯壁纸待机层：没有输入岛、气泡或透明交互区域，Explorer 始终拥有桌面输入。 */
export function LiteIdleScene({ persona, backgroundUrl = '' }: LiteIdleSceneProps) {
  const portrait = useMemo(
    () => persona.assets.portrait ?? placeholderPortrait(persona.kind, 'idle'),
    [persona.assets.portrait, persona.kind],
  )
  const environment = usePortraitEnvironmentBlend(backgroundUrl)

  return (
    <div className="scene scene-idle lite-idle-scene" style={{ ['--persona-primary' as string]: persona.theme.primary }}>
      {backgroundUrl ? <img className="idle-bg-image" src={backgroundUrl} alt="壁纸背景" draggable={false} /> : <div className="idle-bg" />}
      <div
        className="portrait-slot lite-portrait-slot"
        style={{
          ['--portrait-environment-rgb' as string]: environment.rgb,
          ['--portrait-environment-luma' as string]: environment.luminance.toFixed(3),
          ['--portrait-light-angle' as string]: environment.lightAngle,
          ['--portrait-light-contrast' as string]: environment.contrast.toFixed(3),
          ['--portrait-alpha-mask' as string]: `url("${portrait}")`,
        }}
      >
        <img className="portrait" src={portrait} alt={persona.name} draggable={false} />
        <div className="portrait-environment" aria-hidden="true"><i className="portrait-glow" /><i className="portrait-rim" /><i className="portrait-fade" /></div>
      </div>
    </div>
  )
}
