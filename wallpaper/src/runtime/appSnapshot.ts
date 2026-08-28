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

/**
 * Native actions are serialized in Rust, but their IPC responses can arrive
 * out of order in a busy WebView. AppCore increments `revision` for every
 * state change; only snapshots at or after the last applied revision may
 * repaint the renderer.
 */
export function shouldApplyAppSnapshot(lastRevision: number, nextRevision: number): boolean {
  return nextRevision >= lastRevision
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
