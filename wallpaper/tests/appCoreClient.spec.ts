import { describe, expect, it } from 'vitest'
import { PREVIEW_APP_SNAPSHOT } from '../src/runtime/appSnapshot.ts'

describe('app snapshot contract', () => {
  it('keeps transient visibility separate from the user preference', () => {
    expect(PREVIEW_APP_SNAPSHOT.interaction.enabled).toBe(true)
    expect(PREVIEW_APP_SNAPSHOT.interaction.visible).toBe(true)

    const ordinaryAppForeground = {
      ...PREVIEW_APP_SNAPSHOT,
      interaction: {
        ...PREVIEW_APP_SNAPSHOT.interaction,
        enabled: true,
        visible: false,
        desktopForeground: false,
      },
    }

    expect(ordinaryAppForeground.interaction.enabled).toBe(true)
    expect(ordinaryAppForeground.interaction.visible).toBe(false)
  })

  it('has an explicit privacy screen independent of chat activity', () => {
    const locked = { ...PREVIEW_APP_SNAPSHOT, phase: 'locked' as const, privacyScreen: true }
    expect(locked.privacyScreen).toBe(true)
    expect(locked.activity).toBe('idle')
  })
})

