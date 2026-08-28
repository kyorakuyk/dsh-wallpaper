/** 完整版壁纸前端入口 */
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { App } from './App.tsx'
import './styles.css'
import { currentSurface } from './surface.ts'
import { SettingsWindow } from './settings/SettingsWindow.tsx'

const el = document.getElementById('root')
if (el === null) throw new Error('missing #root')

// Do not infer a native surface from the navigation URL. WorkerW can reparent
// the desktop host and WebView2 may then report a stale/rewritten URL; the
// Tauri window label remains the authoritative identity.
const nativeLabel = '__TAURI_INTERNALS__' in window ? getCurrentWindow().label : undefined
const surface = nativeLabel === 'background' || nativeLabel === 'settings'
  ? nativeLabel
  : currentSurface()
document.documentElement.dataset.surface = surface
// The single WorkerW host owns both the scene and conversation UI. A second
// reparented WebView2 host was removed because it caused duplicate scenes,
// frame flashes and placement races.
createRoot(el).render(surface === 'settings' ? <SettingsWindow /> : <App surface="combined" />)
