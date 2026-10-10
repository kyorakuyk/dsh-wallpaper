// 归档测试（B6，2026-10）：原 `wallpaper/tests/launchArgsAndInstances.spec.ts` 中随「启动参数」冻结而 skip 的那一条（原 165-173 行）。
// 来源 commit 3c92772（tag pre-freeze-isolation）；行号指该 commit 中的原文件（B6 之前这几行没有变过）。
// 用例本身逐字保留（包括 `it.skip` 与原来写在上面的冻结说明）；这里只补上它们要用的 import、`source` 帮手与外层
// `describe`（标题与原文件相同）。import 路径按"放回 `wallpaper/tests/`"写，所以留在归档里时 IDE 报未解析属预期。
// Vitest 的发现根是 `wallpaper/`，这个文件不会被运行。恢复办法见同目录上两级的 README.md。

import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

describe('端口从设置一路流到「打开界面」', () => {
  // 冻结（与「启动参数」一起）：它钉的是"改参数就清掉显式端口 pin"那个 handler 的**源码**，
  // 而那个 handler 现在整块被注释掉了（输入控件也不在界面上）。
  it.skip('is cleared of the explicit pin when the args change, so the browser cannot chase a dead port', async () => {
    // 与"换主体就清 pin"是同一条理由：那条 pin 是上一次启动选的那个端口，参数已经把它推翻了。
    const window = await source('src/settings/SettingsWindow.tsx')
    const handler = window.slice(window.indexOf('onSelectLaunchArgs={(value) =>'), window.indexOf('onOpenTui='))
    expect(handler).toContain('endpointPort: undefined')
    expect(handler).toContain('args,')
  })
})
