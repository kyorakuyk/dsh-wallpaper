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
    expect(shutdown).toContain('stop_process_tree(')
    expect(shutdown).toContain('managed.take()')
    // 而且要**安静地**停：taskkill 是控制台程序，从 GUI 进程起它时若不带 CREATE_NO_WINDOW，
    // 退出时会闪一个黑框；用 `.output()` 还会为它建管道并一直等到它结束。
    const clientWindow = await readNative('src/client_window.rs')
    const stopHelper = clientWindow.slice(
      clientWindow.indexOf('pub(crate) fn stop_process_tree'),
      clientWindow.indexOf('/// Exposed within the crate'),
    )
    expect(stopHelper).toContain('CREATE_NO_WINDOW')
    expect(stopHelper).toContain('.status()')
    expect(stopHelper).not.toContain('.output()')
    // 换了主体，"连接"这个词说的就是另一个对象：探测器必须把上一条连接的历史作废，否则界面会
    // 一直写着"已连接"（实测症状：呼吸灯 + bridge 已连接，而一发消息就说会话尚未建立）。
    const monitorLoop = lib.slice(lib.indexOf('fn start_harness_monitor'))
    expect(monitorLoop).toContain('harness_scope_key()')
    expect(monitorLoop).toContain('monitor = HarnessMonitorState::default()')
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
    const handoff = await readNative('src/native_handoff.rs')

    expect(bootstrap).toContain('BOOTSTRAP_WATCHDOG_TIMEOUT')
    expect(bootstrap).toContain('fn arm_watchdog(generation: u64)')
    expect(bootstrap).toContain('fn settle_watchdog(generation: u64)')
    expect(bootstrap).toContain('fn disarm_watchdog()')
    expect(bootstrap).toContain('BOOTSTRAP_WATCHDOG_THREAD_STARTED')
    expect(bootstrap).toContain('pub fn destroy()')
    // Startup, later unlocks, and host changes arm the same epoch-aware wait.
    expect(bootstrap).toMatch(/arm_watchdog\(generation\);/)
    expect(bootstrap).toContain('arm_watchdog(generation)')
    // A successful renderer hand-off settles only its own generation.
    expect(bootstrap).toContain('pub fn release(generation: u64, background_raw: isize)')
    expect(bootstrap).toContain('settle_watchdog(generation)')
    // The state machine rejects late frames and stale watchdog expirations.
    expect(handoff).toContain('pub(crate) fn release(&mut self, generation: u64) -> bool')
    expect(handoff).toContain('pub(crate) fn expire(&mut self, generation: u64) -> bool')
    expect(handoff).toContain('an_old_frame_cannot_release_after_lock_and_unlock')
    // On timeout it posts a generation-scoped destroy and records why.
    expect(bootstrap).toContain('outcome=watchdog-timeout')
    expect(bootstrap).toContain('DESTROY_MESSAGE')
    expect(bootstrap).toContain('pub fn destroy() -> Result<(), String> {\n    Ok(())\n}')
  })
})
