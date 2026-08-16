import { describe, expect, it, vi, afterEach } from 'vitest'
import { createHarnessProbe } from '../src/connect/probe.ts'

function mockFetch(sequence: boolean[]) {
  let i = 0
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => {
      const ok = sequence[Math.min(i, sequence.length - 1)]
      i += 1
      if (ok) return { type: 'opaque' } as Response
      throw new Error('offline')
    }),
  )
}

describe('3080 探测（去抖）', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it('连续 settle 次在线才触发 onOnline', async () => {
    vi.useFakeTimers()
    mockFetch([true, true, true])
    const onOnline = vi.fn()
    const onOffline = vi.fn()
    const handle = createHarnessProbe(onOnline, onOffline, { interval: 100, settle: 2 })

    await vi.advanceTimersByTimeAsync(250) // 首探 + 2 次 tick
    expect(onOnline).toHaveBeenCalledTimes(1)
    expect(onOffline).not.toHaveBeenCalled()
    handle.stop()
  })

  it('连续 settle 次离线才触发 onOffline（且不下线时保持在线）', async () => {
    vi.useFakeTimers()
    mockFetch([true, true, true, false, false, false])
    const onOnline = vi.fn()
    const onOffline = vi.fn()
    const handle = createHarnessProbe(onOnline, onOffline, { interval: 100, settle: 2 })

    await vi.advanceTimersByTimeAsync(250) // 3 次在线 → online
    await vi.advanceTimersByTimeAsync(300) // 3 次离线 → offline
    expect(onOnline).toHaveBeenCalledTimes(1)
    expect(onOffline).toHaveBeenCalledTimes(1)
    handle.stop()
  })

  it('单次抖动不触发切换', async () => {
    vi.useFakeTimers()
    mockFetch([true, false, true, true, true])
    const onOnline = vi.fn()
    const onOffline = vi.fn()
    const handle = createHarnessProbe(onOnline, onOffline, { interval: 100, settle: 2 })

    await vi.advanceTimersByTimeAsync(500)
    expect(onOnline).toHaveBeenCalledTimes(1)
    expect(onOffline).not.toHaveBeenCalled() // 单次 false 未达 settle
    handle.stop()
  })
})
