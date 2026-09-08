import type { DesktopDisplayInfo, DesktopRect } from '../native/runtime.ts'

export function virtualDesktopBounds(displays: readonly DesktopDisplayInfo[]): DesktopRect {
  if (displays.length === 0) return { x: 0, y: 0, width: 1, height: 1 }
  const left = Math.min(...displays.map((display) => display.bounds.x))
  const top = Math.min(...displays.map((display) => display.bounds.y))
  const right = Math.max(...displays.map((display) => display.bounds.x + display.bounds.width))
  const bottom = Math.max(...displays.map((display) => display.bounds.y + display.bounds.height))
  return { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) }
}

/**
 * Convert a physical monitor rectangle into a percentage rectangle within
 * the single WorkerW viewport. Percentages keep negative virtual-desktop
 * origins and different total resolutions out of the renderer's positioning
 * state; each screen layer then owns its own paint containment boundary.
 */
export function displayCssRect(display: DesktopDisplayInfo, virtual: DesktopRect): Record<'left' | 'top' | 'width' | 'height', string> {
  const percent = (value: number, total: number) => `${Math.max(0, Math.min(100, value / Math.max(1, total) * 100))}%`
  return {
    left: percent(display.bounds.x - virtual.x, virtual.width),
    top: percent(display.bounds.y - virtual.y, virtual.height),
    width: percent(display.bounds.width, virtual.width),
    height: percent(display.bounds.height, virtual.height),
  }
}

export function preferredDisplayId(displays: readonly DesktopDisplayInfo[], preferred?: string): string | undefined {
  if (preferred && displays.some((display) => display.id === preferred)) return preferred
  return displays.find((display) => display.primary)?.id ?? displays[0]?.id
}

/**
 * The WorkerW is one HWND and therefore one CSS device-pixel ratio even when
 * it spans monitors with different Windows scaling settings. Fixed CSS sizes
 * in the conversation island would otherwise become physically larger on a
 * high-DPI host. Keep the island's reference size in physical pixels while
 * clamping pathological display values to a safe range.
 */
export function displayUiScale(devicePixelRatio: number): number {
  const safeRatio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1
  return Math.max(0.5, Math.min(1, 1 / safeRatio))
}

export function displayTopologySignature(displays: readonly DesktopDisplayInfo[]): string {
  return displays
    .map((display) => `${display.id}:${display.bounds.x},${display.bounds.y},${display.bounds.width},${display.bounds.height};work=${display.workArea.x},${display.workArea.y},${display.workArea.width},${display.workArea.height};scale=${display.scaleFactor.toFixed(3)};primary=${display.primary ? '1' : '0'}`)
    .sort()
    .join('|')
}
