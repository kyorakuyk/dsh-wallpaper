/**
 * Local launch lifecycle of the Harness connection.
 *
 * This reducer only answers "what is this window doing about starting a Harness":
 * idle, waiting out the subject-switch buffer, supervising a launch, or showing a
 * failed attempt. `availability`, `probing` and the reason code stay in
 * `RuntimeState`, which is the snapshot source and is never copied in here.
 *
 * `startedAt` and `until` act as epochs. A timer or an awaited result carries the
 * value it was created with, and the reducer ignores it when the state has moved
 * on, so a stale timeout or a late failure cannot overwrite a newer launch.
 */
import type { HarnessAvailability } from '../../domain/types.ts'
import type { Message } from '../../i18n/index.ts'

export type HarnessLaunchKind = 'embedded-shell' | 'checkout' | 'installed-cli'

export type HarnessLaunch =
  | { kind: 'idle' }
  | { kind: 'buffering'; until: number }
  | { kind: 'starting'; startedAt: number; launchKind: HarnessLaunchKind }
  | { kind: 'failed'; reason: Message }

export type HarnessLaunchEvent =
  | { type: 'SUBJECT_SWITCHED'; until: number }
  | { type: 'BUFFER_ELAPSED'; until: number }
  | { type: 'LAUNCH_REQUESTED'; startedAt: number; launchKind: HarnessLaunchKind }
  | { type: 'LAUNCH_FAILED'; startedAt: number; reason: Message }
  | { type: 'LAUNCH_ABORTED'; startedAt: number }
  | { type: 'STATUS_RECEIVED'; availability: HarnessAvailability }

export const INITIAL_HARNESS_LAUNCH: HarnessLaunch = { kind: 'idle' }

export function reduceHarnessLaunch(state: HarnessLaunch, event: HarnessLaunchEvent): HarnessLaunch {
  switch (state.kind) {
    case 'idle':
      switch (event.type) {
        case 'SUBJECT_SWITCHED':
          return { kind: 'buffering', until: event.until }
        case 'LAUNCH_REQUESTED':
          return { kind: 'starting', startedAt: event.startedAt, launchKind: event.launchKind }
        default:
          return state
      }
    case 'buffering':
      switch (event.type) {
        case 'SUBJECT_SWITCHED':
          return { kind: 'buffering', until: event.until }
        case 'BUFFER_ELAPSED':
          return event.until === state.until ? INITIAL_HARNESS_LAUNCH : state
        case 'LAUNCH_REQUESTED':
          return { kind: 'starting', startedAt: event.startedAt, launchKind: event.launchKind }
        default:
          return state
      }
    case 'starting':
      switch (event.type) {
        case 'LAUNCH_FAILED':
          return event.startedAt === state.startedAt ? { kind: 'failed', reason: event.reason } : state
        case 'LAUNCH_ABORTED':
          return event.startedAt === state.startedAt ? INITIAL_HARNESS_LAUNCH : state
        case 'STATUS_RECEIVED':
          return event.availability === 'bridge-ready' ? INITIAL_HARNESS_LAUNCH : state
        default:
          return state
      }
    case 'failed':
      switch (event.type) {
        case 'LAUNCH_REQUESTED':
          return { kind: 'starting', startedAt: event.startedAt, launchKind: event.launchKind }
        case 'STATUS_RECEIVED':
          return event.availability === 'bridge-ready' ? INITIAL_HARNESS_LAUNCH : state
        default:
          return state
      }
  }
}

export interface HarnessLampInput {
  launch: HarnessLaunch
  availability: HarnessAvailability
  probing: boolean
}

export interface HarnessLamp {
  /** A launch is being supervised. */
  starting: boolean
  /** The last launch failed. Raw value: hiding it once the bridge is ready is the display layer's job. */
  failed: boolean
  /** Yellow light: starting, probing, the bridge still loading, or the subject-switch buffer. */
  suspect: boolean
}

export function deriveHarnessLamp(input: HarnessLampInput): HarnessLamp {
  const starting = input.launch.kind === 'starting'
  const buffering = input.launch.kind === 'buffering'
  return {
    starting,
    failed: input.launch.kind === 'failed',
    suspect: starting || input.probing || input.availability === 'bridge-loading' || buffering,
  }
}
