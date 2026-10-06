import type { ReactNode } from 'react'

export type SettingsIconName =
  | 'general'
  | 'connections'
  | 'appearance'
  | 'personas'
  | 'history'
  | 'system'
  | 'close'
  | 'monitor'
  | 'person'
  | 'chat'
  | 'check'
  | 'chevron'

const PATHS: Record<SettingsIconName, ReactNode> = {
  general: <><path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" /><circle cx="15" cy="6" r="2" /><circle cx="9" cy="12" r="2" /><circle cx="17" cy="18" r="2" /></>,
  connections: <><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" /><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" /></>,
  appearance: <><circle cx="12" cy="12" r="8" /><path d="M12 4a8 8 0 0 0 0 16z" fill="currentColor" /></>,
  personas: <><circle cx="12" cy="8" r="4" /><path d="M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6" /></>,
  history: <><circle cx="12" cy="12" r="8" /><path d="M12 8v4l3 2" /></>,
  system: <><rect x="5" y="5" width="14" height="14" rx="2" /><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" /></>,
  close: <path d="M6 6l12 12M18 6L6 18" />,
  monitor: <><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></>,
  person: <><circle cx="12" cy="8" r="4" /><path d="M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6" /></>,
  chat: <path d="M5 5h14v10H9l-4 4z" />,
  check: <path d="M5 12l5 5 9-10" />,
  chevron: <path d="M6 9l6 6 6-6" />,
}

export function SettingsIcon({ name, size = 18 }: { name: SettingsIconName; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{PATHS[name]}</svg>
}
