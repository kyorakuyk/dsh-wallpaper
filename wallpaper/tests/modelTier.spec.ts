import { describe, expect, it } from 'vitest'
import { personaIdFor, resolveModelTier } from '../src/domain/modelTier.ts'
import { PersonaRegistry } from '../src/persona/registry.ts'

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

  it('prefers specific built-in Pro markers over broad chat-family markers', () => {
    expect(resolveModelTier('deepseek-api', 'deepseek', 'deepseek-chat-pro', [])).toBe('pro')
    expect(resolveModelTier('deepseek-api', 'deepseek', 'reasoner-chat', [])).toBe('pro')
    expect(resolveModelTier('harness', 'dsh', 'my-r1-chat', [])).toBe('pro')
    expect(resolveModelTier('deepseek-api', 'deepseek', 'deepseek-chat', [])).toBe('flash')
  })

  it('derives all four personas without using reasoning effort', () => {
    expect(personaIdFor('deepseek-web', 'flash')).toBe('blue-child')
    expect(personaIdFor('deepseek-api', 'pro')).toBe('blue-adult')
    expect(personaIdFor('harness', 'flash')).toBe('black-child')
    expect(personaIdFor('harness', 'pro')).toBe('black-adult')
  })

  it('uses the Flash baseline for an unknown black-family registry lookup', () => {
    expect(new PersonaRegistry().byKind('black').id).toBe('black-child')
  })
})
