import type { AppearanceSlot } from '../appearance/theme/index.ts'
import type { AppearanceAssetSummary, AppearanceThemeSummary } from '../features/appearance/appearanceViewModel.ts'

export interface AppearanceSnapshot {
  activeTheme?: { id: string; version: string }
  overrides: Partial<Record<AppearanceSlot, string>>
}

export interface AppearanceImportResult {
  kind: 'inbox' | 'theme'
  source: 'folder' | 'archive' | 'file'
  imported: number
  deduplicated: number
  themeId?: string
  themeVersion?: string
}

export interface AppearanceImportBatch {
  results: AppearanceImportResult[]
  snapshot: AppearanceSnapshot
}

interface NativeThemeSummaryDto {
  id: string
  version: string
  name: string
  author: string | null
  description: string | null
  preview: string | null
  source: 'official' | 'user'
  readonly: boolean
  installedAt: number | null
  baselineId: string
  baselineVersion: string
}

interface NativeAssetSummaryDto {
  id: string
  sha256: string
  mediaType: 'image' | 'font' | 'sequence' | 'skin'
  originalName: string
  width: number | null
  height: number | null
  hasAlpha: boolean | null
  status: 'inbox' | 'classified' | 'corrupt'
  slots: AppearanceSlot[]
  createdAt: number
}

interface NativeAppearanceClient {
  readonly isNative: boolean
  getState(): Promise<AppearanceSnapshot>
  listThemes(): Promise<AppearanceThemeSummary[]>
  listAssets(slot?: AppearanceSlot): Promise<AppearanceAssetSummary[]>
  activateTheme(themeId: string, version: string): Promise<AppearanceSnapshot>
  setOverride(slot: AppearanceSlot, assetId: string): Promise<AppearanceSnapshot>
  clearOverride(slot?: AppearanceSlot): Promise<AppearanceSnapshot>
  importPaths(paths: string[]): Promise<AppearanceImportBatch>
  classifyAsset(assetId: string, slots: AppearanceSlot[]): Promise<AppearanceAssetSummary>
  resolveAsset(slot: AppearanceSlot): Promise<string | undefined>
}

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!('__TAURI_INTERNALS__' in window)) throw new Error('仅桌面版支持外观存储')
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(command, args)
}

function mapTheme(theme: NativeThemeSummaryDto): AppearanceThemeSummary {
  return {
    id: theme.id,
    version: theme.version,
    name: theme.name,
    author: theme.author ?? undefined,
    description: theme.description ?? undefined,
    // Manifest-relative previews need the resource resolver planned for the asset service.
    // Do not feed an unresolved disk/package path to the WebView.
    previewUrl: undefined,
    source: theme.source,
    readonly: theme.readonly,
    manifestPath: theme.id,
    installedAt: theme.installedAt ?? undefined,
  }
}

function mapAsset(asset: NativeAssetSummaryDto): AppearanceAssetSummary {
  return {
    id: asset.id,
    sha256: asset.sha256,
    mediaType: asset.mediaType,
    originalName: asset.originalName,
    width: asset.width ?? undefined,
    height: asset.height ?? undefined,
    hasAlpha: asset.hasAlpha ?? undefined,
    status: asset.status,
    slots: asset.slots,
    // The native command deliberately exposes only loose library assets.
    origin: { kind: 'loose' },
    objectPath: asset.id,
    createdAt: asset.createdAt,
  }
}

function mapSnapshot(snapshot: { activeTheme: { id: string; version: string } | null; overrides: Record<string, string> }): AppearanceSnapshot {
  const overrides: Partial<Record<AppearanceSlot, string>> = {}
  for (const [slot, assetId] of Object.entries(snapshot.overrides)) {
    overrides[slot as AppearanceSlot] = assetId
  }
  return { activeTheme: snapshot.activeTheme ?? undefined, overrides }
}

export const nativeAppearance: NativeAppearanceClient = {
  isNative: '__TAURI_INTERNALS__' in window,
  async getState() {
    if (!this.isNative) return { overrides: {} }
    return mapSnapshot(await tauriInvoke<{ activeTheme: { id: string; version: string } | null; overrides: Record<string, string> }>('appearance_get_state'))
  },
  async listThemes() {
    if (!this.isNative) return []
    const themes = await tauriInvoke<NativeThemeSummaryDto[]>('appearance_list_themes')
    return themes.map(mapTheme)
  },
  async listAssets(slot) {
    if (!this.isNative) return []
    const assets = await tauriInvoke<NativeAssetSummaryDto[]>('appearance_list_assets', { slot })
    return assets.map(mapAsset)
  },
  async activateTheme(themeId, version) {
    if (!this.isNative) return { overrides: {} }
    return mapSnapshot(await tauriInvoke<{ activeTheme: { id: string; version: string } | null; overrides: Record<string, string> }>('appearance_activate_theme', { id: themeId, version }))
  },
  async setOverride(slot, assetId) {
    if (!this.isNative) return { overrides: {} }
    return mapSnapshot(await tauriInvoke<{ activeTheme: { id: string; version: string } | null; overrides: Record<string, string> }>('appearance_set_override', { slot, assetId }))
  },
  async clearOverride(slot) {
    if (!this.isNative) return { overrides: {} }
    return mapSnapshot(await tauriInvoke<{ activeTheme: { id: string; version: string } | null; overrides: Record<string, string> }>('appearance_clear_override', { slot }))
  },
  async importPaths(paths) {
    if (!this.isNative) throw new Error('仅桌面版支持导入外观素材')
    const batch = await tauriInvoke<{
      results: Array<{
        kind: 'inbox' | 'theme'
        source: 'folder' | 'archive' | 'file'
        imported: number
        deduplicated: number
        themeId: string | null
        themeVersion: string | null
      }>
      snapshot: { activeTheme: { id: string; version: string } | null; overrides: Record<string, string> }
    }>('appearance_import_paths', { paths })
    return {
      results: batch.results.map((result) => ({ ...result, themeId: result.themeId ?? undefined, themeVersion: result.themeVersion ?? undefined })),
      snapshot: mapSnapshot(batch.snapshot),
    }
  },
  async classifyAsset(assetId, slots) {
    if (!this.isNative) throw new Error('仅桌面版支持素材分类')
    return mapAsset(await tauriInvoke<NativeAssetSummaryDto>('appearance_classify_asset', { assetId, slots }))
  },
  async resolveAsset(slot) {
    if (!this.isNative) return undefined
    const asset = await tauriInvoke<{ id: string; mediaType: string; mimeType: string; bytesBase64: string } | null>('appearance_resolve_asset', { slot })
    return asset ? `data:${asset.mimeType};base64,${asset.bytesBase64}` : undefined
  },
}

export async function chooseAppearanceImportPaths(): Promise<string[]> {
  if (!('__TAURI_INTERNALS__' in window)) return []
  const { open } = await import('@tauri-apps/plugin-dialog')
  const selected = await open({
    title: '导入主题或独立素材',
    multiple: true,
    directory: false,
    filters: [
      { name: '外观内容', extensions: ['dshwallpaper', 'zip', 'png', 'jpg', 'jpeg', 'webp', 'gif', 'avif', 'ttf', 'otf', 'woff2'] },
      { name: '所有文件', extensions: ['*'] },
    ],
  })
  if (!selected) return []
  return Array.isArray(selected) ? selected : [selected]
}

export async function chooseAppearanceImportFolder(): Promise<string[]> {
  if (!('__TAURI_INTERNALS__' in window)) return []
  const { open } = await import('@tauri-apps/plugin-dialog')
  const selected = await open({ title: '导入素材文件夹', multiple: false, directory: true })
  return selected ? [selected] : []
}
