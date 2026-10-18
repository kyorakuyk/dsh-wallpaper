import { useCallback, useEffect, useReducer, useRef } from 'react'
import type { HarnessAvailability } from '../../domain/types.ts'
import type { Message } from '../../i18n/index.ts'
import { nativeRuntime } from '../../native/runtime.ts'
import { HARNESS_SWITCH_BUFFER_MS, harnessLaunchOutcome } from './harnessLaunch.ts'
import {
  deriveHarnessLamp,
  INITIAL_HARNESS_LAUNCH,
  reduceHarnessLaunch,
  type HarnessLaunchEvent,
  type HarnessLaunchKind,
} from './harnessConnection.ts'

interface UseHarnessConnectionOptions {
  subjectId?: string
  rootPath?: string
  endpointPort?: number
  extraEndpointPorts?: number[]
  availability: HarnessAvailability
  reasonCode?: string
  probing: boolean
  onLaunchFailed: (reason: Message) => void
  onSupervisionError: (error: unknown) => void
}

/**
 * Owns the window-local launch lifecycle and its native readiness supervisor.
 * The mirror advances before React's reducer dispatch so event handlers can
 * reject a second click immediately and async checks can discard stale results.
 */
export function useHarnessConnection({
  subjectId,
  rootPath,
  endpointPort,
  extraEndpointPorts,
  availability,
  reasonCode,
  probing,
  onLaunchFailed,
  onSupervisionError,
}: UseHarnessConnectionOptions) {
  const [launch, reactDispatch] = useReducer(reduceHarnessLaunch, INITIAL_HARNESS_LAUNCH)
  const launchMirrorRef = useRef(launch)
  const latestCallbacksRef = useRef({ onLaunchFailed, onSupervisionError })
  latestCallbacksRef.current = { onLaunchFailed, onSupervisionError }

  const dispatchEvent = useCallback((event: HarnessLaunchEvent) => {
    launchMirrorRef.current = reduceHarnessLaunch(launchMirrorRef.current, event)
    reactDispatch(event)
  }, [])

  const requestLaunch = useCallback((launchKind: HarnessLaunchKind, startedAt = Date.now()) => {
    dispatchEvent({ type: 'LAUNCH_REQUESTED', startedAt, launchKind })
    return startedAt
  }, [dispatchEvent])

  const abortLaunch = useCallback((startedAt: number) => {
    dispatchEvent({ type: 'LAUNCH_ABORTED', startedAt })
  }, [dispatchEvent])

  const isLaunchPending = useCallback(() => launchMirrorRef.current.kind === 'starting', [])

  // Keep this at the same App call site and preserve the dependency values:
  // the native scope effect still runs later and with the same count/order.
  useEffect(() => {
    const until = Date.now() + HARNESS_SWITCH_BUFFER_MS
    dispatchEvent({ type: 'SUBJECT_SWITCHED', until })
    const timer = window.setTimeout(() => dispatchEvent({ type: 'BUFFER_ELAPSED', until }), HARNESS_SWITCH_BUFFER_MS)
    return () => window.clearTimeout(timer)
  }, [subjectId, rootPath, endpointPort, extraEndpointPorts])

  useEffect(() => {
    dispatchEvent({ type: 'STATUS_RECEIVED', availability })
  }, [availability])

  const startedAt = launch.kind === 'starting' ? launch.startedAt : null
  useEffect(() => {
    if (!nativeRuntime.isNative || startedAt === null) return

    // A launch can begin while the bridge is already ready. Reconcile it here
    // as well as on availability changes, matching the old immediate check.
    if (availability === 'bridge-ready') {
      dispatchEvent({ type: 'STATUS_RECEIVED', availability })
      return
    }

    let disposed = false
    const check = async () => {
      try {
        const managed = await nativeRuntime.managedDshStatus()
        if (disposed) return
        const current = launchMirrorRef.current
        if (current.kind !== 'starting' || current.startedAt !== startedAt) return
        const outcome = harnessLaunchOutcome(
          { availability, reasonCode },
          Date.now() - startedAt,
          managed,
          45_000,
          current.launchKind,
        )
        if (!outcome) return
        dispatchEvent({ type: 'LAUNCH_FAILED', startedAt, reason: outcome.message })
        latestCallbacksRef.current.onLaunchFailed(outcome.message)
      } catch (error) {
        if (!disposed) latestCallbacksRef.current.onSupervisionError(error)
      }
    }

    void check()
    const timer = window.setInterval(() => { void check() }, 1000)
    return () => { disposed = true; window.clearInterval(timer) }
  }, [startedAt, availability])

  const lamp = deriveHarnessLamp({ launch, availability, probing })
  return { launch, lamp, requestLaunch, abortLaunch, isLaunchPending }
}
