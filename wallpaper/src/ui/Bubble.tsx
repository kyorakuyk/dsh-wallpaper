/** 气泡组件：鲸鱼娘说话的气泡（可编辑文案、主题色联动） */

import type { CSSProperties } from 'react'
import type { PersonaTheme } from '../persona/types.ts'

/**
 * 气泡的主题色变量。
 *
 * 导出给第二枚气泡（更新气泡，`features/update/UpdateBubble.tsx`）共用一份：两枚气泡挂在同一个
 * 立绘槽位里、用的是同一套 CSS 变量，各写一份迟早会配色漂移。
 */
export function bubbleThemeStyle(theme: PersonaTheme): CSSProperties {
  return {
    ['--bubble-primary' as string]: theme.primary,
    ['--bubble-accent' as string]: theme.accent,
    ['--bubble-glow' as string]: theme.glow,
  }
}

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
      style={bubbleThemeStyle(theme)}
      onAnimationEnd={onDone}
    >
      <div className="bubble-tail" />
      <span>{text}</span>
    </div>
  )
}
