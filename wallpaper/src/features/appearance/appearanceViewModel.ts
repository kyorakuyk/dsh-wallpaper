import { APPEARANCE_SLOTS, isAssetExposedInComponentLibrary, type AppearanceSlot, type AssetMediaType, type AssetRecord, type ThemeRecord } from '../../appearance/theme/index.ts'
import { t } from '../../i18n/index.ts'

export interface AppearanceThemeSummary extends ThemeRecord {
  name: string
  author?: string
  description?: string
  previewUrl?: string
  inheritedSlots?: AppearanceSlot[]
}

export interface AppearanceAssetSummary extends AssetRecord {
  previewUrl?: string
}

export interface AppearanceSlotPresentation {
  label: string
  shortLabel: string
  description: string
  acceptedMedia: AssetMediaType[]
}

export interface AssetClassificationRequest {
  assetIds: string[]
  slots: AppearanceSlot[]
}

/**
 * The wording is read through getters rather than filled in once at module load: the language is
 * restored from the settings document after startup, so a record evaluated at import time would
 * keep showing the Chinese it saw then. `acceptedMedia` is a fact about the slot rather than a
 * sentence, so it stays a plain field — and `SLOT_PRESENTATION[slot].label` still reads the same at
 * every call site.
 */
export const SLOT_PRESENTATION: Record<AppearanceSlot, AppearanceSlotPresentation> = {
  'desktop.background': {
    get label() { return t('appearance.slot.desktop.background.label') },
    get shortLabel() { return t('appearance.slot.desktop.background.short') },
    get description() { return t('appearance.slot.desktop.background.description') },
    acceptedMedia: ['image'],
  },
  // The library can retain a future lock-screen image selection, but the
  // native MSIX takeover intentionally uses the bundled, audited sleep frame
  // until a selected asset has a verified native hand-off path.
  'lockscreen.image': {
    get label() { return t('appearance.slot.lockscreen.image.label') },
    get shortLabel() { return t('appearance.slot.lockscreen.image.short') },
    get description() { return t('appearance.slot.lockscreen.image.description') },
    acceptedMedia: ['image'],
  },
  'wake.sequence': {
    get label() { return t('appearance.slot.wake.sequence.label') },
    get shortLabel() { return t('appearance.slot.wake.sequence.short') },
    get description() { return t('appearance.slot.wake.sequence.description') },
    acceptedMedia: ['sequence'],
  },
  'persona.deepseek.flash': {
    get label() { return t('appearance.slot.persona.deepseek.flash.label') },
    get shortLabel() { return t('appearance.slot.persona.deepseek.flash.short') },
    get description() { return t('appearance.slot.persona.deepseek.flash.description') },
    acceptedMedia: ['image'],
  },
  'persona.deepseek.pro': {
    get label() { return t('appearance.slot.persona.deepseek.pro.label') },
    get shortLabel() { return t('appearance.slot.persona.deepseek.pro.short') },
    get description() { return t('appearance.slot.persona.deepseek.pro.description') },
    acceptedMedia: ['image'],
  },
  'persona.harness.flash': {
    get label() { return t('appearance.slot.persona.harness.flash.label') },
    get shortLabel() { return t('appearance.slot.persona.harness.flash.short') },
    get description() { return t('appearance.slot.persona.harness.flash.description') },
    acceptedMedia: ['image'],
  },
  'persona.harness.pro': {
    get label() { return t('appearance.slot.persona.harness.pro.label') },
    get shortLabel() { return t('appearance.slot.persona.harness.pro.short') },
    get description() { return t('appearance.slot.persona.harness.pro.description') },
    acceptedMedia: ['image'],
  },
  'chat.skin': {
    get label() { return t('appearance.slot.chat.skin.label') },
    get shortLabel() { return t('appearance.slot.chat.skin.short') },
    get description() { return t('appearance.slot.chat.skin.description') },
    acceptedMedia: ['skin'],
  },
  'ui.font': {
    get label() { return t('appearance.slot.ui.font.label') },
    get shortLabel() { return t('appearance.slot.ui.font.short') },
    get description() { return t('appearance.slot.ui.font.description') },
    acceptedMedia: ['font'],
  },
}

export function compatibleSlots(asset: AppearanceAssetSummary): AppearanceSlot[] {
  return APPEARANCE_SLOTS.filter((slot) => SLOT_PRESENTATION[slot].acceptedMedia.includes(asset.mediaType))
}

export function sanitizeClassificationRequest(assets: readonly AppearanceAssetSummary[], assetIds: readonly string[], slots: readonly AppearanceSlot[]): AssetClassificationRequest {
  const selected = assets.filter((asset) => assetIds.includes(asset.id) && asset.origin.kind === 'loose' && asset.status === 'inbox')
  const allowedSlots = new Set(selected.flatMap(compatibleSlots))
  return {
    assetIds: [...new Set(selected.map((asset) => asset.id))],
    slots: APPEARANCE_SLOTS.filter((slot) => slots.includes(slot) && allowedSlots.has(slot)),
  }
}

export function sortThemes(themes: readonly AppearanceThemeSummary[]): AppearanceThemeSummary[] {
  return [...themes].sort((left, right) => {
    if (left.source !== right.source) return left.source === 'official' ? -1 : 1
    if (left.readonly !== right.readonly) return left.readonly ? -1 : 1
    return left.name.localeCompare(right.name, 'zh-CN') || right.version.localeCompare(left.version)
  })
}

/** Only classified loose assets are eligible for per-component overrides. */
export function componentAssets(assets: readonly AppearanceAssetSummary[], slot: AppearanceSlot): AppearanceAssetSummary[] {
  const accepted = SLOT_PRESENTATION[slot].acceptedMedia
  return assets
    .filter((asset) => isAssetExposedInComponentLibrary(asset) && asset.slots.includes(slot) && accepted.includes(asset.mediaType))
    .sort((left, right) => right.createdAt - left.createdAt || left.originalName.localeCompare(right.originalName, 'zh-CN'))
}

export function inboxAssets(assets: readonly AppearanceAssetSummary[]): AppearanceAssetSummary[] {
  return assets
    .filter((asset) => asset.origin.kind === 'loose' && asset.status === 'inbox')
    .sort((left, right) => right.createdAt - left.createdAt)
}

export function activeTheme(themes: readonly AppearanceThemeSummary[], id: string, version: string): AppearanceThemeSummary | undefined {
  return themes.find((theme) => theme.id === id && theme.version === version)
}

export function overrideCount(overrides: Partial<Record<AppearanceSlot, string>>): number {
  return APPEARANCE_SLOTS.filter((slot) => Boolean(overrides[slot])).length
}

export function assetMeta(asset: AppearanceAssetSummary): string {
  if (asset.mediaType === 'font') return t('appearance.asset.meta.font')
  if (asset.mediaType === 'sequence') return t('appearance.asset.meta.sequence')
  if (asset.mediaType === 'skin') return t('appearance.asset.meta.skin')
  const dimensions = asset.width && asset.height ? `${asset.width} × ${asset.height}` : t('appearance.asset.meta.image')
  return `${dimensions}${asset.hasAlpha ? t('appearance.asset.meta.transparent') : ''}`
}
