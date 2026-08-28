import type { DesktopWorkspace } from '../runtime/desktopWorkspace.ts'

/**
 * Public contract for future inner-desktop widgets. A theme package is never
 * executable: widgets must be installed and reviewed separately by the host.
 */
export const WIDGET_API_VERSION = 1

export type WidgetAnchor = 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right' | 'center'
export type WidgetPermission = 'network' | 'file-picker' | 'notifications' | 'dsh'

export interface WidgetSize {
  width: number
  height: number
}

export interface WidgetBounds extends WidgetSize {
  x: number
  y: number
}

export interface WidgetManifest {
  id: string
  version: string
  apiVersion: number
  displayName: string
  defaultAnchor: WidgetAnchor
  defaultSize: WidgetSize
  minSize: WidgetSize
  maxSize?: WidgetSize
  workspaces: readonly ('front' | 'inner')[]
  permissions: readonly WidgetPermission[]
  settingsSchema?: Record<string, unknown>
}

export interface WidgetGeometry {
  width: number
  height: number
  scaleFactor: number
}

export interface WidgetThemeTokens {
  accent: string
  foreground: string
  glassOpacity: number
  glassBlur: number
}

export interface WidgetStorage {
  get<T>(key: string): T | undefined
  set<T>(key: string, value: T): void
  remove(key: string): void
}

export interface WidgetEventBus {
  emit(event: string, payload?: unknown): void
  on(event: string, listener: (payload: unknown) => void): () => void
}

export interface WidgetContext {
  workspace: 'front' | 'inner'
  geometry: Readonly<WidgetGeometry>
  theme: Readonly<WidgetThemeTokens>
  storage: WidgetStorage
  events: WidgetEventBus
}

export interface DesktopWidget {
  destroy?(): void
}

export interface DesktopWidgetPlugin {
  manifest: WidgetManifest
  create(context: WidgetContext): DesktopWidget
}

export interface WidgetLayout {
  enabled: boolean
  bounds: WidgetBounds
}

const allowedWorkspaces = new Set<DesktopWorkspace>(['front', 'entering-inner', 'inner', 'leaving-inner'])
const WIDGET_ID = /^[a-z0-9]+(?:[._-][a-z0-9]+)*$/

export function validateWidgetManifest(manifest: WidgetManifest): string | undefined {
  if (!WIDGET_ID.test(manifest.id)) return '组件 ID 只能使用小写字母、数字、点、短横线或下划线。'
  if (manifest.apiVersion !== WIDGET_API_VERSION) return `组件需要 API v${manifest.apiVersion}，当前宿主仅支持 v${WIDGET_API_VERSION}。`
  if (!manifest.displayName.trim()) return '组件需要显示名称。'
  if (manifest.defaultSize.width <= 0 || manifest.defaultSize.height <= 0) return '组件默认尺寸必须大于零。'
  if (manifest.minSize.width <= 0 || manifest.minSize.height <= 0) return '组件最小尺寸必须大于零。'
  if (manifest.minSize.width > manifest.defaultSize.width || manifest.minSize.height > manifest.defaultSize.height) return '组件最小尺寸不能超过默认尺寸。'
  if (manifest.maxSize && (manifest.maxSize.width < manifest.defaultSize.width || manifest.maxSize.height < manifest.defaultSize.height)) return '组件最大尺寸不能小于默认尺寸。'
  if (!manifest.workspaces.length || manifest.workspaces.some((workspace) => !allowedWorkspaces.has(workspace))) return '组件必须声明可用工作区。'
  return undefined
}

export function isWidgetVisible(manifest: WidgetManifest, workspace: DesktopWorkspace, layout: WidgetLayout | undefined): boolean {
  return Boolean(layout?.enabled) && manifest.workspaces.includes(workspace === 'entering-inner' ? 'inner' : workspace === 'leaving-inner' ? 'inner' : workspace)
}

export function clampWidgetBounds(bounds: WidgetBounds, manifest: WidgetManifest, geometry: WidgetGeometry): WidgetBounds {
  const maxWidth = Math.max(manifest.minSize.width, Math.min(manifest.maxSize?.width ?? geometry.width, geometry.width))
  const maxHeight = Math.max(manifest.minSize.height, Math.min(manifest.maxSize?.height ?? geometry.height, geometry.height))
  const width = Math.min(maxWidth, Math.max(manifest.minSize.width, bounds.width))
  const height = Math.min(maxHeight, Math.max(manifest.minSize.height, bounds.height))
  return {
    width,
    height,
    x: Math.min(Math.max(0, bounds.x), Math.max(0, geometry.width - width)),
    y: Math.min(Math.max(0, bounds.y), Math.max(0, geometry.height - height)),
  }
}
