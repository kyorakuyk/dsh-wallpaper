export type AppSurface = 'background' | 'interaction' | 'combined'

export function currentSurface(search = window.location.search): AppSurface {
  const value = new URLSearchParams(search).get('surface')
  if (value === 'background' || value === 'interaction') return value
  return 'combined'
}
