/** 壁纸状态机：系统阶段与会话/后端状态解耦。 */

import type { RuntimeState } from '../domain/types.ts'

/** v0.1 兼容状态，供纯函数迁移测试使用。 */
export type WallpaperState = 'sleep' | 'waking' | 'idle' | 'chat'

export type WallpaperEvent =
  | 'sleep' // 进入睡眠（SessionSwitch 锁定 / 手动睡眠 / 空闲超时）
  | 'unlock' // 解锁（系统解锁回桌面 / 应用内密码通过）
  | 'wakeDone' // 苏醒动画播完
  | 'openChat' // 单击立绘 / 快捷键
  | 'closeChat' // 关闭会话窗
  | 'harnessOnline' // 3080 上线（副状态：形态联动 + 气泡提示）
  | 'harnessOffline' // 3080 下线

export interface StateTransition {
  from: WallpaperState | WallpaperState[]
  event: WallpaperEvent
  to: WallpaperState
}

/** 状态迁移表：单一数据源，可被测试逐行断言 */
export const TRANSITIONS: StateTransition[] = [
  { from: 'sleep', event: 'unlock', to: 'waking' },
  { from: 'waking', event: 'wakeDone', to: 'idle' },
  { from: 'idle', event: 'openChat', to: 'chat' },
  { from: 'chat', event: 'closeChat', to: 'idle' },
  { from: ['idle', 'chat'], event: 'sleep', to: 'sleep' },
]

/** 状态机：纯函数式转移，返回新状态；非法转移返回原状态 */
export function nextState(current: WallpaperState, event: WallpaperEvent): WallpaperState {
  for (const t of TRANSITIONS) {
    const froms = Array.isArray(t.from) ? t.from : [t.from]
    if (froms.includes(current) && t.event === event) return t.to
  }
  return current
}

export type RuntimeEvent =
  | { type: 'BOOT_READY'; playWake: boolean }
  | { type: 'LOCK' }
  | { type: 'UNLOCK'; playWake: boolean }
  | { type: 'WAKE_DONE' }
  | { type: 'OPEN_CHAT' }
  | { type: 'CLOSE_CHAT' }
  | { type: 'AUTH_REQUIRED' }
  | { type: 'AUTH_READY' }
  | { type: 'FAIL'; message: string }
  | { type: 'RECOVER' }
  | { type: 'TOGGLE_HISTORY' }
  | { type: 'PATCH'; patch: Partial<RuntimeState> }

export const INITIAL_RUNTIME_STATE: RuntimeState = {
  phase: 'booting',
  backend: 'deepseek-web',
  modelTier: 'flash',
  activity: 'idle',
  historyExpanded: false,
  harness: 'offline',
}

export function reduceRuntime(state: RuntimeState, event: RuntimeEvent): RuntimeState {
  switch (event.type) {
    case 'BOOT_READY':
      return { ...state, phase: event.playWake ? 'waking' : 'idle', historyExpanded: false }
    case 'LOCK':
      return { ...state, phase: 'locked', historyExpanded: false }
    case 'UNLOCK':
      return { ...state, phase: event.playWake ? 'waking' : 'idle', historyExpanded: false }
    case 'WAKE_DONE':
      return state.phase === 'waking' ? { ...state, phase: 'idle' } : state
    case 'OPEN_CHAT':
      return state.phase === 'idle' ? { ...state, phase: 'chatting' } : state
    case 'CLOSE_CHAT':
      return state.phase === 'chatting' ? { ...state, phase: 'idle', historyExpanded: false } : state
    case 'AUTH_REQUIRED':
      return { ...state, phase: 'auth-required' }
    case 'AUTH_READY':
      return state.phase === 'auth-required' ? { ...state, phase: 'idle' } : state
    case 'FAIL':
      return { ...state, phase: 'error', error: event.message }
    case 'RECOVER':
      return { ...state, phase: 'idle', error: undefined }
    case 'TOGGLE_HISTORY':
      return { ...state, historyExpanded: !state.historyExpanded }
    case 'PATCH':
      return { ...state, ...event.patch }
  }
}

/** 状态机 hook：返回 [state, dispatch]，dispatch 触发转移并回调 */
export class WallpaperStateMachine {
  private state: WallpaperState = 'sleep'
  private listeners = new Set<(s: WallpaperState) => void>()

  get current(): WallpaperState {
    return this.state
  }

  subscribe(fn: (s: WallpaperState) => void): () => void {
    this.listeners.add(fn)
    return () => this.listeners.delete(fn)
  }

  dispatch(event: WallpaperEvent): WallpaperState {
    const next = nextState(this.state, event)
    if (next !== this.state) {
      this.state = next
      for (const fn of this.listeners) fn(this.state)
    }
    return this.state
  }
}
