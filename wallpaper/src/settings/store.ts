/** 版本化设置；桌面壳会以原生文件存储替换浏览器 localStorage。 */

import type { BackendMode, ConversationPolicy, ModelTierRule } from '../domain/types.ts'
import type { PersonaBubbles } from '../persona/types.ts'
import { assetUrl } from '../runtime/assets.ts'

export { assetUrl }

export type InteractionLayout = 'floating' | 'taskbar-docked'

export const SETTINGS_VERSION = 10
/** Keep renderer validation aligned with the native request boundary. A value
 * beyond this ceiling is almost certainly a unit/configuration error and must
 * not be presented as a configured price when Rust deliberately ignores it. */
export const MAX_PRICE_PER_MILLION = 1_000_000
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

export interface MultiScreenSettings {
  /** Keep the original single-screen composition until the user opts in. */
  enabled: boolean
  /** Missing entries follow the global `background` choice. */
  backgrounds: Partial<Record<string, BackgroundId>>
  conversationDisplayId?: string
  portraitDisplayId?: string
}

export interface ApiSettings {
  baseUrl: string
  model: string
  /** 人民币／每百万 input tokens；留空时不估算费用。 */
  priceInputPerMillion?: number
  /** 人民币／每百万 output tokens；留空时不估算费用。 */
  priceOutputPerMillion?: number
}

export interface DshLaunchSettings {
  rootPath?: string
  profile: string
  command?: string
  /**
   * Start the configured DSH once, every time the wallpaper starts.
   *
   * This is a wallpaper-start trigger, not a login trigger: it is only useful
   * together with the wallpaper's own Windows autostart, which the settings
   * card says explicitly.
   */
  autoStartWithWallpaper: boolean
  /**
   * Explicit confirmation that `command` may be used by the *automatic* path.
   *
   * The manual "启动" button always honours `command`. Executing an arbitrary
   * configured program unattended at every wallpaper start is a different
   * trust decision, so it stays off until the user confirms it in settings.
   */
  trustedCommandForAutoStart: boolean
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
  /** 发送消息快捷键 */
  sendShortcut: 'Enter' | 'Ctrl+Enter'
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
  multiScreen: MultiScreenSettings
  deepseekApi: ApiSettings
  dshLaunch: DshLaunchSettings
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
  sendShortcut: 'Enter',
  background: 'workspace',
  historyStartsExpanded: false,
  animationIntensity: 'normal',
  interactionLayout: 'floating',
  floatingAnchor: { x: 0.5, y: 0.62 },
  portraitAmbientLength: 82,
  portraitAmbientStrength: 0.72,
  conversationOpacity: 0.74,
  conversationBlur: 19,
  multiScreen: { enabled: false, backgrounds: {} },
  deepseekApi: { baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  dshLaunch: { profile: 'desktop', autoStartWithWallpaper: false, trustedCommandForAutoStart: false },
}

const KEY = 'dsh-wallpaper:settings:v10'

/**
 * Keys read during migration, newest first. Renaming the key must never drop an
 * existing configuration, so every previous key stays readable and the first
 * one present wins. `v9` is listed explicitly: the chain below used to jump
 * straight from v8 to v7, which would have silently reset every setting for a
 * user who was on v9.
 */
const LEGACY_SETTINGS_KEYS = [
  KEY,
  'dsh-wallpaper:settings:v9',
  'dsh-wallpaper:settings:v8',
  'dsh-wallpaper:settings:v7',
  'dsh-wallpaper:settings:v6',
  'dsh-wallpaper:settings:v5',
  'dsh-wallpaper:settings:v4',
  'dsh-wallpaper:settings:v3',
  'dsh-wallpaper:settings:v2',
  'dsh-wallpaper:settings',
]

/** The most recent stored document, including the migrated `v8`/`v7` era. */
export function readStoredSettingsDocument(): string | null {
  for (const candidate of LEGACY_SETTINGS_KEYS) {
    const raw = localStorage.getItem(candidate)
    if (raw !== null) return raw
  }
  return null
}

/** Renderer-side length ceilings. They bound what one settings payload can
 * push into storage, the native publish channel, and the request boundary. */
export const MAX_SETTINGS_STRING = 2048
export const MAX_SETTINGS_SHORT_STRING = 128
export const MAX_BUBBLE_OVERRIDE_LENGTH = 2000
export const MAX_MODEL_TIER_RULES = 64
export const MAX_BUBBLE_OVERRIDES = 256

const BACKEND_MODES: readonly BackendMode[] = ['deepseek-web', 'deepseek-api', 'harness']
const CONVERSATION_POLICIES: readonly ConversationPolicy[] = ['resume-last', 'new-on-unlock', 'daily']
const INTERACTION_LAYOUTS: readonly WallpaperSettings['interactionLayout'][] = ['floating', 'taskbar-docked']
const ANIMATION_INTENSITIES: readonly WallpaperSettings['animationIntensity'][] = ['low', 'normal', 'high']
const MODEL_TIER_MATCHES: readonly ModelTierRule['match'][] = ['exact', 'contains', 'regex']
const MODEL_TIER_TIERS: readonly ModelTierRule['tier'][] = ['flash', 'pro']
const BACKGROUND_IDS: readonly string[] = BACKGROUND_OPTIONS.map((option) => option.id)

function settingsBool(candidate: unknown, fallback: boolean): boolean {
  return typeof candidate === 'boolean' ? candidate : fallback
}

function finiteNumber(candidate: unknown, fallback: number): number {
  return typeof candidate === 'number' && Number.isFinite(candidate) ? candidate : fallback
}

function boundedNumber(candidate: unknown, fallback: number, min: number, max: number): number {
  return candidate === undefined ? fallback : Math.min(max, Math.max(min, finiteNumber(candidate, fallback)))
}

function settingsText(candidate: unknown, fallback: string, maxLength = MAX_SETTINGS_STRING): string {
  if (typeof candidate !== 'string') return fallback
  const trimmed = candidate.trim()
  return trimmed.length > 0 && trimmed.length <= maxLength ? trimmed : fallback
}

function optionalText(candidate: unknown, maxLength = MAX_SETTINGS_STRING): string | undefined {
  if (typeof candidate !== 'string') return undefined
  const trimmed = candidate.trim()
  return trimmed.length > 0 && trimmed.length <= maxLength ? trimmed : undefined
}

function oneOf<T extends string>(candidate: unknown, allowed: readonly T[], fallback: T): T {
  return typeof candidate === 'string' && (allowed as readonly string[]).includes(candidate) ? candidate as T : fallback
}

function settingsRecord(raw: unknown): Record<string, unknown> | undefined {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw) ? raw as Record<string, unknown> : undefined
}

/**
 * Every persisted or cross-WebView value passes through this function. It is a
 * whitelist rebuild from `DEFAULT_SETTINGS`: unknown keys are dropped instead
 * of spread in, so a future schema, a hand-edited localStorage entry, or a
 * corrupted encrypted snapshot can never hand the renderer a value of the
 * wrong type. `animationSpeed.toFixed()` in the settings UI depends on the
 * number guarantee this function provides.
 *
 * Pure and synchronous by design, so `loadSettings`, `saveSettings`, the
 * settings window's commit path, and the `settings-changed` receiver can all
 * call the exact same boundary.
 */
export function normalizeSettings(raw: unknown): WallpaperSettings {
  const value = settingsRecord(raw) ?? {}
  // Only a recognized boolean may override the default. An unknown value must
  // not be treated as `true`, which would silently enable a paid backend.
  return {
    version: SETTINGS_VERSION,
    defaultBackend: oneOf(value.defaultBackend, BACKEND_MODES, DEFAULT_SETTINGS.defaultBackend),
    autoSwitchHarness: settingsBool(value.autoSwitchHarness, settingsBool(value.autoSwitchPersona, DEFAULT_SETTINGS.autoSwitchHarness)),
    conversationPolicy: oneOf(value.conversationPolicy, CONVERSATION_POLICIES, DEFAULT_SETTINGS.conversationPolicy),
    modelTierRules: normalizeModelTierRules(value.modelTierRules),
    bubbleOverrides: normalizeBubbleOverrides(value.bubbleOverrides),
    animationsEnabled: settingsBool(value.animationsEnabled, DEFAULT_SETTINGS.animationsEnabled),
    animationSpeed: boundedNumber(value.animationSpeed, DEFAULT_SETTINGS.animationSpeed, 0.5, 2),
    playWakeOnEveryUnlock: settingsBool(value.playWakeOnEveryUnlock, DEFAULT_SETTINGS.playWakeOnEveryUnlock),
    skipWakeAnimation: settingsBool(value.skipWakeAnimation, DEFAULT_SETTINGS.skipWakeAnimation),
    lockScreenEnabled: settingsBool(value.lockScreenEnabled, DEFAULT_SETTINGS.lockScreenEnabled),
    autostart: settingsBool(value.autostart, DEFAULT_SETTINGS.autostart),
    sleepHotkey: settingsText(value.sleepHotkey, DEFAULT_SETTINGS.sleepHotkey, MAX_SETTINGS_SHORT_STRING),
    sendShortcut: oneOf(value.sendShortcut, ['Enter', 'Ctrl+Enter'] as const, DEFAULT_SETTINGS.sendShortcut),
    background: oneOf(value.background, BACKGROUND_IDS, DEFAULT_SETTINGS.background) as BackgroundId,
    historyStartsExpanded: settingsBool(value.historyStartsExpanded, DEFAULT_SETTINGS.historyStartsExpanded),
    animationIntensity: oneOf(value.animationIntensity, ANIMATION_INTENSITIES, DEFAULT_SETTINGS.animationIntensity),
    interactionLayout: oneOf(value.interactionLayout, INTERACTION_LAYOUTS, DEFAULT_SETTINGS.interactionLayout),
    floatingAnchor: normalizeFloatingAnchor(value.floatingAnchor),
    portraitAmbientLength: boundedNumber(value.portraitAmbientLength, DEFAULT_SETTINGS.portraitAmbientLength, 35, 100),
    portraitAmbientStrength: boundedNumber(value.portraitAmbientStrength, DEFAULT_SETTINGS.portraitAmbientStrength, 0, 1),
    conversationOpacity: boundedNumber(value.conversationOpacity, DEFAULT_SETTINGS.conversationOpacity, 0.2, 0.96),
    conversationBlur: boundedNumber(value.conversationBlur, DEFAULT_SETTINGS.conversationBlur, 0, 40),
    multiScreen: normalizeMultiScreenSettings(value.multiScreen),
    deepseekApi: normalizeApiSettings(value.deepseekApi),
    dshLaunch: normalizeDshLaunchSettings(value.dshLaunch),
  }
}

/**
 * The `settings-changed` payload arrives from another WebView as untrusted
 * JSON. `normalizeSettings` cannot throw on a JSON value, but this wrapper
 * keeps that guarantee explicit and total: a renderer that receives garbage
 * must still paint, so the failure mode is the built-in defaults rather than
 * an unhandled exception inside an event callback.
 */
export function normalizeReceivedSettings(raw: unknown): WallpaperSettings {
  try {
    return normalizeSettings(raw)
  } catch {
    return structuredClone(DEFAULT_SETTINGS)
  }
}

function normalizeFloatingAnchor(raw: unknown): WallpaperSettings['floatingAnchor'] {
  if (raw === null || typeof raw !== 'object') return structuredClone(DEFAULT_SETTINGS.floatingAnchor)
  const value = raw as { x?: unknown; y?: unknown }
  const coordinate = (candidate: unknown, fallback: number) =>
    typeof candidate === 'number' && Number.isFinite(candidate)
      ? Math.min(1, Math.max(0, candidate))
      : fallback
  return {
    x: coordinate(value.x, DEFAULT_SETTINGS.floatingAnchor.x),
    y: coordinate(value.y, DEFAULT_SETTINGS.floatingAnchor.y),
  }
}

/**
 * A rule is rebuilt field by field. A `regex` pattern is only stored, never
 * compiled here: migration must not execute an untrusted expression, and the
 * runtime matcher already falls back safely on an invalid one.
 */
function normalizeModelTierRule(raw: unknown): ModelTierRule | undefined {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return undefined
  const value = raw as Record<string, unknown>
  const backend = value.backend === '*' ? '*' : oneOf(value.backend, BACKEND_MODES, 'deepseek-web')
  if (value.backend !== '*' && !BACKEND_MODES.includes(value.backend as BackendMode)) return undefined
  const pattern = typeof value.pattern === 'string' ? value.pattern : undefined
  if (!pattern || pattern.length > MAX_SETTINGS_STRING) return undefined
  const match = oneOf(value.match, MODEL_TIER_MATCHES, 'contains')
  const tier = oneOf(value.tier, MODEL_TIER_TIERS, 'flash')
  const provider = optionalText(value.provider, MAX_SETTINGS_SHORT_STRING)
  return { backend, pattern, match, tier, ...(provider ? { provider } : {}) }
}

function normalizeModelTierRules(raw: unknown): ModelTierRule[] {
  if (!Array.isArray(raw)) return []
  const rules: ModelTierRule[] = []
  for (const entry of raw) {
    if (rules.length >= MAX_MODEL_TIER_RULES) break
    const rule = normalizeModelTierRule(entry)
    if (rule) rules.push(rule)
  }
  return rules
}

function normalizeBubbleOverrides(raw: unknown): Record<string, string> {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return {}
  const overrides: Record<string, string> = {}
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    if (Object.keys(overrides).length >= MAX_BUBBLE_OVERRIDES) break
    if (key.length === 0 || key.length > MAX_SETTINGS_SHORT_STRING) continue
    if (typeof value !== 'string' || value.length > MAX_BUBBLE_OVERRIDE_LENGTH) continue
    overrides[key] = value
  }
  return overrides
}

function normalizeDshLaunchSettings(raw: unknown): DshLaunchSettings {
  if (raw === null || typeof raw !== 'object') return structuredClone(DEFAULT_SETTINGS.dshLaunch)
  const value = raw as Record<string, unknown>
  const profile = typeof value.profile === 'string' ? value.profile.trim() : ''
  return {
    profile: profile.length > 0 && profile.length <= MAX_SETTINGS_SHORT_STRING ? profile : DEFAULT_SETTINGS.dshLaunch.profile,
    rootPath: optionalText(value.rootPath),
    command: optionalText(value.command),
    // Both new flags default to `false` for an upgraded profile. Auto-starting a
    // resident service is opt-in, and so is trusting a custom launcher with it.
    autoStartWithWallpaper: value.autoStartWithWallpaper === true,
    trustedCommandForAutoStart: value.trustedCommandForAutoStart === true,
  }
}

function validDisplayId(value: string): boolean {
  return value.length > 0 && value.length <= 128 && !/[\u0000-\u001f\u007f]/.test(value)
}

function normalizeMultiScreenSettings(raw: unknown): MultiScreenSettings {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return structuredClone(DEFAULT_SETTINGS.multiScreen)
  const value = raw as Partial<MultiScreenSettings> & { backgrounds?: unknown }
  const backgrounds: Partial<Record<string, BackgroundId>> = {}
  if (value.backgrounds && typeof value.backgrounds === 'object' && !Array.isArray(value.backgrounds)) {
    for (const [displayId, background] of Object.entries(value.backgrounds as Record<string, unknown>)) {
      if (validDisplayId(displayId) && typeof background === 'string' && BACKGROUND_OPTIONS.some((option) => option.id === background)) {
        backgrounds[displayId] = background as BackgroundId
      }
    }
  }
  const displayId = (candidate: unknown) => {
    if (typeof candidate !== 'string') return undefined
    const trimmed = candidate.trim()
    return validDisplayId(trimmed) ? trimmed : undefined
  }
  return {
    enabled: value.enabled === true,
    backgrounds,
    conversationDisplayId: displayId(value.conversationDisplayId),
    portraitDisplayId: displayId(value.portraitDisplayId),
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

function normalizeApiSettings(value: unknown): ApiSettings {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return structuredClone(DEFAULT_SETTINGS.deepseekApi)
  const raw = value as Record<string, unknown>
  const boundedText = (candidate: unknown, fallback: string) =>
    typeof candidate === 'string' && candidate.trim().length > 0 && candidate.trim().length <= MAX_SETTINGS_STRING
      ? candidate.trim()
      : fallback
  return {
    baseUrl: boundedText(raw.baseUrl, DEFAULT_SETTINGS.deepseekApi.baseUrl),
    model: boundedText(raw.model, DEFAULT_SETTINGS.deepseekApi.model),
    priceInputPerMillion: normalizedPrice(raw.priceInputPerMillion),
    priceOutputPerMillion: normalizedPrice(raw.priceOutputPerMillion),
  }
}

export function loadSettings(): WallpaperSettings {
  try {
    const raw = readStoredSettingsDocument()
    if (raw) return normalizeSettings(JSON.parse(raw))
  } catch {
    /* 忽略损坏的配置 */
  }
  return structuredClone(DEFAULT_SETTINGS)
}

/**
 * Settings are normalized again on write. Every mutation path in the UI is an
 * untyped HTML control, so this is the last place that can guarantee the value
 * stored here (and later published to the other WebView) is well formed.
 */
export function saveSettings(s: WallpaperSettings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(normalizeSettings(s)))
  } catch {
    /* localStorage 不可用时静默 */
  }
}

export interface ConversationPointers {
  'deepseek-web'?: { id: string; updatedAt: number; day: string }
  'deepseek-api'?: { id: string; updatedAt: number; day: string }
  harness?: { id: string; updatedAt: number; day: string; bridgeRevision?: number }
}

/** Conversation IDs are embedded in an official DeepSeek route by Rust. */
export function isValidConversationId(value: unknown): value is string {
  return typeof value === 'string'
    && value.length > 0
    && value.length <= 200
    && /^[A-Za-z0-9._-]+$/.test(value)
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
  if (backend === 'deepseek-web' && !isValidConversationId(id)) return
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
  return backend === 'deepseek-web'
    ? isValidConversationId(pointer.id) ? pointer.id : undefined
    : pointer.id
}

/** 应用气泡覆盖：persona 的 bubbles 与设置覆盖合并 */
export function applyBubbleOverrides(
  bubbles: PersonaBubbles,
  overrides: Record<string, string>,
): PersonaBubbles {
  const nonEmpty = Object.fromEntries(Object.entries(overrides).filter(([, value]) => value.trim() !== ''))
  return { ...bubbles, ...nonEmpty }
}
