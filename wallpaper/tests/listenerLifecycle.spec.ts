import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'
import { asyncDisposer, listenUntilDisposed } from '../src/runtime/lifecycle.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** A subscription whose native disposer is only handed out when we say so. */
function deferredSubscription() {
  let release: ((disposer: () => void) => void) | undefined
  const disposer = vi.fn()
  const subscribe = vi.fn(() => new Promise<() => void>((resolve) => {
    release = (impl) => resolve(impl)
  }))
  return {
    subscribe,
    disposer,
    /** Resolve the pending `listen()` with the real disposer. */
    async settle() { release?.(disposer); await Promise.resolve(); await Promise.resolve() },
    async reject(error: unknown) { await Promise.resolve(); throw error },
  }
}

describe('async listener disposal', () => {
  it('releases the disposer when the component unmounts before the promise resolves', async () => {
    const pending = deferredSubscription()
    const listener = listenUntilDisposed<number>(pending.subscribe, () => undefined)

    // This is the leak the old `let dispose = () => undefined; return () =>
    // dispose()` pattern caused: cleanup ran first, so the real disposer was
    // dropped on the floor.
    listener.dispose()
    await pending.settle()
    expect(pending.disposer).toHaveBeenCalledTimes(1)
    expect(listener.disposed).toBe(true)
  })

  it('releases the disposer exactly once for a normal mount and unmount', async () => {
    const pending = deferredSubscription()
    const listener = listenUntilDisposed<number>(pending.subscribe, () => undefined)
    await pending.settle()
    expect(pending.disposer).not.toHaveBeenCalled()

    listener.dispose()
    expect(pending.disposer).toHaveBeenCalledTimes(1)
    // A second teardown (double-invoked cleanup, React StrictMode, a mode
    // switch) must not call the native unlisten again.
    listener.dispose()
    listener.dispose()
    expect(pending.disposer).toHaveBeenCalledTimes(1)
  })

  it('never delivers an event after disposal', async () => {
    const pending = deferredSubscription()
    const received: number[] = []
    const listener = listenUntilDisposed<number>(pending.subscribe, (event) => received.push(event))
    await pending.settle()

    const emit = pending.subscribe.mock.calls[0]?.[0]
    expect(emit).toBeTypeOf('function')
    emit(1)
    listener.dispose()
    emit(2)
    emit(3)
    expect(received).toEqual([1])
  })

  it('converts a rejected subscription into a reported error', async () => {
    const onError = vi.fn()
    const subscribe = vi.fn(async () => { throw new Error('native listen failed') })
    const listener = listenUntilDisposed<number>(subscribe, () => undefined, { onError })

    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
    expect(onError).toHaveBeenCalledTimes(1)
    expect(String(onError.mock.calls[0]?.[0])).toContain('native listen failed')
    expect(listener.settled).toBe(false)
    // Disposing after a failed subscription is still safe and idempotent.
    listener.dispose()
    listener.dispose()
  })

  it('swallows a rejected subscription when there is no error reporter', async () => {
    const subscribe = vi.fn(async () => { throw new Error('ignored') })
    expect(() => listenUntilDisposed<number>(subscribe, () => undefined)).not.toThrow()
    await Promise.resolve()
    await Promise.resolve()
  })

  it('releases a disposer that arrives after a rejection-style teardown', async () => {
    let resolveDisposer: ((impl: () => void) => void) | undefined
    const disposer = vi.fn()
    const listener = listenUntilDisposed<number>(
      () => new Promise<() => void>((resolve) => { resolveDisposer = resolve }),
      () => undefined,
    )
    listener.dispose()
    resolveDisposer?.(disposer)
    await Promise.resolve()
    await Promise.resolve()
    expect(disposer).toHaveBeenCalledTimes(1)
  })

  it('behaves the same through the disposed-flag helper', async () => {
    let disposed = false
    let releaseImpl: ((impl: () => void) => void) | undefined
    const disposer = vi.fn()
    const release = asyncDisposer(
      () => new Promise<() => void>((resolve) => { releaseImpl = resolve }),
      () => disposed,
    )
    disposed = true
    release()
    releaseImpl?.(disposer)
    await Promise.resolve()
    await Promise.resolve()
    expect(disposer).toHaveBeenCalledTimes(1)
  })

  it('reports asyncDisposer failures instead of rejecting unhandled', async () => {
    const onError = vi.fn()
    asyncDisposer(async () => { throw new Error('boom') }, () => false, { onError })
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
    expect(onError).toHaveBeenCalledTimes(1)
  })
})

describe('listener wiring', () => {
  it('no longer uses a fire-and-forget disposer placeholder anywhere', async () => {
    const sources = await Promise.all([
      'src/App.tsx',
      'src/lite/LiteApp.tsx',
      'src/lite/LiteSettingsWindow.tsx',
      'src/settings/SettingsWindow.tsx',
    ].map(async (relative) => [relative, (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')] as const))

    for (const [relative, source] of sources) {
      // The leaking shape: a placeholder disposer that a late promise
      // overwrites, combined with a cleanup that already ran.
      expect(source, relative).not.toMatch(/let dispose: \(\) => void = \(\) => undefined/)
      expect(source, relative).not.toMatch(/let unsubscribe: \(\) => void = \(\) => undefined/)
      expect(source, relative).not.toContain('then((unlisten) => { dispose = unlisten })')
      expect(source, relative).not.toContain('then((dispose) => { unsubscribe = dispose })')
    }
  })

  it('routes every event subscription through the lifecycle helper', async () => {
    const app = (await readFile(resolve(wallpaperRoot, 'src/App.tsx'), 'utf8')).replace(/\r\n?/g, '\n')
    const lite = (await readFile(resolve(wallpaperRoot, 'src/lite/LiteApp.tsx'), 'utf8')).replace(/\r\n?/g, '\n')

    // `settings-changed`, `appearance-changed`, display, system-session, tray
    // and workspace listeners are all owned by the helper.
    expect(app.match(/listenUntilDisposed/g)?.length ?? 0).toBeGreaterThanOrEqual(6)
    expect(lite.match(/listenUntilDisposed/g)?.length ?? 0).toBeGreaterThanOrEqual(3)
    expect(app).toContain('appCoreClient.subscribe(onSnapshot)')
  })
})
