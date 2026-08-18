import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../src/domain/types.ts'
import { composerPlaceholder, formatCost, isBusyActivity, sessionCost, sessionCostSummary, usageTokenCount } from '../src/features/chat/conversationViewModel.ts'

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

  it('uses explicit disabled and busy composer guidance', () => {
    expect(composerPlaceholder(true, 'idle')).toBe('当前模式暂不可用')
    expect(composerPlaceholder(false, 'thinking')).toContain('上一条消息')
    expect(composerPlaceholder(false, 'idle')).toBe('今天要一起处理什么？')
  })
})
