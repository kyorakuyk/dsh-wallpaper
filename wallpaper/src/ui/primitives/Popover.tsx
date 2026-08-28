import { useEffect, useRef, type ReactNode } from 'react'
import { Glass } from './Glass.tsx'

export interface PopoverProps {
  open: boolean
  anchor: ReactNode
  children: ReactNode
  align?: 'start' | 'center' | 'end'
  className?: string
  onOpenChange: (open: boolean) => void
}

export function Popover({ open, anchor, children, align = 'center', className = '', onOpenChange }: PopoverProps) {
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) onOpenChange(false)
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onOpenChange(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [onOpenChange, open])

  return <div className="dsh-popover" ref={rootRef}>
    {anchor}
    {open && <Glass className={`dsh-popover__panel dsh-popover__panel--${align} ${className}`.trim()} elevation="floating">{children}</Glass>}
  </div>
}
