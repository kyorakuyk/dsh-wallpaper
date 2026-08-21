import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ConversationBubble } from '../src/features/chat/ConversationBubble.tsx'

const callbacks = {
  onToggleHistory: () => undefined,
  onSend: () => undefined,
  onStop: () => undefined,
  onClose: () => undefined,
}

describe('ConversationBubble', () => {
  it('renders the compact DeepSeek composer with accessible controls', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="Flash · 幼年形态"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      {...callbacks}
    />)

    expect(html).toContain('DeepSeek Web')
    expect(html).toContain('aria-label="输入消息"')
    expect(html).toContain('aria-label="发送消息"')
    expect(html).not.toContain('当前会话记录')
    expect(html).toContain('本轮 入 未提供')
    expect(html).toContain('出 未提供')
    expect(html).toContain('缓存 未提供')
    expect(html).toContain('费用未提供')
    expect(html).toContain('会话费用未提供')
  })

  it('renders Harness theme, history, streaming state and usage metadata', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="harness"
      activity="streaming"
      modelLabel="deepseek-reasoner"
      messages={[{ id: 'message-1', role: 'user', content: '继续', createdAt: 1 }]}
      streamingText="正在处理"
      historyExpanded
      usage={{ input: 10, output: 20 }}
      {...callbacks}
    />)

    expect(html).toContain('data-dsh-theme="harness"')
    expect(html).toContain('当前会话记录')
    expect(html).toContain('DSH Bridge 离线')
    expect(html).toContain('role="switch"')
    expect(html).toContain('本轮 入 10')
    expect(html).toContain('出 20')
    expect(html).toContain('缓存 未提供')
    expect(html).toContain('费用未提供')
    expect(html).toContain('正在处理')
  })

  it('renders zero-priced API usage as measured rather than unavailable', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-api"
      activity="done"
      modelLabel="deepseek-chat"
      messages={[{ id: 'message-1', role: 'assistant', content: '完成', createdAt: 1, usage: { input: 2, output: 3, cacheRead: 0, cost: 0, estimated: true } }]}
      streamingText=""
      historyExpanded={false}
      usage={{ input: 2, output: 3, cacheRead: 0, cost: 0, estimated: true }}
      apiPricingConfigured
      {...callbacks}
    />)

    expect(html).toContain('本轮 入 2')
    expect(html).toContain('出 3')
    expect(html).toContain('缓存 0')
    expect(html).toContain('费用 约 ¥0.0000')
    expect(html).toContain('会话 约 ¥0.0000')
    expect(html).not.toContain('价格未配置')
  })
})
