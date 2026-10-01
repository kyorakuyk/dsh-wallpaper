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
import { useSyncExternalStore } from 'react'

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

/** 组件里订阅语言变化；语言一变，用到 `t()` 的组件就会重渲染。 */
export function useLanguage(): Language {
  return useSyncExternalStore(subscribe, getLanguage, getLanguage)
}
