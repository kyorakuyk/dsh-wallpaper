import { describe, expect, it } from 'vitest'

import { DEFAULT_SETTINGS, SETTINGS_VERSION, normalizeSettings } from '../src/settings/store.ts'

/**
 * 语言是**存下来的设置**，所以这里盯的是"老文档怎么办"：
 * 设置文档从 10 升到 11 时新增了这个键，而每个人的机器上都有一份 10 的文档。
 */
describe('the language setting', () => {
  it('is Chinese on a document that predates it, and the other values survive', () => {
    const migrated = normalizeSettings({ version: 10, autostart: true, sendShortcut: 'Ctrl+Enter' })
    expect(migrated.language).toBe('zh')
    // 迁移只补新键，不能顺手把别人的设置重置了。
    expect(migrated.autostart).toBe(true)
    expect(migrated.sendShortcut).toBe('Ctrl+Enter')
  })

  it('carries a real choice through', () => {
    expect(normalizeSettings({ language: 'en' }).language).toBe('en')
    expect(normalizeSettings({ version: 11, language: 'en' }).language).toBe('en')
  })

  it('falls back instead of trusting a hand-edited or corrupted document', () => {
    expect(normalizeSettings({ language: 'fr' }).language).toBe('zh')
    expect(normalizeSettings({ language: 42 }).language).toBe('zh')
    expect(normalizeSettings({ language: null }).language).toBe('zh')
    expect(normalizeSettings('not an object').language).toBe('zh')
  })

  it('stamps the current version, which is what makes the addition a migration', () => {
    expect(SETTINGS_VERSION).toBe(11)
    expect(normalizeSettings({ version: 10 }).version).toBe(11)
  })

  it('defaults to Chinese, so an update never changes the language under someone', () => {
    // 英文用户要自己切一次；反过来（默认英文）会让所有中文用户在升级那天发现界面变了语言。
    expect(DEFAULT_SETTINGS.language).toBe('zh')
  })
})
