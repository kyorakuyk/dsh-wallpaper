import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * The web backend can only read DeepSeek's **rendered** DOM: by the time the
 * injected script runs, `**bold**` is a `<strong>`, a list is a row of `<p>`s
 * and code is a `<code>`. The extractor therefore rebuilds Markdown from the
 * structure, and this file is where that rebuild is checked — without a browser
 * and without their site.
 *
 * The converter is exposed on `window.__DSHWallpaperMarkdown` by the injected
 * snapshot script (temporarily, alongside the reply probe), so the real shipped
 * function is what runs here, not a copy.
 */

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

interface FakeNode {
  nodeType: number
  tagName?: string
  textContent: string
  childNodes: FakeNode[]
  children: FakeNode[]
  parentElement: FakeNode | null
  className?: string
  attributes?: Record<string, string>
  getAttribute(name: string): string | null
  querySelector?(selector: string): FakeNode | null
  querySelectorAll?(selector: string): FakeNode[]
  closest?(selector: string): FakeNode | null
  compareDocumentPosition?(other: FakeNode): number
  matches?(selector: string): boolean
  getClientRects?(): unknown[]
  disabled?: boolean
}

const BLOCK_SELECTOR_TAGS = ['ul', 'ol', 'pre', 'table', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr']

function text(value: string): FakeNode {
  return {
    nodeType: 3,
    textContent: value,
    childNodes: [],
    children: [],
    parentElement: null,
    getAttribute: () => null,
  }
}

function descendants(node: FakeNode): FakeNode[] {
  const found: FakeNode[] = []
  for (const child of node.children) {
    found.push(child, ...descendants(child))
  }
  return found
}

/** Tag names and `.class` selectors, comma-separated — what the injected script queries. */
function matchesSelector(node: FakeNode, selector: string): boolean {
  return selector.split(',').some((part) => {
    const trimmed = part.trim()
    if (!trimmed) return false
    if (trimmed.startsWith('.')) return (node.className || '').split(/\s+/).includes(trimmed.slice(1))
    return (node.tagName || '').toLowerCase() === trimmed.toLowerCase()
  })
}

function el(tagName: string, children: Array<FakeNode | string> = [], attributes: Record<string, string> = {}): FakeNode {
  const node: FakeNode = {
    nodeType: 1,
    tagName: tagName.toUpperCase(),
    childNodes: [],
    children: [],
    parentElement: null,
    attributes,
    className: attributes.class || '',
    textContent: '',
    disabled: false,
    getAttribute: (name: string) => (name in attributes ? attributes[name] : null),
    querySelectorAll: (selector: string) => descendants(node).filter((candidate) => matchesSelector(candidate, selector)),
    querySelector: (selector: string) => node.querySelectorAll?.(selector)[0] ?? null,
    closest: () => null,
    compareDocumentPosition: () => 0,
    matches: (selector: string) => matchesSelector(node, selector),
    getClientRects: () => [{}],
  }
  for (const child of children) {
    const created = typeof child === 'string' ? text(child) : child
    created.parentElement = node
    node.childNodes.push(created)
    if (created.nodeType === 1) node.children.push(created)
    node.textContent += created.textContent
  }
  return node
}

/** The assistant message node as the site renders it: chrome + one markdown container. */
function assistantMessage(containerChildren: Array<FakeNode | string>): FakeNode {
  const container = el('div', containerChildren, { class: 'ds-markdown ds-markdown--block' })
  const toolbar = el('div', [el('button', ['复制'], { class: 'ds-icon-button' })], { class: 'ds-markdown-toolbar' })
  return el('div', [toolbar, container], { 'data-message-id': 'answer-1' })
}

async function loadConverter(): Promise<(assistant: FakeNode, paragraphs: FakeNode[]) => string> {
  const source = await readFile(resolve(wallpaperRoot, 'src-tauri/src/deepseek_web.rs'), 'utf8')
  const match = /const SNAPSHOT_SCRIPT: &str = r#"(.*?)"#;/s.exec(source)
  expect(match, 'SNAPSHOT_SCRIPT not found in deepseek_web.rs').toBeTruthy()
  const script = (match?.[1] || '').replace('__DSH_DEEPSEEK_WEB_ADAPTER_CONFIG__', JSON.stringify({
    assistantSelectors: ['[data-message-id]'],
    markdownSelectors: ['.ds-markdown-paragraph'],
    messageSelectors: [],
    composerSelectors: [],
    sendTokens: [],
    stopTokens: [],
    terminalTokens: [],
    loginTokens: [],
    conversationPathTemplate: '/a/chat/s/{id}',
  }))
  const emptyList: FakeNode[] = []
  const documentStub = {
    readyState: 'complete',
    documentElement: el('html', []),
    body: el('body', []),
    querySelectorAll: () => emptyList,
    querySelector: () => null,
  }
  const windowStub: Record<string, unknown> = { __DSHWallpaperDomObserver: { revision: 1 } }
  const locationStub = { pathname: '/a/chat/s/abc' }
  const styleStub = { display: 'block', visibility: 'visible', opacity: '1', fontWeight: '400', fontStyle: 'normal', textDecorationLine: 'none' }
  const factory = new Function(
    'window', 'document', 'location', 'getComputedStyle', 'Node',
    `${script}\n;return window.__DSHWallpaperMarkdown;`,
  ) as (...args: unknown[]) => (assistant: FakeNode, paragraphs: FakeNode[]) => string
  const converter = factory(
    windowStub,
    documentStub,
    locationStub,
    () => styleStub,
    { DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_PRECEDING: 2 },
  )
  expect(typeof converter, 'the snapshot script must expose __DSHWallpaperMarkdown').toBe('function')
  return converter
}

function paragraphsOf(assistant: FakeNode): FakeNode[] {
  return assistant.querySelectorAll?.('.ds-markdown-paragraph') ?? []
}

describe('deepseek web DOM → Markdown', () => {
  it('rebuilds inline emphasis, code and links', async () => {
    const convert = await loadConverter()
    const assistant = assistantMessage([
      el('p', ['这段文字演示了 ', el('strong', ['加粗']), '、', el('em', ['斜体']), ' 以及行内代码 ', el('code', ['npm install']), ' 的写法。'], { class: 'ds-markdown-paragraph' }),
    ])
    expect(convert(assistant, paragraphsOf(assistant))).toBe(
      '这段文字演示了 **加粗**、*斜体* 以及行内代码 `npm install` 的写法。',
    )
  })

  it('leaves the strikethrough the site does not render as literal tildes', async () => {
    const convert = await loadConverter()
    // 页面对 ~~ 不做任何处理：波浪号就以文本节点的形式夹在 span 两侧。
    const assistant = assistantMessage([
      el('p', ['删除线：', '~~', el('span', ['删除线文字']), '~~', '。'], { class: 'ds-markdown-paragraph' }),
    ])
    expect(convert(assistant, paragraphsOf(assistant))).toContain('~~删除线文字~~')
  })

  it('turns the paragraph rows back into nested lists', async () => {
    const convert = await loadConverter()
    // 站点把每个列表项渲染成一个 <p>，靠结构找不到 <ul>；这里正是"容器发现 + 还原"要解决的。
    const list = el('ul', [
      el('li', ['苹果']),
      el('li', ['香蕉', el('ul', [el('li', ['进口香蕉'])])]),
      el('li', ['橙子']),
    ])
    const assistant = assistantMessage([
      el('p', ['水果：'], { class: 'ds-markdown-paragraph' }),
      list,
      el('ol', [el('li', ['安装依赖']), el('li', ['配置环境'])]),
    ])
    const markdown = convert(assistant, paragraphsOf(assistant))
    expect(markdown).toContain('- 苹果')
    expect(markdown).toContain('- 香蕉\n  - 进口香蕉')
    expect(markdown).toContain('1. 安装依赖\n1. 配置环境')
  })

  it('rebuilds headings, quotes, code fences and tables', async () => {
    const convert = await loadConverter()
    const assistant = assistantMessage([
      el('h2', ['Markdown 综合示例']),
      el('blockquote', [el('p', ['学而时习之'])]),
      el('pre', [el('code', ['def hello(name):\n    return name'], { class: 'language-python' })]),
      el('table', [
        el('thead', [el('tr', [el('th', ['项目']), el('th', ['状态'])])]),
        el('tbody', [el('tr', [el('td', ['需求']), el('td', ['完成'])])]),
      ]),
      el('hr'),
    ])
    const markdown = convert(assistant, paragraphsOf(assistant))
    expect(markdown).toContain('## Markdown 综合示例')
    expect(markdown).toContain('> 学而时习之')
    expect(markdown).toContain('```python\ndef hello(name):\n    return name\n```')
    expect(markdown).toContain('| 项目 | 状态 |')
    expect(markdown).toContain('| --- | --- |')
    expect(markdown).toContain('| 需求 | 完成 |')
    expect(markdown).toContain('---')
  })

  it('never lets toolbar chrome into the answer', async () => {
    const convert = await loadConverter()
    const assistant = assistantMessage([
      el('p', ['正文。'], { class: 'ds-markdown-paragraph' }),
      el('ul', [el('li', ['一项'])]),
    ])
    const markdown = convert(assistant, paragraphsOf(assistant))
    expect(markdown).not.toContain('复制')
    expect(markdown.trim()).toBe('正文。\n\n- 一项')
  })

  it('falls back to the paragraphs when a reply has no block markup', async () => {
    const convert = await loadConverter()
    const assistant = assistantMessage([
      el('p', ['第一段'], { class: 'ds-markdown-paragraph' }),
      el('p', ['第二段'], { class: 'ds-markdown-paragraph' }),
    ])
    expect(convert(assistant, paragraphsOf(assistant))).toBe('第一段\n\n第二段')
  })
})
