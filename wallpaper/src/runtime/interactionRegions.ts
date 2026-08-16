export interface InteractionRegion {
  id: string
  x: number
  y: number
  width: number
  height: number
}

export interface InteractionRegionSnapshot {
  revision: number
  scaleFactor: number
  regions: InteractionRegion[]
}

export function collectInteractionRegions(root: ParentNode = document): InteractionRegion[] {
  return [...root.querySelectorAll<HTMLElement>('[data-interaction-region]')]
    .filter((element) => element.getClientRects().length > 0 && getComputedStyle(element).visibility !== 'hidden')
    .map((element, index) => {
      const rect = element.getBoundingClientRect()
      return {
        id: element.dataset.interactionRegion || `region-${index}`,
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
      }
    })
    .filter((region) => region.width > 0 && region.height > 0)
}

export async function publishInteractionRegions(snapshot: InteractionRegionSnapshot): Promise<void> {
  if (!('__TAURI_INTERNALS__' in window)) return
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('update_interaction_regions', {
    regions: snapshot.regions,
    scaleFactor: snapshot.scaleFactor,
    revision: snapshot.revision,
  })
}
