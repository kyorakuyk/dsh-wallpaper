import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import type { ChatEvent } from '../src/domain/types.ts'
import { formatMessage, formatSentence, getLanguage, msg, setLanguage, t, type Sentence } from '../src/i18n/index.ts'
import { en } from '../src/i18n/en.ts'
import { zh } from '../src/i18n/zh.ts'

/**
 * 存进状态里的句子，切语言之后必须跟着变。
 *
 * 这一条修的是一个**机制性**缺陷：句子一旦被渲染成字符串再存起来（通知条的 `error`、聊天层的
 * `chatNotice`、模型目录的 `reason`、设置窗口的通知……），它就成了一份**数据** —— 之后切语言
 * 不会再重译，英文界面里会留下一句中文。修法是存 `Message`（键 + 参数），显示的地方在**渲染期**
 * 用 `formatMessage()` / `formatSentence()` 现取。
 *
 * 所以这里的断言分两层：
 * 1. 存进去的那份值**原样不动**（引用/形状不变），语言换了它照样说的是新语言；
 * 2. 渲染出来之后**没有汉字**（原生自由文本除外，见文件末尾那条）。
 */
const HAN = /[\u4e00-\u9fff]/

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

// App.tsx 的浏览器预览分支在 import 时求值，而这些是纯函数测试 —— 给一个最小的 window 就够
// （与 `appChatLifecycle.spec.ts`、`dshAutostart.spec.ts` 同一条规矩）。
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

// 语言是模块级状态，测试之间必须还原，否则先跑的那条会决定后跑的那条看到什么。
afterEach(() => setLanguage('zh'))

async function appModule() {
  return import('../src/App.tsx')
}

describe('a sentence held in state follows the language', () => {
  it('re-translates the notice bar of App.tsx, both the error and the chat notice', async () => {
    const { harnessAvailabilityPatch, harnessDisconnectedNotice, visibleNotice } = await appModule()

    const patch = harnessAvailabilityPatch('harness', 'offline')
    expect(patch?.error).toEqual(harnessDisconnectedNotice('offline'))

    // `error`：桥掉线那句。存的是**词条**，而显示的是渲染后的样子。
    const disconnected = { chatNotice: undefined, error: patch?.error }
    expect(getLanguage()).toBe('zh')
    const chineseDisconnected = formatSentence(visibleNotice(disconnected))
    expect(chineseDisconnected).toMatch(HAN)
    // 中文逐字不变：三个词条按原来的顺序接起来，一个字都不差。
    expect(chineseDisconnected).toBe(
      `${t('app.bubble.harness.disconnected-prefix')}${t('harness.detail.offline')}${t('app.bubble.harness.session-preserved')}`,
    )

    setLanguage('en')
    // 同一份状态值（没有重新 patch），换的是语言。
    const englishDisconnected = formatSentence(visibleNotice(disconnected))
    expect(englishDisconnected).not.toMatch(HAN)
    expect(englishDisconnected).toBe(
      `${en['app.bubble.harness.disconnected-prefix']}${en['harness.detail.offline']}${en['app.bubble.harness.session-preserved']}`,
    )

    // `chatNotice`：聊天层那句（换会话的通知），优先级在 `error` 之前。
    const { archivedSessionNotice, blockedTurnNotice } = await import('../src/chat/nativeAdapter.ts')
    const reset: ChatEvent = { type: 'conversation-reset', reason: 'session-archived', message: archivedSessionNotice() }
    const failure: ChatEvent = { type: 'error', code: 'NATIVE_SEND_FAILED', recoverable: true, message: blockedTurnNotice() }
    const chatState = { chatNotice: reset.message, error: failure.message }
    setLanguage('zh')
    expect(formatSentence(visibleNotice(chatState))).toBe(zh['chat.native.archived-notice'])
    setLanguage('en')
    expect(formatSentence(visibleNotice(chatState))).toBe(en['chat.native.archived-notice'])
    expect(formatSentence(visibleNotice(chatState))).not.toMatch(HAN)
    expect(formatSentence(visibleNotice({ chatNotice: undefined, error: failure.message })))
      .toBe(en['chat.native.blocked-notice'])
  })

  it('clears that notice through a flag, never through the text of the sentence', async () => {
    const { harnessAvailabilityPatch, harnessSelectionUnavailableNotice } = await appModule()
    const { reduceRuntime, INITIAL_RUNTIME_STATE } = await import('../src/scenes/stateMachine.ts')

    // 「不能切 Harness」那句与掉线那句共用同一个框，中文逐字不变（前缀 + 状态说明 + 后半句）。
    expect(formatMessage(harnessSelectionUnavailableNotice('web-only'))).toBe(
      `${t('app.bubble.harness.disconnected-prefix')}${t('harness.detail.web-only')}${t('app.bubble.harness.selection-unavailable')}`,
    )

    const patch = harnessAvailabilityPatch('harness', 'offline')
    // 判据是状态里的标志位，与语言无关。
    expect(patch?.errorKind).toBe('bridge-unavailable')

    setLanguage('en')
    const english = formatSentence(patch?.error)!
    // 这正是旧做法坏掉的地方：英文界面里那句话**不再**以中文前缀开头，于是
    // `startsWith(前缀)` 认不出"这句是我们写的"，掉线提示永远清不掉。
    expect(english).not.toContain(zh['app.bubble.harness.disconnected-prefix'])
    expect(english).not.toMatch(HAN)
    // 换到英文之后，同一套标志位照样清得掉。
    expect(harnessAvailabilityPatch('harness', 'bridge-ready', patch?.errorKind))
      .toEqual({ activity: 'idle', error: undefined, errorKind: undefined })
    expect(harnessAvailabilityPatch('harness', 'offline', patch?.errorKind)).toBeUndefined()

    // 换了那句话就作废那个标志位：原生自由文本换上来的那一刻，旧标志不能留着。
    const replaced = reduceRuntime(
      { ...INITIAL_RUNTIME_STATE, error: patch?.error, errorKind: patch?.errorKind },
      { type: 'PATCH', patch: { error: 'RAW NATIVE FAILURE' } },
    )
    expect(replaced.errorKind).toBeUndefined()
    // 原生（Rust）的自由文本原样留着 —— 本批不翻译它，也不假装它已经国际化了。
    expect(replaced.error).toBe('RAW NATIVE FAILURE')
  })

  it('re-translates the model directory reason the model picker is disabled with', async () => {
    const { bridgeModelDirectory, modelUnavailableReason, unavailableDirectory } = await import('../src/connect/modelDirectory.ts')

    const fromBridge = modelUnavailableReason(bridgeModelDirectory(undefined))!
    const fromRead = modelUnavailableReason(
      unavailableDirectory(msg('app.bubble.model.harness-read-failed', { error: 'E' })),
    )!
    expect(formatMessage(fromBridge)).toBe(zh['connect.model.bridge-empty'])
    expect(formatMessage(fromBridge)).toMatch(HAN)

    setLanguage('en')
    expect(formatMessage(fromBridge)).toBe(en['connect.model.bridge-empty'])
    expect(formatMessage(fromBridge)).not.toMatch(HAN)
    // 参数（原生异常原文）留着，句子本身是英文。
    expect(formatMessage(fromRead)).toBe(en['app.bubble.model.harness-read-failed'].replace('{error}', 'E'))
  })

  it('re-translates the settings-window failure, including the nested sentence', async () => {
    const { settingsProbeErrorMessage } = await import('../src/settings/settingsProbes.ts')

    const failure = settingsProbeErrorMessage('autostartStatus', new Error('probe failed'))
    expect(formatMessage(failure)).toBe('读取开机自启状态失败：Error: probe failed')

    setLanguage('en')
    // `{message}` 这个参数自己也是一条词条：它跟着一起重译，而不是留在存进去那一刻的语言上。
    expect(formatMessage(failure)).toBe(
      en['settings.probe.error'].replace('{message}', en['settings.probe.autostart-status']).replace('{error}', 'Error: probe failed'),
    )
    expect(formatMessage(failure)).not.toMatch(HAN)

    // 异常原文是**原生**自由文本：它原样进句子（中文也一样），而句子本身仍是英文。
    const nativeBody = settingsProbeErrorMessage('autostartStatus', new Error('系统探测失败'))
    expect(formatMessage(nativeBody)).toContain('Error: 系统探测失败')
    expect(formatMessage(nativeBody)).toContain(en['settings.probe.autostart-status'])
  })

  it('re-translates every sentence our own code hands to a long-lived field', async () => {
    const { harnessDisconnectedNotice, harnessSelectionUnavailableNotice, harnessLaunchOutcome, dshAutostartNotice } = await appModule()
    const { bridgeModelDirectory, modelUnavailableReason } = await import('../src/connect/modelDirectory.ts')
    const { settingsProbeErrorMessage } = await import('../src/settings/settingsProbes.ts')
    const { archivedSessionNotice, blockedTurnNotice } = await import('../src/chat/nativeAdapter.ts')
    const { launchOutcomeNotice, subjectChoicePrompt } = await import('../src/connect/harnessSubjects.ts')
    const { raiseOutcomeNotice, unsupportedShellSubjectFallback } = await import('../src/connect/endpoints.ts')
    const { summarizeBridgeInstall } = await import('../src/connect/bridgeInstall.ts')
    const { backendModeMessage } = await import('../src/settings/SettingsPanel.tsx')

    // 每一条都是"会被存进状态、活到渲染那一刻"的句子：通知条、模型选择器、设置窗口的通知、
    // 聊天层的通知、执行主体那行说明。
    const held: Array<[string, () => Sentence]> = [
      ['RuntimeState.error（掉线）', () => harnessDisconnectedNotice('offline')],
      ['RuntimeState.error（不能切 Harness）', () => harnessSelectionUnavailableNotice('web-only')],
      ['RuntimeState.error（启动监督）', () => harnessLaunchOutcome({ availability: 'offline' }, 45_001, { managed: true, running: true })!.message],
      ['RuntimeState.error（自动启动失败码）', () => dshAutostartNotice({ outcome: 'launcher-missing', external: false })!],
      ['RuntimeState.chatNotice（归档）', () => archivedSessionNotice()],
      ['RuntimeState.chatNotice（宿主拒绝这一轮）', () => blockedTurnNotice()],
      ['ModelDirectory.reason', () => modelUnavailableReason(bridgeModelDirectory(undefined))!],
      ['SettingsNotice.text（探针失败）', () => settingsProbeErrorMessage('apiHistory', new Error('E'))],
      ['SettingsNotice.text（拉起结果）', () => launchOutcomeNotice({ outcome: 'started', kind: 'checkout' })!],
      ['SettingsNotice.text（选择执行主体）', () => subjectChoicePrompt([{ kind: 'checkout' }, { kind: 'checkout' }] as never)!],
      ['SettingsNotice.text（客户端界面）', () => raiseOutcomeNotice('no-window', 'official-desktop')!],
      ['SettingsNotice.text（Bridge 安装）', () => summarizeBridgeInstall([{ profile: 'web', status: 'installed', detail: '', command: 'x' }])!.text],
      ['SettingsNotice.text（存储的主体不再受支持）', () => unsupportedShellSubjectFallback('shell:unknown')!.notice],
      ['SettingsNotice.text（后端名字）', () => backendModeMessage('deepseek-api')],
    ]

    for (const [what, produce] of held) {
      setLanguage('zh')
      const chinese = formatSentence(produce())
      setLanguage('en')
      const english = formatSentence(produce())
      expect(english, `${what} 在英文下不该留中文`).not.toMatch(HAN)
      // 产生的是一份**值**还是渲染好的字符串，这里只能看结果：中文那一份不该是英文那一份
      // （同一条词条在中英两种语言里逐字相同的极少，逐个断言会把偶合当契约）。
      expect(chinese, `${what} 的中文没变`).toBeTruthy()
    }
  })
})

describe('nothing renders a sentence at write time any more', () => {
  it('renders the Lite notice, the clear-result detail and the settings notice at render time', async () => {
    // 这三个字段都是"存进状态、之后才显示"的：写入那一刻渲染就等于把它们钉死在那时的语言上。
    // 组件要挂起来才能端到端验证（它们都会去调原生），所以这里钉的是**机制**：存的是消息，
    // 渲染期才求值。
    const lite = await source('src/lite/LiteSettingsWindow.tsx')
    expect(lite).toContain('useState<Sentence>()')
    expect(lite).toContain('formatSentence(notice)')
    expect(lite, 'Lite 的通知不该在写入时渲染').not.toMatch(/setNotice\(t\(/)

    const panel = await source('src/settings/SettingsPanel.tsx')
    expect(panel).toContain('useState<Message[]>()')
    expect(panel).toContain('clearDetail.map(formatMessage).join(t(')
    expect(panel, '清空结果不该在写入时渲染').not.toMatch(/setClearDetail\(t\(/)

    const window = await source('src/settings/SettingsWindow.tsx')
    expect(window).toContain('formatSentence(notice.text)')
    expect(window, '设置窗口的通知不该在写入时渲染').not.toMatch(/show(Notice|Failure)\(t\(/)
  })

  it('keeps the native free text out of the mechanism, because this batch does not translate it', async () => {
    // 原生（Rust）返回的自由文本仍然是字符串：它没有词条可查，本批也不假装已经国际化
    // （`docs/plans/i18n-plan.md` 第一节第 6 条）。这里钉住"两种形状分得开"这件事。
    const { visibleNotice } = await appModule()
    const { reduceRuntime, INITIAL_RUNTIME_STATE } = await import('../src/scenes/stateMachine.ts')
    const native = 'Failed to read DSH bridge token from the credential store'
    const state = reduceRuntime(INITIAL_RUNTIME_STATE, { type: 'PATCH', patch: { error: native } })
    setLanguage('en')
    expect(formatSentence(visibleNotice(state))).toBe(native)
    expect(formatSentence(undefined)).toBeUndefined()
  })
})

describe('the dictionary entries the composed notices are stored as', () => {
  it('is a frame that orders the three parts, identical in both languages', () => {
    // 整句存的是这个框：三段各自是词条，所以切语言三段一起变（见 `i18n/zh.ts`）。
    expect(zh['app.bubble.harness.disconnected-frame']).toBe('{prefix}{detail}{preserved}')
    expect(zh['app.bubble.harness.selection-frame']).toBe('{prefix}{detail}{unavailable}')
    expect(en['app.bubble.harness.disconnected-frame']).toBe(zh['app.bubble.harness.disconnected-frame'])
    expect(en['app.bubble.harness.selection-frame']).toBe(zh['app.bubble.harness.selection-frame'])
  })
})

/**
 * 这里**没有**钉的东西，写下来免得被当成疏漏：`AutostartStatus.reason`、
 * `DeepSeekWebAdapterConfigStatus.warning`、以及 `sentenceOf()` 的兜底分支（`String(error)` ——
 * 原生（Rust）自由文本走的就是它）仍是中文（计划第一节第 6 条把它留给第二批）。上面那条
 * "keeps the native free text out of the mechanism" 钉的是"这两种形状分得开"这件事本身。
 * 我们**自己的**异常不走那条兜底：它们带的是词条，见 `tests/i18nErrors.spec.ts`。
 */
