// 归档测试（B6，2026-10）：原 `wallpaper/tests/launchArgsAndInstances.spec.ts` 中随「起别名」与实例下拉冻结而 skip 的三条（原 217-227 行、267-297 行）。
// 来源 commit 3c92772（tag pre-freeze-isolation）；行号指该 commit 中的原文件（B6 之前这几行没有变过）。
// 用例本身逐字保留（包括 `it.skip` 与原来写在上面的冻结说明）；这里只补上它们要用的 import、`source` 帮手与外层
// `describe`（标题与原文件相同）。import 路径按"放回 `wallpaper/tests/`"写，所以留在归档里时 IDE 报未解析属预期。
// Vitest 的发现根是 `wallpaper/`，这个文件不会被运行。恢复办法见同目录上两级的 README.md。

import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { normalizeSettings } from '../src/settings/store.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

describe('别名与实例行', () => {
  // 冻结（与「起别名」一起）：前半段仍然是纯逻辑（别名表照常存储与读取 ✓），后半段钉的是
  // SettingsWindow 里"清空就删键"那段代码的源码 —— 而那段 handler 现在整块被注释掉了。
  it.skip('keeps one alias per subject, and drops a cleared one instead of storing an empty string', async () => {
    const settings = normalizeSettings({
      dshLaunch: { profile: 'desktop', aliases: { 'D:\\a': '主树', 'D:\\b': '旧树' } },
    })
    expect(settings.dshLaunch.aliases).toEqual({ 'D:\\a': '主树', 'D:\\b': '旧树' })
    // 界面上清空 ⇒ 删键（见 SettingsWindow 的处理），所以"没起别名"只有一种表示。
    const window = await source('src/settings/SettingsWindow.tsx')
    expect(window).toContain('else delete aliases[subjectId]')
  })
})

describe('实例下拉与「全部停止」的接线', () => {
  // 冻结（与实例下拉一起）：它钉的是标题右上角那个控件的**存在**（Card 的 action、每一行的 ×、
  // 「全部停止」），而那个控件现在整块被注释掉了 —— 底部「停止本应用启动的 DSH」那一行恢复了。
  it.skip('renders the dropdown in the card header, with 全部停止 beside it', async () => {
    const panel = await source('src/settings/SettingsPanel.tsx')
    // 用户画红框的位置就是卡片标题右上角，所以控件是 Card 的 action，不是又一行 Field。
    expect(panel).toContain('action={<RunningInstances')
    expect(panel).toContain('aria-label="当前已启动实例"')
    expect(panel).toContain('全部停止')
    // 每一行一个 ×，且那个 × 说的是"停这一个实例"，不是一个通用的关闭图标。
    expect(panel).toContain('aria-label={`停止实例 ${label}`}')
    expect(panel).toContain('onStopInstance(instance.instanceKey)')
    // 行文字就是 `别名 · 端口` 那一条规则渲染出来的。
    expect(panel).toContain('instanceLabel(instance.subjectId, instance.port, targets, aliases)')
  })

  // 冻结（与实例下拉一起）：它钉的正是"底部那个按钮**不在了**"（`not.toContain('onStopManagedDsh')`
  // 等三条断言），而冻结的一部分工作就是把它**恢复**回来 —— 那三条断言与被冻结的界面互相矛盾。
  it.skip('consolidated the two stop controls into one action', async () => {
    const panel = await source('src/settings/SettingsPanel.tsx')
    // 底部那个按钮已经不在了：它与「全部停止」是同一个动作，两个控件做同一件事用户就得猜区别。
    // （注释里还会提到那个旧按钮的名字，所以这里钉的是**控件**：那行 JSX 与那个 prop 都没了。）
    expect(panel).not.toContain('onStopManagedDsh')
    expect(panel).not.toContain('>停止本应用启动的 DSH<')
    expect(panel).toContain('>全部停止<')
    // 一个动作、一个实现：同一个命令，多了（或者少了）一个 instanceKey。
    const window = await source('src/settings/SettingsWindow.tsx')
    expect(window).toContain('nativeRuntime.stopManagedDsh(instanceKey)')
    expect(window).toContain('onStopAllManagedDsh={() => { void stopManagedInstance() }}')
    // 「刷新」跟着搬到了下拉里（它不是停止，所以留着不会造成两个控件做同一件事）。
    expect(panel).toContain('onRefresh={props.onRefreshManagedDsh}')
  })
})
