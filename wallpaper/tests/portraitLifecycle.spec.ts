import { beforeAll, describe, expect, it } from 'vitest'
import { BUILTIN_MODEL_RULES, resolveModelTier } from '../src/domain/modelTier.ts'

// App.tsx evaluates its browser-preview fallback when imported, so it needs the same
// minimal `window` the other pure-logic suites provide.
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

const appModule = () => import('../src/App.tsx')

/**
 * 立绘形态与模型选择共用同一个生命周期。
 *
 * 实测 bug：Harness 里切到 pro → 立绘成年；切到网页再切回来 → 立绘变幼年，而下拉列表里仍然
 * 是 pro。两个原因都在这里：形态读了会被别的后端覆盖的 `runtime.model`，以及原生 `model`
 * 事件带的 `unknown` 把"上一次的层级"这个兜底归零。
 */
describe('the portrait follows the model the picker shows', () => {
  it('keeps the chosen model driving the portrait, whatever the host reports', async () => {
    const { portraitTierModel } = await appModule()
    // Harness 重连时宿主报的是它自己的当前模型（实测是 deepseek-flash），而这次会话用的是
    // 壁纸选定的那个 —— 形态必须跟后者走。
    const driven = portraitTierModel('harness', 'deepseek-flash', 'deepseek-v4-pro')
    expect(driven).toBe('deepseek-v4-pro')
    expect(resolveModelTier('harness', 'deepseek-official', driven, [], 'flash')).toBe('pro')
    // 同一个函数也保证了"重启后先按持久化的值立绘"：`selectedModel` 里就带着持久化的选择，
    // 不必等宿主回报。
    expect(resolveModelTier('harness', undefined, portraitTierModel('harness', undefined, 'deepseek-v4-pro'), [], 'flash')).toBe('pro')

    // 网页入口是唯一例外：它的模型由 DeepSeek 页面决定，壁纸没有选择可读。
    expect(portraitTierModel('deepseek-web', 'deepseek-chat', 'deepseek-v4-pro')).toBe('deepseek-chat')
    expect(resolveModelTier('deepseek-web', undefined, 'deepseek-chat', [], 'flash')).toBe('flash')
    // API 侧同理读选择：端点的模型就是壁纸配的那个。
    expect(portraitTierModel('deepseek-api', undefined, 'deepseek-reasoner')).toBe('deepseek-reasoner')
    expect(resolveModelTier('deepseek-api', undefined, 'deepseek-reasoner', [], 'flash')).toBe('pro')
  })

  it('never lets an unknown tier wipe the tier the portrait is using', async () => {
    const { tierPatchFromEvent } = await appModule()
    // 原生侧固定发 unknown：照抄会把 `runtime.modelTier` 写成 unknown，而那正是
    // `resolveModelTier` 的兜底依据（模型名认不出来时沿用上一次的层级）。
    expect(tierPatchFromEvent('unknown')).toEqual({})
    expect(tierPatchFromEvent(undefined)).toEqual({})
    expect(tierPatchFromEvent('pro')).toEqual({ modelTier: 'pro' })
    expect(tierPatchFromEvent('flash')).toEqual({ modelTier: 'flash' })
  })

  it('recognises the models this machine actually gets from the host', () => {
    // 规则本身没变，但它是上面两条的落点：宿主目录实测是 deepseek-flash / deepseek-v4-pro。
    const tierOf = (model: string) => resolveModelTier('harness', 'deepseek-official', model, [], 'flash')
    expect(tierOf('deepseek-flash')).toBe('flash')
    expect(tierOf('deepseek-v4-pro')).toBe('pro')
    // 用户规则（设置里那份）优先，且 exact 优先于 contains。
    const rules = [
      { backend: '*' as const, pattern: 'deepseek-v4-pro', match: 'contains' as const, tier: 'flash' as const },
      { backend: 'harness' as const, pattern: 'deepseek-v4-pro', match: 'exact' as const, tier: 'pro' as const },
    ]
    expect(resolveModelTier('harness', undefined, 'deepseek-v4-pro', rules, 'flash')).toBe('pro')
    expect(BUILTIN_MODEL_RULES.some((rule) => rule.pattern === 'pro')).toBe(true)
  })
})
