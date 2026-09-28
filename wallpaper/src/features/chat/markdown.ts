/**
 * 会话轨道用的一小套 Markdown 解析：**只认用户点名的那些**，其余一律当普通文字。
 *
 * 为什么不是"接一个 Markdown 库"：
 *
 *  1. 这条轨道喂进来的东西不可信 —— 模型输出、用户粘贴的网页片段、DSH 的日志都会到这里。任何
 *     "把文本当 HTML 处理"的路径都是注入面，所以这里**只产出结构化数据**，绝不产出 HTML 字符串；
 *  2. 纯函数可以被钉住行为，"库升级后渲染变了"不是我想在深夜排查的东西；
 *  3. 少即是稳：没被点名的语法保持**字面**显示，用户看到的就是他写的。
 *
 * 支持：代码围栏、有序/无序列表（含嵌套）、表格、引用、分割线；
 *      行内代码、粗体、斜体、删除线（且允许互相嵌套，例如**加粗里含 `代码`**）。
 * 不支持（刻意）：标题、链接、图片、任务清单、下划线斜体（`a_b_c` 里的下划线常常是标识符的
 * 一部分，认它会把 `snake_case` 撕成斜体）。
 */

export type MarkdownAlign = 'left' | 'center' | 'right'

export type MarkdownBlock =
  | { kind: 'paragraph'; text: string }
  | { kind: 'code'; language?: string; text: string }
  | { kind: 'list'; ordered: boolean; items: MarkdownListItem[] }
  | { kind: 'table'; align: MarkdownAlign[]; head: string[]; rows: string[][] }
  | { kind: 'quote'; text: string }
  | { kind: 'rule' }

/** 列表项：`depth` 是缩进层级（0 为顶层），`ordered` 是**它自己那一行的标记**。
 *
 * 为什么每项各自记：原文实测（官壳那个宿主上的真实消息）同一棵树里混着 `1.` 与 `-` —— 按整块
 * 一个类型渲染的话，"1. 有序第二层"会变成圆点，分类就没了。
 */
export interface MarkdownListItem {
  text: string
  depth: number
  ordered: boolean
}

/** 行内片段。`children` 让粗体里能套代码、代码里不能再套任何东西。 */
export type InlineToken =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'bold'; children: InlineToken[] }
  | { kind: 'italic'; children: InlineToken[] }
  | { kind: 'strike'; children: InlineToken[] }

const FENCE = /^\s{0,3}(?:```|~~~)\s*([\w+#.-]*)\s*$/
const UNORDERED = /^(\s*)[-*+]\s+(.*)$/
const ORDERED = /^(\s*)\d+[.)]\s+(.*)$/
/** 分割线：整行只有三个以上同样的 `-`/`*`/`_`（允许空格）。 */
const RULE = /^\s{0,3}([-*_])\s*(?:\1\s*){2,}$/
const QUOTE = /^\s{0,3}>\s?(.*)$/
const TABLE_ROW = /^\s*\|(.+)\|\s*$/

/** 缩进层级：两个空格算一层，制表符算一层。Markdown 允许四空格，两空格是模型最常用的写法。 */
function depthOf(indent: string): number {
  let depth = 0
  for (const ch of indent) depth += ch === '\t' ? 1 : 0
  return depth + Math.floor(indent.replace(/\t/g, '').length / 2)
}

/**
 * 行内解析。**先代码后强调**：反引号里的星号没有强调含义（`` `a**b**` `` 就是一个标识符）。
 *
 * 递归而不是一层扫到底，是因为"粗体里含行内代码"是模型真会写的形状（用户实测输出里就有）。
 */
export function parseInline(source: string): InlineToken[] {
  const tokens: InlineToken[] = []
  const push = (token: InlineToken) => {
    if (token.kind === 'text') {
      if (token.text.length === 0) return
      const last = tokens[tokens.length - 1]
      if (last?.kind === 'text') last.text += token.text
      else tokens.push(token)
      return
    }
    tokens.push(token)
  }
  let rest = source
  while (rest.length > 0) {
    const code = rest.match(/^([^`]*)`([^`]+)`/)
    const bold = rest.match(/^([^*]*(?:\*(?!\*)[^*]*)*)\*\*([^*]+)\*\*/)
    const strike = rest.match(/^([^~]*)~~([^~]+)~~/)
    const italic = rest.match(/^([^*]*)\*([^*\s][^*]*?)\*/)
    const found = [code, bold, strike, italic]
      .filter((match): match is RegExpMatchArray => Boolean(match))
      .sort((a, b) => (a[1] ?? '').length - (b[1] ?? '').length)[0]
    if (!found) {
      push({ kind: 'text', text: rest })
      break
    }
    const at = (found[1] ?? '').length
    const body = found[2] ?? ''
    push({ kind: 'text', text: rest.slice(0, at) })
    if (found === code) push({ kind: 'code', text: body })
    else if (found === bold) push({ kind: 'bold', children: parseInline(body) })
    else if (found === strike) push({ kind: 'strike', children: parseInline(body) })
    else push({ kind: 'italic', children: parseInline(body) })
    rest = rest.slice(at + (found[0] ?? '').length - at)
  }
  return tokens
}

function cellsOf(line: string): string[] | undefined {
  const match = line.match(TABLE_ROW)
  if (!match) return undefined
  return (match[1] ?? '').split('|').map((cell) => cell.trim())
}

function alignOf(separator: string): MarkdownAlign[] | undefined {
  const match = separator.match(TABLE_ROW) ?? separator.match(/^\s*\|?(.*?)\|?\s*$/)
  if (!match) return undefined
  const cells = (match[1] ?? '').split('|').map((cell) => cell.trim())
  if (cells.length === 0) return undefined
  const align: MarkdownAlign[] = []
  for (const cell of cells) {
    if (!/^:?-{1,}:?$/.test(cell)) return undefined
    const left = cell.startsWith(':')
    const right = cell.endsWith(':')
    align.push(left && right ? 'center' : right ? 'right' : 'left')
  }
  return align
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
      blocks.push({ kind: 'code', ...(language ? { language } : {}), text: body.join('\n') })
      if (!closed) break
      continue
    }
    if (RULE.test(line)) {
      flushParagraph()
      blocks.push({ kind: 'rule' })
      continue
    }
    const quote = line.match(QUOTE)
    if (quote) {
      flushParagraph()
      const body: string[] = [quote[1] ?? '']
      for (index += 1; index < lines.length; index += 1) {
        const next = (lines[index] ?? '').match(QUOTE)
        if (!next) {
          index -= 1
          break
        }
        body.push(next[1] ?? '')
      }
      blocks.push({ kind: 'quote', text: body.join('\n') })
      continue
    }
    // 表格：首行有竖线、第二行是分隔行、且列数一致。三条缺一条就当普通文字 —— 正文里一个竖线
    // 不该被吃掉，而"看起来像表格"的排版事故比"没渲染表格"更让人困惑。
    const head = cellsOf(line)
    if (head && head.length >= 2) {
      const align = alignOf(lines[index + 1] ?? '')
      if (align && align.length === head.length) {
        flushParagraph()
        const rows: string[][] = []
        for (index += 2; index < lines.length; index += 1) {
          const row = cellsOf(lines[index] ?? '')
          if (!row) {
            index -= 1
            break
          }
          rows.push(row)
        }
        blocks.push({ kind: 'table', align, head, rows })
        continue
      }
    }
    const item = line.match(UNORDERED) ?? line.match(ORDERED)
    if (item) {
      flushParagraph()
      const ordered = !UNORDERED.test(line)
      const items: MarkdownListItem[] = [{ text: item[2] ?? '', depth: depthOf(item[1] ?? ''), ordered }]
      for (index += 1; index < lines.length; index += 1) {
        const next = lines[index] ?? ''
        if (next.trim() === '') {
          // **列表里的空行不该结束这个列表**：模型很常在条目之间空一行（"松散列表"），而那些
          // 空行后往往正是缩进的子项。实测症状：截图里嵌套项整段不见或全部平铺 —— 列表在第一个
          // 空行处被截断，剩下的子项另外成了一个新的（于是层级被归一化回顶层）。
          const following = (lines[index + 1] ?? '')
          const continues = UNORDERED.test(following) || ORDERED.test(following)
          if (continues) continue
          break
        }
        const nextItem = next.match(UNORDERED) ?? next.match(ORDERED)
        if (nextItem) {
          // 每一项记**它自己那一行**的标记：原文实测（官壳宿主上的真实消息）同一棵树里混着
          // `1.` 与 `-`，按整块一个类型渲染会把"1. 有序第二层"变成圆点，分类就没了。
          items.push({
            text: nextItem[2] ?? '',
            depth: depthOf(nextItem[1] ?? ''),
            ordered: !UNORDERED.test(next),
          })
          continue
        }
        if (/^\s+\S/.test(next)) {
          const last = items[items.length - 1]
          if (last) last.text = `${last.text}\n${next.trim()}`
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
