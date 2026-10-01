import type { ReactNode } from 'react'
import { t, useLanguage } from '../../i18n/index.ts'
import { Button } from './Button.tsx'
import { Glass } from './Glass.tsx'

export type ToastTone = 'info' | 'success' | 'warning' | 'error'

export interface ToastMessage {
  id: string
  title: string
  message?: string
  tone?: ToastTone
  action?: ReactNode
}

export interface ToastViewportProps {
  messages: ToastMessage[]
  onDismiss: (id: string) => void
}

const toneIcon: Record<ToastTone, string> = { info: '●', success: '✓', warning: '!', error: '×' }

export function ToastViewport({ messages, onDismiss }: ToastViewportProps) {
  // 关闭按钮的可访问名字来自词条。
  useLanguage()
  return <div className="dsh-toast-viewport" aria-live="polite" aria-relevant="additions removals">
    {messages.map((toast) => {
      const tone = toast.tone ?? 'info'
      return <Glass key={toast.id} className={`dsh-toast dsh-toast--${tone}`} elevation="floating" strength="strong" role={tone === 'error' ? 'alert' : 'status'}>
        <span className="dsh-toast__icon" aria-hidden="true">{toneIcon[tone]}</span>
        <div><p className="dsh-toast__title">{toast.title}</p>{toast.message && <p className="dsh-toast__message">{toast.message}</p>}{toast.action}</div>
        <Button variant="ghost" iconOnly onClick={() => onDismiss(toast.id)} aria-label={t('ui.toast.close')}>×</Button>
      </Glass>
    })}
  </div>
}
