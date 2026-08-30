export type LiteBackgroundId = 'workspace' | 'deepsea-2' | 'deepsea-3' | 'custom'
export type LitePortraitId = 'blue-adult' | 'blue-child' | 'black-adult' | 'black-child' | 'custom'
export type LiteAssetId = LiteBackgroundId | LitePortraitId

export interface LiteSettings {
  version: number
  background: LiteBackgroundId
  portrait: LitePortraitId
  animationsEnabled: boolean
  animationSpeed: number
  playWakeOnEveryUnlock: boolean
  skipWakeAnimation: boolean
  lockScreenEnabled: boolean
  /** Keep Explorer's desktop wallpaper on the sleep artwork during startup. */
  desktopWallpaperFallback: boolean
  autostart: boolean
}
