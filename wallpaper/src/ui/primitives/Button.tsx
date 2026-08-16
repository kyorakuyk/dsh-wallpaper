import type { ButtonHTMLAttributes, ReactNode } from 'react'

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
  iconOnly?: boolean
  children?: ReactNode
}

export function Button({ variant = 'secondary', iconOnly = false, className = '', type = 'button', ...props }: ButtonProps) {
  return <button type={type} className={`dsh-button dsh-button--${variant}${iconOnly ? ' dsh-button--icon' : ''} ${className}`.trim()} {...props} />
}
