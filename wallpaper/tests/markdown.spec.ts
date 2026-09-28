import { describe, expect, it } from 'vitest'
import { parseInline, parseMarkdown } from '../src/features/chat/markdown.ts'

/**
 * 这套解析只做用户点名的四样：代码围栏、行内代码、粗体、列表。这里钉住的正是"多做的那部分不做"——
 * 没有被点名的语法必须保持**字面**显示，否则界面会开始自作主张地改写用户看到的东西。
 */
describe('conversation markdown', () => {
  it('splits paragraphs on blank lines and keeps line breaks inside one', () => {
    const blocks = parseMarkdown('第一行\n第二行\n\n下一段')
    expect(blocks).toEqual([
      { kind: 'paragraph', text: '第一行\n第二行' },
      { kind: 'paragraph', text: '下一段' },
    ])
  })

  it('makes a fenced block, with or without a language', () => {
    expect(parseMarkdown('```ts\nconst a = 1\n```')).toEqual([
      { kind: 'code', language: 'ts', text: 'const a = 1' },
    ])
    expect(parseMarkdown('```\nplain\n```')).toEqual([{ kind: 'code', text: 'plain' }])
  })

  it('treats an unclosed fence as code to the end', () => {
    // 流式输出时围栏常常先开一半：这一刻用户就该看到"这是代码"，而不是半截普通文字。
    expect(parseMarkdown('说明\n```js\nconst a = 1')).toEqual([
      { kind: 'paragraph', text: '说明' },
      { kind: 'code', language: 'js', text: 'const a = 1' },
    ])
  })

  it('groups loose and numbered lists, merging an indented continuation', () => {
    expect(parseMarkdown('- 一\n- 二\n  续行\n\n1. 甲\n2) 乙')).toEqual([
      { kind: 'list', ordered: false, items: ['一', '二\n续行'] },
      { kind: 'list', ordered: true, items: ['甲', '乙'] },
    ])
  })

  it('reads inline code before bold, so stars inside backticks stay literal', () => {
    expect(parseInline('前 `a**b**` 后')).toEqual([
      { kind: 'text', text: '前 ' },
      { kind: 'code', text: 'a**b**' },
      { kind: 'text', text: ' 后' },
    ])
    expect(parseInline('这是**重点**。')).toEqual([
      { kind: 'text', text: '这是' },
      { kind: 'bold', text: '重点' },
      { kind: 'text', text: '。' },
    ])
  })

  it('leaves everything else exactly as written', () => {
    // 没被点名的语法一律字面显示：标题、引用、表格、HTML 片段都当普通文字。
    const tables = parseMarkdown('## 标题\n\n| a | b |\n| - | - |\n\n<b>粗</b> 和 ![图](x.png)')
    expect(tables).toEqual([
      { kind: 'paragraph', text: '## 标题' },
      { kind: 'paragraph', text: '| a | b |\n| - | - |' },
      { kind: 'paragraph', text: '<b>粗</b> 和 ![图](x.png)' },
    ])
    // 并且 `<b>` 只是**文字片段**，不是任何元素：这条路不存在把文本当 HTML 渲染的分支。
    expect(parseInline('<b>粗</b>')).toEqual([{ kind: 'text', text: '<b>粗</b>' }])
  })
})
