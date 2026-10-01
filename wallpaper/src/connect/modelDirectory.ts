/**
 * 模型目录：把"可切换的模型"从三个来的地方归一。
 *
 * 三个后端的真实来源完全不同，而且**都可能拿不到**，所以这里不返回"一个数组"，
 * 而是返回"当前处于哪种情况"——把「宿主说没有」和「压根问不到」分开：
 *
 * - Harness：名单由**宿主**提供（宿主的 LLM 缝 `listModels`），经桥接转发。三种执行主体
 *   （检出 / 桌面 / 官壳）各自跑自己的桥接，所以同一套代码对三种情况都成立；
 *   宿主版本较老、没有这个能力时，桥接回答 `supported: false`。
 * - DeepSeek API：名单由**端点自己**给出（兼容接口的 `/models`）。网关没实现该路由时
 *   同样报 `supported: false`。
 * - 两者都没连上/读取失败：`unavailable`，此时不显示任何模型选项。
 *
 * 这条界限是有意的：**空列表不等于"不能问"**。把"问不到"当成"没有模型"，用户会以为
 * 自己的账号没有模型；把"没有模型"当成"问不到"，又会在可切换时白关掉选择器。
 */

import { msg, type Message } from '../i18n/index.ts'

export interface ModelOption {
  id: string
  name: string
}

export type ModelDirectory =
  /** 拿到了名单：可切换。`current` 是宿主/端点自己报的当前模型。 */
  | { kind: 'enumerated'; provider?: string; current?: string; models: ModelOption[] }
  /** 问到了，但对方不具备枚举能力：只能显示当前模型，不假装有得选。 */
  | { kind: 'current-only'; provider?: string; current?: string }
  /**
   * 还没连上 / 读取失败：连当前模型都不该装作知道。
   *
   * `reason` 是一句**没求值的**话（`Message`）：它会被存进运行状态、一路活到选择器渲染的那一刻，
   * 而语言可能在中间换过 —— 存句子就等于把它钉在写入那一刻的语言上。
   */
  | { kind: 'unavailable'; reason: Message }

/** 宿主/端点在"不支持枚举"时也报出的当前模型，用来兜住选择器的显示值。 */
export function currentModelOf(directory: ModelDirectory, fallback?: string): string | undefined {
  // 空串按"没有值"处理：`<select value="">` 又没有任何 option 匹配它时，浏览器会把这个
  // 控件显示成空白（实测可访问性树里读到 `ValuePattern.Value = ''`），用户看到的就是
  // "没有任何选项"——尽管选项其实在 DOM 里、控件也是 enabled。
  const usable = fallback?.trim() ? fallback : undefined
  if (directory.kind === 'unavailable') return usable
  return directory.current || usable
}

/**
 * 选择器里该显示哪些模型 id。
 *
 * 只有 `enumerated` 才扩展成多行；其余情况退化成"当前模型 + 用户曾经选过的那个"，
 * 这样已经选定的值不会因为一次读取失败而消失。
 */
export function modelIdsFor(directory: ModelDirectory, fallbackCurrent?: string, chosen?: string): string[] {
  const ids = directory.kind === 'enumerated' ? directory.models.map((model) => model.id) : []
  // 顺序有意是**当前模型在前**：`<select>` 在 value 没有匹配项时会回退到第一个 option，
  // 于是最坏情况也只是退化成"当前模型"，而不是一个空白框。
  return [...new Set([currentModelOf(directory, fallbackCurrent), chosen, ...ids].filter((id): id is string => Boolean(id?.trim())))]
}

/** 是否可以真的切换（决定选择器是否禁用）。 */
export function canSwitchModel(directory: ModelDirectory): boolean {
  return directory.kind === 'enumerated' && directory.models.length > 0
}

/** 禁用时给用户的解释；`undefined` 表示不禁用，无需解释。 */
export function modelUnavailableReason(directory: ModelDirectory): Message | undefined {
  if (canSwitchModel(directory)) return undefined
  if (directory.kind === 'unavailable') return directory.reason
  return msg('connect.model.no-options')
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' ? (value as Record<string, unknown>) : undefined
}

function readModels(value: unknown): ModelOption[] {
  if (!Array.isArray(value)) return []
  return value.flatMap((entry) => {
    const record = asRecord(entry)
    const id = typeof record?.id === 'string' ? record.id.trim() : ''
    if (!id) return []
    const name = typeof record?.name === 'string' && record.name.trim() ? record.name.trim() : id
    return [{ id, name }]
  })
}

/**
 * 桥接 `/control/models` 的回答。
 *
 * 形状：`{ supported, provider, current: { provider, model }, models: [{ id, name }] }`。
 * 形状不认识时按"问不到"处理，而不是硬塞一个空目录。
 */
export function bridgeModelDirectory(payload: unknown): ModelDirectory {
  const record = asRecord(payload)
  if (!record || typeof record.supported !== 'boolean') {
    return { kind: 'unavailable', reason: msg('connect.model.bridge-empty') }
  }
  const current = asRecord(record.current)
  const currentModel = typeof current?.model === 'string' && current.model.trim() ? current.model.trim() : undefined
  const provider = typeof record.provider === 'string' && record.provider.trim() ? record.provider.trim() : undefined
  if (!record.supported) return { kind: 'current-only', ...(provider ? { provider } : {}), ...(currentModel ? { current: currentModel } : {}) }
  const models = readModels(record.models)
  if (models.length === 0) return { kind: 'current-only', ...(provider ? { provider } : {}), ...(currentModel ? { current: currentModel } : {}) }
  return { kind: 'enumerated', ...(provider ? { provider } : {}), ...(currentModel ? { current: currentModel } : {}), models }
}

/** DeepSeek API `/models` 的回答（已经由原生侧归一成 `{ supported, models }`）。 */
export function apiModelDirectory(payload: unknown, configured?: string): ModelDirectory {
  const record = asRecord(payload)
  if (!record || typeof record.supported !== 'boolean') {
    return { kind: 'unavailable', reason: msg('connect.model.endpoint-empty') }
  }
  const models = readModels(record.models)
  const current = configured?.trim() || undefined
  if (!record.supported || models.length === 0) return { kind: 'current-only', ...(current ? { current } : {}) }
  return { kind: 'enumerated', ...(current ? { current } : {}), models }
}

/** 读取失败（异常）时的目录：把原因留给选择器解释（原因同样是**没求值的**一句话）。 */
export function unavailableDirectory(reason: Message): ModelDirectory {
  return { kind: 'unavailable', reason }
}

/**
 * 一个非空候选：空串当作"没有值"，**不是**当作一个值。
 *
 * `??` 只在 null/undefined 时回退，而宿主/桥接完全可能把模型报成空串；空串穿到
 * `<select value="">` 上没有任何 option 匹配它，浏览器就把这个框画成空白（实测可访问性树里读到
 * `ValuePattern.Value = ''`），用户看到的是"没有任何选项"，尽管选项其实在 DOM 里。
 */
function firstNonEmpty(...candidates: Array<string | undefined>): string | undefined {
  return candidates.find((candidate): candidate is string => Boolean(candidate?.trim()))
}

/**
 * 选择器当前该显示哪个模型 id，**按后端分别决定**。
 *
 * 抽成纯函数是因为这条规则连着两次被同一个 bug 咬到，而且两次都不是"少写了一层回退"，是
 * **后端之间串了值**：`runtime.model` 是三个后端共用的一份状态，于是 API 的 `deepseek-chat`
 * 会出现在网页/Harness 的选择器里，而 Harness 自己那份目录（宿主实测报的是 `deepseek-flash`）
 * 反而被盖掉。所以每个后端只读自己那份事实，**没有一层回退跨过后端边界**：
 *
 * - `harness`：壁纸里选过的 > 上次持久化的 > **宿主自己报的当前模型** > 目录里的第一个；
 * - `deepseek-api`：按端点选的 > 设置里配的 > 端点名单的第一个；
 * - `deepseek-web`：`undefined`——它的模型由 DeepSeek 页面决定，显示一个 id 会是编的。
 */
export function selectedModelFor(options: {
  backend: 'deepseek-web' | 'deepseek-api' | 'harness'
  /** 壁纸这次运行期间选的（最先）。 */
  chosen?: string
  /** 上次持久化的选择（Harness 用）。 */
  persisted?: string
  /** 配置里的模型（API 用）。 */
  configured?: string
  /** 该后端自己的目录。 */
  directory?: ModelDirectory
  /** 该后端可选项的 id，按顺序。 */
  ids?: readonly string[]
}): string | undefined {
  // 目录是 `unavailable` 时不摆出任何 id：那时连被问的对象都没有，"当前模型"只是残留。
  const fromDirectory = options.directory?.kind === 'unavailable' ? undefined : options.directory?.current
  if (options.backend === 'harness') {
    return firstNonEmpty(options.chosen, options.persisted, fromDirectory, options.ids?.[0])
  }
  if (options.backend === 'deepseek-api') {
    return firstNonEmpty(options.chosen, options.configured, fromDirectory, options.ids?.[0])
  }
  return undefined
}
