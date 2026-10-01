import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { ReactElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { UpdateBubble } from '../src/features/update/UpdateBubble.tsx'
import {
  downloadFailedMessage,
  downloadFailureMessage,
  downloadProgressMessage,
  updateCallMessage,
  updateFailureMessage,
  updateOutcomeMessage,
  updateSkipMessage,
} from '../src/features/update/updateCopy.ts'
import {
  RELEASES_PAGE_URL,
  applyDownloadEvent,
  downloadPercent,
  formatBytes,
  offeredUpdate,
  releasePageUrl,
  updateBubbleVisible,
  updatePhase,
} from '../src/features/update/updateState.ts'
import { createOnceGate } from '../src/features/update/useUpdate.ts'
import { formatMessage, msg, setLanguage, t, type Message, type MessageKey } from '../src/i18n/index.ts'
import { nativeRuntime, type UpdateCheckReport, type UpdateDownloadEvent, type UpdateDownloadFailure, type UpdateDownloadFailureCode } from '../src/native/runtime.ts'
import { BUILTIN_PERSONAS } from '../src/persona/registry.ts'
import { dictionarySource } from './i18nSource.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const noop = () => undefined
const bubbleTheme = BUILTIN_PERSONAS['blue-child'].theme

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

/** 一份"有新版本"的报告（字段名与原生 `UpdateCheckReport` 的序列化形状一致）。 */
function availableReport(patch: Partial<UpdateCheckReport> = {}): UpdateCheckReport {
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
    ...patch,
  }
}

/** 把气泡渲染成静态标记：两个按钮在不在、说的是哪句话，一眼可断言。 */
function renderBubble(node: ReactElement): string {
  return renderToStaticMarkup(node)
}

/** 一整个下载状态的事件（字段名与原生 `DownloadEvent` 的序列化形状一致）。 */
function downloadEvent(patch: Partial<UpdateDownloadEvent> = {}): UpdateDownloadEvent {
  return {
    version: '0.4.2',
    phase: 'downloading',
    downloadedBytes: 0,
    totalBytes: 31_457_280,
    path: null,
    sha256: null,
    failure: null,
    ...patch,
  }
}

/** 一条失败事件 → 界面状态（原因码是这里唯一要变的东西）。 */
function failedEvent(code: UpdateDownloadFailureCode, patch: Partial<UpdateDownloadFailure> = {}) {
  return applyDownloadEvent(
    downloadEvent({
      phase: 'failed',
      downloadedBytes: 1024,
      failure: {
        code,
        httpStatus: null,
        expectedBytes: null,
        actualBytes: null,
        expectedSha256: null,
        actualSha256: null,
        ...patch,
      },
    }),
  )
}

afterEach(() => {
  // 语言是模块级状态，测试之间必须还原（下面几条会切到英文）。
  setLanguage('zh')
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('更新气泡的出现时机', () => {
  const offer = offeredUpdate(availableReport())!

  it('表桌面阶段不出现，进入里桌面之后才出现', () => {
    // 验收 1：表桌面（`front`）整段时间——包括「早上好…」那枚气泡——都不出现。
    expect(updateBubbleVisible({ workspace: 'front', phase: 'idle', offer })).toBe(false)
    // 正在进/正在出也不算"进入之后"。
    expect(updateBubbleVisible({ workspace: 'entering-inner', phase: 'idle', offer })).toBe(false)
    expect(updateBubbleVisible({ workspace: 'leaving-inner', phase: 'idle', offer })).toBe(false)
    // 里桌面：出现。相位是 `chatting`（进入里桌面本身就是"打开会话"）与 `idle` 都算过完唤醒动画。
    expect(updateBubbleVisible({ workspace: 'inner', phase: 'chatting', offer })).toBe(true)
    expect(updateBubbleVisible({ workspace: 'inner', phase: 'idle', offer })).toBe(true)
    // 锁屏、苏醒、启动中都不出现（已过唤醒动画才算）。
    for (const phase of ['booting', 'locked', 'waking', 'error', 'auth-required'] as const) {
      expect(updateBubbleVisible({ workspace: 'inner', phase, offer }), phase).toBe(false)
    }
  })

  it('没有要提示的更新时没有气泡', () => {
    expect(updateBubbleVisible({ workspace: 'inner', phase: 'idle', offer: undefined })).toBe(false)
    // 失败不弹气泡（§五）：只在设置里留一行结果。
    expect(offeredUpdate(availableReport({ outcome: 'failed', failure: { code: 'network', httpStatus: null }, asset: null }))).toBeUndefined()
    expect(offeredUpdate(availableReport({ outcome: 'upToDate' }))).toBeUndefined()
    expect(offeredUpdate(availableReport({ outcome: 'skipped', skipReason: 'throttled' }))).toBeUndefined()
    expect(offeredUpdate(undefined)).toBeUndefined()
  })

  it('已忽略的那个版本不再提示，更晚的版本仍然提示', () => {
    // 验收 3、4：`dismissed` 是原生按**版本等价**算好的（`0.4.1` 与 `0.4.1.0` 是同一个版本），
    // 界面不再实现第二遍。
    expect(offeredUpdate(availableReport({ dismissed: true, dismissedVersion: '0.4.2' }))).toBeUndefined()
    expect(updatePhase(availableReport({ dismissed: true, dismissedVersion: '0.4.2' }))).toBe('dismissed')
    // 更晚的版本：仍然是 available。
    expect(offeredUpdate(availableReport({ latestVersion: '0.4.3', dismissedVersion: '0.4.2' }))?.version).toBe('0.4.3')
    expect(updatePhase(availableReport())).toBe('available')
    expect(updatePhase(undefined)).toBe('idle')
  })

  it('发布页地址取自报告，缺了就退到仓库的 releases 页', () => {
    expect(releasePageUrl(availableReport())).toBe('https://github.com/kyorakuyk/dsh-wallpaper/releases/tag/v0.4.2')
    expect(releasePageUrl(availableReport({ releaseUrl: null }))).toBe(RELEASES_PAGE_URL)
    expect(releasePageUrl(undefined)).toBe(RELEASES_PAGE_URL)
  })

  it('这次发布没有可安装资产时仍然提示，只是主按钮不是「下载」', () => {
    const offered = offeredUpdate(availableReport({ outcome: 'noInstallableAsset', asset: null }))
    expect(offered?.version).toBe('0.4.2')
    expect(offered?.asset).toBeUndefined()
    // 气泡与设置卡片都按"有没有资产"决定主按钮那句话（§3.1 的回落）。
    const html = renderBubble(
      <UpdateBubble offer={offered!} phase="available" theme={bubbleTheme} onDownload={noop} onInstall={noop} onOpenReleasePage={noop} onDismiss={noop} />,
    )
    expect(html).toContain(t('update.action.release-page'))
    expect(html).not.toContain(`>${t('update.action.download')}<`)
    expect(html).toContain(t('update.action.dismiss'))
  })
})

describe('更新气泡的两个按钮', () => {
  it('可用更新时给出「下载」与「忽略」两个按钮', () => {
    const offer = offeredUpdate(availableReport())!
    const html = renderBubble(
      <UpdateBubble offer={offer} phase="available" theme={bubbleTheme} onDownload={noop} onInstall={noop} onOpenReleasePage={noop} onDismiss={noop} />,
    )

    // 正文带版本号；两个按钮各一枚（规矩 3：不许"点一下气泡就升级"，也不许"没点过就算忽略"）。
    expect(html).toContain(t('update.outcome.available', { version: '0.4.2' }))
    // 与设置卡片上那行「上次结果」是**同一句话**（同一组词条，不是两份措辞）。
    expect(html).toContain(formatMessage(updateOutcomeMessage(availableReport())))
    expect(html).toContain(`>${t('update.action.download')}<`)
    expect(html).toContain(`>${t('update.action.dismiss')}<`)
    expect(html.match(/<button/g)).toHaveLength(2)
    // 点气泡本体只 `stopPropagation`（立绘槽位的 onClick 是"打开对话"），不带任何动作。
    expect(html).toContain('data-interaction-region="update-bubble"')
  })

  it('「忽略」没落盘时提示行说出来，两个按钮还在（可以再按一次）', () => {
    const offer = offeredUpdate(availableReport())!
    const notice: Message = { key: 'update.notice.dismiss-unpersisted' }
    const html = renderBubble(
      <UpdateBubble offer={offer} phase="available" theme={bubbleTheme} notice={notice} onDownload={noop} onInstall={noop} onOpenReleasePage={noop} onDismiss={noop} />,
    )
    expect(html).toContain(formatMessage(notice))
    expect(html.match(/<button/g)).toHaveLength(2)
  })

  it('自动检查每个进程只发一次（模块级闸门）', () => {
    const gate = createOnceGate()
    expect(gate()).toBe(true)
    expect(gate()).toBe(false)
    expect(gate()).toBe(false)
  })
})

describe('气泡与卡片挂在这个位置上', () => {
  it('App 用 `updateBubbleVisible` 决定挂不挂，并把节点交给两个待机场景', async () => {
    const app = await readSource('src/App.tsx')
    expect(app).toContain('updateBubbleVisible({ workspace, phase: runtime.phase, offer: updateOffer })')
    // 表桌面阶段：条件不成立 → `undefined` → 场景里什么都不渲染。
    expect(app).toMatch(/updateBubbleNode = updateBubbleVisible\([\s\S]*?: undefined/)
    expect(app).toContain('updateBubble={updateBubbleNode}')
    // 自动检查的触发点就是"进入里桌面"（表桌面阶段不查）。
    expect(app).toMatch(/useUpdateBubble\(\{[\s\S]*?inInnerDesktop: workspace === 'inner'/)
  })

  it('气泡挂在立绘槽位里（`.portrait-slot`），两个场景都收这个节点', async () => {
    const idle = await readSource('src/scenes/IdleScene.tsx')
    const multi = await readSource('src/scenes/MultiScreenIdleScene.tsx')
    expect(idle).toContain('{updateBubble}')
    expect(multi).toContain('{updateBubble}')
    // 单屏那条路里槽位本身不是热区（只有里面的立绘是），所以壁纸窗口靠气泡自己那一条
    // `data-interaction-region` 才收得到鼠标消息 —— 否则真机上就是"点了没反应"。
    expect(await readSource('src/features/update/UpdateBubble.tsx')).toContain('data-interaction-region="update-bubble"')
    expect(await readSource('src/runtime/interactionRegions.ts')).toContain("'[data-interaction-region]'")
  })
})

describe('码翻译成人话', () => {
  it('五种结论、两种跳过原因、三种失败原因各有说法', () => {
    expect(formatMessage(updateOutcomeMessage(availableReport()))).toBe('有新版本 0.4.2')
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'upToDate' })))).toBe('已是最新')
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'noInstallableAsset', asset: null }))))
      .toBe('有新版本 0.4.2，但这次发布没有可安装的安装包')
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'skipped', skipReason: 'throttled' }))))
      .toBe('本次没有检查：距上次检查不足 6 小时')
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'skipped', skipReason: 'versionUnavailable' }))))
      .toBe('本次没有检查：读不到本机版本')
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'failed', failure: { code: 'httpStatus', httpStatus: 403 } }))))
      .toBe('检查失败：服务器返回 403')
    expect(formatMessage(updateFailureMessage({ code: 'network', httpStatus: null }))).toBe('网络不可用或请求超时')
    expect(formatMessage(updateFailureMessage({ code: 'malformedResponse', httpStatus: null }))).toBe('发布信息读不懂')
    // 没有状态码时不许印一个假的出来。
    expect(formatMessage(updateFailureMessage({ code: 'httpStatus', httpStatus: null }))).toBe('服务器返回了一个错误状态')
    expect(formatMessage(updateSkipMessage('throttled'))).toBe('距上次检查不足 6 小时')
    // 报告说"跳过/失败"却没带原因码（原生不该出现）：不替它挑一个原因，只说那半句。
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'skipped', skipReason: null })))).toBe('本次没有检查')
    expect(formatMessage(updateOutcomeMessage(availableReport({ outcome: 'failed', failure: null, asset: null })))).toBe('检查失败')
  })

  it('结论嵌着原因，切到英文整句一起变', () => {
    const failure = updateOutcomeMessage(availableReport({ outcome: 'failed', failure: { code: 'httpStatus', httpStatus: 503 } }))
    setLanguage('en')
    const english = formatMessage(failure)
    expect(english).not.toContain('检查失败')
    expect(english).toContain('503')
    expect(formatMessage(updateFailureMessage({ code: 'network', httpStatus: null }))).toBe('the network is unavailable or the request timed out')
    setLanguage('zh')
    // 同一份**没求值的**值，切回来又是中文（这正是"存 Message 不存句子"要的效果）。
    expect(formatMessage(failure)).toBe('检查失败：服务器返回 503')
  })
})

describe('两个面都说同一句话', () => {
  it('气泡与设置卡片共用同一组词条', async () => {
    const bubbleSource = await readSource('src/features/update/UpdateBubble.tsx')
    const panelSource = await readSource('src/settings/SettingsPanel.tsx')
    // 同一句话在两处渲染，但键只有一份：气泡说 `update.outcome.available`，卡片也说它。
    expect(bubbleSource).toContain("msg('update.outcome.available', { version: offer.version })")
    expect(panelSource).toContain('updateOutcomeMessage(updateReport)')
    for (const source of [bubbleSource, panelSource]) {
      expect(source).toContain("t('update.action.download')")
      expect(source).toContain("t('update.action.dismiss')")
    }
  })

  it('设置卡片：当前版本、上次检查、检查按钮与那几个动作都在（与气泡同一个状态机）', async () => {
    const panel = await readSource('src/settings/SettingsPanel.tsx')
    const window = await readSource('src/settings/SettingsWindow.tsx')

    expect(panel).toContain("t('settings.system.update.title')")
    expect(panel).toContain("t('settings.system.update.current.title')")
    // 读不到本机版本时说清楚（§八 9：非打包运行不报错、也不猜版本）。
    expect(panel).toContain("t('settings.system.update.current.unavailable')")
    expect(panel).toContain("t('settings.system.update.last-check.title')")
    expect(panel).toContain("t('settings.system.update.last-check.never')")
    expect(panel).toContain('onClick={props.onCheckForUpdates}')
    // 有可用更新时，同一张卡片上就是气泡那几个动作（同一状态机、同一份状态）。
    expect(panel).toContain('{updateOffer && <>')
    expect(panel).toContain('updatePhase(updateReport, props.updateDownload)')
    expect(panel).toContain('props.onDownloadUpdate')
    expect(panel).toContain('props.onInstallUpdate')
    expect(panel).toContain('props.onOpenUpdatePage')
    expect(panel).toContain('props.onDismissUpdate(updateOffer.version)')
    // 下载中卡片给的是同一句话（进度），失败时把原因显示在卡片上（§四、§八 7）。
    expect(panel).toContain('downloadProgressMessage(props.updateDownload)')
    expect(panel).toContain('downloadFailedMessage(props.updateDownload)')
  })

  it('那枚「检查更新」按钮走的是手动检查（不受 6 小时节流），失败原因也显示在卡片上', async () => {
    const window = await readSource('src/settings/SettingsWindow.tsx')
    // 手动：`check(true)`；探针（打开系统页那一次）走同一条路 —— 于是卡片上一定有真结论。
    expect(window).toMatch(/const checkForUpdates = async \(\) => \{[\s\S]*?check\(true\)/)
    expect(window).toMatch(/updateStatus: async \(\) => \{[\s\S]*?updateRef\.current\.check\(true\)/)
    expect(window).toContain('onCheckForUpdates={() => { void checkForUpdates() }}')
    // 没落盘/失败那两句由控制器写进 `update.notice`，卡片原地显示（不另弹一条通知说同一件事）。
    expect(window).toContain('updateNotice={update.notice}')
  })
})

/**
 * **第三片把下面这一条翻了过来 —— 这是本片最重要的一处断言改动。**
 *
 * 第二片这里钉的是**反方向**，原文（提交 `edb0ffe`，`describe('第三片要接的手，本片不假装')`）：
 *
 * ```ts
 * expect(hook).not.toContain('update_download')          // 本片没有下载命令可调
 * expect(state).not.toContain("return 'downloading'")     // 那三个状态没有产生它们的代码路径
 * expect(state).not.toContain("return 'ready'")
 * ```
 *
 * 那是对的：那时界面上还没有下载，任何进度都只能是界面自己编出来的。第三片把原生侧的流式下载
 * （`update_download` + `update-download` 事件）接上之后，"这三个状态可达"从**不许**变成了
 * **必须** —— 所以断言翻转，而不是删掉：删掉的话"界面自己编一个进度出来"这件事就再也没人盯着。
 *
 * 翻转之后仍然成立的那一半（也在这一组里钉着）：三个状态**只由事件产生**，界面不许预置一个
 * `downloading`（`useUpdate` 里那句 `setDownload` 只出现在事件回调上），而「打开发布页」降级成
 * §六 的回落、仍在代码里。
 */
describe('三个新状态真的可达，而且只由事件驱动', () => {
  it('「下载」调的是原生下载命令，不再是"打开发布页"', async () => {
    const hook = await readSource('src/features/update/useUpdate.ts')

    expect(hook).toContain('nativeRuntime.updateDownload(')
    expect(hook).toContain('nativeRuntime.updateInstall()')
    // 只有一个调用点，而且就在 downloadUpdate 那一段里（见下一条：检查那条路上一个都没有）。
    expect(hook.match(/nativeRuntime\.updateDownload\(/g)).toHaveLength(1)
    const downloadBody = hook.slice(
      hook.indexOf('const downloadUpdate = useCallback'),
      hook.indexOf('const install = useCallback'),
    )
    expect(downloadBody).toContain('nativeRuntime.updateDownload(offer.version, offer.asset)')
    // 「打开发布页」还在：它是 §六 规定的回落，只是不再是主按钮。
    expect(hook).toContain('nativeRuntime.openExternalLink(offer.releaseUrl)')
    // 临时实现的那个 TODO 已经兑现。
    expect(hook).not.toContain('TODO（第三片）')
  })

  it('检查那条路上没有下载：下载只可能在按下「下载」之后发生（§六）', async () => {
    const hook = await readSource('src/features/update/useUpdate.ts')
    const checkBody = hook.slice(hook.indexOf('const check = useCallback'), hook.indexOf('const checkIfDue'))
    // 检查里不许出现下载相关的调用（`updateDownload` / `install` / 打开发布页）。
    expect(checkBody).not.toContain('updateDownload')
    expect(checkBody).not.toContain('openExternalLink')
    // 原生侧同样的声明由 Rust 那条 `checking_never_downloads_anything` 钉着。
    const rust = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'update', 'commands.rs'), 'utf8')
    expect(rust).toContain('fn checking_never_downloads_anything')
  })

  it('事件是那三个状态的唯一来源：`setDownload` 只出现在事件回调上', async () => {
    const hook = await readSource('src/features/update/useUpdate.ts')
    // 界面自己预置一个 `downloading`（点一下就先摆出进度条）就是"假装下载"：那会让"点了没反应"
    // 与"原生真的开工了"看起来一样。这条钉着它不发生。
    expect(hook.match(/setDownload\(/g)).toHaveLength(1)
    expect(hook).toContain('setDownload(applyDownloadEvent(event))')
    expect(hook).not.toContain("setDownload({")
    // 订阅走的是生命周期助手（挂载后立刻卸载时不会漏掉那个原生监听器）。
    expect(hook).toContain('listenUntilDisposed<UpdateDownloadEvent>')
    expect(hook).toContain('nativeRuntime.listenUpdateDownload(receive)')
  })

  it('进度走一条**全局**事件：两个窗口都收得到（上一片缺的就是它）', async () => {
    const runtime = await readSource('src/native/runtime.ts')
    expect(runtime).toContain("listen<UpdateDownloadEvent>('update-download'")
    const rust = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'update', 'commands.rs'), 'utf8')
    const events = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'update', 'download.rs'), 'utf8')
    // 名字只有一份（`DOWNLOAD_EVENT`），发的是**全局** `emit`：`emit_to` 只能到某一个窗口，
    // 而这里要的是"壁纸按的和设置页按的都让两边一起动"。
    expect(events).toContain('pub(crate) const DOWNLOAD_EVENT: &str = "update-download";')
    expect(rust).toContain('app.emit(download::DOWNLOAD_EVENT, event)')
    expect(rust).not.toContain('emit_to')
    // 两个窗口都挂在同一个控制器上：壁纸（`useUpdateBubble` → `useUpdate`）与设置中心。
    const app = await readSource('src/App.tsx')
    const window = await readSource('src/settings/SettingsWindow.tsx')
    expect(app).toMatch(/useUpdateBubble\(\{/)
    expect(window).toContain('useUpdate()')
  })

  /**
   * 「点击安装」这一条命令的**两种走法**各说各的话（原生用 `nextStep` 告诉界面是哪种）。
   *
   * 为什么这一条重要：`exiting` 那一次调用之后应用就关了 —— 界面**没有下一次说话的机会**，
   * 所以"正在退出以便安装"必须当场说出来；而 `opened`（助手起不来）时应用还在，界面要说的是
   * "已直接打开安装包，应用不会退出"。两句话混起来，用户就会以为窗口会关（或者以为它不会关）。
   */
  it('「点击安装」的两种走法各说各的话：正在退出 / 已直接打开', async () => {
    const hook = await readSource('src/features/update/useUpdate.ts')
    const installBody = hook.slice(
      hook.indexOf('const install = useCallback'),
      // `openReleasePage` 是普通函数（没有 `useCallback`），所以下界取那之后的第一个锚点。
      hook.indexOf('return useMemo('),
    )
    // 分支读的就是原生那个字段，而且两条分支各对应一条词条。
    expect(installBody).toContain("result.nextStep === 'exiting'")
    expect(installBody).toContain("msg('update.notice.install-exiting')")
    expect(installBody).toContain("msg('update.notice.install-fallback-opened')")
    // 旧的"已交给 Windows 的{target}"那句话没了：`nextStep` 把两种走法分开了，界面不该再猜
    // 交给的是哪一种处理程序。
    expect(hook).not.toContain('install-handed-off')

    setLanguage('zh')
    expect(formatMessage(msg('update.notice.install-exiting'))).toContain('退出')
    expect(formatMessage(msg('update.notice.install-fallback-opened'))).toContain('不会退出')
    setLanguage('en')
    expect(formatMessage(msg('update.notice.install-exiting'))).toContain('Exiting')
    expect(formatMessage(msg('update.notice.install-fallback-opened'))).toContain('stay open')
    setLanguage('zh')

    // 原生侧那两半同样钉在 Rust 里（`InstallLaunch` ⇄ `nextStep`），名字对不上界面就分不清走法。
    const rust = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'update', 'commands.rs'), 'utf8')
    expect(rust).toContain('pub next_step: InstallLaunch')
    expect(rust).toContain('let report = run_install(&state_path)?;')
    expect(rust).toContain('app.exit(0);')
  })
})

describe('三个新状态由事件驱动', () => {
  it('原生事件 → 界面状态（进度、就绪、失败）', () => {
    const progress = applyDownloadEvent(downloadEvent({ downloadedBytes: 1024, totalBytes: 4096 }))
    expect(progress).toEqual({
      version: '0.4.2',
      phase: 'downloading',
      downloadedBytes: 1024,
      totalBytes: 4096,
      path: undefined,
      sha256: undefined,
      failure: undefined,
    })

    const ready = applyDownloadEvent(
      downloadEvent({
        phase: 'ready',
        downloadedBytes: 4096,
        totalBytes: 4096,
        path: 'C:\\Users\\me\\AppData\\Local\\com.dsh.wallpaper\\updates\\0.4.2\\setup.exe',
        sha256: 'sha256:ab',
      }),
    )
    expect(ready.phase).toBe('ready')
    expect(ready.path).toContain('0.4.2')
    expect(ready.sha256).toBe('sha256:ab')

    const failed = failedEvent('diskFull')
    expect(failed.phase).toBe('failed')
    expect(failed.failure?.code).toBe('diskFull')

    // 原生没给总大小时（`totalBytes: null`）就是"不知道"，不是 0。
    expect(applyDownloadEvent(downloadEvent({ totalBytes: null })).totalBytes).toBeUndefined()
  })

  it('状态迁移：可用 → 下载中 → 就绪 / 失败，全部由事件推动（§四 那张图）', () => {
    const report = availableReport()
    // 报告给前三个状态。
    expect(updatePhase(report)).toBe('available')
    // 事件给后三个。
    expect(updatePhase(report, applyDownloadEvent(downloadEvent()))).toBe('downloading')
    expect(updatePhase(report, applyDownloadEvent(downloadEvent({ phase: 'ready' })))).toBe('ready')
    expect(updatePhase(report, failedEvent('network'))).toBe('failed')
    // 「重试」：失败之后再来一条进度事件，又回到下载中。
    expect(updatePhase(report, applyDownloadEvent(downloadEvent()))).toBe('downloading')
    // 事件缺席时那三个状态一个都不出现 —— 第二片那条断言的"另一半"仍然成立。
    expect(updatePhase(report, undefined)).toBe('available')
    // 别的版本的旧事件不作数（上一轮下过 0.4.2，现在提示的是 0.4.3）。
    expect(updatePhase(report, applyDownloadEvent(downloadEvent({ version: '0.4.3', phase: 'ready' })))).toBe('available')
    expect(updatePhase(report, applyDownloadEvent(downloadEvent({ version: 'v0.4.2', phase: 'ready' })))).toBe('ready')
    // 已忽略的版本不提示，也就没有下载可言。
    const dismissed = availableReport({ dismissed: true, dismissedVersion: '0.4.2' })
    expect(updatePhase(dismissed, applyDownloadEvent(downloadEvent({ phase: 'ready' })))).toBe('dismissed')
  })

  it('进度：有总大小说百分比，没有就说已下载多少，超了也不印 130%', () => {
    expect(downloadPercent(applyDownloadEvent(downloadEvent({ downloadedBytes: 0, totalBytes: 100 })))).toBe(0)
    expect(downloadPercent(applyDownloadEvent(downloadEvent({ downloadedBytes: 42, totalBytes: 100 })))).toBe(42)
    expect(downloadPercent(applyDownloadEvent(downloadEvent({ downloadedBytes: 100, totalBytes: 100 })))).toBe(100)
    // 总大小未知：没有百分比，而不是一个编出来的数字。
    const unknown = applyDownloadEvent(downloadEvent({ downloadedBytes: 3 * 1024 * 1024, totalBytes: null }))
    expect(downloadPercent(unknown)).toBeUndefined()
    expect(downloadPercent(undefined)).toBeUndefined()
    // 已经超过总大小（不该发生）：封在 100。
    expect(downloadPercent(applyDownloadEvent(downloadEvent({ downloadedBytes: 130, totalBytes: 100 })))).toBe(100)
    // 进度条上那两个数字也是这么说的。
    expect(formatMessage(downloadProgressMessage(applyDownloadEvent(downloadEvent({ downloadedBytes: 42, totalBytes: 100 })))))
      .toBe('正在下载 42%')
    expect(formatMessage(downloadProgressMessage(unknown))).toBe('正在下载（已下载 3.0 MB）')
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(31_457_280)).toBe('30.0 MB')
    // 两句都进词条（中英各一份，见最后一组）。
    setLanguage('en')
    expect(formatMessage(downloadProgressMessage(unknown))).toBe('Downloading (3.0 MB so far)')
    setLanguage('zh')
  })

  it('失败的原因码翻成人话（磁盘满、写不下去、保存位置、校验不符）', () => {
    expect(formatMessage(downloadFailureMessage(failedEvent('diskFull').failure!))).toBe('磁盘空间不足')
    expect(formatMessage(downloadFailureMessage(failedEvent('writeFailed').failure!))).toBe('文件写不下去（可能是权限或磁盘问题）')
    expect(formatMessage(downloadFailureMessage(failedEvent('destinationUnavailable').failure!)))
      .toBe('保存位置不可用（目录建不出来）')
    expect(formatMessage(downloadFailureMessage(failedEvent('network').failure!))).toBe('网络不可用或请求超时')
    expect(formatMessage(downloadFailureMessage(failedEvent('httpStatus', { httpStatus: 404 }).failure!)))
      .toBe('服务器返回 404')
    expect(
      formatMessage(
        downloadFailureMessage(
          failedEvent('sizeMismatch', { expectedBytes: 31_457_280, actualBytes: 1024 }).failure!,
        ),
      ),
    ).toBe('文件大小与发布信息不符（应为 30.0 MB，实际 1.0 KB）')
    expect(formatMessage(downloadFailureMessage(failedEvent('digestMismatch').failure!))).toBe('文件校验和不符（已删除这个文件）')
    // 两个数字缺一个时不印假数字。
    expect(formatMessage(downloadFailureMessage(failedEvent('sizeMismatch').failure!))).toBe('文件没有通过校验')
    setLanguage('en')
    expect(formatMessage(downloadFailureMessage(failedEvent('diskFull').failure!))).toBe('there is not enough disk space')
    setLanguage('zh')
    // `failed` 那一整句（气泡正文与设置卡片共用）。
    expect(formatMessage(downloadFailedMessage(failedEvent('diskFull')))).toBe('下载失败：磁盘空间不足')
    // 没有原因码时只说那半句：不替原生挑一个原因。
    expect(formatMessage(downloadFailedMessage(undefined))).toBe('下载失败')
  })

  it('原生拒绝一次调用：按码说人话，不是 `[object Object]`', () => {
    // 原生给的是一个对象（`{ code }`）：`String(error)` 只会得到 `[object Object]`，
    // 所以这些码必须各有各的句子。
    expect(formatMessage(updateCallMessage({ code: 'forbidden' }))).toContain('只有壁纸与设置中心')
    expect(formatMessage(updateCallMessage({ code: 'untrustedAssetUrl' }))).toBe('下载地址不是发布仓库（已拒绝）')
    expect(formatMessage(updateCallMessage({ code: 'installerMissing' }))).toBe('下载好的安装包已经不在原来的位置了')
    expect(formatMessage(updateCallMessage({ code: 'unsupportedAsset' }))).toContain('.msix')
    expect(formatMessage(updateCallMessage({ code: 'openFailed' }))).toBe('Windows 没有打开这个安装包')
    // 认不出的码：回落原文，而不是假装知道原因。
    expect(formatMessage(updateCallMessage({ code: 'somethingNewer' }))).toContain('somethingNewer')
    expect(formatMessage(updateCallMessage('plain text error'))).toContain('plain text error')
  })
})

describe('气泡按状态画', () => {
  const offer = offeredUpdate(availableReport())!

  it('下载中是进度条：正文说进度、有条子、一个按钮都没有', () => {
    const html = renderBubble(
      <UpdateBubble
        offer={offer}
        phase="downloading"
        download={applyDownloadEvent(downloadEvent({ downloadedBytes: 42, totalBytes: 100 }))}
        theme={bubbleTheme}
        onDownload={noop}
        onInstall={noop}
        onOpenReleasePage={noop}
        onDismiss={noop}
      />,
    )
    expect(html).toContain('正在下载 42%')
    expect(html).toContain('role="progressbar"')
    expect(html).toContain('aria-valuenow="42"')
    expect(html).toContain('width:42%')
    // §十：下载中不提供取消 —— 一个按钮都没有（也就没有"点了没反应"的按钮）。
    expect(html).not.toContain('<button')
  })

  it('总大小未知时：条子是不定态（没有 aria-valuenow），正文说已下载多少', () => {
    const html = renderBubble(
      <UpdateBubble
        offer={offer}
        phase="downloading"
        download={applyDownloadEvent(downloadEvent({ downloadedBytes: 2048, totalBytes: null }))}
        theme={bubbleTheme}
        onDownload={noop}
        onInstall={noop}
        onOpenReleasePage={noop}
        onDismiss={noop}
      />,
    )
    expect(html).toContain('正在下载（已下载 2.0 KB）')
    expect(html).toContain('data-known="unknown"')
    expect(html).not.toContain('aria-valuenow')
  })

  it('就绪是「点击安装」；失败是可读原因 + 「重试」+「打开发布页」', () => {
    const ready = renderBubble(
      <UpdateBubble
        offer={offer}
        phase="ready"
        download={applyDownloadEvent(downloadEvent({ phase: 'ready', downloadedBytes: 100, totalBytes: 100, path: 'C:\\u\\setup.exe' }))}
        theme={bubbleTheme}
        onDownload={noop}
        onInstall={noop}
        onOpenReleasePage={noop}
        onDismiss={noop}
      />,
    )
    expect(ready).toContain('新版本 0.4.2 已下载')
    expect(ready).toContain(`>${t('update.action.install')}<`)
    expect(ready).toContain(`>${t('update.action.dismiss')}<`)
    expect(ready.match(/<button/g)).toHaveLength(2)

    const failed = renderBubble(
      <UpdateBubble
        offer={offer}
        phase="failed"
        download={failedEvent('diskFull')}
        theme={bubbleTheme}
        onDownload={noop}
        onInstall={noop}
        onOpenReleasePage={noop}
        onDismiss={noop}
      />,
    )
    expect(failed).toContain('下载失败：磁盘空间不足')
    expect(failed).toContain(`>${t('update.action.retry')}<`)
    // §六 的回落：下载失败时给「打开发布页」。
    expect(failed).toContain(`>${t('update.action.release-page')}<`)
    expect(failed.match(/<button/g)).toHaveLength(2)
  })
})

describe('原生命令边界', () => {
  it('四个命令的参数形状就是原生读的那些字段名', async () => {
    const invokes = fakeTauri((command) => {
      switch (command) {
        case 'update_check':
          return availableReport()
        case 'update_dismiss':
          return { dismissedVersion: '0.4.2', persisted: true }
        case 'update_download':
          return { started: true, version: '0.4.2', destination: 'C:\\updates\\0.4.2\\setup.exe' }
        default:
          return { path: 'C:\\updates\\0.4.2\\setup.exe', kind: 'exe', nextStep: 'exiting' }
      }
    })

    const report = await nativeRuntime.updateCheck(true)
    expect(report?.latestVersion).toBe('0.4.2')
    const dismissed = await nativeRuntime.updateDismiss('0.4.2')
    const started = await nativeRuntime.updateDownload('0.4.2', availableReport().asset!)
    const installed = await nativeRuntime.updateInstall()

    expect(invokes).toEqual([
      { command: 'update_check', args: { manual: true } },
      { command: 'update_dismiss', args: { version: '0.4.2' } },
      // 「下载」把**报告里选中的那一个资产**原样递过去（`version` + `asset`）：资产选择在检查那一步
      // 就做完了（§六），界面不再选一遍。
      {
        command: 'update_download',
        args: {
          version: '0.4.2',
          asset: {
            name: 'dsh-wallpaper_0.4.2_x64-setup.exe',
            size: 31_457_280,
            downloadUrl: 'https://github.com/kyorakuyk/dsh-wallpaper/releases/download/v0.4.2/dsh-wallpaper_0.4.2_x64-setup.exe',
            digest: null,
          },
        },
      },
      // 「安装」不带参数：路径由原生从状态文件里读（界面无法让它打开任意文件）。
      { command: 'update_install', args: {} },
    ])
    expect(dismissed).toEqual({ dismissedVersion: '0.4.2', persisted: true })
    expect(started).toEqual({ started: true, version: '0.4.2', destination: 'C:\\updates\\0.4.2\\setup.exe' })
    expect(installed).toEqual({ path: 'C:\\updates\\0.4.2\\setup.exe', kind: 'exe', nextStep: 'exiting' })
  })

  it('预览（没有原生宿主）不伪造报告，也不假装忽略成功或开工下载', async () => {
    vi.stubGlobal('window', {})
    expect(await nativeRuntime.updateCheck(false)).toBeUndefined()
    // 没有状态文件可写：如实回报"没落盘"，界面照同一句话处理。
    expect(await nativeRuntime.updateDismiss('0.4.2')).toEqual({ dismissedVersion: '0.4.2', persisted: false })
    // 预览里没有下载：`undefined`（不是一份"已开工"的回执，界面据此说"当前环境没有原生下载"）。
    expect(await nativeRuntime.updateDownload('0.4.2', availableReport().asset!)).toBeUndefined()
    expect(await nativeRuntime.updateInstall()).toBeUndefined()
    // 订阅也一样：给一个空 disposer，而不是在预览里憋出一个永远不会来的进度。
    expect(typeof (await nativeRuntime.listenUpdateDownload(() => undefined))).toBe('function')
  })

  it('两个窗口的 capability 都授权了这四条命令（缺一条就是"点了没反应"）', async () => {
    const capability = async (name: string) => JSON.parse(
      await readFile(resolve(wallpaperRoot, 'src-tauri', 'capabilities', `${name}.json`), 'utf8'),
    ) as { permissions: string[] }

    for (const name of ['background', 'settings']) {
      const permissions = (await capability(name)).permissions
      expect(permissions, name).toContain('allow-update-check')
      expect(permissions, name).toContain('allow-update-dismiss')
      // 第三片加的两条：气泡与设置卡片都要能「下载」与「点击安装」。
      expect(permissions, name).toContain('allow-update-download')
      expect(permissions, name).toContain('allow-update-install')
    }
    // Lite 两个窗口都不给（命令本身也不在 Lite 里，计划书 §七）。
    for (const name of ['lite-background', 'lite-settings']) {
      const permissions = (await capability(name)).permissions
      for (const permission of [
        'allow-update-check',
        'allow-update-dismiss',
        'allow-update-download',
        'allow-update-install',
      ]) {
        expect(permissions, name).not.toContain(permission)
      }
    }
  })

  it('四条命令在原生侧四处都登记齐全（permission 文件、build.rs、generate_handler!）', async () => {
    // 表驱动的那条守卫测试在 `update/commands.rs` 里；这里只确认它覆盖了四条（少一条就是漏一处，
    // 而漏了在构建期一声不响）。
    const rust = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'update', 'commands.rs'), 'utf8')
    for (const command of ['update_check', 'update_dismiss', 'update_download', 'update_install']) {
      expect(rust, command).toContain(`("${command}", "allow-`)
    }
    const build = await readFile(resolve(wallpaperRoot, 'src-tauri', 'build.rs'), 'utf8')
    for (const command of ['update_download', 'update_install']) {
      expect(build, command).toContain(`"${command}"`)
    }
    // 进度事件**不**需要单独的 ACL 条目（事件不是命令）；要的是 `core:event:allow-listen`，
    // 两个窗口本来就有（订阅别的窗口广播时用过）。
    for (const name of ['background', 'settings']) {
      const permissions = (await capabilityOf(name)).permissions
      expect(permissions, name).toContain('core:event:allow-listen')
    }
  })
})

/** 读一个窗口的 capability（上面两组测试共用）。 */
async function capabilityOf(name: string): Promise<{ permissions: string[] }> {
  return JSON.parse(
    await readFile(resolve(wallpaperRoot, 'src-tauri', 'capabilities', `${name}.json`), 'utf8'),
  ) as { permissions: string[] }
}

describe('词条', () => {
  it('每一句新文案中英各一份，而且都住在完整版那一半', async () => {
    const keys: MessageKey[] = [
      'settings.system.update.title',
      'settings.system.update.description',
      'settings.system.update.current.title',
      'settings.system.update.current.unavailable',
      'settings.system.update.last-check.title',
      'settings.system.update.last-check.never',
      'settings.system.update.check',
      'settings.system.update.checking',
      'settings.probe.update',
      'update.outcome.available',
      'update.outcome.available-unversioned',
      'update.outcome.up-to-date',
      'update.outcome.no-asset',
      'update.outcome.skipped',
      'update.outcome.failed',
      'update.outcome.skipped-unstated',
      'update.outcome.failed-unstated',
      'update.skip.throttled',
      'update.skip.version-unavailable',
      'update.failure.network',
      'update.failure.http-status',
      'update.failure.http-status-unknown',
      'update.failure.malformed-response',
      'update.action.download',
      'update.action.dismiss',
      'update.action.release-page',
      // 第三片（下载与安装）新增的那些。
      'update.action.install',
      'update.action.retry',
      'update.outcome.ready',
      'update.progress.percent',
      'update.progress.downloaded',
      'update.download.failed',
      'update.download.failed-unstated',
      'update.download.failure.destination-unavailable',
      'update.download.failure.write-failed',
      'update.download.failure.disk-full',
      'update.download.failure.size-mismatch',
      'update.download.failure.digest-mismatch',
      'update.download.failure.verification',
      'update.download.failure.unknown',
      'update.call.forbidden',
      'update.call.state-path-unavailable',
      'update.call.invalid-version',
      'update.call.destination-unavailable',
      'update.call.untrusted-asset-url',
      'update.call.nothing-downloaded',
      'update.call.installer-missing',
      'update.call.unsupported-asset',
      'update.call.open-failed',
      'update.call.unknown',
      'update.notice.listen-failed',
      'update.notice.download-failed',
      'update.notice.update-unavailable',
      'update.notice.install-failed',
      // 「点击安装」之后那两句：安排好了就说"正在退出以便安装"（这句话必须在退出**之前**说 ——
      // 应用一关，界面就没有下一次说话的机会），助手起不来时才说回落那句。
      'update.notice.install-exiting',
      'update.notice.install-fallback-opened',
      'update.notice.dismiss-unpersisted',
      'update.notice.dismiss-failed',
      'update.notice.check-failed',
      'update.notice.open-release-failed',
    ]
    const zh = await dictionarySource('zh')
    const en = await dictionarySource('en')
    for (const key of keys) {
      expect(zh, `zh 缺 ${key}`).toContain(`'${key}':`)
      expect(en, `en 缺 ${key}`).toContain(`'${key}':`)
      // 两边都登记了才不会回落成键名（回落是给"真出事了"用的兜底，不是常态）。
      expect(t(key), key).not.toBe(key)
    }
    // Lite 那一半不许出现更新检测的话：Lite 本次不做（§七），加错地方会让边界校验当场失败。
    for (const half of ['zh.shared.ts', 'en.shared.ts']) {
      const source = await readSource(`src/i18n/${half}`)
      expect(source).not.toContain("'update.")
      expect(source).not.toContain("'settings.system.update.")
    }
  })

  it('「下载」与「忽略」就是按钮上那两个词', () => {
    expect(t('update.action.download')).toBe('下载')
    expect(t('update.action.dismiss')).toBe('忽略')
    setLanguage('en')
    expect(t('update.action.download')).toBe('Download')
    expect(t('update.action.dismiss')).toBe('Ignore')
  })

  it('发布页回落与原生侧的仓库常量是同一个仓库', async () => {
    const rust = await readFile(resolve(wallpaperRoot, 'src-tauri', 'src', 'update', 'mod.rs'), 'utf8')
    const repository = /RELEASE_REPOSITORY: &str = "([^"]+)"/.exec(rust)?.[1]
    expect(repository).toBeTruthy()
    // 两边各写一份还各说各的，就会去打开一个别人的仓库。
    expect(RELEASES_PAGE_URL).toBe(`https://github.com/${repository}/releases`)
  })
})
