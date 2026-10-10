import type { LiteBackgroundId, LitePortraitId, LiteSettings } from './types.ts'
import { t } from '../i18n/index.ts'

export const LITE_SETTINGS_VERSION = 1

/**
 * 候选格子上的字。`label` 是 **getter**：这两张表在 import 时建好，而格子上写的是当下的语言
 * （与 `DEFAULT_BUBBLES` 同一条规矩）；`id` 与 `path` 是内部标识与素材路径，不随语言走。
 */
export interface LiteOption<Id extends string> {
  id: Id
  readonly label: string
  path: string
}

export const LITE_BACKGROUND_OPTIONS: Array<LiteOption<LiteBackgroundId>> = [
  { id: 'workspace', get label() { return t('lite.option.background.workspace') }, path: 'personas/deepsea-bg/deepsea-studio.webp' },
  { id: 'deepsea-2', get label() { return t('lite.option.background.deepsea-2') }, path: 'personas/deepsea-bg/deepsea-dome.webp' },
  { id: 'deepsea-3', get label() { return t('lite.option.background.deepsea-3') }, path: 'personas/deepsea-bg/deepsea-study.webp' },
]

export const LITE_PORTRAIT_OPTIONS: Array<LiteOption<Exclude<LitePortraitId, 'custom'>>> = [
  { id: 'blue-adult', get label() { return t('lite.option.portrait.blue-adult') }, path: 'personas/portrait-blue-adult.png' },
  { id: 'blue-child', get label() { return t('lite.option.portrait.blue-child') }, path: 'personas/portrait-blue-child.png' },
  { id: 'black-adult', get label() { return t('lite.option.portrait.black-adult') }, path: 'personas/portrait-black-adult.png' },
  { id: 'black-child', get label() { return t('lite.option.portrait.black-child') }, path: 'personas/portrait-black-child.png' },
]

export const DEFAULT_LITE_SETTINGS: LiteSettings = {
  version: LITE_SETTINGS_VERSION,
  background: 'workspace',
  portrait: 'blue-adult',
  animationsEnabled: true,
  animationSpeed: 1,
  playWakeOnEveryUnlock: true,
  skipWakeAnimation: false,
  // lockScreenEnabled: false,
  // FREEZE(1B)：随系统集成冻结（2026-09-30）。
  // desktopWallpaperFallback: false,
  autostart: false,
}

const STORAGE_KEY = 'dsh-wallpaper-lite:settings:v1'

export function assetUrl(path: string): string {
  return `${import.meta.env.BASE_URL}${path.replace(/^\//, '')}`
}

export function normalizeLiteSettings(raw: unknown): LiteSettings {
  if (raw === null || typeof raw !== 'object') return structuredClone(DEFAULT_LITE_SETTINGS)
  const value = raw as Partial<LiteSettings>
  const background = value.background === 'custom' || LITE_BACKGROUND_OPTIONS.some((option) => option.id === value.background)
    ? value.background!
    : DEFAULT_LITE_SETTINGS.background
  const portrait = value.portrait === 'custom' || LITE_PORTRAIT_OPTIONS.some((option) => option.id === value.portrait)
    ? value.portrait!
    : DEFAULT_LITE_SETTINGS.portrait
  const animationSpeed = typeof value.animationSpeed === 'number' && Number.isFinite(value.animationSpeed)
    ? Math.min(2, Math.max(.5, value.animationSpeed))
    : DEFAULT_LITE_SETTINGS.animationSpeed
  return {
    ...DEFAULT_LITE_SETTINGS,
    ...value,
    version: LITE_SETTINGS_VERSION,
    background,
    portrait,
    animationsEnabled: value.animationsEnabled !== false,
    animationSpeed,
    playWakeOnEveryUnlock: value.playWakeOnEveryUnlock !== false,
    skipWakeAnimation: value.skipWakeAnimation === true,
    // lockScreenEnabled: value.lockScreenEnabled === true,
    // FREEZE(1B)：随系统集成冻结（2026-09-30）。
    // desktopWallpaperFallback: value.desktopWallpaperFallback === true,
    autostart: value.autostart === true,
  }
}

function readBrowserSettings(): LiteSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? normalizeLiteSettings(JSON.parse(raw)) : structuredClone(DEFAULT_LITE_SETTINGS)
  } catch {
    return structuredClone(DEFAULT_LITE_SETTINGS)
  }
}

function writeBrowserSettings(settings: LiteSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings))
  } catch {
    // Browser preview storage is optional. Native builds use the Rust store.
  }
}

async function tauriAvailable(): Promise<boolean> {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

let saveQueue: Promise<void> = Promise.resolve()

export async function loadLiteSettings(): Promise<LiteSettings> {
  if (!await tauriAvailable()) return readBrowserSettings()
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const raw = await invoke<unknown>('lite_settings_get')
    return normalizeLiteSettings(raw)
  } catch {
    return readBrowserSettings()
  }
}

export async function saveLiteSettings(settings: LiteSettings): Promise<void> {
  const normalized = normalizeLiteSettings(settings)
  // Slider/input events can fire faster than Tauri IPC replies. Serialize
  // writes so the final user choice can never be overwritten by an older
  // in-flight value.
  saveQueue = saveQueue.catch(() => undefined).then(async () => {
    if (!await tauriAvailable()) {
      writeBrowserSettings(normalized)
      return
    }
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('lite_settings_save', { settings: normalized })
  })
  return saveQueue
}
