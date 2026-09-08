import { useEffect, useState } from 'react'

export interface PortraitEnvironment {
  /** RGB used by the integration layers around the cut-out portrait. */
  rgb: string
  /** 0 (dark) to 1 (bright), used to tune edge light and the contact shadow. */
  luminance: number
  /** Gradient direction, flowing from the darker side to the brighter side. */
  lightAngle: string
  /** Difference between the two sides, used to keep flat scenes subtle. */
  contrast: number
}

export interface PortraitSampleSize {
  width: number
  height: number
}

const FALLBACK: PortraitEnvironment = { rgb: '11 30 52', luminance: 0.1, lightAngle: '90deg', contrast: 0.08 }

function srgbLuminance(red: number, green: number, blue: number): number {
  const linear = (value: number) => {
    const channel = value / 255
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  }
  return linear(red) * 0.2126 + linear(green) * 0.7152 + linear(blue) * 0.0722
}

/**
 * Samples the visible part of a cover-fitted desktop background where the
 * right-side portrait rests. The values deliberately live in CSS variables so
 * any later drag/position setting only needs to supply a new anchor.
 */
export function usePortraitEnvironmentBlend(backgroundUrl: string, anchor = { x: 0.79, y: 0.78 }, sampleSize?: PortraitSampleSize): PortraitEnvironment {
  const [environment, setEnvironment] = useState<PortraitEnvironment>(FALLBACK)
  const [viewport, setViewport] = useState(() => `${sampleSize?.width ?? window.innerWidth}x${sampleSize?.height ?? window.innerHeight}`)

  useEffect(() => {
    if (!backgroundUrl) {
      setEnvironment(FALLBACK)
      return
    }
    let cancelled = false
    const image = new Image()
    image.decoding = 'async'
    image.onload = () => {
      try {
        const viewportWidth = Math.max(sampleSize?.width ?? window.innerWidth, 1)
        const viewportHeight = Math.max(sampleSize?.height ?? window.innerHeight, 1)
        const scale = Math.max(viewportWidth / image.naturalWidth, viewportHeight / image.naturalHeight)
        const renderedWidth = image.naturalWidth * scale
        const renderedHeight = image.naturalHeight * scale
        const offsetX = (viewportWidth - renderedWidth) / 2
        const offsetY = (viewportHeight - renderedHeight) / 2
        const canvas = document.createElement('canvas')
        canvas.width = image.naturalWidth
        canvas.height = image.naturalHeight
        const context = canvas.getContext('2d', { willReadFrequently: true })
        if (!context) return
        context.drawImage(image, 0, 0)

        // Centre, lower body and contact-floor points make the result stable
        // against small motions while still reacting to a new backdrop.
        const points = [[0, 0], [-.055, .07], [.055, .07], [0, .15], [-.09, .13], [.09, .13]]
        const colors: Array<[number, number, number]> = []
        for (const [dx, dy] of points) {
          const screenX = Math.min(viewportWidth - 1, Math.max(0, viewportWidth * (anchor.x + dx)))
          const screenY = Math.min(viewportHeight - 1, Math.max(0, viewportHeight * (anchor.y + dy)))
          const sourceX = Math.round((screenX - offsetX) / scale)
          const sourceY = Math.round((screenY - offsetY) / scale)
          const pixel = context.getImageData(Math.min(image.naturalWidth - 1, Math.max(0, sourceX)), Math.min(image.naturalHeight - 1, Math.max(0, sourceY)), 1, 1).data
          colors.push([pixel[0], pixel[1], pixel[2]])
        }
        const average = colors.reduce((sum, color) => [sum[0] + color[0], sum[1] + color[1], sum[2] + color[2]], [0, 0, 0]).map((value) => Math.round(value / colors.length))
        const sampleSide = (offset: number) => {
          const screenX = Math.min(viewportWidth - 1, Math.max(0, viewportWidth * (anchor.x + offset)))
          const screenY = Math.min(viewportHeight - 1, Math.max(0, viewportHeight * (anchor.y + .01)))
          const pixel = context.getImageData(Math.min(image.naturalWidth - 1, Math.max(0, Math.round((screenX - offsetX) / scale))), Math.min(image.naturalHeight - 1, Math.max(0, Math.round((screenY - offsetY) / scale))), 1, 1).data
          return srgbLuminance(pixel[0], pixel[1], pixel[2])
        }
        const left = sampleSide(-.14)
        const right = sampleSide(.14)
        if (!cancelled) setEnvironment({ rgb: `${average[0]} ${average[1]} ${average[2]}`, luminance: srgbLuminance(average[0], average[1], average[2]), lightAngle: right >= left ? '90deg' : '270deg', contrast: Math.min(.35, Math.abs(right - left)) })
      } catch {
        // External/custom URLs can be canvas-tainted. The fallback remains
        // intentionally subtle so those portraits never receive a hard mask.
        if (!cancelled) setEnvironment(FALLBACK)
      }
    }
    image.onerror = () => { if (!cancelled) setEnvironment(FALLBACK) }
    image.src = backgroundUrl
    return () => { cancelled = true }
  }, [anchor.x, anchor.y, backgroundUrl, sampleSize?.height, sampleSize?.width, viewport])

  useEffect(() => {
    if (sampleSize) return
    const refresh = () => setViewport(`${window.innerWidth}x${window.innerHeight}`)
    window.addEventListener('resize', refresh)
    return () => window.removeEventListener('resize', refresh)
  }, [])

  return environment
}
