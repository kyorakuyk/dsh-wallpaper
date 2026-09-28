import { Fragment, useMemo, type ReactNode } from 'react'
import { parseInline, parseMarkdown, type InlineToken, type MarkdownListItem } from './markdown.ts'

/**
 * 行内片段 → React 节点（可递归：粗体里套代码是模型真会写的形状）。
 *
 * 这里没有任何 `innerHTML`：文本永远进文本节点，`code` / `strong` / `em` / `del` 都是我们自己造的
 * 元素。于是"把模型输出当 HTML 渲染"这条注入路径在结构上就不存在，而不是靠转义函数去堵。
 */
function Inline({ tokens }: { tokens: InlineToken[] }): ReactNode {
  return <>
    {tokens.map((token, index) => {
      switch (token.kind) {
        case 'code':
          return <code key={index} className="dsh-chat__inline-code">{token.text}</code>
        case 'bold':
          return <strong key={index}>{Inline({ tokens: token.children })}</strong>
        case 'italic':
          return <em key={index}>{Inline({ tokens: token.children })}</em>
        case 'strike':
          return <del key={index}>{Inline({ tokens: token.children })}</del>
        default:
          return <Fragment key={index}>{token.text}</Fragment>
      }
    })}
  </>
}

/**
 * 把扁平带层级的列表项还原成嵌套的 `ul`/`ol`。
 *
 * 层级先归一化成 0、1、2…：模型写两空格、四空格、制表符都有，逐个去猜它用了哪种缩进会把
 * "缩进差一点"变成"结构差一层"。归一化之后只看"比上一层深"这件事。
 */
function listTree(items: readonly MarkdownListItem[], ordered: boolean, level = 0): ReactNode {
  const levels = [...new Set(items.map((item) => item.depth))].sort((a, b) => a - b)
  const normalised = items.map((item) => ({ ...item, depth: levels.indexOf(item.depth) }))
  const walk = (from: number, depth: number): { nodes: ReactNode[]; next: number } => {
    const nodes: ReactNode[] = []
    let index = from
    while (index < normalised.length) {
      const item = normalised[index]!
      if (item.depth < depth) break
      if (item.depth > depth) break
      let end = index + 1
      while (end < normalised.length && normalised[end]!.depth > depth) end += 1
      const children = normalised.slice(index + 1, end)
      nodes.push(<li key={index}>
        {Inline({ tokens: parseInline(item.text) })}
        {children.length > 0 ? listTree(children, ordered, depth + 1) : null}
      </li>)
      index = end
    }
    return { nodes, next: index }
  }
  const { nodes } = walk(0, level === 0 ? 0 : level)
  return ordered
    ? <ol className="dsh-chat__list">{nodes}</ol>
    : <ul className="dsh-chat__list">{nodes}</ul>
}

/**
 * 助手消息正文的最小 Markdown 呈现。
 *
 * **只给助手消息用**：用户自己敲进来的字必须原样显示。这不是对称洁癖 —— "我打的字被它改了"比
 * "表格没对齐"严重得多，而用户输入里的 `*` 和反引号往往正是他要的字面意思。
 *
 * 分块解析结果按文本 memo：流式输出时文本每个动画帧才提交一次（上游已经合帧），所以这是每帧
 * 一次线性扫描，而不是每个字符一次。
 */
export function MarkdownBody({ text }: { text: string }) {
  const blocks = useMemo(() => parseMarkdown(text), [text])
  return <>
    {blocks.map((block, index) => {
      if (block.kind === 'code') {
        return <pre
          key={index}
          className="dsh-chat__code"
          {...(block.language ? { 'data-language': block.language } : {})}
        ><code>{block.text}</code></pre>
      }
      if (block.kind === 'list') {
        return <Fragment key={index}>{listTree(block.items, block.ordered)}</Fragment>
      }
      if (block.kind === 'table') {
        // 外层负责横向滚动：岛很窄，列多了滚动比挤压变形诚实。
        return <div key={index} className="dsh-chat__table-scroll"><table className="dsh-chat__table">
          <thead><tr>{block.head.map((cell, cellIndex) => (
            <th key={cellIndex} style={{ textAlign: block.align[cellIndex] ?? 'left' }}>
              {Inline({ tokens: parseInline(cell) })}
            </th>
          ))}</tr></thead>
          <tbody>{block.rows.map((row, rowIndex) => (
            <tr key={rowIndex}>{block.head.map((_, cellIndex) => (
              <td key={cellIndex} style={{ textAlign: block.align[cellIndex] ?? 'left' }}>
                {Inline({ tokens: parseInline(row[cellIndex] ?? '') })}
              </td>
            ))}</tr>
          ))}</tbody>
        </table></div>
      }
      if (block.kind === 'quote') {
        return <blockquote key={index} className="dsh-chat__quote">{Inline({ tokens: parseInline(block.text) })}</blockquote>
      }
      if (block.kind === 'rule') return <hr key={index} className="dsh-chat__rule" />
      return <p key={index} className="dsh-chat__message-body">{Inline({ tokens: parseInline(block.text) })}</p>
    })}
  </>
}
