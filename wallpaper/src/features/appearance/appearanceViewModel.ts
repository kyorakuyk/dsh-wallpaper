import { APPEARANCE_SLOTS, isAssetExposedInComponentLibrary, type AppearanceSlot, type AssetMediaType, type AssetRecord, type ThemeRecord } from '../../appearance/theme/index.ts'

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

export const SLOT_PRESENTATION: Record<AppearanceSlot, AppearanceSlotPresentation> = {
  'desktop.background': { label: '桌面背景', shortLabel: '背景', description: '桌面场景的底图', acceptedMedia: ['image'] },
  'lockscreen.image': { label: '锁屏图片', shortLabel: '锁屏', description: 'Windows 锁屏使用的熟睡画面', acceptedMedia: ['image'] },
  'wake.sequence': { label: '苏醒动画', shortLabel: '苏醒', description: '解锁后播放的有序帧组', acceptedMedia: ['sequence'] },
  'persona.deepseek.flash': { label: 'DeepSeek Flash 立绘', shortLabel: '蓝色幼年', description: 'DeepSeek Flash 模型形态', acceptedMedia: ['image'] },
  'persona.deepseek.pro': { label: 'DeepSeek Pro 立绘', shortLabel: '蓝色成年', description: 'DeepSeek Pro 模型形态', acceptedMedia: ['image'] },
  'persona.harness.flash': { label: 'Harness Flash 立绘', shortLabel: '黑红幼年', description: 'Harness Flash 模型形态', acceptedMedia: ['image'] },
  'persona.harness.pro': { label: 'Harness Pro 立绘', shortLabel: '黑红成年', description: 'Harness Pro 模型形态', acceptedMedia: ['image'] },
  'chat.skin': { label: '对话气泡皮肤', shortLabel: '气泡', description: '声明式玻璃材质与纹理', acceptedMedia: ['skin'] },
  'ui.font': { label: '界面字体', shortLabel: '字体', description: '聊天和菜单使用的字体', acceptedMedia: ['font'] },
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
  if (asset.mediaType === 'font') return '字体'
  if (asset.mediaType === 'sequence') return '动画序列'
  if (asset.mediaType === 'skin') return '气泡皮肤'
  const dimensions = asset.width && asset.height ? `${asset.width} × ${asset.height}` : '图片'
  return `${dimensions}${asset.hasAlpha ? ' · 透明背景' : ''}`
}
