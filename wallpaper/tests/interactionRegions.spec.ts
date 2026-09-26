import { describe, expect, it } from 'vitest'
import { unionBoxes, type InteractionRegionSnapshot } from '../src/runtime/interactionRegions.ts'

describe('interaction region contract', () => {
  it('allows an empty region list to make the overlay fully click-through', () => {
    const snapshot: InteractionRegionSnapshot = { revision: 1, scaleFactor: 1.5, regions: [] }
    expect(snapshot.regions).toHaveLength(0)
  })

  it('keeps CSS-pixel geometry and scale factor explicit', () => {
    const snapshot: InteractionRegionSnapshot = {
      revision: 2,
      scaleFactor: 1.25,
      regions: [{ id: 'chat', x: 100, y: 200, width: 640, height: 180 }],
    }
    expect(snapshot.regions[0]).toEqual({ id: 'chat', x: 100, y: 200, width: 640, height: 180 })
    expect(snapshot.scaleFactor).toBe(1.25)
  })
})

/**
 * 热区必须覆盖岛里的控件，否则点击会被原生的"桌面空白双击"判成空白桌面，双击功能组件
 * 反而切回表桌面。实测证据（0.2.0.104 日志）：
 *
 *   rects=[(801,1225)-(1760,1548)]                    ← 只按容器盒子发布
 *   cursor=(1839,1515) hits_interaction_region=false  ← 发送键 / 模型下拉落在这里
 *   cursor=(1799,1268) hits_interaction_region=false  ← 会话记录 / 收起对话落在这里
 *
 * 那些控件是绝对定位、溢出容器的，`getBoundingClientRect()` 不含溢出的子元素，所以热区
 * 取"容器 ∪ 内部控件"。
 */
describe('interaction region boxes', () => {
  const container = { left: 801, top: 1225, width: 959, height: 323 }

  it('keeps the container box when nothing overflows', () => {
    expect(unionBoxes(container)).toEqual(container)
    expect(unionBoxes(container, { left: 830, top: 1250, width: 100, height: 40 })).toEqual(container)
  })

  it('extends to a control that overflows to the right and below', () => {
    const send = { left: 1842, top: 1515, width: 70, height: 40 }
    expect(unionBoxes(container, send)).toEqual({ left: 801, top: 1225, width: 1111, height: 330 })
  })

  it('covers the points that used to fall outside the published region', () => {
    const region = unionBoxes(
      container,
      { left: 1747, top: 1301, width: 161, height: 43 }, // 会话记录
      { left: 1773, top: 1508, width: 128, height: 23 }, // 模型下拉
    )
    const inside = (x: number, y: number) =>
      x >= region.left && x < region.left + region.width && y >= region.top && y < region.top + region.height
    expect(inside(1839, 1515)).toBe(true)
    expect(inside(1799, 1268)).toBe(true)
    // 空白桌面仍须在热区之外：取并集不能把整块桌面吞掉，否则双击离开里桌面就失效了。
    expect(inside(1568, 1132)).toBe(false)
  })

  it('extends upwards and to the left as well', () => {
    expect(unionBoxes(container, { left: 700, top: 1100, width: 40, height: 40 }))
      .toEqual({ left: 700, top: 1100, width: 1060, height: 448 })
  })
})

