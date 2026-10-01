import { enFull } from './en.full.ts'
import { enShared } from './en.shared.ts'
import type { AssertKeysIn, Dict, MessageKey } from './zh.ts'

/**
 * English entries — both halves of the split in one place (`zh.ts` is the Chinese side of the same
 * two files, and the reasons for the split are written down there).
 *
 * Written, not machine-translated: the point of this file is that an English reader gets English,
 * not a word-for-word shadow of the Chinese. Keys must match `zh.ts` exactly - the `Dict` type sees
 * to that, so a missing or extra key fails `pnpm typecheck` rather than showing up as a stray
 * Chinese string in the middle of an English window.
 */
export const en: Dict = { ...enShared, ...enFull }

/** `en.ts` 里不许出现 `zh.ts` 没有的键：`Dict` 挡缺键，这一条挡多键（展开进来的更拦不住，所以要显式）。 */
export type EnglishKeysAreDeclared = AssertKeysIn<keyof typeof en, MessageKey>
