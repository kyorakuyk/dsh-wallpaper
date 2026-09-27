import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  DEFAULT_SETTINGS,
  MAX_SETTINGS_SHORT_STRING,
  SETTINGS_VERSION,
  assistantDay,
  loadSettings,
  normalizeReceivedSettings,
  normalizeSettings,
  saveSettings,
  type WallpaperSettings,
} from '../src/settings/store.ts'

const storage = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => storage.set(key, value),
})

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const KEY = 'dsh-wallpaper:settings:v10'

/** A complete, valid settings object as the settings window would publish it. */
const VALID: WallpaperSettings = {
  ...DEFAULT_SETTINGS,
  defaultBackend: 'deepseek-api',
  autoSwitchHarness: true,
  conversationPolicy: 'daily',
  modelTierRules: [{ backend: 'harness', provider: 'deepseek-official', pattern: 'v4-pro', match: 'exact', tier: 'pro' }],
  bubbleOverrides: { morning: '早上好' },
  animationsEnabled: false,
  animationSpeed: 1.5,
  playWakeOnEveryUnlock: false,
  skipWakeAnimation: true,
  lockScreenEnabled: true,
  autostart: true,
  sleepHotkey: 'Ctrl+Alt+Q',
  sendShortcut: 'Ctrl+Enter',
  background: 'deepsea-2',
  historyStartsExpanded: true,
  animationIntensity: 'high',
  interactionLayout: 'taskbar-docked',
  floatingAnchor: { x: 0.25, y: 0.75 },
  portraitAmbientLength: 60,
  portraitAmbientStrength: 0.4,
  conversationOpacity: 0.5,
  conversationBlur: 12,
  multiScreen: {
    enabled: true,
    backgrounds: { DISPLAY1: 'workspace' },
    conversationDisplayId: 'DISPLAY2',
    portraitDisplayId: 'DISPLAY1',
  },
  deepseekApi: { baseUrl: 'https://proxy.example.test/v1', model: 'deepseek-reasoner', priceInputPerMillion: 3, priceOutputPerMillion: 4 },
  dshLaunch: {
    profile: 'desktop',
    rootPath: 'D:\\Family\\DeepSeekHarness',
    command: 'node bin.js',
    autoStartWithWallpaper: true,
    trustedCommandForAutoStart: true,
  },
}

afterEach(() => storage.clear())

describe('settings normalization boundary', () => {
  it('round-trips a fully valid settings object without losing a field', () => {
    expect(normalizeSettings(VALID)).toEqual({ ...VALID, version: SETTINGS_VERSION })
    saveSettings(VALID)
    expect(loadSettings()).toEqual({ ...VALID, version: SETTINGS_VERSION })
  })

  it('replaces a string animation speed with a number', () => {
    const settings = normalizeSettings({ ...VALID, animationSpeed: 'fast' })
    expect(settings.animationSpeed).toBe(DEFAULT_SETTINGS.animationSpeed)
    // The settings page renders `${animationSpeed.toFixed(1)}×`.
    expect(() => settings.animationSpeed.toFixed(1)).not.toThrow()
  })

  it('keeps only structurally valid model tier rules', () => {
    const settings = normalizeSettings({
      ...VALID,
      modelTierRules: [null, {}, VALID.modelTierRules[0], { backend: 'nope', pattern: 'x', match: 'exact', tier: 'pro' }, { backend: '*', pattern: '', match: 'exact', tier: 'pro' }],
    })
    expect(settings.modelTierRules).toEqual([VALID.modelTierRules[0]])
  })

  it('falls back for a non-object bubble override map', () => {
    expect(normalizeSettings({ ...VALID, bubbleOverrides: 'bad' }).bubbleOverrides).toEqual({})
    expect(normalizeSettings({ ...VALID, bubbleOverrides: ['bad'] }).bubbleOverrides).toEqual({})
    expect(normalizeSettings({ ...VALID, bubbleOverrides: { ok: 'fine', bad: 42, [''.padEnd(MAX_SETTINGS_SHORT_STRING + 1, 'k')]: 'too long a key' } }).bubbleOverrides).toEqual({ ok: 'fine' })
  })

  it('rejects an unknown backend instead of trusting the string', () => {
    expect(normalizeSettings({ ...VALID, defaultBackend: 'unknown' }).defaultBackend).toBe(DEFAULT_SETTINGS.defaultBackend)
    expect(normalizeSettings({ ...VALID, defaultBackend: 42 }).defaultBackend).toBe(DEFAULT_SETTINGS.defaultBackend)
    expect(normalizeSettings({ ...VALID, defaultBackend: 'harness' }).defaultBackend).toBe('harness')
  })

  it('clamps, rejects, and never propagates non-finite numbers', () => {
    expect(normalizeSettings({ ...VALID, animationSpeed: 99 }).animationSpeed).toBe(2)
    expect(normalizeSettings({ ...VALID, animationSpeed: -1 }).animationSpeed).toBe(0.5)
    expect(normalizeSettings({ ...VALID, animationSpeed: Number.NaN }).animationSpeed).toBe(DEFAULT_SETTINGS.animationSpeed)
    expect(normalizeSettings({ ...VALID, conversationBlur: Number.POSITIVE_INFINITY }).conversationBlur).toBe(DEFAULT_SETTINGS.conversationBlur)
    expect(normalizeSettings({ ...VALID, conversationOpacity: -5 }).conversationOpacity).toBe(0.2)
    expect(normalizeSettings({ ...VALID, portraitAmbientLength: 1000 }).portraitAmbientLength).toBe(100)
    expect(normalizeSettings({ ...VALID, floatingAnchor: { x: 7, y: Number.NaN } }).floatingAnchor).toEqual({ x: 1, y: DEFAULT_SETTINGS.floatingAnchor.y })
    for (const key of ['animationSpeed', 'portraitAmbientStrength', 'conversationOpacity', 'conversationBlur', 'portraitAmbientLength'] as const) {
      expect(Number.isFinite(normalizeSettings({ ...VALID, [key]: '1' as never })[key])).toBe(true)
    }
  })

  it('keeps the assistant day on a 04:00 boundary across months and years', () => {
    // 用户定的规则：深夜还在做的事，"今天"还没过去 —— 00:30 与 03:59 都属前一天，04:00 才翻页。
    // 桥用同一个规则给日会话命名，所以这条错了会让"同一段对话"在两侧变成两天。
    expect(assistantDay(new Date(2026, 8, 27, 0, 30))).toBe('2026-09-26')
    expect(assistantDay(new Date(2026, 8, 27, 3, 59))).toBe('2026-09-26')
    expect(assistantDay(new Date(2026, 8, 27, 4, 0))).toBe('2026-09-27')
    // 跨月与跨年：都是把时间往前挪 4 小时再取日历日。
    expect(assistantDay(new Date(2026, 9, 1, 0, 30))).toBe('2026-09-30')
    expect(assistantDay(new Date(2027, 0, 1, 0, 30))).toBe('2026-12-31')
    // 边界小时可配且被夹在 0–23；0 = 零点跨日（旧行为）；坏值退回默认 4。
    expect(assistantDay(new Date(2026, 8, 27, 0, 30), 0)).toBe('2026-09-27')
    expect(assistantDay(new Date(2026, 8, 27, 0, 30), 23)).toBe('2026-09-26')
    expect(assistantDay(new Date(2026, 8, 27, 0, 30), Number.NaN)).toBe('2026-09-26')
    expect(DEFAULT_SETTINGS.dayBoundaryHour).toBe(4)
    expect(normalizeSettings({ ...VALID, dayBoundaryHour: 99 }).dayBoundaryHour).toBe(23)
    expect(normalizeSettings({ ...VALID, dayBoundaryHour: -5 }).dayBoundaryHour).toBe(0)
    expect(normalizeSettings({ ...VALID, dayBoundaryHour: 3.6 }).dayBoundaryHour).toBe(4)
  })

  it('accepts non-boolean flags only as their default', () => {
    expect(normalizeSettings({ ...VALID, autoSwitchHarness: 'yes' }).autoSwitchHarness).toBe(false)
    expect(normalizeSettings({ ...VALID, lockScreenEnabled: 1 }).lockScreenEnabled).toBe(DEFAULT_SETTINGS.lockScreenEnabled)
    expect(normalizeSettings({ ...VALID, animationsEnabled: null }).animationsEnabled).toBe(DEFAULT_SETTINGS.animationsEnabled)
  })

  it('keeps the wallpaper\'s own backend-reset claim only when it is a real timestamp', () => {
    // 这条记录是"壁纸自己把滑槽拨离 harness 过"的证据，必须活过进程重启：升级安装会重启壁纸，
    // 只记在内存里的规则会在那一刻失效（滑槽停在左侧、桥的灯却是绿的，输入进错后端）。
    const claim = Date.parse('2026-09-27T14:06:00+08:00')
    expect(normalizeSettings({ ...VALID, harnessAutoResetAt: claim }).harnessAutoResetAt).toBe(claim)
    // 不是真实时间戳就等于没有：宁可不自动拨回去，也不拿坏值当凭据把用户搬走。
    for (const bad of [0, -1, Number.NaN, Number.POSITIVE_INFINITY, 'yesterday', null, {}, []]) {
      expect(normalizeSettings({ ...VALID, harnessAutoResetAt: bad as never }).harnessAutoResetAt).toBeUndefined()
    }
    // 用户手动选过后端（或还没发生过复位）：字段根本不出现。
    expect(normalizeSettings({ ...VALID, harnessAutoResetAt: undefined }).harnessAutoResetAt).toBeUndefined()
    expect(normalizeSettings(VALID).harnessAutoResetAt).toBeUndefined()
  })

  it('drops a non-object multi-screen block and over-long display ids', () => {
    expect(normalizeSettings({ ...VALID, multiScreen: 'bad' }).multiScreen).toEqual(DEFAULT_SETTINGS.multiScreen)
    expect(normalizeSettings({ ...VALID, multiScreen: null }).multiScreen).toEqual(DEFAULT_SETTINGS.multiScreen)
    const longId = 'D'.repeat(200)
    const settings = normalizeSettings({
      ...VALID,
      multiScreen: { enabled: true, backgrounds: { [longId]: 'workspace', DISPLAY1: 'workspace' }, conversationDisplayId: longId },
    })
    expect(settings.multiScreen.backgrounds).toEqual({ DISPLAY1: 'workspace' })
    expect(settings.multiScreen.conversationDisplayId).toBeUndefined()
  })

  it('drops unknown keys and truncated strings instead of spreading them in', () => {
    const settings = normalizeSettings({ ...VALID, futureField: { nested: true }, sleepHotkey: 'x'.repeat(MAX_SETTINGS_SHORT_STRING + 1) })
    expect(settings).not.toHaveProperty('futureField')
    expect(settings.sleepHotkey).toBe(DEFAULT_SETTINGS.sleepHotkey)
  })

  it('never throws and always produces a usable object for garbage input', () => {
    for (const raw of [undefined, null, 42, 'settings', [], [1, 2], { version: null }]) {
      const settings = normalizeReceivedSettings(raw)
      expect(settings.version).toBe(SETTINGS_VERSION)
      expect(settings.deepseekApi.baseUrl).toBe(DEFAULT_SETTINGS.deepseekApi.baseUrl)
      expect(() => settings.animationSpeed.toFixed(1)).not.toThrow()
    }
    // A cross-WebView payload that fails to parse is the real "bad payload"
    // case: the receiver must fall back to defaults, not throw into the event
    // callback that would otherwise leave the desktop surface stuck.
    const circular: Record<string, unknown> = { autoSwitchHarness: true }
    circular.self = circular
    expect(() => normalizeReceivedSettings(circular)).not.toThrow()
    expect(normalizeReceivedSettings(() => undefined).defaultBackend).toBe(DEFAULT_SETTINGS.defaultBackend)
    expect(normalizeReceivedSettings(Symbol('x')).version).toBe(SETTINGS_VERSION)
  })

  it('recovers defaults for a whole corrupt stored payload', () => {
    storage.set(KEY, '{not json')
    expect(loadSettings()).toEqual(DEFAULT_SETTINGS)
    storage.set(KEY, JSON.stringify({ animationSpeed: 'fast', defaultBackend: 'unknown', multiScreen: 'bad' }))
    const settings = loadSettings()
    expect(settings.defaultBackend).toBe(DEFAULT_SETTINGS.defaultBackend)
    expect(settings.animationSpeed).toBe(DEFAULT_SETTINGS.animationSpeed)
    expect(settings.multiScreen).toEqual(DEFAULT_SETTINGS.multiScreen)
  })

  it('normalizes again on write so untyped UI input cannot be persisted raw', () => {
    saveSettings({ ...VALID, animationSpeed: 'fast' as never })
    const stored = JSON.parse(storage.get(KEY) ?? '{}') as { animationSpeed?: unknown }
    expect(stored.animationSpeed).toBe(DEFAULT_SETTINGS.animationSpeed)
  })

  /**
   * The version bump is the highest-risk part of adding a setting: a renamed
   * storage key that is not back-read silently resets everything the user
   * configured. These two tests pin the migration contract.
   */
  it('reads a v9 profile without losing the user configuration', () => {
    const previous: Partial<WallpaperSettings> = {
      ...VALID,
      version: 9,
      dshLaunch: { profile: 'work', rootPath: 'D:\\DSH', command: 'node custom.js' },
    }
    // Deliberately write the *old* key, as an upgraded install would have.
    storage.set('dsh-wallpaper:settings:v9', JSON.stringify(previous))

    const settings = loadSettings()
    expect(settings.version).toBe(SETTINGS_VERSION)
    // Everything the user had is preserved...
    expect(settings.defaultBackend).toBe('deepseek-api')
    expect(settings.modelTierRules).toEqual(VALID.modelTierRules)
    expect(settings.bubbleOverrides).toEqual(VALID.bubbleOverrides)
    expect(settings.multiScreen).toEqual(VALID.multiScreen)
    expect(settings.dshLaunch.profile).toBe('work')
    expect(settings.dshLaunch.rootPath).toBe('D:\\DSH')
    expect(settings.dshLaunch.command).toBe('node custom.js')
    // ...and the new flags arrive off, because starting a resident DSH
    // unattended, and trusting a custom launcher with it, are both opt-in.
    expect(settings.dshLaunch.autoStartWithWallpaper).toBe(false)
    expect(settings.dshLaunch.trustedCommandForAutoStart).toBe(false)
  })

  it('migrates every historical storage key, newest first', () => {
    // One representative setting per generation, so a key dropped from the
    // migration chain shows up as a lost field rather than a passing test.
    const generations: Array<[string, Partial<WallpaperSettings>]> = [
      ['dsh-wallpaper:settings', { sleepHotkey: 'Ctrl+Alt+1' }],
      ['dsh-wallpaper:settings:v2', { sleepHotkey: 'Ctrl+Alt+2' }],
      ['dsh-wallpaper:settings:v3', { sleepHotkey: 'Ctrl+Alt+3' }],
      ['dsh-wallpaper:settings:v4', { sleepHotkey: 'Ctrl+Alt+4' }],
      ['dsh-wallpaper:settings:v5', { sleepHotkey: 'Ctrl+Alt+5' }],
      ['dsh-wallpaper:settings:v6', { sleepHotkey: 'Ctrl+Alt+6' }],
      ['dsh-wallpaper:settings:v7', { sleepHotkey: 'Ctrl+Alt+7' }],
      ['dsh-wallpaper:settings:v8', { sleepHotkey: 'Ctrl+Alt+8' }],
      ['dsh-wallpaper:settings:v9', { sleepHotkey: 'Ctrl+Alt+9' }],
      [KEY, { sleepHotkey: 'Ctrl+Alt+10' }],
    ]
    for (const [key, patch] of generations) {
      storage.clear()
      storage.set(key, JSON.stringify({ ...DEFAULT_SETTINGS, ...patch }))
      expect(loadSettings().sleepHotkey, `key ${key}`).toBe(patch.sleepHotkey)
    }
    // When several keys are present the newest one wins.
    storage.clear()
    storage.set('dsh-wallpaper:settings:v7', JSON.stringify({ ...DEFAULT_SETTINGS, sleepHotkey: 'Ctrl+Alt+old' }))
    storage.set('dsh-wallpaper:settings:v9', JSON.stringify({ ...DEFAULT_SETTINGS, sleepHotkey: 'Ctrl+Alt:new' }))
    expect(loadSettings().sleepHotkey).toBe('Ctrl+Alt:new')
  })
})

describe('settings normalization wiring', () => {
  it('routes every settings boundary through the shared normalizer', async () => {
    const app = (await readFile(resolve(wallpaperRoot, 'src/App.tsx'), 'utf8')).replace(/\r\n?/g, '\n')
    const store = (await readFile(resolve(wallpaperRoot, 'src/settings/store.ts'), 'utf8')).replace(/\r\n?/g, '\n')
    const window = (await readFile(resolve(wallpaperRoot, 'src/settings/SettingsWindow.tsx'), 'utf8')).replace(/\r\n?/g, '\n')

    // The desktop receiver no longer trusts the cross-WebView payload.
    expect(app).toContain('setSettings(normalizeReceivedSettings(payload))')
    expect(app).toContain('(emit) => listen<WallpaperSettings>')
    expect(app).not.toContain('setSettings(event.payload)')
    // Load and save share the same pure boundary.
    expect(store).toMatch(/if \(raw\) return normalizeSettings\(JSON\.parse\(raw\)\)/)
    expect(store).toContain('localStorage.setItem(KEY, JSON.stringify(normalizeSettings(s)))')
    // The settings window commits through saveSettings, which normalizes.
    expect(window).toMatch(/const commitSettings = \(next: WallpaperSettings\) => \{[\s\S]*?saveSettings\(next\)/)
  })
})
