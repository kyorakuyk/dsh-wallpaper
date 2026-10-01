/**
 * 界面语言的运行时。
 *
 * 为什么是**模块级的当前语言 + 订阅**，而不是 React Context：
 * 需要出文案的不止组件 —— `native/runtime.ts`、`connect/*`、`features/*` 里的纯函数也会返回
 * 面向用户的句子。Context 只能到组件为止，模块级状态两者都能用；React 侧再用 `useLanguage()`
 * 订阅重渲染即可。
 *
 * 词条编译进产物（不做运行时加载语言包）：少一次 IO，也少一类"语言包没加载出来"的故障。
 */
import { useEffect, useSyncExternalStore } from 'react'

import { en } from './en'
import { zh, type Dict, type MessageKey } from './zh'

// 词条类型统一从这一处取，消费方不需要知道真源在哪个文件。
export type { Dict, MessageKey } from './zh'

export type Language = 'zh' | 'en'

/** 界面上能选的语言，顺序即下拉里的顺序。 */
export const LANGUAGES: readonly Language[] = ['zh', 'en']

const DICTS: Record<Language, Dict> = { zh, en }

let current: Language = 'zh'

const listeners = new Set<() => void>()

export function getLanguage(): Language {
  return current
}

/**
 * 切换语言并通知订阅者。
 *
 * 相同语言不发通知：设置文档在启动时会回填一次，若每次都通知，界面会白重渲染一轮。
 */
export function setLanguage(next: Language): void {
  if (next === current) return
  current = next
  for (const listener of listeners) listener()
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/**
 * 取词条。
 *
 * 缺失时回落到中文并在控制台留一行 —— 类型上不该发生（`Dict` 保证了键齐全），但真发生时
 * 宁可显示中文句子加一条警告，也不要显示键名或者空白，那会让人以为界面坏了。
 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const raw = DICTS[current][key] ?? zh[key]
  if (raw === undefined) {
    console.warn(`i18n: 缺少词条 ${key}`)
    return key
  }
  if (!params) return raw
  return raw.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match,
  )
}

/**
 * 把**设置文档里的语言**应用到运行时。
 *
 * 为什么必须有这条：设置窗口与壁纸窗口是两个独立 WebView，各自一份 JS 运行时。切换动作
 * （下拉）只发生在设置窗口，它调用 `setLanguage()` 只改到自己那一份；壁纸窗口要靠**设置文档
 * 的广播**才跟得上。而且重启后，存下来的选择也必须有人应用 —— 曾经没有任何地方这么做，
 * 于是"切了英文、桌面还是中文"，且重启后又变回中文（用户第一次看到的就是这个）。
 *
 * 所以运行时语言应当**由设置推导**，而不是由"用户点过下拉"这个动作决定。两个窗口都调用它。
 */
export function useStoredLanguage(language: Language): void {
  useEffect(() => {
    setLanguage(language)
  }, [language])
}

/** 组件里订阅语言变化；语言一变，用到 `t()` 的组件就会重渲染。 */
export function useLanguage(): Language {
  return useSyncExternalStore(subscribe, getLanguage, getLanguage)
}

/**
 * 一句**还没求值**的界面文案：键 + 参数。
 *
 * 为什么需要它：句子一旦被渲染成字符串再存起来（通知条上的那句话、模型目录的 `reason`、设置
 * 窗口的通知……），它就成了一份**数据** —— 之后切语言不会再重译，英文界面里会留下一句中文。
 * 存 `Message`、在**渲染那一刻**才 `formatMessage()`，切语言时整句跟着变。
 *
 * 与"由原生返回的自由文本"的区分见 `Sentence`。
 */
export interface Message {
  key: MessageKey
  params?: Record<string, MessageParam>
}

/**
 * 参数除了字面量，还可以是**另一条词条**（或 `undefined` = "这一段没有"）。
 *
 * 掉线提示就是三个词条拼成的一句（前缀 + 状态说明 + 后半句）。整句必须留在"未求值"的状态里，
 * 否则那三段里任何一段都会定在拼装那一刻的语言上 —— 那正是这一批要修掉的毛病。
 * `undefined` 渲染成空串，与拼接时代"这一段不存在"的结果逐字一致。
 */
export type MessageParam = string | number | Message | undefined

/** 造一条消息引用（不渲染）。 */
export const msg = (key: MessageKey, params?: Record<string, MessageParam>): Message => ({ key, params })

/** 渲染一条消息引用：参数里的消息按**当前语言**递归求值。 */
export function formatMessage(message: Message): string {
  if (!message.params) return t(message.key)
  const resolved: Record<string, string | number> = {}
  for (const [name, value] of Object.entries(message.params)) {
    if (value === undefined) resolved[name] = ''
    else if (typeof value === 'object') resolved[name] = formatMessage(value)
    else resolved[name] = value
  }
  return t(message.key, resolved)
}

/**
 * 状态里的一句话：**我们自己的**词条（`Message`，渲染期求值），或**原生返回的自由文本**
 * （`string`，本批不翻译 —— 见 `docs/plans/i18n-plan.md` 第一节第 6 条）。
 *
 * 两种形状分得很开，所以这里不需要额外的标志位：字符串就是"这不是我们写的句子"。
 * （`RuntimeState.errorKind` 回答的是另一个问题：`error` 里那句**我们自己的**提示属于哪一类。）
 * 把原生文本包成 `Message` 只会假装它已经国际化了。
 *
 * 异常通道用的是同一个类型：`SentenceError.sentence` 就是一句 `Sentence`（见下），所以"我们自己
 * 的异常"和"状态里的句子"遵守同一条规矩，不需要第二套表示。
 */
export type Sentence = Message | string

/** 渲染状态里的一句话；`undefined` 原样传出去，调用方不必为"没有话可说"多写一层判断。 */
export function formatSentence(sentence: Sentence | undefined): string | undefined {
  if (sentence === undefined) return undefined
  return typeof sentence === 'string' ? sentence : formatMessage(sentence)
}

/**
 * 我们自己抛出的异常：它带的是一句**还没求值**的话（`Sentence`）。
 *
 * 为什么不让异常直接带渲染好的字符串：`throw new Error(t('…'))` 把**抛出那一刻**的语言钉死在
 * 异常里，而异常活得比抛出点久 —— 它跨 await、进日志、被上层存进通知状态，用户切到英文之后
 * 那句话仍是抛出时那句中文。它和状态里的句子（`Message`）是同一种东西：存键和参数，显示时才
 * 求值。捕获处一律用 `sentenceOf()` 把它取回来，**不要**再 `String(error)`。
 *
 * `message` 是访问器而不是构造时渲染的字符串：任何仍然 `String(error)` / `error.message` 的
 * 地方（日志、还没换成 `sentenceOf` 的兜底路径）读到的是**读的那一刻**的语言，而不是抛出那一刻
 * 的。给代码看的那条路是 `instanceof SentenceError` + `.sentence`。
 */
export class SentenceError extends Error {
  /** 那句没求值的话（我们自己的异常里是 `Message`；字符串只用于刻意转发原生原文的场合）。 */
  readonly sentence: Sentence
  /** 与 `Error` 的 `cause` 同义；ES2020 的 lib 里还没有它，所以自己声明。 */
  readonly cause?: unknown

  constructor(sentence: Sentence, options?: { cause?: unknown }) {
    // 不把渲染结果交给 `super()`：`message` 由下面的访问器现取。
    super()
    this.name = 'SentenceError'
    this.sentence = sentence
    if (options && 'cause' in options) this.cause = options.cause
  }

  get message(): string {
    return typeof this.sentence === 'string' ? this.sentence : formatMessage(this.sentence)
  }
}

/**
 * 从任意 catch 到的东西里取出**可显示的句子**：我们的异常取回那句没求值的话（`Message`），
 * 其余原样 `String()`。
 *
 * 兜底那一支仍然可能拿到中文：原生的自由文本本批不翻译（见 `docs/plans/i18n-plan.md` 第一节
 * 第 6 条），而 `String(error)` 自带的 `Error: ` 前缀也保持原样 —— 中文界面逐字不变。区别在于
 * **我们自己的**异常不再经过那条兜底：它取回来的是词条，切语言时跟着变。
 */
export function sentenceOf(error: unknown): Sentence {
  return error instanceof SentenceError ? error.sentence : String(error)
}
