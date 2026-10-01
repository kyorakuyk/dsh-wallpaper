import { readdir, readFile } from 'node:fs/promises'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

import { DeepSeekWebAdapter } from '../src/chat/deepseekWebAdapter.ts'
import { classifyNativeSessionFailure } from '../src/chat/nativeAdapter.ts'
import {
  SentenceError,
  formatMessage,
  formatSentence,
  getLanguage,
  msg,
  sentenceOf,
  setLanguage,
  t,
} from '../src/i18n/index.ts'
import { en } from '../src/i18n/en.ts'
import { zh } from '../src/i18n/zh.ts'
import { nativeRuntime } from '../src/native/runtime.ts'
import { settingsProbeErrorMessage } from '../src/settings/settingsProbes.ts'

/**
 * 异常通道上的那句话，必须和状态里的句子守同一条规矩：**存的是词条，显示时才求值**。
 *
 * `throw new Error(t('…'))` 把句子在抛出那一刻渲染好，异常带着那句中文一路走 —— 用户切到英文，
 * 它还是中文（异常活得比抛出点久：跨 await、进日志、被上层存进状态）。这里的断言分三层：
 * 1. 同一个异常对象，切语言之后读出来的是新语言；
 * 2. 判据（"这是不是归档会话的错误"）按**标志**而不是按文字 —— 文字匹配在英文界面里迟早失效；
 * 3. 原生（Rust）的自由文本原样穿过兜底路径（`docs/plans/i18n-plan.md` 第一节第 6 条）。
 */
const HAN = /[\u4e00-\u9fff]/

// 语言是模块级状态，测试之间必须还原（与 tests/i18n.spec.ts 同一条规矩）。
afterEach(() => setLanguage('zh'))

/** 跑一次会抛出的动作，把 catch 到的东西原样交出来给断言。 */
async function rejectionOf(run: () => unknown): Promise<unknown> {
  try {
    await run()
  } catch (error) {
    return error
  }
  throw new Error('expected the action to throw')
}

describe('an exception of ours carries a key, not a translation', () => {
  it('says English after the language changes, where the old shape stayed Chinese', async () => {
    // 改动前的写法：句子在**抛出那一刻**渲染成字符串，异常带着那句中文走完全程。
    const legacy = await rejectionOf(() => { throw new Error(t('chat.web.busy')) })
    // 现在的写法：异常带的是词条（键 + 参数），渲染留给显示时。
    const ours = await rejectionOf(() => { throw new SentenceError(msg('chat.web.busy')) })

    expect(getLanguage()).toBe('zh')
    expect(String(legacy)).toBe(`Error: ${zh['chat.web.busy']}`)
    expect(sentenceOf(ours)).toEqual(msg('chat.web.busy'))
    expect(formatSentence(sentenceOf(ours))).toBe(zh['chat.web.busy'])

    setLanguage('en')
    // 旧写法坏掉的样子：同一份异常，切了语言仍然说中文 —— 这正是这一批要修的缺陷。
    expect(String(legacy)).toBe(`Error: ${zh['chat.web.busy']}`)
    expect(String(legacy)).toMatch(HAN)
    // 新写法：同一个异常对象（没有重新构造），读出来的是**读的那一刻**的语言。
    expect(formatSentence(sentenceOf(ours))).toBe(en['chat.web.busy'])
    expect(formatSentence(sentenceOf(ours))).not.toMatch(HAN)
    // 兜底路径（仍在 `String(error)` / `error.message` 的地方）读到的也是当前语言。
    expect(String(ours)).not.toMatch(HAN)
  })

  it('keeps the real throw sites translatable, not just a synthetic one', async () => {
    // 两处**生产代码**里的抛出点：运行时的桌面端限定命令，与网页后端的发送前置检查。
    const fromRuntime = await rejectionOf(() => nativeRuntime.openDeepSeekWebAdapterConfig())
    const adapter = new DeepSeekWebAdapter()
    const fromAdapter = await rejectionOf(() => adapter.send('你好'))

    expect(sentenceOf(fromRuntime)).toEqual(msg('runtime.web-adapter.preview-open'))
    expect(sentenceOf(fromAdapter)).toEqual(msg('chat.web.not-connected'))

    expect(formatSentence(sentenceOf(fromRuntime))).toBe(zh['runtime.web-adapter.preview-open'])
    expect(formatSentence(sentenceOf(fromAdapter))).toBe(zh['chat.web.not-connected'])

    setLanguage('en')
    expect(formatSentence(sentenceOf(fromRuntime))).toBe(en['runtime.web-adapter.preview-open'])
    expect(formatSentence(sentenceOf(fromAdapter))).toBe(en['chat.web.not-connected'])
    expect(formatSentence(sentenceOf(fromRuntime))).not.toMatch(HAN)
    expect(formatSentence(sentenceOf(fromAdapter))).not.toMatch(HAN)
  })

  it('nests the sentence where a catch site used to stringify the exception', () => {
    // `settingsProbes.settingsProbeErrorMessage()` 是捕获处：它把异常放进一条通知里。
    // 以前用 `String(error)`，异常自己那句就被钉在抛出的那一刻；现在取回来的是那句话本身。
    const failure = settingsProbeErrorMessage('autostartStatus', new SentenceError(msg('runtime.tui.desktop-only')))
    expect(formatMessage(failure)).toBe(
      zh['settings.probe.error']
        .replace('{message}', zh['settings.probe.autostart-status'])
        .replace('{error}', zh['runtime.tui.desktop-only']),
    )
    setLanguage('en')
    expect(formatMessage(failure)).toBe(
      en['settings.probe.error']
        .replace('{message}', en['settings.probe.autostart-status'])
        .replace('{error}', en['runtime.tui.desktop-only']),
    )
    expect(formatMessage(failure)).not.toMatch(HAN)
  })

  it('leaves the native free text exactly as String(error) had it', () => {
    // 原生（Rust）的自由文本本批不翻译：兜底路径逐字不变 —— 连 `String(error)` 自带的
    // `Error: ` 前缀都一样（`docs/plans/i18n-plan.md` 第一节第 6 条）。
    expect(sentenceOf(new Error('系统探测失败'))).toBe('Error: 系统探测失败')
    expect(sentenceOf('RAW NATIVE FAILURE')).toBe('RAW NATIVE FAILURE')
    expect(sentenceOf(undefined)).toBe('undefined')
    // 而"我们自己的"异常不会走这条兜底。
    expect(typeof sentenceOf(new SentenceError(msg('chat.web.busy')))).toBe('object')
  })
})

describe('the archived/missing-session judgement is a flag, not a text match', () => {
  // 原生侧抛出来的就是这几条**自由文本**：机器标记在前，中文说明在后（见 `chat.rs`）。
  const ARCHIVED_NATIVE = 'HARNESS_SESSION_ARCHIVED: 这条会话已在桌面端归档，桥不再接受它的消息。'
  const MISSING_NATIVE = 'HARNESS_NO_SESSION: Harness 会话尚未建立'
  /** 别的 409（"另一台 DSH 占用这条会话"）绝不能冒充归档。 */
  const OTHER_409 = '发送失败：409 另一台 DSH 正在使用这条会话'

  it('classifies the same way in both languages, and never by the copy around it', () => {
    const kinds: Array<string | undefined> = []
    for (const language of ['zh', 'en'] as const) {
      setLanguage(language)
      expect(classifyNativeSessionFailure(ARCHIVED_NATIVE)?.kind).toBe('session-archived')
      expect(classifyNativeSessionFailure(new Error(ARCHIVED_NATIVE))?.kind).toBe('session-archived')
      expect(classifyNativeSessionFailure(MISSING_NATIVE)?.kind).toBe('no-session')
      expect(classifyNativeSessionFailure(new Error(MISSING_NATIVE))?.kind).toBe('no-session')
      expect(classifyNativeSessionFailure(OTHER_409)).toBeUndefined()
      expect(classifyNativeSessionFailure(undefined)).toBeUndefined()
      // 判据不看任何语言的**文案**：把我们自己那句提示（当前语言的版本）递进去，
      // 什么也不该认出来。旧写法（拿 `t()` 出来的句子做 `includes`）正是在这里坏掉的：
      // 换了语言，那句话不再等于判据里写死的那串文字。
      expect(classifyNativeSessionFailure(t('chat.native.archived-notice'))).toBeUndefined()
      kinds.push(classifyNativeSessionFailure(ARCHIVED_NATIVE)?.kind)
    }
    // 语言换过一次，判据的答案一模一样。
    expect(kinds[1]).toBe(kinds[0])
  })

  it('hands the native free text through as the detail, unchanged', () => {
    const failure = classifyNativeSessionFailure(ARCHIVED_NATIVE)
    expect(failure?.detail).toBe(ARCHIVED_NATIVE)
    // 它在英文界面里仍是原生原文（本批不翻译），但"哪一类失败"这个判断与语言无关。
    setLanguage('en')
    expect(formatSentence(failure?.detail)).toBe(ARCHIVED_NATIVE)
    expect(failure?.kind).toBe('session-archived')
  })
})

/**
 * 棘轮：这一批**删掉**的两种写法不许再回来。
 *
 * 与 `noHardcodedCopy.spec.ts` 同一条思路 —— 机制光靠"记得这么写"守不住，得有一条会红的测试。
 * 注释要剥掉：这个仓库的注释是中文的，而且它们**会**引用这两种写法来说明为什么不用它们。
 */
describe('the shapes this batch removed', () => {
  const SRC = fileURLToPath(new URL('../src', import.meta.url))

  function withoutComments(source: string): string {
    return source
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .split(/\r?\n/)
      .map((line) => {
        const at = line.search(/(^|\s)\/\//)
        return at >= 0 ? line.slice(0, at) : line
      })
      .join('\n')
  }

  async function uiSources(dir: string): Promise<string[]> {
    const found: string[] = []
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name)
      if (entry.isDirectory()) found.push(...(await uiSources(full)))
      else if (/\.(ts|tsx)$/.test(entry.name)) found.push(full)
    }
    return found
  }

  it('never renders a sentence at throw time, and never judges by exception text', async () => {
    const shapes: Array<[string, RegExp]> = [
      ['在抛出那一刻把句子渲染进异常', /throw new Error\(t\(/],
      ['拿异常消息去比文字', /String\((?:error|err|e)\)\.(?:includes|startsWith|indexOf)\(/],
      ['拿异常消息去比文字', /(?:error|err)\.message\s*(?:===|!==|\.includes\(|\.startsWith\()/],
    ]
    const offenders: string[] = []
    for (const file of await uiSources(SRC)) {
      const source = withoutComments(await readFile(file, 'utf8'))
      for (const [what, pattern] of shapes) {
        if (pattern.test(source)) offenders.push(`${file.slice(SRC.length + 1)}：${what}`)
      }
    }
    expect(offenders, '异常要么带词条（SentenceError），要么按类型/标志判断').toEqual([])
  })
})
