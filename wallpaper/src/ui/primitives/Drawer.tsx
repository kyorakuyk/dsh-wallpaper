import { useEffect, type ReactNode } from 'react'
import { t, useLanguage } from '../../i18n/index.ts'
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
  // 关闭按钮的可访问名字里有词条，语言一变就要重渲染一次。
  useLanguage()
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
      <Button variant="ghost" iconOnly onClick={onClose} aria-label={t('ui.drawer.close', { title })}>×</Button>
    </header>
    <div className="dsh-drawer__body">{children}</div>
    {footer && <footer className="dsh-drawer__footer">{footer}</footer>}
  </Glass>
}
