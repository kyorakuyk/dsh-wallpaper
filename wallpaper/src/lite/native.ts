import type { AutostartStatus } from '../native/runtime.ts'
import { t } from '../i18n/index.ts'

async function invokeNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

export async function nativeBootstrapGeneration(): Promise<number> {
  return invokeNative<number>('native_bootstrap_generation')
}

export async function releaseNativeBootstrap(generation: number): Promise<boolean> {
  return invokeNative<boolean>('release_native_bootstrap', { generation })
}

export async function autostartStatus(): Promise<AutostartStatus> {
  return invokeNative<AutostartStatus>('autostart_status')
}

export async function setAutostart(enabled: boolean): Promise<AutostartStatus> {
  return invokeNative<AutostartStatus>('set_autostart', { enabled })
}

export type LiteImageSlot = 'background' | 'portrait'

export interface LiteImageData {
  mimeType: string
  bytesBase64: string
}

export async function chooseImage(slot: LiteImageSlot): Promise<string | undefined> {
  if (!('__TAURI_INTERNALS__' in window)) return undefined
  const { open } = await import('@tauri-apps/plugin-dialog')
  const selected = await open({
    title: slot === 'background' ? t('lite.native.pick.background') : t('lite.native.pick.portrait'),
    multiple: false,
    directory: false,
    filters: [{ name: t('lite.native.image-filter'), extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
  })
  return typeof selected === 'string' ? selected : undefined
}

export async function importImage(slot: LiteImageSlot, path: string): Promise<void> {
  await invokeNative('lite_image_import', { slot, path })
}

export async function resolveImage(slot: LiteImageSlot): Promise<string | undefined> {
  if (!('__TAURI_INTERNALS__' in window)) return undefined
  const image = await invokeNative<LiteImageData | null>('lite_image_resolve', { slot })
  return image ? `data:${image.mimeType};base64,${image.bytesBase64}` : undefined
}
