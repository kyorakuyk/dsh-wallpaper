import { renderToStaticMarkup } from 'react-dom/server'
import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'
import { ConversationBubble, bubbleDateLocale, formatBubbleDate, insertNewlineAtSelection, shouldRevealHistory } from '../src/features/chat/ConversationBubble.tsx'
import { setLanguage } from '../src/i18n/index.ts'

const callbacks = {
  onToggleHistory: () => undefined,
  onSend: () => undefined,
  onStop: () => undefined,
  onClose: () => undefined,
}

// 语言是模块级状态：这个文件里有一条切到英文的用例，跑完必须还原，否则后面按中文写的期望会跟着变
// （与 tests/i18n.spec.ts 同一条规矩）。
afterEach(() => {
  setLanguage('zh')
})

describe('ConversationBubble', () => {
  it('inserts a newline at the selected range without submitting', () => {
    expect(insertNewlineAtSelection('前后', 1, 1)).toEqual({ value: '前\n后', caret: 2 })
    expect(insertNewlineAtSelection('保留这段', 1, 3)).toEqual({ value: '保\n段', caret: 2 })
  })

  it('renders the compact DeepSeek composer with accessible controls', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      {...callbacks}
    />)

    expect(html).not.toContain('DeepSeek Web')
    expect(html).toContain('aria-label="输入消息"')
    expect(html).toContain('aria-label="发送消息"')
    expect(html).not.toContain('当前会话记录')
    // 用量读数已冻结（见下面「用量与费用读数冻结之后」）：这五条原先断言的是"未提供"那一串
    // 必须出现，现在钉的是**反面** —— 同样的渲染里一格读数都不许有。
    expect(html).not.toContain('本轮 入')
    expect(html).not.toContain('缓存 未提供')
    expect(html).not.toContain('费用未提供')
    expect(html).not.toContain('会话费用未提供')
    expect(html).not.toContain('dsh-chat__usage-rail')
    // 顶栏本尊还在（否则上面那条"没有读数"会因为整个顶栏都没渲染而假绿）：两枚占位符留着，
    // 身份在左、Bridge 状态在右的排布不变。
    expect(html).toContain('dsh-chat__topbar-spacer')
    expect(html).toContain('dsh-chat__bottom-status')
  })

  it('renders Harness theme, history and streaming state, and no usage readout', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="harness"
      activity="streaming"
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
    // 这一条原来还传了 `usage={{ input: 10, output: 20 }}` 并断言「本轮 入 10」「出 20」「缓存 未提供」
    // 「费用未提供」。冻结之后 `usage` 仍然照传（数据照旧送进来），但一格读数都不再渲染。
    expect(html).not.toContain('本轮 入 10')
    expect(html).not.toContain('出 20')
    expect(html).not.toContain('dsh-chat__usage-rail')
    expect(html).not.toContain('dsh-chat__turn-usage')
    expect(html).toContain('正在处理')
  })

  it('hides default speaker labels while allowing custom labels', () => {
    const baseProps = {
      backend: 'deepseek-web' as const,
      activity: 'idle' as const,
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

  it('renders no usage readout at all once the readout is frozen, even with a configured zero price', () => {
    // 这条原先钉的是"零价是**量出来的**，不是'未提供'"：它要求 `费用 约 ¥0.0000` 与 `会话 约 ¥0.0000`
    // 出现在渲染里，且不许出现"价格未配置"。读数冻结之后，这一层分辨力没有丢，只是换了被测对象：
    //  - 纯函数那一层（`turnUsageSummary().priceUnconfigured`、`formatCost(0)`）由
    //    `conversationViewModel.spec.ts` 继续逐条钉住，函数本身一行未改；
    //  - 界面这一层改成钉**反面** —— 连"配了价、有 usage、零费用"这种最该出数字的输入，
    //    也不许渲染出任何一格读数。断言不是变松，是"必须显示"换成了"必须已冻结"。
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-api"
      activity="done"
      messages={[{ id: 'message-1', role: 'assistant', content: '完成', createdAt: 1, usage: { input: 2, output: 3, cacheRead: 0, cost: 0, estimated: true } }]}
      streamingText=""
      historyExpanded
      usage={{ input: 2, output: 3, cacheRead: 0, cost: 0, estimated: true }}
      apiPricingConfigured
      {...callbacks}
    />)

    // 转写本身照常渲染 —— 冻的只是读数，不是消息。
    expect(html).toContain('完成')
    expect(html).not.toContain('dsh-chat__usage-rail')
    expect(html).not.toContain('dsh-chat__turn-usage')
    expect(html).not.toContain('dsh-chat__session-cost')
    expect(html).not.toContain('dsh-chat__message-usage')
    expect(html).not.toContain('本轮 入')
    expect(html).not.toContain('价格未配置')
    // 钱与 tokens 的痕迹一个不留：符号、费用字样、以及每条消息下面那行"输入 x · 输出 y"。
    expect(html).not.toContain('¥')
    expect(html).not.toContain('费用')
    expect(html).not.toContain('输入 2 · 输出 3')
  })

  it('places a functional model picker beside conversation history', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="harness"
      activity="idle"
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
      messages={[]}
      streamingText=""
      historyExpanded
      keptTranscript
      {...callbacks}
    />)
    expect(empty).not.toContain('dsh-chat__history-note')
  })

  it('renders the assistant as markdown but never rewrites what the user typed', () => {
    const base = {
      activity: 'idle' as const,
      streamingText: '',
      // 转写只在展开时渲染 —— 不展开的话这条测试什么都没测到（第一版就是这么假绿的）。
      historyExpanded: true,
      ...callbacks,
    }
    const assistant = renderToStaticMarkup(<ConversationBubble
      {...base}
      backend="deepseek-web"
      messages={[{ id: 'a', role: 'assistant', content: '看这段：\n```ts\nconst a = 1\n```\n- 一\n- 二', createdAt: 0 }]}
    />)
    expect(assistant).toContain('dsh-chat__code')
    expect(assistant).toContain('dsh-chat__list')

    // 用户自己的字原样显示：他敲的反引号和星号往往**正是字面意思**，"我打的字被它改了"比排版问题
    // 严重得多，所以这条规则由结构保证（用户消息根本不走 MarkdownBody）。
    const user = renderToStaticMarkup(<ConversationBubble
      {...base}
      backend="deepseek-web"
      messages={[{ id: 'u', role: 'user', content: '```ts\nconst a = 1\n```', createdAt: 0 }]}
    />)
    expect(user).not.toContain('dsh-chat__code')
    expect(user).toContain('```ts')
  })

  it('shows the amber light while connecting, and the dark one only when it is gone', () => {
    // 三段语义：连上（绿）／正在连（黄呼吸）／不在了（熄灭）。"正在连"时 `harnessReady`
    // 是 false，所以黄灯必须优先于熄灭态——否则"启动中"与"已经死了"看起来一模一样，而这两件
    // 事要让用户做的动作完全不同。
    const connecting = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
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

  it('never says 连接失败 while the Bridge is ready', () => {
    // 实测症状：上一次启动尝试失败后标记粘住，常驻监视器后来把桥连上了，界面上就成了
    // "绿灯 + 连接失败"。绿的事实来自当下，失败标记只是对上一次尝试的记录，不能盖过它。
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      harnessAvailability="bridge-ready"
      harnessReady
      harnessFailed
      {...callbacks}
    />)
    expect(html).not.toContain('连接失败')
    expect(html).toContain('DSH Bridge 已连接')
    expect(html).not.toContain('data-ready="failed"')
  })

  it('keeps the switch usable while a start is still in flight', () => {
    // A start can take the whole 45-second readiness window. Dimming the only switch
    // on the island for that long is what made it look stuck: no way back, no way to
    // retry. The label carries the state instead.
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
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

  it('shows the host indicator in the footer of every backend', () => {
    const props = {
      activity: 'idle' as const,
      messages: [],
      streamingText: '',
      historyExpanded: false,
      ...callbacks,
    }
    // 网页与 API 两侧都要有：用户分不清的正是这两个（免费额度还是自己的钱）。
    const web = renderToStaticMarkup(<ConversationBubble {...props} backend="deepseek-web" hostChip={{ text: 'Web', title: 'DeepSeek 网页额度，不产生 API 费用。' }} />)
    expect(web).toContain('dsh-chat__host')
    expect(web).toContain('>Web</span>')
    expect(web).toContain('title="DeepSeek 网页额度，不产生 API 费用。"')

    const api = renderToStaticMarkup(<ConversationBubble {...props} backend="deepseek-api" hostChip={{ text: 'API', title: '你自己的 DeepSeek API key，按 token 计费。' }} />)
    expect(api).toContain('>API</span>')

    // 之前这个位置是"模型不可切换时显示模型名"的只读盒子，harness 模式里干脆不渲染。
    // 现在两侧都有，而且它替换掉了那个盒子。
    const harness = renderToStaticMarkup(<ConversationBubble {...props} backend="harness" hostChip={{ text: 'Desktop', title: '本机 DeepSeek Harness 客户端，它带自己的窗口。' }} />)
    expect(harness).toContain('>Desktop</span>')
    expect(harness).not.toContain('dsh-chat__model"')

    // 指示器是状态不是控件：它自己那一段里不能出现可点的元素。
    for (const html of [web, api, harness]) {
      const chipStart = html.indexOf('dsh-chat__meta')
      const textStart = html.indexOf('dsh-chat__host')
      expect(chipStart).toBeGreaterThan(-1)
      expect(html.slice(chipStart, textStart)).not.toContain('<button')
    }
  })

  it('omits the indicator when the caller has no host to name', () => {
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      {...callbacks}
    />)
    expect(html).not.toContain('dsh-chat__host')
  })

  it('formats the session date in the language on screen', () => {
    // 「桌面会话」旁边那一格日期原先写死 `Intl.DateTimeFormat('zh-CN', …)`：切到英文之后
    // 界面是英文、日期还是中文格式。locale 现在由当前语言推出（计划第 4 步明确要求这一条），
    // 而 formatter 每次调用新建 —— 提到模块级常量上就会在 import 那一刻把语言定死。
    expect(bubbleDateLocale('zh')).toBe('zh-CN')
    expect(bubbleDateLocale('en')).toBe('en-US')

    // 同一个时间戳，两种语言：格式必须不同，而且各自就是那套 locale 的结果。
    const date = new Date(2026, 2, 5, 12)
    expect(formatBubbleDate(date, 'zh')).toBe('3月5日周四')
    expect(formatBubbleDate(date, 'en')).toBe('Thu, March 5')

    setLanguage('en')
    const english = formatBubbleDate(date) // 不传语言 → 取当下这一份（与 `t()` 同一条规矩）
    expect(english).toBe('Thu, March 5')
    expect(english).not.toBe(formatBubbleDate(date, 'zh'))

    // 组件真的用它算了那一格：英文下渲染出来的是英文格式（跨零点的一瞬间两个候选都认）。
    const before = new Date()
    const html = renderToStaticMarkup(<ConversationBubble
      backend="deepseek-web"
      activity="idle"
      messages={[]}
      streamingText=""
      historyExpanded={false}
      {...callbacks}
    />)
    const after = new Date()
    expect([before, after].map((now) => formatBubbleDate(now, 'en')).some((text) => html.includes(text))).toBe(true)
  })
})

/**
 * 用量与费用读数冻结之后（用户要求界面上不再出现"计价"与"tokens 消耗"）。
 *
 * 上面那些渲染断言只能证明"这一次渲染里没有它"。这一组钉的是**源码里的形状**：
 * 两块读数还在文件里（注释着，连恢复用的 import 原样都留着），而活着的代码里一行都不剩。
 * 写法与 `launchArgsAndInstances.spec.ts` 的「启动参数冻结之后：没有参数流出去」同源 ——
 * 不是把断言删掉，而是把"必须显示"换成"必须已被冻结、且留了复活的话"。
 */
describe('用量与费用读数冻结之后', () => {
  async function bubbleSource(): Promise<string> {
    return (await readFile(resolve(dirname(fileURLToPath(import.meta.url)), '../src/features/chat/ConversationBubble.tsx'), 'utf8'))
      .replace(/\r\n?/g, '\n')
  }

  /** 剥掉注释之后还剩什么 —— 与 `noHardcodedCopy.spec.ts` 同一个剥法（注释里的中文是允许的）。 */
  function withoutComments(source: string): string {
    return source.replace(/\/\*[\s\S]*?\*\//g, '')
      .split('\n')
      .map((line) => {
        const at = line.search(/(^|\s)\/\//)
        return at >= 0 ? line.slice(0, at) : line
      })
      .join('\n')
  }

  it('keeps both frozen blocks in the file, so reviving them needs no git archaeology', async () => {
    const source = await bubbleSource()
    // 两块读数都还在（在注释里）：顶栏那条用量串，与每条消息下面那行。
    expect(source).toContain('className="dsh-chat__usage-rail"')
    expect(source).toContain('className="dsh-chat__message-usage"')
    // 恢复用的原始 import 逐字留着（照 `// args: parseLaunchArgs` 那条先例）。
    expect(source).toMatch(/^\s*\/\/ import \{ composerPlaceholder, formatCost/m)
    // 每一处冻结都有说明：import、两个 prop、两个派生值、`UsageLine`、调用处、顶栏。
    expect((source.match(/FREEZE/g) ?? []).length).toBeGreaterThanOrEqual(6)
  })

  it('leaves nothing alive that could render a usage or cost readout', async () => {
    const alive = withoutComments(await bubbleSource())
    for (const symbol of [
      'dsh-chat__usage-rail',
      'dsh-chat__turn-usage',
      'dsh-chat__session-cost',
      'dsh-chat__billing',
      'dsh-chat__message-usage',
      'UsageLine',
      'turnUsageSummary',
      'sessionCostSummary',
      'formatCost',
      'chat.bubble.turn-usage',
      'chat.bubble.session-cost',
      'chat.bubble.usage.',
    ]) {
      expect(alive, `${symbol} 仍然活在代码里`).not.toContain(symbol)
    }
    // 但冻结的是**读数**，不是整个气泡：其余部分照常活着（否则上面那两条会自动变成假绿）。
    expect(alive).toContain('composerPlaceholder')
    expect(alive).toContain('isBusyActivity')
    expect(alive).toContain('dsh-chat__topbar-spacer')
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
