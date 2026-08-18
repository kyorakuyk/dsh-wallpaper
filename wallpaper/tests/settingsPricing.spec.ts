import { describe, expect, it } from 'vitest'
import { MAX_PRICE_PER_MILLION, normalizedPrice } from '../src/settings/store.ts'

describe('API pricing setting normalization', () => {
  it('keeps valid zero pricing but rejects malformed or unsafe rates', () => {
    expect(normalizedPrice(0)).toBe(0)
    expect(normalizedPrice(MAX_PRICE_PER_MILLION)).toBe(MAX_PRICE_PER_MILLION)
    expect(normalizedPrice(MAX_PRICE_PER_MILLION + 0.01)).toBeUndefined()
    expect(normalizedPrice(-1)).toBeUndefined()
    expect(normalizedPrice(Infinity)).toBeUndefined()
    expect(normalizedPrice('1')).toBeUndefined()
  })
})
