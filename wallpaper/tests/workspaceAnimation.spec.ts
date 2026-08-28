import { readFile } from 'node:fs/promises'
import { describe, expect, it } from 'vitest'

const stylesheet = new URL('../src/styles.css', import.meta.url)

describe('inner workspace conversation animation', () => {
  it('keeps the conversation shell centered through every transform keyframe', async () => {
    const css = await readFile(stylesheet, 'utf8')
    const focusIn = css.match(/@keyframes workspace-focus-in\s*\{([^]*?)\n\}/)?.[1] ?? ''
    const focusOut = css.match(/@keyframes workspace-focus-out\s*\{([^]*?)\n\}/)?.[1] ?? ''

    expect(focusIn).toContain('translateX(-50%) scale(.94)')
    expect(focusIn).toContain('translateX(-50%) scale(1)')
    expect(focusOut).toContain('translateX(-50%) scale(.97)')
  })
})
