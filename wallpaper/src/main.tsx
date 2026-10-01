/** 完整版壁纸前端入口 */
// 词条先于一切：完整版的词典由入口登记（Lite 走 `i18n/lite.ts` 那一份），
// 而下面这些模块在 import 期就可能要句子。
import './i18n/full.ts'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { App } from './App.tsx'
import './styles.css'
import { currentSurface } from './surface.ts'
import { SettingsWindow } from './settings/SettingsWindow.tsx'
import { BallWindow } from './floating/BallWindow.tsx'

const el = document.getElementById('root')
if (el === null) throw new Error('missing #root')

// Do not infer a native surface from the navigation URL. WorkerW can reparent
// the desktop host and WebView2 may then report a stale/rewritten URL; the
// Tauri window label remains the authoritative identity.
const nativeLabel = '__TAURI_INTERNALS__' in window ? getCurrentWindow().label : undefined
// 悬浮球窗口的原生标签是 `floating-ball`，对外是独立 surface `ball`（与 surface.ts 同一映射）。
const nativeSurface = nativeLabel === 'floating-ball' ? 'ball' : nativeLabel
const surface = nativeSurface === 'background' || nativeSurface === 'settings' || nativeSurface === 'ball'
  ? nativeSurface
  : currentSurface()
document.documentElement.dataset.surface = surface
// The single WorkerW host owns both the scene and conversation UI. A second
// reparented WebView2 host was removed because it caused duplicate scenes,
// frame flashes and placement races. The floating ball is the exception: it is
// its own top-level window (never a child of the desktop host) and it renders
// only the collapsed capsule.
createRoot(el).render(
  surface === 'settings'
    ? <SettingsWindow />
    : surface === 'ball'
      ? <BallWindow />
      : <App surface="combined" />,
)
