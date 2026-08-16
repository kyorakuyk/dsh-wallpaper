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
    expect(html).toContain('正在回复')
    expect(html).toContain('30 tokens')
    expect(html).toContain('正在处理')
  })
})
