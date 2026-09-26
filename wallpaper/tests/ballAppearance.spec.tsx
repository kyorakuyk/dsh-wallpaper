import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { BallWindow } from '../src/floating/BallWindow.tsx'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/** 只取一条规则的花括号内文，用来断言"这条规则里没有画材质"。 */
function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`)
  expect(start, `找不到规则 ${selector}`).toBeGreaterThanOrEqual(0)
  return css.slice(start, css.indexOf('}', start))
}

/**
 * 悬浮球的观感：**扁平的圆形"消息"按钮**。
 *
 * 用户本轮的原话："把玻璃感变成扁平的圆形图标，内嵌『消息』的 icon，可能更容易让用户明白
 * 这个小球是干什么的"。这是一个**语义**上的要求，不只是配色：球是表桌面上唯一的入口，
 * 一枚没有语义的圆球只能靠猜。
 */
describe('the floating ball', () => {
  it('shows a message glyph instead of an empty circle', async () => {
    const html = renderToStaticMarkup(<BallWindow />)
    // 按钮本身仍有可访问名字（无障碍树里也是它）。
    expect(html).toContain('aria-label="打开 AI 输入岛"')
    expect(html).toContain('title="打开 AI 输入岛"')
    // 图标真的画出来了：以前这里是一个空按钮。
    expect(html).toContain('<svg')
    expect(html).toContain('viewBox="0 0 24 24"')
    // 而且用的是"消息"那一枚（源码里指名，避免换成别的图标也照样通过）。
    expect(await source('src/floating/BallWindow.tsx')).toContain('<Icon name="message"')
    expect(await source('src/ui/primitives/Icon.tsx')).toContain('message:')
  })

  it('is flat: one solid fill and one glyph, no glass', async () => {
    const css = await source('src/floating/BallWindow.css')
    const ball = ruleBody(css, '.ball')
    // 扁平 = 实色 + 字形：没有渐变、没有模糊、没有内外发光/阴影。
    expect(ball).toContain('background: var(--dsh-accent')
    expect(ball).not.toContain('gradient')
    expect(ball).not.toContain('backdrop-filter')
    expect(ball).not.toContain('box-shadow')
    // 字形用能压在实色 accent 上的墨色（浅色字压亮蓝/亮红对比度太低）。
    expect(ball).toContain('--dsh-accent-ink')
    // 原来那层"内部呼吸光"属于玻璃质感，已随扁平化去掉。
    expect(css).not.toContain('ball::after')
    expect(css).not.toContain('ball-breathe')
    // 反馈保留，但只落在颜色与位移上。
    expect(ruleBody(css, '.ball:hover')).toContain('filter')
    expect(ruleBody(css, '.ball:active')).toContain('transform')
  })

  it('keeps the glyph readable on either theme accent', async () => {
    const tokens = await source('src/ui/tokens/tokens.css')
    // 深色墨色对两种主题色都够：浅蓝 #5caeff 与哈内斯红 #e54f5e。
    expect(tokens).toMatch(/--dsh-accent-ink:\s*#07121f/)
    // 强制色彩模式下不能用字面色：系统只认 Highlight / HighlightText。
    expect(tokens).toMatch(/--dsh-accent-ink:\s*HighlightText/)
  })
})
