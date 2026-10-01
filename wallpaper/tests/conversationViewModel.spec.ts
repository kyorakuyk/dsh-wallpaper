import { afterEach, describe, expect, it } from 'vitest'
import type { ChatMessage } from '../src/domain/types.ts'
import { composerPlaceholder, formatCost, isBusyActivity, sessionCost, sessionCostSummary, turnUsageSummary, usageTokenCount } from '../src/features/chat/conversationViewModel.ts'
import { setLanguage } from '../src/i18n/index.ts'

// 语言是模块级状态：这个文件里多了一条切到英文的断言，测试之间必须还原，
// 否则后面几条按中文写的期望会跟着变（与 tests/i18n.spec.ts 同一条规矩）。
afterEach(() => {
  setLanguage('zh')
})

describe('conversation view model', () => {
  it('treats only active request phases as busy', () => {
    expect(isBusyActivity('sending')).toBe(true)
    expect(isBusyActivity('thinking')).toBe(true)
    expect(isBusyActivity('streaming')).toBe(true)
    expect(isBusyActivity('tool')).toBe(true)
    expect(isBusyActivity('idle')).toBe(false)
    expect(isBusyActivity('done')).toBe(false)
  })

  it('sums only available message costs', () => {
    const messages: ChatMessage[] = [
      { id: '1', role: 'user', content: 'hello', createdAt: 1 },
      { id: '2', role: 'assistant', content: 'hi', createdAt: 2, usage: { input: 10, output: 20, cost: 0.0123 } },
      { id: '3', role: 'assistant', content: 'again', createdAt: 3, usage: { input: 4, output: 6, cost: 0.004 } },
    ]
    expect(sessionCost(messages)).toBeCloseTo(0.0163)
    expect(sessionCostSummary(messages)).toMatchObject({ estimated: false })
    expect(sessionCostSummary(messages)?.cost).toBeCloseTo(0.0163)
  })

  it('keeps a configured zero price distinct from an unpriced transcript', () => {
    const zeroCost: ChatMessage[] = [{ id: 'zero', role: 'assistant', content: 'ok', createdAt: 1, usage: { input: 8, output: 3, cost: 0, estimated: true } }]
    const noPrice: ChatMessage[] = [{ id: 'none', role: 'assistant', content: 'ok', createdAt: 1, usage: { input: 8, output: 3 } }]

    expect(sessionCostSummary(zeroCost)).toEqual({ cost: 0, estimated: true })
    expect(sessionCostSummary(noPrice)).toBeUndefined()
  })

  it('formats token and price metadata without inventing usage', () => {
    expect(usageTokenCount()).toBeUndefined()
    expect(usageTokenCount({ input: 18, output: 7, cacheRead: 4 })).toBe(25)
    expect(formatCost(0.02345)).toBe('¥0.0234')
    expect(formatCost(0.02345, true)).toBe('约 ¥0.0234')
  })

  it('keeps every per-turn footer field explicit, including zero values', () => {
    expect(turnUsageSummary(undefined, 'harness', false)).toEqual({
      available: false,
      input: '未提供',
      output: '未提供',
      cacheRead: '未提供',
      priceUnconfigured: false,
      cost: '费用未提供',
    })
    expect(turnUsageSummary(undefined, 'deepseek-api', false).cost).toBe('价格未配置')
    expect(turnUsageSummary({ input: 1, output: 2 }, 'deepseek-api', false).cost).toBe('价格未配置')
    expect(turnUsageSummary({ input: 0, output: 0, cacheRead: 0, cost: 0, estimated: true }, 'deepseek-api', true)).toEqual({
      available: true,
      input: '0',
      output: '0',
      cacheRead: '0',
      priceUnconfigured: false,
      cost: '约 ¥0.0000',
    })
    expect(turnUsageSummary({ input: 18, output: 7 }, 'deepseek-api', true).cost).toBe('费用未提供')
  })

  it('says why a cost cell is unpriced as a flag, not as translated text', () => {
    // 界面按这个标志位给费用那一格加样式。原先它比的是**翻译过的**那句"价格未配置"，
    // 于是英文界面里样式会悄悄消失——所以这条钉的是"标志位与语言无关"。
    expect(turnUsageSummary(undefined, 'deepseek-api', false).priceUnconfigured).toBe(true)
    expect(turnUsageSummary({ input: 1, output: 2 }, 'deepseek-api', false).priceUnconfigured).toBe(true)
    // 配了价格就不标；零价是**真的价格**，照样算配置好了。
    expect(turnUsageSummary({ input: 1, output: 2 }, 'deepseek-api', true).priceUnconfigured).toBe(false)
    expect(turnUsageSummary({ input: 0, output: 0, cost: 0 }, 'deepseek-api', true).priceUnconfigured).toBe(false)
    // 非 API 后端与"有 usage 但没给费用"两种情形都不是"价格未配置"。
    expect(turnUsageSummary(undefined, 'harness', false).priceUnconfigured).toBe(false)
    expect(turnUsageSummary({ input: 18, output: 7 }, 'deepseek-api', true).priceUnconfigured).toBe(false)
    setLanguage('en')
    expect(turnUsageSummary(undefined, 'deepseek-api', false).priceUnconfigured).toBe(true)
  })

  it('uses explicit disabled and busy composer guidance', () => {
    expect(composerPlaceholder(true, 'idle')).toBe('当前模式暂不可用')
    expect(composerPlaceholder(false, 'thinking')).toContain('上一条消息')
    expect(composerPlaceholder(false, 'idle')).toBe('今天要一起处理什么？')
  })
})
