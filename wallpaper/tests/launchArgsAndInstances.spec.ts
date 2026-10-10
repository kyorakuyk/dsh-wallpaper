import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { launchArgsIssue, launchPortFromArgs, launchSettingsPort, parseLaunchArgs } from '../src/connect/launchArgs.ts'
import { endpointScopeOf, subjectEndpointPorts, WALLPAPER_HOST_PORT } from '../src/connect/endpoints.ts'
import { instanceLabel, subjectOptionLabel } from '../src/connect/harnessSubjects.ts'
import type { HarnessTarget } from '../src/native/runtime.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

// B6 of the freeze isolation moved the commented-out launch-args and instance UI out of
// wallpaper/src into readable fragments under archive/frozen/. The revival checks read them.
const archiveRoot = resolve(wallpaperRoot, '..', 'archive')
const archivedLaunchUiRoot = 'frozen/launch-ui/wallpaper/src'
const readArchive = async (relative: string): Promise<string> =>
  (await readFile(resolve(archiveRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

function checkout(rootPath: string, version?: string): HarnessTarget {
  return {
    id: rootPath,
    kind: 'checkout',
    client: 'official-web',
    label: 'deepseek-harness',
    version,
    source: '磁盘扫描',
    identity: { rootPath, defaultPorts: [3080] },
    launch: { kind: 'managed-command' },
    capabilities: { singleInstance: false, ownsWindow: false, canStartHidden: true, needsProfile: true },
  }
}

function installedCli(): HarnessTarget {
  const launcher = 'C:\\Users\\someone\\AppData\\Roaming\\npm\\dsh.cmd'
  return {
    id: `cli:${launcher}`,
    kind: 'installed-cli',
    client: 'official-web',
    label: 'DSH CLI',
    version: '0.2.0-rc.1',
    source: launcher,
    identity: { defaultPorts: [3080] },
    launch: { kind: 'managed-command' },
    capabilities: { singleInstance: false, ownsWindow: false, canStartHidden: true, needsProfile: true },
  }
}

/**
 * 「启动参数」：分词、读端口，以及它与设置里那个 pin 的关系。
 *
 * 这一项取代了「启动命令」，所以这里钉的第一件事就是**能力边界**：用户写下的词只会被追加到
 * 本应用选定的启动器后面，永远不构成一条命令行。分词规则只有三条（空白分隔、双引号、单引号），
 * 因为每多一条就多一种"用户以为会这样、其实那样"的写法。
 */
describe('「启动参数」的分词', () => {
  it('splits on whitespace and keeps quoted spans together', () => {
    expect(parseLaunchArgs('--port 3081')).toEqual(['--port', '3081'])
    expect(parseLaunchArgs('   --host   127.0.0.1  ')).toEqual(['--host', '127.0.0.1'])
    // 双引号里的空格不分割；`\"` 与 `\\` 是仅有的两条转义。
    expect(parseLaunchArgs('--title "hello world"')).toEqual(['--title', 'hello world'])
    expect(parseLaunchArgs('--x "a\\"b"')).toEqual(['--x', 'a"b'])
    expect(parseLaunchArgs('--x "a\\\\b"')).toEqual(['--x', 'a\\b'])
    // 单引号里**不做任何转义**：Windows 路径里的反斜杠因此不必写两遍。
    expect(parseLaunchArgs("--root 'D:\\Family\\DeepSeekHarness'")).toEqual(['--root', 'D:\\Family\\DeepSeekHarness'])
    // 空串与没有参数是同一件事：不留一个空词给启动器。
    expect(parseLaunchArgs('')).toEqual([])
    expect(parseLaunchArgs(undefined)).toEqual([])
    expect(parseLaunchArgs('   ')).toEqual([])
    expect(parseLaunchArgs('"" ""')).toEqual([])
  })

  it('is not a command interpreter', () => {
    // 分号、管道符、`&&`、`$VAR`、`*` 都只是普通字符：这里没有 shell，原样传下去才是对的。
    expect(parseLaunchArgs('a;b | c && d')).toEqual(['a;b', '|', 'c', '&&', 'd'])
    expect(parseLaunchArgs('--x $HOME')).toEqual(['--x', '$HOME'])
    expect(parseLaunchArgs('--x *.log')).toEqual(['--x', '*.log'])
    // 未闭合的引号按已读内容结束，而不是吞掉整行或抛错（这个字段是随打随存的）。
    expect(parseLaunchArgs('--x "abc')).toEqual(['--x', 'abc'])
  })

  it('refuses what the launcher could not be given', () => {
    expect(launchArgsIssue('--port 3081')).toBeNull()
    expect(launchArgsIssue('')).toBeNull()
    // 控制字符会让原生侧的实例键（`\u{1f}` 分隔）把两个实例撞成一个，所以它被拒绝是有理由的。
    expect(launchArgsIssue('a\u001fb')).toContain('控制字符')
    // 引号里那一段不会被空白切开，所以里面的控制字符留得下来 —— 这条正是"换行只是空白"的反例：
    // 换行会被分词消掉（那是空白，不是控制字符），而这里这个不会。
    expect(launchArgsIssue('--x "a\u0007b"')).toContain('控制字符')
    expect(launchArgsIssue(`--x ${'y'.repeat(600)}`)).toContain('512')
    expect(launchArgsIssue(Array.from({ length: 40 }, (_, i) => `a${i}`).join(' '))).toContain('32')
  })
})

describe('参数里声明的端口', () => {
  it('reads the flag exactly the way dsh reads it', () => {
    // 实测 `dsh web --help`：`--port <port>  listen port; pass 0 to let the OS pick a free one`。
    expect(launchPortFromArgs(['--port', '3081'])).toBe(3081)
    expect(launchPortFromArgs(['--port=3082'])).toBe(3082)
    expect(launchPortFromArgs(['--no-open', '--port', '8080', '--x'])).toBe(8080)
    // 读不出来就说读不出来：绝不猜一个端口。
    expect(launchPortFromArgs([])).toBeUndefined()
    expect(launchPortFromArgs(['--port'])).toBeUndefined()
    expect(launchPortFromArgs(['--port', 'abc'])).toBeUndefined()
    expect(launchPortFromArgs(['--port', '0'])).toBeUndefined()
    expect(launchPortFromArgs(['--port', '70000'])).toBeUndefined()
    // 重复旗标只认第一个：把最后一个当答案，就是解析器替用户猜。
    expect(launchPortFromArgs(['--port', '3081', '--port', '3082'])).toBe(3081)
    expect(launchSettingsPort('--port 3081')).toBe(3081)
    expect(launchSettingsPort(undefined)).toBeUndefined()
  })

  it('is the same vector list the native side is tested against', async () => {
    // 两份实现（渲染层读浏览器地址、原生查端口占用）由同一组例子钉住，与 `SHELL_APPS` /
    // `SHELL_SUBJECTS` 那对必须一致的常量是同一个理由：一致性靠测试，不靠"记得两边都改"。
    const rust = await source('src-tauri/src/harness_launch.rs')
    for (const literal of ['"--port"', '"--port="', 'fn port_from_args']) {
      expect(rust, literal).toContain(literal)
    }
    // 而"读不到就不猜"这条规则在两边都必须成立。
    expect(rust).toContain('.filter(|port| (1..=65535).contains(port))')
  })
})

describe('端口从设置一路流到「打开界面」', () => {
  it('puts the port from 「启动参数」 first among the subject’s own ports', () => {
    const launch = { subjectId: 'D:\\tree', args: '--port 3081' }
    // 3081 在最前面：它是这个主体**被要求**服务的地方，比默认端口更具体。
    expect(subjectEndpointPorts(endpointScopeOf(launch))).toEqual([3081, 3080])
    // 没写端口 ⇒ 与今天完全一样：[默认, 额外加的]。
    expect(subjectEndpointPorts(endpointScopeOf({ subjectId: 'D:\\tree' }))).toEqual([3080])
    expect(subjectEndpointPorts(endpointScopeOf({ subjectId: 'D:\\tree', extraEndpointPorts: [9000] })))
      .toEqual([3080, 9000])
    expect(subjectEndpointPorts(endpointScopeOf({ subjectId: 'D:\\tree', args: '--port 3081', extraEndpointPorts: [9000] })))
      .toEqual([3081, 3080, 9000])
    // 用户显式 pin 的仍然最优先（那条规则是冻结的）。
    expect(subjectEndpointPorts(endpointScopeOf({ subjectId: 'D:\\tree', args: '--port 3081', endpointPort: 4000 })))
      .toEqual([4000])
  })

  it('does not let a launch arg move a shell off the port compiled into it', () => {
    // 官壳的端口编译在它自己的包里，参数改不了它 —— 给它"另一个端口"等于说那是另一个客户端。
    // 后随那个是**壁纸自己宿主的**端口（客户端随时可以打开，不会撞车），同样不受参数影响。
    expect(subjectEndpointPorts({ subjectId: 'shell:com.deepseek.dsh', args: '--port 4000' }))
      .toEqual([19387, WALLPAPER_HOST_PORT])
  })

  it('is carried by endpointScopeOf, which is the one place the stored fields become a scope', () => {
    expect(endpointScopeOf({ subjectId: 'D:\\tree', args: '--port 3081' }).args).toBe('--port 3081')
    // rootPath 这条旧拼写仍然被认作主体，参数也照样带过去。
    expect(endpointScopeOf({ rootPath: 'D:\\tree', args: '--port 3082' }))
      .toMatchObject({ subjectId: 'D:\\tree', args: '--port 3082' })
  })

  it('reaches the native probe too, so the monitor and the browser watch one port', async () => {
    // 少了这一环，并行实例的第二个就会永远显示成离线：壁纸的探针盯着 3080，而桥在 3081 上应答。
    const app = await source('src/App.tsx')
    expect(app).toContain('nativeRuntime.setHarnessEndpointScope(endpointScopeOf(settings.dshLaunch))')
    const runtime = await source('src/native/runtime.ts')
    const setter = runtime.slice(runtime.indexOf('async setHarnessEndpointScope'), runtime.indexOf('async leaveInnerDesktop'))
    expect(setter).toContain('args: parseLaunchArgs(scope.args)')
    // 原生那一侧从同一份 argv 里读端口，规则与渲染层一致（两份由各自的测试用同一组例子钉住）。
    const lib = await source('src-tauri/src/lib.rs')
    expect(lib).toContain('let declared = args.and_then(harness_launch::port_from_args);')
    expect(lib).toContain('harness_targets::subject_endpoint_ports(&subject_id, extra_ports, declared)')
    const targets = await source('src-tauri/src/harness_targets.rs')
    expect(targets).toContain('declared_port: Option<u16>')
  })
})

/**
 * 「起别名」与实例下拉里的行文字。用户点名了这两个形状，所以它们逐字钉在这里。
 */
describe('别名与实例行', () => {
  const tree = checkout('D:\\Family\\DeepSeekHarness\\deepseek-harness', '0.1.0-rc.5')
  const other = checkout('C:\\DeepSeekHarness.old\\deepseek-harness', '0.1.0-rc.5')

  it('replaces the parent-directory disambiguator in the option label', () => {
    // 用户点名的形状：`源码目录 · 别名 · 版本`。别名**顶替**上级目录那一段，而不是加在它后面。
    expect(subjectOptionLabel(tree, [tree, other], { [tree.id]: '主树' }))
      .toBe('源码目录 · 主树 · 0.1.0-rc.5')
    // 没有别名时与今天逐字一样（同名两份靠上级目录区分）。**这一条现在是界面上真正会走的形态**：
    // 「运行方式」的下拉不再传别名表（冻结），于是 `源码目录 · DeepSeekHarness.old · 0.1.0-rc.5`
    // 就是用户看到的那一行。
    expect(subjectOptionLabel(tree, [tree, other])).toBe('源码目录 · DeepSeekHarness · 0.1.0-rc.5')
    expect(subjectOptionLabel(other, [tree, other])).toBe('源码目录 · DeepSeekHarness.old · 0.1.0-rc.5')
    // 只有一棵时，别名与目录名都不该把仓库名挤掉 —— 别名顶的是"区分"那一段，不是名字本身。
    expect(subjectOptionLabel(tree, [tree])).toBe('源码目录 · deepseek-harness · 0.1.0-rc.5')
    expect(subjectOptionLabel(tree, [tree], { [tree.id]: '主树' })).toBe('源码目录 · 主树 · 0.1.0-rc.5')
    // 别的主体的别名不会串到这一条上（别名按主体 id 存，就是为了这件事）。
    expect(subjectOptionLabel(tree, [tree, other], { [other.id]: '旧树' }))
      .toBe('源码目录 · DeepSeekHarness · 0.1.0-rc.5')
    expect(subjectOptionLabel(other, [tree, other], { [other.id]: '旧树' }))
      .toBe('源码目录 · 旧树 · 0.1.0-rc.5')
  })

  it('labels a running instance as 别名 · 端口', () => {
    expect(instanceLabel(tree.id, 3081, [tree], { [tree.id]: '主树' })).toBe('主树 · 3081')
    // 没起别名时用目录那段名字（同名两份仍旧分得开）。
    expect(instanceLabel(tree.id, 3081, [tree, other])).toBe('DeepSeekHarness · 3081')
    expect(instanceLabel(other.id, 3082, [tree, other])).toBe('DeepSeekHarness.old · 3082')
    // 已安装的 CLI 没有目录可指。
    const cli = installedCli()
    expect(instanceLabel(cli.id, 3080, [cli])).toBe('DSH CLI · 3080')
    // 端口读不出来时写「端口未确认」，不写 0、也不省略：省略会读成"这个实例没有端口"，
    // 而 `--port 0`（让系统挑一个）真的长这样 —— 两种情况的下一步并不相同。
    expect(instanceLabel(cli.id, undefined, [cli])).toBe('DSH CLI · 端口未确认')
    // 扫描之后目录消失了：至少还要叫得出名字。
    expect(instanceLabel('D:\\gone\\my-tree', 3081, [])).toBe('my-tree · 3081')
  })
})

/**
 * 冻结后的形状：这个功能的**入口**全都不在了，而原生侧一行都没动。
 *
 * 这一组是"复活时该从哪里开始"的清单：它钉住的是"现在没有参数流出去"，所以哪天有人把某条
 * `args:` 加回来而忘了另一条，这里会立刻失败，而不是等到用户发现只有自动启动带参数。
 * 冻结隔离（B6）把那几行注释掉的 `args:` 连同说明一起移进了 `archive/frozen/launch-ui/`：现役源码
 * 只钉"没有参数上路"，"恢复材料还在"改由读取归档来钉。被冻结控件的那几条 UI 断言也随 `it.skip`
 * 一起迁进了那份归档（以及 `archive/frozen/instance-ui/`）。
 */
describe('启动参数冻结之后：没有参数流出去', () => {
  const LAUNCH_CALL_NAMES = ['ensureHarnessUi', 'autostartHarnessTarget', 'launchHarnessTarget'] as const

  it('keeps the args property out of every live launch call site, with the revival lines in the archive', async () => {
    const app = await source('src/App.tsx')
    // 冻结隔离（B6）之后，现役 App.tsx 连那行注释掉的 import 也不再带着：没有 `parseLaunchArgs` 的任何痕迹。
    expect(app).not.toContain('parseLaunchArgs')
    for (const call of LAUNCH_CALL_NAMES) {
      const from = app.indexOf(`nativeRuntime.${call}({`)
      expect(from, call).toBeGreaterThanOrEqual(0)
      const callText = app.slice(from, app.indexOf('})', from))
      expect(callText, `${call} 仍然带着参数上路`).not.toMatch(/^\s*args:/m)
    }
    // 而且留了话：可恢复的那三行在 launch-ui 归档里逐个调用列着，不必去 git 历史里找。
    const archived = await readArchive(`${archivedLaunchUiRoot}/App.launch-args.tsx`)
    expect(archived).toContain("import { parseLaunchArgs } from './connect/launchArgs.ts'")
    for (const call of LAUNCH_CALL_NAMES) {
      const from = archived.indexOf(`nativeRuntime.${call}({`)
      expect(from, `归档里缺少 ${call}`).toBeGreaterThanOrEqual(0)
      const callText = archived.slice(from, archived.indexOf('})', from))
      expect(callText, `${call} 缺少 FREEZE 说明`).toContain('FREEZE')
      expect(callText, `${call} 缺少可恢复的那一行`).toMatch(/^\s*args: parseLaunchArgs\(/m)
    }
  })

  it('keeps the args property out of the settings window too, with the revival lines in the archive', async () => {
    const window = await source('src/settings/SettingsWindow.tsx')
    // 两条路：手动「打开界面」（ensureHarnessUi）与「拉起 TUI」（openSubjectTui）。
    expect(window).not.toMatch(/^\s*args: parseLaunchArgs/m)
    const archived = await readArchive(`${archivedLaunchUiRoot}/settings/SettingsWindow.launch-args.tsx`)
    expect(archived).toContain('args: parseLaunchArgs(current.args),')
    // TUI 那一条原来是直接传实参的（`openSubjectTui(parseLaunchArgs(…))`），所以它的恢复形态是
    // "把那个实参加回来"（归档里写着那一行），而现役是无参调用。
    expect(archived).toContain('nativeRuntime.openSubjectTui(parseLaunchArgs(settingsRef.current.dshLaunch.args))')
    expect(window).toContain('nativeRuntime.openSubjectTui()')
    expect(window).not.toContain('openSubjectTui(parseLaunchArgs')
  })
})

describe('实例下拉与「全部停止」的接线', () => {
  it('never offers the official desktop shell as something to stop', async () => {
    // 规则在原生侧是明写的（清单过滤 + 命令里的拒绝），不在界面侧靠"不显示它"来成立。
    const launch = await source('src-tauri/src/harness_launch.rs')
    expect(launch).toContain('pub(crate) fn is_managed_by_us')
    expect(launch).toMatch(/!subject_id\.trim\(\)\.starts_with\(SHELL_ID_PREFIX\)/)
    const lib = await source('src-tauri/src/lib.rs')
    expect(lib).toContain('这个主体不由本应用管理，因此没有停止它。')
    // 而在这之前它已经被拒绝了：官壳那条记录是给门票用的，它**确实**会出现在落盘记录里，
    // 所以"停不掉官壳"不能靠"记录里没有它"。
    expect(lib).toContain('merged.retain(|item| harness_launch::is_managed_by_us(&item.subject_id));')
    // 两条来源各有一道门：内存里的实例与落盘记录各自进来时就过一遍，最后再统一过滤一次。
    expect(lib).toMatch(/for instance in in_memory \{[\s\S]{0,200}?if !harness_launch::is_managed_by_us\(&instance\.subject_id\) \{/)
  })

  it('shows every instance the app started, keyed per instance rather than per subject', async () => {
    const runtime = await source('src/native/runtime.ts')
    expect(runtime).toContain('instances: ManagedDshInstance[]')
    expect(runtime).toContain('instanceKey: string')
    expect(runtime).toContain('stopManagedDsh(instanceKey?: string)')
    const rust = await source('src-tauri/src/harness_launch.rs')
    // 键里带参数（于是同一个主体的两个端口是两个键），而没有参数时恰好等于主体 id（旧记录不用迁移）。
    expect(rust).toMatch(/pub\(crate\) fn instance_key\(subject_id: &str, args: &\[String\]\) -> String \{\n\s*let subject = subject_id\.trim\(\);\n\s*if args\.is_empty\(\) \{\n\s*return subject\.to_string\(\);/)
  })
})
