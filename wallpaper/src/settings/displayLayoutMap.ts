import type { DesktopDisplayInfo } from '../native/runtime.ts'

export interface DisplayTile {
  id: string
  left: number
  top: number
  width: number
  height: number
}

/** Scale physical display bounds into one centered, bottom-aligned preview frame. */
export function layoutDisplays(
  displays: DesktopDisplayInfo[],
  maxWidth: number,
  maxHeight: number,
  gap = 12,
): { tiles: DisplayTile[]; width: number; height: number } {
  if (displays.length === 0) return { tiles: [], width: 0, height: 0 }

  const minX = Math.min(...displays.map((display) => display.bounds.x))
  const minY = Math.min(...displays.map((display) => display.bounds.y))
  const maxX = Math.max(...displays.map((display) => display.bounds.x + display.bounds.width))
  const maxY = Math.max(...displays.map((display) => display.bounds.y + display.bounds.height))
  const totalWidth = Math.max(1, maxX - minX)
  const totalHeight = Math.max(1, maxY - minY)
  const frameWidth = Math.max(1, maxWidth)
  const frameHeight = Math.max(1, maxHeight)
  const scale = Math.min(frameWidth / totalWidth, frameHeight / totalHeight)
  const renderedWidth = totalWidth * scale
  const renderedHeight = totalHeight * scale
  const offsetX = (frameWidth - renderedWidth) / 2
  const offsetY = frameHeight - renderedHeight
  const safeGap = Math.max(0, gap)

  const tiles = displays.map((display) => ({
    id: display.id,
    left: Math.round(offsetX + (display.bounds.x - minX) * scale + safeGap / 2),
    top: Math.round(offsetY + (display.bounds.y - minY) * scale + safeGap / 2),
    width: Math.max(1, Math.round(display.bounds.width * scale - safeGap)),
    height: Math.max(1, Math.round(display.bounds.height * scale - safeGap)),
  }))

  return { tiles, width: frameWidth, height: frameHeight }
}
