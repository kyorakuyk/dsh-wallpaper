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
      ['launch-args-invalid', '「启动参数」无效'],
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
      'launcher-missing', 'profile-invalid', 'port-occupied-external', 'launch-args-invalid', 'spawn-failed'] as const
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

  it('does not call a slow start an immediate exit', async () => {
    const { harnessLaunchOutcome } = await appModule()
    // 实测：CLI 宿主起来要两三秒（启动 22:27:54、端口与门票 22:27:56），而这条判定原先**立刻**
    // 就下结论 ⇒ 用户先看到"启动后立即退出"，两秒后指示灯又变绿 —— 一次假警报，比不说更糟。
    expect(harnessLaunchOutcome({ availability: 'offline' }, 500, { managed: false, running: false })).toBeNull()
    expect(harnessLaunchOutcome({ availability: 'offline' }, 7_999, { managed: true, running: false })).toBeNull()
    // 过了宽限期仍然没有受管进程，才说"很快退出"（措辞也改成不夸大：8 秒不是"立即"）。
    expect(harnessLaunchOutcome({ availability: 'offline' }, 8_001, { managed: true, running: false })?.message)
      .toContain('很快退出')
    expect(harnessLaunchOutcome({ availability: 'offline' }, 8_001, { managed: false, running: false })?.message)
      .toContain('很快退出')
  })

  it('waits longer for the installed CLI, which boots through a shell first', async () => {
    const { harnessLaunchOutcome } = await appModule()
    const cli = 'installed-cli' as const
    // 源码目录 8 秒就说"很快退出"，而这个时间点上 CLI 还可能在启动：它要先经 npm 批处理（cmd）
    // 再拉起 node。同一条时限套在所有人身上，用户就会看到"刚报错就连上"—— 报错早于事实。
    expect(harnessLaunchOutcome({ availability: 'offline' }, 8_001, { managed: false, running: false }, 45_000, cli))
      .toBeNull()
    expect(harnessLaunchOutcome({ availability: 'offline' }, 19_999, { managed: false, running: false }, 45_000, cli))
      .toBeNull()
    expect(harnessLaunchOutcome({ availability: 'offline' }, 20_001, { managed: false, running: false }, 45_000, cli)?.message)
      .toContain('很快退出')
    // 壳没有"退出"可观察，任何时刻都不该由这条判定发言。
    expect(harnessLaunchOutcome({ availability: 'offline' }, 60_000, { managed: false, running: false }, 45_000, 'embedded-shell'))
      .not.toBeNull()
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
      'spawn-failed', 'launch-args-invalid'] as const
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

  // 冻结（与「启动参数」一起）：它钉的是"自动启动这条路上带着参数"（`App.tsx` 里那行
  // `args: parseLaunchArgs(settings.dshLaunch.args)`）以及界面上「启动参数」那个控件 —— 两者都
  // 随这次冻结被注释掉了（参数不再上路，"控件在不在"由注释决定）。`launch-args-invalid` 那句话
  // 本身仍然钉在上面那两条报告类测试里；"启动参数不再上路"改由 launchArgsAndInstances.spec.ts
  // 里那一组「启动参数冻结之后：没有参数流出去」钉住。
  it.skip('gives the automatic path the same launcher and the same args as the button', async () => {
    const app = await source('src/App.tsx')
    // 「启动参数」在两条路上一视同仁：加的是参数，跑的是谁由本应用决定，所以这里不再需要
    // 任何"要不要授权"的字段。
    expect(app).toContain('args: parseLaunchArgs(settings.dshLaunch.args)')
    const panel = await source('src/settings/SettingsPanel.tsx')
    // 那个授权复选框与它那一行随「启动命令」一起消失；界面上不该再留一个不生效的开关。
    expect(panel).not.toContain('trustedCommandForAutoStart')
    expect(panel).not.toContain('自动启动不使用自定义启动命令')
    expect(panel).not.toContain('允许自动启动使用该命令')
    // 而**控件本身**也换了：没有「启动命令」这个输入框了，只有「启动参数」。
    // （注释里还会出现"启动命令"三个字，那是说明为什么它没了 —— 所以这里钉的是字段标题。）
    expect(panel).not.toContain('title="启动命令"')
    expect(panel).toContain('title="启动参数"')
    expect(panel).toContain('aria-label="启动参数"')
  })

  it('never runs a program the user typed: only args are appended, and never through a shell', async () => {
    const lib = await source('src-tauri/src/lib.rs')
    const autostart = lib.slice(lib.indexOf('fn autostart_managed_dsh'), lib.indexOf('fn managed_dsh_autostart_status'))
    // The trust boundary moved: it is no longer "is this launcher allowlisted", because there is no
    // user-supplied launcher at all. What must hold now is that nothing on this path *chooses* a
    // program from configuration — the managed chain picks node/pnpm itself.
    expect(lib).not.toContain('AUTO_START_LAUNCHER_ALLOWLIST')
    expect(lib).not.toContain('is_allowlisted_auto_start_launcher')
    expect(lib).not.toMatch(/configured_launcher/)
    expect(lib).toContain('let launcher = if use_bundled_cli { "node.exe" } else { "pnpm.cmd" };')
    // 参数是 argv 数组，永远不拼成一条命令行；而且原生**再分一次词**才是注入的成因。
    expect(lib).toContain('harness_launch::normalize_launch_args')
    expect(lib).not.toMatch(/Command::new\("cmd(\.exe)?"\)/)
    expect(lib).not.toMatch(/\/C\s/)
    expect(lib).not.toMatch(/split_whitespace/)
    // Arguments are always passed as an array, never concatenated into a line.
    expect(lib).toContain('launch.args(["dsh", "--profile", profile])')
    expect(lib).toContain('launch.arg(bundled_cli).args(["--profile", profile])')
    // 而那一行之后紧接着就是「启动参数」的追加：**顺序**是功能的一部分（DSH 的启动器只解析
    // 自己那几个旗标，第一个不认识的词之后整段交给被 boot 的档案）。
    expect(lib).toMatch(/launch\.arg\(bundled_cli\)\.args\(\["--profile", profile\]\);\n\s*\} else \{\n\s*launch\.args\(\["dsh", "--profile", profile\]\);\n\s*\}\n[\s\S]{0,400}?launch\.args\(&args\);/)
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
    // instead of retried. 现在读的是**这一次要用的那个端口**：并行实例的第二、三个各在自己的
    // 端口上，写死 3080 会让它们被自己的第一个实例挡住。
    const autostart = lib.slice(lib.indexOf('fn autostart_managed_dsh'), lib.indexOf('fn managed_dsh_autostart_status'))
    expect(autostart).toContain('dsh_port_is_occupied(port)')
    expect(autostart.indexOf('dsh_port_is_occupied(port)')).toBeLessThan(autostart.indexOf('spawn_managed_dsh'))
    expect(autostart).toContain('port-occupied-external')
    // Nothing on this path may stop or claim a process it did not start.
    expect(autostart).not.toContain('stop_managed_dsh')
    expect(autostart).not.toContain('kill')
    // 端口本身由「启动参数」读出，不由一个常量决定。
    expect(autostart).toContain('harness_launch::instance_port(&args)')
    expect(lib).not.toContain('SocketAddr::from(([127, 0, 0, 1], 3080))')
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
    // 那两项（自定义启动命令、以及它为自动启动要的那次授权）已经不在了：字段本身不该再出现，
    // 否则"改回默认"会留下一个读不到也写不出的残迹。
    expect('command' in DEFAULT_SETTINGS.dshLaunch).toBe(false)
    expect('trustedCommandForAutoStart' in DEFAULT_SETTINGS.dshLaunch).toBe(false)
    const upgraded = normalizeSettings({ dshLaunch: { profile: 'work', rootPath: 'D:\\DSH', trustedCommandForAutoStart: true } })
    expect(upgraded.dshLaunch.autoStartWithWallpaper).toBe(false)
    expect('trustedCommandForAutoStart' in upgraded.dshLaunch).toBe(false)
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
