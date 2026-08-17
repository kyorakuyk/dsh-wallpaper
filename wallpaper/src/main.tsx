/** 壁纸前端入口 */
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { App } from './App.tsx'
import './styles.css'
import { currentSurface } from './surface.ts'
import { SettingsWindow } from './settings/SettingsWindow.tsx'

const el = document.getElementById('root')
if (el === null) throw new Error('missing #root')

// Do not infer a native surface from the navigation URL. WorkerW reparents
// the interaction host and WebView2 may then report a stale/rewritten URL;
// the Tauri window label remains the authoritative identity.
const nativeLabel = '__TAURI_INTERNALS__' in window ? getCurrentWindow().label : undefined
const surface = nativeLabel === 'background' || nativeLabel === 'interaction' || nativeLabel === 'settings'
  ? nativeLabel
  : currentSurface()
document.documentElement.dataset.surface = surface
// A second WorkerW-reparented WebView2 host is not stable on current Windows
// builds: it can reload as the complete wallpaper and visibly jump. The
// established background host therefore owns both the scene and the desktop
// conversation UI; keep the legacy interaction host intentionally empty.
createRoot(el).render(surface === 'settings' ? <SettingsWindow /> : surface === 'interaction' ? null : <App surface="combined" />)
