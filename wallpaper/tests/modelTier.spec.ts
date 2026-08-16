import { describe, expect, it } from 'vitest'
import { personaIdFor, resolveModelTier } from '../src/domain/modelTier.ts'

describe('model tier mapping', () => {
  it('prioritizes exact user rules, then flexible rules, then built-ins', () => {
    const rules = [
      { backend: '*' as const, pattern: 'special', match: 'contains' as const, tier: 'flash' as const },
      { backend: 'harness' as const, pattern: 'special-pro', match: 'exact' as const, tier: 'pro' as const },
    ]
    expect(resolveModelTier('harness', 'dsh', 'special-pro', rules)).toBe('pro')
    expect(resolveModelTier('deepseek-web', 'deepseek', 'special-pro', rules)).toBe('flash')
    expect(resolveModelTier('deepseek-api', 'deepseek', 'deepseek-reasoner', [])).toBe('pro')
  })

  it('derives all four personas without using reasoning effort', () => {
    expect(personaIdFor('deepseek-web', 'flash')).toBe('blue-child')
    expect(personaIdFor('deepseek-api', 'pro')).toBe('blue-adult')
    expect(personaIdFor('harness', 'flash')).toBe('black-child')
    expect(personaIdFor('harness', 'pro')).toBe('black-adult')
  })
})
