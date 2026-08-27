import { describe, expect, it } from 'vitest'
import { DEFAULT_LITE_SETTINGS, normalizeLiteSettings } from '../src/lite/settings.ts'

describe('Lite settings', () => {
  it('starts with one deterministic built-in scene and no connection state', () => {
    expect(DEFAULT_LITE_SETTINGS.background).toBe('workspace')
    expect(DEFAULT_LITE_SETTINGS.portrait).toBe('blue-adult')
    expect(DEFAULT_LITE_SETTINGS).not.toHaveProperty('defaultBackend')
    expect(DEFAULT_LITE_SETTINGS).not.toHaveProperty('conversationPolicy')
  })

  it('accepts simple custom image slots and clamps animation settings', () => {
    const settings = normalizeLiteSettings({
      background: 'custom',
      portrait: 'custom',
      animationSpeed: 99,
      animationsEnabled: false,
    })
    expect(settings.background).toBe('custom')
    expect(settings.portrait).toBe('custom')
    expect(settings.animationSpeed).toBe(2)
    expect(settings.animationsEnabled).toBe(false)
  })

  it('rejects unknown ids instead of letting a missing asset blank the scene', () => {
    const settings = normalizeLiteSettings({ background: 'unknown', portrait: 'unknown', animationSpeed: -1 })
    expect(settings.background).toBe('workspace')
    expect(settings.portrait).toBe('blue-adult')
    expect(settings.animationSpeed).toBe(.5)
  })
})
