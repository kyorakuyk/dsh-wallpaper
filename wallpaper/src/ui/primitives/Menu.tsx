import type { ReactNode } from 'react'

export interface MenuItemProps {
  label: string
  icon?: ReactNode
  shortcut?: string
  checked?: boolean
  disabled?: boolean
  onSelect: () => void
}

export function Menu({ children, label }: { children: ReactNode; label?: string }) {
  return <div className="dsh-menu" role="menu" aria-label={label}>{children}</div>
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return <div className="dsh-menu__label">{children}</div>
}

export function MenuSeparator() { return <div className="dsh-menu__separator" role="separator" /> }

export function MenuItem({ label, icon, shortcut, checked, disabled, onSelect }: MenuItemProps) {
  return <button className="dsh-menu__item" role="menuitemcheckbox" aria-checked={checked ?? false} disabled={disabled} onClick={onSelect}>
    <span aria-hidden="true">{icon ?? (checked ? '✓' : '')}</span><span>{label}</span>{shortcut && <span className="dsh-menu__shortcut">{shortcut}</span>}
  </button>
}
