import { describe, expect, it } from 'vitest'
import type { DesktopDisplayInfo } from '../src/native/runtime.ts'
import { displayNumbers, layoutDisplays } from '../src/settings/displayLayoutMap.ts'

function display(id: string, x: number, y: number, width: number, height: number): DesktopDisplayInfo {
  return {
    id,
    name: id,
    bounds: { x, y, width, height },
    workArea: { x, y, width, height },
    scaleFactor: 1,
    primary: id === 'primary',
  }
}

describe('layoutDisplays', () => {
  it('numbers the primary display first without changing the input order', () => {
    const displays = [
      display('DISPLAY5', -1920, 0, 1920, 1080),
      display('DISPLAY1', 0, 0, 2560, 1440),
      display('DISPLAY3', 2560, 0, 1920, 1080),
    ]
    displays[1].primary = true

    expect(displayNumbers(displays)).toEqual([2, 1, 3])
  })

  it('uses the first display as the fallback primary when Windows reports none', () => {
    expect(displayNumbers([
      display('DISPLAY5', 0, 0, 1920, 1080),
      display('DISPLAY3', 1920, 0, 1920, 1080),
    ])).toEqual([1, 2])
  })

  it('preserves left-to-right placement and monitor size ratio', () => {
    const result = layoutDisplays([
      display('primary', 0, 0, 2560, 1600),
      display('secondary', 2560, 0, 1920, 1080),
    ], 640, 240)
    const [primary, secondary] = result.tiles

    expect(result.width).toBe(640)
    expect(result.height).toBe(240)
    expect(secondary.left).toBeGreaterThan(primary.left)
    expect(primary.width / secondary.width).toBeCloseTo(2560 / 1920, 1)
  })

  it('normalizes a negative secondary-display origin', () => {
    const result = layoutDisplays([
      display('primary', 0, 0, 1920, 1080),
      display('left', -1600, 120, 1600, 900),
    ], 640, 240)

    expect(result.tiles.every((tile) => tile.left >= 0 && tile.top >= 0)).toBe(true)
    expect(result.tiles.find((tile) => tile.id === 'left')?.left).toBeLessThan(
      result.tiles.find((tile) => tile.id === 'primary')!.left,
    )
  })

  it('preserves vertical placement', () => {
    const result = layoutDisplays([
      display('upper', 0, 0, 1920, 1080),
      display('lower', 0, 1080, 1920, 1080),
    ], 640, 240)

    expect(result.tiles[1].top).toBeGreaterThan(result.tiles[0].top)
  })

  it('lays out one display within the requested frame', () => {
    const result = layoutDisplays([display('primary', 0, 0, 1920, 1080)], 640, 240)

    expect(result.tiles).toHaveLength(1)
    expect(result.tiles[0].left).toBeGreaterThanOrEqual(0)
    expect(result.tiles[0].width).toBeLessThanOrEqual(640)
  })

  it('returns a zero-sized frame for no displays', () => {
    expect(layoutDisplays([], 640, 240)).toEqual({ tiles: [], width: 0, height: 0 })
  })
})
