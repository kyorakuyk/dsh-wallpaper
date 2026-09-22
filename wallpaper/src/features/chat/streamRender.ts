/**
 * Render-side batching for streaming chat output.
 *
 * A streaming turn produces one `delta` event per model token chunk, and the
 * previous implementation committed a React state update (and a `scrollTo`)
 * for every single one. On a resident wallpaper that is hundreds of renders
 * per answer. This module coalesces text inside a frame and keeps the history
 * DOM bounded without ever throwing away in-memory history.
 */

export type CommitScheduler = (run: () => void) => void

/**
 * Frame-scheduling target. A server-side render (markup tests, prerender) has
 * no animation frames, so a timer is used instead of throwing inside a render
 * or an event handler.
 */
export function frameSchedulerTarget(): Pick<Window, 'requestAnimationFrame' | 'cancelAnimationFrame'> {
  const host = globalThis.window as (Window & typeof globalThis) | undefined
  if (typeof host?.requestAnimationFrame === 'function' && typeof host.cancelAnimationFrame === 'function') return host
  return {
    requestAnimationFrame: (callback: FrameRequestCallback) => Number(setTimeout(() => callback(Date.now()), 16)),
    cancelAnimationFrame: (handle: number) => { clearTimeout(handle) },
  }
}

/**
 * Batch commits into one animation frame. The first delta of a frame pairs
 * with the flush; every later delta in the same frame joins that same commit,
 * so N deltas cost one render instead of N.
 */
export function createFrameScheduler(target: Window): CommitScheduler {
  let frame: number | undefined
  return (run) => {
    if (frame !== undefined) return
    frame = target.requestAnimationFrame(() => {
      frame = undefined
      run()
    })
  }
}

/**
 * Accumulates streaming deltas and hands them to `commit` at most once per
 * frame. Text order is preserved exactly: appends are concatenated in arrival
 * order, so the committed buffer is always a prefix of the final answer.
 */
export class StreamTextBuffer {
  private pending = ''
  private frame: number | undefined

  constructor(
    private readonly target: Pick<Window, 'requestAnimationFrame' | 'cancelAnimationFrame'>,
    private readonly commit: (text: string) => void,
  ) {}

  /** Queue one delta. Returns the text accumulated but not yet committed. */
  append(text: string): string {
    if (text.length === 0) return this.pending
    this.pending += text
    this.schedule()
    return this.pending
  }

  /** Text queued for the next frame, without scheduling anything. */
  get uncommitted(): string {
    return this.pending
  }

  /** Commit immediately. Used when a turn ends or the surface unmounts, so the
   * final answer cannot be lost to a cancelled frame. */
  flush(): string {
    if (this.frame !== undefined) {
      this.target.cancelAnimationFrame(this.frame)
      this.frame = undefined
    }
    const text = this.pending
    this.pending = ''
    if (text.length > 0) this.commit(text)
    return text
  }

  /** Drop everything queued and cancel the pending frame. */
  reset(): void {
    if (this.frame !== undefined) {
      this.target.cancelAnimationFrame(this.frame)
      this.frame = undefined
    }
    this.pending = ''
  }

  private schedule(): void {
    if (this.frame !== undefined) return
    this.frame = this.target.requestAnimationFrame(() => {
      this.frame = undefined
      const text = this.pending
      this.pending = ''
      if (text.length > 0) this.commit(text)
    })
  }
}

/** How many history messages the transcript renders by default. */
export const HISTORY_RENDER_WINDOW = 100

export interface HistoryWindow {
  /** Messages that should be in the DOM, oldest first. */
  visible: number
  /** Messages that exist but are not rendered yet. */
  hidden: number
  hasEarlier: boolean
}

/**
 * Window the rendered history. This only bounds the DOM: the caller keeps the
 * full in-memory transcript, so loading earlier is a render decision, never a
 * data loss. The newest message is always inside the window, which is what
 * keeps a streaming answer from being truncated.
 */
export function historyWindow(total: number, requested: number): HistoryWindow {
  const visible = Math.max(0, Math.min(total, Math.floor(requested)))
  return { visible, hidden: total - visible, hasEarlier: total > visible }
}

/** One step of "load earlier" history. */
export function growHistoryWindow(current: number, total: number): number {
  const next = Math.max(HISTORY_RENDER_WINDOW, current * 2)
  return Math.min(total, next)
}
