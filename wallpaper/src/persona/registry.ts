/** 形态注册表：内置形态引用 public/personas/ 素材；无素材字段时回退程序占位 */

import type { PersonaId, PersonaManifest, ThemeKind } from './types.ts'
import { DEFAULT_BUBBLES } from './types.ts'
import { assetUrl } from '../settings/store.ts'

/**
 * 素材 URL 约定（Vite public 目录）：
 *   /personas/sleep.jpg                睡眠场景
 *   /personas/wake.jpg                 苏醒场景
 *   /personas/portrait-<id>.png        待机立绘（透明底，scripts/remove-bg.py 抠图）
 * 用户新增立绘：放入 wallpaper/public/personas/ 并在 manifest 的 assets 里引用。
 */

export const BUILTIN_PERSONAS: Record<string, PersonaManifest> = {
  'blue-child': {
    id: 'blue-child',
    name: '蓝色萝莉鲸鱼娘',
    theme: { primary: '#4da6ff', accent: '#7fc4ff', glow: 'rgba(77,166,255,0.18)' },
    age: 'child',
    kind: 'blue',
    bubbles: DEFAULT_BUBBLES,
    assets: {
      portrait: assetUrl('personas/portrait-blue-child.png'),
      sleep: assetUrl('personas/wake-frames/variant-anima/sleep.png'),
      wake: assetUrl('personas/wake.jpg'),
    },
  },
  'blue-adult': {
    id: 'blue-adult',
    name: '蓝色成年鲸鱼娘',
    theme: { primary: '#4da6ff', accent: '#9ad0ff', glow: 'rgba(77,166,255,0.18)' },
    age: 'adult',
    kind: 'blue',
    bubbles: DEFAULT_BUBBLES,
    assets: {
      portrait: assetUrl('personas/portrait-blue-adult.png'),
      sleep: assetUrl('personas/wake-frames/variant-anima/sleep.png'),
      wake: assetUrl('personas/wake.jpg'),
    },
  },
  'black-adult': {
    id: 'black-adult',
    name: '黑红成年鲸鱼娘',
    theme: { primary: '#e03050', accent: '#ff6b81', glow: 'rgba(224,48,80,0.20)' },
    age: 'adult',
    kind: 'black',
    bubbles: DEFAULT_BUBBLES,
    assets: {
      portrait: assetUrl('personas/portrait-black-adult.png'),
      sleep: assetUrl('personas/wake-frames/variant-anima/sleep.png'),
      wake: assetUrl('personas/wake.jpg'),
    },
  },
  'black-child': {
    id: 'black-child',
    name: '黑红萝莉鲸鱼娘',
    theme: { primary: '#e03050', accent: '#ff8a9a', glow: 'rgba(224,48,80,0.20)' },
    age: 'child',
    kind: 'black',
    bubbles: DEFAULT_BUBBLES,
    assets: {
      portrait: assetUrl('personas/portrait-black-child.png'),
      sleep: assetUrl('personas/wake-frames/variant-anima/sleep.png'),
      wake: assetUrl('personas/wake.jpg'),
    },
  },
}

/** 形态注册表：内置 + 用户目录扫描（目录含 manifest.json 即注册） */
export class PersonaRegistry {
  private personas = new Map<PersonaId, PersonaManifest>(Object.entries(BUILTIN_PERSONAS))

  list(): PersonaManifest[] {
    return [...this.personas.values()]
  }

  get(id: PersonaId): PersonaManifest {
    return this.personas.get(id) ?? BUILTIN_PERSONAS['blue-child']
  }

  /** 按后端类型取默认形态：blue → blue-child；black → black-adult */
  byKind(kind: ThemeKind, age?: 'child' | 'adult'): PersonaManifest {
    const fallback = kind === 'black' ? 'black-adult' : 'blue-child'
    const withAge = `${kind}-${age ?? (kind === 'black' ? 'adult' : 'child')}`
    return this.personas.get(withAge) ?? this.personas.get(fallback)!
  }

  /** 扫描用户目录（P2：目录含 manifest.json 即注册，替换目录即换装） */
  async scanUserPersonas(baseUrl: string): Promise<void> {
    // 网页环境下无法直接枚举目录；通过 manifest 约定 + 设置面板导入（P2）。
    // 此处预留：若宿主注入 persona 清单（如 Tauri 壳），从这里合并。
    void baseUrl
  }
}
