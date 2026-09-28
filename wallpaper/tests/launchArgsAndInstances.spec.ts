import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { launchArgsIssue, launchPortFromArgs, launchSettingsPort, parseLaunchArgs } from '../src/connect/launchArgs.ts'
import { endpointScopeOf, subjectEndpointPorts } from '../src/connect/endpoints.ts'
import { instanceLabel, subjectOptionLabel } from '../src/connect/harnessSubjects.ts'
import { normalizeSettings } from '../src/settings/store.ts'
import type { HarnessTarget } from '../src/native/runtime.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

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
    expect(subjectEndpointPorts({ subjectId: 'shell:com.deepseek.dsh', args: '--port 4000' })).toEqual([19387])
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

  it('is cleared of the explicit pin when the args change, so the browser cannot chase a dead port', async () => {
    // 与"换主体就清 pin"是同一条理由：那条 pin 是上一次启动选的那个端口，参数已经把它推翻了。
    const window = await source('src/settings/SettingsWindow.tsx')
    const handler = window.slice(window.indexOf('onSelectLaunchArgs={(value) =>'), window.indexOf('onOpenTui='))
    expect(handler).toContain('endpointPort: undefined')
    expect(handler).toContain('args,')
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
    // 没有别名时与今天逐字一样（同名两份靠上级目录区分）。
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

  it('keeps one alias per subject, and drops a cleared one instead of storing an empty string', async () => {
    const settings = normalizeSettings({
      dshLaunch: { profile: 'desktop', aliases: { 'D:\\a': '主树', 'D:\\b': '旧树' } },
    })
    expect(settings.dshLaunch.aliases).toEqual({ 'D:\\a': '主树', 'D:\\b': '旧树' })
    // 界面上清空 ⇒ 删键（见 SettingsWindow 的处理），所以"没起别名"只有一种表示。
    const window = await source('src/settings/SettingsWindow.tsx')
    expect(window).toContain('else delete aliases[subjectId]')
  })
})

describe('实例下拉与「全部停止」的接线', () => {
  it('renders the dropdown in the card header, with 全部停止 beside it', async () => {
    const panel = await source('src/settings/SettingsPanel.tsx')
    // 用户画红框的位置就是卡片标题右上角，所以控件是 Card 的 action，不是又一行 Field。
    expect(panel).toContain('action={<RunningInstances')
    expect(panel).toContain('aria-label="当前已启动实例"')
    expect(panel).toContain('全部停止')
    // 每一行一个 ×，且那个 × 说的是"停这一个实例"，不是一个通用的关闭图标。
    expect(panel).toContain('aria-label={`停止实例 ${label}`}')
    expect(panel).toContain('onStopInstance(instance.instanceKey)')
    // 行文字就是 `别名 · 端口` 那一条规则渲染出来的。
    expect(panel).toContain('instanceLabel(instance.subjectId, instance.port, targets, aliases)')
  })

  it('consolidated the two stop controls into one action', async () => {
    const panel = await source('src/settings/SettingsPanel.tsx')
    // 底部那个按钮已经不在了：它与「全部停止」是同一个动作，两个控件做同一件事用户就得猜区别。
    // （注释里还会提到那个旧按钮的名字，所以这里钉的是**控件**：那行 JSX 与那个 prop 都没了。）
    expect(panel).not.toContain('onStopManagedDsh')
    expect(panel).not.toContain('>停止本应用启动的 DSH<')
    expect(panel).toContain('>全部停止<')
    // 一个动作、一个实现：同一个命令，多了（或者少了）一个 instanceKey。
    const window = await source('src/settings/SettingsWindow.tsx')
    expect(window).toContain('nativeRuntime.stopManagedDsh(instanceKey)')
    expect(window).toContain('onStopAllManagedDsh={() => { void stopManagedInstance() }}')
    // 「刷新」跟着搬到了下拉里（它不是停止，所以留着不会造成两个控件做同一件事）。
    expect(panel).toContain('onRefresh={props.onRefreshManagedDsh}')
  })

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
