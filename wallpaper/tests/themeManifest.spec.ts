import { describe, expect, it } from 'vitest'
import {
  buildThemeInheritanceReport,
  isAssetExposedInComponentLibrary,
  isSafePackagePath,
  validateThemeManifest,
  type AssetRecord,
  type PackageFileMetadata,
  type ThemeManifest,
} from '../src/appearance/theme/index.ts'

const BACKGROUND_HASH = 'a'.repeat(64)
const PREVIEW_HASH = 'b'.repeat(64)

function manifest(overrides: Partial<ThemeManifest> = {}): ThemeManifest {
  return {
    schemaVersion: 1,
    kind: 'theme',
    id: 'user.deepsea-night',
    version: '1.0.0',
    name: 'Deepsea Night',
    compatibility: { minAppVersion: '0.3.0' },
    baseline: { id: 'official.deepsea', version: '1.0.0' },
    preview: 'preview.webp',
    components: {
      'desktop.background': { kind: 'asset', path: 'scenes/background.webp' },
    },
    files: [
      { path: 'preview.webp', sha256: PREVIEW_HASH, size: 12 },
      { path: 'scenes/background.webp', sha256: BACKGROUND_HASH, size: 24 },
    ],
    ...overrides,
  }
}

function files(): PackageFileMetadata[] {
  return [
    { path: 'theme.json', sha256: 'c'.repeat(64), size: 300 },
    { path: 'preview.webp', sha256: PREVIEW_HASH, size: 12 },
    { path: 'scenes/background.webp', sha256: BACKGROUND_HASH, size: 24 },
  ]
}

describe('theme manifest validation', () => {
  it('accepts a valid partial theme and reports every inherited slot', () => {
    const report = validateThemeManifest(manifest(), files())

    expect(report.valid).toBe(true)
    expect(report.inheritance?.complete).toBe(false)
    expect(report.inheritance?.inheritedSlots).toContain('lockscreen.image')
    expect(report.inheritance?.entries).toContainEqual({
      slot: 'lockscreen.image',
      source: 'baseline',
      baseline: { id: 'official.deepsea', version: '1.0.0' },
    })
    expect(report.inheritance?.entries).toContainEqual({ slot: 'desktop.background', source: 'theme' })
  })

  it('rejects unsupported schemas and malformed component declarations', () => {
    const unsupported = validateThemeManifest({ ...manifest(), schemaVersion: 2 }, files())
    const wrongSlotKind = validateThemeManifest(
      { ...manifest(), components: { 'wake.sequence': { kind: 'asset', path: 'scenes/background.webp' } } },
      files(),
    )

    expect(unsupported.issues.map((issue) => issue.code)).toContain('unsupported-schema')
    expect(wrongSlotKind.issues.map((issue) => issue.code)).toContain('invalid-component')
  })

  it('rejects traversal, absolute, backslash, device and ambiguous paths', () => {
    expect(isSafePackagePath('personas/blue.webp')).toBe(true)
    for (const path of ['../secret', '/absolute', 'C:/windows/file', 'ui\\skin.json', 'a//b', 'CON.png', 'image.']) {
      expect(isSafePackagePath(path), path).toBe(false)
    }

    const unsafe = manifest({
      preview: '../preview.webp',
      files: [{ path: '../preview.webp', sha256: PREVIEW_HASH, size: 12 }],
      components: {},
    })
    const report = validateThemeManifest(unsafe, [{ path: '../preview.webp', sha256: PREVIEW_HASH, size: 12 }])
    expect(report.valid).toBe(false)
    expect(report.issues.some((issue) => issue.code === 'unsafe-path')).toBe(true)
  })

  it('rejects case-insensitive path collisions on Windows', () => {
    const input = manifest({
      files: [
        { path: 'preview.webp', sha256: PREVIEW_HASH, size: 12 },
        { path: 'Preview.webp', sha256: PREVIEW_HASH, size: 12 },
        { path: 'scenes/background.webp', sha256: BACKGROUND_HASH, size: 24 },
      ],
    })
    const report = validateThemeManifest(input, files())

    expect(report.valid).toBe(false)
    expect(report.issues.some((issue) => issue.code === 'duplicate-file')).toBe(true)
  })

  it('checks hashes, sizes, duplicate entries and exact file inventory', () => {
    const badManifest = manifest({
      files: [
        { path: 'preview.webp', sha256: 'invalid', size: 12 },
        { path: 'preview.webp', sha256: PREVIEW_HASH, size: 12 },
        { path: 'scenes/background.webp', sha256: BACKGROUND_HASH, size: 24 },
      ],
    })
    const report = validateThemeManifest(badManifest, [
      { path: 'theme.json', sha256: 'c'.repeat(64), size: 300 },
      { path: 'preview.webp', sha256: PREVIEW_HASH, size: 13 },
      { path: 'extra.txt', sha256: 'd'.repeat(64), size: 1 },
    ])
    const codes = report.issues.map((issue) => issue.code)

    expect(codes).toContain('invalid-hash')
    expect(codes).toContain('duplicate-file')
    expect(codes).toContain('size-mismatch')
    expect(codes).toContain('missing-file')
    expect(codes).toContain('unexpected-file')
  })

  it('rejects a referenced resource that is absent from the signed file list', () => {
    const input = manifest({
      components: { 'desktop.background': { kind: 'asset', path: 'scenes/unlisted.webp' } },
    })
    const report = validateThemeManifest(input, files())

    expect(report.valid).toBe(false)
    expect(report.issues).toContainEqual(expect.objectContaining({ code: 'unlisted-reference', path: 'scenes/unlisted.webp' }))
  })
})

describe('theme asset visibility', () => {
  const baseAsset: AssetRecord = {
    id: 'asset-1',
    sha256: BACKGROUND_HASH,
    mediaType: 'image',
    originalName: 'background.webp',
    objectPath: 'objects/sha256/aa/hash.webp',
    status: 'classified',
    slots: ['desktop.background'],
    origin: { kind: 'loose' },
    createdAt: 1,
  }

  it('only exposes classified loose assets to component menus', () => {
    expect(isAssetExposedInComponentLibrary(baseAsset)).toBe(true)
    expect(isAssetExposedInComponentLibrary({ ...baseAsset, status: 'inbox' })).toBe(false)
    expect(
      isAssetExposedInComponentLibrary({
        ...baseAsset,
        origin: { kind: 'theme-private', themeId: 'user.deepsea-night', themeVersion: '1.0.0' },
      }),
    ).toBe(false)
  })

  it('reports a fully self-contained theme without inheritance', () => {
    const allComponents = Object.fromEntries(
      [
        'desktop.background',
        'lockscreen.image',
        'persona.deepseek.flash',
        'persona.deepseek.pro',
        'persona.harness.flash',
        'persona.harness.pro',
        'ui.font',
      ].map((slot) => [slot, { kind: 'asset', path: 'scenes/background.webp' }]),
    ) as ThemeManifest['components']
    allComponents['wake.sequence'] = { kind: 'sequence', frames: [{ path: 'scenes/background.webp', durationMs: 500 }] }
    allComponents['chat.skin'] = { kind: 'skin', definition: 'scenes/background.webp' }

    expect(buildThemeInheritanceReport(manifest({ components: allComponents })).complete).toBe(true)
  })
})
