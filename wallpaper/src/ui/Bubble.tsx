/** 气泡组件：鲸鱼娘说话的气泡（可编辑文案、主题色联动） */

import type { PersonaTheme } from '../persona/types.ts'

export interface BubbleProps {
  text: string
  theme: PersonaTheme
  /** 气泡出现动画方向 */
  from?: 'left' | 'right' | 'top'
  onDone?: () => void
}

export function Bubble({ text, theme, from = 'left', onDone }: BubbleProps) {
  return (
    <div
      className={`bubble bubble-${from}`}
      style={{
        ['--bubble-primary' as string]: theme.primary,
        ['--bubble-accent' as string]: theme.accent,
        ['--bubble-glow' as string]: theme.glow,
      }}
      onAnimationEnd={onDone}
    >
      <div className="bubble-tail" />
      <span>{text}</span>
    </div>
  )
}
