import { renderToStaticMarkup } from 'react-dom/server'
import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { ConversationBubble, insertNewlineAtSelection, shouldRevealHistory } from '../src/features/chat/ConversationBubble.tsx'

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
    expect(html).toContain('DSH 当前离线')
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
    expect(html).toContain('dsh-chat__command-menu-button')
    expect(html).not.toContain('dsh-chat__meta')
    // 选择器是岛内自绘的下拉（原生 <select> 的弹层这个窗口弹不出来），所以：
    // 初始渲染只有一枚按钮，选项列表按需展开——静态标记里不应出现第二个模型。
    expect(html).toContain('aria-haspopup="listbox"')
    expect(html).toContain('aria-expanded="false"')
    expect(html).not.toContain('deepseek-v4-pro')
    expect(html).not.toContain('dsh-chat__model-menu')
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

  it('labels a kept transcript as the previous Harness session', () => {
    // 壁纸因主体退出自己复位时保留轨道里的转写（用户要求"缓存**进会话轨道**"）。一段
    // 来路不明的记录会让人以为它是当前后端的，所以要说清它是什么、什么时候回来。
    const kept = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[{ id: 'message-1', role: 'assistant', content: '上次的回答', createdAt: 1 }]}
      streamingText=""
      historyExpanded
      keptTranscript
      {...callbacks}
    />)
    expect(kept).toContain('dsh-chat__history-note')
    expect(kept).toContain('上次的 Harness 会话')
    expect(kept).toContain('上次的回答')

    // 正常情况（没有保留转写）轨道里不该出现这行旁白。
    const fresh = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[{ id: 'message-1', role: 'assistant', content: '当前后端的回答', createdAt: 1 }]}
      streamingText=""
      historyExpanded
      {...callbacks}
    />)
    expect(fresh).not.toContain('dsh-chat__history-note')

    // 标了却没有任何记录可看时也没有旁白：没有什么需要解释的。
    const empty = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[]}
      streamingText=""
      historyExpanded
      keptTranscript
      {...callbacks}
    />)
    expect(empty).not.toContain('dsh-chat__history-note')
  })

  it('shows the amber light while connecting, and the dark one only when it is gone', () => {
    // 三段语义：连上（绿）／正在连（黄呼吸）／不在了（熄灭）。"正在连"时 `harnessReady`
    // 是 false，所以黄灯必须优先于熄灭态——否则"启动中"与"已经死了"看起来一模一样，而这两件
    // 事要让用户做的动作完全不同。
    const connecting = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      harnessAvailability="bridge-loading"
      harnessReady={false}
      harnessSuspect
      {...callbacks}
    />)
    expect(connecting).toContain('data-ready="suspect"')
    // 黄灯一律说"连接中"（用户定的规则）：它在呼吸就说明"还没定"，此时再写别的状态说明
    // （"已连接""正在装载"）都是把不确定性说成了结论。给用户看到的只有一句话：还在连。
    expect(connecting).toContain('连接中')
    expect(connecting).not.toContain('已连接')

    const gone = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      harnessAvailability="offline"
      harnessReady={false}
      {...callbacks}
    />)
    expect(gone).toContain('data-ready="false"')
    expect(gone).not.toContain('data-ready="suspect"')

    // 红灯：**试过、没成**。与黄灯（还没定）必须分开：黄灯下用户只需等，红灯下点滑槽就要立刻
    // 再试一次（拉起对应 harness 进程 + 发握手申请），所以它不能长得跟黄灯一样。
    const failed = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      harnessAvailability="offline"
      harnessReady={false}
      harnessFailed
      {...callbacks}
    />)
    expect(failed).toContain('data-ready="failed"')
    expect(failed).not.toContain('data-ready="suspect"')
    expect(failed).toContain('连接失败')
  })

  it('keeps the switch usable while a start is still in flight', () => {
    // A start can take the whole 45-second readiness window. Dimming the only switch
    // on the island for that long is what made it look stuck: no way back, no way to
    // retry. The label carries the state instead.
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      modelLabel="deepseek-chat"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      harnessAvailability="offline"
      harnessStarting
      onStartHarness={() => undefined}
      {...callbacks}
    />)

    expect(html).toContain('DSH 正在启动')
    expect(html).toContain('data-starting="true"')
    expect(html).not.toMatch(/class="dsh-chat__mode-switch[^"]*"[^>]*disabled/)
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

describe('shouldRevealHistory', () => {
  it('keeps a collapsed island collapsed while the user types', () => {
    // Expanding on the first keystroke moves the composer while the user is aiming at it.
    expect(shouldRevealHistory('typing', false)).toBe(false)
  })

  it('reveals the history when a collapsed island sends', () => {
    // The reply is about to arrive, so the conversation must become visible.
    expect(shouldRevealHistory('send', false)).toBe(true)
  })

  it('leaves an expanded island alone for both actions', () => {
    expect(shouldRevealHistory('typing', true)).toBe(false)
    expect(shouldRevealHistory('send', true)).toBe(false)
  })
})
