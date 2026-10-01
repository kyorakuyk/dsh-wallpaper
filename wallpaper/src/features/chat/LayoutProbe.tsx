import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { t } from '../../i18n/index.ts'

/**
 * 调试量尺：把元素的**内容**左右边缘画成引导线，并在左上角列出浏览器量到的真实盒子。
 *
 * **本程序不给入口**：`ConversationBubble.tsx` 里的 import 与挂载两行是注释掉的，所以它不进产物。
 * 需要时把那两行打开即可（展开记录时才看得到东西 —— 岛折叠时没有可量的行）。
 *
 * 它是为一次真实的错位取证写的，留着是因为这类问题还会再来：两个元素的边对不齐时，靠读 CSS
 * 推断会推错对象（这个文件出现之前，我们就先按"文字对齐文字"改了一轮，而真正要的是"文字列对齐
 * 盒子"）。它给出的每个数字都是浏览器自己量的 CSS 像素，不需要任何截图换算。
 *
 * 两个当时踩到的坑写在代码里，别重复：
 *  1. 必须 `createPortal` 到 `document.body`。岛带 `transform`（`.conversation-shell` 用
 *     `translateX(-50%)` 居中），`position: fixed` 会改成相对岛定位 —— 整组线右移一个岛的左边缘，
 *     探针自己的数字块也被顶出屏幕。
 *  2. 服务端渲染（`renderToStaticMarkup`，测试里会用）没有 `document`，所以先判空再 portal。
 */
export function LayoutProbe(): React.ReactNode {
  const [rows, setRows] = useState<string[]>([])
  const [guides, setGuides] = useState<Array<{ x: number; color: string; dashed: boolean }>>([])

  useEffect(() => {
    const measure = () => {
      const pick = (selector: string) => document.querySelector<HTMLElement>(selector)
      const targets: Array<[string, HTMLElement | null, string]> = [
        ['shell   ', pick('.conversation-shell.dsh-chat'), '#9aa7b8'],
        ['history ', pick('.dsh-chat__history'), '#ff4d6d'],
        ['textarea', pick('.dsh-chat__textarea'), '#43d17a'],
        ['card    ', pick('.dsh-chat__card'), '#ffd166'],
        ['toolbar ', pick('.dsh-chat__island-toolbar'), '#4cc9f0'],
        ['footer  ', pick('.dsh-chat__footer'), '#c77dff'],
      ]
      const next: string[] = [`dpr=${window.devicePixelRatio} inner=${window.innerWidth}x${window.innerHeight}`]
      const nextGuides: Array<{ x: number; color: string; dashed: boolean }> = []
      const contentEdges: Record<string, [number, number]> = {}
      for (const [label, element, color] of targets) {
        if (!element) {
          next.push(`${label} —`)
          continue
        }
        const rect = element.getBoundingClientRect()
        const style = getComputedStyle(element)
        const insetLeft = Number.parseFloat(style.paddingLeft || '0') + Number.parseFloat(style.borderLeftWidth || '0')
        const insetRight = Number.parseFloat(style.paddingRight || '0') + Number.parseFloat(style.borderRightWidth || '0')
        const contentLeft = rect.left + insetLeft
        const contentRight = rect.right - insetRight
        contentEdges[label.trim()] = [contentLeft, contentRight]
        next.push(
          `${label} left=${rect.left.toFixed(1)} content=${contentLeft.toFixed(1)}..${contentRight.toFixed(1)} w=${rect.width.toFixed(1)}`,
        )
        nextGuides.push({ x: contentLeft, color, dashed: false }, { x: contentRight, color, dashed: true })
      }
      // 直接把"差多少"写出来：截图里最先要看的就是这几个数。
      const pairs: Array<[string, string]> = [
        ['history', 'textarea'],
        ['history', 'toolbar'],
        ['history', 'footer'],
        ['textarea', 'footer'],
      ]
      for (const [a, b] of pairs) {
        const left = contentEdges[a]
        const right = contentEdges[b]
        if (!left || !right) continue
        next.push(t('chat.layout-probe.delta', {
          a,
          b,
          left: (right[0] - left[0]).toFixed(1),
          right: (right[1] - left[1]).toFixed(1),
        }))
      }
      setRows(next)
      setGuides(nextGuides)
    }
    measure()
    const timer = window.setInterval(measure, 500)
    return () => window.clearInterval(timer)
  }, [])

  // `renderToStaticMarkup`（测试）里没有 document：探针在那种环境下什么都不画。
  if (typeof document === 'undefined') return null

  return createPortal(<>
    {guides.map((guide, index) => <div key={index} style={{
      position: 'fixed', top: 0, bottom: 0, width: 1, zIndex: 400, pointerEvents: 'none',
      left: `${guide.x}px`, background: guide.color, boxShadow: `0 0 6px ${guide.color}`,
      opacity: guide.dashed ? .45 : 1,
    }} />)}
    <div style={{
      position: 'fixed', left: 12, top: 12, zIndex: 401, pointerEvents: 'none',
      padding: '8px 10px', borderRadius: 8, background: 'rgba(0,0,0,.82)', color: '#eaf4ff',
      font: '11px/1.5 ui-monospace, Consolas, monospace', whiteSpace: 'pre',
    }}>{rows.join('\n')}</div>
  </>, document.body)
}
