import { beforeAll, describe, expect, it } from 'vitest'
import type { HarnessAvailability } from '../src/domain/types.ts'
import { msg } from '../src/i18n/index.ts'
import {
  INITIAL_HARNESS_LAUNCH,
  deriveHarnessLamp,
  reduceHarnessLaunch,
  type HarnessLaunch,
} from '../src/features/harness/harnessConnection.ts'

// App.tsx evaluates a browser preview fallback on import; a minimal window is enough
// for the equivalence check against `isHarnessTransitioning` below.
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

const reason = msg('app.bubble.launch.timeout')
const otherReason = msg('app.bubble.launch.exited-early')

const idle: HarnessLaunch = { kind: 'idle' }
const buffering: HarnessLaunch = { kind: 'buffering', until: 100 }
const starting: HarnessLaunch = { kind: 'starting', startedAt: 10, launchKind: 'checkout' }
const failed: HarnessLaunch = { kind: 'failed', reason }

const startedFromRequest: HarnessLaunch = { kind: 'starting', startedAt: 20, launchKind: 'installed-cli' }
const request = { type: 'LAUNCH_REQUESTED', startedAt: 20, launchKind: 'installed-cli' } as const

describe('reduceHarnessLaunch: idle', () => {
  it('idle + SUBJECT_SWITCHED starts buffering', () => {
    expect(reduceHarnessLaunch(idle, { type: 'SUBJECT_SWITCHED', until: 5 })).toEqual({ kind: 'buffering', until: 5 })
  })
  it('idle + BUFFER_ELAPSED is unchanged', () => {
    expect(reduceHarnessLaunch(idle, { type: 'BUFFER_ELAPSED', until: 5 })).toBe(idle)
  })
  it('idle + LAUNCH_REQUESTED starts a launch', () => {
    expect(reduceHarnessLaunch(idle, request)).toEqual(startedFromRequest)
  })
  it('idle + LAUNCH_FAILED is unchanged', () => {
    expect(reduceHarnessLaunch(idle, { type: 'LAUNCH_FAILED', startedAt: 10, reason })).toBe(idle)
  })
  it('idle + LAUNCH_ABORTED is unchanged', () => {
    expect(reduceHarnessLaunch(idle, { type: 'LAUNCH_ABORTED', startedAt: 10 })).toBe(idle)
  })
  it('idle + STATUS ready is unchanged', () => {
    expect(reduceHarnessLaunch(idle, { type: 'STATUS_RECEIVED', availability: 'bridge-ready' })).toBe(idle)
  })
  it('idle + STATUS not ready is unchanged', () => {
    for (const availability of ['offline', 'bridge-loading'] as const) {
      expect(reduceHarnessLaunch(idle, { type: 'STATUS_RECEIVED', availability })).toBe(idle)
    }
  })
})

describe('reduceHarnessLaunch: buffering', () => {
  it('buffering + SUBJECT_SWITCHED restarts the buffer with the new deadline', () => {
    expect(reduceHarnessLaunch(buffering, { type: 'SUBJECT_SWITCHED', until: 200 })).toEqual({ kind: 'buffering', until: 200 })
  })
  it('buffering + BUFFER_ELAPSED ends the buffer when until matches, otherwise is unchanged', () => {
    expect(reduceHarnessLaunch(buffering, { type: 'BUFFER_ELAPSED', until: 100 })).toEqual(idle)
    expect(reduceHarnessLaunch(buffering, { type: 'BUFFER_ELAPSED', until: 99 })).toBe(buffering)
  })
  it('buffering + LAUNCH_REQUESTED ends the buffer and starts a launch', () => {
    expect(reduceHarnessLaunch(buffering, request)).toEqual(startedFromRequest)
  })
  it('buffering + LAUNCH_FAILED is unchanged', () => {
    expect(reduceHarnessLaunch(buffering, { type: 'LAUNCH_FAILED', startedAt: 10, reason })).toBe(buffering)
  })
  it('buffering + LAUNCH_ABORTED is unchanged', () => {
    expect(reduceHarnessLaunch(buffering, { type: 'LAUNCH_ABORTED', startedAt: 10 })).toBe(buffering)
  })
  it('buffering + STATUS ready does not end the buffer early', () => {
    expect(reduceHarnessLaunch(buffering, { type: 'STATUS_RECEIVED', availability: 'bridge-ready' })).toBe(buffering)
  })
  it('buffering + STATUS not ready is unchanged', () => {
    for (const availability of ['offline', 'bridge-loading'] as const) {
      expect(reduceHarnessLaunch(buffering, { type: 'STATUS_RECEIVED', availability })).toBe(buffering)
    }
  })
})

describe('reduceHarnessLaunch: starting', () => {
  it('starting + SUBJECT_SWITCHED keeps supervising the launch', () => {
    expect(reduceHarnessLaunch(starting, { type: 'SUBJECT_SWITCHED', until: 200 })).toBe(starting)
  })
  it('starting + BUFFER_ELAPSED is unchanged', () => {
    expect(reduceHarnessLaunch(starting, { type: 'BUFFER_ELAPSED', until: 100 })).toBe(starting)
  })
  it('starting + LAUNCH_REQUESTED is idempotent', () => {
    expect(reduceHarnessLaunch(starting, request)).toBe(starting)
  })
  it('starting + LAUNCH_FAILED fails when startedAt matches, otherwise is unchanged', () => {
    expect(reduceHarnessLaunch(starting, { type: 'LAUNCH_FAILED', startedAt: 10, reason })).toEqual({ kind: 'failed', reason })
    expect(reduceHarnessLaunch(starting, { type: 'LAUNCH_FAILED', startedAt: 9, reason })).toBe(starting)
  })
  it('starting + LAUNCH_ABORTED goes idle when startedAt matches, otherwise is unchanged', () => {
    expect(reduceHarnessLaunch(starting, { type: 'LAUNCH_ABORTED', startedAt: 10 })).toEqual(idle)
    expect(reduceHarnessLaunch(starting, { type: 'LAUNCH_ABORTED', startedAt: 9 })).toBe(starting)
  })
  it('starting + STATUS ready goes idle', () => {
    expect(reduceHarnessLaunch(starting, { type: 'STATUS_RECEIVED', availability: 'bridge-ready' })).toEqual(idle)
  })
  it('starting + STATUS not ready is unchanged', () => {
    for (const availability of ['offline', 'bridge-loading'] as const) {
      expect(reduceHarnessLaunch(starting, { type: 'STATUS_RECEIVED', availability })).toBe(starting)
    }
  })
})

describe('reduceHarnessLaunch: failed', () => {
  it('failed + SUBJECT_SWITCHED keeps the failure visible', () => {
    expect(reduceHarnessLaunch(failed, { type: 'SUBJECT_SWITCHED', until: 200 })).toBe(failed)
  })
  it('failed + BUFFER_ELAPSED is unchanged', () => {
    expect(reduceHarnessLaunch(failed, { type: 'BUFFER_ELAPSED', until: 100 })).toBe(failed)
  })
  it('failed + LAUNCH_REQUESTED clears the failure and starts a launch', () => {
    expect(reduceHarnessLaunch(failed, request)).toEqual(startedFromRequest)
  })
  it('failed + LAUNCH_FAILED is unchanged', () => {
    expect(reduceHarnessLaunch(failed, { type: 'LAUNCH_FAILED', startedAt: 10, reason: otherReason })).toBe(failed)
  })
  it('failed + LAUNCH_ABORTED is unchanged', () => {
    expect(reduceHarnessLaunch(failed, { type: 'LAUNCH_ABORTED', startedAt: 10 })).toBe(failed)
  })
  it('failed + STATUS ready clears the failure', () => {
    expect(reduceHarnessLaunch(failed, { type: 'STATUS_RECEIVED', availability: 'bridge-ready' })).toEqual(idle)
  })
  it('failed + STATUS not ready is unchanged', () => {
    for (const availability of ['offline', 'bridge-loading'] as const) {
      expect(reduceHarnessLaunch(failed, { type: 'STATUS_RECEIVED', availability })).toBe(failed)
    }
  })
})

describe('reduceHarnessLaunch: epochs', () => {
  it('ignores a BUFFER_ELAPSED from an older deadline while buffering', () => {
    expect(reduceHarnessLaunch(buffering, { type: 'BUFFER_ELAPSED', until: 50 })).toBe(buffering)
  })
  it('ignores LAUNCH_FAILED and LAUNCH_ABORTED from an older launch while starting', () => {
    expect(reduceHarnessLaunch(starting, { type: 'LAUNCH_FAILED', startedAt: 1, reason })).toBe(starting)
    expect(reduceHarnessLaunch(starting, { type: 'LAUNCH_ABORTED', startedAt: 1 })).toBe(starting)
  })
  it('lets only the latest deadline end the buffer after two subject switches', () => {
    const first = reduceHarnessLaunch(INITIAL_HARNESS_LAUNCH, { type: 'SUBJECT_SWITCHED', until: 100 })
    const second = reduceHarnessLaunch(first, { type: 'SUBJECT_SWITCHED', until: 200 })
    const afterOld = reduceHarnessLaunch(second, { type: 'BUFFER_ELAPSED', until: 100 })
    expect(afterOld).toBe(second)
    expect(reduceHarnessLaunch(afterOld, { type: 'BUFFER_ELAPSED', until: 200 })).toEqual(idle)
  })
})

describe('deriveHarnessLamp', () => {
  const launches: Array<[string, HarnessLaunch]> = [
    ['idle', idle],
    ['buffering', buffering],
    ['starting', starting],
    ['failed', failed],
  ]
  const availabilities: HarnessAvailability[] = ['offline', 'bridge-loading', 'bridge-ready']

  // Reference: the three values App.tsx passes to ConversationBubble before this module exists.
  async function today(launch: HarnessLaunch, availability: HarnessAvailability, probing: boolean) {
    const { isHarnessTransitioning } = await import('../src/App.tsx')
    const harnessStarting = launch.kind === 'starting'
    const harnessFailed = launch.kind === 'failed'
    const harnessBuffering = launch.kind === 'buffering'
    return {
      starting: harnessStarting,
      failed: harnessFailed,
      suspect: isHarnessTransitioning({ starting: harnessStarting, availability, probing }) || harnessBuffering,
    }
  }

  for (const [name, launch] of launches) {
    for (const availability of availabilities) {
      for (const probing of [false, true]) {
        it(`matches the current formula for ${name} / ${availability} / probing=${probing}`, async () => {
          expect(deriveHarnessLamp({ launch, availability, probing })).toEqual(await today(launch, availability, probing))
        })
      }
    }
  }

  it('reports the raw failed flag even when the bridge is ready', () => {
    expect(deriveHarnessLamp({ launch: failed, availability: 'bridge-ready', probing: false }).failed).toBe(true)
  })
})
