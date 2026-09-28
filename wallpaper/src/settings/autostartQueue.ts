import type { AutostartStatus } from '../native/runtime.ts'

export interface AutostartQueueOptions {
  /** The one write that may be on the wire. */
  send: (enabled: boolean) => Promise<AutostartStatus>
  /** Fired when a write starts and again when none is left to send. */
  onBusy: (busy: boolean) => void
  /**
   * `requested` is the value that produced `status`. It is deliberately not
   * "the latest intent": the caller can then tell a current answer from one a
   * newer toggle has already superseded.
   */
  onSettled: (status: AutostartStatus, requested: boolean) => void
  onError: (error: unknown) => void
}

/**
 * Serialize the autostart switch.
 *
 * There is one value on one machine, so two writes at the same time are
 * meaningless; the user's latest intent is not. The switch used to *drop* a
 * toggle made while a request was in flight, and the older response then
 * committed its own result over the newer intent — the second half of 关上之后
 * 不让打开了. This keeps at most one request on the wire, remembers the newest
 * intent, and sends that one as soon as the wire frees up.
 */
export function createAutostartQueue(options: AutostartQueueOptions) {
  let inFlight = false
  let queued: boolean | undefined

  const pump = async (): Promise<void> => {
    if (inFlight || queued === undefined) return
    const requested = queued
    queued = undefined
    inFlight = true
    options.onBusy(true)
    try {
      const status = await options.send(requested)
      options.onSettled(status, requested)
    } catch (error) {
      options.onError(error)
    } finally {
      inFlight = false
      // Stay busy while something is still queued: the switch must not become
      // editable in the gap between two writes.
      if (queued === undefined) options.onBusy(false)
      void pump()
    }
  }

  return {
    /** Ask Windows for `enabled`. A request still waiting replaces its own. */
    request(enabled: boolean): void {
      queued = enabled
      void pump()
    },
    /** Forget queued work. A request already on the wire still settles. */
    dispose(): void {
      queued = undefined
    },
  }
}

export type AutostartQueue = ReturnType<typeof createAutostartQueue>
