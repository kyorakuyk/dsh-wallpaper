import type { BackendMode, ModelTier, ModelTierRule } from './types.ts'
import { officialPersonaIdFor } from '../persona/officialCatalog.ts'

export const BUILTIN_MODEL_RULES: ModelTierRule[] = [
  { backend: '*', pattern: 'flash', match: 'contains', tier: 'flash' },
  { backend: '*', pattern: 'lite', match: 'contains', tier: 'flash' },
  { backend: '*', pattern: 'chat', match: 'contains', tier: 'flash' },
  { backend: '*', pattern: 'pro', match: 'contains', tier: 'pro' },
  { backend: '*', pattern: 'reasoner', match: 'contains', tier: 'pro' },
  { backend: '*', pattern: 'r1', match: 'contains', tier: 'pro' },
]

function matches(rule: ModelTierRule, backend: BackendMode, provider: string | undefined, model: string): boolean {
  if (rule.backend !== '*' && rule.backend !== backend) return false
  if (rule.provider && rule.provider.toLowerCase() !== provider?.toLowerCase()) return false
  const candidate = model.toLowerCase()
  const pattern = rule.pattern.toLowerCase()
  if (rule.match === 'exact') return candidate === pattern
  if (rule.match === 'contains') return candidate.includes(pattern)
  try {
    return new RegExp(rule.pattern, 'i').test(model)
  } catch {
    return false
  }
}

export function resolveModelTier(
  backend: BackendMode,
  provider: string | undefined,
  model: string | undefined,
  userRules: ModelTierRule[],
  previous: ModelTier = 'flash',
): ModelTier {
  if (!model) return previous === 'unknown' ? 'flash' : previous
  const exact = userRules.find((rule) => rule.match === 'exact' && matches(rule, backend, provider, model))
  if (exact) return exact.tier
  const flexible = userRules.find((rule) => rule.match !== 'exact' && matches(rule, backend, provider, model))
  if (flexible) return flexible.tier
  return BUILTIN_MODEL_RULES.find((rule) => matches(rule, backend, provider, model))?.tier ?? previous
}

export function personaIdFor(backend: BackendMode, tier: ModelTier): string {
  return officialPersonaIdFor(backend, tier)
}
