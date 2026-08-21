/** 版本化设置；桌面壳会以原生文件存储替换浏览器 localStorage。 */

import type { BackendMode, ConversationPolicy, ModelTierRule } from '../domain/types.ts'
import type { PersonaBubbles } from '../persona/types.ts'

export type InteractionLayout = 'floating' | 'taskbar-docked'

export const SETTINGS_VERSION = 7
/** Keep renderer validation aligned with the native request boundary. A value
 * beyond this ceiling is almost certainly a unit/configuration error and must
 * not be presented as a configured price when Rust deliberately ignores it. */
export const MAX_PRICE_PER_MILLION = 1_000_000
export const assetUrl = (path: string): string => `${import.meta.env.BASE_URL}${path.replace(/^\//, '')}`
const CONVERSATION_KEY = 'dsh-wallpaper:conversations:v1'
// Existing v1 Harness pointers were created before the bridge supplied the
// required DSH model/cwd context. Do not resume those invalid sessions; API
// and web pointers remain unaffected.
const HARNESS_POINTER_REVISION = 2

/** 背景选项：'default' = 主题色渐变；其他项 = 正式深海室内插画。 */
export const BACKGROUND_OPTIONS = [
  { id: 'workspace', name: '深夜工作室', path: 'personas/deepsea-bg/deepsea-studio.png' },
  { id: 'deepsea-2', name: '深海穹顶舱', path: 'personas/deepsea-bg/deepsea-dome.png' },
  { id: 'deepsea-3', name: '深海书房', path: 'personas/deepsea-bg/deepsea-study.png' },
  { id: 'default', name: '默认主题渐变', path: '' },
] as const

export type BackgroundId = (typeof BACKGROUND_OPTIONS)[number]['id']

export interface ApiSettings {
  baseUrl: string
  model: string
  /** 人民币／每百万 input tokens；留空时不估算费用。 */
  priceInputPerMillion?: number
  /** 人民币／每百万 output tokens；留空时不估算费用。 */
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
  interactionLayout: InteractionLayout
  floatingAnchor: { x: number; y: number }
  portraitAmbientLength: number
  portraitAmbientStrength: number
  /** 中央会话窗的亚克力不透明度（0=最通透，1=最实）。 */
  conversationOpacity: number
  /** 中央会话窗背景模糊半径（px）。 */
  conversationBlur: number
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
  interactionLayout: 'floating',
  floatingAnchor: { x: 0.5, y: 0.62 },
  portraitAmbientLength: 82,
  portraitAmbientStrength: 0.72,
  conversationOpacity: 0.74,
  conversationBlur: 19,
  deepseekApi: { baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
}

const KEY = 'dsh-wallpaper:settings:v6'

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
    portraitAmbientLength: typeof value.portraitAmbientLength === 'number' ? Math.min(100, Math.max(35, value.portraitAmbientLength)) : DEFAULT_SETTINGS.portraitAmbientLength,
    portraitAmbientStrength: typeof value.portraitAmbientStrength === 'number' ? Math.min(1, Math.max(0, value.portraitAmbientStrength)) : DEFAULT_SETTINGS.portraitAmbientStrength,
    conversationOpacity: typeof value.conversationOpacity === 'number' ? Math.min(.96, Math.max(.2, value.conversationOpacity)) : DEFAULT_SETTINGS.conversationOpacity,
    conversationBlur: typeof value.conversationBlur === 'number' ? Math.min(40, Math.max(0, value.conversationBlur)) : DEFAULT_SETTINGS.conversationBlur,
    deepseekApi: normalizeApiSettings(value.deepseekApi),
  }
}

/**
 * A zero price is a deliberate (and valid) configuration, while an empty,
 * malformed, negative, or non-finite value means "price not configured".
 * Keep that distinction through schema migrations and the native boundary.
 */
export function normalizedPrice(value: unknown): number | undefined {
  return typeof value === 'number'
    && Number.isFinite(value)
    && value >= 0
    && value <= MAX_PRICE_PER_MILLION
    ? value
    : undefined
}

function normalizeApiSettings(value: Partial<ApiSettings> | undefined): ApiSettings {
  return {
    ...DEFAULT_SETTINGS.deepseekApi,
    ...(value ?? {}),
    priceInputPerMillion: normalizedPrice(value?.priceInputPerMillion),
    priceOutputPerMillion: normalizedPrice(value?.priceOutputPerMillion),
  }
}

export function loadSettings(): WallpaperSettings {
  try {
  const raw = localStorage.getItem(KEY) ?? localStorage.getItem('dsh-wallpaper:settings:v6') ?? localStorage.getItem('dsh-wallpaper:settings:v5') ?? localStorage.getItem('dsh-wallpaper:settings:v4') ?? localStorage.getItem('dsh-wallpaper:settings:v3') ?? localStorage.getItem('dsh-wallpaper:settings:v2') ?? localStorage.getItem('dsh-wallpaper:settings')
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
  harness?: { id: string; updatedAt: number; day: string; bridgeRevision?: number }
}

/**
 * The "daily" lifecycle belongs to the user's local calendar day, not UTC.
 * `toISOString()` would incorrectly begin a new transcript around local
 * midnight for users outside UTC.
 */
export function localCalendarDay(now: Date = new Date()): string {
  const year = now.getFullYear()
  const month = String(now.getMonth() + 1).padStart(2, '0')
  const day = String(now.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

export function loadConversationPointers(): ConversationPointers {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(CONVERSATION_KEY) ?? '{}')
    return parsed && typeof parsed === 'object' ? parsed as ConversationPointers : {}
  } catch { return {} }
}

export function saveConversationPointer(backend: BackendMode, id: string, now: Date = new Date()): void {
  try {
    const pointers = loadConversationPointers()
    pointers[backend] = {
      id,
      updatedAt: now.getTime(),
      day: localCalendarDay(now),
      ...(backend === 'harness' ? { bridgeRevision: HARNESS_POINTER_REVISION } : {}),
    }
    localStorage.setItem(CONVERSATION_KEY, JSON.stringify(pointers))
  } catch { /* unavailable storage: start a fresh conversation next time */ }
}

export function resumeConversationId(backend: BackendMode, policy: ConversationPolicy, now: Date = new Date()): string | undefined {
  if (policy === 'new-on-unlock') return undefined
  const pointer = loadConversationPointers()[backend]
  if (!pointer) return undefined
  if (backend === 'harness'
    && (pointer as ConversationPointers['harness'])?.bridgeRevision !== HARNESS_POINTER_REVISION) return undefined
  if (policy === 'daily' && pointer.day !== localCalendarDay(now)) return undefined
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
