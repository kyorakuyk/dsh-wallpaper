import type { Activity, BackendMode, SystemPhase } from '../domain/types.ts'

export type HarnessAvailability = 'offline' | 'web-only' | 'bridge-ready'

export interface InteractionSnapshot {
  enabled: boolean
  visible: boolean
  desktopForeground: boolean
  historyExpanded: boolean
  settingsOpen: boolean
}

export interface WallpaperHostSnapshot {
  mode: 'starting' | 'worker-w' | 'progman-fallback' | 'recovering' | 'unavailable'
  generation: number
  recoveryCount: number
  lastError?: string
}

export interface AppSnapshot {
  revision: number
  phase: SystemPhase
  backend: BackendMode
  activity: Activity
  harness: HarnessAvailability
  wallpaperHost: WallpaperHostSnapshot
  interaction: InteractionSnapshot
  privacyScreen: boolean
  error?: string
}

export const PREVIEW_APP_SNAPSHOT: AppSnapshot = {
  revision: 0,
  phase: 'idle',
  backend: 'deepseek-web',
  activity: 'idle',
  harness: 'offline',
  wallpaperHost: {
    mode: 'starting',
    generation: 0,
    recoveryCount: 0,
  },
  interaction: {
    enabled: true,
    visible: true,
    desktopForeground: true,
    historyExpanded: false,
    settingsOpen: false,
  },
  privacyScreen: false,
}
