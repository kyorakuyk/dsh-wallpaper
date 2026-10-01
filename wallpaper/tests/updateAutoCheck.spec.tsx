// @vitest-environment jsdom
/**
 * 自动检查的**触发行为** —— 一条真把 hook 挂起来、真看 IPC 的测试。
 *
 * 为什么非要有它：`updateUi.spec.tsx` 里那一组读的是**源码文本**（`App.tsx` 里写着
 * `inInnerDesktop: workspace === 'inner'`、`useUpdate.ts` 里写着 `nativeRuntime.updateCheck(`）。
 * 它们能证明"接线写在那儿"，证明不了"接线真的通"：条件写反、effect 的依赖数组少一项、那道
 * 闸门被谁先认领了，界面看起来都一模一样，而原生一次都没被调用 —— 真机上"更新检查从未发生过"
 * 正是这一类症状。所以这里挂的是**同一个 hook**（`App.tsx` 挂的就是它），记的是**IPC 边界上
 * 真实发生的事**：`invoke('update_check', { manual })`。
 *
 * 两件刻意的事：
 *
 *  - 假原生层沿用 `updateUi.spec.tsx` 的做法（拦 `window.__TAURI_INTERNALS__`），不再造第二个
 *    注入点 —— 那一层就是 hook 与原生之间唯一的门。区别只有一处：本文件跑在 jsdom 里，
 *    React DOM 要的那个真 `window` 不能被整个换掉，所以假的两样是**加**上去、用完删掉。
 *  - 断言停在"界面发出了哪条命令、带了什么参数"。"原生收到之后做了什么"由 Rust 那侧自己的
 *    测试钉着（`src-tauri/src/update/commands.rs`），不在这一条里替它假装。
 *
 * 用例顺序上有一处依赖是**刻意的**：`useUpdate` 里那道"每进程一次"的闸门是模块级的，因此
 * 同一份进程里"进入里桌面"只可能发生一次调用。第一条用例只断言**不查**（表桌面、还没过唤醒
 * 动画），走的是提前 return 的那一支，不会认领闸门 —— 于是第二条用例才看得到那唯一的一次。
 * 反过来说，两条用例各自单独跑也都成立（`-t` 挑一条跑不会受影响）。
 */
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest'

import { useUpdateBubble } from '../src/features/update/useUpdate.ts'
import type { UpdateCheckReport } from '../src/native/runtime.ts'

/** `invoke` 记下来的一条：命令名与参数就是界面与原生之间的全部契约。 */
interface InvokeCall {
  command: string
  args: Record<string, unknown>
}

/**
 * jsdom 的 `window` 上本来没有这两样；它们是 Tauri 宿主注入的，所以这里自己加上。
 *
 * `__TAURI_EVENT_PLUGIN_INTERNALS__` 只有取消订阅那条路会碰（`@tauri-apps/api/event` 的
 * `_unlisten`），而 `listen()` 本身要求 `__TAURI_INTERNALS__.transformCallback` 存在 ——
 * 少了它，下载进度那条订阅会在挂载时抛错、顺手往 `notice` 里写一句与本测试无关的话。
 */
type FakeNativeWindow = Window & {
  __TAURI_INTERNALS__?: Record<string, unknown>
  __TAURI_EVENT_PLUGIN_INTERNALS__?: Record<string, unknown>
}

type ProbeProps = { inInnerDesktop: boolean; awake: boolean }

const fakeNativeWindow = (): FakeNativeWindow => globalThis.window as unknown as FakeNativeWindow

let container: HTMLDivElement
let root: Root
let calls: InvokeCall[]
let internalsBefore: Record<string, unknown> | undefined
let eventInternalsBefore: Record<string, unknown> | undefined

/** 一份"有新版本"的报告（字段名与原生 `UpdateCheckReport` 的序列化形状一致）。 */
function availableReport(): UpdateCheckReport {
  return {
    outcome: 'updateAvailable',
    skipReason: null,
    failure: null,
    currentVersion: '0.4.1',
    currentVersionSource: 'uninstallEntry',
    latestVersion: '0.4.2',
    releaseUrl: 'https://github.com/kyorakuyk/dsh-wallpaper/releases/tag/v0.4.2',
    asset: {
      name: 'dsh-wallpaper_0.4.2_x64-setup.exe',
      size: 31_457_280,
      downloadUrl: 'https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/dsh-wallpaper_0.4.2_x64-setup.exe',
      digest: null,
    },
    checkedAtMs: 1_700_000_000_000,
    dismissedVersion: null,
    dismissed: false,
    statePersisted: true,
  }
}

/** 假原生层：每一条 `invoke` 都记下来；`update_check` 回一份"有新版本"，别的返回空。 */
function installFakeNative(): void {
  const win = fakeNativeWindow()
  internalsBefore = win.__TAURI_INTERNALS__
  eventInternalsBefore = win.__TAURI_EVENT_PLUGIN_INTERNALS__
  calls = []
  let callbackId = 0
  win.__TAURI_INTERNALS__ = {
    // 订阅那条路（`update-download` 的进度）只要一个回调 id：本文件测的是"检查有没有发出去"。
    transformCallback: () => ++callbackId,
    unregisterCallback: () => undefined,
    invoke: async (command: string, args: Record<string, unknown>) => {
      calls.push({ command, args })
      return command === 'update_check' ? availableReport() : undefined
    },
  }
  win.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined }
}

/**
 * 探针：挂的就是 `App.tsx` 挂的那个 hook，把它手上的状态写进 DOM。
 *
 * 写进 DOM 而不是从闭包里捞：这样"报告真的回来了"与"调用发出去了"用的是同一条证据链
 * （原生回执 → 控制器状态 → 界面），而不是只证明前半段。
 */
function UpdateProbe(props: ProbeProps) {
  const update = useUpdateBubble(props)
  return <div data-testid="update-probe" data-phase={update.phase} data-offer={update.offer?.version ?? ''} />
}

/** 渲染一次（同一个 root 重渲染），并让原生那一次调用的 promise 链跑完。 */
async function mount(props: ProbeProps): Promise<void> {
  await act(async () => {
    root.render(<UpdateProbe {...props} />)
  })
  // `invoke` 只 resolve 一次：一个宏任务边界足够让 `check` 把状态写完（否则断言跑在它前面，
  // 看到的就是"调了但界面没变"，而那不是事实）。
  await act(async () => {
    // 多让出几个宏任务：这条测试跑在真渲染器里，invoke 的 promise 链要跨几个 tick。
    // 只让一次 setTimeout(0) 在空闲时够用，但构建机忙着编译 cargo 时不够 ——
    // 2026-10-02 的发布构建就这么红过一次（偶发的门禁比没有门禁更坏）。
    for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setTimeout(resolve, 5))
  })
}

/** 界面一共发了几次检查、各带的什么参数。 */
const checkCalls = (): InvokeCall[] => calls.filter((call) => call.command === 'update_check')

/** 探针上那两个属性：相位与手上那枚更新。 */
function probe(): { phase: string | null; offer: string | null } {
  const node = container.querySelector('[data-testid="update-probe"]')
  return { phase: node?.getAttribute('data-phase') ?? null, offer: node?.getAttribute('data-offer') ?? null }
}

function restoreProp(name: keyof FakeNativeWindow, previous: Record<string, unknown> | undefined): void {
  const win = fakeNativeWindow()
  if (previous === undefined) delete win[name]
  else win[name] = previous
}

beforeAll(() => {
  // React 18 只有在这个全局为真时才认为"当前处在 act 环境"（否则每次状态更新都印一条警告）。
  ;(globalThis as unknown as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
})

afterAll(() => {
  delete (globalThis as unknown as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT
})

beforeEach(() => {
  installFakeNative()
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => {
    root.unmount()
  })
  container.remove()
  restoreProp('__TAURI_INTERNALS__', internalsBefore)
  restoreProp('__TAURI_EVENT_PLUGIN_INTERNALS__', eventInternalsBefore)
})

describe('自动检查的触发条件（真挂 hook）', () => {
  it('留在表桌面、或者还没过唤醒动画：一次都不查', async () => {
    // 表桌面阶段：`inInnerDesktop` 为假（`App.tsx` 里就是 `workspace === 'inner'`）。
    await mount({ inInnerDesktop: false, awake: true })
    expect(checkCalls(), '表桌面阶段不该有原生检查').toHaveLength(0)

    // 里桌面但还在锁屏/苏醒/启动中：`awake` 为假，同样不查。
    await mount({ inInnerDesktop: true, awake: false })
    expect(checkCalls(), '没过唤醒动画的那一段不该有原生检查').toHaveLength(0)

    // 而这一条用例不认领那道闸门（两次都提前 return 了）—— 下面那一条才看得到那一次调用。
  })

  it('进入里桌面 ⇒ 一次自动检查（manual:false），报告回到界面；反复进出不再打', async () => {
    await mount({ inInnerDesktop: true, awake: true })

    // 一次，而且**只有**一次；参数是自动检查那一种（`manual: true` 是设置页那枚按钮与打开
    // 系统页那一次，那条路上必须不受 6 小时节流限制，走的是另一条分支）。
    expect(checkCalls()).toEqual([{ command: 'update_check', args: { manual: false } }])
    // 原生回执真的走到了界面上：相位与手上那枚更新都变了 —— "点了没反应"的这一半也是它钉的。
    expect(probe()).toEqual({ phase: 'available', offer: '0.4.2' })

    // 出里桌面、再进来：界面侧那道"每进程一次"的闸门挡着，不会再打一次。
    // （跨进程那一道由原生负责：重启壁纸后再进里桌面，界面会调、原生按 6 小时节流回 `skipped`。）
    await mount({ inInnerDesktop: false, awake: true })
    await mount({ inInnerDesktop: true, awake: true })
    expect(checkCalls(), '反复进出不该反复打').toHaveLength(1)

    // 自动那一条路上没有下载、没有安装、也没有忽略：它只查（§六；Rust 那侧另有一条钉着
    // `run_check` 里不出现下载调用）。
    expect(calls.filter((call) => call.command.startsWith('update_')).map((call) => call.command))
      .toEqual(['update_check'])
  })
})
