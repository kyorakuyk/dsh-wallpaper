/**
 * Page-scoped, once-only system probes for the settings window.
 *
 * Opening settings used to fan out every Win32/PowerShell/IPC probe at once:
 * TranslucentTB detection, a full DSH directory scan, managed-DSH status,
 * autostart state, lock-screen diagnostics, display enumeration and the web
 * adapter configuration. That burst is what made the first paint wait on a
 * black frame. This module keeps the window's first frame free of probes and
 * starts each group only when its page is actually shown, at most once.
 */

export const SETTINGS_PAGES = ['general', 'connections', 'appearance', 'personas', 'history', 'system'] as const
export type SettingsPage = (typeof SETTINGS_PAGES)[number]

/**
 * Fallback cadence for re-reading the monitor list while its page is open. The
 * page also has an explicit refresh control, so this is a safety net for a
 * hot-plug that arrives while settings is open, not a resident poll.
 */
export const DISPLAY_LIST_FALLBACK_INTERVAL_MS = 30_000

export const SETTINGS_PROBES = [
  'translucentTb',
  'managedDsh',
  'deepseekWebAdapterConfig',
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，理由见 docs/plans/release-scope-cleanup-plan.md 第一节）。
  // 恢复办法：取消这里的注释，并把下面 PAGE_PROBES.system 与 PROBE_ERROR_MESSAGES 里同名的两处一起还原。
  // 'lockScreenDiagnostics',
  'autostartStatus',
  'desktopDisplays',
  'apiHistory',
] as const
export type SettingsProbe = (typeof SETTINGS_PROBES)[number]

/**
 * Probes a page needs to be usable. They start right after the window paints
 * its own content, still grouped per page so switching pages stays cheap.
 */
export const PAGE_PROBES: Record<SettingsPage, readonly SettingsProbe[]> = {
  general: ['desktopDisplays'],
  connections: ['managedDsh', 'translucentTb', 'deepseekWebAdapterConfig'],
  appearance: [],
  personas: [],
  history: ['apiHistory'],
  // FREEZE(1A)：同上。
  // system: ['lockScreenDiagnostics', 'autostartStatus'],
  system: ['autostartStatus'],
}

/**
 * Cheap, non-blocking probes that only decorate the page. They wait for an
 * idle slot instead of competing with the first paint and with the page's
 * required data. `managedDsh` is deliberately *not* here: it gates the
 * "启动 DSH" button, so it belongs to the required group.
 */
export const LOW_PRIORITY_PROBES: ReadonlySet<SettingsProbe> = new Set<SettingsProbe>([
  'translucentTb',
  'deepseekWebAdapterConfig',
])

export type ProbeScheduler = (run: () => void) => () => void

interface IdleDeadlineLike { didTimeout: boolean; timeRemaining(): number }

/**
 * Schedule work after the next two animation frames, then on the first idle
 * slot. `requestIdleCallback` is not available in every WebView2 runtime, so
 * the timeout fallback is part of the contract rather than a nicety.
 */
export function createProbeScheduler(target: Window): ProbeScheduler {
  return (run) => {
    let cancelled = false
    const frames: number[] = []
    let idle: number | undefined
    let timer: number | undefined

    const commit = () => {
      if (cancelled) return
      const idleCallback = (target as unknown as {
        requestIdleCallback?: (callback: (deadline: IdleDeadlineLike) => void, options?: { timeout: number }) => number
      }).requestIdleCallback
      if (typeof idleCallback === 'function') {
        idle = idleCallback.call(target, () => { if (!cancelled) run() }, { timeout: 1200 })
        return
      }
      timer = target.setTimeout(() => { if (!cancelled) run() }, 0)
    }

    frames.push(target.requestAnimationFrame(() => {
      frames.push(target.requestAnimationFrame(commit))
    }))

    return () => {
      cancelled = true
      for (const frame of frames) target.cancelAnimationFrame(frame)
      const cancelIdle = (target as unknown as { cancelIdleCallback?: (handle: number) => void }).cancelIdleCallback
      if (idle !== undefined && typeof cancelIdle === 'function') cancelIdle.call(target, idle)
      if (timer !== undefined) target.clearTimeout(timer)
    }
  }
}

export interface SettingsProbeController {
  /** Start the required + deferred probes for a page. Idempotent per page. */
  activate(page: SettingsPage): void
  /** True when a probe already ran (successfully or not) for this window. */
  hasRun(probe: SettingsProbe): boolean
  /** True while a probe's native call is still in flight. */
  isInFlight(probe: SettingsProbe): boolean
  /** Resolves once a probe's current/past run has settled. */
  settled(probe: SettingsProbe): Promise<void>
  /** Run a probe again on explicit user request; never overlaps a run. */
  refresh(probe: SettingsProbe): Promise<void>
  dispose(): void
}

export function createSettingsProbeController(options: {
  runProbe: (probe: SettingsProbe) => Promise<unknown>
  schedule: ProbeScheduler
  onError: (probe: SettingsProbe, error: unknown) => void
}): SettingsProbeController {
  const activated = new Set<SettingsPage>()
  const started = new Set<SettingsProbe>()
  const inFlight = new Set<SettingsProbe>()
  const settledRuns = new Map<SettingsProbe, Promise<void>>()
  const cancelScheduled = new Map<SettingsProbe, () => void>()
  let disposed = false

  /** Returns true when a new run started; false when one is in flight or the
   * controller is disposed. A manual refresh may repeat a probe, but never
   * while the previous run of that same probe is still pending. */
  const start = (probe: SettingsProbe, allowRepeat = false): boolean => {
    if (disposed) return false
    if (inFlight.has(probe)) return false
    if (started.has(probe) && !allowRepeat) return false
    started.add(probe)
    cancelScheduled.delete(probe)
    inFlight.add(probe)
    const pending = (async () => {
      try {
        await options.runProbe(probe)
      } catch (error) {
        if (!disposed) options.onError(probe, error)
      } finally {
        inFlight.delete(probe)
      }
    })()
    settledRuns.set(probe, pending)
    return true
  }

  const activate = (page: SettingsPage): void => {
    if (disposed || activated.has(page)) return
    activated.add(page)
    const required: SettingsProbe[] = []
    const deferred: SettingsProbe[] = []
    for (const probe of PAGE_PROBES[page]) {
      if (started.has(probe) || cancelScheduled.has(probe)) continue
      if (LOW_PRIORITY_PROBES.has(probe)) deferred.push(probe)
      else required.push(probe)
    }
    // The React effect that activates a page already runs after its first
    // paint, but the probes it starts are still async IPC. Required probes
    // start right away so a button that gates on one becomes usable as soon as
    // possible; decorative probes wait for an idle slot of their own so they
    // cannot compete with the page's real data.
    for (const probe of required) start(probe)
    for (const probe of deferred) cancelScheduled.set(probe, options.schedule(() => start(probe)))
  }

  return {
    activate,
    hasRun: (probe) => started.has(probe),
    isInFlight: (probe) => inFlight.has(probe),
    settled: (probe) => settledRuns.get(probe) ?? Promise.resolve(),
    async refresh(probe) {
      if (disposed) return
      if (start(probe, true)) await settledRuns.get(probe)
    },
    dispose() {
      disposed = true
      for (const cancel of cancelScheduled.values()) cancel()
      cancelScheduled.clear()
    },
  }
}

const PROBE_ERROR_MESSAGES: Record<SettingsProbe, string> = {
  translucentTb: '读取 TranslucentTB 状态失败',
  managedDsh: '读取受管 DSH 状态失败',
  deepseekWebAdapterConfig: '网页适配器配置读取失败',
  // FREEZE(1A)：同上。
  // lockScreenDiagnostics: '锁屏检查失败',
  autostartStatus: '读取开机自启状态失败',
  desktopDisplays: '显示器列表读取失败',
  apiHistory: '读取 API 会话记录失败',
}

export function settingsProbeErrorMessage(probe: SettingsProbe, error?: unknown): string {
  const suffix = error === undefined ? '' : `：${String(error)}`
  return `${PROBE_ERROR_MESSAGES[probe]}${suffix}`
}
