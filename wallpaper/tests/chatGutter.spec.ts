import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const stylesheet = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'src/features/chat/ConversationBubble.css')

/** 与输入框、轨道共享同一条边线的那些行。 */
const GUTTER_ROWS = [
  'dsh-chat__history-wrap',
  'dsh-chat__composer',
  'dsh-chat__topbar',
  'dsh-chat__footer',
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

/**
 * 轨道与输入框曾经各写一个左右内缩：轨道靠宽度 clamp 缩进 29px，输入框自己缩进 21px，长回答的
 * 左边缘和输入文字差 7px。现在四行共用一个变量，这条测试就是防止数字再长回去 —— 靠眼睛发现
 * 差几像素是发现不了的。
 */
describe('the island gutter', () => {
  it('defines the gutter once per mode and nowhere else', async () => {
    const css = await readFile(stylesheet, 'utf8')
    const definitions = [...css.matchAll(/--dsh-chat-gutter:\s*(\d+)px/g)].map((match) => match[1])
    // 默认（窄岛）/ 桌面岛 / 窄屏媒体查询
    expect(definitions).toEqual(['18', '22', '12'])
    expect(css).toContain('.dsh-chat { --dsh-chat-gutter: 18px; }')
    expect(css).toContain("--dsh-chat-gutter: 22px;")
    expect(css).toContain('.conversation-shell.dsh-chat { --dsh-chat-gutter: 12px;')
  })

  it('puts every one of those rows on that single inset', async () => {
    const css = await readFile(stylesheet, 'utf8')
    const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((match) => ({ selector: match[1]!.trim(), body: match[2]! }))
    const offenders: string[] = []
    let checked = 0
    for (const rule of rules) {
      if (!GUTTER_ROWS.some((row) => rule.selector.includes(row))) continue
      for (const declaration of rule.body.split(';')) {
        if (!/^\s*padding/.test(declaration)) continue
        const values = horizontalValues(declaration)
        if (values.length === 0) continue
        checked += 1
        for (const value of values) {
          if (value !== 'var(--dsh-chat-gutter)') {
            offenders.push(`${rule.selector} → ${declaration.trim()}`)
          }
        }
      }
    }
    expect(offenders).toEqual([])
    // 四行都得真的带上这条内缩，而不是"没有硬编码"而已（把声明删掉也能通过上面那条）。
    expect(checked).toBeGreaterThanOrEqual(6)
    for (const row of GUTTER_ROWS) {
      expect(css, row).toContain(`var(--dsh-chat-gutter)`)
    }
  })
})
