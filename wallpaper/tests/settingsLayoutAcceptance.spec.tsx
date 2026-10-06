// @vitest-environment jsdom
import { act, useState } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeAll, afterAll, describe, expect, it } from 'vitest'
import type { DesktopDisplayInfo } from '../src/native/runtime.ts'
import { SettingsPanel } from '../src/settings/SettingsPanel.tsx'
import type { SettingsPage } from '../src/settings/settingsProbes.ts'
import { settingsPanelProps } from './settingsPanelFixture.ts'

let container: HTMLDivElement | undefined
let root: Root | undefined

function PageHarness() {
  const [page, setPage] = useState<SettingsPage>('general')
  return <SettingsPanel {...settingsPanelProps({ page, onPageChange: setPage })} />
}

beforeAll(() => {
  ;(globalThis as unknown as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
})

afterAll(() => {
  delete (globalThis as unknown as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT
})

afterEach(() => {
  if (root) {
    act(() => root!.unmount())
    root = undefined
  }
  container?.remove()
  container = undefined
})

describe('settings layout acceptance', () => {
  it('renders the per-display background strip as a full-width section, not a Field control', () => {
    const base = settingsPanelProps().settings
    const settings = { ...base, multiScreen: { ...base.multiScreen, enabled: true } }
    const display = (id: string, x: number, primary: boolean): DesktopDisplayInfo => ({
      id,
      name: id,
      bounds: { x, y: 0, width: 1920, height: 1080 },
      workArea: { x, y: 0, width: 1920, height: 1040 },
      scaleFactor: 1,
      primary,
    })
    const html = renderToStaticMarkup(<SettingsPanel {...settingsPanelProps({
      settings,
      page: 'general',
      desktopDisplays: [display('DISPLAY5', -1920, false), display('DISPLAY1', 0, true)],
    })} />)
    const staticContainer = document.createElement('div')
    staticContainer.innerHTML = html
    const strip = staticContainer.querySelector('.settings-display-background-grid')

    expect(strip).not.toBeNull()
    expect(strip?.closest('.settings-field')).toBeNull()
    expect(strip?.closest('.settings-field__control')).toBeNull()
    staticContainer.remove()
  })

  it('resets the content scroll position when the page changes', () => {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    act(() => root!.render(<PageHarness />))

    const content = container.querySelector<HTMLElement>('.settings-content')
    const appearanceTab = container.querySelector<HTMLButtonElement>('.settings-sidebar nav button:nth-child(3)')
    expect(content).not.toBeNull()
    expect(appearanceTab).not.toBeNull()
    content!.scrollTop = 480

    act(() => appearanceTab!.click())

    expect(content!.scrollTop).toBe(0)
    expect(container.querySelector('.settings-page-heading h1')?.textContent).toBe('外观')
  })
})
