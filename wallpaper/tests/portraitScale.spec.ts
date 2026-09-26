import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { PORTRAIT_ADULT_SCALE, portraitAgeScale } from '../src/persona/portraitScale.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/**
 * 立绘的体型适配。
 *
 * 用户实测："幼态与成年态的立绘高度一致，但是身体比例实打实的不一样"——同高渲染之下，幼态是
 * 大头短身、成年态是长腿细腰，成年态的头明显偏小，两个形态不像同一个角色。按用户提议把成年态
 * **等比放大**，数值来自实测（同高之下成年态头高 138px vs 幼态 151px）而不是手感。
 */
describe('portrait body-proportion matching', () => {
  it('scales the official adult portraits by the measured factor', () => {
    expect(PORTRAIT_ADULT_SCALE).toBeGreaterThan(1)
    // 138 × 1.09 ≈ 150 ≈ 151：放大后两头一样大。
    expect(138 * PORTRAIT_ADULT_SCALE).toBeGreaterThan(149)
    expect(138 * PORTRAIT_ADULT_SCALE).toBeLessThan(152)

    // 只有内置的两套成年立绘被放大。
    expect(portraitAgeScale({ id: 'blue-adult' })).toBe(PORTRAIT_ADULT_SCALE)
    expect(portraitAgeScale({ id: 'black-adult' })).toBe(PORTRAIT_ADULT_SCALE)
    // 幼态不动：它本来就是基准。
    expect(portraitAgeScale({ id: 'blue-child' })).toBe(1)
    expect(portraitAgeScale({ id: 'black-child' })).toBe(1)
    // 用户导入的 JPG 立绘（`.portrait-asset`，构图自定）一律不动。
    expect(portraitAgeScale({ id: 'custom' })).toBe(1)
    expect(portraitAgeScale({ id: 'whatever' })).toBe(1)
  })

  it('applies it in both scenes, as the variable the stylesheet reads', async () => {
    for (const file of ['src/scenes/IdleScene.tsx', 'src/scenes/MultiScreenIdleScene.tsx']) {
      const scene = await source(file)
      expect(scene).toContain("import { portraitAgeScale } from '../persona/portraitScale.ts'")
      expect(scene).toContain("['--portrait-age-scale' as string]: String(portraitAgeScale(persona))")
    }
  })

  it('scales the whole slot from the bottom, so the hot zone grows with the figure', async () => {
    const css = await source('src/styles.css')

    // 缩放加在槽位上：它的 `getBoundingClientRect`（多屏那份热区就是它）跟着长大，
    // 于是放大后"点头顶也能点到"。立绘与氛围层都在槽里，因此必然同进同退，不会错位。
    expect(css).toMatch(/\.portrait-slot \{[\s\S]{0,900}?--portrait-age-scale: 1;[\s\S]{0,200}?transform: scale\(var\(--portrait-age-scale\)\);[\s\S]{0,120}?transform-origin: bottom center;/)
  })

  it('composes the hover lift with the scale instead of replacing it', async () => {
    const css = await source('src/styles.css')

    // 只写 `scale(1.02)` 会让成年态一悬停就丢掉放大，画面会跳。
    expect(css).toMatch(/\.portrait-slot:hover \{ transform: translateY\(-6px\) scale\(calc\(1\.02 \* var\(--portrait-age-scale\)\)\); \}/)
    // 旧的、不带系数的悬停规则必须已经删掉（留下它就会覆盖上面那条）。
    expect(css).not.toContain('transform: translateY(-6px) scale(1.02);')
    // Lite 那份也不能写 `transform: none`：那会连系数一起抹掉。
    expect(css).toContain('.lite-portrait-slot:hover { transform: scale(var(--portrait-age-scale)); }')
    expect(css).not.toContain('.lite-portrait-slot:hover { transform: none; }')
  })

  it('keeps the bubble the same size, only higher with the head', async () => {
    const css = await source('src/styles.css')

    // 气泡在槽里会被一起放大，但它是文字：反比缩放还原尺寸，只留下"随头顶抬高"，
    // 与头顶的间距因此不变。
    expect(css).toMatch(/\.portrait-slot > \.bubble \{[\s\S]{0,300}?transform: translateX\(-50%\) scale\(calc\(1 \/ var\(--portrait-age-scale\)\)\);[\s\S]{0,80}?transform-origin: bottom center;/)
  })
})
