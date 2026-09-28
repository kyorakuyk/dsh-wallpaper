import { describe, expect, it } from 'vitest'
import { parseInline, parseMarkdown } from '../src/features/chat/markdown.ts'

/**
 * 这套解析只认用户点名的语法。这里既钉"要生效的"，也钉"必须保持字面"的 —— 后者同样重要：界面
 * 自作主张改写用户看到的东西，比少渲染一种语法糟糕得多。
 */
describe('conversation markdown', () => {
  it('splits paragraphs on blank lines, keeping line breaks inside one', () => {
    expect(parseMarkdown('第一行\n第二行\n\n下一段')).toEqual([
      { kind: 'paragraph', text: '第一行\n第二行' },
      { kind: 'paragraph', text: '下一段' },
    ])
  })

  it('makes a fenced block, with or without a language, closed or not', () => {
    expect(parseMarkdown('```ts\nconst a = 1\n```')).toEqual([
      { kind: 'code', language: 'ts', text: 'const a = 1' },
    ])
    expect(parseMarkdown('```\nplain\n```')).toEqual([{ kind: 'code', text: 'plain' }])
    // 未闭合也成块：流式输出时围栏先开一半，那一刻就该显示"这是代码"。
    expect(parseMarkdown('说明\n```js\nconst a = 1')).toEqual([
      { kind: 'paragraph', text: '说明' },
      { kind: 'code', language: 'js', text: 'const a = 1' },
    ])
  })

  it('keeps list nesting as depth, and merges an indented continuation', () => {
    expect(parseMarkdown('- 一\n- 二\n  续行\n  - 嵌套\n- 三')).toEqual([
      {
        kind: 'list',
        ordered: false,
        items: [
          { text: '一', depth: 0, ordered: false },
          { text: '二\n续行', depth: 0, ordered: false },
          { text: '嵌套', depth: 1, ordered: false },
          { text: '三', depth: 0, ordered: false },
        ],
      },
    ])
    expect(parseMarkdown('1. 甲\n2) 乙')).toEqual([
      {
        kind: 'list',
        ordered: true,
        items: [{ text: '甲', depth: 0, ordered: true }, { text: '乙', depth: 0, ordered: true }],
      },
    ])
  })

  it('reads a real nested list from a session, mixed markers and all', () => {
    // **原文**（从官壳宿主的会话历史里取出来的真实消息，空格按原文保留）。这份fixture 存在的
    // 理由很实在：之前两轮我照截图推断缩进，改了两版都没修对；真实输入一次就把问题说清了 ——
    // 每层缩进 2 空格、共 4 层、且同一棵树里混着 `1.` 与 `-`。
    const source = ['- 第一层', '  - 第二层', '    - 第三层', '      - 第四层', '1. 有序第一层', '   - 无序第二层', '     1. 有序第二层'].join('\n')
    const blocks = parseMarkdown(source)
    // 中间没有空行 ⇒ 整段是**一棵**列表：混着 `-` 与 `1.` 也仍是一棵树（这正是原文的样子，
    // 而每一项各自记着自己的标记）。
    const list = blocks[0]
    expect(list?.kind).toBe('list')
    if (list?.kind !== 'list') return
    expect(list.items.map((item) => [item.text, item.depth, item.ordered])).toEqual([
      ['第一层', 0, false],
      ['第二层', 1, false],
      ['第三层', 2, false],
      ['第四层', 3, false],
      ['有序第一层', 0, true],
      ['无序第二层', 1, false],
      ['有序第二层', 2, true],
    ])
  })

  it('keeps a list together across a blank line, so nested items stay nested', () => {
    // 模型常写"松散列表"（条目之间空一行）。空行后面的缩进子项必须仍属于同一个列表 —— 否则列表
    // 在空行处被截断，子项另成一个列表、层级被归一化回顶层，用户看到的就是"嵌套没生效"。
    expect(parseMarkdown('- 父\n\n  - 子\n- 另一个')).toEqual([
      {
        kind: 'list',
        ordered: false,
        items: [
          { text: '父', depth: 0, ordered: false },
          { text: '子', depth: 1, ordered: false },
          { text: '另一个', depth: 0, ordered: false },
        ],
      },
    ])
  })

  it('reads task items as a state, keeping the rest of the list intact', () => {
    // 原文里就有 `- [x]` / `- [ ]`。做成**只读**方框：这是模型说的话，不是待办应用，点不动，
    // 也就不会让用户以为"点了会发生什么"。
    expect(parseMarkdown('- [x] 已完成\n- [ ] 未完成\n- 普通项')).toEqual([
      {
        kind: 'list',
        ordered: false,
        items: [
          { text: '已完成', depth: 0, ordered: false, checked: true },
          { text: '未完成', depth: 0, ordered: false, checked: false },
          { text: '普通项', depth: 0, ordered: false },
        ],
      },
    ])
  })

  it('reads a strict table, and leaves everything else alone', () => {
    expect(parseMarkdown('| 名称 | 值 |\n| :--- | ---: |\n| a | 1 |\n| b | 2 |')).toEqual([
      {
        kind: 'table',
        align: ['left', 'right'],
        head: ['名称', '值'],
        rows: [['a', '1'], ['b', '2']],
      },
    ])
    // 列数不一致 ⇒ 不是表格，保持字面。
    expect(parseMarkdown('| a | b |\n| --- |\n| 1 | 2 |')).toEqual([
      { kind: 'paragraph', text: '| a | b |\n| --- |\n| 1 | 2 |' },
    ])
    // 缺分隔行 ⇒ 不是表格（正文里一个竖线不该被吃掉）。
    expect(parseMarkdown('| a | b |\n| 1 | 2 |')).toEqual([
      { kind: 'paragraph', text: '| a | b |\n| 1 | 2 |' },
    ])
  })

  it('reads quotes and rules', () => {
    expect(parseMarkdown('> 引用第一行\n> 继续\n\n---\n\n***')).toEqual([
      { kind: 'quote', text: '引用第一行\n继续' },
      { kind: 'rule' },
      { kind: 'rule' },
    ])
  })

  it('reads inline code, bold, italic and strike — and nests them', () => {
    expect(parseInline('前 `a**b**` 后')).toEqual([
      { kind: 'text', text: '前 ' },
      { kind: 'code', text: 'a**b**' },
      { kind: 'text', text: ' 后' },
    ])
    expect(parseInline('这是**重点**。')).toEqual([
      { kind: 'text', text: '这是' },
      { kind: 'bold', children: [{ kind: 'text', text: '重点' }] },
      { kind: 'text', text: '。' },
    ])
    // 模型真会写"粗体里含行内代码"，所以这里是递归而不是一层扫描。
    expect(parseInline('**加粗里含 `代码` 也行**')).toEqual([
      {
        kind: 'bold',
        children: [
          { kind: 'text', text: '加粗里含 ' },
          { kind: 'code', text: '代码' },
          { kind: 'text', text: ' 也行' },
        ],
      },
    ])
    expect(parseInline('*斜* 与 ~~删~~')).toEqual([
      { kind: 'italic', children: [{ kind: 'text', text: '斜' }] },
      { kind: 'text', text: ' 与 ' },
      { kind: 'strike', children: [{ kind: 'text', text: '删' }] },
    ])
  })

  it('leaves what it does not implement exactly as written', () => {
    // 刻意不做下划线斜体：`a_b_c` 里的下划线常常是标识符的一部分，认它会把 snake_case 撕开。
    expect(parseInline('snake_case_name')).toEqual([{ kind: 'text', text: 'snake_case_name' }])
    expect(parseMarkdown('## 标题\n\n<b>粗</b> 和 ![图](x.png)')).toEqual([
      { kind: 'paragraph', text: '## 标题' },
      { kind: 'paragraph', text: '<b>粗</b> 和 ![图](x.png)' },
    ])
    // `<b>` 只是文字片段，不是元素：这条路上不存在把文本当 HTML 渲染的分支。
    expect(parseInline('<b>粗</b>')).toEqual([{ kind: 'text', text: '<b>粗</b>' }])
  })
})
