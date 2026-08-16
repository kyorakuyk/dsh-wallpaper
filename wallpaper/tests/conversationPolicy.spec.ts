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

  it('does not resume a stale daily pointer', async () => {
    const { resumeConversationId } = await import('../src/settings/store.ts')
    storage.set('dsh-wallpaper:conversations:v1', JSON.stringify({ harness: { id: 'old', updatedAt: 0, day: '2000-01-01' } }))
    expect(resumeConversationId('harness', 'daily')).toBeUndefined()
  })
})
