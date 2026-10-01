import { t } from '../../i18n/index.ts'
import {
  APPEARANCE_SLOTS,
  type AppearanceSlot,
  type AssetRecord,
  type PackageFileMetadata,
  type ThemeComponent,
  type ThemeInheritanceReport,
  type ThemeManifest,
  type ThemeValidationIssue,
  type ThemeValidationReport,
} from './types.ts'

const SHA256_PATTERN = /^[a-f0-9]{64}$/
const ID_PATTERN = /^[a-z0-9](?:[a-z0-9._-]{0,62}[a-z0-9])?$/
const VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/
const WINDOWS_DEVICE_NAME = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i

const SINGLE_ASSET_SLOTS = new Set<AppearanceSlot>([
  'desktop.background',
  'lockscreen.image',
  'persona.deepseek.flash',
  'persona.deepseek.pro',
  'persona.harness.flash',
  'persona.harness.pro',
  'ui.font',
])

type UnknownRecord = Record<string, unknown>

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0
}

function isOptionalString(value: unknown): value is string | undefined {
  return value === undefined || typeof value === 'string'
}

export function isSafePackagePath(path: string): boolean {
  if (!path || path !== path.trim() || path.includes('\0') || path.includes('\\')) return false
  if (path.startsWith('/') || path.startsWith('//') || /^[A-Za-z]:/.test(path)) return false

  const segments = path.split('/')
  return segments.every(
    (segment) =>
      segment.length > 0 &&
      segment !== '.' &&
      segment !== '..' &&
      !segment.includes(':') &&
      !/[. ]$/.test(segment) &&
      !/[\u0000-\u001f]/.test(segment) &&
      !WINDOWS_DEVICE_NAME.test(segment),
  )
}

export function buildThemeInheritanceReport(manifest: ThemeManifest): ThemeInheritanceReport {
  const inheritedSlots = APPEARANCE_SLOTS.filter((slot) => manifest.components[slot] === undefined)
  return {
    complete: inheritedSlots.length === 0,
    inheritedSlots,
    entries: APPEARANCE_SLOTS.map((slot) =>
      manifest.components[slot]
        ? { slot, source: 'theme' }
        : { slot, source: 'baseline', baseline: { ...manifest.baseline } },
    ),
  }
}

/** Theme-owned assets never enter the loose component menus. */
export function isAssetExposedInComponentLibrary(asset: AssetRecord): boolean {
  return asset.origin.kind === 'loose' && asset.status === 'classified'
}

function validateComponent(
  slot: AppearanceSlot,
  value: unknown,
  issues: ThemeValidationIssue[],
): value is ThemeComponent {
  if (!isRecord(value) || !isNonEmptyString(value.kind)) {
    issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.component-invalid', { slot }) })
    return false
  }

  if (SINGLE_ASSET_SLOTS.has(slot)) {
    if (value.kind !== 'asset' || !isNonEmptyString(value.path)) {
      issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.single-asset-required', { slot }) })
      return false
    }
    return true
  }

  if (slot === 'wake.sequence') {
    if (value.kind !== 'sequence' || !Array.isArray(value.frames) || value.frames.length === 0) {
      issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.wake-sequence-frames-required') })
      return false
    }
    const valid = value.frames.every(
      (frame) =>
        isRecord(frame) &&
        isNonEmptyString(frame.path) &&
        Number.isFinite(frame.durationMs) &&
        Number(frame.durationMs) > 0 &&
        (frame.fadeMs === undefined || (Number.isFinite(frame.fadeMs) && Number(frame.fadeMs) >= 0)),
    )
    if (!valid) {
      issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.wake-sequence-frame-invalid') })
    }
    return valid
  }

  if (value.kind !== 'skin' || !isNonEmptyString(value.definition)) {
    issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.skin-required') })
    return false
  }
  if (value.textures !== undefined && (!Array.isArray(value.textures) || !value.textures.every(isNonEmptyString))) {
    issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.skin-textures') })
    return false
  }
  return true
}

function parseManifest(input: unknown, issues: ThemeValidationIssue[]): ThemeManifest | undefined {
  if (!isRecord(input)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.manifest-object') })
    return undefined
  }
  if (input.schemaVersion !== 1) {
    issues.push({ code: 'unsupported-schema', severity: 'error', message: t('appearance.validation.schema-version', { version: String(input.schemaVersion) }) })
    return undefined
  }
  if (input.kind !== 'theme') {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.kind-theme') })
  }
  if (!isNonEmptyString(input.id) || !ID_PATTERN.test(input.id)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.id-format') })
  }
  if (!isNonEmptyString(input.version) || !VERSION_PATTERN.test(input.version)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.version-format') })
  }
  if (!isNonEmptyString(input.name)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.name-empty') })
  }
  if (!isOptionalString(input.author) || !isOptionalString(input.description) || !isOptionalString(input.preview)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.optional-text-type') })
  }
  if (!isRecord(input.compatibility) || !isNonEmptyString(input.compatibility.minAppVersion) || !VERSION_PATTERN.test(input.compatibility.minAppVersion)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.min-app-version') })
  }
  if (
    !isRecord(input.baseline) ||
    !isNonEmptyString(input.baseline.id) ||
    !ID_PATTERN.test(input.baseline.id) ||
    !isNonEmptyString(input.baseline.version) ||
    !VERSION_PATTERN.test(input.baseline.version)
  ) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.baseline-lock') })
  }
  if (!isRecord(input.components)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.components-object') })
  } else {
    for (const [slot, component] of Object.entries(input.components)) {
      if (!APPEARANCE_SLOTS.includes(slot as AppearanceSlot)) {
        issues.push({ code: 'invalid-component', severity: 'error', message: t('appearance.validation.unknown-slot', { slot }) })
      } else {
        validateComponent(slot as AppearanceSlot, component, issues)
      }
    }
  }
  if (!Array.isArray(input.files)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.files-array') })
  } else {
    for (const entry of input.files) {
      if (!isRecord(entry) || !isNonEmptyString(entry.path) || !isNonEmptyString(entry.sha256) || !Number.isSafeInteger(entry.size) || Number(entry.size) < 0) {
        issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.file-entry') })
      }
    }
  }
  if (input.ui !== undefined && (!isRecord(input.ui) || !isOptionalString(input.ui.tokens) || !isOptionalString(input.ui.text) || !isOptionalString(input.ui.layout))) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: t('appearance.validation.ui-declaration') })
  }

  if (issues.some((issue) => issue.severity === 'error')) return undefined
  return input as unknown as ThemeManifest
}

function referencedPaths(manifest: ThemeManifest): string[] {
  const paths: string[] = []
  if (manifest.preview) paths.push(manifest.preview)
  if (manifest.ui?.tokens) paths.push(manifest.ui.tokens)
  if (manifest.ui?.text) paths.push(manifest.ui.text)
  if (manifest.ui?.layout) paths.push(manifest.ui.layout)

  for (const component of Object.values(manifest.components)) {
    if (!component) continue
    if (component.kind === 'asset') paths.push(component.path)
    if (component.kind === 'sequence') paths.push(...component.frames.map((frame) => frame.path))
    if (component.kind === 'skin') paths.push(component.definition, ...(component.textures ?? []))
  }
  return paths
}

export function validateThemeManifest(input: unknown, packageFiles: readonly PackageFileMetadata[]): ThemeValidationReport {
  const issues: ThemeValidationIssue[] = []
  const manifest = parseManifest(input, issues)
  if (!manifest) return { valid: false, issues }

  const listed = new Map<string, (typeof manifest.files)[number]>()
  const listedCanonicalPaths = new Set<string>()
  for (const entry of manifest.files) {
    if (!isSafePackagePath(entry.path)) {
      issues.push({ code: 'unsafe-path', severity: 'error', path: entry.path, message: t('appearance.validation.listed-path-unsafe', { path: entry.path }) })
    }
    if (!SHA256_PATTERN.test(entry.sha256)) {
      issues.push({ code: 'invalid-hash', severity: 'error', path: entry.path, message: t('appearance.validation.listed-hash-format', { path: entry.path }) })
    }
    const canonicalPath = entry.path.toLocaleLowerCase('en-US')
    if (listedCanonicalPaths.has(canonicalPath)) {
      issues.push({ code: 'duplicate-file', severity: 'error', path: entry.path, message: t('appearance.validation.listed-duplicate', { path: entry.path }) })
    } else {
      listed.set(entry.path, entry)
      listedCanonicalPaths.add(canonicalPath)
    }
  }

  const actual = new Map<string, PackageFileMetadata>()
  const actualCanonicalPaths = new Set<string>()
  for (const file of packageFiles) {
    if (!isSafePackagePath(file.path)) {
      issues.push({ code: 'unsafe-path', severity: 'error', path: file.path, message: t('appearance.validation.package-path-unsafe', { path: file.path }) })
      continue
    }
    if (file.kind === 'symlink') {
      issues.push({ code: 'symlink-not-allowed', severity: 'error', path: file.path, message: t('appearance.validation.package-symlink', { path: file.path }) })
      continue
    }
    if (file.kind === 'directory') continue
    const canonicalPath = file.path.toLocaleLowerCase('en-US')
    if (actualCanonicalPaths.has(canonicalPath)) {
      issues.push({ code: 'duplicate-file', severity: 'error', path: file.path, message: t('appearance.validation.package-duplicate', { path: file.path }) })
    } else {
      actual.set(file.path, file)
      actualCanonicalPaths.add(canonicalPath)
    }
  }

  for (const path of referencedPaths(manifest)) {
    if (!isSafePackagePath(path)) {
      issues.push({ code: 'unsafe-path', severity: 'error', path, message: t('appearance.validation.reference-path-unsafe', { path }) })
    } else if (!listed.has(path)) {
      issues.push({ code: 'unlisted-reference', severity: 'error', path, message: t('appearance.validation.reference-unlisted', { path }) })
    }
  }

  for (const [path, entry] of listed) {
    const file = actual.get(path)
    if (!file) {
      issues.push({ code: 'missing-file', severity: 'error', path, message: t('appearance.validation.missing-file', { path }) })
      continue
    }
    if (file.size !== entry.size) {
      issues.push({ code: 'size-mismatch', severity: 'error', path, message: t('appearance.validation.size-mismatch', { path }) })
    }
    if (file.sha256.toLowerCase() !== entry.sha256) {
      issues.push({ code: 'hash-mismatch', severity: 'error', path, message: t('appearance.validation.hash-mismatch', { path }) })
    }
  }

  for (const path of actual.keys()) {
    if (path !== 'theme.json' && !listed.has(path)) {
      issues.push({ code: 'unexpected-file', severity: 'error', path, message: t('appearance.validation.unexpected-file', { path }) })
    }
  }

  return {
    valid: !issues.some((issue) => issue.severity === 'error'),
    manifest,
    issues,
    inheritance: buildThemeInheritanceReport(manifest),
  }
}
