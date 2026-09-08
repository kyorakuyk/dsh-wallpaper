import { describe, expect, it } from 'vitest'
import { displayCssRect, displayTopologySignature, preferredDisplayId, virtualDesktopBounds } from '../src/runtime/displayLayout.ts'
import type { DesktopDisplayInfo } from '../src/native/runtime.ts'

const display = (patch: Partial<DesktopDisplayInfo> = {}): DesktopDisplayInfo => ({
  id: 'DISPLAY1',
  name: 'DISPLAY1',
  bounds: { x: 0, y: 0, width: 1920, height: 1080 },
  workArea: { x: 0, y: 0, width: 1920, height: 1040 },
  scaleFactor: 1,
  primary: true,
  ...patch,
})

describe('display layout helpers', () => {
  it('normalizes monitors with negative virtual-desktop origins', () => {
    const displays = [
      display(),
      display({ id: 'DISPLAY2', name: 'DISPLAY2', bounds: { x: -1280, y: 80, width: 1280, height: 1024 }, primary: false }),
    ]
    const virtual = virtualDesktopBounds(displays)
    expect(virtual).toEqual({ x: -1280, y: 0, width: 3200, height: 1104 })
    expect(displayCssRect(displays[1], virtual)).toEqual({ left: '0%', top: '7.246376811594203%', width: '40%', height: '92.7536231884058%' })
  })

  it('falls back to primary then first display when a saved id disappears', () => {
    const displays = [display({ primary: false }), display({ id: 'DISPLAY2', name: 'DISPLAY2', primary: true })]
    expect(preferredDisplayId(displays, 'MISSING')).toBe('DISPLAY2')
    expect(preferredDisplayId(displays, 'DISPLAY1')).toBe('DISPLAY1')
  })

  it('changes topology signatures when bounds or scale changes', () => {
    const first = [display()]
    expect(displayTopologySignature(first)).toBe(displayTopologySignature([display()]))
    expect(displayTopologySignature(first)).not.toBe(displayTopologySignature([display({ scaleFactor: 1.25 })]))
    expect(displayTopologySignature(first)).not.toBe(displayTopologySignature([display({ workArea: { x: 0, y: 0, width: 1920, height: 1000 } })]))
    expect(displayTopologySignature(first)).not.toBe(displayTopologySignature([display({ primary: false })]))
  })

  it('keeps a display id stable when only work-area metadata changes', () => {
    const displays = [display({ workArea: { x: 0, y: 0, width: 1920, height: 1000 } })]
    expect(preferredDisplayId(displays, 'DISPLAY1')).toBe('DISPLAY1')
  })
})
