/** 3080 端口探测：轮询 DSH web 是否在线，事件去抖后回调 */

export interface ProbeOptions {
  /** 探测间隔 ms */
  interval?: number
  /** 请求超时 ms */
  timeout?: number
  /** 触发状态变化前需连续稳定次数 */
  settle?: number
  /** DSH 特征校验路径（默认探测根路径即可） */
  url?: string
}

export interface ProbeHandle {
  stop: () => void
  /** 立即探测一次 */
  probeNow: () => Promise<boolean>
}

/** 探测 DSH web（3080）：HTTP GET 成功即在线；连续 settle 次一致才回调 */
export function createHarnessProbe(
  onOnline: () => void,
  onOffline: () => void,
  opts: ProbeOptions = {},
): ProbeHandle {
  const { interval = 3000, timeout = 1200, settle = 2, url = 'http://127.0.0.1:3080' } = opts

  let onlineStreak = 0
  let offlineStreak = 0
  let current = false
  let timer: ReturnType<typeof setInterval> | null = null
  let stopped = false

  const probeNow = async (): Promise<boolean> => {
    try {
      const res = await fetch(url, { signal: AbortSignal.timeout(timeout), mode: 'no-cors' })
      return res.type === 'opaque' || res.ok
    } catch {
      return false
    }
  }

  const tick = async (): Promise<void> => {
    if (stopped) return
    const ok = await probeNow()
    if (ok) {
      onlineStreak += 1
      offlineStreak = 0
      if (!current && onlineStreak >= settle) {
        current = true
        onOnline()
      }
    } else {
      offlineStreak += 1
      onlineStreak = 0
      if (current && offlineStreak >= settle) {
        current = false
        onOffline()
      }
    }
  }

  timer = setInterval(() => void tick(), interval)
  void tick() // 立即首探

  return {
    stop: () => {
      stopped = true
      if (timer !== null) clearInterval(timer)
    },
    probeNow,
  }
}
