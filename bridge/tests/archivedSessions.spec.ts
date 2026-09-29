import { describe, expect, it } from 'vitest'
import { visibleSessionIdSet } from '../src/index.ts'

/**
 * "这条会话还在不在"这个判断，来自 DSH 会话存储的可见集合。
 *
 * 用户实测报过一个临界情况：通过壁纸开始的会话，在桌面端被**归档**之后，壳里连归档去处都没有，
 * 而我们的活句柄还活着 —— 不报错、不掉线、灯照旧是绿的，消息于是发进一个再也看不见的会话里
 * （"吞输入"）。判断依据就是这里：DSH 自己把 `list()` 描述为 "one snapshot per **visible**
 * stored session"。
 *
 * 判错一次的代价是双向的：把还在的会话判死 → 用户莫名其妙丢上下文；把归档的会话判活 →
 * 消息继续进黑洞。所以形状读不懂时一律跳过，整体不是数组时回答"不可知"（`undefined`），
 * 调用方据此**不做任何判断**、照旧工作。
 */
describe('visible stored sessions', () => {
  /**
   * 真实形状（真机 2026-09-27 抓到的那一份，原样保留）：id 在 **`header.id`** 里。
   * 以前这里只读顶层 `id`/`sessionId`，81 份这样的快照被读成**空集合** —— 而空集合的语义是
   * "每一条会话都不可见"，于是归档判断对**所有**会话成立（活会话被判死：岛的开关切不回去）。
   */
  const realSnapshot = (id: string) => ({
    header: { version: 4, id, createdAt: 1786706891267, cwd: 'C:\\DeepSeekHarness', origin: 'subagent', isSeeded: false, delegationDepth: 1, agentPreset: 'standard' },
    revision: '917830321:562949955321754:256564',
    sizeBytes: 256564,
  })

  it('reads ids from the shape DSH really returns', () => {
    const ids = visibleSessionIdSet([realSnapshot('wallpaper-2026-09-27'), realSnapshot('wallpaper-2026-09-26')])
    expect([...ids ?? []]).toEqual(['wallpaper-2026-09-27', 'wallpaper-2026-09-26'])
  })

  it('still reads the flatter shapes instead of losing a session', () => {
    const ids = visibleSessionIdSet([
      { id: 'wallpaper-2026-09-27', title: '桌面会话' },
      { sessionId: 'wallpaper-2026-09-26' },
      { id: '  wallpaper-2026-09-25  ' },
    ])
    expect([...ids ?? []]).toEqual(['wallpaper-2026-09-27', 'wallpaper-2026-09-26', 'wallpaper-2026-09-25'])
  })

  it('skips anything it cannot read instead of guessing', () => {
    const ids = visibleSessionIdSet([
      null,
      42,
      'wallpaper-not-an-object',
      { title: '没有 id' },
      { id: '' },
      { id: '   ' },
      { sessionId: 7 },
      { id: 'wallpaper-2026-09-27' },
    ])
    expect([...ids ?? []]).toEqual(['wallpaper-2026-09-27'])
  })

  it('answers "unknown" when the store is unavailable or unreadable', () => {
    // 不可知 = 调用方不做任何判断：宁可照旧工作，也不要把好会话判死。
    expect(visibleSessionIdSet(undefined)).toBeUndefined()
    expect(visibleSessionIdSet(null)).toBeUndefined()
    expect(visibleSessionIdSet('sessions')).toBeUndefined()
    expect(visibleSessionIdSet({ sessions: [] })).toBeUndefined()
    // 空数组是**知道**的结果：一条可见会话都没有（全部被归档）。
    expect(visibleSessionIdSet([])?.size).toBe(0)
    // 但**非空却一个 id 都读不出来**不是"全都不可见"，而是"形状变了" —— 必须退回不可知。
    // 把它当成空集合，就是刚才真机上发生的事：每一条活会话都被判死。
    expect(visibleSessionIdSet([{ header: {} }, { header: { id: 7 } }, { unrelated: true }])).toBeUndefined()
  })
})
