export type AppSurface = 'background' | 'settings' | 'combined' | 'ball'

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
      // 悬浮球窗口的原生标签是 `floating-ball`（见 `src-tauri/src/floating_ball.rs`），
      // 但它对我们是一个独立的 surface `ball`；两个写法都认，避免以后改标签时
      // 悄悄退回整屏壁纸。
      if (label === 'ball' || label === 'floating-ball') return 'ball'
    } catch {
      // URL fallback below remains valid for previews and older Tauri builds.
    }
  }
  const value = new URLSearchParams(resolvedSearch).get('surface')
  if (value === 'background' || value === 'settings' || value === 'ball') return value
  return 'combined'
}
