import { describe, expect, it } from 'vitest'
import { INITIAL_RUNTIME_STATE, reduceRuntime } from '../src/scenes/stateMachine.ts'

describe('runtime state', () => {
  it('collapses history on lock and unlock', () => {
    const expanded = { ...INITIAL_RUNTIME_STATE, phase: 'chatting' as const, historyExpanded: true }
    const locked = reduceRuntime(expanded, { type: 'LOCK' })
    expect(locked.phase).toBe('locked')
    expect(locked.historyExpanded).toBe(false)
    const waking = reduceRuntime(locked, { type: 'UNLOCK', playWake: true })
    expect(waking.phase).toBe('waking')
    expect(waking.historyExpanded).toBe(false)
  })

  it('keeps backend and model state orthogonal to the phase', () => {
    const harness = reduceRuntime(INITIAL_RUNTIME_STATE, { type: 'PATCH', patch: { backend: 'harness', model: 'deepseek-pro', modelTier: 'pro' } })
    expect(reduceRuntime(harness, { type: 'OPEN_CHAT' })).toMatchObject({ backend: 'harness', modelTier: 'pro' })
  })
})
