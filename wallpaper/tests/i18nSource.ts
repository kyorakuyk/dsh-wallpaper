/**
 * 词条真源**全文**的读取入口 —— 给"某句话在不在字典里"这类断言用。
 *
 * 词条按"面"拆成了两半：`<语言>.shared.ts`（完整版与 Lite 都要说的）与 `<语言>.full.ts`
 * （只有完整版会说的），见 `tests/liteI18nBoundary.spec.ts`。所以核对文案时**必须把两半合起来看**
 * —— 这条规矩只写在这一处，免得每个测试各扫一半，漏掉的那一半还照样绿。
 *
 * 剥不剥注释由调用方决定：`conversationHost.spec.ts` 要区分"活在词条里"和"只活在 FREEZE 注释里"。
 */
import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

export async function dictionarySource(language: 'zh' | 'en' = 'zh'): Promise<string> {
  const halves = await Promise.all(
    ['shared', 'full'].map(async (half) => readFile(resolve(wallpaperRoot, `src/i18n/${language}.${half}.ts`), 'utf8')),
  )
  return halves.join('\n').replace(/\r\n?/g, '\n')
}
