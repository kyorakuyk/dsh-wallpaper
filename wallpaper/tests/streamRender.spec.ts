import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'
import {
  frameSchedulerTarget,
  growHistoryWindow,
  historyWindow,
  HISTORY_RENDER_WINDOW,
  StreamTextBuffer,
} from '../src/features/chat/streamRender.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** A frame clock a test drives by hand. */
function frameClock() {
  const frames = new Map<number, () => void>()
  let next = 1
  return {
    target: {
      requestAnimationFrame: (callback: () => void) => { const handle = next++; frames.set(handle, callback); return handle },
      cancelAnimationFrame: (handle: number) => { frames.delete(handle) },
    },
    /** Run the callbacks queued for this frame, exactly once. */
    runFrame() {
      const batch = [...frames.entries()]
      frames.clear()
      for (const [, callback] of batch) callback()
    },
    get pending() { return frames.size },
  }
}

describe('streaming delta batching', () => {
  it('coalesces every delta of one frame into a single commit in order', () => {
    const clock = frameClock()
    const commits: string[] = []
    const buffer = new StreamTextBuffer(clock.target, (text) => commits.push(text))

    buffer.append('一')
    buffer.append('二')
    buffer.append('三')
    // Nothing is committed until the frame runs: four deltas in one frame are
    // one state update, not four.
    expect(commits).toEqual([])
    expect(clock.pending).toBe(1)

    clock.runFrame()
    expect(commits).toEqual(['一二三'])
  })

  it('commits once per frame while a turn keeps streaming', () => {
    const clock = frameClock()
    const total: string[] = []
    let commits = 0
    const buffer = new StreamTextBuffer(clock.target, (text) => { commits += 1; total.push(text) })

    for (const delta of ['A', 'B', 'C']) {
      buffer.append(delta)
      clock.runFrame()
    }
    expect(commits).toBe(3)
    // Order is preserved exactly across frames.
    expect(total.join('')).toBe('ABC')
    // The visible text is always a prefix of the final answer.
    expect('ABC'.startsWith(total.join(''))).toBe(true)
  })

  it('always keeps at least one frame of coalescing for a burst', () => {
    const clock = frameClock()
    let commits = 0
    const buffer = new StreamTextBuffer(clock.target, () => { commits += 1 })
    for (let index = 0; index < 500; index += 1) buffer.append('x')
    expect(commits).toBe(0)
    clock.runFrame()
    expect(commits).toBe(1)
  })

  it('shows the newest delta before its frame is committed', () => {
    const clock = frameClock()
    const buffer = new StreamTextBuffer(clock.target, () => undefined)
    buffer.append('正在')
    expect(buffer.uncommitted).toBe('正在')
    clock.runFrame()
    expect(buffer.uncommitted).toBe('')
  })

  it('flush commits immediately and does not double-commit', () => {
    const clock = frameClock()
    const commits: string[] = []
    const buffer = new StreamTextBuffer(clock.target, (text) => commits.push(text))
    buffer.append('最终')
    expect(buffer.flush()).toBe('最终')
    expect(commits).toEqual(['最终'])
    // The cancelled frame must not commit the same text again.
    clock.runFrame()
    expect(commits).toEqual(['最终'])
  })

  it('reset drops queued text so a finished answer is not appended twice', () => {
    const clock = frameClock()
    const commits: string[] = []
    const buffer = new StreamTextBuffer(clock.target, (text) => commits.push(text))
    buffer.append('半句')
    buffer.reset()
    expect(buffer.uncommitted).toBe('')
    clock.runFrame()
    expect(commits).toEqual([])
  })

  it('provides a timer fallback when the host has no animation frames', () => {
    vi.useFakeTimers()
    try {
      const target = frameSchedulerTarget()
      expect(typeof target.requestAnimationFrame).toBe('function')
      const commits: string[] = []
      const buffer = new StreamTextBuffer(target, (text) => commits.push(text))
      buffer.append('降级')
      expect(commits).toEqual([])
      vi.advanceTimersByTime(20)
      expect(commits).toEqual(['降级'])
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('history render window', () => {
  it('renders the newest window and reports how many are hidden', () => {
    const view = historyWindow(250, HISTORY_RENDER_WINDOW)
    expect(view.visible).toBe(100)
    expect(view.hidden).toBe(150)
    expect(view.hasEarlier).toBe(true)
  })

  it('does not window a short transcript', () => {
    expect(historyWindow(12, HISTORY_RENDER_WINDOW)).toEqual({ visible: 12, hidden: 0, hasEarlier: false })
    expect(historyWindow(0, HISTORY_RENDER_WINDOW)).toEqual({ visible: 0, hidden: 0, hasEarlier: false })
  })

  it('never renders more messages than exist, even for an absurd request', () => {
    expect(historyWindow(5, 10_000)).toEqual({ visible: 5, hidden: 0, hasEarlier: false })
  })

  it('grows the window to the whole transcript without repeating a step', () => {
    expect(growHistoryWindow(HISTORY_RENDER_WINDOW, 250)).toBe(200)
    expect(growHistoryWindow(200, 250)).toBe(250)
    expect(growHistoryWindow(250, 250)).toBe(250)
    // A window smaller than the default still grows by at least one step, so
    // "load earlier" can never be a no-op button.
    expect(growHistoryWindow(10, 250)).toBe(HISTORY_RENDER_WINDOW)
  })
})

describe('streaming render wiring', () => {
  it('coalesces deltas in the app and keeps the bubble a pure view', async () => {
    const app = (await readFile(resolve(wallpaperRoot, 'src/App.tsx'), 'utf8')).replace(/\r\n?/g, '\n')
    const bubble = (await readFile(resolve(wallpaperRoot, 'src/features/chat/ConversationBubble.tsx'), 'utf8')).replace(/\r\n?/g, '\n')

    // The delta handler buffers instead of committing one render per chunk.
    expect(app).toContain('streamBuffer.append(event.text)')
    expect(app).not.toContain('setStreamingText((value) => value + event.text)')
    // A terminal assistant message drops anything still queued for a frame.
    expect(app).toContain('streamBuffer.reset()')
    // The transcript is windowed in the DOM only.
    expect(bubble).toContain('historyWindow(props.messages.length, historyLimit)')
    // 「加载更早」那一枚按钮还在（文案已搬进字典，所以这里钉的是调用处 + 词条本身 ——
    // 与 dshAutostart.spec.ts 里"面板只剩键，句子去字典里核对"同一条规矩）。
    expect(bubble).toContain("t('chat.bubble.history.load-earlier'")
    const dictionary = (await readFile(resolve(wallpaperRoot, 'src/i18n/zh.ts'), 'utf8')).replace(/\r\n?/g, '\n')
    expect(dictionary).toContain('加载更早的 {hidden} 条记录')
    expect(bubble).toContain('props.messages.slice(props.messages.length - historyView.visible)')
    // The live answer still renders straight from the prop.
    expect(bubble).toContain('const streamText = props.streamingText')
  })
})
