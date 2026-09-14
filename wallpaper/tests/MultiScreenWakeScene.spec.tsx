import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { MultiScreenWakeScene } from '../src/scenes/MultiScreenWakeScene.tsx'
import { BUILTIN_PERSONAS } from '../src/persona/registry.ts'
import type { DesktopDisplayInfo } from '../src/native/runtime.ts'

const display = (patch: Partial<DesktopDisplayInfo> = {}): DesktopDisplayInfo => ({
  id: 'DISPLAY1',
  name: 'DISPLAY1',
  bounds: { x: 0, y: 0, width: 1920, height: 1080 },
  workArea: { x: 0, y: 0, width: 1920, height: 1040 },
  scaleFactor: 1,
  primary: true,
  ...patch,
})

describe('MultiScreenWakeScene', () => {
  it('renders one independent frame stack per display', () => {
    const html = renderToStaticMarkup(<MultiScreenWakeScene
      displays={[
        display(),
        display({ id: 'DISPLAY2', name: 'DISPLAY2', bounds: { x: 1920, y: 120, width: 2560, height: 1440 }, primary: false }),
      ]}
      persona={BUILTIN_PERSONAS['blue-child']}
      startIndex={1}
      onWakeDone={() => undefined}
    />)

    expect(html.match(/data-display-id=/g)).toHaveLength(2)
    expect(html.match(/multi-screen-wake-frame active/g)).toHaveLength(2)
    expect(html).toContain('left:0%')
    expect(html).toContain('width:57.14285714285714%')
  })

  it('keeps a safe full-viewport fallback before display enumeration completes', () => {
    const html = renderToStaticMarkup(<MultiScreenWakeScene
      displays={[]}
      persona={BUILTIN_PERSONAS['blue-child']}
      onWakeDone={() => undefined}
    />)

    expect(html).toContain('multi-screen-wake-surface--virtual')
    expect(html).toContain('multi-screen-wake-frame active')
  })
})
