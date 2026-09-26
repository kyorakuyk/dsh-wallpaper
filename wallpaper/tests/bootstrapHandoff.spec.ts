import { afterEach, describe, expect, it, vi } from 'vitest'
import { decodeImageSource, releaseAfterPreparedFrame, reportNativeBootstrapReady } from '../src/native/bootstrapHandoff.ts'

class MockImage {
  complete = false
  naturalWidth = 0
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  decode: () => Promise<void> = async () => { this.naturalWidth = 32 }
  set src(_value: string) {
    this.complete = true
    queueMicrotask(() => this.onload?.())
  }
}

describe('native startup hand-off', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it('accepts only successfully decoded frame images', async () => {
    vi.stubGlobal('Image', MockImage)
    expect(await decodeImageSource('/wake.png')).toBe(true)

    vi.stubGlobal('Image', class extends MockImage {
      decode = async () => { throw new Error('decode failed') }
    })
    expect(await decodeImageSource('/broken.png')).toBe(false)
  })

  it('does not request release if image decode times out', async () => {
    vi.useFakeTimers()
    vi.stubGlobal('Image', class extends MockImage {
      decode = () => new Promise<void>(() => undefined)
    })
    const result = decodeImageSource('/slow.png')
    await vi.advanceTimersByTimeAsync(1200)
    expect(await result).toBe(false)
  })

  it('retries temporary native host readiness but stays within the same generation', async () => {
    const release = vi.fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
    const currentGeneration = vi.fn(async () => 7)
    const requestFrame = vi.fn(async () => true)

    expect(await releaseAfterPreparedFrame(7, release, currentGeneration, undefined, requestFrame)).toBe(true)
    expect(release).toHaveBeenCalledTimes(2)
    expect(requestFrame).toHaveBeenCalledTimes(4)
  })

  it('rejects a stale generation instead of releasing a newer lock/unlock cover', async () => {
    const release = vi.fn(async () => false)
    const currentGeneration = vi.fn(async () => 9)
    const requestFrame = vi.fn(async () => true)

    expect(await releaseAfterPreparedFrame(8, release, currentGeneration, undefined, requestFrame)).toBe(false)
    expect(release).toHaveBeenCalledTimes(1)
    expect(requestFrame).toHaveBeenCalledTimes(2)
  })

  it('uses the caller-provided edition-local native bridge', async () => {
    vi.stubGlobal('window', {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        queueMicrotask(() => callback(0))
        return 1
      },
      cancelAnimationFrame: vi.fn(),
    })
    const control = {
      releaseNativeBootstrap: vi.fn()
        .mockResolvedValueOnce(false)
        .mockResolvedValueOnce(true),
      nativeBootstrapGeneration: vi.fn(async () => 11),
    }

    expect(await reportNativeBootstrapReady(11, control, { verifySceneImages: false })).toBe(true)
    expect(control.releaseNativeBootstrap).toHaveBeenCalledTimes(2)
    expect(control.nativeBootstrapGeneration).toHaveBeenCalledOnce()
  })
})
