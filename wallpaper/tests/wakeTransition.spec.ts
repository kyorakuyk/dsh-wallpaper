import { readFile, stat } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/**
 * The wake hand-off touches three files that have to agree: the frame list in
 * the scene, the two per-edition asset lists in the Vite config, and the
 * stylesheet that draws the curtain and the portrait ghost. A rename that
 * updates only one of them is invisible until an unlock shows a black screen.
 */
describe('wake hand-off assets and transition', () => {
  it('lists wake frames that exist on disk', async () => {
    const source = await readFile(resolve(wallpaperRoot, 'src/scenes/WakeScene.tsx'), 'utf8')
    const frames = [...source.matchAll(/assetUrl\('personas\/([^']+)'\)/g)].map((match) => match[1])
    expect(frames.length).toBeGreaterThanOrEqual(4)
    for (const frame of frames) {
      await expect(stat(resolve(wallpaperRoot, 'public/personas', frame)), frame).resolves.toBeTruthy()
    }
  })

  it('keeps every wake frame in both editions asset lists', async () => {
    const vite = await readFile(resolve(wallpaperRoot, 'vite.config.ts'), 'utf8')
    for (const frame of ['sleep.png', 'frame-2-eyes.png', 'frame-3-yawn.webp', 'frame-4-awake.webp']) {
      // Once in `fullRuntimeAssets`, once in `liteRuntimeAssets`.
      expect(vite.split(frame).length - 1, frame).toBe(2)
    }
    // The two frames the native layer decodes stay PNG: the lock-screen image
    // and the first wake frame go through WIC/Windows, which has no guaranteed
    // WebP decoder.
    expect(vite).not.toContain('frame-2-eyes.webp')
  })

  it('draws the curtain, the layering and the portrait ghost', async () => {
    // The sheet travels with WakeScene because the Lite edition never loads
    // styles.css: put these rules there and Lite gets an unstyled black div.
    const css = await readFile(resolve(wallpaperRoot, 'src/scenes/wakeTransition.css'), 'utf8')
    const scene = await readFile(resolve(wallpaperRoot, 'src/scenes/WakeScene.tsx'), 'utf8')
    expect(scene).toContain("import './wakeTransition.css'")
    expect(css).toContain('.wake-curtain {')
    expect(css).toContain('.wake-curtain-out {')
    expect(css).toContain('@keyframes wake-curtain-in')
    expect(css).toContain('@keyframes wake-curtain-out')
    expect(css).toContain('.wake-enter .portrait-slot')
    expect(css).toContain('@keyframes portrait-from-ghost')
    // Reduced motion must not leave the user staring at a black screen.
    expect(css).toMatch(/prefers-reduced-motion[\s\S]*?\.wake-curtain-out \{ animation-duration: 1ms/)
  })

  it('keeps the animation above the desktop interaction layer while it plays', async () => {
    // The reported bug: during the frame animation the input island painted over
    // the frames. The rule needs both the sheet and the phase class on the root,
    // so a missing half has to fail here rather than on screen.
    const css = await readFile(resolve(wallpaperRoot, 'src/scenes/wakeTransition.css'), 'utf8')
    for (const phase of ['phase-booting', 'phase-locked', 'phase-waking']) {
      expect(css, phase).toContain(`.wallpaper-root.${phase} .scene`)
      expect(css, phase).toContain(`.lite-wallpaper-root.lite-${phase} .scene`)
    }
    expect(css).toMatch(/\.wallpaper-root\.phase-waking \.scene,[\s\S]*?z-index: 100;/)
    // ...and the desktop itself must stay out of that rule: on idle the island
    // belongs on top.
    expect(css).not.toContain('.phase-idle .scene')
    const app = await readFile(resolve(wallpaperRoot, 'src/App.tsx'), 'utf8')
    expect(app).toContain('phase-${runtime.phase}')
    const lite = await readFile(resolve(wallpaperRoot, 'src/lite/LiteApp.tsx'), 'utf8')
    expect(lite).toContain('lite-phase-${phase}')
  })

  it('only plays the hand-off when the animation itself played', async () => {
    for (const file of ['src/App.tsx', 'src/lite/LiteApp.tsx']) {
      const source = await readFile(resolve(wallpaperRoot, file), 'utf8')
      expect(source).toContain('wake-enter')
      expect(source).toContain('wake-curtain-out')
      expect(source).toMatch(/animationsEnabled && !settings(Ref\.current)?\.skipWakeAnimation\) setWakeEnter\(true\)/)
    }
  })
})
