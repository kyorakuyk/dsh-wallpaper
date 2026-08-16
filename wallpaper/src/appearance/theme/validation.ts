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
    issues.push({ code: 'invalid-component', severity: 'error', message: `${slot} 的组件声明无效` })
    return false
  }

  if (SINGLE_ASSET_SLOTS.has(slot)) {
    if (value.kind !== 'asset' || !isNonEmptyString(value.path)) {
      issues.push({ code: 'invalid-component', severity: 'error', message: `${slot} 必须声明为单文件 asset` })
      return false
    }
    return true
  }

  if (slot === 'wake.sequence') {
    if (value.kind !== 'sequence' || !Array.isArray(value.frames) || value.frames.length === 0) {
      issues.push({ code: 'invalid-component', severity: 'error', message: 'wake.sequence 必须包含至少一帧' })
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
      issues.push({ code: 'invalid-component', severity: 'error', message: 'wake.sequence 包含无效帧或时长' })
    }
    return valid
  }

  if (value.kind !== 'skin' || !isNonEmptyString(value.definition)) {
    issues.push({ code: 'invalid-component', severity: 'error', message: 'chat.skin 必须声明为 skin' })
    return false
  }
  if (value.textures !== undefined && (!Array.isArray(value.textures) || !value.textures.every(isNonEmptyString))) {
    issues.push({ code: 'invalid-component', severity: 'error', message: 'chat.skin textures 必须是路径数组' })
    return false
  }
  return true
}

function parseManifest(input: unknown, issues: ThemeValidationIssue[]): ThemeManifest | undefined {
  if (!isRecord(input)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'theme.json 必须是对象' })
    return undefined
  }
  if (input.schemaVersion !== 1) {
    issues.push({ code: 'unsupported-schema', severity: 'error', message: `不支持 schemaVersion ${String(input.schemaVersion)}` })
    return undefined
  }
  if (input.kind !== 'theme') {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'kind 必须为 theme' })
  }
  if (!isNonEmptyString(input.id) || !ID_PATTERN.test(input.id)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: '主题 id 格式无效' })
  }
  if (!isNonEmptyString(input.version) || !VERSION_PATTERN.test(input.version)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: '主题 version 必须是语义版本' })
  }
  if (!isNonEmptyString(input.name)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: '主题 name 不能为空' })
  }
  if (!isOptionalString(input.author) || !isOptionalString(input.description) || !isOptionalString(input.preview)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: '可选文本字段类型无效' })
  }
  if (!isRecord(input.compatibility) || !isNonEmptyString(input.compatibility.minAppVersion) || !VERSION_PATTERN.test(input.compatibility.minAppVersion)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'compatibility.minAppVersion 格式无效' })
  }
  if (
    !isRecord(input.baseline) ||
    !isNonEmptyString(input.baseline.id) ||
    !ID_PATTERN.test(input.baseline.id) ||
    !isNonEmptyString(input.baseline.version) ||
    !VERSION_PATTERN.test(input.baseline.version)
  ) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'baseline 必须锁定有效的 id 和版本' })
  }
  if (!isRecord(input.components)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'components 必须是对象' })
  } else {
    for (const [slot, component] of Object.entries(input.components)) {
      if (!APPEARANCE_SLOTS.includes(slot as AppearanceSlot)) {
        issues.push({ code: 'invalid-component', severity: 'error', message: `未知外观槽位 ${slot}` })
      } else {
        validateComponent(slot as AppearanceSlot, component, issues)
      }
    }
  }
  if (!Array.isArray(input.files)) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'files 必须是文件清单数组' })
  } else {
    for (const entry of input.files) {
      if (!isRecord(entry) || !isNonEmptyString(entry.path) || !isNonEmptyString(entry.sha256) || !Number.isSafeInteger(entry.size) || Number(entry.size) < 0) {
        issues.push({ code: 'invalid-manifest', severity: 'error', message: 'files 中存在无效条目' })
      }
    }
  }
  if (input.ui !== undefined && (!isRecord(input.ui) || !isOptionalString(input.ui.tokens) || !isOptionalString(input.ui.text) || !isOptionalString(input.ui.layout))) {
    issues.push({ code: 'invalid-manifest', severity: 'error', message: 'ui 声明无效' })
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
      issues.push({ code: 'unsafe-path', severity: 'error', path: entry.path, message: `文件清单路径不安全：${entry.path}` })
    }
    if (!SHA256_PATTERN.test(entry.sha256)) {
      issues.push({ code: 'invalid-hash', severity: 'error', path: entry.path, message: `SHA-256 格式无效：${entry.path}` })
    }
    const canonicalPath = entry.path.toLocaleLowerCase('en-US')
    if (listedCanonicalPaths.has(canonicalPath)) {
      issues.push({ code: 'duplicate-file', severity: 'error', path: entry.path, message: `文件清单重复：${entry.path}` })
    } else {
      listed.set(entry.path, entry)
      listedCanonicalPaths.add(canonicalPath)
    }
  }

  const actual = new Map<string, PackageFileMetadata>()
  const actualCanonicalPaths = new Set<string>()
  for (const file of packageFiles) {
    if (!isSafePackagePath(file.path)) {
      issues.push({ code: 'unsafe-path', severity: 'error', path: file.path, message: `包内路径不安全：${file.path}` })
      continue
    }
    if (file.kind === 'symlink') {
      issues.push({ code: 'symlink-not-allowed', severity: 'error', path: file.path, message: `主题包不允许符号链接：${file.path}` })
      continue
    }
    if (file.kind === 'directory') continue
    const canonicalPath = file.path.toLocaleLowerCase('en-US')
    if (actualCanonicalPaths.has(canonicalPath)) {
      issues.push({ code: 'duplicate-file', severity: 'error', path: file.path, message: `包内文件重复：${file.path}` })
    } else {
      actual.set(file.path, file)
      actualCanonicalPaths.add(canonicalPath)
    }
  }

  for (const path of referencedPaths(manifest)) {
    if (!isSafePackagePath(path)) {
      issues.push({ code: 'unsafe-path', severity: 'error', path, message: `资源引用路径不安全：${path}` })
    } else if (!listed.has(path)) {
      issues.push({ code: 'unlisted-reference', severity: 'error', path, message: `资源引用未列入 files：${path}` })
    }
  }

  for (const [path, entry] of listed) {
    const file = actual.get(path)
    if (!file) {
      issues.push({ code: 'missing-file', severity: 'error', path, message: `主题包缺少文件：${path}` })
      continue
    }
    if (file.size !== entry.size) {
      issues.push({ code: 'size-mismatch', severity: 'error', path, message: `文件大小不匹配：${path}` })
    }
    if (file.sha256.toLowerCase() !== entry.sha256) {
      issues.push({ code: 'hash-mismatch', severity: 'error', path, message: `文件哈希不匹配：${path}` })
    }
  }

  for (const path of actual.keys()) {
    if (path !== 'theme.json' && !listed.has(path)) {
      issues.push({ code: 'unexpected-file', severity: 'error', path, message: `主题包包含未声明文件：${path}` })
    }
  }

  return {
    valid: !issues.some((issue) => issue.severity === 'error'),
    manifest,
    issues,
    inheritance: buildThemeInheritanceReport(manifest),
  }
}
