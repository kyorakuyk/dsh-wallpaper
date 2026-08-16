import { describe, expect, it } from 'vitest'
import { nextState, TRANSITIONS, WallpaperStateMachine } from '../src/scenes/stateMachine.ts'

describe('状态迁移表', () => {
  it('覆盖所有已声明迁移', () => {
    // 每一条迁移都能从 from 经 event 到达 to
    for (const t of TRANSITIONS) {
      const froms = Array.isArray(t.from) ? t.from : [t.from]
      for (const f of froms) {
        expect(nextState(f, t.event)).toBe(t.to)
      }
    }
  })

  it('非法转移保持原状态', () => {
    expect(nextState('sleep', 'wakeDone')).toBe('sleep')
    expect(nextState('waking', 'openChat')).toBe('waking')
    expect(nextState('idle', 'wakeDone')).toBe('idle')
  })
})

describe('状态机实例', () => {
  it('完整链路: sleep → waking → idle → chat → idle', () => {
    const m = new WallpaperStateMachine()
    const log: string[] = []
    m.subscribe((s) => log.push(s))
    expect(m.current).toBe('sleep')
    m.dispatch('unlock') // → waking
    m.dispatch('wakeDone') // → idle
    m.dispatch('openChat') // → chat
    m.dispatch('closeChat') // → idle
    expect(log).toEqual(['waking', 'idle', 'chat', 'idle'])
  })

  it('sleep 可从 idle/chat 触发', () => {
    const m = new WallpaperStateMachine()
    m.dispatch('unlock')
    m.dispatch('wakeDone') // idle
    m.dispatch('sleep') // → sleep
    expect(m.current).toBe('sleep')
  })
})
