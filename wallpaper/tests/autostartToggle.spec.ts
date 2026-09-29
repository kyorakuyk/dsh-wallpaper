import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { AutostartStatus } from '../src/native/runtime.ts'
import { autostartDetail, autostartKnown, autostartRefusalNotice } from '../src/settings/autostartCopy.ts'
import { createAutostartQueue } from '../src/settings/autostartQueue.ts'

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'src')

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, resolve, reject }
}

/** Let every pending microtask and the one macrotask behind them finish. */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

const status = (overrides: Partial<AutostartStatus> = {}): AutostartStatus => ({
  enabled: false,
  source: 'none',
  reason: null,
  ...overrides,
})

describe('autostart switch serialization', () => {
  it('sends the newest intent instead of dropping a toggle made while a write is in flight', async () => {
    // 关上之后不让打开了: the second half of that report was this switch
    // dropping a toggle because a request was already on the wire.
    const first = deferred<AutostartStatus>()
    const sent: boolean[] = []
    const settled: Array<{ enabled: boolean; requested: boolean }> = []
    const queue = createAutostartQueue({
      send: (enabled) => {
        sent.push(enabled)
        return enabled === false ? first.promise : Promise.resolve(status({ enabled: true, source: 'run' }))
      },
      onBusy: () => {},
      onSettled: (result, requested) => settled.push({ enabled: result.enabled, requested }),
      onError: () => {},
    })

    queue.request(false)
    expect(sent).toEqual([false])
    // The user changes their mind while Windows is still being asked.
    queue.request(true)
    expect(sent).toEqual([false])

    first.resolve(status({ enabled: false }))
    await flush()

    expect(sent).toEqual([false, true])
    // Each answer carries the value that produced it, so the caller can tell a
    // current answer from one a newer toggle has already superseded.
    expect(settled).toEqual([
      { enabled: false, requested: false },
      { enabled: true, requested: true },
    ])
  })

  it('stays busy while a newer request is still queued', async () => {
    const first = deferred<AutostartStatus>()
    const busy: boolean[] = []
    let calls = 0
    const queue = createAutostartQueue({
      send: () => {
        calls += 1
        return calls === 1 ? first.promise : Promise.resolve(status({ enabled: true }))
      },
      onBusy: (value) => busy.push(value),
      onSettled: () => {},
      onError: () => {},
    })

    queue.request(false)
    queue.request(true)
    first.resolve(status({ enabled: false }))
    await flush()

    // The switch must not become editable in the gap between the two writes.
    expect(busy.length).toBeGreaterThanOrEqual(2)
    expect(busy.slice(0, -1).every(Boolean)).toBe(true)
    expect(busy.at(-1)).toBe(false)
  })

  it('reports a failed write without pretending it was queued behind anything', async () => {
    const errors: unknown[] = []
    const queue = createAutostartQueue({
      send: () => Promise.reject(new Error('无法读取当前应用的 MSIX 包族名（Windows 错误码 122）。')),
      onBusy: () => {},
      onSettled: () => {},
      onError: (error) => errors.push(error),
    })

    queue.request(true)
    await flush()

    expect(errors).toHaveLength(1)
    expect(String(errors[0])).toContain('122')
  })
})

describe('autostart copy', () => {
  it('names the path that really carries autostart', () => {
    const viaRun = status({
      enabled: true,
      source: 'run',
      reason: 'Windows 启动任务不可用（参数错误。 (0x80070057)）；当前由当前用户启动项承载。',
    })
    expect(autostartDetail(viaRun)).toContain('当前用户启动项')
    expect(autostartDetail(viaRun)).toContain('0x80070057')
    expect(autostartDetail(status({ enabled: true, source: 'startup-task' }))).toContain('Windows 启动任务')
    expect(autostartDetail(status({ reason: '当前用户启动项里没有 DSH Wallpaper。' }))).toContain('没有 DSH Wallpaper')
  })

  it('tells "not read yet" apart from "really off"', () => {
    // Rust never reports `none` without naming what it looked at, so the
    // placeholder is the only state with neither. The 常规 page warned
    // 「壁纸开机自启未生效」 from that placeholder for an autostart that was on.
    expect(autostartKnown(status())).toBe(false)
    expect(autostartKnown(status({ reason: '当前用户启动项里没有 DSH Wallpaper。' }))).toBe(true)
    expect(autostartKnown(status({ enabled: true, source: 'run' }))).toBe(true)
    expect(autostartKnown(status({ enabled: true, source: 'startup-task' }))).toBe(true)
    expect(autostartDetail(status())).toContain('正在读取')
  })

  it('explains a refusal with the reason Rust reported', () => {
    const reason = 'Windows 启动任务不可用（参数错误。 (0x80070057)）；当前用户启动项里没有 DSH Wallpaper。'
    expect(autostartRefusalNotice(status({ reason }), true)).toBe(`开机自启没有打开：${reason}`)
    // The change took effect: nothing to explain.
    expect(autostartRefusalNotice(status({ enabled: true, source: 'run' }), true)).toBeNull()
    // No reason from Rust at all: only then may the page fall back to telling the
    // user where to look in Windows.
    expect(autostartRefusalNotice(status(), true)).toContain('系统设置')
    expect(autostartRefusalNotice(status({ enabled: true }), false)).toContain('关闭')
  })
})

describe('the settings pages refuse to guess', () => {
  it('routes every autostart change through the queue and the native reason', async () => {
    for (const file of ['settings/SettingsWindow.tsx', 'lite/LiteSettingsWindow.tsx']) {
      const source = await readFile(resolve(sourceRoot, file), 'utf8')
      expect(source).toContain('autostartQueue().request(')
      expect(source).toContain('autostartRefusalNotice(status, requested)')
      // The canned sentence sent every user to a Windows permission page that
      // was not the problem on the machine where this was diagnosed.
      expect(source).not.toContain('请检查系统启动应用权限')
    }
    const panel = await readFile(resolve(sourceRoot, 'settings/SettingsPanel.tsx'), 'utf8')
    expect(panel).toContain('autostartDetail(props.autostart)')
    // The 常规 page's 「未生效」 warning must not fire from the placeholder: the
    // state it reads only arrives when the 系统 page opens.
    expect(panel).toContain('autostartKnown(props.autostart) && !props.autostart.enabled')
  })
})
