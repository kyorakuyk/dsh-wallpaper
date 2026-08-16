import type { HTMLAttributes, ReactNode } from 'react'

export interface GlassProps extends HTMLAttributes<HTMLElement> {
  as?: 'div' | 'section' | 'aside' | 'article'
  strength?: 'soft' | 'normal' | 'strong'
  elevation?: 'resting' | 'floating'
  children?: ReactNode
}

export function Glass({ as: Element = 'div', strength = 'normal', elevation = 'resting', className = '', ...props }: GlassProps) {
  return <Element className={`dsh-glass dsh-glass--${strength} dsh-glass--${elevation} ${className}`.trim()} {...props} />
}
