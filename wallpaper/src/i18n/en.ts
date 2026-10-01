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

  'nav.general.label': 'General',
  'nav.general.hint': 'Startup and usage',
  'nav.connections.label': 'Connections',
  'nav.connections.hint': 'DeepSeek and DSH',
  'nav.appearance.label': 'Appearance',
  'nav.appearance.hint': 'Background and animation',
  'nav.personas.label': 'Personas',
  'nav.personas.hint': 'Model mapping rules',
  'nav.history.label': 'History',
  'nav.history.hint': 'API conversations',
  'nav.system.label': 'System',
  'nav.system.hint': 'Windows integration',

  'statusbar.autosave': 'Changes are saved automatically',

  'language.card': 'Language',
}
