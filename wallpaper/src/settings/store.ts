/** 版本化设置；桌面壳会以原生文件存储替换浏览器 localStorage。 */

import type { BackendMode, ConversationPolicy, ModelTierRule } from '../domain/types.ts'
import type { PersonaBubbles } from '../persona/types.ts'

export const SETTINGS_VERSION = 2
export const assetUrl = (path: string): string => `${import.meta.env.BASE_URL}${path.replace(/^\//, '')}`
const CONVERSATION_KEY = 'dsh-wallpaper:conversations:v1'

/** 背景选项：'default' = 主题色渐变；'deepsea-1/2/3' = 深海室内插画 */
export const BACKGROUND_OPTIONS = [
  { id: 'workspace', name: '深夜工作室', path: 'personas/deepsea-bg/bg-cand1.png' },
  { id: 'deepsea-2', name: '深海穹顶舱', path: 'personas/deepsea-bg/bg-cand2.png' },
  { id: 'deepsea-3', name: '深海书房', path: 'personas/deepsea-bg/bg-cand3.png' },
  { id: 'default', name: '默认主题渐变', path: '' },
] as const

export type BackgroundId = (typeof BACKGROUND_OPTIONS)[number]['id']

export interface ApiSettings {
  baseUrl: string
  model: string
  priceInputPerMillion?: number
  priceOutputPerMillion?: number
}

export interface WallpaperSettings {
  version: number
  defaultBackend: BackendMode
  autoSwitchHarness: boolean
  conversationPolicy: ConversationPolicy
  modelTierRules: ModelTierRule[]
  /** 气泡文案覆盖（key: 文案） */
  bubbleOverrides: Record<string, string>
  /** 动画开关 */
  animationsEnabled: boolean
  /** 动画速度倍率 */
  animationSpeed: number
  playWakeOnEveryUnlock: boolean
  skipWakeAnimation: boolean
  lockScreenEnabled: boolean
  autostart: boolean
  /** 应用内睡眠模式快捷键 */
  sleepHotkey: string
  background: BackgroundId
  historyStartsExpanded: boolean
  animationIntensity: 'low' | 'normal' | 'high'
  deepseekApi: ApiSettings
}

export const DEFAULT_SETTINGS: WallpaperSettings = {
  version: SETTINGS_VERSION,
  defaultBackend: 'deepseek-web',
  autoSwitchHarness: false,
  conversationPolicy: 'resume-last',
  modelTierRules: [],
  bubbleOverrides: {},
  animationsEnabled: true,
  animationSpeed: 1,
  playWakeOnEveryUnlock: true,
  skipWakeAnimation: false,
  lockScreenEnabled: false,
  autostart: false,
  sleepHotkey: 'Alt+W',
  background: 'workspace',
  historyStartsExpanded: false,
  animationIntensity: 'normal',
  deepseekApi: { baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
}

const KEY = 'dsh-wallpaper:settings:v2'

function migrate(raw: unknown): WallpaperSettings {
  if (raw === null || typeof raw !== 'object') return structuredClone(DEFAULT_SETTINGS)
  const value = raw as Partial<WallpaperSettings> & { autoSwitchPersona?: boolean }
  return {
    ...DEFAULT_SETTINGS,
    ...value,
    version: SETTINGS_VERSION,
    defaultBackend: value.defaultBackend ?? 'deepseek-web',
    autoSwitchHarness: value.autoSwitchHarness ?? value.autoSwitchPersona ?? false,
    modelTierRules: Array.isArray(value.modelTierRules) ? value.modelTierRules : [],
    bubbleOverrides: value.bubbleOverrides ?? {},
    deepseekApi: { ...DEFAULT_SETTINGS.deepseekApi, ...(value.deepseekApi ?? {}) },
  }
}

export function loadSettings(): WallpaperSettings {
  try {
    const raw = localStorage.getItem(KEY) ?? localStorage.getItem('dsh-wallpaper:settings')
    if (raw) return migrate(JSON.parse(raw))
  } catch {
    /* 忽略损坏的配置 */
  }
  return structuredClone(DEFAULT_SETTINGS)
}

export function saveSettings(s: WallpaperSettings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...s, version: SETTINGS_VERSION }))
  } catch {
    /* localStorage 不可用时静默 */
  }
}

export interface ConversationPointers {
  'deepseek-web'?: { id: string; updatedAt: number; day: string }
  'deepseek-api'?: { id: string; updatedAt: number; day: string }
  harness?: { id: string; updatedAt: number; day: string }
}

export function loadConversationPointers(): ConversationPointers {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(CONVERSATION_KEY) ?? '{}')
    return parsed && typeof parsed === 'object' ? parsed as ConversationPointers : {}
  } catch { return {} }
}

export function saveConversationPointer(backend: BackendMode, id: string): void {
  try {
    const pointers = loadConversationPointers()
    pointers[backend] = { id, updatedAt: Date.now(), day: new Date().toISOString().slice(0, 10) }
    localStorage.setItem(CONVERSATION_KEY, JSON.stringify(pointers))
  } catch { /* unavailable storage: start a fresh conversation next time */ }
}

export function resumeConversationId(backend: BackendMode, policy: ConversationPolicy): string | undefined {
  if (policy === 'new-on-unlock') return undefined
  const pointer = loadConversationPointers()[backend]
  if (!pointer) return undefined
  if (policy === 'daily' && pointer.day !== new Date().toISOString().slice(0, 10)) return undefined
  return pointer.id
}

/** 应用气泡覆盖：persona 的 bubbles 与设置覆盖合并 */
export function applyBubbleOverrides(
  bubbles: PersonaBubbles,
  overrides: Record<string, string>,
): PersonaBubbles {
  const nonEmpty = Object.fromEntries(Object.entries(overrides).filter(([, value]) => value.trim() !== ''))
  return { ...bubbles, ...nonEmpty }
}
