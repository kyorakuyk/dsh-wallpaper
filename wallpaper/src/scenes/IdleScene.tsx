/** 待机场景（纯展示）：背景插画 + 右侧立绘 + 气泡；交互逻辑由父组件管理 */

import { useMemo, type ReactNode } from 'react'
import { t, useLanguage } from '../i18n/index.ts'
import type { PersonaManifest } from '../persona/types.ts'
import { Bubble } from '../ui/Bubble.tsx'
import { placeholderPortrait } from '../ui/whale.ts'
import { portraitAgeScale } from '../persona/portraitScale.ts'
import { usePortraitEnvironmentBlend } from './usePortraitEnvironmentBlend.ts'

export interface IdleSceneProps {
  persona: PersonaManifest
  /** 当前气泡文案（父组件根据事件更新） */
  bubbleText: string
  /** 待机背景图 URL（空字符串 = 空白背景渐变） */
  backgroundUrl?: string
  portraitAmbientLength?: number
  portraitAmbientStrength?: number
  /** 进入里桌面后由中央会话窗承担沟通入口，避免双气泡并存。 */
  hideBubble?: boolean
  /**
   * 第二枚气泡：更新提示（`features/update/UpdateBubble.tsx`）。
   *
   * 由父组件决定挂不挂 —— 出现时机（进入里桌面之后）是 `updateState.updateBubbleVisible` 的
   * 判断，场景本身只负责把它放进立绘槽位（`.portrait-slot`，与「早上好…」那枚共用一套样式）。
   */
  updateBubble?: ReactNode
  onOpenChat: () => void
}

export function IdleScene({
  persona,
  bubbleText,
  backgroundUrl = '',
  portraitAmbientLength = 82,
  portraitAmbientStrength = .72,
  hideBubble = false,
  updateBubble,
  onOpenChat,
}: IdleSceneProps) {
  // 背景的 alt 与立绘上那句提示都是词条。
  useLanguage()
  const img = useMemo(
    () => persona.assets.portrait ?? placeholderPortrait(persona.kind, 'idle'),
    [persona.assets.portrait, persona.kind],
  )
  const environment = usePortraitEnvironmentBlend(backgroundUrl)

  return (
    <div
      className="scene scene-idle"
      style={{ ['--persona-primary' as string]: persona.theme.primary }}
    >
      {/* 背景：深海插画 or 空白背景渐变 */}
      {backgroundUrl ? (
        <img className="idle-bg-image" src={backgroundUrl} alt={t('scene.idle.background')} draggable={false} />
      ) : (
        <div className="idle-bg" />
      )}
      {/* 右侧立绘（透明 PNG；带背景的 JPG 素材则圆角融入） */}
      <div className="portrait-slot" onClick={onOpenChat} title={t('scene.idle.portrait-title')} style={{ ['--portrait-environment-rgb' as string]: environment.rgb, ['--portrait-environment-luma' as string]: environment.luminance.toFixed(3), ['--portrait-light-angle' as string]: environment.lightAngle, ['--portrait-light-contrast' as string]: environment.contrast.toFixed(3), ['--portrait-ambient-length' as string]: `${portraitAmbientLength}%`, ['--portrait-ambient-strength' as string]: portraitAmbientStrength.toFixed(2), ['--portrait-alpha-mask' as string]: `url("${img}")`, ['--portrait-age-scale' as string]: String(portraitAgeScale(persona)) }}>
        <img data-interaction-region="persona" onMouseDown={(event) => event.preventDefault()} 
          className={`portrait ${persona.assets.portrait?.endsWith('.jpg') ? 'portrait-asset' : ''}`}
          src={img}
          alt={persona.name}
          draggable={false}
        />
        <div className="portrait-environment" aria-hidden="true"><i className="portrait-glow" /><i className="portrait-rim" /><i className="portrait-fade" /><i className="portrait-contact" /></div>
        {/* 气泡定位在立绘头部上方（跟随大肥鱼） */}
        {!hideBubble && <Bubble text={bubbleText} theme={persona.theme} from="top" />}
        {/* 更新气泡：与上面那枚同一个槽位、同一套样式；出现时机由父组件（App）判断 */}
        {updateBubble}
      </div>
    </div>
  )
}
