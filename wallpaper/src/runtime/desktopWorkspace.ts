export type DesktopWorkspace = 'front' | 'entering-inner' | 'inner' | 'leaving-inner'

export function settledWorkspace(workspace: DesktopWorkspace): 'front' | 'inner' {
  return workspace === 'inner' || workspace === 'entering-inner' ? 'inner' : 'front'
}
