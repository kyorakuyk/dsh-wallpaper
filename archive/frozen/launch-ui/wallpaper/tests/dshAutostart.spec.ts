// 归档测试（B6，2026-10）：原 `wallpaper/tests/dshAutostart.spec.ts` 中随「启动参数」冻结而 skip 的那一条（原 186-206 行）。
// 来源 commit 3c92772（tag pre-freeze-isolation）；行号指该 commit 中的原文件（B6 之前这几行没有变过）。
// 用例本身逐字保留（包括 `it.skip` 与原来写在上面的冻结说明）；这里只补上它要用的 import、`source` 帮手与外层
// `describe('DSH autostart wiring')`。import 路径按"放回 `wallpaper/tests/`"写，所以留在归档里时 IDE 报未解析属预期。
// Vitest 的发现根是 `wallpaper/`，这个文件不会被运行。恢复办法见同目录上两级的 README.md。

import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

describe('DSH autostart wiring', () => {
  // 冻结（与「启动参数」一起）：它钉的是"自动启动这条路上带着参数"（`App.tsx` 里那行
  // `args: parseLaunchArgs(settings.dshLaunch.args)`）以及界面上「启动参数」那个控件 —— 两者都
  // 随这次冻结被注释掉了（参数不再上路，"控件在不在"由注释决定）。`launch-args-invalid` 那句话
  // 本身仍然钉在上面那两条报告类测试里；"启动参数不再上路"改由 launchArgsAndInstances.spec.ts
  // 里那一组「启动参数冻结之后：没有参数流出去」钉住。
  it.skip('gives the automatic path the same launcher and the same args as the button', async () => {
    const app = await source('src/App.tsx')
    // 「启动参数」在两条路上一视同仁：加的是参数，跑的是谁由本应用决定，所以这里不再需要
    // 任何"要不要授权"的字段。
    expect(app).toContain('args: parseLaunchArgs(settings.dshLaunch.args)')
    const panel = await source('src/settings/SettingsPanel.tsx')
    // 那个授权复选框与它那一行随「启动命令」一起消失；界面上不该再留一个不生效的开关。
    expect(panel).not.toContain('trustedCommandForAutoStart')
    expect(panel).not.toContain('自动启动不使用自定义启动命令')
    expect(panel).not.toContain('允许自动启动使用该命令')
    // 而**控件本身**也换了：没有「启动命令」这个输入框了，只有「启动参数」。
    // （注释里还会出现"启动命令"三个字，那是说明为什么它没了 —— 所以这里钉的是字段标题。）
    expect(panel).not.toContain('title="启动命令"')
    expect(panel).toContain('title="启动参数"')
    expect(panel).toContain('aria-label="启动参数"')
  })
})
