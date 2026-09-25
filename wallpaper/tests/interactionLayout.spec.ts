import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * Two defects that pure logic could not catch, pinned where they happened.
 *
 * Both were *wiring* mistakes rather than wrong algorithms: a subscription that named a
 * preference in its dependency list, and a capsule that relied on a caller to size it.
 * The state machine and the CSS were each correct on their own, so the guards below
 * assert the structure that made them go wrong — cheap, and specific enough that a
 * future change has to mean it.
 */
const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

describe('boot is an event, not a preference', () => {
  it('dispatches the wake exactly once, from one place', async () => {
    const app = await source('src/App.tsx')
    // Exactly one dispatch per transport (native app-core, browser preview). If either
    // count grows, a second code path has learned to say "this desktop just woke up",
    // which is how the entrance animation came back on a settings change. Comments are
    // not counted, because prose is allowed to name the event.
    expect(app.match(/dispatchCore\(\s*'boot-ready'/g) ?? []).toHaveLength(1)
    expect(app.match(/type:\s*'BOOT_READY'/g) ?? []).toHaveLength(1)
  })

  it('never keys a subscription on an interaction preference', async () => {
    const app = await source('src/App.tsx')
    // The regression in one line: `}, [settings.interactionLayout])` re-ran the boot
    // effect, which re-dispatched the boot event. A preference may change what is
    // drawn; it must not be able to re-trigger "this session just started".
    expect(app).not.toMatch(/\},\s*\[[^\]]*settings\.interactionLayout/)
    expect(app).not.toMatch(/\},\s*\[[^\]]*settings\.skipWakeAnimation/)
    // The ref is what makes that safe: long-lived callbacks still read current values.
    expect(app).toContain('const settingsRef = useRef(settings)')
    expect(app).toContain('settingsRef.current = settings')
  })
})

describe('the collapsed island is a capsule, wherever it is rendered', () => {
  it('carries its own size instead of stretching to fill its parent', async () => {
    const css = await source('src/features/chat/ConversationBubble.css')
    // Comments explain the bug by quoting the values that caused it, so they must not
    // be searched: only the declarations are the contract here.
    const rule = (css.match(/\.dsh-chat-collapsed\s*\{[\s\S]*?\}/)?.[0] ?? '')
      .replace(/\/\*[\s\S]*?\*\//g, '')
    expect(rule).not.toBe('')
    // Stretching is what made the single-screen path paint a full-screen pill: there
    // the button's parent is `.wallpaper-root`, so 100% meant the whole desktop — and
    // a real button that big swallows every click meant for the wallpaper.
    expect(rule).not.toContain('inset: 0')
    expect(rule).not.toContain('height: 100%')
    expect(rule).toContain('height: 44px')
    expect(rule).toContain('width: min(190px')
    // Centring lives in the rule, so the hover lift must preserve it: a hover rule
    // that only set translateY would slide the capsule half its width sideways.
    expect(css).toMatch(/\.dsh-chat-collapsed:hover\s*\{[^}]*translateX\(-50%\)[^}]*translateY/)
  })
})
