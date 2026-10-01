import type { LitePortraitId } from './types.ts'
import { assetUrl, LITE_PORTRAIT_OPTIONS } from './settings.ts'
import type { PersonaManifest } from '../persona/types.ts'

// Lite never renders a bubble.  Keep only the two legacy fields needed to
// satisfy the shared PersonaManifest shape; do not pull backend/chat labels
// into the first-release bundle just to populate unused copy.
const EMPTY_BUBBLES = { morning: '', done: '' } as PersonaManifest['bubbles']
// 名字不在这里：它与 `LITE_PORTRAIT_OPTIONS` 的格子文字是同一句，所以从那张表里取（见下）。
const PERSONA_META: Record<Exclude<LitePortraitId, 'custom'>, Pick<PersonaManifest, 'age' | 'kind' | 'theme'>> = {
  'blue-adult': { age: 'adult', kind: 'blue', theme: { primary: '#4da6ff', accent: '#9ad0ff', glow: 'rgba(77,166,255,0.18)' } },
  'blue-child': { age: 'child', kind: 'blue', theme: { primary: '#4da6ff', accent: '#7fc4ff', glow: 'rgba(77,166,255,0.18)' } },
  'black-adult': { age: 'adult', kind: 'black', theme: { primary: '#e03050', accent: '#ff6b81', glow: 'rgba(224,48,80,0.20)' } },
  'black-child': { age: 'child', kind: 'black', theme: { primary: '#e03050', accent: '#ff8a9a', glow: 'rgba(224,48,80,0.20)' } },
}

export function litePersona(id: LitePortraitId, portraitUrl?: string): PersonaManifest {
  const resolvedId = id === 'custom' ? 'blue-adult' : id
  const meta = PERSONA_META[resolvedId]
  const option = LITE_PORTRAIT_OPTIONS.find((candidate) => candidate.id === resolvedId)
  return {
    id: resolvedId,
    ...meta,
    // getter：语言切了之后同一份 manifest 也要给出新的名字（它每次渲染时被重算，但不能被定死）。
    get name() { return option?.label ?? resolvedId },
    bubbles: EMPTY_BUBBLES,
    assets: {
      portrait: portraitUrl ?? assetUrl(option?.path ?? 'personas/portrait-blue-adult.png'),
      sleep: assetUrl('personas/wake-frames/variant-anima/sleep.png'),
    },
  }
}
