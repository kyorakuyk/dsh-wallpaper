import type { DesktopWorkspace } from '../runtime/desktopWorkspace.ts'
import type { ReactNode } from 'react'
import type { WidgetLayout, WidgetManifest } from './sdk.ts'
import { isWidgetVisible } from './sdk.ts'

export interface WidgetHostItem {
  manifest: WidgetManifest
  layout?: WidgetLayout
  content: ReactNode
}

/**
 * The host is deliberately inert until the controlled installer exists.
 * It establishes the inner-desktop-only mount point without allowing theme
 * packages or arbitrary files to execute code in the wallpaper process.
 */
export function WidgetHost({ workspace, widgets }: { workspace: DesktopWorkspace; widgets: readonly WidgetHostItem[] }) {
  const visible = widgets.filter((widget) => isWidgetVisible(widget.manifest, workspace, widget.layout))
  if (visible.length === 0) return null
  return <section className="desktop-widget-host" aria-label="桌面组件">
    {visible.map(({ manifest, layout, content }) => <div
      key={manifest.id}
      className="desktop-widget-host__item"
      data-interaction-region={`widget:${manifest.id}`}
      style={{ left: layout?.bounds.x, top: layout?.bounds.y, width: layout?.bounds.width, height: layout?.bounds.height }}
    >
      {content}
    </div>)}
  </section>
}
