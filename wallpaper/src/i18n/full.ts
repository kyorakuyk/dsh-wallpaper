/**
 * **完整版**登记的词条：两份合起来的 `zh.ts` / `en.ts`。
 *
 * `src/main.tsx` 的第一行 import 它 —— 必须在任何模块级 `t()` 之前完成登记（ES 模块按 import
 * 顺序求值，写在第一位就是这个保证）。Lite 走 `i18n/lite.ts`，只登记 `zh.shared.ts` /
 * `en.shared.ts`，两个入口各登记各的，互不牵连。
 */
import { en } from './en.ts'
import { registerDictionaries } from './index.ts'
import { zh } from './zh.ts'

/** 完整版这一版有哪几条（测试读它，用来核对"Lite ⊆ 全量"）。 */
export const FULL_DICTIONARIES = { zh, en }

registerDictionaries(FULL_DICTIONARIES)
