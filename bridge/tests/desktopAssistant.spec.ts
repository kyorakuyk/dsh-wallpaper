import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  DEFAULT_DAY_BOUNDARY_HOUR,
  MAX_PROJECT_MEMORY_CHARS,
  PROJECT_MEMORY_FILE_NAME,
  assistantDay,
  configuredDayBoundaryHour,
  desktopEntryPrompt,
  localDailyWallpaperSessionId,
  readProjectMemory,
} from '../src/index.ts'

/**
 * 「助手日」：用户定的规则是**本地 04:00 跨日**，不是零点 —— 深夜还在做的事在他心里"今天"
 * 还没过去（游戏里的跨日缓冲是同一个道理）。这条规则有两侧实现（这里与壁纸设置里的
 * `assistantDay`），所以纯函数的边界必须钉死：判错一次就会把"同一段对话"变成两天。
 */
describe('assistant day (04:00 boundary)', () => {
  it('treats the small hours as the previous day', () => {
    expect(assistantDay(new Date(2026, 8, 27, 0, 30))).toBe('2026-09-26')
    expect(assistantDay(new Date(2026, 8, 27, 3, 59))).toBe('2026-09-26')
    expect(assistantDay(new Date(2026, 8, 27, 4, 0))).toBe('2026-09-27')
    // 跨月与跨年：都是"往前挪 4 小时再取日历日"。
    expect(assistantDay(new Date(2026, 9, 1, 1, 0))).toBe('2026-09-30')
    expect(assistantDay(new Date(2027, 0, 1, 2, 0))).toBe('2026-12-31')
    // 0 是合法的：退回"零点跨日"的旧行为。
    expect(assistantDay(new Date(2026, 8, 27, 0, 30), 0)).toBe('2026-09-27')
    expect(assistantDay(new Date(2026, 8, 27, 0, 30), Number.NaN)).toBe('2026-09-26')
  })

  it('names the daily session after the assistant day and the configured boundary', () => {
    expect(localDailyWallpaperSessionId(new Date(2026, 8, 27, 1, 0))).toBe('wallpaper-2026-09-26')
    expect(localDailyWallpaperSessionId(new Date(2026, 8, 27, 5, 0))).toBe('wallpaper-2026-09-27')
    expect(configuredDayBoundaryHour({})).toBe(DEFAULT_DAY_BOUNDARY_HOUR)
    expect(configuredDayBoundaryHour({ dayBoundaryHour: 2 })).toBe(2)
    expect(configuredDayBoundaryHour({ dayBoundaryHour: 99 })).toBe(23)
    expect(configuredDayBoundaryHour({ dayBoundaryHour: -1 })).toBe(0)
    expect(configuredDayBoundaryHour({ dayBoundaryHour: Number.NaN })).toBe(4)
  })
})

/**
 * 桌面简报：它是"你是谁、能做什么、边界在哪、怎么改人格、记忆在哪"的唯一注入点，也只注入
 * 桌面工作区这一处的会话（用户定的范围：别的目录的对话不受影响）。
 */
describe('desktop briefing', () => {
  it('states the real read/write boundary instead of implying more', () => {
    const text = desktopEntryPrompt('C:\\data\\桌面会话', '桌面会话', 'workspace-write', { day: '2026-09-26' })
    expect(text).toContain('C:\\data\\桌面会话')
    expect(text).toContain('workspace-write')
    // 边界必须写清楚：读放行、写只在工作区内、越界被拒时不要重试。
    expect(text).toContain('越界写会被沙箱直接拒绝')
    expect(text).toContain('不要反复重试')
    expect(text).toContain('2026-09-26')
    // 说话人格与外观是两件事、两个入口 —— 用户 2026-09-27 明确过。
    expect(text).toContain('说话人格')
    expect(text).toContain('外观')
    expect(text).toContain('设置中心')
    // 路径只给它自己看：桌面回复里不许贴绝对路径，改记忆的入口在设置里（用户 2026-09-27 提的）。
    expect(text).toContain('不要在桌面回复里贴绝对路径')
    expect(text).toContain('打开项目记忆')
  })

  it('injects the project memory when there is one, and says nothing when there is not', () => {
    const withoutMemory = desktopEntryPrompt('C:\\w', '桌面会话', 'workspace-write', { memoryPath: 'C:\\w\\项目记忆.md' })
    expect(withoutMemory).toContain('C:\\w\\项目记忆.md')
    expect(withoutMemory).not.toContain('【项目记忆内容】')

    const withMemory = desktopEntryPrompt('C:\\w', '桌面会话', 'workspace-write', {
      memoryPath: 'C:\\w\\项目记忆.md',
      memory: '说话像猫娘；改文件前先问我。',
    })
    expect(withMemory).toContain('【项目记忆内容】')
    expect(withMemory).toContain('说话像猫娘')
  })
})

describe('project memory file', () => {
  it('reads it, caps it with an honest note, and treats absence as none', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'wallpaper-memory-'))
    try {
      // 没有文件 = 没有记忆：它从来不是创建会话的前提。
      expect(await readProjectMemory(directory)).toBeUndefined()
      await writeFile(join(directory, PROJECT_MEMORY_FILE_NAME), '  改文件前先问我。  ', 'utf8')
      expect(await readProjectMemory(directory)).toBe('改文件前先问我。')
      await writeFile(join(directory, PROJECT_MEMORY_FILE_NAME), '   ', 'utf8')
      expect(await readProjectMemory(directory)).toBeUndefined()
      // 超长要截断，并且**说明**截断了 —— 不静默丢内容。
      await writeFile(join(directory, PROJECT_MEMORY_FILE_NAME), '记'.repeat(MAX_PROJECT_MEMORY_CHARS + 50), 'utf8')
      const capped = await readProjectMemory(directory)
      expect(capped?.startsWith('记')).toBe(true)
      expect(capped).toContain('已截断')
      expect(capped?.length).toBeLessThan(MAX_PROJECT_MEMORY_CHARS + 200)
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  })
})
