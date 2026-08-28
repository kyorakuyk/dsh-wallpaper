import { describe, expect, it } from 'vitest'
import type { InteractionRegionSnapshot } from '../src/runtime/interactionRegions.ts'

describe('interaction region contract', () => {
  it('allows an empty region list to make the overlay fully click-through', () => {
    const snapshot: InteractionRegionSnapshot = { revision: 1, scaleFactor: 1.5, regions: [] }
    expect(snapshot.regions).toHaveLength(0)
  })

  it('keeps CSS-pixel geometry and scale factor explicit', () => {
    const snapshot: InteractionRegionSnapshot = {
      revision: 2,
      scaleFactor: 1.25,
      regions: [{ id: 'chat', x: 100, y: 200, width: 640, height: 180 }],
    }
    expect(snapshot.regions[0]).toEqual({ id: 'chat', x: 100, y: 200, width: 640, height: 180 })
    expect(snapshot.scaleFactor).toBe(1.25)
  })
})

