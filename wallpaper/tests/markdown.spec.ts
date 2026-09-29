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

  it('makes a heading only when the hashes end at a space', () => {
    // 两端的回答里都会出现标题：API 端直接发 `##`，网页端由 <h2> 还原而来。不支持它，用户看到
    // 的就是字面的 `## Markdown 综合示例`。
    expect(parseMarkdown('## Markdown 综合示例')).toEqual([
      { kind: 'heading', level: 2, text: 'Markdown 综合示例' },
    ])
    expect(parseMarkdown('# 一\n###### 六')).toEqual([
      { kind: 'heading', level: 1, text: '一' },
      { kind: 'heading', level: 6, text: '六' },
    ])
    // 标题里的行内标记照旧交给行内解析，块本身只记层级和文本。
    expect(parseMarkdown('### **加粗**的标题')).toEqual([
      { kind: 'heading', level: 3, text: '**加粗**的标题' },
    ])
    // 没有空格、或超过六个井号，都保持字面：`#标签` 是标签，`#######` 只是七个井号。
    expect(parseMarkdown('#标签')).toEqual([{ kind: 'paragraph', text: '#标签' }])
    expect(parseMarkdown('####### 七个')).toEqual([{ kind: 'paragraph', text: '####### 七个' }])
    // 代码块里的 `#` 是注释，不是标题 —— 围栏那一支已经把整块吃掉了。
    expect(parseMarkdown('```sh\n# 注释\necho hi\n```')).toEqual([
      { kind: 'code', language: 'sh', text: '# 注释\necho hi' },
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
    // 标题不再是"没实现"的那一类（2026-09-28 起支持，见上面的用例）；这里留下的是链接与图片：
    // 它们要么变成可点的东西（要接原生打开 + 命中区域），要么就保持字面，不做半吊子。
    expect(parseMarkdown('<b>粗</b> 和 ![图](x.png)')).toEqual([
      { kind: 'paragraph', text: '<b>粗</b> 和 ![图](x.png)' },
    ])
    // `<b>` 只是文字片段，不是元素：这条路上不存在把文本当 HTML 渲染的分支。
    expect(parseInline('<b>粗</b>')).toEqual([{ kind: 'text', text: '<b>粗</b>' }])
  })
})
