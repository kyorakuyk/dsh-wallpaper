import { afterEach, describe, expect, it, vi } from 'vitest'

const storage = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => storage.set(key, value),
})

describe('conversation lifecycle policy', () => {
  afterEach(() => storage.clear())

  it('resumes the latest backend pointer except for new-on-unlock', async () => {
    const { resumeConversationId, saveConversationPointer } = await import('../src/settings/store.ts')
    saveConversationPointer('harness', 'session-a')
    expect(resumeConversationId('harness', 'resume-last')).toBe('session-a')
    expect(resumeConversationId('harness', 'daily')).toBe('session-a')
    expect(resumeConversationId('harness', 'new-on-unlock')).toBeUndefined()
  })

  it('does not resume a Harness pointer created before the bridge context fix', async () => {
    storage.set('dsh-wallpaper:conversations:v1', JSON.stringify({
      harness: { id: 'old-harness-session', updatedAt: Date.now(), day: '2026-08-22' },
    }))
    const { resumeConversationId } = await import('../src/settings/store.ts')
    expect(resumeConversationId('harness', 'resume-last')).toBeUndefined()
  })

  it('does not resume a stale daily pointer', async () => {
    const { resumeConversationId } = await import('../src/settings/store.ts')
    storage.set('dsh-wallpaper:conversations:v1', JSON.stringify({ harness: { id: 'old', updatedAt: 0, day: '2000-01-01' } }))
    expect(resumeConversationId('harness', 'daily')).toBeUndefined()
  })

  it('uses the local calendar day instead of UTC for daily pointers', async () => {
    const { localCalendarDay, resumeConversationId, saveConversationPointer } = await import('../src/settings/store.ts')
    // 00:30 in UTC+08 is still the previous UTC date. The policy must not
    // accidentally discard a just-created local-day conversation.
    const localMidnight = new Date(2026, 7, 18, 0, 30, 0)
    saveConversationPointer('deepseek-api', 'local-day', localMidnight)
    expect(localCalendarDay(localMidnight)).toBe('2026-08-18')
    expect(resumeConversationId('deepseek-api', 'daily', localMidnight)).toBe('local-day')
  })
})
