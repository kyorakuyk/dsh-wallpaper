import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

const storage = new Map<string, string>()
vi.stubGlobal('localStorage', {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => storage.set(key, value),
})

beforeAll(() => {
  // App.tsx also exposes pure lifecycle helpers, but its native clients read
  // `window` while the module is initialized.
  if (!('window' in globalThis)) vi.stubGlobal('window', {})
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

  it('only starts a fresh web route when the selected policy has no resumable pointer', async () => {
    const { shouldStartNewWebConversation } = await import('../src/App.tsx')
    expect(shouldStartNewWebConversation('resume-last', undefined)).toBe(false)
    expect(shouldStartNewWebConversation('resume-last', 'saved-web')).toBe(false)
    expect(shouldStartNewWebConversation('daily', 'saved-web')).toBe(false)
    expect(shouldStartNewWebConversation('daily', undefined)).toBe(true)
    expect(shouldStartNewWebConversation('new-on-unlock', 'saved-web')).toBe(true)
  })

  it('does not persist route-unsafe conversation pointers', async () => {
    const { isValidConversationId, resumeConversationId, saveConversationPointer } = await import('../src/settings/store.ts')
    expect(isValidConversationId('saved-web')).toBe(true)
    expect(isValidConversationId('saved/web')).toBe(false)
    saveConversationPointer('deepseek-web', 'saved/web')
    expect(resumeConversationId('deepseek-web', 'resume-last')).toBeUndefined()
  })
})
