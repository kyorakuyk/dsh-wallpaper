import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { OfficialPersonaCards } from '../src/persona/OfficialPersonaCards.tsx'
import { OFFICIAL_PERSONA_CARDS, officialPersonaIdFor } from '../src/persona/officialCatalog.ts'
import type { AppearanceAssetSummary } from '../src/features/appearance/appearanceViewModel.ts'

describe('official persona catalog', () => {
  it('keeps the four formal backend/model forms mapped to their fixed slots', () => {
    expect(OFFICIAL_PERSONA_CARDS.map(({ id, slot }) => ({ id, slot }))).toEqual([
      { id: 'blue-child', slot: 'persona.deepseek.flash' },
      { id: 'blue-adult', slot: 'persona.deepseek.pro' },
      { id: 'black-child', slot: 'persona.harness.flash' },
      { id: 'black-adult', slot: 'persona.harness.pro' },
    ])
    expect(officialPersonaIdFor('deepseek-web', 'flash')).toBe('blue-child')
    expect(officialPersonaIdFor('deepseek-api', 'pro')).toBe('blue-adult')
    expect(officialPersonaIdFor('harness', 'flash')).toBe('black-child')
    expect(officialPersonaIdFor('harness', 'pro')).toBe('black-adult')
    expect(officialPersonaIdFor('harness', 'unknown')).toBe('black-child')
  })

  it('renders a read-only 2×2 reference grid and reports per-slot replacements', () => {
    const replacement: AppearanceAssetSummary = {
      id: 'replacement',
      sha256: 'a'.repeat(64),
      mediaType: 'image',
      originalName: '我的蓝色 Flash 立绘.png',
      objectPath: 'objects/replacement.png',
      status: 'classified',
      slots: ['persona.deepseek.flash'],
      origin: { kind: 'loose' },
      createdAt: 1,
    }
    const html = renderToStaticMarkup(<OfficialPersonaCards
      assets={[replacement]}
      overrides={{ 'persona.deepseek.flash': replacement.id }}
    />)

    expect(html).toContain('aria-label="官方人物列表"')
    expect(html.match(/official-persona-card official-persona-card--/g)).toHaveLength(4)
    expect(html).toContain('DeepSeek</span><strong>Flash · 幼年')
    expect(html).toContain('DeepSeek Harness</span><strong>Pro · 成年')
    expect(html).toContain('已替换：我的蓝色 Flash 立绘.png')
    expect(html).toContain('官方基础立绘')
    expect(html).not.toContain('<button')
  })
})
