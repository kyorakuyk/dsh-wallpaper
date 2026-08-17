export type AppSurface = 'background' | 'settings' | 'combined'

export function currentSurface(search?: string): AppSurface {
  const browserWindow = typeof window === 'undefined' ? undefined : window
  const resolvedSearch = search ?? browserWindow?.location.search ?? ''
  // When Windows reparents a WebView beneath WorkerW, its navigated URL can
  // temporarily lose the query string. The native Tauri label is stable, and
  // must win over the URL so the desktop host never renders a duplicate scene.
  if (browserWindow && '__TAURI_INTERNALS__' in browserWindow) {
    try {
      // Avoid a static import here: the same source is also used by the web
      // preview, where no native window object exists.
      const label = (browserWindow as Window & { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } })
        .__TAURI_INTERNALS__?.metadata?.currentWindow?.label
      if (label === 'background' || label === 'settings') return label
    } catch {
      // URL fallback below remains valid for previews and older Tauri builds.
    }
  }
  const value = new URLSearchParams(resolvedSearch).get('surface')
  if (value === 'background' || value === 'settings') return value
  return 'combined'
}
