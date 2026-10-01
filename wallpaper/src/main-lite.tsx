/** Lite 首发版壁纸前端入口：只加载纯壁纸、动画和精简设置。 */
// 词条先于一切，而且**只登记 Lite 那一半**（`zh.shared.ts` / `en.shared.ts`）—— 完整版的词条
// 不进这个入口的可达图，这是 `dist-lite` 产物的边界。
import './i18n/lite.ts'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { LiteApp } from './lite/LiteApp.tsx'
import { LiteSettingsWindow } from './lite/LiteSettingsWindow.tsx'
import { currentSurface } from './surface.ts'

const el = document.getElementById('root')
if (el === null) throw new Error('missing #root')

const nativeLabel = '__TAURI_INTERNALS__' in window ? getCurrentWindow().label : undefined
const surface = nativeLabel === 'background' || nativeLabel === 'settings'
  ? nativeLabel
  : currentSurface()
document.documentElement.dataset.surface = surface
createRoot(el).render(surface === 'settings' ? <LiteSettingsWindow /> : <LiteApp />)
