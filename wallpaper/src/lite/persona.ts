import type { LitePortraitId } from './types.ts'
import { assetUrl, LITE_PORTRAIT_OPTIONS } from './settings.ts'
import type { PersonaManifest } from '../persona/types.ts'

const EMPTY_BUBBLES = {
  morning: '',
  done: '',
  harnessOnline: '',
  harnessOffline: '',
  chatOpen: '',
}
const PERSONA_META: Record<Exclude<LitePortraitId, 'custom'>, Pick<PersonaManifest, 'name' | 'age' | 'kind' | 'theme'>> = {
  'blue-adult': { name: '蓝色成年形态', age: 'adult', kind: 'blue', theme: { primary: '#4da6ff', accent: '#9ad0ff', glow: 'rgba(77,166,255,0.18)' } },
  'blue-child': { name: '蓝色幼年形态', age: 'child', kind: 'blue', theme: { primary: '#4da6ff', accent: '#7fc4ff', glow: 'rgba(77,166,255,0.18)' } },
  'black-adult': { name: '黑红成年形态', age: 'adult', kind: 'black', theme: { primary: '#e03050', accent: '#ff6b81', glow: 'rgba(224,48,80,0.20)' } },
  'black-child': { name: '黑红幼年形态', age: 'child', kind: 'black', theme: { primary: '#e03050', accent: '#ff8a9a', glow: 'rgba(224,48,80,0.20)' } },
}

export function litePersona(id: LitePortraitId, portraitUrl?: string): PersonaManifest {
  const resolvedId = id === 'custom' ? 'blue-adult' : id
  const meta = PERSONA_META[resolvedId]
  const builtInPortrait = LITE_PORTRAIT_OPTIONS.find((option) => option.id === resolvedId)?.path
  return {
    id: resolvedId,
    ...meta,
    bubbles: EMPTY_BUBBLES,
    assets: {
      portrait: portraitUrl ?? assetUrl(builtInPortrait ?? 'personas/portrait-blue-adult.png'),
      sleep: assetUrl('personas/wake-frames/variant-anima/sleep.png'),
    },
  }
}
