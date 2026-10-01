/**
 * **Lite** 登记的词条：只有 `zh.shared.ts` / `en.shared.ts`。
 *
 * `src/main-lite.tsx` 的第一行 import 它 —— 必须在任何模块级 `t()` 之前完成登记（ES 模块按 import
 * 顺序求值，写在第一位就是这个保证）。这里是 Lite 产物的边界所在：Lite 说不上完整版的任何一句话，
 * 所以完整版的键名与文案都不该进它的 bundle（`scripts/verify-lite-bundle.ps1` 拦这件事，
 * `tests/liteI18nBoundary.spec.ts` 钉住"Lite 用到的键都在这两份里"）。
 */
import { enShared } from './en.shared.ts'
import { registerDictionaries } from './index.ts'
import { zhShared } from './zh.shared.ts'

/** Lite 这一版有哪几条（测试读它；它与 `zh.shared.ts` / `en.shared.ts` 是同一份，没有第二处清单）。 */
export const LITE_DICTIONARIES = { zh: zhShared, en: enShared }

registerDictionaries(LITE_DICTIONARIES)
