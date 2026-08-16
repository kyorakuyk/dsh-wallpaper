import { describe, expect, it } from 'vitest'
import { currentSurface } from '../src/surface.ts'

describe('surface routing', () => {
  it('selects native window surfaces without affecting browser preview', () => {
    expect(currentSurface('')).toBe('combined')
    expect(currentSurface('?surface=background')).toBe('background')
    expect(currentSurface('?surface=interaction')).toBe('interaction')
    expect(currentSurface('?surface=unknown')).toBe('combined')
  })
})
