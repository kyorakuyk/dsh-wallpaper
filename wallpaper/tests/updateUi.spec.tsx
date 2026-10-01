import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { ReactElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { UpdateBubble } from '../src/features/update/UpdateBubble.tsx'
import { updateFailureMessage, updateOutcomeMessage, updateSkipMessage } from '../src/features/update/updateCopy.ts'
import { RELEASES_PAGE_URL, offeredUpdate, releasePageUrl, updateBubbleVisible, updatePhase } from '../src/features/update/updateState.ts'
import { createOnceGate } from '../src/features/update/useUpdate.ts'
import { formatMessage, setLanguage, t, type Message, type MessageKey } from '../src/i18n/index.ts'
import { nativeRuntime, type UpdateCheckReport } from '../src/native/runtime.ts'
import { BUILTIN_PERSONAS } from '../src/persona/registry.ts'
import { dictionarySource } from './i18nSource.ts'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

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
    const html = renderBubble(<UpdateBubble offer={offered!} theme={BUILTIN_PERSONAS['blue-child'].theme} onDownload={() => undefined} onDismiss={() => undefined} />)
    expect(html).toContain(t('update.action.release-page'))
    expect(html).not.toContain(`>${t('update.action.download')}<`)
    expect(html).toContain(t('update.action.dismiss'))
  })
})

describe('更新气泡的两个按钮', () => {
  it('可用更新时给出「下载」与「忽略」两个按钮', () => {
    const offer = offeredUpdate(availableReport())!
    const html = renderBubble(
      <UpdateBubble offer={offer} theme={BUILTIN_PERSONAS['blue-child'].theme} onDownload={() => undefined} onDismiss={() => undefined} />,
    )

    // 正文带版本号；两个按钮各一枚（规矩 3：不许"点一下气泡就升级"，也不许"没点过就算忽略"）。
    expect(html).toContain(t('update.outcome.available', { version: '0.4.2' }))
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
      <UpdateBubble offer={offer} theme={BUILTIN_PERSONAS['blue-child'].theme} notice={notice} onDownload={() => undefined} onDismiss={() => undefined} />,
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
    expect(bubbleSource).toContain("t('update.outcome.available', { version: offer.version })")
    expect(panelSource).toContain('updateOutcomeMessage(updateReport)')
    for (const source of [bubbleSource, panelSource]) {
      expect(source).toContain("t('update.action.download')")
      expect(source).toContain("t('update.action.dismiss')")
    }
  })

  it('设置卡片：当前版本、上次检查、检查按钮与那两个按钮都在', async () => {
    const panel = await readSource('src/settings/SettingsPanel.tsx')
    const window = await readSource('src/settings/SettingsWindow.tsx')

    expect(panel).toContain("t('settings.system.update.title')")
    expect(panel).toContain("t('settings.system.update.current.title')")
    // 读不到本机版本时说清楚（§八 9：非打包运行不报错、也不猜版本）。
    expect(panel).toContain("t('settings.system.update.current.unavailable')")
    expect(panel).toContain("t('settings.system.update.last-check.title')")
    expect(panel).toContain("t('settings.system.update.last-check.never')")
    expect(panel).toContain('onClick={props.onCheckForUpdates}')
    // 有可用更新时，同一张卡片上就是气泡那两个按钮（同一状态机、同一份状态）。
    expect(panel).toContain('{updateOffer && <>')
    expect(panel).toContain('props.onDownloadUpdate')
    expect(panel).toContain('props.onDismissUpdate(updateOffer.version)')
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

describe('第三片要接的手，本片不假装', () => {
  it('「下载」本片是打开发布页，而且这个临时实现写在代码里', async () => {
    const hook = await readSource('src/features/update/useUpdate.ts')
    // 打开发布页走的就是现有的打开外链路径（§六 的回落路径），地址取自报告（缺了退到 releases 页）。
    expect(hook).toContain('nativeRuntime.openExternalLink(offer.releaseUrl)')
    expect(hook).toContain('offerRef.current')
    // 本片**不许**发一次注定失败的调用：没有可提示的更新时按钮不存在，真被调到就当没按。
    expect(hook).toMatch(/const offer = offerRef\.current[\s\S]*?if \(!offer\) \{/)
    // 临时实现必须留着手：第三片换成真下载 + 进度 + 安装。
    expect(hook).toContain('TODO（第三片）')
    // 本片**没有**下载命令可调 —— 有的话就是假装实现了。
    expect(hook).not.toContain('update_download')
    // 状态机里那三个状态只有注释，没有产生它们的代码路径。
    const state = await readSource('src/features/update/updateState.ts')
    expect(state).toContain("'downloading' | 'ready' | 'failed'")
    expect(state).toContain('第三片')
    expect(state).not.toContain("return 'downloading'")
    expect(state).not.toContain("return 'ready'")
  })
})

describe('原生命令边界', () => {
  it('两个命令的参数形状就是原生读的那些字段名', async () => {
    const invokes = fakeTauri((command) => (command === 'update_check'
      ? availableReport()
      : { dismissedVersion: '0.4.2', persisted: true }))

    const report = await nativeRuntime.updateCheck(true)
    expect(report?.latestVersion).toBe('0.4.2')
    const dismissed = await nativeRuntime.updateDismiss('0.4.2')

    expect(invokes).toEqual([
      { command: 'update_check', args: { manual: true } },
      { command: 'update_dismiss', args: { version: '0.4.2' } },
    ])
    expect(dismissed).toEqual({ dismissedVersion: '0.4.2', persisted: true })
  })

  it('预览（没有原生宿主）不伪造报告，也不假装忽略成功', async () => {
    vi.stubGlobal('window', {})
    expect(await nativeRuntime.updateCheck(false)).toBeUndefined()
    // 没有状态文件可写：如实回报"没落盘"，界面照同一句话处理。
    expect(await nativeRuntime.updateDismiss('0.4.2')).toEqual({ dismissedVersion: '0.4.2', persisted: false })
  })

  it('两个窗口的 capability 都授权了这两条命令（缺一条就是"点了没反应"）', async () => {
    const capability = async (name: string) => JSON.parse(
      await readFile(resolve(wallpaperRoot, 'src-tauri', 'capabilities', `${name}.json`), 'utf8'),
    ) as { permissions: string[] }

    for (const name of ['background', 'settings']) {
      const permissions = (await capability(name)).permissions
      expect(permissions, name).toContain('allow-update-check')
      expect(permissions, name).toContain('allow-update-dismiss')
    }
    // Lite 两个窗口都不给（命令本身也不在 Lite 里，计划书 §七）。
    for (const name of ['lite-background', 'lite-settings']) {
      const permissions = (await capability(name)).permissions
      expect(permissions, name).not.toContain('allow-update-check')
      expect(permissions, name).not.toContain('allow-update-dismiss')
    }
  })
})

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
