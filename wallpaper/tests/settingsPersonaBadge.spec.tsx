import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { SettingsPersonaBadge } from '../src/settings/SettingsPersonaBadge.tsx'

describe('SettingsPersonaBadge connection indicator', () => {
  it('distinguishes ready, connecting, and offline states', () => {
    const render = (harnessStatus: 'bridge-ready' | 'bridge-loading' | 'offline') =>
      renderToStaticMarkup(<SettingsPersonaBadge backend="harness" backgroundId="default" harnessStatus={harnessStatus} />)

    expect(render('bridge-ready')).toContain('<i class="is-online">')
    expect(render('bridge-loading')).toContain('<i class="is-pending">')
    expect(render('offline')).toContain('<i class="">')
  })
})
