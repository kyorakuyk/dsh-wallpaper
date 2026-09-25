import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { beforeAll, describe, expect, it } from 'vitest'
import { DEFAULT_SETTINGS, normalizeSettings } from '../src/settings/store.ts'

// AppCore's browser preview fallback is evaluated when App.tsx is imported, and
// App.tsx evaluates a frame scheduler against `window`. These are pure-logic
// tests, so a minimal window is enough (same approach as the chat lifecycle
// suite) and avoids mounting a desktop surface to check two pure functions.
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

async function appModule() {
  return import('../src/App.tsx')
}

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

describe('DSH autostart outcome reporting', () => {
  it('stays silent for outcomes that need no user action', async () => {
    const { dshAutostartNotice } = await appModule()
    // `started` and `already-attempted` are normal; `port-occupied-external`
    // means someone else's DSH owns 3080, which the user must not be told to
    // "fix" — the wallpaper just probes it instead.
    for (const outcome of ['started', 'already-attempted', 'port-occupied-external'] as const) {
      expect(dshAutostartNotice({ outcome, external: outcome === 'port-occupied-external' })).toBeNull()
    }
  })

  it('names the concrete fix for every correctable failure', async () => {
    const { dshAutostartNotice } = await appModule()
    const cases: Array<[Parameters<typeof dshAutostartNotice>[0]['outcome'], string]> = [
      ['root-path-missing', '尚未选择执行主体'],
      ['root-path-invalid', '不是可识别的 DSH 项目'],
      ['launcher-missing', '未找到 Node.js 或 pnpm'],
      ['profile-invalid', 'profile 名称无效'],
      ['command-not-confirmed', '不会在无人值守时执行自定义启动命令'],
      ['spawn-failed', '进程启动失败'],
    ]
    for (const [outcome, expected] of cases) {
      const notice = dshAutostartNotice({ outcome, external: false })
      expect(notice, outcome).toContain(expected)
      expect(notice, outcome).toContain('设置中心')
    }
  })

  it('never leaks a path, a token, or an exception body', async () => {
    const { dshAutostartNotice } = await appModule()
    const outcomes = ['started', 'already-attempted', 'root-path-missing', 'root-path-invalid',
      'launcher-missing', 'profile-invalid', 'port-occupied-external', 'command-not-confirmed', 'spawn-failed'] as const
    for (const outcome of outcomes) {
      const notice = dshAutostartNotice({ outcome, external: false }) ?? ''
      expect(notice, outcome).not.toMatch(/[A-Za-z]:\\/)
      expect(notice, outcome).not.toMatch(/Bearer|token|Error:|at /i)
    }
  })
})

describe('launch supervision', () => {
  it('reports nothing while the launch is legitimately pending', async () => {
    const { harnessLaunchOutcome } = await appModule()
    expect(harnessLaunchOutcome({ availability: 'offline' }, 1_000, { managed: true, running: true })).toBeNull()
    expect(harnessLaunchOutcome({ availability: 'bridge-loading' }, 44_999, { managed: true, running: true })).toBeNull()
    // A ready Bridge ends the supervision regardless of the clock.
    expect(harnessLaunchOutcome({ availability: 'bridge-ready' }, 999_999, { managed: true, running: true })).toBeNull()
  })

  it('reports an immediate exit as soon as the managed process is gone', async () => {
    const { harnessLaunchOutcome } = await appModule()
    // This must not wait for the timeout: a process that died is already known.
    expect(harnessLaunchOutcome({ availability: 'offline' }, 500, { managed: true, running: false })?.message)
      .toContain('立即退出')
    expect(harnessLaunchOutcome({ availability: 'offline' }, 500, { managed: false, running: false })?.message)
      .toContain('立即退出')
  })

  it('prefers the most specific cause once the deadline passes', async () => {
    const { harnessLaunchOutcome } = await appModule()
    const late = (availability: 'offline' | 'web-only' | 'bridge-loading' | 'bridge-auth-unavailable' | 'bridge-incompatible') =>
      harnessLaunchOutcome({ availability }, 45_001, { managed: true, running: true })?.message ?? ''
    // A Bridge that answered but cannot be used needs a different fix from one
    // that never appeared, so the generic timeout must not swallow it.
    expect(late('bridge-auth-unavailable')).toContain('令牌不可用')
    expect(late('bridge-incompatible')).toContain('不兼容')
    expect(late('bridge-loading')).toContain('仍在装载')
    expect(late('offline')).toContain('启动超时')
    expect(late('web-only')).toContain('启动超时')
  })
})

describe('every condition the plan requires an actionable message for', () => {
  /**
   * Plan §6 lists the conditions that must each produce an actionable message:
   * DSH not running, only the Web UI, a missing or stale Bridge, token/ACL
   * unavailable, an unsupported host version, an invalid config path, and 3080
   * held by an external service. This test asserts the coverage as one set so a
   * future state cannot be added to the union without wording.
   */
  it('has non-empty, distinct wording for every Harness state', async () => {
    const { HARNESS_STATE_DETAILS, harnessStateLabel } = await import('../src/connect/harnessLabels.ts')
    const states = ['offline', 'web-only', 'bridge-loading', 'bridge-auth-unavailable', 'bridge-incompatible', 'bridge-ready'] as const
    const seen = new Set<string>()
    for (const state of states) {
      expect(harnessStateLabel(state), `label ${state}`).toBeTruthy()
      expect(seen.has(harnessStateLabel(state)), `duplicate label for ${state}`).toBe(false)
      seen.add(harnessStateLabel(state))
    }
    for (const state of states.filter((s) => s !== 'bridge-ready')) {
      const detail = HARNESS_STATE_DETAILS[state]
      expect(detail, `detail ${state}`).toBeTruthy()
      // Actionable means it says what to do, not only what is wrong.
      expect(detail, `detail ${state}`).toMatch(/请|未能|正在|更新|重启|等待|检测到|已启动/)
      // Never an exception body, a token, or an OS path.
      expect(detail, `detail ${state}`).not.toMatch(/[A-Za-z]:\\|Bearer|Error:|at \w+\./)
    }
    // `bridge-ready` is the absence of a problem, so it has no detail sentence.
    expect(HARNESS_STATE_DETAILS['bridge-ready']).toBe('')
  })

  it('covers the launch failures the plan names, each with its own code', async () => {
    const { dshAutostartNotice } = await appModule()
    // One code per named condition, and no two share wording.
    const codes = ['root-path-missing', 'root-path-invalid', 'launcher-missing', 'profile-invalid',
      'spawn-failed', 'command-not-confirmed'] as const
    const notices = codes.map((outcome) => dshAutostartNotice({ outcome, external: false }))
    expect(new Set(notices).size, 'each failure needs its own wording').toBe(codes.length)
    for (const notice of notices) expect(notice).toBeTruthy()
  })
})

describe('DSH autostart wiring', () => {  it('starts DSH from the background host only, once per setting transition', async () => {
    const app = await source('src/App.tsx')
    expect(app).toContain('nativeRuntime.autostartHarnessTarget({')
    // Guarded twice: a module-scope marker so a React remount cannot re-request,
    // and the native record (asserted in its own test) which is the guarantee.
    expect(app).toContain('dshAutostartRequestedInProcess')
    expect(app).toMatch(/if \(dshAutostartRequestedRef\.current \|\| dshAutostartRequestedInProcess\) return/)
    expect(app).toMatch(/let dshAutostartRequestedInProcess = false/)
    // Keyed to the setting alone: editing rootPath/profile after a launch must
    // not spawn another process in the same run.
    expect(app).toMatch(/\}, \[settings\.dshLaunch\.autoStartWithWallpaper\]\)/)
    // The settings window is never an autostart initiator.
    const settingsWindow = await source('src/settings/SettingsWindow.tsx')
    expect(settingsWindow).not.toContain('autostartManagedDsh')
  })

  it('keeps the automatic path off a custom launcher until the user confirms it', async () => {
    const app = await source('src/App.tsx')
    expect(app).toContain('trustedCommand: settings.dshLaunch.trustedCommandForAutoStart')
    // The confirmation is a separate, explicit choice in the settings card.
    const panel = await source('src/settings/SettingsPanel.tsx')
    expect(panel).toContain('trustedCommandForAutoStart')
    expect(panel).toContain('允许自动启动使用该命令')
    // ...and it is dropped when the user clears the command, so a stale
    // confirmation cannot outlive the command it applied to.
    expect(panel).toMatch(/command: e\.target\.value \|\| undefined, trustedCommandForAutoStart: e\.target\.value \?/)
  })

  it('allows only node/pnpm launchers without confirmation, and never a shell', async () => {
    const lib = await source('src-tauri/src/lib.rs')
    const autostart = lib.slice(lib.indexOf('fn autostart_managed_dsh'), lib.indexOf('fn managed_dsh_autostart_status'))
    // The trust boundary is stated at the automatic entry point, not left
    // implicit in a file lookup failing.
    expect(autostart).toMatch(/if !is_allowlisted_auto_start_launcher\(configured\) && trusted_command != Some\(true\)/)
    expect(lib).toContain('AUTO_START_LAUNCHER_ALLOWLIST')
    expect(lib).toMatch(/\["node\.exe", "node", "pnpm\.cmd", "pnpm"\]/)
    // The configured command must be one executable path. A shell would turn a
    // stored string into an arbitrary command line.
    expect(lib).not.toMatch(/Command::new\("cmd(\.exe)?"\)/)
    expect(lib).not.toMatch(/\/C\s/)
    expect(lib).not.toMatch(/split_whitespace/)
    // Arguments are always passed as an array, never concatenated into a line.
    expect(lib).toContain('launch.args(["dsh", "--profile", profile])')
    expect(lib).toContain('launch.arg(bundled_cli).args(["--profile", profile])')
  })

  it('tells the user that login-time start needs the wallpaper to autostart', async () => {
    const panel = await source('src/settings/SettingsPanel.tsx')
    // The requirement itself, not the old second name for it: the toggle now points at
    // the page that owns the setting instead of introducing 系统自启设置 as well.
    expect(panel).toContain('还需要在「常规」里开启壁纸开机自启')
    // The warning is driven by the real Windows state, not by a guess.
    expect(panel).toContain('props.autostart.enabled')
    expect(panel).toContain('disabled-by-policy')
    const window = await source('src/settings/SettingsWindow.tsx')
    expect(window).toContain('autostart={autostartState}')
  })

  it('does not take over or stop an external DSH', async () => {
    const lib = await source('src-tauri/src/lib.rs')
    // The external check runs before any spawn, and the outcome is remembered
    // instead of retried.
    const autostart = lib.slice(lib.indexOf('fn autostart_managed_dsh'), lib.indexOf('fn managed_dsh_autostart_status'))
    expect(autostart).toContain('dsh_port_is_occupied()')
    expect(autostart.indexOf('dsh_port_is_occupied()')).toBeLessThan(autostart.indexOf('spawn_managed_dsh'))
    expect(autostart).toContain('port-occupied-external')
    // Nothing on this path may stop or claim a process it did not start.
    expect(autostart).not.toContain('stop_managed_dsh')
    expect(autostart).not.toContain('kill')
  })

  it('records the attempt on every exit path, so a failure cannot become a retry loop', async () => {
    const lib = await source('src-tauri/src/lib.rs')
    const autostart = lib.slice(lib.indexOf('fn autostart_managed_dsh'), lib.indexOf('fn managed_dsh_autostart_status'))
    // One guard at the top...
    expect(autostart).toMatch(/if let Some\(previous\) = record\.as_ref\(\)/)
    // ...and every path that returns an outcome must store it first. Counting
    // the two keeps a new early return from silently escaping the record.
    const returns = autostart.match(/return Ok\(/g) ?? []
    const stores = autostart.match(/\*record = Some\(/g) ?? []
    expect(stores.length).toBeGreaterThanOrEqual(returns.length)
  })

  it('shares one validated launch path between the manual button and autostart', async () => {
    const lib = await source('src-tauri/src/lib.rs')
    // An autostart that re-implemented validation could drift from the button.
    const manual = lib.slice(lib.indexOf('fn launch_dsh('), lib.indexOf('fn autostart_managed_dsh'))
    expect(manual).toContain('spawn_managed_dsh(state.inner()')
    const autostart = lib.slice(lib.indexOf('fn autostart_managed_dsh'), lib.indexOf('fn managed_dsh_autostart_status'))
    expect(autostart).toContain('spawn_managed_dsh(state.inner()')
    // Validation is not duplicated into either caller.
    expect(manual).not.toContain('canonicalize')
    expect(autostart).not.toContain('canonicalize')
  })
})

describe('autoStartWithWallpaper defaults', () => {
  it('is off by default and off for an upgraded profile', () => {
    expect(DEFAULT_SETTINGS.dshLaunch.autoStartWithWallpaper).toBe(false)
    expect(DEFAULT_SETTINGS.dshLaunch.trustedCommandForAutoStart).toBe(false)
    const upgraded = normalizeSettings({ dshLaunch: { profile: 'work', rootPath: 'D:\\DSH' } })
    expect(upgraded.dshLaunch.autoStartWithWallpaper).toBe(false)
    expect(upgraded.dshLaunch.trustedCommandForAutoStart).toBe(false)
    expect(upgraded.dshLaunch.profile).toBe('work')
    expect(upgraded.dshLaunch.rootPath).toBe('D:\\DSH')
  })

  it('only accepts the literal true, so junk cannot enable a resident service', () => {
    for (const raw of ['true', 1, {}, [], 'yes']) {
      const settings = normalizeSettings({ dshLaunch: { profile: 'desktop', autoStartWithWallpaper: raw } })
      expect(settings.dshLaunch.autoStartWithWallpaper, JSON.stringify(raw)).toBe(false)
    }
    const enabled = normalizeSettings({ dshLaunch: { profile: 'desktop', autoStartWithWallpaper: true } })
    expect(enabled.dshLaunch.autoStartWithWallpaper).toBe(true)
  })
})
