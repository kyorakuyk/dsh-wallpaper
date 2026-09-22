/**
 * Async-safe teardown helpers.
 *
 * Tauri's `listen()`/`subscribe()` return a disposer through a Promise. The
 * naive pattern
 *
 * ```ts
 * let dispose = () => undefined
 * void listen(...).then((unlisten) => { dispose = unlisten })
 * return () => dispose()
 * ```
 *
 * silently leaks the native listener whenever the component unmounts before
 * that Promise settles: the cleanup already ran against the placeholder, so
 * the real disposer is never called and the subscription survives for the
 * lifetime of the WebView. Every listener in the app must go through these
 * helpers instead.
 */

export interface AsyncEventListenerHandle {
  /** Releases the listener. Safe to call before, during, or after settle. */
  dispose(): void
  /** True once the listener is released (or was released before it settled). */
  readonly disposed: boolean
  /** True once the subscription promise settled (either way). */
  readonly settled: boolean
}

/**
 * Connect one asynchronous listener with a dispose contract that cannot leak.
 *
 * - a promise that settles after `dispose()` immediately releases the
 *   disposer it received instead of storing it;
 * - the disposer is called at most once, no matter how often `dispose()` runs;
 * - the callback never fires after disposal;
 * - a rejected subscription promise is reported to `onError` (or swallowed)
 *   and never becomes an unhandled rejection.
 */
export function listenUntilDisposed<T>(
  subscribe: (listener: (event: T) => void) => Promise<() => void>,
  listener: (event: T) => void,
  options: { onError?: (error: unknown) => void } = {},
): AsyncEventListenerHandle {
  let settled = false
  let released = false
  let impl: (() => void) | undefined

  const release = () => {
    if (released) return
    released = true
    const disposer = impl
    impl = undefined
    if (disposer) {
      try {
        disposer()
      } catch (error) {
        options.onError?.(error)
      }
    }
  }

  void subscribe((event) => {
    if (released) return
    listener(event)
  }).then((disposer) => {
    settled = true
    if (released) {
      // The component is gone; release immediately rather than storing it.
      try {
        disposer()
      } catch (error) {
        options.onError?.(error)
      }
      return
    }
    impl = disposer
  }).catch((error: unknown) => {
    // Never let a failed native subscription escape as an unhandled rejection.
    options.onError?.(error)
  })

  return {
    dispose: release,
    get disposed() { return released },
    get settled() { return settled },
  }
}

/**
 * Run an async subscription exactly once for the lifetime of a caller that
 * already owns a `disposed` flag. Returns the synchronous disposer to hand to
 * `useEffect`.
 */
export function asyncDisposer(
  subscribe: () => Promise<() => void>,
  isDisposed: () => boolean,
  options: { onError?: (error: unknown) => void } = {},
): () => void {
  let released = false
  let impl: (() => void) | undefined
  const release = () => {
    if (released) return
    released = true
    impl?.()
    impl = undefined
  }
  void subscribe().then((disposer) => {
    if (released || isDisposed()) {
      disposer()
      return
    }
    impl = disposer
  }).catch((error: unknown) => {
    options.onError?.(error)
  })
  return release
}
