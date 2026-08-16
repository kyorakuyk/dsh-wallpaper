import { useEffect, type ReactNode } from 'react'
import { Button } from './Button.tsx'
import { Glass } from './Glass.tsx'

export interface DrawerProps {
  open: boolean
  title: string
  description?: string
  children: ReactNode
  footer?: ReactNode
  onClose: () => void
}

export function Drawer({ open, title, description, children, footer, onClose }: DrawerProps) {
  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose, open])

  if (!open) return null
  return <Glass as="aside" className="dsh-drawer" elevation="floating" strength="strong" aria-label={title} data-interaction-region="drawer">
    <header className="dsh-drawer__header">
      <div><h2 className="dsh-drawer__title">{title}</h2>{description && <p className="dsh-drawer__description">{description}</p>}</div>
      <Button variant="ghost" iconOnly onClick={onClose} aria-label={`关闭${title}`}>×</Button>
    </header>
    <div className="dsh-drawer__body">{children}</div>
    {footer && <footer className="dsh-drawer__footer">{footer}</footer>}
  </Glass>
}
