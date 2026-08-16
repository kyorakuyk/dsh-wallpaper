import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { AppearanceDrawer } from '../src/features/appearance/AppearanceDrawer.tsx'
import type { AppearanceAssetSummary, AppearanceThemeSummary } from '../src/features/appearance/appearanceViewModel.ts'

const themes: AppearanceThemeSummary[] = [
  { id: 'user-theme', version: '1.0.0', name: '用户主题', source: 'user', readonly: false, manifestPath: 'user/theme.json' },
  { id: 'official', version: '2.0.0', name: '官方深海', source: 'official', readonly: true, manifestPath: 'builtin/theme.json', inheritedSlots: [] },
]

const assets: AppearanceAssetSummary[] = [
  { id: 'pending', sha256: 'a'.repeat(64), mediaType: 'image', originalName: '新立绘.webp', objectPath: 'objects/a.webp', status: 'inbox', slots: [], origin: { kind: 'loose' }, createdAt: 2 },
  { id: 'private', sha256: 'b'.repeat(64), mediaType: 'image', originalName: '主题私有秘密.webp', objectPath: 'objects/b.webp', status: 'classified', slots: ['desktop.background'], origin: { kind: 'theme-private', themeId: 'user-theme', themeVersion: '1.0.0' }, createdAt: 3 },
]

const callbacks = {
  onClose: () => undefined,
  onActivateTheme: () => undefined,
  onSetOverride: () => undefined,
  onClearOverride: () => undefined,
  onReviewInbox: () => undefined,
  onImport: () => undefined,
  onImportFolder: () => undefined,
  onExport: () => undefined,
}

describe('AppearanceDrawer', () => {
  it('renders official themes first, shows inbox count and keeps private asset names hidden', () => {
    const html = renderToStaticMarkup(<AppearanceDrawer
      open
      themes={themes}
      assets={assets}
      activeThemeId="official"
      activeThemeVersion="2.0.0"
      overrides={{}}
      {...callbacks}
    />)
    expect(html.indexOf('官方深海')).toBeLessThan(html.indexOf('用户主题'))
    expect(html).toContain('待分类区 <span class="dsh-appearance__count">1</span>')
    expect(html).toContain('新立绘.webp')
    expect(html).not.toContain('主题私有秘密.webp')
    expect(html).toContain('导出当前搭配')
  })

  it('does not render drawer content while closed', () => {
    const html = renderToStaticMarkup(<AppearanceDrawer
      open={false}
      themes={themes}
      assets={assets}
      activeThemeId="official"
      activeThemeVersion="2.0.0"
      overrides={{}}
      {...callbacks}
    />)
    expect(html).toBe('')
  })
})
