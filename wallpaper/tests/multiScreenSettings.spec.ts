import { afterEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS, SETTINGS_VERSION, loadSettings } from '../src/settings/store.ts'

const storage = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => storage.set(key, value),
})

describe('multi-screen settings migration', () => {
  afterEach(() => storage.clear())

  it('keeps valid per-display choices and rejects unsafe or unknown entries', () => {
    storage.set('dsh-wallpaper:settings:v7', JSON.stringify({
      ...DEFAULT_SETTINGS,
      multiScreen: {
        enabled: true,
        backgrounds: {
          DISPLAY1: 'deepsea-2',
          DISPLAY2: 'not-a-background',
          '\u0000bad': 'workspace',
        },
        conversationDisplayId: ' DISPLAY2 ',
        portraitDisplayId: '\u0000',
      },
    }))
    const settings = loadSettings()
    expect(settings.version).toBe(SETTINGS_VERSION)
    expect(settings.multiScreen).toEqual({
      enabled: true,
      backgrounds: { DISPLAY1: 'deepsea-2' },
      conversationDisplayId: 'DISPLAY2',
      portraitDisplayId: undefined,
    })
  })

  it('preserves the existing single-screen default when no map is stored', () => {
    const settings = loadSettings()
    expect(settings.multiScreen).toEqual({ enabled: false, backgrounds: {} })
  })
})
