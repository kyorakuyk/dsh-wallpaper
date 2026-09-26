import { describe, expect, it } from 'vitest'
import {
  apiModelDirectory,
  bridgeModelDirectory,
  canSwitchModel,
  currentModelOf,
  modelIdsFor,
  modelUnavailableReason,
  selectedModelFor,
  unavailableDirectory,
} from '../src/connect/modelDirectory.ts'

/**
 * 三种情况必须分得清：
 * 1. 对方能枚举 → 可以切换；
 * 2. 对方不能枚举（老版本宿主 / 端点没有 /models）→ 只显示当前模型；
 * 3. 压根没问到（未连接 / 读取失败）→ 连"当前模型"都不该假装知道。
 *
 * 这些测试钉的就是这三条的边界，尤其是"空列表 ≠ 问不到"。
 */

describe('harness model directory (bridge payload)', () => {
  it('enumerates the host catalog and keeps its current selection', () => {
    const directory = bridgeModelDirectory({
      supported: true,
      provider: 'deepseek-official',
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [
        { id: 'deepseek-flash', name: 'DeepSeek-Flash' },
        { id: 'deepseek-v4-pro', name: 'DeepSeek-V4-Pro' },
      ],
    })
    expect(directory).toEqual({
      kind: 'enumerated',
      provider: 'deepseek-official',
      current: 'deepseek-flash',
      models: [
        { id: 'deepseek-flash', name: 'DeepSeek-Flash' },
        { id: 'deepseek-v4-pro', name: 'DeepSeek-V4-Pro' },
      ],
    })
    expect(canSwitchModel(directory)).toBe(true)
    // 当前模型排在最前：`<select>` 在没有匹配值时回退到第一个 option。
    expect(modelIdsFor(directory, undefined, 'deepseek-v4-pro')).toEqual(['deepseek-flash', 'deepseek-v4-pro'])
    expect(modelUnavailableReason(directory)).toBeUndefined()
  })

  it('treats an empty string as "no value" so the box cannot render blank', () => {
    // 实测：宿主把模型报成空串时 `??` 会放它过去，`<select value="">` 没有匹配项，
    // 浏览器就把控件显示成空白（可访问性树里读到 `ValuePattern.Value = ''`），
    // 用户看到的就是"没有任何选项"——而选项其实在 DOM 里、控件也是 enabled。
    const directory = bridgeModelDirectory({
      supported: true,
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [{ id: 'deepseek-flash', name: 'DeepSeek-Flash' }],
    })
    expect(modelIdsFor(directory, '', '')).toEqual(['deepseek-flash'])
    expect(currentModelOf(directory, '')).toBe('deepseek-flash')
    expect(currentModelOf(unavailableDirectory('读取失败'), '')).toBeUndefined()
  })

  it('treats an unsupported host as "current model only", never as "no models"', () => {
    const directory = bridgeModelDirectory({
      supported: false,
      provider: 'deepseek-official',
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [],
    })
    expect(directory).toEqual({ kind: 'current-only', provider: 'deepseek-official', current: 'deepseek-flash' })
    expect(canSwitchModel(directory)).toBe(false)
    // 当前模型仍然显示出来，且不提供第二个选项。
    expect(modelIdsFor(directory, undefined)).toEqual(['deepseek-flash'])
    expect(modelUnavailableReason(directory)).toBe('当前 Harness 未提供可选模型')
  })

  it('never offers a switch it cannot honour when the catalog is empty', () => {
    // supported=true 但列表为空：这是"宿主能枚举、但没有可用模型"，
    // 依然不该给用户一个可以点却没用的下拉。
    const directory = bridgeModelDirectory({
      supported: true,
      provider: 'deepseek-official',
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [],
    })
    expect(directory.kind).toBe('current-only')
    expect(canSwitchModel(directory)).toBe(false)
  })

  it('reports an unrecognised payload as unavailable instead of inventing a list', () => {
    expect(bridgeModelDirectory(undefined)).toEqual({ kind: 'unavailable', reason: '桥接未返回模型目录' })
    expect(bridgeModelDirectory({ models: [{ id: 'x' }] })).toEqual({ kind: 'unavailable', reason: '桥接未返回模型目录' })
  })

  it('falls back to the id when the host sends no display name', () => {
    const directory = bridgeModelDirectory({
      supported: true,
      current: { provider: 'p', model: 'm' },
      models: [{ id: 'm' }, { id: '' }, { name: 'no id' }],
    })
    expect(directory.kind).toBe('enumerated')
    expect(directory.kind === 'enumerated' ? directory.models : []).toEqual([{ id: 'm', name: 'm' }])
  })
})

describe('api model directory (endpoint payload)', () => {
  it('lists what the endpoint reported and marks the configured model as current', () => {
    const directory = apiModelDirectory({ supported: true, models: [{ id: 'deepseek-chat', name: 'deepseek-chat' }] }, 'deepseek-reasoner')
    expect(directory).toEqual({
      kind: 'enumerated',
      current: 'deepseek-reasoner',
      models: [{ id: 'deepseek-chat', name: 'deepseek-chat' }],
    })
  })

  it('degrades to the configured model when the gateway has no /models route', () => {
    const directory = apiModelDirectory({ supported: false, models: [] }, 'deepseek-chat')
    expect(directory).toEqual({ kind: 'current-only', current: 'deepseek-chat' })
    expect(modelIdsFor(directory, undefined)).toEqual(['deepseek-chat'])
  })

  it('keeps a previously chosen model visible after a failed read', () => {
    const directory = unavailableDirectory('端点模型列表读取失败：网络不可达')
    // 读取失败时不动用户已经选定的值，只是禁用切换并说明原因。
    expect(modelIdsFor(directory, undefined, 'deepseek-reasoner')).toEqual(['deepseek-reasoner'])
    expect(currentModelOf(directory, 'deepseek-chat')).toBe('deepseek-chat')
    expect(modelUnavailableReason(directory)).toBe('端点模型列表读取失败：网络不可达')
  })
})

/**
 * 每个后端只读自己那份事实。
 *
 * 这条规则连着两次被同一个 bug 咬到：`runtime.model` 是三个后端共用的一份状态，于是 API 的
 * `deepseek-chat` 出现在别的后端的选择器里，而 Harness 自己那份目录（宿主实测报
 * `deepseek-flash`）反而被盖掉。这里钉的就是"没有一层回退跨过后端边界"。
 */
describe('the model a backend shows', () => {
  const hostCatalog = bridgeModelDirectory({
    supported: true,
    current: { provider: 'deepseek-official', model: 'deepseek-flash' },
    models: [
      { id: 'deepseek-flash', name: 'DeepSeek-Flash' },
      { id: 'deepseek-v4-pro', name: 'DeepSeek-V4-Pro' },
    ],
  })

  it('falls back to the host\u2019s own current model when nothing was ever chosen', () => {
    // 从没选过时显示**宿主自己的当前模型**，不是共用的 runtime.model（那可能带着 API 的值）。
    expect(selectedModelFor({ backend: 'harness', directory: hostCatalog, ids: ['deepseek-flash', 'deepseek-v4-pro'] }))
      .toBe('deepseek-flash')
  })

  it('never lets one backend\u2019s model id reach another one', () => {
    // API 的配置值只属于 API：Harness 侧读的是它自己的目录与选择，`configured` 在这条路上
    // 根本不是一个输入，所以它既不能回显，也盖不掉宿主报的当前模型。
    expect(selectedModelFor({
      backend: 'harness',
      configured: 'deepseek-chat',
      directory: hostCatalog,
      ids: ['deepseek-flash'],
    })).toBe('deepseek-flash')
    // 但没有任何 Harness 事实时，也绝不会把 API 的配置值当成 Harness 的模型。
    expect(selectedModelFor({ backend: 'harness', directory: unavailableDirectory('Harness 未运行'), ids: [] }))
      .toBeUndefined()
    // 网页入口的模型由 DeepSeek 页面决定：给一个 id 就是编的。
    expect(selectedModelFor({ backend: 'deepseek-web', configured: 'deepseek-chat', ids: ['deepseek-chat'] }))
      .toBeUndefined()
  })

  it('prefers what the user chose, then what was remembered, then the host', () => {
    expect(selectedModelFor({
      backend: 'harness',
      chosen: 'deepseek-v4-pro',
      persisted: 'deepseek-flash',
      directory: hostCatalog,
      ids: ['deepseek-flash', 'deepseek-v4-pro'],
    })).toBe('deepseek-v4-pro')
    expect(selectedModelFor({
      backend: 'harness',
      persisted: 'deepseek-v4-pro',
      directory: hostCatalog,
      ids: ['deepseek-flash', 'deepseek-v4-pro'],
    })).toBe('deepseek-v4-pro')
  })

  it('treats an empty answer as no answer, for every layer', () => {
    // 空串穿到 `<select value="">` 上时没有任何 option 匹配它，控件会画成**空白**（实测
    // 可访问性树里读到 `ValuePattern.Value = ''`），所以每一层都必须当它没有值。
    expect(selectedModelFor({
      backend: 'harness',
      chosen: '   ',
      persisted: '',
      directory: { kind: 'enumerated', current: '  ', models: [] },
      ids: ['deepseek-flash'],
    })).toBe('deepseek-flash')
    expect(selectedModelFor({ backend: 'deepseek-api', configured: '', ids: ['deepseek-chat'] }))
      .toBe('deepseek-chat')
  })

  it('reads the endpoint\u2019s own answer for the API backend', () => {
    const endpoint = apiModelDirectory({ supported: true, models: [{ id: 'deepseek-chat', name: 'deepseek-chat' }] }, 'deepseek-reasoner')
    expect(selectedModelFor({
      backend: 'deepseek-api',
      configured: 'deepseek-reasoner',
      directory: endpoint,
      ids: ['deepseek-reasoner', 'deepseek-chat'],
    })).toBe('deepseek-reasoner')
    // 读取失败时连端点的"当前值"都不摆出来——那时它只是残留。
    expect(selectedModelFor({
      backend: 'deepseek-api',
      directory: unavailableDirectory('端点模型列表读取失败'),
      ids: [],
    })).toBeUndefined()
  })
})
