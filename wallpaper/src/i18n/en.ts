import type { Dict } from './zh'

/**
 * English entries.
 *
 * Written, not machine-translated: the point of this file is that an English reader gets English,
 * not a word-for-word shadow of the Chinese. Keys must match `zh.ts` exactly - the `Dict` type sees
 * to that, so a missing or extra key fails `pnpm typecheck` rather than showing up as a stray
 * Chinese string in the middle of an English window.
 */
export const en: Dict = {
  'language.label': 'Interface language',
  'language.hint': 'Takes effect immediately, and the choice is saved.',
  'language.zh': '简体中文',
  'language.en': 'English',
}
