import { describe, expect, it } from 'vitest'
import { computeInteractionPlacement } from '../src/runtime/interactionLayout.ts'
import type { DesktopGeometry } from '../src/native/runtime.ts'

const geometry = (edge: DesktopGeometry['taskbar']['edge'], width = 1920, height = 1040): DesktopGeometry => ({
  monitorId: 'primary',
  monitorBounds: { x: 0, y: 0, width: 1920, height: 1080 },
  workArea: { x: 0, y: 0, width, height },
  scaleFactor: 1,
  taskbar: { edge, autoHide: false, visible: true },
  revision: 1,
})

describe('interaction layout', () => {
  it('keeps floating panels inside the work area', () => {
    const result = computeInteractionPlacement(geometry('bottom'), {
      layout: 'floating', state: 'expanded', anchor: { x: 1, y: 1 },
    })
    expect(result.x + result.width).toBeLessThanOrEqual(1908)
    expect(result.y + result.height).toBeLessThanOrEqual(1028)
  })

  it.each([
    ['bottom', 'up'], ['top', 'down'], ['left', 'right'], ['right', 'left'],
  ] as const)('expands inward from a %s taskbar', (edge, direction) => {
    expect(computeInteractionPlacement(geometry(edge), {
      layout: 'taskbar-docked', state: 'expanded',
    }).expandDirection).toBe(direction)
  })

  it('uses compact dimensions for the collapsed handle', () => {
    const result = computeInteractionPlacement(geometry('bottom'), {
      layout: 'taskbar-docked', state: 'collapsed',
    })
    expect(result.width).toBe(52)
    expect(result.height).toBe(52)
  })

  it('keeps the floating collapsed state as a readable glass capsule', () => {
    const result = computeInteractionPlacement(geometry('bottom'), {
      layout: 'floating', state: 'collapsed',
    })
    expect(result.width).toBe(188)
    expect(result.height).toBe(48)
  })
})
