import type { SVGAttributes } from 'react'

export type IconName = 'arrow-up' | 'check' | 'chevron-up' | 'close' | 'export' | 'history' | 'image' | 'import' | 'inbox' | 'lock' | 'model' | 'palette' | 'refresh' | 'spark' | 'stop'

const paths: Record<IconName, string> = {
  'arrow-up': 'M12 19V5m0 0-6 6m6-6 6 6',
  check: 'm5 12 4 4L19 6',
  'chevron-up': 'm6 15 6-6 6 6',
  close: 'M7 7l10 10M17 7 7 17',
  export: 'M12 15V3m0 0L7 8m5-5 5 5M5 13v7h14v-7',
  history: 'M3 12a9 9 0 1 0 3-6.7L3 8m0 0h5M3 8V3m9 4v5l3 2',
  image: 'M4 5h16v14H4V5Zm0 11 4.5-4.5 3.5 3 2.5-2.5 5.5 5M9 9h.01',
  import: 'M12 3v12m0 0-5-5m5 5 5-5M5 18v2h14v-2',
  inbox: 'M4 5h16l-2 14H6L4 5Zm1.3 9h4l1.2 2h3l1.2-2h4',
  lock: 'M7 10V7a5 5 0 0 1 10 0v3m-11 0h12v10H6V10Z',
  model: 'M12 3 4.5 7.2 12 11.5l7.5-4.3L12 3Zm-7.5 9L12 16.3l7.5-4.3M4.5 16.8 12 21l7.5-4.2',
  palette: 'M12 3a9 9 0 1 0 0 18h1.2a2 2 0 0 0 0-4H12a2 2 0 0 1 0-4h5.5A3.5 3.5 0 0 0 21 9.5C21 5.9 17 3 12 3Z',
  refresh: 'M20 7v5h-5M4 17v-5h5m10.2-3A8 8 0 0 0 5.5 6M4.8 15A8 8 0 0 0 18.5 18',
  spark: 'M12 2.8c.5 4.2 2.8 6.5 7 7-4.2.5-6.5 2.8-7 7-.5-4.2-2.8-6.5-7-7 4.2-.5 6.5-2.8 7-7Z',
  stop: 'M8 8h8v8H8z',
}

export interface IconProps extends SVGAttributes<SVGElement> { name: IconName; size?: number }

export function Icon({ name, size = 18, ...props }: IconProps) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" {...props}>
    <path d={paths[name]} />
  </svg>
}
