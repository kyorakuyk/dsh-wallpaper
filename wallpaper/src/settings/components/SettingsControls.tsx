import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

export function Card({ title, description, action, children }: { title: string; description?: string; action?: ReactNode; children: ReactNode }) {
  return <section className="settings-card"><header><div className="settings-card__heading"><h2>{title}</h2>{description && <p>{description}</p>}</div>{action && <div className="settings-card__action">{action}</div>}</header><div className="settings-card__body">{children}</div></section>
}

/**
 * 一行设置：左边是文案列（标题 + 说明 + 备注），右边是控件列。
 *
 * `note` 和 `detail` 一样住在**文案列**里；`children` 只放真的控件（开关、下拉、按钮）。
 * 这条界线是踩过一次才划出来的：一句话当 `children` 传进来会落进**控件列**，而控件列按
 * max-content 定宽、不收缩也不折行（`.settings-field__control`），于是那句话横穿卡片、被
 * `.settings-card` 的 `overflow: hidden` 裁掉；唯一能收缩的文案列被挤到只剩一个词宽，
 * 标题与说明都变成一词一行 —— 「壁纸自启未生效」那条警告就是这么坏的（它没有控件，
 * 唯一的内容就是那句话）。没有控件的那一行，请把话交给 `detail` / `note`，别塞进 `children`。
 */
export function Field({ title, detail, note, children }: { title: string; detail?: string; note?: string; children?: ReactNode }) {
  return <div className="settings-field"><div className="settings-field__copy"><strong>{title}</strong>{detail && <span>{detail}</span>}{note && <span className="settings-field__note">{note}</span>}</div>{children !== undefined && children !== null && <div className="settings-field__control">{children}</div>}</div>
}

export function Toggle({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={`settings-toggle ${checked ? 'is-on' : ''}`} onClick={() => onChange(!checked)}><span /></button>
}

export function Segmented({ value, options, onChange, label }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string }) {
  return <div className="settings-segmented" role="radiogroup" aria-label={label}>{options.map((option) =>
    <button type="button" key={option.value} role="radio" aria-checked={option.value === value} className={option.value === value ? 'is-selected' : ''} onClick={() => onChange(option.value)}>{option.label}</button>)}</div>
}

export function Choice({ value, options, onChange, label, disabled = false, emptyMessage, emptyLabel }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string; disabled?: boolean; emptyMessage?: string; emptyLabel: string }) {
  const [open, setOpen] = useState(false)
  const [openUpward, setOpenUpward] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const current = options.find((option) => option.value === value)?.label ?? options[0]?.label ?? emptyLabel
  useEffect(() => {
    const close = (event: MouseEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false) }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [])
  useLayoutEffect(() => {
    if (!open) {
      setOpenUpward(false)
      return
    }
    const element = root.current
    const trigger = element?.querySelector('.settings-choice__trigger')
    const menu = element?.querySelector('.settings-choice__menu')
    const scroller = element?.closest('.settings-content')
    if (!trigger || !menu || !scroller) return

    const triggerBounds = trigger.getBoundingClientRect()
    const menuBounds = menu.getBoundingClientRect()
    const scrollBounds = scroller.getBoundingClientRect()
    const roomBelow = scrollBounds.bottom - triggerBounds.bottom
    const roomAbove = triggerBounds.top - scrollBounds.top
    setOpenUpward(menuBounds.height + 5 > roomBelow && roomAbove >= menuBounds.height + 5)
  }, [open])
  return <div className={`settings-choice ${open ? 'is-open' : ''} ${openUpward ? 'is-open-upward' : ''}`} ref={root}>
    <button type="button" className="settings-choice__trigger" aria-label={label} aria-expanded={open} disabled={disabled} onClick={() => setOpen((shown) => !shown)}><span>{current}</span><i>⌄</i></button>
    {open && <div className="settings-choice__menu" role="listbox" aria-label={label}>{options.map((option) => <button type="button" key={option.value} className={option.value === value ? 'is-selected' : ''} role="option" aria-selected={option.value === value} onClick={() => { onChange(option.value); setOpen(false) }}>{option.label}</button>)}{emptyMessage && <span className="settings-choice__empty">{emptyMessage}</span>}</div>}
  </div>
}
