import { useEffect, useState } from 'react'
import { nativeRuntime } from '../native/runtime.ts'
import { computeInteractionPlacement, type InteractionLayout, type InteractionState } from './interactionLayout.ts'

export function useInteractionLayout(options: {
  enabled: boolean
  layout: InteractionLayout
  state: InteractionState
  anchor: { x: number; y: number }
  refreshKey?: unknown
}) {
  const [direction, setDirection] = useState<'up' | 'down' | 'left' | 'right' | 'center'>('center')

  useEffect(() => {
    if (!options.enabled || !nativeRuntime.isNative) return
    // The floating workspace has a permanently desktop-sized native host.
    // Moving it again after React mounts causes a visible right/left snap as
    // WorkerW, WebView and CSS each report a different intermediate size.
    // Its child controls already use CSS viewport coordinates, so only docked
    // capsules need native placement updates.
    if (options.layout === 'floating') {
      setDirection('center')
      return
    }
    let cancelled = false
    let frame = 0
    const apply = async () => {
      const geometry = await nativeRuntime.desktopGeometry()
      if (!geometry || cancelled) return
      const placement = computeInteractionPlacement(geometry, {
        layout: options.layout,
        state: options.state,
        anchor: options.anchor,
      })
      setDirection(placement.expandDirection)
      await nativeRuntime.applyInteractionPlacement(placement)
      if (!cancelled) window.dispatchEvent(new Event('dsh-interaction-placement'))
    }
    const schedule = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => { void apply() })
    }
    schedule()
    window.addEventListener('resize', schedule)
    return () => {
      cancelled = true
      cancelAnimationFrame(frame)
      window.removeEventListener('resize', schedule)
    }
  }, [options.enabled, options.layout, options.state, options.anchor.x, options.anchor.y, options.refreshKey])

  useEffect(() => {
    if (!options.enabled || !nativeRuntime.isNative) return
    if (options.layout === 'floating') return
    let dispose: () => void = () => undefined
    void import('@tauri-apps/api/event').then(({ listen }) => listen('desktop-geometry-changed', () => {
      void nativeRuntime.desktopGeometry().then((geometry) => {
        if (!geometry) return
        const placement = computeInteractionPlacement(geometry, {
          layout: options.layout,
          state: options.state,
          anchor: options.anchor,
        })
        setDirection(placement.expandDirection)
        return nativeRuntime.applyInteractionPlacement(placement).then(() => {
          window.dispatchEvent(new Event('dsh-interaction-placement'))
        })
      })
    })).then((unlisten) => { dispose = unlisten })
    return () => dispose()
  }, [options.enabled, options.layout, options.state, options.anchor.x, options.anchor.y])

  return direction
}
