import { describe, expect, it } from 'vitest'
import {
  apiModelDirectory,
  bridgeModelDirectory,
  canSwitchModel,
  currentModelOf,
  modelIdsFor,
  modelUnavailableReason,
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
    expect(modelIdsFor(directory, undefined, 'deepseek-v4-pro')).toEqual(['deepseek-v4-pro', 'deepseek-flash'])
    expect(modelUnavailableReason(directory)).toBeUndefined()
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
