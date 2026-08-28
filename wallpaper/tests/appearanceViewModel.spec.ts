import { describe, expect, it } from 'vitest'
import type { AppearanceAssetSummary, AppearanceThemeSummary } from '../src/features/appearance/appearanceViewModel.ts'
import { activeTheme, compatibleSlots, componentAssets, inboxAssets, overrideCount, sanitizeClassificationRequest, sortThemes } from '../src/features/appearance/appearanceViewModel.ts'

const asset = (patch: Partial<AppearanceAssetSummary> & Pick<AppearanceAssetSummary, 'id'>): AppearanceAssetSummary => ({
  sha256: patch.id.padEnd(64, '0').slice(0, 64),
  mediaType: 'image',
  originalName: `${patch.id}.webp`,
  objectPath: `objects/${patch.id}.webp`,
  status: 'classified',
  slots: ['desktop.background'],
  origin: { kind: 'loose' },
  createdAt: 1,
  ...patch,
})

const theme = (patch: Partial<AppearanceThemeSummary> & Pick<AppearanceThemeSummary, 'id' | 'name'>): AppearanceThemeSummary => ({
  version: '1.0.0',
  source: 'user',
  manifestPath: `${patch.id}/theme.json`,
  readonly: false,
  ...patch,
})

describe('appearance view model', () => {
  it('always sorts official themes before user themes', () => {
    const themes = sortThemes([
      theme({ id: 'user-a', name: 'A 用户主题' }),
      theme({ id: 'official-z', name: 'Z 官方主题', source: 'official', readonly: true }),
      theme({ id: 'user-b', name: 'B 用户主题' }),
    ])
    expect(themes.map((item) => item.id)).toEqual(['official-z', 'user-a', 'user-b'])
  })

  it('never exposes theme-private, inbox, corrupt or incompatible assets in component menus', () => {
    const assets = [
      asset({ id: 'loose-ok', slots: ['desktop.background'], createdAt: 5 }),
      asset({ id: 'private', origin: { kind: 'theme-private', themeId: 'theme', themeVersion: '1.0.0' }, slots: ['desktop.background'] }),
      asset({ id: 'inbox', status: 'inbox', slots: ['desktop.background'] }),
      asset({ id: 'corrupt', status: 'corrupt', slots: ['desktop.background'] }),
      asset({ id: 'wrong-slot', slots: ['lockscreen.image'] }),
      asset({ id: 'wrong-media', mediaType: 'font', slots: ['desktop.background'] }),
    ]
    expect(componentAssets(assets, 'desktop.background').map((item) => item.id)).toEqual(['loose-ok'])
  })

  it('only shows loose unclassified assets in the inbox, newest first', () => {
    const assets = [
      asset({ id: 'older', status: 'inbox', createdAt: 2 }),
      asset({ id: 'newer', status: 'inbox', createdAt: 8 }),
      asset({ id: 'classified', createdAt: 9 }),
      asset({ id: 'private', status: 'inbox', createdAt: 10, origin: { kind: 'theme-private', themeId: 'x', themeVersion: '1.0.0' } }),
    ]
    expect(inboxAssets(assets).map((item) => item.id)).toEqual(['newer', 'older'])
  })

  it('resolves the exact active theme version and counts slot overrides', () => {
    const themes = [theme({ id: 'ocean', name: 'Ocean', version: '1.0.0' }), theme({ id: 'ocean', name: 'Ocean', version: '2.0.0' })]
    expect(activeTheme(themes, 'ocean', '2.0.0')?.version).toBe('2.0.0')
    expect(overrideCount({ 'desktop.background': 'asset-a', 'chat.skin': 'asset-b' })).toBe(2)
  })

  it('limits classification to loose inbox assets and media-compatible slots', () => {
    const image = asset({ id: 'image', status: 'inbox', slots: [] })
    const font = asset({ id: 'font', mediaType: 'font', status: 'inbox', slots: [] })
    const privateImage = asset({ id: 'private-image', status: 'inbox', slots: [], origin: { kind: 'theme-private', themeId: 'theme', themeVersion: '1.0.0' } })
    expect(compatibleSlots(font)).toEqual(['ui.font'])
    expect(sanitizeClassificationRequest(
      [image, font, privateImage],
      ['image', 'font', 'private-image', 'missing'],
      ['desktop.background', 'ui.font', 'wake.sequence'],
    )).toEqual({ assetIds: ['image', 'font'], slots: ['desktop.background', 'ui.font'] })
  })
})
