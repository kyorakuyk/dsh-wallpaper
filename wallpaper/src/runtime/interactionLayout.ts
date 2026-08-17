import type { DesktopGeometry, GeometryRect } from '../native/runtime.ts'

export type InteractionLayout = 'floating' | 'taskbar-docked'
export type InteractionState = 'collapsed' | 'expanded'

export interface InteractionLayoutRequest {
  layout: InteractionLayout
  state: InteractionState
  anchor?: { x: number; y: number }
  safeMarginDip?: number
}

export interface InteractionPlacement extends GeometryRect {
  expandDirection: 'up' | 'down' | 'left' | 'right' | 'center'
}

const clamp = (value: number, minimum: number, maximum: number) => Math.min(maximum, Math.max(minimum, value))

export function computeInteractionPlacement(
  geometry: DesktopGeometry,
  request: InteractionLayoutRequest,
): InteractionPlacement {
  const scale = geometry.scaleFactor || 1
  const workWidthDip = geometry.workArea.width / scale
  const workHeightDip = geometry.workArea.height / scale
  const margin = request.safeMarginDip ?? 12
  const collapsedWidth = request.layout === 'taskbar-docked' ? 52 : 188
  const collapsedHeight = request.layout === 'taskbar-docked' ? 52 : 48
  // The central workspace uses a desktop-sized transparent host.  Only the
  // React-published component regions accept input, while the full height lets
  // the transcript fade naturally into the top of the desktop.
  const width = request.state === 'collapsed' ? collapsedWidth : request.layout === 'floating'
    ? workWidthDip
    : clamp(workWidthDip * 0.34, 360, 680)
  const height = request.state === 'collapsed'
    ? collapsedHeight
    : request.layout === 'floating'
    ? workHeightDip
      : clamp(workHeightDip * 0.42, 260, 620)

  let x = (workWidthDip - width) / 2
  let y = (workHeightDip - height) / 2
  let expandDirection: InteractionPlacement['expandDirection'] = 'center'

  if (request.layout === 'floating') {
    x = 0
    y = 0
  } else {
    const edge = geometry.taskbar.edge === 'unknown' || geometry.taskbar.edge === 'hidden'
      ? 'bottom'
      : geometry.taskbar.edge
    if (edge === 'top') {
      y = margin
      expandDirection = 'down'
    } else if (edge === 'left') {
      x = margin
      expandDirection = 'right'
    } else if (edge === 'right') {
      x = workWidthDip - width - margin
      expandDirection = 'left'
    } else {
      y = workHeightDip - height - margin
      expandDirection = 'up'
    }
  }

  return {
    x: Math.round(x * scale),
    y: Math.round(y * scale),
    width: Math.round(width * scale),
    height: Math.round(height * scale),
    expandDirection,
  }
}
