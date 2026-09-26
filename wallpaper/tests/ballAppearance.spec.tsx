import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { BallWindow } from '../src/floating/BallWindow.tsx'
import { iconPaths } from '../src/ui/primitives/Icon.tsx'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/** 只取一条规则的花括号内文，用来断言"这条规则里没有画材质"。 */
function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`)
  expect(start, `找不到规则 ${selector}`).toBeGreaterThanOrEqual(0)
  return css.slice(start, css.indexOf('}', start))
}

/** 去掉注释：注释里会**提到**被禁止的属性名（"不用 backdrop-filter"），那不是声明。 */
function withoutComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '')
}

/**
 * 路径上**真正画出来**的点到 (28,28) 的最大距离。
 *
 * 只取曲线上的点（`M x y` 的起点，以及每段 `C c1x c1y c2x c2y x y` 的落点）：贝塞尔控制点
 * 可以落在曲线之外，拿它们量"有没有画到窗口边缘"会量出一个并不存在的越界。
 */
function maxDrawnRadius(path: string, centre = 28): number {
  const numbers = path.match(/-?\d+(?:\.\d+)?/g)?.map(Number) ?? []
  const onCurve: Array<[number, number]> = [[numbers[0]!, numbers[1]!]]
  for (let index = 2; index + 5 < numbers.length; index += 6) {
    onCurve.push([numbers[index + 4]!, numbers[index + 5]!])
  }
  return onCurve.reduce(
    (furthest, [x, y]) => Math.max(furthest, Math.hypot(x - centre, y - centre)),
    0,
  )
}

/**
 * 悬浮球的观感：**玻璃碟 + 黑笔圈 + 黑笔"消息"**。
 *
 * 用户的要求一路收窄到这里：球是表桌面上唯一的入口，一枚没有语义的圆球只能靠猜（所以要
 * "消息"图标）；但上一版扁平实色圆**露出了原生区域的毛边**（"有一些毛边"），于是改回平面玻璃风、
 * 黑笔圈、黑笔字，并明确"这次不要透镜效应"。
 */
describe('the floating ball', () => {
  it('shows an inked message glyph instead of an empty circle', async () => {
    const html = renderToStaticMarkup(<BallWindow />)
    // 按钮本身仍有可访问名字（无障碍树里也是它）。
    expect(html).toContain('aria-label="打开 AI 输入岛"')
    expect(html).toContain('title="打开 AI 输入岛"')
    expect(html).toContain('viewBox="0 0 56 56"')
    // 字形复用设计系统那一份路径，而不是在这里另抄一个 `d`。
    expect(html).toContain(iconPaths.message)
    expect(html).toContain('ball__message')
  })

  it('keeps every ink stroke away from the window edge', async () => {
    // **这条是"毛边"的回归钉子**：原生 `SetWindowRgn` 的圆形区域是 1bit 掩码、没有抗锯齿，
    // 图形一旦画到窗口边缘，那道阶梯硬边就会露出来。56 的 viewBox 映射整个 56 逻辑像素窗口，
    // 所以"最大半径 ≤ 26"= 四周至少留 2px 透明边距 = 区域只裁得到透明像素。
    const { BALL_GLASS_PATH, BALL_RING_PATH } = await import('../src/floating/BallWindow.tsx')
    expect(maxDrawnRadius(BALL_GLASS_PATH)).toBeLessThanOrEqual(26)
    expect(maxDrawnRadius(BALL_RING_PATH)).toBeLessThanOrEqual(26)
    // 而且不是"贴着 26 差一点点"：确实留出了边距（真圆半径会是 28）。
    expect(maxDrawnRadius(BALL_RING_PATH)).toBeLessThan(27)
  })

  it('is flat glass with an ink ring — no lens, no blur, no glow', async () => {
    const css = withoutComments(await source('src/floating/BallWindow.css'))
    // 窗口底面不画东西：可见的一切都来自 SVG（并列的图形会让毛边重新出现）。
    const ball = ruleBody(css, '.ball')
    expect(ball).toContain('background: transparent')
    expect(ball).toContain('border: 0')
    // 墨与玻璃各一份定义。
    expect(ball).toMatch(/--ball-ink:\s*#0b0d10/)
    expect(ball).toContain('--ball-glass:')
    // **玻璃必须糊得够实**：球不能 `backdrop-filter`（独立顶层窗口没有可采样的底），
    // 所以背后那个窗口的标题栏只能靠不透明度挡住——用户实测报过它从球身里透出来。
    const glassAlpha = Number(ball.match(/--ball-glass:\s*rgba\([^)]*?,\s*(0?\.\d+|1)\)/)![1])
    expect(glassAlpha, '玻璃不透明度低于 0.8 时，背后的标题栏会重新读得出来').toBeGreaterThanOrEqual(0.8)
    // 平面玻璃 = 一层均匀的半透明色；透镜效应来自渐变/发光，这里一个都不许有。
    expect(css).not.toContain('gradient')
    expect(css).not.toContain('backdrop-filter')
    expect(css).not.toContain('box-shadow')
    // 墨圈是实心填充 + evenodd 的两条轮廓；墨字是黑笔描边。
    expect(ruleBody(css, '.ball__ring')).toContain('fill: var(--ball-ink)')
    expect(ruleBody(css, '.ball__message')).toContain('stroke: var(--ball-ink)')
    // 反馈保留，但只落在位移/亮度上。
    expect(ruleBody(css, '.ball:hover')).toContain('transform')
    expect(ruleBody(css, '.ball:active')).toContain('transform')
  })

  it('draws the ring as one closed hand-drawn loop, not a perfect circle', async () => {
    const markup = renderToStaticMarkup(<BallWindow />)
    // 墨圈是两条闭合轮廓（外 + 内）填成的实心圈，并交给 evenodd。
    expect(markup).toContain('fill-rule="evenodd"')
    expect(markup).toContain('ball__ring')
    // 手绘感：轮廓点在半径上有抖动，所以同一圈上不同方向的半径不相等。
    const { BALL_RING_PATH } = await import('../src/floating/BallWindow.tsx')
    const outer = BALL_RING_PATH.slice(0, BALL_RING_PATH.indexOf('M50.20'))
    const numbers = outer.match(/-?\d+(?:\.\d+)?/g)?.map(Number) ?? []
    const radii: number[] = []
    for (let index = 0; index + 1 < numbers.length; index += 2) {
      radii.push(Math.hypot(numbers[index]! - 28, numbers[index + 1]! - 28))
    }
    const distinct = new Set(radii.map((radius) => radius.toFixed(1))).size
    expect(distinct).toBeGreaterThan(3)
  })
})
