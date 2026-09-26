import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { shouldSuppressContextMenu } from '../src/runtime/contextMenu.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/** A target that answers `closest` the way a real element would for one selector set. */
function elementIn(inside: string | null) {
  return { closest: (selector: string) => (selector === inside ? {} : null) }
}

/**
 * 里桌面的"外壳"细节：右键、关闭按钮、轨道材质。三件都是实测报回来的问题，各自留一条钉子。
 */
describe('the inner desktop\u2019s chrome', () => {
  it('freezes the native context menu, except in text entry', () => {
    // 背景插画上的右键会弹出"图像另存为 / 复制图像链接 / 更多工具"——用户要求冻结它。
    expect(shouldSuppressContextMenu(elementIn(null))).toBe(true)
    // 但输入框里的复制/粘贴是真有用的，一起关掉是在惩罚用户。
    expect(shouldSuppressContextMenu(elementIn('input, textarea, [contenteditable="true"]'))).toBe(false)
    // 目标不可判断时按"拦"处理：壁纸上没有别处需要那个菜单。
    expect(shouldSuppressContextMenu(undefined)).toBe(true)
    expect(shouldSuppressContextMenu({})).toBe(true)
  })

  it('installs that rule on the wallpaper surfaces, not in the settings window', async () => {
    for (const file of ['src/App.tsx', 'src/floating/BallWindow.tsx']) {
      expect(await source(file), file).toContain('suppressNativeContextMenu()')
    }
    // 设置中心是普通应用窗口，不该被顺手改掉。
    expect(await source('src/settings/SettingsWindow.tsx')).not.toContain('suppressNativeContextMenu')
  })

  it('closes the island out of the inner workspace through native', async () => {
    const app = await source('src/App.tsx')
    // 实测报的原文："右上角的 X 还是没有任何作用"。它当时是 `() => undefined`：按钮画出来了，
    // 点下去什么都不发生。
    expect(app).not.toMatch(/onClose=\{\(\) => undefined\}/)
    // 而且必须落回**原生**：前端的界面迁移不会改原生的"是否在里桌面"，实测后果是点完 X 之后
    // 悬浮球再也弹不出来、点球也唤不起输入岛。
    expect(app).toContain('nativeRuntime.leaveInnerWorkspace()')
    // 表桌面上没有"离开"可言（原生那边不在里桌面、也不会发事件），那里就是收回胶囊。
    expect(app).toMatch(/workspace === 'front'\)\s*\{\s*leaveInnerWorkspace\(\)/)
    // 原生那一条命令必须存在，并授予壁纸宿主这一侧。
    expect(await source('src/native/runtime.ts')).toContain("invoke('leave_inner_workspace')")
    const cargo = await source('src-tauri/build.rs')
    expect(cargo).toContain('"leave_inner_workspace"')
    const capability = await source('src-tauri/capabilities/background.json')
    expect(capability).toContain('allow-leave-inner-workspace')
  })

  it('re-asserts the ball\u2019s borderless frame every time it pops out', async () => {
    const ball = await source('src-tauri/src/floating_ball.rs')
    // 实测报的原文："单击悬浮球之后球内部会出现状态栏（有最小化、恢复和X）"——标题栏回来了，
    // 和当年悬浮球/输入岛是同一类问题。样式只在创建时断言一次不够：任何一条让窗口重新带上
    // 非客户区的路径，用户看到的就是一个带标题栏的球。
    const monitor = ball.slice(ball.indexOf('pub fn start_ball_monitor'))
    const assertions = monitor.match(/apply_ball_ex_style\(hwnd\)/g) ?? []
    expect(assertions.length).toBeGreaterThanOrEqual(2)
  })

  it('gives the docked track the same transparent material as the floating one', async () => {
    const css = await source('src/features/chat/ConversationBubble.css')
    const base = css.slice(css.indexOf('.dsh-chat__history-wrap {'), css.indexOf('.dsh-chat__history {'))
    // 一份定义：透明、无边框、无模糊、无阴影。
    expect(base).toContain('background: transparent')
    expect(base).toContain('border: 0')
    expect(base).toContain('backdrop-filter: none')
    expect(base).toContain('box-shadow: none')
    // 悬浮布局不再重复材质与几何，只补它自己的层级差异——否则就是两份定义，迟早再次分叉。
    const persistent = css.slice(css.indexOf(".dsh-chat[data-persistent='true'] .dsh-chat__history-wrap"))
    const persistentRule = persistent.slice(0, persistent.indexOf('}'))
    expect(persistentRule).not.toContain('background')
    expect(persistentRule).not.toContain('width')
    expect(persistentRule).toContain('z-index: 1')
  })
})
