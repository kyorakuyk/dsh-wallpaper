import { renderToStaticMarkup } from 'react-dom/server'
import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { ConversationBubble, insertNewlineAtSelection } from '../src/features/chat/ConversationBubble.tsx'

const callbacks = {
  onToggleHistory: () => undefined,
  onSend: () => undefined,
  onStop: () => undefined,
  onClose: () => undefined,
}

describe('ConversationBubble', () => {
  it('inserts a newline at the selected range without submitting', () => {
    expect(insertNewlineAtSelection('前后', 1, 1)).toEqual({ value: '前\n后', caret: 2 })
    expect(insertNewlineAtSelection('保留这段', 1, 3)).toEqual({ value: '保\n段', caret: 2 })
  })

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

    expect(html).not.toContain('DeepSeek Web')
    expect(html).toContain('aria-label="输入消息"')
    expect(html).toContain('aria-label="发送消息"')
    expect(html).not.toContain('当前会话记录')
    expect(html).toContain('本轮 入 未提供')
    expect(html).toContain('出 未提供')
    expect(html).toContain('缓存 未提供')
    expect(html).toContain('费用未提供')
    expect(html).toContain('会话费用未提供')
    expect(html).toContain('dsh-chat__usage-rail')
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

  it('hides default speaker labels while allowing custom labels', () => {
    const baseProps = {
      backend: 'deepseek-web' as const,
      activity: 'idle' as const,
      modelLabel: 'Flash · 幼年形态',
      messages: [{ id: 'message-1', role: 'user' as const, content: '测试', createdAt: 1 }],
      streamingText: '回复',
      historyExpanded: true,
      ...callbacks,
    }
    const hidden = renderToStaticMarkup(<ConversationBubble {...baseProps} />)
    expect(hidden).not.toContain('dsh-chat__message-label')
    expect(hidden).not.toContain('大肥鱼')
    expect(hidden).not.toContain('你')

    const custom = renderToStaticMarkup(<ConversationBubble
      {...baseProps}
      speakerLabels={{ user: '访客', assistant: '助手' }}
    />)
    expect(custom).toContain('访客')
    expect(custom).toContain('助手')
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

  it('places a functional model picker beside conversation history', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="harness"
      activity="idle"
      modelLabel="deepseek-v4-flash"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      modelOptions={['deepseek-v4-flash', 'deepseek-v4-pro']}
      selectedModel="deepseek-v4-flash"
      onSelectModel={() => undefined}
      commands={[{ name: 'plan', description: '进入计划模式' }]}
      {...callbacks}
    />)

    expect(html).toContain('aria-label="切换模型"')
    expect(html).toContain('deepseek-v4-flash')
    expect(html).toContain('deepseek-v4-pro')
    expect(html).toContain('会话记录')
    expect(html).toContain('dsh-chat__command-menu-button')
    expect(html).not.toContain('dsh-chat__meta')
  })

  it('keeps the offline DSH switch clickable and offers setup when no path is configured', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      harnessAvailability="offline"
      onConfigureHarness={() => undefined}
      {...callbacks}
    />)

    expect(html).toContain('aria-label="配置 DSH"')
    expect(html).not.toMatch(/class="dsh-chat__mode-switch"[^>]*disabled/)
  })

  it('keeps the composer editable while Harness is offline', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="harness"
      activity="idle"
      modelLabel="deepseek-v4-flash"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      disabled
      {...callbacks}
    />)

    expect(html).toContain('aria-label="输入消息"')
    expect(html).not.toMatch(/<textarea[^>]*disabled/)
    expect(html).toMatch(/<button[^>]*disabled[^>]*aria-label="发送消息"/)
  })

  it('keeps transcript text above the history fade overlay', async () => {
    const css = await readFile(resolve(dirname(fileURLToPath(import.meta.url)), '../src/features/chat/ConversationBubble.css'), 'utf8')
    expect(css).toMatch(/\.dsh-chat__history-wrap::before,[\s\S]*?z-index: 0;/)
    expect(css).toMatch(/\.dsh-chat__history \{[\s\S]*?position: relative;[\s\S]*?z-index: 1;/)
    expect(css).toContain('mask-image: none')
    expect(css).toContain('-webkit-mask-image: none')
  })
})
