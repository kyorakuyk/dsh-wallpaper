import type { Activity, BackendMode, SystemPhase } from '../domain/types.ts'
import type { HarnessAvailability } from '../connect/harness.ts'

/**
 * The snapshot carries the same Harness state the probe produces. Re-exported
 * rather than redeclared: two independent unions previously drifted here (this
 * file knew only three of the states), which is how "the Bridge answered but is
 * not usable" kept collapsing into a single misleading label.
 */
export type { HarnessAvailability }

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
  /** Stable, non-sensitive reason for a non-ready Harness state. */
  harnessReasonCode?: string
  /**
   * A Bridge that was ready is not answering, and its death is not confirmed.
   *
   * Optional because it was added after the first snapshots: an older native side
   * simply omits it, and the renderer then shows no amber light rather than inventing
   * a suspicion of its own.
   */
  harnessProbing?: boolean
  wallpaperHost: WallpaperHostSnapshot
  interaction: InteractionSnapshot
  /**
   * 输入岛是否被当前布局固定在桌面上（中央玻璃悬浮）。
   *
   * 渲染层自己不读它 —— 它是报给原生的一份状态，摆在这里是为了让诊断能看到"球为什么不弹"。
   */
  islandPinned: boolean
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
  islandPinned: false,
  privacyScreen: false,
}
