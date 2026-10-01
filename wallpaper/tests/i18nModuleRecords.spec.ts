import { afterEach, describe, expect, it } from 'vitest'

import { getLanguage, setLanguage } from '../src/i18n/index.ts'
import { t } from '../src/i18n/index.ts'
import { LITE_BACKGROUND_OPTIONS, LITE_PORTRAIT_OPTIONS } from '../src/lite/settings.ts'
import { litePersona } from '../src/lite/persona.ts'
import { OFFICIAL_PERSONA_CARDS } from '../src/persona/officialCatalog.ts'
import { BUILTIN_PERSONAS } from '../src/persona/registry.ts'
import { DEFAULT_BUBBLES } from '../src/persona/types.ts'
import { BACKGROUND_OPTIONS } from '../src/settings/store.ts'

// 语言是模块级状态，测试之间必须还原（与 tests/i18n.spec.ts 同一条规矩）。
afterEach(() => {
  setLanguage('zh')
})

/**
 * 这一组钉的是**模块级记录里的语言**。
 *
 * 内置形态表、背景表、Lite 的候选表都在 import 那一刻建好。写法如果退回到"存字符串"，
 * 中文界面毫无变化、类型检查也照样通过 —— 只有"先切英文再读这张表"才能发现它被定死在了
 * 启动那一刻。所以每条断言都走一遍 zh → en 的切换，而不只是读一次当下这一份。
 */
describe('records built at import time follow the current language', () => {
  it('gives the built-in personas a name per language, without rebuilding the table', () => {
    const persona = BUILTIN_PERSONAS['blue-child']!
    expect(persona.name).toBe(t('persona.builtin.blue-child'))
    expect(persona.name).toBe('蓝色幼年鲸鱼娘')
    setLanguage('en')
    // 同一份 manifest（引用相等），换的是名字。
    expect(BUILTIN_PERSONAS['blue-child']).toBe(persona)
    expect(persona.name).toBe(t('persona.builtin.blue-child'))
    expect(persona.name).not.toBe('蓝色幼年鲸鱼娘')
    expect(persona.name).not.toBe('')
  })

  it('follows the language for the fallback bubbles as well', () => {
    expect(DEFAULT_BUBBLES.morning).toBe('早上好！今天要做什么呢？')
    setLanguage('en')
    expect(DEFAULT_BUBBLES.morning).toBe(t('persona.bubble.morning'))
    expect(DEFAULT_BUBBLES.chatOpen).toBe(t('persona.bubble.chat-open'))
  })

  it('follows the language for the official catalog and the Lite option tiles', () => {
    expect(OFFICIAL_PERSONA_CARDS[0].name).toBe('DeepSeek Flash · 蓝色幼年')
    expect(LITE_BACKGROUND_OPTIONS[0]!.label).toBe('深夜工作室')
    expect(LITE_PORTRAIT_OPTIONS[0]!.label).toBe('蓝色成年形态')
    setLanguage('en')
    expect(OFFICIAL_PERSONA_CARDS[0].name).toBe(t('persona.official.deepseek.flash.name'))
    expect(LITE_BACKGROUND_OPTIONS[0]!.label).toBe(t('lite.option.background.workspace'))
    expect(LITE_PORTRAIT_OPTIONS[0]!.label).toBe(t('lite.option.portrait.blue-adult'))
  })

  it('follows the language for the background options the settings pages render', () => {
    expect(BACKGROUND_OPTIONS.map((option) => option.name)).toEqual(['深夜工作室', '深海穹顶舱', '深海书房', '默认渐变主题'])
    setLanguage('en')
    expect(BACKGROUND_OPTIONS.map((option) => option.name)).toEqual([
      t('settings.appearance.background.workspace'),
      t('settings.appearance.background.deepsea-2'),
      t('settings.appearance.background.deepsea-3'),
      t('settings.appearance.background.default'),
    ])
    // id 与路径是标识，不随语言走。
    expect(BACKGROUND_OPTIONS.map((option) => option.id)).toEqual(['workspace', 'deepsea-2', 'deepsea-3', 'default'])
  })

  it('names a Lite persona from the option table, at read time', () => {
    const persona = litePersona('blue-adult')
    expect(persona.name).toBe('蓝色成年形态')
    setLanguage('en')
    expect(persona.name).toBe(t('lite.option.portrait.blue-adult'))
    // Lite 不渲染气泡，所以那两格一直是空串（它们不来自词条）。
    expect(litePersona('custom').name).toBe(t('lite.option.portrait.blue-adult'))
  })

  it('hands the notice sentences out per call instead of freezing them at import', async () => {
    // 这两句曾经是模块级常量：常量在 import 那一刻求值，切语言不会让它变。
    const { archivedSessionNotice, blockedTurnNotice } = await import('../src/chat/nativeAdapter.ts')
    expect(getLanguage()).toBe('zh')
    expect(archivedSessionNotice()).toBe(t('chat.native.archived-notice'))
    setLanguage('en')
    expect(archivedSessionNotice()).toBe(t('chat.native.archived-notice'))
    expect(archivedSessionNotice()).not.toBe('这条会话已被归档，已在今天的新会话里重新发送。')
    expect(blockedTurnNotice()).toBe(t('chat.native.blocked-notice'))
  })
})
