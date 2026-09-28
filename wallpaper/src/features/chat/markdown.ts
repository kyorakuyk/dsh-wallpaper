/**
 * 会话轨道用的一小套 Markdown 解析：**只认用户要的那几样**，其余一律当普通文字。
 *
 * 为什么不是"接一个 Markdown 库"：
 *
 *  1. 这条轨道喂进来的东西不可信 —— 模型输出、用户粘贴的网页片段、DSH 的日志都会到这里。任何
 *     "把文本当 HTML 处理"的路径都是注入面，所以这里**只产出结构化数据**，绝不产出 HTML 字符串；
 *  2. 需要的语法只有代码块、行内代码、粗体、列表。多解析出来的东西（表格、图片、链接自动识别）
 *     在这块窄轨道里只会添乱：表格要正确渲染得靠等宽列宽，而这里的正文是比例字体；
 *  3. 纯函数可以被钉住行为，而"库升级后渲染变了"不是我想在深夜排查的东西。
 *
 * 支持的块：
 *   - 代码围栏（三反引号，可有语言标注；**未闭合也成块** —— 流式输出时围栏常常先开一半）
 *   - 有序 / 无序列表（`-` `*` `+` 与 `1.`；缩进续行并入上一条）
 *   - 段落（连续非空行合成一段，空行分段）
 * 行内：`code`、**bold**。
 * 其它一切（`##`、`>`、`|`、`![..]`、`<b>`…）按字面显示 —— 与现在完全一致。
 */

export type MarkdownBlock =
  | { kind: 'paragraph'; text: string }
  | { kind: 'code'; language?: string; text: string }
  | { kind: 'list'; ordered: boolean; items: string[] }

/** 行内片段：文字、代码、粗体。渲染端只把它变成文本节点与 `<code>`/`<strong>`。 */
export type InlineToken =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'bold'; text: string }

const FENCE = /^\s{0,3}(?:```|~~~)\s*([\w+#.-]*)\s*$/
const UNORDERED = /^(\s*)[-*+]\s+(.*)$/
const ORDERED = /^(\s*)\d+[.)]\s+(.*)$/

/**
 * 行内解析：**先代码后粗体**，因为反引号里的星号没有强调含义（`` `a**b**` `` 就是一个标识符）。
 * 不处理转义：这条轨道上没人写 `\*`，而"支持转义"意味着更多能出错的地方。
 */
export function parseInline(source: string): InlineToken[] {
  const tokens: InlineToken[] = []
  let rest = source
  const push = (token: InlineToken) => {
    if (token.text.length === 0) return
    const last = tokens[tokens.length - 1]
    if (token.kind === 'text' && last?.kind === 'text') last.text += token.text
    else tokens.push(token)
  }
  while (rest.length > 0) {
    const code = rest.match(/^([^`]*)`([^`]+)`/)
    const bold = rest.match(/^([^*]*)\*\*([^*]+)\*\*/)
    const next = [code, bold]
      .filter((match): match is RegExpMatchArray => Boolean(match))
      .sort((a, b) => (a[1] ?? '').length - (b[1] ?? '').length)[0]
    if (!next) {
      push({ kind: 'text', text: rest })
      break
    }
    const at = (next[1] ?? '').length
    push({ kind: 'text', text: rest.slice(0, at) })
    push(next === code
      ? { kind: 'code', text: next[2] ?? '' }
      : { kind: 'bold', text: next[2] ?? '' })
    rest = rest.slice(at + next[0].length - (next[1] ?? '').length)
  }
  return tokens
}

export function parseMarkdown(source: string): MarkdownBlock[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n')
  const blocks: MarkdownBlock[] = []
  let paragraph: string[] = []
  const flushParagraph = () => {
    if (paragraph.length === 0) return
    blocks.push({ kind: 'paragraph', text: paragraph.join('\n') })
    paragraph = []
  }
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? ''
    const fence = line.match(FENCE)
    if (fence) {
      flushParagraph()
      const body: string[] = []
      let closed = false
      for (index += 1; index < lines.length; index += 1) {
        const inner = lines[index] ?? ''
        if (FENCE.test(inner)) {
          closed = true
          break
        }
        body.push(inner)
      }
      // 未闭合的围栏也成块：流式输出时它先开一半，用户此刻就该看到"这是代码"。
      const language = (fence[1] ?? '').trim()
      blocks.push({
        kind: 'code',
        ...(language ? { language } : {}),
        text: body.join('\n'),
      })
      if (!closed) break
      continue
    }
    const item = line.match(UNORDERED) ?? line.match(ORDERED)
    if (item) {
      flushParagraph()
      const ordered = UNORDERED.test(line) === false
      const items: string[] = [item[2] ?? '']
      // 缩进更深的后续行并入上一条（Markdown 的软换行），空行结束这个列表。
      for (index += 1; index < lines.length; index += 1) {
        const next = lines[index] ?? ''
        if (next.trim() === '') break
        const nextItem = next.match(UNORDERED) ?? next.match(ORDERED)
        if (nextItem) {
          items.push(nextItem[2] ?? '')
          continue
        }
        if (/^\s+\S/.test(next)) {
          items[items.length - 1] = `${items[items.length - 1]}\n${next.trim()}`
          continue
        }
        index -= 1
        break
      }
      blocks.push({ kind: 'list', ordered, items })
      continue
    }
    if (line.trim() === '') {
      flushParagraph()
      continue
    }
    paragraph.push(line)
  }
  flushParagraph()
  return blocks
}
