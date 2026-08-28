/**
 * Minimal native bridge for the Lite entry.
 *
 * Keeping this client separate from the full AppCore client is intentional:
 * the Lite bundle should not carry backend, conversation, or Harness labels
 * merely because the shared full-edition snapshot type happens to mention
 * them.  Rust still owns the complete snapshot; this entry only consumes the
 * phase field and the four lifecycle actions it is allowed to dispatch.
 */

export type LiteNativeAction = 'boot-ready' | 'unlock' | 'wake-done'

interface LiteNativeSnapshot {
  phase: string
}

const PREVIEW_SNAPSHOT: LiteNativeSnapshot = { phase: 'idle' }
const native = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const liteRuntime = {
  native,
  async snapshot(): Promise<LiteNativeSnapshot> {
    if (!native) return { ...PREVIEW_SNAPSHOT }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<LiteNativeSnapshot>('get_app_snapshot')
  },
  async subscribe(listener: (snapshot: LiteNativeSnapshot) => void): Promise<() => void> {
    if (!native) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    return listen<LiteNativeSnapshot>('app-snapshot', (event) => listener(event.payload))
  },
  async dispatch(action: LiteNativeAction, playWake?: boolean): Promise<LiteNativeSnapshot> {
    if (!native) return { ...PREVIEW_SNAPSHOT, phase: playWake === false ? 'idle' : 'waking' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<LiteNativeSnapshot>('dispatch_app_action', {
      action,
      playWake,
    })
  },
}
