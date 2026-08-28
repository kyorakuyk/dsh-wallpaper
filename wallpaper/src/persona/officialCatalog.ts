/**
 * Official persona baseline.
 *
 * This is intentionally a small, typed catalog rather than a user-selectable
 * persona list: the active form is derived from backend + model tier.  Users
 * may replace the artwork assigned to a slot, but they cannot use this
 * catalog to override the Flash/Pro age rule.
 */

import type { AppearanceSlot } from '../appearance/theme/types.ts'
import type { BackendMode, ModelTier } from '../domain/types.ts'
import type { AgeKind, PersonaTheme, ThemeKind } from './types.ts'

export type OfficialPersonaFamily = 'deepseek' | 'harness'
export type OfficialPersonaTier = Exclude<ModelTier, 'unknown'>
export type OfficialPersonaId = 'blue-child' | 'blue-adult' | 'black-child' | 'black-adult'
export type OfficialPersonaSlot = Extract<AppearanceSlot,
  | 'persona.deepseek.flash'
  | 'persona.deepseek.pro'
  | 'persona.harness.flash'
  | 'persona.harness.pro'
>

export interface OfficialPersonaCard {
  /** Stable built-in persona identity used by the runtime registry. */
  id: OfficialPersonaId
  /** The only material slot this form represents. */
  slot: OfficialPersonaSlot
  /** Product/backend family. Web and API DeepSeek share the blue family. */
  family: OfficialPersonaFamily
  /** Flash is always child; Pro is always adult. */
  tier: OfficialPersonaTier
  age: AgeKind
  kind: ThemeKind
  name: string
  theme: PersonaTheme
  /** Relative Vite public path; callers resolve it with the app resource base. */
  portraitPath: `personas/${string}.png`
}

/**
 * The four formal assets shipped by the application.  Keep this catalog
 * independent from the mutable appearance library: it defines the baseline,
 * whereas the library records optional per-slot replacements.
 */
export const OFFICIAL_PERSONA_CARDS = [
  {
    id: 'blue-child',
    slot: 'persona.deepseek.flash',
    family: 'deepseek',
    tier: 'flash',
    age: 'child',
    kind: 'blue',
    name: 'DeepSeek Flash · 蓝色幼年',
    theme: { primary: '#4da6ff', accent: '#7fc4ff', glow: 'rgba(77,166,255,0.18)' },
    portraitPath: 'personas/portrait-blue-child.png',
  },
  {
    id: 'blue-adult',
    slot: 'persona.deepseek.pro',
    family: 'deepseek',
    tier: 'pro',
    age: 'adult',
    kind: 'blue',
    name: 'DeepSeek Pro · 蓝色成年',
    theme: { primary: '#4da6ff', accent: '#9ad0ff', glow: 'rgba(77,166,255,0.18)' },
    portraitPath: 'personas/portrait-blue-adult.png',
  },
  {
    id: 'black-child',
    slot: 'persona.harness.flash',
    family: 'harness',
    tier: 'flash',
    age: 'child',
    kind: 'black',
    name: 'Harness Flash · 黑红幼年',
    theme: { primary: '#e03050', accent: '#ff8a9a', glow: 'rgba(224,48,80,0.20)' },
    portraitPath: 'personas/portrait-black-child.png',
  },
  {
    id: 'black-adult',
    slot: 'persona.harness.pro',
    family: 'harness',
    tier: 'pro',
    age: 'adult',
    kind: 'black',
    name: 'Harness Pro · 黑红成年',
    theme: { primary: '#e03050', accent: '#ff6b81', glow: 'rgba(224,48,80,0.20)' },
    portraitPath: 'personas/portrait-black-adult.png',
  },
] as const satisfies readonly OfficialPersonaCard[]

function familyForBackend(backend: BackendMode): OfficialPersonaFamily {
  return backend === 'harness' ? 'harness' : 'deepseek'
}

/** Unknown models use the cold-start Flash baseline until a model rule resolves them. */
function normalizeTier(tier: ModelTier): OfficialPersonaTier {
  return tier === 'pro' ? 'pro' : 'flash'
}

/** Resolves the sole official form permitted for a backend/model tier pair. */
export function officialPersonaCardFor(backend: BackendMode, tier: ModelTier): OfficialPersonaCard {
  const family = familyForBackend(backend)
  const normalizedTier = normalizeTier(tier)
  const card = OFFICIAL_PERSONA_CARDS.find((candidate) => candidate.family === family && candidate.tier === normalizedTier)
  // The catalog is a compile-time shipped invariant. Keep a defensive fallback
  // for a corrupted future catalog without exposing an arbitrary age selector.
  return card ?? OFFICIAL_PERSONA_CARDS[0]
}

export function officialPersonaIdFor(backend: BackendMode, tier: ModelTier): OfficialPersonaId {
  return officialPersonaCardFor(backend, tier).id
}
