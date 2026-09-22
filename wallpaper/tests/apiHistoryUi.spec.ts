import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nativeRuntime, type ApiConversationListing } from '../src/native/runtime.ts'
import { PAGE_PROBES, SETTINGS_PAGES, settingsProbeErrorMessage } from '../src/settings/settingsProbes.ts'
import { formatBytes, formatHistoryTime, historyPressure } from '../src/settings/SettingsPanel.tsx'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function readSource(relative: string): Promise<string> {
  return (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')
}

function fakeTauri(reply: (command: string, args: Record<string, unknown>) => unknown) {
  const invokes: Array<{ command: string; args: Record<string, unknown> }> = []
  vi.stubGlobal('window', {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        invokes.push({ command, args })
        return reply(command, args)
      },
    },
  })
  return invokes
}

const listing: ApiConversationListing = {
  conversations: [
    { id: 'api-1', messageCount: 4, bytes: 2_048, updatedAt: 1_700_000_000_000, firstMessageAt: 1, lastMessageAt: 1_700_000_000_000, active: true },
    { id: 'api-2', messageCount: 2, bytes: 512, updatedAt: 1_600_000_000_000, firstMessageAt: 2, lastMessageAt: 1_600_000_000_000, active: false },
  ],
  totalBytes: 2_560,
  totalMessages: 6,
  budgetBytes: 12 * 1024 * 1024,
  maxBytes: 16 * 1024 * 1024,
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('API history listing IPC', () => {
  it('maps the native listing and fills in missing numeric fields', async () => {
    const invokes = fakeTauri((command) => {
      expect(command).toBe('list_api_conversations')
      return {
        conversations: [{ id: 'api-1', messageCount: 4, bytes: 2_048, updatedAt: 5, firstMessageAt: 1, lastMessageAt: 5, active: true }],
        totalBytes: 2_048,
        totalMessages: 4,
        budgetBytes: 12 * 1024 * 1024,
        maxBytes: 16 * 1024 * 1024,
      }
    })

    const result = await nativeRuntime.listApiConversations()
    expect(invokes).toHaveLength(1)
    expect(result.conversations[0]).toMatchObject({ id: 'api-1', messageCount: 4, active: true })
    expect(result.totalBytes).toBe(2_048)
    expect(result.maxBytes).toBe(16 * 1024 * 1024)
  })

  it('degrades to an empty listing rather than throwing on a partial reply', async () => {
    fakeTauri(() => ({}))
    const result = await nativeRuntime.listApiConversations()
    expect(result.conversations).toEqual([])
    expect(result.totalBytes).toBe(0)
    expect(result.budgetBytes).toBe(0)
  })

  it('sends the conversation id when deleting and nothing when clearing', async () => {
    const invokes = fakeTauri((command) => (command === 'delete_api_conversation' ? true : 2))
    expect(await nativeRuntime.deleteApiConversation('api-1')).toBe(true)
    expect(await nativeRuntime.clearApiHistory()).toBe(2)
    expect(invokes).toEqual([
      { command: 'delete_api_conversation', args: { conversationId: 'api-1' } },
      { command: 'clear_api_history', args: {} },
    ])
  })
})

describe('history view model', () => {
  it('formats transcript sizes at a readable scale', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(-1)).toBe('0 B')
    expect(formatBytes(Number.NaN)).toBe('0 B')
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2_048)).toBe('2.0 KB')
    expect(formatBytes(12 * 1024 * 1024)).toBe('12.0 MB')
  })

  it('formats a same-day timestamp as a time and an older one as a date', () => {
    const now = new Date(2026, 8, 22, 15, 30)
    const earlierToday = new Date(2026, 8, 22, 9, 5).getTime()
    const older = new Date(2026, 7, 3, 7, 4).getTime()
    expect(formatHistoryTime(earlierToday, now)).toBe('今天 09:05')
    expect(formatHistoryTime(older, now)).toBe('2026-08-03 07:04')
    // Missing or unusable timestamps must not render `NaN` or `Invalid Date`.
    expect(formatHistoryTime(0, now)).toBe('—')
    expect(formatHistoryTime(Number.NaN, now)).toBe('—')
  })

  it('clamps archive pressure and never divides by a zero budget', () => {
    expect(historyPressure(0, 12 * 1024 * 1024)).toBe(0)
    expect(historyPressure(6 * 1024 * 1024, 12 * 1024 * 1024)).toBe(50)
    // Over budget reports 100, not a runaway percentage: the trimmer acts at
    // the budget, so the bar must not imply a state the user cannot reach.
    expect(historyPressure(20 * 1024 * 1024, 12 * 1024 * 1024)).toBe(100)
    expect(historyPressure(1_000, 0)).toBe(0)
    expect(historyPressure(Number.NaN, 1_000)).toBe(0)
  })
})

describe('history page wiring', () => {
  it('is a settings page that reads the archive only when opened', async () => {
    expect(SETTINGS_PAGES).toContain('history')
    expect([...PAGE_PROBES.history]).toEqual(['apiHistory'])

    const panel = await readSource('src/settings/SettingsPanel.tsx')
    const window = await readSource('src/settings/SettingsWindow.tsx')
    const nav = panel.slice(panel.indexOf('const pages:'), panel.indexOf(']\n\n/** `18.4 MB`'))
    expect(nav).toContain("id: 'history'")
    expect(nav).toContain("label: '历史'")
    // The listing is a page probe, so it starts when the page opens. It must
    // never be polled: an archive management view has no live state.
    expect(window).toMatch(/apiHistory: async \(\) => \{[\s\S]*?listApiConversations\(\)/)
    expect(window).not.toMatch(/setInterval\([^)]*refreshApiHistory/)
    expect(window).not.toMatch(/setInterval\([^)]*listApiConversations/)
  })

  it('offers per-conversation delete and a clear-all control', async () => {
    const panel = await readSource('src/settings/SettingsPanel.tsx')

    expect(panel).toContain('onDeleteApiConversation(conversation.id)')
    expect(panel).toContain('onClearApiHistory')
    expect(panel).toContain('清空全部 API 历史')
    // The page shows metadata only, never a message body.
    expect(panel).toContain('conversation.messageCount} 条消息')
    expect(panel).not.toMatch(/conversation\.messages/)
    // Destructive actions must say what they do not affect.
    expect(panel).toContain('不影响 DeepSeek 网页入口或 Harness 会话')
    // An empty archive must render an explanation, not a blank card.
    expect(panel).toContain('没有可删除的 API 会话记录')
    // The currently active transcript is marked so the user does not delete it
    // blind.
    expect(panel).toContain('history-row__badge')
    expect(panel).toContain('当前会话')
  })

  it('confirms before deleting and re-reads the archive afterwards', async () => {
    const window = await readSource('src/settings/SettingsWindow.tsx')

    // Both destructive paths confirm first, and both refresh even on failure
    // so a failed delete never leaves a row that looks deleted.
    expect(window).toMatch(/const deleteApiConversation = async[\s\S]*?window\.confirm\(/)
    expect(window).toMatch(/const clearApiHistory = async[\s\S]*?window\.confirm\(/)
    expect(window).toMatch(/const deleteApiConversation = async[\s\S]*?finally \{[\s\S]*?await refreshApiHistory\(\)/)
    expect(window).toMatch(/const clearApiHistory = async[\s\S]*?finally \{[\s\S]*?await refreshApiHistory\(\)/)
    // A double click cannot start a second archive rewrite.
    expect(window).toMatch(/const deleteApiConversation = async[\s\S]*?if \(apiHistoryOperationRef\.current\) return/)
    expect(window).toMatch(/const clearApiHistory = async[\s\S]*?if \(apiHistoryOperationRef\.current\) return/)
  })

  it('reports a listing failure as a notice instead of a blank page', () => {
    expect(settingsProbeErrorMessage('apiHistory', new Error('decrypt failed')))
      .toBe('读取 API 会话记录失败：Error: decrypt failed')
  })
})

describe('API history command boundary', () => {
  it('grants the management commands to exactly the two owning WebViews', async () => {
    const readCapability = async (name: 'background' | 'settings') => JSON.parse(
      await readFile(resolve(wallpaperRoot, 'src-tauri', 'capabilities', `${name}.json`), 'utf8'),
    ) as { windows: string[]; permissions: string[] }

    const background = await readCapability('background')
    const settings = await readCapability('settings')
    const management = ['allow-list-api-conversations', 'allow-delete-api-conversation', 'allow-clear-api-history']

    // The settings center owns the delete UI, so it must be granted...
    for (const permission of management) expect(settings.permissions).toContain(permission)
    // ...and the wallpaper keeps them for its own archive handling.
    for (const permission of management) expect(background.permissions).toContain(permission)

    // No third window may reach the archive. Every declared window is one of
    // the two owners.
    expect(background.windows).toEqual(['background'])
    expect(settings.windows).toEqual(['settings'])
  })

  it('gates the management commands behind the shared owner check', async () => {
    const lib = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'lib.rs'), 'utf8')
    const build = await readFile(resolve(wallpaperRoot, 'src-tauri', 'build.rs'), 'utf8')

    // Both commands use one explicit owner check, not `require_background`:
    // the settings WebView is a distinct window label.
    for (const command of ['list_api_conversations', 'delete_api_conversation', 'clear_api_history']) {
      expect(build).toContain(`"${command}"`)
      const signature = new RegExp(`async fn ${command}\\s*\\([\\s\\S]*?\\)\\s*->[^\\{]*\\{[\\s\\S]*?require_api_history_owner\\(&caller\\)\\?;`)
      expect(lib, command).toMatch(signature)
    }
    expect(lib).toMatch(/fn require_api_history_owner\(caller: &tauri::WebviewWindow\) -> Result<\(\), String> \{[\s\S]*?BACKGROUND_WINDOW_LABEL \| SETTINGS_WINDOW_LABEL => Ok\(\(\)\)/)
    // Reading the archive for a listing must not be a way to publish bodies:
    // the listing command returns metadata only.
    expect(lib).toContain('chat::list_api_conversations(&state)')
  })
})
