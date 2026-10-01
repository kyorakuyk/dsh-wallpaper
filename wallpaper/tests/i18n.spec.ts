import { afterEach, describe, expect, it, vi } from 'vitest'

import { LANGUAGES, getLanguage, setLanguage, subscribe, t } from '../src/i18n/index.ts'
import { en } from '../src/i18n/en.ts'
import { zh } from '../src/i18n/zh.ts'

// 语言是模块级状态，测试之间必须还原，否则先跑的那条会决定后跑的那条看到什么。
afterEach(() => {
  setLanguage('zh')
  vi.restoreAllMocks()
})

describe('translating', () => {
  it('starts in Chinese, because that is what the product has always shown', () => {
    expect(getLanguage()).toBe('zh')
    expect(t('language.label')).toBe(zh['language.label'])
  })

  it('returns the English entry once the language is English', () => {
    setLanguage('en')
    expect(t('language.label')).toBe(en['language.label'])
    expect(t('language.label')).not.toBe(zh['language.label'])
  })

  it('fills placeholders, and leaves unknown ones alone rather than printing undefined', () => {
    // 用一条临时键验证插值规则：`{name}` 被替换，未知占位符原样保留。
    const dict = zh as Record<string, string>
    dict['test.greeting'] = '你好，{name}！{unknown} 留着'
    setLanguage('en')
    const english = en as Record<string, string>
    english['test.greeting'] = 'Hello, {name}! {unknown} stays'
    expect(t('test.greeting' as never, { name: '鲸鱼娘' })).toBe('Hello, 鲸鱼娘! {unknown} stays')
    delete dict['test.greeting']
    delete english['test.greeting']
  })
})

describe('telling the interface', () => {
  it('notifies subscribers when the language really changes', () => {
    const listener = vi.fn()
    const unsubscribe = subscribe(listener)
    setLanguage('en')
    expect(listener).toHaveBeenCalledTimes(1)
    unsubscribe()
    setLanguage('zh')
    expect(listener).toHaveBeenCalledTimes(1)
  })

  it('stays quiet when the same language is set again', () => {
    // 设置文档在启动时会回填一次语言；那一次不该让整个界面白重渲染。
    const listener = vi.fn()
    const unsubscribe = subscribe(listener)
    setLanguage('zh')
    expect(listener).not.toHaveBeenCalled()
    unsubscribe()
  })
})

describe('the two dictionaries', () => {
  it('cover exactly the same keys', () => {
    // 类型已经保证了这一点；这条是运行时的第二道锁，因为类型可以被 `as` 绕过去。
    expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort())
  })

  it('offer both languages, Chinese first', () => {
    expect(LANGUAGES).toEqual(['zh', 'en'])
  })

  it('do not ship an empty entry', () => {
    for (const [key, value] of Object.entries(zh)) expect(value, key).not.toBe('')
    for (const [key, value] of Object.entries(en)) expect(value, key).not.toBe('')
  })
})
