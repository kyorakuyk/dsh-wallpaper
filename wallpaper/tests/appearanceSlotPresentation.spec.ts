import { describe, expect, it } from 'vitest'
import { SLOT_PRESENTATION } from '../src/features/appearance/appearanceViewModel.ts'

describe('lock-screen asset slot presentation', () => {
  it('does not imply that a user-selected asset is already used by the native lock screen', () => {
    const presentation = SLOT_PRESENTATION['lockscreen.image']
    expect(presentation.label).toContain('预留')
    expect(presentation.description).toContain('固定使用内置熟睡画面')
  })
})
