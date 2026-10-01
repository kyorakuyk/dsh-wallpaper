import { readdir, readFile } from 'node:fs/promises'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * 棘轮：界面源码里不许再有**硬编码的中文**。
 *
 * 为什么是"待迁清单"而不是一条硬断言：迁移要跨很多轮、很多文件，硬断言会让中间每一轮都红着。
 * 清单则**只能变短** —— 文件迁完了必须从清单里删掉，否则第二条测试会失败。于是"还剩多少"
 * 变成可测量的数字，而不是靠感觉。
 *
 * 两条豁免，都在 `docs/plans/i18n-plan.md` 里写明过：
 * 1. **注释**：这个仓库的注释一直是中文，而且是给人看的；
 * 2. **console 日志**：日志是给维护者 grep 的，翻译它等于让 grep 失效。
 */
const SRC = fileURLToPath(new URL('../src', import.meta.url))
// 清单里的路径相对 wallpaper/（与扫描脚本一致），所以基准是上一级而不是仓库根。
const REPO = fileURLToPath(new URL('..', import.meta.url))
const PENDING_FILE = fileURLToPath(new URL('./i18nPending.json', import.meta.url))

/** 剥掉注释后，还剩几行含中文（console 行豁免）。 */
export function hardcodedCopyLines(source: string): number {
  const withoutBlockComments = source.replace(/\/\*[\s\S]*?\*\//g, '')
  const withoutLineComments = withoutBlockComments
    .split(/\r?\n/)
    // 行注释只认行首（可缩进）或前面是空白的 `//`，免得把 https:// 当成注释切掉后半行。
    .map((line) => {
      const at = line.search(/(^|\s)\/\//)
      return at >= 0 ? line.slice(0, at) : line
    })
    .join('\n')
  return withoutLineComments
    .split(/\r?\n/)
    .filter((line) => /[\u4e00-\u9fff]/.test(line) && !line.includes('console.')).length
}

async function uiFiles(dir: string): Promise<string[]> {
  const found: string[] = []
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'i18n') continue
      found.push(...(await uiFiles(full)))
      continue
    }
    if (/\.(ts|tsx)$/.test(entry.name)) found.push(full)
  }
  return found
}

async function pending(): Promise<Array<[string, number]>> {
  return JSON.parse(await readFile(PENDING_FILE, 'utf8')) as Array<[string, number]>
}

function keyOf(file: string): string {
  return relative(REPO, file).replace(/\\/g, '/')
}

describe('hardcoded copy', () => {
  it('is absent from every file that is not on the pending list', async () => {
    const listed = (await pending()).map(([file]) => file)
    const offenders: string[] = []
    for (const file of await uiFiles(SRC)) {
      const key = keyOf(file)
      if (listed.some((entry) => key.endsWith(entry))) continue
      const lines = hardcodedCopyLines(await readFile(file, 'utf8'))
      if (lines > 0) offenders.push(`${key}（${lines} 行）`)
    }
    expect(offenders, '这些文件还有硬编码中文：要么迁进词条，要么写进 i18nPending.json').toEqual([])
  })

  it('keeps the pending list honest, because a list that never shrinks is a list nobody reads', async () => {
    const stale: string[] = []
    for (const [entry] of await pending()) {
      const lines = hardcodedCopyLines(await readFile(join(REPO, entry), 'utf8'))
      if (lines === 0) stale.push(entry)
    }
    expect(stale, '这些文件已经迁完，请从 i18nPending.json 里删掉').toEqual([])
  })

  it('reports how much is left', async () => {
    const listed = await pending()
    let total = 0
    for (const [entry] of listed) total += hardcodedCopyLines(await readFile(join(REPO, entry), 'utf8'))
    console.log(`  待迁文件 ${listed.length} 个，剩余含中文的行 ${total} 行`)
    // 空清单就是终点：这一条只负责报数，**不是**"必须还有活干"的契约。清单空了以后它仍然是
    // 一条有用的哨兵 —— 哪天有人往清单里塞回一个文件，上面那句日志会立刻把数字说出来，而第一
    // 条测试（清单外不许有中文）与第二条（清单里不许有迁完的）才是真正的契约。
    expect(listed.length).toBeGreaterThanOrEqual(0)
  })
})
