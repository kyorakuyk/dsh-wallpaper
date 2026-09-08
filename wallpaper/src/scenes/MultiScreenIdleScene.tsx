/**
 * Multi-screen idle composition for the full edition.
 *
 * The native host is intentionally still one WorkerW/WebView. Each monitor
 * gets an independent, paint-contained background layer, while only the
 * selected monitor mounts the portrait layer. React.memo keeps a background
 * whose assignment did not change out of the update path when another screen
 * or the conversation state changes.
 */
import { memo, useMemo } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import { Bubble } from '../ui/Bubble.tsx'
import { placeholderPortrait } from '../ui/whale.ts'
import type { DesktopDisplayInfo } from '../native/runtime.ts'
import { displayCssRect, virtualDesktopBounds } from '../runtime/displayLayout.ts'
import { usePortraitEnvironmentBlend } from './usePortraitEnvironmentBlend.ts'
import './MultiScreenIdleScene.css'

interface DisplayLayerProps {
  display: DesktopDisplayInfo
  virtualBounds: ReturnType<typeof virtualDesktopBounds>
}

interface ScreenBackgroundProps extends DisplayLayerProps {
  backgroundUrl?: string
  personaPrimary: string
}

const ScreenBackground = memo(function ScreenBackground({ display, virtualBounds, backgroundUrl, personaPrimary }: ScreenBackgroundProps) {
  return <div
    className="multi-screen-surface multi-screen-background"
    style={{ ...displayCssRect(display, virtualBounds), ['--persona-primary' as string]: personaPrimary }}
    data-display-id={display.id}
    aria-hidden="true"
  >
    {backgroundUrl ? <img className="multi-screen-background__image" src={backgroundUrl} alt="" draggable={false} /> : <div className="multi-screen-background__fallback" />}
  </div>
}, (previous, next) => (
  previous.display.id === next.display.id
  && previous.display.bounds.x === next.display.bounds.x
  && previous.display.bounds.y === next.display.bounds.y
  && previous.display.bounds.width === next.display.bounds.width
  && previous.display.bounds.height === next.display.bounds.height
  && previous.backgroundUrl === next.backgroundUrl
  && previous.personaPrimary === next.personaPrimary
  && previous.virtualBounds.x === next.virtualBounds.x
  && previous.virtualBounds.y === next.virtualBounds.y
  && previous.virtualBounds.width === next.virtualBounds.width
  && previous.virtualBounds.height === next.virtualBounds.height
))

interface ScreenPortraitProps extends DisplayLayerProps {
  persona: PersonaManifest
  backgroundUrl?: string
  bubbleText: string
  portraitAmbientLength: number
  portraitAmbientStrength: number
  onOpenChat: () => void
}

const ScreenPortrait = memo(function ScreenPortrait({ display, virtualBounds, persona, backgroundUrl, bubbleText, portraitAmbientLength, portraitAmbientStrength, onOpenChat }: ScreenPortraitProps) {
  const image = useMemo(
    () => persona.assets.portrait ?? placeholderPortrait(persona.kind, 'idle'),
    [persona.assets.portrait, persona.kind],
  )
  const environment = usePortraitEnvironmentBlend(backgroundUrl ?? '', { x: 0.79, y: 0.78 }, { width: display.bounds.width, height: display.bounds.height })
  return <div
    className="multi-screen-surface multi-screen-portrait"
    style={displayCssRect(display, virtualBounds)}
    data-display-id={display.id}
  >
    <div
      className="portrait-slot multi-screen-portrait__slot"
      onClick={onOpenChat}
      title="点击开始对话"
      data-interaction-region="portrait"
      style={{
        ['--persona-primary' as string]: persona.theme.primary,
        ['--portrait-environment-rgb' as string]: environment.rgb,
        ['--portrait-environment-luma' as string]: environment.luminance.toFixed(3),
        ['--portrait-light-angle' as string]: environment.lightAngle,
        ['--portrait-light-contrast' as string]: environment.contrast.toFixed(3),
        ['--portrait-ambient-length' as string]: `${portraitAmbientLength}%`,
        ['--portrait-ambient-strength' as string]: portraitAmbientStrength.toFixed(2),
        ['--portrait-alpha-mask' as string]: `url("${image}")`,
      }}
    >
      <img
        className={`portrait ${persona.assets.portrait?.endsWith('.jpg') ? 'portrait-asset' : ''}`}
        src={image}
        alt={persona.name}
        draggable={false}
      />
      <div className="portrait-environment" aria-hidden="true"><i className="portrait-glow" /><i className="portrait-rim" /><i className="portrait-fade" /><i className="portrait-contact" /></div>
      {bubbleText.trim() && <Bubble text={bubbleText} theme={persona.theme} from="top" />}
    </div>
  </div>
}, (previous, next) => (
  previous.display.id === next.display.id
  && previous.display.bounds.x === next.display.bounds.x
  && previous.display.bounds.y === next.display.bounds.y
  && previous.display.bounds.width === next.display.bounds.width
  && previous.display.bounds.height === next.display.bounds.height
  && previous.virtualBounds.x === next.virtualBounds.x
  && previous.virtualBounds.y === next.virtualBounds.y
  && previous.virtualBounds.width === next.virtualBounds.width
  && previous.virtualBounds.height === next.virtualBounds.height
  && previous.persona === next.persona
  && previous.backgroundUrl === next.backgroundUrl
  && previous.bubbleText === next.bubbleText
  && previous.portraitAmbientLength === next.portraitAmbientLength
  && previous.portraitAmbientStrength === next.portraitAmbientStrength
  && previous.onOpenChat === next.onOpenChat
))

export interface MultiScreenIdleSceneProps {
  displays: readonly DesktopDisplayInfo[]
  backgroundUrls: Readonly<Record<string, string | undefined>>
  portraitDisplayId?: string
  persona: PersonaManifest
  bubbleText: string
  portraitAmbientLength?: number
  portraitAmbientStrength?: number
  onOpenChat: () => void
}

export function MultiScreenIdleScene({ displays, backgroundUrls, portraitDisplayId, persona, bubbleText, portraitAmbientLength = 82, portraitAmbientStrength = .72, onOpenChat }: MultiScreenIdleSceneProps) {
  const virtualBounds = useMemo(() => virtualDesktopBounds(displays), [displays])
  const portraitDisplay = displays.find((display) => display.id === portraitDisplayId)
    ?? displays.find((display) => display.primary)
    ?? displays[0]

  return <div className="scene multi-screen-idle-scene">
    {displays.map((display) => <ScreenBackground
      key={display.id}
      display={display}
      virtualBounds={virtualBounds}
      backgroundUrl={backgroundUrls[display.id]}
      personaPrimary={persona.theme.primary}
    />)}
    {portraitDisplay && <ScreenPortrait
      display={portraitDisplay}
      virtualBounds={virtualBounds}
      persona={persona}
      backgroundUrl={backgroundUrls[portraitDisplay.id]}
      bubbleText={bubbleText}
      portraitAmbientLength={portraitAmbientLength}
      portraitAmbientStrength={portraitAmbientStrength}
      onOpenChat={onOpenChat}
    />}
  </div>
}
