import type { LiteBackgroundId, LitePortraitId, LiteSettings } from './types.ts'

export const LITE_SETTINGS_VERSION = 1

export const LITE_BACKGROUND_OPTIONS: Array<{ id: LiteBackgroundId; label: string; path: string }> = [
  { id: 'workspace', label: '深夜工作室', path: 'personas/deepsea-bg/deepsea-studio.png' },
  { id: 'deepsea-2', label: '深海穹顶舱', path: 'personas/deepsea-bg/deepsea-dome.png' },
  { id: 'deepsea-3', label: '深海书房', path: 'personas/deepsea-bg/deepsea-study.png' },
]

export const LITE_PORTRAIT_OPTIONS: Array<{ id: LitePortraitId; label: string; path: string }> = [
  { id: 'blue-adult', label: '蓝色成年形态', path: 'personas/portrait-blue-adult.png' },
  { id: 'blue-child', label: '蓝色幼年形态', path: 'personas/portrait-blue-child.png' },
  { id: 'black-adult', label: '黑红成年形态', path: 'personas/portrait-black-adult.png' },
  { id: 'black-child', label: '黑红幼年形态', path: 'personas/portrait-black-child.png' },
]

export const DEFAULT_LITE_SETTINGS: LiteSettings = {
  version: LITE_SETTINGS_VERSION,
  background: 'workspace',
  portrait: 'blue-adult',
  animationsEnabled: true,
  animationSpeed: 1,
  playWakeOnEveryUnlock: true,
  skipWakeAnimation: false,
  lockScreenEnabled: false,
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
    lockScreenEnabled: value.lockScreenEnabled === true,
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
