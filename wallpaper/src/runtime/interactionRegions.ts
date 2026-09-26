export interface InteractionRegion {
  id: string
  x: number
  y: number
  width: number
  height: number
}

export interface InteractionRegionSnapshot {
  session: number
  revision: number
  scaleFactor: number
  regions: InteractionRegion[]
}

export async function beginInteractionRegionSession(): Promise<number> {
  if (!('__TAURI_INTERNALS__' in window)) return 0
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<number>('begin_interaction_region_session')
}

/**
 * 哪些元素算"点它就是跟岛交互"。
 *
 * 只挑真正的控件：容器自己也带上会把整块区域算进来，反而把"桌面空白双击"挤掉。
 */
const INTERACTIVE_SELECTOR = 'button, input, select, textarea, a[href], [role="button"], [tabindex]'

export interface Box {
  left: number
  top: number
  width: number
  height: number
}

/** 两个盒子的并集（纯函数，便于测试）。 */
export function unionBoxes(base: Box, ...extras: readonly Box[]): Box {
  let left = base.left
  let top = base.top
  let right = base.left + base.width
  let bottom = base.top + base.height
  for (const extra of extras) {
    left = Math.min(left, extra.left)
    top = Math.min(top, extra.top)
    right = Math.max(right, extra.left + extra.width)
    bottom = Math.max(bottom, extra.top + extra.height)
  }
  return { left, top, width: right - left, height: bottom - top }
}

function elementBox(element: HTMLElement): Box | undefined {
  if (element.getClientRects().length === 0) return undefined
  if (getComputedStyle(element).visibility === 'hidden') return undefined
  const rect = element.getBoundingClientRect()
  if (rect.width === 0 || rect.height === 0) return undefined
  return { left: rect.left, top: rect.top, width: rect.width, height: rect.height }
}

/**
 * 热区矩形 = 元素自身的盒子 **并上** 它内部可交互控件的盒子。
 *
 * 岛里有控件是绝对定位、溢出容器的：实测 section 的盒子到 x=1760，而「会话记录 / 模型下拉 /
 * 发送」落在 x≈1747–1918。`getBoundingClientRect()` 不含溢出的子元素，于是这些点落在热区之外，
 * 被原生的"桌面空白双击"判成空白桌面——双击功能组件反而切回表桌面（日志里
 * `hits_interaction_region=false`、`cursor=(1839,1515)`、`rects=[(801,1225)-(1760,1548)]`）。
 * 取并集之后，"点岛上的控件"永远属于岛自己，而空白桌面仍然是空白桌面。
 */
function regionBox(element: HTMLElement): Box | undefined {
  const base = elementBox(element)
  if (!base) return undefined
  const controls = [...element.querySelectorAll<HTMLElement>(INTERACTIVE_SELECTOR)]
    .map(elementBox)
    .filter((box): box is Box => box !== undefined)
  return unionBoxes(base, ...controls)
}

export function collectInteractionRegions(root: ParentNode = document): InteractionRegion[] {
  return [...root.querySelectorAll<HTMLElement>('[data-interaction-region]')]
    .map((element, index) => ({ element, index }))
    .flatMap(({ element, index }) => {
      const box = regionBox(element)
      if (!box) return []
      return [{
        id: element.dataset.interactionRegion || `region-${index}`,
        x: box.left,
        y: box.top,
        width: box.width,
        height: box.height,
      }]
    })
    .filter((region) => region.width > 0 && region.height > 0)
}

export async function publishInteractionRegions(snapshot: InteractionRegionSnapshot): Promise<void> {
  if (!('__TAURI_INTERNALS__' in window)) return
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('update_interaction_regions', {
    regions: snapshot.regions,
    scaleFactor: snapshot.scaleFactor,
    session: snapshot.session,
    revision: snapshot.revision,
  })
}
