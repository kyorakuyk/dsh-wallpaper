import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..')
const read = (relative: string) => readFile(resolve(repo, relative), 'utf8')

/**
 * 中央玻璃悬浮（`interactionLayout === 'floating'`）把输入岛固定在桌面上，悬浮球因此没有职责 ——
 * 用户实测报告过它在表桌面底下仍然弹出。
 *
 * 根因不是"判据想错了"，而是**窗口期**：球原本只看热区列表里有没有 `chat`，而岛在重挂载与相位
 * 切换时有几十毫秒不在列表里。日志（0.2.0.197 那次）：
 *
 *     interaction regions: island_visible=false region_count=0 revision=201 rects=[]
 *     floating ball: shown reason=approach cursor=1309,1448 …
 *
 * 修法是把"岛是否常驻"作为**状态**从渲染层报到原生，并让它参与球的判据。这条链子任何一段断掉，
 * bug 都会回来，所以在这里逐段钉住。
 */
describe('the pinned island keeps the floating ball away', () => {
  it('reports the layout as state instead of letting native infer it', async () => {
    const app = await read('wallpaper/src/App.tsx')
    // 上报点是**事件**：启动一次、设置变更一次。刻意不用以偏好为依赖的 effect ——
    // tests/interactionLayout.spec.ts 禁止那种写法（偏好不该能重新触发"这次会话刚开始"）。
    expect(app).toMatch(/reportIslandPinned\(settingsRef\.current\.interactionLayout\)/)
    expect(app).toMatch(/if \(next\.interactionLayout !== settingsRef\.current\.interactionLayout\) \{\s*\n\s*reportIslandPinned\(next\.interactionLayout\)/)
    expect(app).toContain("dispatchCore('set-island-pinned', { value: layout === 'floating' ? 'true' : 'false' })")

    const client = await read('wallpaper/src/runtime/appCoreClient.ts')
    expect(client).toContain("| 'set-island-pinned'")

    const snapshot = await read('wallpaper/src/runtime/appSnapshot.ts')
    expect(snapshot).toContain('islandPinned: boolean')
    expect(snapshot).toContain('islandPinned: false')
  })

  it('carries the flag through the native core and into the ball decision', async () => {
    const core = await read('wallpaper/src-tauri/src/app_core.rs')
    expect(core).toContain('SetIslandPinned(bool)')
    expect(core).toContain('AppAction::SetIslandPinned(pinned) => state.island_pinned = pinned')
    expect(core).toContain('pub island_pinned: bool')

    const lib = await read('wallpaper/src-tauri/src/lib.rs')
    expect(lib).toMatch(/"set-island-pinned" => AppAction::SetIslandPinned\(match value\.as_deref\(\)/)

    const ball = await read('wallpaper/src-tauri/src/floating_ball.rs')
    // 球从核心状态里读这个标记，并把它并进"岛在前面"的判据（而不是只信热区列表）。
    expect(ball).toContain('core.snapshot().island_pinned')
    expect(ball).toContain('let island_visible = island_pinned || regions_visible;')
    // 退场原因要能分辨：日志里"岛在前面"和"布局把岛钉住了"是两件事。
    expect(ball).toContain('"island-pinned"')
    expect(ball).toContain('staying hidden reason=island-pinned')
  })
})
