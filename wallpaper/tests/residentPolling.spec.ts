import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { DISPLAY_LIST_FALLBACK_INTERVAL_MS } from '../src/settings/settingsProbes.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function readSource(relative: string): Promise<string> {
  return (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')
}

async function readNative(relative: string): Promise<string> {
  return (await readFile(resolve(wallpaperRoot, 'src-tauri', relative), 'utf8')).replace(/\r\n?/g, '\n')
}

describe('resident polling budget', () => {
  it('measures the expanded layout inset from events instead of every second', async () => {
    const app = await readSource('src/App.tsx')

    // The fallback cadence is the contract; the measurement itself is event
    // driven (display-changed, resize, workspace transitions).
    expect(app).toContain('export const LAYOUT_METRICS_FALLBACK_INTERVAL_MS = 30_000')
    expect(app).toContain('window.setInterval(refresh, LAYOUT_METRICS_FALLBACK_INTERVAL_MS)')
    expect(app).not.toContain('window.setInterval(refresh, 1000)')
    expect(app).not.toMatch(/setInterval\([^)]*,\s*1000\)/)
    // Display topology keeps a slow fallback too; `display-changed` is the
    // real trigger.
    expect(app).toContain('export const DISPLAY_TOPOLOGY_FALLBACK_INTERVAL_MS = 30_000')
    expect(app).toContain('window.setInterval(() => { void refresh() }, DISPLAY_TOPOLOGY_FALLBACK_INTERVAL_MS)')
    expect(app).not.toContain('window.setInterval(() => { void refresh() }, 5000)')
  })

  it('only polls the DSH launcher while a launch is actually pending', async () => {
    const app = await readSource('src/App.tsx')
    const effect = app.slice(app.indexOf('if (!nativeRuntime.isNative || !harnessLaunchPendingRef.current) return'))
    const body = effect.slice(0, effect.indexOf('}, [harnessStarting, runtime.harness])'))

    // The one remaining 1s poll is gated on a launch in flight and cleared on
    // every exit path (ready, exited, timeout, unmount).
    expect(body).toContain('window.setInterval(() => { void check() }, 1000)')
    expect(body).toContain('harnessLaunchPendingRef.current = false')
    expect(body).toContain('window.clearInterval(timer)')
    expect(app.match(/window\.setInterval\(/g) ?? []).toHaveLength(3)
  })

  it('only polls the settings monitor list while its page owns it', async () => {
    const settings = await readSource('src/settings/SettingsWindow.tsx')

    expect(DISPLAY_LIST_FALLBACK_INTERVAL_MS).toBe(30_000)
    expect(settings).toContain("if (!PAGE_PROBES[page].includes('desktopDisplays')) return")
    expect(settings).toContain('window.setInterval(() => { void refreshDesktopDisplays() }, DISPLAY_LIST_FALLBACK_INTERVAL_MS)')
    expect(settings).not.toContain('}, 5000)')
  })
})

describe('native exit lifecycle', () => {
  it('restores desktop state on exit and never conflates it with hiding to the tray', async () => {
    const lib = await readNative('src/lib.rs')

    // The run loop handles both exit events through one idempotent function.
    expect(lib).toMatch(/\.run\(\|app, event\| \{[\s\S]*?RunEvent::ExitRequested[\s\S]*?RunEvent::Exit[\s\S]*?shutdown_native_state\(app\)/)
    const shutdown = lib.slice(lib.indexOf('fn shutdown_native_state'))
    expect(shutdown).toContain('restore_desktop_icons()')
    expect(shutdown).toContain('native_bootstrap::destroy()')
    expect(shutdown).toContain('unregister_session_events(app)')
    // Only the DSH child this process launched is stopped.
    expect(shutdown).toContain('taskkill.exe')
    expect(shutdown).toContain('managed.take()')
    // Every step is inside a `Once`, because ExitRequested can precede Exit.
    expect(shutdown).toContain('SHUTDOWN.call_once')
    // The tray "hide" item is an action dispatch, not an exit.
    expect(lib).toMatch(/"hide" => \{\s*\n\s*dispatch_ui_action\(app, AppAction::SetInteractionEnabled\(false\), false\)/)
  })

  it('releases the session notification registration it installed', async () => {
    const integration = await readNative('src/windows_integration.rs')

    expect(integration).toContain('pub fn unregister_session_events(app: &tauri::AppHandle)')
    expect(integration).toMatch(/pub fn unregister_session_events\(app: &tauri::AppHandle\) \{[\s\S]*?WTSUnRegisterSessionNotification\(hwnd\)[\s\S]*?RemoveWindowSubclass\(hwnd, Some\(session_subclass_proc\), 1\)/)
    // Non-Windows builds still expose the symbol so the shared exit path
    // compiles without a cfg at the call site.
    expect(integration).toContain('pub fn unregister_session_events(_: &tauri::AppHandle) {}')
  })

  it('gives the native hand-off window a watchdog for a frontend that never reports', async () => {
    const bootstrap = await readNative('src/native_bootstrap.rs')

    expect(bootstrap).toContain('BOOTSTRAP_WATCHDOG_TIMEOUT')
    expect(bootstrap).toContain('fn arm_watchdog()')
    expect(bootstrap).toContain('fn settle_watchdog()')
    expect(bootstrap).toContain('pub fn destroy()')
    // Armed only after the hand-off window actually exists...
    expect(bootstrap).toMatch(/arm_watchdog\(\);\s*\n\}/)
    // ...and disarmed by both the renderer's frame report and a release.
    expect(bootstrap).toMatch(/BOOTSTRAP_READY_REPORTED\.swap\(true, Ordering::AcqRel\)[\s\S]{0,400}settle_watchdog\(\)/)
    expect(bootstrap).toMatch(/pub fn release\(\) -> Result<\(\), String> \{[\s\S]{0,200}settle_watchdog\(\)/)
    // On timeout it destroys rather than hiding, and records why.
    expect(bootstrap).toContain('outcome=watchdog-timeout')
    expect(bootstrap).toMatch(/outcome=watchdog-timeout[\s\S]{0,400}destroy\(\)/)
    expect(bootstrap).toContain('pub fn destroy() -> Result<(), String> {\n    Ok(())\n}')
  })
})
