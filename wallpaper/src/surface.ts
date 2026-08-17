export type AppSurface = 'background' | 'interaction' | 'settings' | 'combined'

export function currentSurface(search = window.location.search): AppSurface {
  const value = new URLSearchParams(search).get('surface')
  if (value === 'background' || value === 'interaction' || value === 'settings') return value
  return 'combined'
}
