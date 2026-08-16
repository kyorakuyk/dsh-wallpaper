import type { BackendMode } from '../domain/types.ts'
import { PREVIEW_APP_SNAPSHOT, type AppSnapshot } from './appSnapshot.ts'

export interface AppCoreClient {
  readonly native: boolean
  snapshot(): Promise<AppSnapshot>
  subscribe(listener: (snapshot: AppSnapshot) => void): Promise<() => void>
  setInteractionEnabled(enabled: boolean): Promise<AppSnapshot>
  selectBackend(backend: BackendMode): Promise<AppSnapshot>
  dispatch(action: AppCoreAction, options?: { playWake?: boolean; value?: string }): Promise<AppSnapshot>
}

export type AppCoreAction =
  | 'boot-ready' | 'lock' | 'unlock' | 'wake-done'
  | 'open-chat' | 'close-chat' | 'open-settings' | 'close-settings'
  | 'toggle-history' | 'auth-required' | 'auth-ready' | 'recover' | 'fail'
  | 'set-activity' | 'set-harness'

const isNative = '__TAURI_INTERNALS__' in window

export const appCoreClient: AppCoreClient = {
  native: isNative,
  async snapshot() {
    if (!isNative) return structuredClone(PREVIEW_APP_SNAPSHOT)
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AppSnapshot>('get_app_snapshot')
  },
  async subscribe(listener) {
    if (!isNative) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    return listen<AppSnapshot>('app-snapshot', (event) => listener(event.payload))
  },
  async setInteractionEnabled(enabled) {
    if (!isNative) return { ...PREVIEW_APP_SNAPSHOT, interaction: { ...PREVIEW_APP_SNAPSHOT.interaction, enabled, visible: enabled } }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AppSnapshot>('set_interaction_enabled', { enabled })
  },
  async selectBackend(backend) {
    if (!isNative) return { ...PREVIEW_APP_SNAPSHOT, backend }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AppSnapshot>('select_backend', { backend })
  },
  async dispatch(action, options) {
    if (!isNative) return structuredClone(PREVIEW_APP_SNAPSHOT)
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AppSnapshot>('dispatch_app_action', {
      action,
      playWake: options?.playWake,
      value: options?.value,
    })
  },
}
