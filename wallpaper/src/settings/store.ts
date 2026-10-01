/** 版本化设置；桌面壳会以原生文件存储替换浏览器 localStorage。 */

import type { BackendMode, ConversationPolicy, ModelTierRule } from '../domain/types.ts'
import type { Language } from '../i18n/index.ts'
import { t } from '../i18n/index.ts'
import type { PersonaBubbles } from '../persona/types.ts'
import { assetUrl } from '../runtime/assets.ts'

export { assetUrl }

export type InteractionLayout = 'floating' | 'taskbar-docked'

// 11：新增 language（界面语言）。旧文档里没有这个键，归一化会补默认值。
export const SETTINGS_VERSION = 11
/** Keep renderer validation aligned with the native request boundary. A value
 * beyond this ceiling is almost certainly a unit/configuration error and must
 * not be presented as a configured price when Rust deliberately ignores it. */
export const MAX_PRICE_PER_MILLION = 1_000_000
const CONVERSATION_KEY = 'dsh-wallpaper:conversations:v1'
// Existing v1 Harness pointers were created before the bridge supplied the
// required DSH model/cwd context. Do not resume those invalid sessions; API
// and web pointers remain unaffected.
const HARNESS_POINTER_REVISION = 2

export type BackgroundId = 'workspace' | 'deepsea-2' | 'deepsea-3' | 'default'

/**
 * 背景选项：'default' = 默认渐变主题（纯 CSS 的空白背景）；其他项 = 深海室内插画。
 *
 * `name` 是 **getter**：这张表在 import 时建好，而格子上的字要跟着界面语言走
 * （与 `DEFAULT_BUBBLES` 同一条规矩）。id 与路径不随语言变，所以它们是普通字段。
 */
export interface BackgroundOption {
  id: BackgroundId
  readonly name: string
  path: string
}

export const BACKGROUND_OPTIONS: readonly BackgroundOption[] = [
  { id: 'workspace', get name() { return t('settings.appearance.background.workspace') }, path: 'personas/deepsea-bg/deepsea-studio.png' },
  { id: 'deepsea-2', get name() { return t('settings.appearance.background.deepsea-2') }, path: 'personas/deepsea-bg/deepsea-dome.png' },
  { id: 'deepsea-3', get name() { return t('settings.appearance.background.deepsea-3') }, path: 'personas/deepsea-bg/deepsea-study.png' },
  { id: 'default', get name() { return t('settings.appearance.background.default') }, path: '' },
]

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
  /** 人民币／每百万 input tokens；留白时不估算费用。 */
  priceInputPerMillion?: number
  /** 人民币／每百万 output tokens；留白时不估算费用。 */
  priceOutputPerMillion?: number
  /**
   * 上次拉取到的可用模型（含来源地址与时间）。
   *
   * 存下来是因为用户实测报过："api 的刷新结果也没有持久化，下次仍旧需要重新刷新"。它不是真相
   * ——真相永远在 `api_models` 那一次请求里；这里只保证"打开设置就能看见上次那份列表"，并且
   * **带上地址**：换了地址的缓存不能当成新地址的模型清单。
   */
  modelCatalog?: ApiModelCatalog
}

export interface ApiModelCatalog {
  /** 这份列表是从哪个地址拉来的。地址一变，缓存作废。 */
  baseUrl: string
  models: Array<{ id: string; name: string }>
  /** ISO 时间串，用于在界面上说明"上次拉取是什么时候"。 */
  fetchedAt: string
}

export interface HarnessModelSettings {
  /**
   * 上次在壁纸里选的 Harness 模型 id。
   *
   * 作用是**回显**：壁纸重启后模型选择器要显示上次用的那个，而不是宿主报的当前值。
   * 只在宿主目录里仍然有它时才采用——宿主可能已经不再提供该模型，那时按"没记住"处理
   * 而不是把一个不存在的 id 摆进选择器。
   */
  model?: string
}

export interface DshLaunchSettings {
  rootPath?: string
  profile: string
  /**
   * 「启动参数」：追加到启动器后面的额外参数，一个字符串（如 `--port 3081`）。
   *
   * 它取代了原来的 `command`（一个可执行的程序路径），这是一次**能力上的收缩**：任选一个程序
   * 让壁纸去跑，是项目决定不要的能力；而把词追加到启动器后面，启动器的身份仍然由扫描决定、
   * 由原生侧保管。原生收到的是一份 `argv` 数组（渲染层已经分好词，见 `connect/launchArgs.ts`），
   * 所以这一段永远不经过命令行解释器 —— 没有变量展开、没有 `;`、没有管道。
   *
   * 与 `command` 的另一处不同：它**对手动启动与开机自启一视同仁**。原来那条"自动启动不使用
   * 自定义启动命令"的警示与它那个复选框因此一并消失 —— 参数不改变"跑的是谁"，需要用户额外
   * 授权的那个问题也随之不成立。
   */
  args?: string
  /**
   * 每个主体一个别名（键是主体 id）。
   *
   * 只影响**它怎么被称呼**，不影响它怎么被启动：源码目录在「运行方式」里原本只显示一份上级
   * 目录名（两个同名克隆时用来区分），别名把这最后一段也交给用户。为空表示"用目录名" ——
   * 也就是今天的行为，一个字符都不变。
   *
   * 按主体 id 存，而不是按"当前主体"存一个值：用户可能在两棵树之间来回切，别名的归属必须
   * 跟着树走，否则切回去就会发现名字换了人。
   */
  aliases?: Record<string, string>
  /**
   * The chosen harness execution subject, as an id from the target scan.
   *
   * Two classes are folded into this one id, because a subject is not a free
   * combination (`docs/design/harness-subject-and-ui-design.md` §3–§4): a shell
   * that carries its own checkout is identified by AUMID and fixes the window too,
   * while a source tree's id *is* its path and leaves the window a separate
   * choice. Storing the id rather than a path is what lets a client update leave
   * the choice valid.
   *
   * Absent means "nothing chosen yet"; `rootPath` is then still honoured as a
   * checkout subject, so a profile written by an older version keeps working.
   */
  subjectId?: string
  /**
   * Start the configured DSH once, every time the wallpaper starts.
   *
   * This is a wallpaper-start trigger, not a login trigger: it is only useful
   * together with the wallpaper's own Windows autostart, which the settings
   * card says explicitly.
   */
  autoStartWithWallpaper: boolean
  /**
   * The port of the DSH endpoint to talk to, or undefined for "auto".
   *
   * The client shapes listen on different ports and only some are
   * configurable (official desktop shell 19387, CLI/core 3080, and the official
   * web app's own default is 3080 via
   * `ctx.webStartup.port`). Before this setting existed the wallpaper probed
   * 3080 unconditionally, so a user running the official shell saw `offline`
   * while a ready Bridge listened one port away.
   *
   * `undefined` keeps the shipped priority order (official desktop → community
   * desktop → official web/CLI). A number pins one endpoint; it is honoured even
   * when that endpoint is not ready, because silently switching to a different
   * client would answer a different session than the user selected.
   */
  endpointPort?: number
  /** Extra ports the user added to the scan, beyond the three known shapes. */
  extraEndpointPorts?: number[]
  /**
   * Which interface 「打开」 raises when the subject has no window of its own.
   *
   * Only meaningful for a subject this build reaches through a *browser*: the official
   * desktop client owns a Windows window, so the question does not arise for it. For an
   * installed DSH CLI the user picks between the same web UI in a browser and the same
   * host under the TUI (`dst`), which is a real choice — the two show the same sessions
   * and differ in where the conversation is typed.
   *
   * Absent means `browser`: the route that has always been there, so a stored or older
   * profile keeps behaving exactly as it did.
   */
  window?: 'browser' | 'tui'
}

export interface WallpaperSettings {
  version: number
  defaultBackend: BackendMode
  autoSwitchHarness: boolean
  conversationPolicy: ConversationPolicy
  /**
   * 「助手日」的边界小时（0–23，默认 4）。跨日重置、日会话命名、会话指针的"哪一天"都以它为准：
   * 4 表示一条助手日从本地 04:00 开始 —— 深夜还在做的事不会被零点切走。
   */
  dayBoundaryHour: number
  modelTierRules: ModelTierRule[]
  /** 气泡文案覆盖（key: 文案） */
  bubbleOverrides: Record<string, string>
  /** 动画开关 */
  animationsEnabled: boolean
  /** 动画速度倍率 */
  animationSpeed: number
  playWakeOnEveryUnlock: boolean
  skipWakeAnimation: boolean
  // lockScreenEnabled: boolean
  autostart: boolean
  /** 界面语言。默认中文 —— 与这个产品一直以来的表现一致，升级不会突然换语言。 */
  language: Language
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
  /** 壁纸端记住的 Harness 模型选择（用于重启后回显）。 */
  harnessModel: HarnessModelSettings
  /**
   * 新建 Harness 会话时装载哪个 agent 预设（`minimal` / `standard` / …）。
   *
   * 缺省（没写过）时由 `DEFAULT_HARNESS_PRESET` 兜底为 `minimal`（用户要求："工作区的预设先默认为
   * '极简模式'试试，应该能省不少上下文"）。留成可选是为了不把默认值抄进每个存档点：只有一个地方
   * 说"默认是什么"，替换时也只改那一处。
   */
  harnessPreset?: string
  dshLaunch: DshLaunchSettings
  /**
   * 壁纸**自己**把滑槽拨离 harness 的那一次（主体掉线时的自动复位），记的是时间戳。
   *
   * 这件事必须跨进程存活。它决定两件用户能看见的东西：主体回来后壁纸是否有权自己拨回去
   * （用户定死的规则：再次联通之后要以 harness 后端为准），以及轨道里那段保留下来的转写要不要
   * 标明"这不是当前会话"。只放在内存里的后果实测过：升级安装重启了壁纸，这条规则悄悄失效，
   * 滑槽停在左侧、桥的灯却是绿的，用户以为还在跟 DSH 说话——输入其实进了另一个后端（"输入被吞"）。
   */
  harnessAutoResetAt?: number
}

/**
 * 「助手日」的默认边界：本地 **04:00**。
 *
 * 用户定的规则（2026-09-27）：跨日不在零点，而在凌晨四点 —— 深夜还在做的事，在他心里"今天"
 * 还没过去；00:30 开始的一件事到 04:00 之前都算前一天，日重置才会落在真正"新的一天开始"的
 * 时刻，而不是把他正在做的事从中间切断（游戏里的跨日缓冲是同一个道理）。
 * 声明在 `DEFAULT_SETTINGS` **之前**，因为默认值要引用它。
 */
export const DEFAULT_DAY_BOUNDARY_HOUR = 4

export const DEFAULT_SETTINGS: WallpaperSettings = {
  version: SETTINGS_VERSION,
  defaultBackend: 'deepseek-web',
  autoSwitchHarness: false,
  conversationPolicy: 'resume-last',
  dayBoundaryHour: DEFAULT_DAY_BOUNDARY_HOUR,
  modelTierRules: [],
  bubbleOverrides: {},
  animationsEnabled: true,
  animationSpeed: 1,
  playWakeOnEveryUnlock: true,
  skipWakeAnimation: false,
  // lockScreenEnabled: false,
  autostart: false,
  language: 'zh',
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
  harnessModel: {},
  dshLaunch: { profile: 'desktop', autoStartWithWallpaper: false },
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
/** A user needs a handful of extra endpoints at most; more means a mistake. */
export const MAX_EXTRA_ENDPOINT_PORTS = 8
export const MAX_BUBBLE_OVERRIDE_LENGTH = 2000
export const MAX_MODEL_TIER_RULES = 64
export const MAX_BUBBLE_OVERRIDES = 256

const SETTING_LANGUAGES: readonly Language[] = ['zh', 'en']
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

/** An optional record of when something happened. Anything that is not a real
 * timestamp means "no record", never a guessed one. */
function optionalTimestamp(candidate: unknown): number | undefined {
  return typeof candidate === 'number' && Number.isFinite(candidate) && candidate > 0 ? candidate : undefined
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
    // 0 是合法的（退回旧行为：零点跨日），所以用 boundedNumber 而不是"非零才算"。
    dayBoundaryHour: Math.round(boundedNumber(value.dayBoundaryHour, DEFAULT_SETTINGS.dayBoundaryHour, 0, 23)),
    modelTierRules: normalizeModelTierRules(value.modelTierRules),
    bubbleOverrides: normalizeBubbleOverrides(value.bubbleOverrides),
    animationsEnabled: settingsBool(value.animationsEnabled, DEFAULT_SETTINGS.animationsEnabled),
    animationSpeed: boundedNumber(value.animationSpeed, DEFAULT_SETTINGS.animationSpeed, 0.5, 2),
    playWakeOnEveryUnlock: settingsBool(value.playWakeOnEveryUnlock, DEFAULT_SETTINGS.playWakeOnEveryUnlock),
    skipWakeAnimation: settingsBool(value.skipWakeAnimation, DEFAULT_SETTINGS.skipWakeAnimation),
    // lockScreenEnabled: settingsBool(value.lockScreenEnabled, DEFAULT_SETTINGS.lockScreenEnabled),
    autostart: settingsBool(value.autostart, DEFAULT_SETTINGS.autostart),
    language: oneOf(value.language, SETTING_LANGUAGES, DEFAULT_SETTINGS.language),
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
    harnessModel: normalizeHarnessModelSettings(value.harnessModel),
    dshLaunch: normalizeDshLaunchSettings(value.dshLaunch),
    // 只在它是个真实时间戳时才算数：这个字段是"壁纸自己复位过"的证据，值不可信就等于没有
    // ——宁可不自动拨回去，也不要拿一个坏值当凭据把用户从他自己选的滑槽那边搬走。
    harnessAutoResetAt: optionalTimestamp(value.harnessAutoResetAt),
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
  const aliases = normalizeSubjectAliases(value.aliases)
  return {
    profile: profile.length > 0 && profile.length <= MAX_SETTINGS_SHORT_STRING ? profile : DEFAULT_SETTINGS.dshLaunch.profile,
    rootPath: optionalText(value.rootPath),
    /*
      旧档案里的 `command`（一个可执行程序路径）与 `trustedCommandForAutoStart`（它那次授权）
      在这里被**丢掉**，而不是被改写成「启动参数」：一个程序路径与一串参数不是同一种东西，
      把 `C:\tools\dsh.exe` 当成参数追加到我们自己的启动器后面，只会让启动器收到一个它不认识的
      词、然后启动失败 —— 那比"这一项没了"更难懂。设置界面也不再有这两个键，写回一次就清干净。
    */
    args: optionalText(value.args),
    subjectId: optionalText(value.subjectId),
    // 开机自动启动一个常驻服务是选择性加入的；只有字面的 true 才算开了。
    autoStartWithWallpaper: value.autoStartWithWallpaper === true,
    // Only a usable TCP port is accepted; anything else falls back to "auto",
    // which keeps the shipped priority order rather than pinning a bad port and
    // reporting `offline` forever.
    ...(validPort(value.endpointPort) ? { endpointPort: value.endpointPort as number } : {}),
    ...(normalizeExtraPorts(value.extraEndpointPorts).length > 0
      ? { extraEndpointPorts: normalizeExtraPorts(value.extraEndpointPorts) }
      : {}),
    ...(Object.keys(aliases).length > 0 ? { aliases } : {}),
    // 只认这两个值；缺省不写入，读的时候按 `browser` 处理（与旧档案行为一致 ✓）。
    ...(value.window === 'tui' ? { window: 'tui' as const } : {}),
  }
}

/**
 * 别名表：键是主体 id、值是要显示的名字。
 *
 * 键与值都按"用户会看到的字符串"收紧：控制字符会让同一行文字在标签里换行或错位，空值等价于
 * "没起别名"（所以在写回时也被丢掉，而不是存一个空串）。上限沿用设置短串那一档 —— 别名是一行
 * 标签的一部分，不是一段说明。
 */
function normalizeSubjectAliases(raw: unknown): Record<string, string> {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return {}
  const aliases: Record<string, string> = {}
  for (const [subjectId, alias] of Object.entries(raw as Record<string, unknown>)) {
    if (subjectId.trim().length === 0 || subjectId.length > MAX_SETTINGS_STRING) continue
    if (typeof alias !== 'string') continue
    const name = alias.trim()
    if (name.length === 0 || name.length > MAX_SETTINGS_SHORT_STRING) continue
    // eslint-disable-next-line no-control-regex
    if (/[\u0000-\u001f\u007f]/.test(name)) continue
    aliases[subjectId] = name
  }
  return aliases
}

function validPort(value: unknown): boolean {
  return typeof value === 'number' && Number.isInteger(value) && value >= 1 && value <= 65535
}

function normalizeExtraPorts(raw: unknown): number[] {
  if (!Array.isArray(raw)) return []
  const ports: number[] = []
  for (const entry of raw) {
    if (!validPort(entry) || ports.includes(entry as number)) continue
    ports.push(entry as number)
    if (ports.length >= MAX_EXTRA_ENDPOINT_PORTS) break
  }
  return ports
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
  const modelCatalog = normalizeApiModelCatalog(raw.modelCatalog)
  return {
    baseUrl: boundedText(raw.baseUrl, DEFAULT_SETTINGS.deepseekApi.baseUrl),
    model: boundedText(raw.model, DEFAULT_SETTINGS.deepseekApi.model),
    priceInputPerMillion: normalizedPrice(raw.priceInputPerMillion),
    priceOutputPerMillion: normalizedPrice(raw.priceOutputPerMillion),
    ...(modelCatalog ? { modelCatalog } : {}),
  }
}

/** 缓存上限：一份列表不该能把设置撑大。DeepSeek 只有个位数模型，200 已经极其宽松。 */
export const MAX_API_MODEL_CATALOG = 200

/**
 * 归一化模型缓存。
 *
 * 这是**别人给的数据**（一次 HTTP 响应经设置同步进来），所以逐项校验：地址、id、名字都要是
 * 有界字符串；任何不合法就整块丢掉，宁可不显示缓存，也不显示一份来路不明的清单。
 */
function normalizeApiModelCatalog(value: unknown): ApiModelCatalog | undefined {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined
  const raw = value as Record<string, unknown>
  const baseUrl = settingsText(raw.baseUrl, '', MAX_SETTINGS_STRING)
  const fetchedAt = settingsText(raw.fetchedAt, '', MAX_SETTINGS_SHORT_STRING)
  if (!baseUrl || !fetchedAt || !Array.isArray(raw.models)) return undefined
  const models = raw.models
    .slice(0, MAX_API_MODEL_CATALOG)
    .flatMap((entry) => {
      if (entry === null || typeof entry !== 'object' || Array.isArray(entry)) return []
      const candidate = entry as Record<string, unknown>
      const id = settingsText(candidate.id, '', MAX_SETTINGS_SHORT_STRING)
      if (!id) return []
      const name = settingsText(candidate.name, id, MAX_SETTINGS_SHORT_STRING)
      return [{ id, name }]
    })
  // 一条模型都没有的空缓存没有意义：当作"没缓存过"，下次照常拉。
  return models.length > 0 ? { baseUrl, models, fetchedAt } : undefined
}

function normalizeHarnessModelSettings(value: unknown): HarnessModelSettings {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return {}
  const candidate = (value as Record<string, unknown>).model
  // 空串等同于"没记住"：选择器的 value 若是空串而没有被选中的项，浏览器会把这个控件
  // 画成空白（实测过），所以这里不能把一个空 id 存下来。
  if (typeof candidate !== 'string') return {}
  const model = candidate.trim()
  if (model.length === 0 || model.length > MAX_SETTINGS_STRING) return {}
  return { model }
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
    /* localStorage 不可用时静认 */
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
    && !isLocalPlaceholderConversationId(value)
}

/**
 * Whether an id is one this client invented rather than one DeepSeek issued.
 *
 * `deepseekWebAdapter` falls back to `web-<millis>` until the page reports a real
 * conversation id, and that placeholder satisfies the character check above — so it
 * was persisted as a resume pointer. Every later send then navigated to
 * `/a/chat/s/web-<millis>`, a conversation DeepSeek does not have, where the page
 * never becomes ready and the send stalled for the whole page-ready timeout.
 * Rejecting it here keeps the placeholder out of storage; the native side also
 * treats it as "no conversation", so an already-poisoned pointer heals instead of
 * failing forever.
 */
export function isLocalPlaceholderConversationId(value: string): boolean {
  if (!value.startsWith('web-')) return false
  const rest = value.slice(4)
  const digits = rest.startsWith('request-') ? rest.slice(8) : rest
  return digits.length > 0 && /^\d+$/.test(digits)
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

/**
 * 「助手日」：把 `now` 往前挪 `boundaryHour` 小时，再取那个日历日。
 *
 * **一条规则、两侧同值**：桥用它给日会话命名（`wallpaper-<助手日>`），前端用它决定"要不要换
 * 新会话"、以及会话指针属于哪一天。任何一边单独改动都会让"同一段对话"在两侧变成两天，所以
 * 两侧都必须调用各自实现的同一个函数、取同一个边界小时（前端取设置 `dayBoundaryHour`，
 * 桥取配置 `dayBoundaryHour`；P1 会加一条启动自检，不一致就报警而不是静默）。
 */
export function assistantDay(now: Date = new Date(), boundaryHour: number = DEFAULT_DAY_BOUNDARY_HOUR): string {
  const boundary = Number.isFinite(boundaryHour)
    ? Math.min(23, Math.max(0, Math.floor(boundaryHour)))
    : DEFAULT_DAY_BOUNDARY_HOUR
  return localCalendarDay(new Date(now.getTime() - boundary * 3_600_000))
}

export function loadConversationPointers(): ConversationPointers {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(CONVERSATION_KEY) ?? '{}')
    return parsed && typeof parsed === 'object' ? parsed as ConversationPointers : {}
  } catch { return {} }
}

export function saveConversationPointer(
  backend: BackendMode,
  id: string,
  now: Date = new Date(),
  boundaryHour: number = DEFAULT_DAY_BOUNDARY_HOUR,
): void {
  if (backend === 'deepseek-web' && !isValidConversationId(id)) return
  try {
    const pointers = loadConversationPointers()
    pointers[backend] = {
      id,
      updatedAt: now.getTime(),
      // 助手日而不是日历日：指针属于哪一天，要与"要不要换新会话"用的是同一条规则。
      day: assistantDay(now, boundaryHour),
      ...(backend === 'harness' ? { bridgeRevision: HARNESS_POINTER_REVISION } : {}),
    }
    localStorage.setItem(CONVERSATION_KEY, JSON.stringify(pointers))
  } catch { /* unavailable storage: start a fresh conversation next time */ }
}

export function resumeConversationId(
  backend: BackendMode,
  policy: ConversationPolicy,
  now: Date = new Date(),
  boundaryHour: number = DEFAULT_DAY_BOUNDARY_HOUR,
): string | undefined {
  if (policy === 'new-on-unlock') return undefined
  const pointer = loadConversationPointers()[backend]
  if (!pointer) return undefined
  if (backend === 'harness'
    && (pointer as ConversationPointers['harness'])?.bridgeRevision !== HARNESS_POINTER_REVISION) return undefined
  if (policy === 'daily' && pointer.day !== assistantDay(now, boundaryHour)) return undefined
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
