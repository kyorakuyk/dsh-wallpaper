import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { beforeAll, describe, expect, it } from 'vitest'
import { reachNeedsBrowser } from '../src/connect/harnessSubjects.ts'

// App.tsx evaluates its browser-preview fallback when it is imported, so the module
// needs the same minimal `window` the other pure-logic suites provide.
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/**
 * 「拉起可视化窗口」的规则，与它被实测咬过的两处一起钉住。
 *
 * 这不是理论问题：用户实测"点了那个图标，窗口没有起来"。两个原因都在这里 —— 一是图标在
 * 网页模式下根本不是按钮，二是"要不要改用浏览器"是**猜**出来的，而在什么都没配置时没有形态
 * 可猜，猜出来的"无窗口"让浏览器抢在窗口之前被打开。
 */
describe('raising a subject\u2019s own interface', () => {
  it('asks for the browser only when native found no window', () => {
    // 原生说"这个端口上没有可拉起的窗口"——那才是唯一需要浏览器的情形。
    expect(reachNeedsBrowser('no-window')).toBe(true)
    // `raise-refused` 是成功：窗口被恢复了，只是 Windows 拒绝了前台切换。
    expect(reachNeedsBrowser('raise-refused')).toBe(false)
    expect(reachNeedsBrowser('raised')).toBe(false)
    // 主体不在（无论是否刚被启动）：要报给用户，不是悄悄开个浏览器。
    expect(reachNeedsBrowser('not-running')).toBe(false)
    expect(reachNeedsBrowser('unknown-target')).toBe(false)
  })

  it('keeps the icon a working shortcut in every backend mode', async () => {
    const app = await source('src/App.tsx')
    // 这次报的就是这一条：只在 Harness 模式下接上回调，于是网页模式里它是一枚纯图标
    // ——点下去什么都不发生，看起来就是"按钮拉不起来窗口"。
    expect(app).toContain('onRaiseClientWindow={openSubjectInterface}')
    expect(app).not.toMatch(/onRaiseClientWindow=\{[^}]*runtime\.backend/)
    // 而且它和"当前后端"无关：`openSubjectInterface` 自己不看后端。
    const handler = app.slice(app.indexOf('const openSubjectInterface'), app.indexOf('const selectedModel'))
    expect(handler).not.toContain('runtime.backend')
  })

  it('decides the browser fallback from what native did, in both surfaces', async () => {
    for (const file of ['src/App.tsx', 'src/settings/SettingsWindow.tsx']) {
      const text = await source(file)
      expect(text, file).toContain('reachNeedsBrowser(ensured.outcome)')
      // 旧的猜法必须消失：拿主体形态去判断"要不要开浏览器"正是让窗口永远等不到请求的原因。
      expect(text, file).not.toMatch(/clientRaiseAction\([^)]*\)\s*===\s*'browser'/)
    }
  })
})
