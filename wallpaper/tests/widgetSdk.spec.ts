import { describe, expect, it } from 'vitest'
import { clampWidgetBounds, isWidgetVisible, validateWidgetManifest, WIDGET_API_VERSION, type WidgetManifest } from '../src/widgets/sdk.ts'

const manifest: WidgetManifest = {
  id: 'official.clock', version: '1.0.0', apiVersion: WIDGET_API_VERSION, displayName: '时钟',
  defaultAnchor: 'top-right', defaultSize: { width: 240, height: 120 }, minSize: { width: 160, height: 80 },
  workspaces: ['inner'], permissions: [],
}

describe('widget SDK boundary', () => {
  it('accepts a constrained, inner-only manifest', () => expect(validateWidgetManifest(manifest)).toBeUndefined())
  it('does not leak inner widgets onto the front desktop', () => {
    expect(isWidgetVisible(manifest, 'front', { enabled: true, bounds: { x: 0, y: 0, width: 240, height: 120 } })).toBe(false)
    expect(isWidgetVisible(manifest, 'inner', { enabled: true, bounds: { x: 0, y: 0, width: 240, height: 120 } })).toBe(true)
  })
  it('keeps layout inside the host viewport', () => {
    expect(clampWidgetBounds({ x: 900, y: -20, width: 400, height: 20 }, manifest, { width: 1000, height: 700, scaleFactor: 1 }))
      .toEqual({ x: 600, y: 0, width: 400, height: 80 })
  })
})
