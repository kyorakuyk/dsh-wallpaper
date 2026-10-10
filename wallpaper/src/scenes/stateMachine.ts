/** 壁纸状态机：系统阶段与会话/后端状态解耦。 */

import type { RuntimeState } from '../domain/types.ts'

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
    case 'PATCH': {
      /**
       * 换了 `error` 那句话，就一起清掉它的**种类**。
       *
       * `errorKind` 描述的是"现在 `error` 里那句是我们哪一类提示"（与语言无关的判据，见
       * `domain/types.ts`）。补丁若没自带 `errorKind`，那它换的就是别的一句话（原生自由文本、
       * 另一类提示），旧标志必须作废 —— 否则 `harnessAvailabilityPatch` 会拿着旧标志去清一句
       * 根本不是掉线提示的话。只有明确写了 `errorKind` 的补丁（掉线提示自己）才保留它。
       *
       * 放在这里而不是各个调用点：调用点有二十几个，靠"记得写"迟早会漏一个，而这一条是
       * **状态的形状**该保证的事。
       */
      const patch = 'error' in event.patch && !('errorKind' in event.patch)
        ? { ...event.patch, errorKind: undefined }
        : event.patch
      return { ...state, ...patch }
    }
  }
}
