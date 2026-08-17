/** 壁纸前端入口 */
import { createRoot } from 'react-dom/client'
import { App } from './App.tsx'
import './styles.css'
import { currentSurface } from './surface.ts'
import { SettingsWindow } from './settings/SettingsWindow.tsx'

const el = document.getElementById('root')
if (el === null) throw new Error('missing #root')

const surface = currentSurface()
document.documentElement.dataset.surface = surface
createRoot(el).render(surface === 'settings' ? <SettingsWindow /> : <App surface={surface} />)
