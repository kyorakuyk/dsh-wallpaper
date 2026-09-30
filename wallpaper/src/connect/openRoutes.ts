/**
 * 「打开」有哪些路线可走 —— 一条规则，从 JSX 里搬出来。
 *
 * 为什么值得是纯函数：这条规则里有两个**容易想当然**的地方，而它们都由测试挡住了：
 *
 * 1. 「已安装的 CLI」确实有两条路（浏览器 / 终端），但**终端要先装了 TUI**。没装的人看到
 *    那个选项，点下去只会拿到一句"本机没有找到 TUI（dst）" —— 那不是选项，是坑。
 *    所以 `tuiAvailable` 来自扫描结果，而不是"这个类别理论上支持"。
 * 2. 官壳与源码树**各只有一条路**（官壳用它独占的 `desktop` 档案，源码树没有自己的窗口），
 *    所以它们不给选项 —— 单项做成下拉就是一个点了没反应、也无法改变的控件。
 */

export interface OpenRoute {
  value: 'browser' | 'tui'
  label: string
}

export type OpenRouteSubjectKind = 'embedded-shell' | 'checkout' | 'installed-cli'

export interface OpenRouteInput {
  /** 有没有选主体。没选就什么都打不开。 */
  hasSubject: boolean
  /** 选的是官壳。 */
  shellSelected: boolean
  /** 所选主体的类别；没选时为 undefined。 */
  subjectKind: OpenRouteSubjectKind | undefined
  /** 本机有没有 TUI，来自扫描结果（`HarnessTargetScan.tuiAvailable`）。 */
  tuiAvailable: boolean
}

export function openRoutesFor(input: OpenRouteInput): OpenRoute[] {
  if (!input.hasSubject) return []
  if (input.shellSelected) return [{ value: 'browser', label: '官方客户端窗口' }]
  if (input.subjectKind === 'installed-cli') {
    const browser: OpenRoute = { value: 'browser', label: '浏览器' }
    if (!input.tuiAvailable) return [browser]
    return [browser, { value: 'tui', label: '终端里的 TUI' }]
  }
  return [{ value: 'browser', label: '浏览器' }]
}

/**
 * 实际走哪条路：存着的选择只有在**真的还有两条路**时才作数。
 *
 * 这一条是"卸掉 TUI 之后界面不会卡住"的保证：档案里存着 `tui`，但机器上已经没有 TUI，
 * 于是选项只剩一条，这里回落到浏览器，而不是让用户面对一个已经无效的选择。
 */
export function selectedOpenRoute(
  routes: readonly OpenRoute[],
  stored: 'browser' | 'tui' | undefined,
): 'browser' | 'tui' {
  return routes.length > 1 && stored === 'tui' ? 'tui' : 'browser'
}
