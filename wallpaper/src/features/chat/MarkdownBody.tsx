import { Fragment, useMemo } from 'react'
import { parseInline, parseMarkdown, type InlineToken } from './markdown.ts'

/**
 * 行内片段 → React 节点。
 *
 * 这里没有任何 `innerHTML`：文本永远进文本节点，`code` / `strong` 是我们自己造的元素。于是"把
 * 模型输出当 HTML 渲染"这条注入路径在结构上就不存在，而不是靠转义函数去堵。
 */
function Inline({ tokens }: { tokens: InlineToken[] }) {
  return <>
    {tokens.map((token, index) => token.kind === 'text'
      ? <Fragment key={index}>{token.text}</Fragment>
      : token.kind === 'code'
        ? <code key={index} className="dsh-chat__inline-code">{token.text}</code>
        : <strong key={index}>{token.text}</strong>)}
  </>
}

/**
 * 助手消息正文的最小 Markdown 呈现（代码围栏、行内代码、粗体、列表）。
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
        const items = block.items.map((item, itemIndex) => (
          <li key={itemIndex}><Inline tokens={parseInline(item)} /></li>
        ))
        return block.ordered
          ? <ol key={index} className="dsh-chat__list">{items}</ol>
          : <ul key={index} className="dsh-chat__list">{items}</ul>
      }
      return <p key={index} className="dsh-chat__message-body"><Inline tokens={parseInline(block.text)} /></p>
    })}
  </>
}
