export const APPEARANCE_SLOTS = [
  'desktop.background',
  'lockscreen.image',
  'wake.sequence',
  'persona.deepseek.flash',
  'persona.deepseek.pro',
  'persona.harness.flash',
  'persona.harness.pro',
  'chat.skin',
  'ui.font',
] as const

export type AppearanceSlot = (typeof APPEARANCE_SLOTS)[number]

export interface AssetThemeComponent {
  kind: 'asset'
  path: string
}

export interface WakeFrame {
  path: string
  durationMs: number
  fadeMs?: number
}

export interface SequenceThemeComponent {
  kind: 'sequence'
  frames: WakeFrame[]
}

export interface SkinThemeComponent {
  kind: 'skin'
  definition: string
  textures?: string[]
}

export type ThemeComponent = AssetThemeComponent | SequenceThemeComponent | SkinThemeComponent

export interface ThemeFileEntry {
  path: string
  sha256: string
  size: number
}

export interface ThemeManifest {
  schemaVersion: 1
  kind: 'theme'
  id: string
  version: string
  name: string
  author?: string
  description?: string
  preview?: string
  compatibility: {
    minAppVersion: string
  }
  baseline: {
    id: string
    version: string
  }
  components: Partial<Record<AppearanceSlot, ThemeComponent>>
  ui?: {
    tokens?: string
    text?: string
    layout?: string
  }
  files: ThemeFileEntry[]
}

export type AssetMediaType = 'image' | 'font' | 'sequence' | 'skin'
export type AssetStatus = 'inbox' | 'classified' | 'corrupt'

export type AssetOrigin =
  | { kind: 'loose' }
  | { kind: 'theme-private'; themeId: string; themeVersion: string }

export interface AssetRecord {
  id: string
  sha256: string
  mediaType: AssetMediaType
  originalName: string
  objectPath: string
  width?: number
  height?: number
  hasAlpha?: boolean
  status: AssetStatus
  slots: AppearanceSlot[]
  origin: AssetOrigin
  createdAt: number
}

export interface ThemeRecord {
  id: string
  version: string
  source: 'official' | 'user'
  manifestPath: string
  readonly: boolean
  installedAt?: number
}

/** Metadata produced by the archive/directory reader before pure validation. */
export interface PackageFileMetadata {
  path: string
  size: number
  sha256: string
  kind?: 'file' | 'directory' | 'symlink'
}

export type ThemeValidationSeverity = 'error' | 'warning'

export interface ThemeValidationIssue {
  code:
    | 'invalid-manifest'
    | 'unsupported-schema'
    | 'unsafe-path'
    | 'invalid-hash'
    | 'duplicate-file'
    | 'missing-file'
    | 'unexpected-file'
    | 'size-mismatch'
    | 'hash-mismatch'
    | 'invalid-component'
    | 'unlisted-reference'
    | 'symlink-not-allowed'
  severity: ThemeValidationSeverity
  message: string
  path?: string
}

export interface ThemeInheritanceEntry {
  slot: AppearanceSlot
  source: 'theme' | 'baseline'
  baseline?: { id: string; version: string }
}

export interface ThemeInheritanceReport {
  complete: boolean
  inheritedSlots: AppearanceSlot[]
  entries: ThemeInheritanceEntry[]
}

export interface ThemeValidationReport {
  valid: boolean
  manifest?: ThemeManifest
  issues: ThemeValidationIssue[]
  inheritance?: ThemeInheritanceReport
}
