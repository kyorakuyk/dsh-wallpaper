/** 待机场景（纯展示）：背景插画 + 右侧立绘 + 气泡；交互逻辑由父组件管理 */

import { useMemo } from 'react'
import type { PersonaManifest } from '../persona/types.ts'
import { Bubble } from '../ui/Bubble.tsx'
import { placeholderPortrait } from '../ui/whale.ts'

export interface IdleSceneProps {
  persona: PersonaManifest
  /** 当前气泡文案（父组件根据事件更新） */
  bubbleText: string
  /** 是否显示 DSH 上线询问条 */
  showHarnessPrompt: boolean
  /** 3080 是否在线（角标显示） */
  harnessOnline: boolean
  /** 待机背景图 URL（空字符串 = 默认主题渐变） */
  backgroundUrl?: string
  onOpenChat: () => void
  onSwitchToHarness: () => void
  onDismissHarnessPrompt: () => void
}

export function IdleScene({
  persona,
  bubbleText,
  showHarnessPrompt,
  harnessOnline,
  backgroundUrl = '',
  onOpenChat,
  onSwitchToHarness,
  onDismissHarnessPrompt,
}: IdleSceneProps) {
  const img = useMemo(
    () => persona.assets.portrait ?? placeholderPortrait(persona.kind, 'idle'),
    [persona.assets.portrait, persona.kind],
  )

  return (
    <div
      className="scene scene-idle"
      style={{ ['--persona-primary' as string]: persona.theme.primary }}
    >
      {/* 背景：深海插画 or 默认主题渐变 */}
      {backgroundUrl ? (
        <img className="idle-bg-image" src={backgroundUrl} alt="背景" draggable={false} />
      ) : (
        <div className="idle-bg" />
      )}
      {/* 右侧立绘（透明 PNG；带背景的 JPG 素材则圆角融入） */}
      <div className="portrait-slot" onClick={onOpenChat} title="点击开始对话">
        <img
          className={`portrait ${persona.assets.portrait?.endsWith('.jpg') ? 'portrait-asset' : ''}`}
          src={img}
          alt={persona.name}
          draggable={false}
        />
        <div className="portrait-glow" />
        {/* 底部渐隐遮罩：让立绘底部融入背景，不生硬 */}
        <div className="portrait-fade" />
        {/* 气泡定位在立绘头部上方（跟随大肥鱼） */}
        <Bubble text={bubbleText} theme={persona.theme} from="top" />
      </div>
      {/* DSH 上线询问 */}
      {showHarnessPrompt && (
        <div className="harness-prompt">
          <span>{persona.bubbles.harnessOnline}</span>
          <button onClick={onSwitchToHarness}>切换到 Harness</button>
          <button onClick={onDismissHarnessPrompt}>暂不</button>
        </div>
      )}
      {/* 状态角标：3080 在线/离线 */}
      <div className={`conn-badge ${harnessOnline ? 'on' : 'off'}`}>
        {harnessOnline ? '● DSH 在线' : '○ DeepSeek 网页模式'}
      </div>
    </div>
  )
}
