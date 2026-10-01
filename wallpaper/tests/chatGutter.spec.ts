import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const stylesheet = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'src/features/chat/ConversationBubble.css')

/** 盒子（输入框所在那块玻璃）内部的行：它们共用同一条左右内边距。 */
const GUTTER_ROWS = [
  'dsh-chat__composer',
  'dsh-chat__topbar',
  'dsh-chat__footer',
  // FREEZE（临时冻结，不是删除）：用量串的 JSX 冻住了（见 `ConversationBubble.tsx` 里那处
  // FREEZE），但它的 CSS 规则**一行没删** —— 恢复时一起复活。所以它仍然算"盒子内部的一行"，
  // 这份清单不动：真删掉这一项，下面的 `checked >= 5` 也会跟着少一行。
  'dsh-chat__usage-rail',
  'dsh-chat__island-toolbar',
]

/** 简写里哪几个值是"左右"。单值那种写法只有 `0` 会出现在这些行上。 */
function horizontalValues(declaration: string): string[] {
  const [property, ...rest] = declaration.split(':')
  const value = rest.join(':').replace(/;.*$/, '').trim()
  if (!property || !value) return []
  if (/padding-(inline|left|right)$/.test(property.trim())) return [value]
  const parts = value.split(/\s+/)
  if (parts.length === 1) return []
  if (parts.length === 2 || parts.length === 3) return [parts[1]!]
  return [parts[1]!, parts[3]!]
}

function rules(css: string): Array<{ selector: string; body: string }> {
  // 注释要先剥掉：规则前面的说明会被 `[^{}]+` 一起吞进选择器里，于是精确匹配一个选择器永远找不到。
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, '')
  return [...stripped.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((match) => ({ selector: match[1]!.trim(), body: match[2]! }))
}

/**
 * 这一块的对齐目标有两层，别再混成一个数（混过一次，改错了对象）：
 *
 *  1. **轨道**的文字列 = 输入框那个盒子的内容盒（用户要求"粉色对齐黄色"）：轨道自己不吃内边距，
 *     也不靠"变窄 + 居中"来缩进。
 *  2. **盒子内部各行**（输入框、顶栏、底栏、用量行、工具条）共用同一条内边距 —— 它们曾经各写
 *     一个数字（21 / 22 / 18），长回答的左边缘与输入文字差 7px。
 */
describe('the island gutter', () => {
  it('keeps the boxes rows on one inset, defined once per mode', async () => {
    const css = await readFile(stylesheet, 'utf8')
    const definitions = [...css.matchAll(/--dsh-chat-gutter:\s*(\d+)px/g)].map((match) => match[1])
    // 默认（窄岛）/ 桌面岛 / 窄屏媒体查询
    expect(definitions).toEqual(['18', '22', '12'])

    const offenders: string[] = []
    let checked = 0
    for (const rule of rules(css)) {
      if (!GUTTER_ROWS.some((row) => rule.selector.includes(row))) continue
      for (const declaration of rule.body.split(';')) {
        if (!/^\s*padding/.test(declaration)) continue
        const values = horizontalValues(declaration)
        if (values.length === 0) continue
        checked += 1
        for (const value of values) {
          if (value !== 'var(--dsh-chat-gutter)') offenders.push(`${rule.selector} → ${declaration.trim()}`)
        }
      }
    }
    expect(offenders).toEqual([])
    // 五行都得真的带上这条内边距，而不是"没有硬编码"而已。
    expect(checked).toBeGreaterThanOrEqual(5)
  })

  it('lets the transcript column span the box instead of insetting it', async () => {
    const css = await readFile(stylesheet, 'utf8')
    const wrap = rules(css).find((rule) => rule.selector === '.dsh-chat__history-wrap')
    expect(wrap, '.dsh-chat__history-wrap rule').toBeTruthy()
    // 不吃内边距：内容盒 = 盒子的内容盒。
    expect(wrap!.body).toMatch(/padding:\s*0;/)
    // 也不再靠"变窄 + 居中"缩进 —— 那正是它比盒子窄 29px 的原因。
    expect(wrap!.body).toMatch(/width:\s*100%;/)
    expect(wrap!.body).not.toContain('clamp(')
    expect(wrap!.body).not.toContain('auto')
    // 轨道内层只剩上下内边距。
    const history = rules(css).find((rule) => rule.selector === '.dsh-chat__history')
    expect(history!.body).toMatch(/padding:\s*10px 0 20px;/)
    // 窄屏也不再给轨道单独叠一层。
    expect(css).not.toContain('padding-inline: 15px')
  })
})
