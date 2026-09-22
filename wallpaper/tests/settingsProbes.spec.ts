import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'
import {
  createProbeScheduler,
  createSettingsProbeController,
  LOW_PRIORITY_PROBES,
  PAGE_PROBES,
  SETTINGS_PAGES,
  SETTINGS_PROBES,
  settingsProbeErrorMessage,
  type SettingsPage,
  type SettingsProbe,
} from '../src/settings/settingsProbes.ts'

const settingsRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'settings')

/** Collects scheduled callbacks so a test can decide when work actually runs. */
function manualScheduler() {
  const queued: Array<() => void> = []
  return {
    schedule: (run: () => void) => {
      queued.push(run)
      return () => {
        const index = queued.indexOf(run)
        if (index >= 0) queued.splice(index, 1)
      }
    },
    /** Run everything queued so far, including work queued by that work. */
    flush(rounds = 8) {
      for (let round = 0; round < rounds; round += 1) {
        if (queued.length === 0) return
        const batch = queued.splice(0, queued.length)
        for (const run of batch) run()
      }
    },
    get pending() { return queued.length },
  }
}

interface Harness {
  controller: ReturnType<typeof createSettingsProbeController>
  calls: SettingsProbe[]
  errors: Array<{ probe: SettingsProbe; error: unknown }>
  scheduler: ReturnType<typeof manualScheduler>
  /** Resolvers of probes that are deliberately left pending. */
  pending: Map<SettingsProbe, () => void>
}

function harness(options: { autoResolve?: boolean } = {}): Harness {
  const autoResolve = options.autoResolve ?? true
  const calls: SettingsProbe[] = []
  const errors: Array<{ probe: SettingsProbe; error: unknown }> = []
  const pending = new Map<SettingsProbe, () => void>()
  const scheduler = manualScheduler()
  const controller = createSettingsProbeController({
    runProbe: (probe) => {
      calls.push(probe)
      if (autoResolve) return Promise.resolve(undefined)
      return new Promise<void>((resolve) => pending.set(probe, resolve))
    },
    schedule: scheduler.schedule,
    onError: (probe, error) => errors.push({ probe, error }),
  })
  return { controller, calls, errors, scheduler, pending }
}

describe('settings probe plan', () => {
  it('never includes a disk-scanning probe in any page plan', async () => {
    // A full DSH directory scan is only ever a manual action. It must not be
    // reachable from the probe scheduler at all, which also proves that
    // opening the window cannot walk the disk.
    const planSource = await readFile(resolve(settingsRoot, 'settingsProbes.ts'), 'utf8')
    const windowSource = await readFile(resolve(settingsRoot, 'SettingsWindow.tsx'), 'utf8')
    expect(planSource).not.toContain('scanDsh')
    expect(planSource).not.toContain('scan_dsh_paths')
    expect(PAGE_PROBES.connections).not.toContain('scanDsh' as never)
    // The window only scans in response to the explicit button callback.
    expect(windowSource).toContain("onScanDsh={() => { void scanDsh(true) }}")
    expect(windowSource).not.toContain('void scanDsh()')
  })

  it('assigns each probe to exactly the pages that display it', () => {
    expect([...PAGE_PROBES.general]).toEqual(['desktopDisplays'])
    expect([...PAGE_PROBES.connections].sort()).toEqual(['deepseekWebAdapterConfig', 'managedDsh', 'translucentTb'])
    expect(PAGE_PROBES.appearance).toEqual([])
    expect(PAGE_PROBES.personas).toEqual([])
    expect([...PAGE_PROBES.system].sort()).toEqual(['autostartStatus', 'lockScreenDiagnostics'])
  })
})

describe('settings probe scheduling', () => {
  it('does not run the lock-screen or autostart probes before the system page is opened', () => {
    const test = harness()
    test.controller.activate('general')
    test.scheduler.flush()
    test.controller.activate('connections')

    // Only the pages that were actually opened contributed probes.
    expect(test.calls).toEqual(['desktopDisplays', 'managedDsh'])
    expect(test.calls).not.toContain('lockScreenDiagnostics')
    expect(test.calls).not.toContain('autostartStatus')

    test.controller.activate('system')
    expect(test.calls).toEqual(expect.arrayContaining(['lockScreenDiagnostics', 'autostartStatus']))
  })

  it('starts a page probe exactly once even across repeated page switches', () => {
    const test = harness()
    for (const page of ['connections', 'general', 'connections', 'connections'] as SettingsPage[]) {
      test.controller.activate(page)
      test.scheduler.flush()
    }
    expect(test.calls.filter((probe) => probe === 'managedDsh')).toHaveLength(1)
    expect(test.calls.filter((probe) => probe === 'translucentTb')).toHaveLength(1)
    expect(test.calls.filter((probe) => probe === 'desktopDisplays')).toHaveLength(1)
  })

  it('defers decorative probes behind the page data that gates controls', () => {
    const test = harness()
    test.controller.activate('connections')
    // The probe that gates the "启动 DSH" button is already running, while the
    // decorative probes are still waiting for their own idle slot.
    expect(test.calls).toEqual(['managedDsh'])
    expect(test.scheduler.pending).toBe(2)

    test.scheduler.flush()
    expect(test.calls).toEqual(expect.arrayContaining(['translucentTb', 'deepseekWebAdapterConfig']))
    for (const probe of LOW_PRIORITY_PROBES) expect(PAGE_PROBES.connections).toContain(probe)
  })

  it('never runs a second probe of the same kind while one is in flight', () => {
    const test = harness({ autoResolve: false })
    test.controller.activate('connections')
    expect(test.calls).toEqual(['managedDsh'])
    expect(test.controller.isInFlight('managedDsh')).toBe(true)

    // A manual refresh while the first run is still pending must be ignored.
    void test.controller.refresh('managedDsh')
    expect(test.scheduler.pending).toBe(2)
    expect(test.calls).toEqual(['managedDsh'])
  })

  it('lets an explicit refresh run a probe again after it settled', async () => {
    const test = harness()
    test.controller.activate('connections')
    test.scheduler.flush()
    await test.controller.settled('managedDsh')
    expect(test.controller.isInFlight('managedDsh')).toBe(false)

    test.controller.activate('connections')
    test.scheduler.flush()
    expect(test.calls.filter((probe) => probe === 'managedDsh')).toHaveLength(1)

    await test.controller.refresh('managedDsh')
    expect(test.calls.filter((probe) => probe === 'managedDsh')).toHaveLength(2)
  })

  it('reports a failed probe once without blocking the rest of the page', async () => {
    const errors: Array<{ probe: SettingsProbe; error: unknown }> = []
    const calls: SettingsProbe[] = []
    const scheduler = manualScheduler()
    const controller = createSettingsProbeController({
      runProbe: (probe) => {
        calls.push(probe)
        return probe === 'lockScreenDiagnostics' ? Promise.reject(new Error('系统探测失败')) : Promise.resolve(undefined)
      },
      schedule: scheduler.schedule,
      onError: (probe, error) => errors.push({ probe, error }),
    })

    controller.activate('system')
    scheduler.flush()
    await Promise.all([controller.settled('lockScreenDiagnostics'), controller.settled('autostartStatus')])
    expect(calls).toEqual(expect.arrayContaining(['lockScreenDiagnostics', 'autostartStatus']))
    expect(errors).toEqual([{ probe: 'lockScreenDiagnostics', error: expect.any(Error) }])
    expect(settingsProbeErrorMessage('lockScreenDiagnostics', new Error('系统探测失败'))).toBe('锁屏检查失败：Error: 系统探测失败')
  })

  it('cancels scheduled probes and stops reporting after disposal', async () => {
    const errors: Array<{ probe: SettingsProbe; error: unknown }> = []
    const calls: SettingsProbe[] = []
    const scheduler = manualScheduler()
    const controller = createSettingsProbeController({
      runProbe: (probe) => { calls.push(probe); return Promise.reject(new Error('晚到失败')) },
      schedule: scheduler.schedule,
      onError: (probe, error) => errors.push({ probe, error }),
    })

    controller.activate('system')
    // A probe already started before the window closed may reject; it must not
    // try to update (or notify) the closed window.
    controller.dispose()
    scheduler.flush()
    await Promise.all([controller.settled('lockScreenDiagnostics'), controller.settled('autostartStatus')])
    expect(calls).toEqual(['lockScreenDiagnostics', 'autostartStatus'])
    expect(errors).toEqual([])
    expect(scheduler.pending).toBe(0)
  })

  it('cancels decorative probes that were still waiting when the window closed', () => {
    const calls: SettingsProbe[] = []
    const scheduler = manualScheduler()
    const controller = createSettingsProbeController({
      runProbe: (probe) => { calls.push(probe); return Promise.resolve(undefined) },
      schedule: scheduler.schedule,
      onError: () => undefined,
    })

    controller.activate('connections')
    expect(calls).toEqual(['managedDsh'])
    controller.dispose()
    scheduler.flush()
    expect(calls).toEqual(['managedDsh'])
    expect(scheduler.pending).toBe(0)
  })

  it('ignores a probe result that arrives after disposal', async () => {
    const errors: Array<{ probe: SettingsProbe; error: unknown }> = []
    const scheduler = manualScheduler()
    let rejectProbe: ((error: unknown) => void) | undefined
    const controller = createSettingsProbeController({
      runProbe: () => new Promise((_resolve, reject) => { rejectProbe = reject }),
      schedule: scheduler.schedule,
      onError: (probe, error) => errors.push({ probe, error }),
    })

    controller.activate('general')
    scheduler.flush()
    controller.dispose()
    rejectProbe?.(new Error('late'))
    await Promise.resolve()
    await Promise.resolve()
    expect(errors).toEqual([])
  })
})

describe('probe scheduler fallback', () => {
  function fakeWindow(options: { idle?: boolean } = {}) {
    const frames = new Map<number, () => void>()
    const timers = new Map<number, () => void>()
    const idleCallbacks = new Map<number, () => void>()
    let next = 1
    const target = {
      requestAnimationFrame: vi.fn((callback: () => void) => { const handle = next++; frames.set(handle, callback); return handle }),
      cancelAnimationFrame: vi.fn((handle: number) => { frames.delete(handle) }),
      setTimeout: vi.fn((callback: () => void) => { const handle = next++; timers.set(handle, callback); return handle }),
      clearTimeout: vi.fn((handle: number) => { timers.delete(handle) }),
    } as unknown as Window & Record<string, unknown>
    if (options.idle) {
      target.requestIdleCallback = (callback: () => void) => { const handle = next++; idleCallbacks.set(handle, callback); return handle }
      target.cancelIdleCallback = (handle: number) => { idleCallbacks.delete(handle) }
    }
    return {
      target,
      runFrames() { const batch = [...frames.values()]; frames.clear(); for (const frame of batch) frame() },
      runTimers() { const batch = [...timers.values()]; timers.clear(); for (const timer of batch) timer() },
      runIdle() { const batch = [...idleCallbacks.values()]; idleCallbacks.clear(); for (const callback of batch) callback() },
      get frameCount() { return frames.size },
      get timerCount() { return timers.size },
    }
  }

  it('waits two animation frames, then uses requestIdleCallback when available', () => {
    const fake = fakeWindow({ idle: true })
    const run = vi.fn()
    createProbeScheduler(fake.target)(run)

    expect(run).not.toHaveBeenCalled()
    fake.runFrames()
    expect(run).not.toHaveBeenCalled()
    fake.runFrames()
    expect(run).not.toHaveBeenCalled()
    fake.runIdle()
    expect(run).toHaveBeenCalledTimes(1)
  })

  it('falls back to a timer when requestIdleCallback is unavailable', () => {
    const fake = fakeWindow()
    const run = vi.fn()
    createProbeScheduler(fake.target)(run)

    fake.runFrames()
    fake.runFrames()
    expect(run).not.toHaveBeenCalled()
    fake.runTimers()
    expect(run).toHaveBeenCalledTimes(1)
  })

  it('cancels pending frames and the idle callback on teardown', () => {
    const fake = fakeWindow({ idle: true })
    const run = vi.fn()
    const cancel = createProbeScheduler(fake.target)(run)

    fake.runFrames()
    fake.runFrames()
    cancel()
    fake.runIdle()
    fake.runTimers()
    expect(run).not.toHaveBeenCalled()
  })
})
