export type AppSurface = 'background' | 'interaction' | 'settings' | 'combined'

export function currentSurface(search = window.location.search): AppSurface {
  // When Windows reparents a WebView beneath WorkerW, its navigated URL can
  // temporarily lose the query string. The native Tauri label is stable, and
  // must win over the URL so the interaction host never renders a duplicate
  // wallpaper scene.
  if ('__TAURI_INTERNALS__' in window) {
    try {
      // Avoid a static import here: the same source is also used by the web
      // preview, where no native window object exists.
      const label = (window as Window & { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } })
        .__TAURI_INTERNALS__?.metadata?.currentWindow?.label
      if (label === 'background' || label === 'interaction' || label === 'settings') return label
    } catch {
      // URL fallback below remains valid for previews and older Tauri builds.
    }
  }
  const value = new URLSearchParams(search).get('surface')
  if (value === 'background' || value === 'interaction' || value === 'settings') return value
  return 'combined'
}
