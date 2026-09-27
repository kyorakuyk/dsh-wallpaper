export type SystemPhase =
  | 'booting'
  | 'locked'
  | 'waking'
  | 'idle'
  | 'chatting'
  | 'auth-required'
  | 'error'

export type BackendMode = 'deepseek-web' | 'deepseek-api' | 'harness'
export type ModelTier = 'flash' | 'pro' | 'unknown'
export type Activity = 'idle' | 'sending' | 'thinking' | 'streaming' | 'tool' | 'done'
export type ConversationPolicy = 'resume-last' | 'new-on-unlock' | 'daily'

/**
 * Re-exported so domain consumers do not need to reach into `connect/harness`.
 * The definition, the reason codes and the status interpreter all live there.
 */
export type { HarnessAvailability } from '../connect/harness.ts'

export interface TokenUsage {
  input: number
  output: number
  cacheRead?: number
  cost?: number
  estimated?: boolean
}

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  createdAt: number
  usage?: TokenUsage
}

export interface ChatQuestionOption {
  label: string
  description?: string
}

export interface ChatQuestion {
  id: string
  question: string
  detail?: string
  header?: string
  options?: ChatQuestionOption[]
  multiSelect?: boolean
}

export type ChatEvent =
  | { type: 'status'; activity: Activity }
  | { type: 'delta'; text: string }
  | { type: 'message'; role: 'user' | 'assistant'; content: string; usage?: TokenUsage }
  | ({ type: 'usage' } & TokenUsage)
  | { type: 'model'; provider?: string; model: string; tier: ModelTier; effort?: string }
  | { type: 'question-required'; sessionId: string; questions: ChatQuestion[] }
  | { type: 'approval-required'; sessionId: string; summary: string }
  | { type: 'auth-required' }
  /**
   * 这条转写所属的会话已经不是当前会话了：`session-archived` 是桥明确回报归档，
   * `turn-blocked` 是宿主拒绝执行这一轮（实测归档会话就是这样，几毫秒结束、没有回答）。
   * 两者对用户的动作相同：轨道上那段记录必须立刻停止看起来像活的。`message` 是通知，不是报错。
   */
  | { type: 'conversation-reset'; reason: 'session-archived' | 'turn-blocked'; message: string }
  | { type: 'error'; code: string; recoverable: boolean; message: string }

/**
 * Native chat traffic shares Tauri's app-wide event channel. Origin metadata
 * lets a live adapter discard events from an old backend or transcript.
 */
export type ScopedChatEvent = ChatEvent & {
  backend?: BackendMode
  conversationId?: string
  requestId?: string
}

export interface ModelTierRule {
  backend: BackendMode | '*'
  provider?: string
  pattern: string
  match: 'exact' | 'contains' | 'regex'
  tier: Exclude<ModelTier, 'unknown'>
}

export interface ConversationRef {
  id: string
  updatedAt: number
}

export interface RuntimeState {
  phase: SystemPhase
  backend: BackendMode
  modelTier: ModelTier
  activity: Activity
  model?: string
  provider?: string
  reasoningEffort?: string
  historyExpanded: boolean
  harness: import('../connect/harness.ts').HarnessAvailability
  /** Stable, non-sensitive reason for a non-ready Harness state. */
  harnessReasonCode?: string
  /**
   * 一个就绪过的后端失联了，但还没到"确认掉线"的程度。
   *
   * 与 `harness` **分开**是有意的：这时 `harness` 仍然是 `bridge-ready`，滑槽、模型列表和
   * 会话都还按"它还在"处理 ✓，只有提示灯变黄 ✓。原生化之后这个事实由原生监控给出（它会看
   * 那个端口的进程是否还活着），浏览器预览那条路自己算。
   */
  harnessProbing?: boolean
  error?: string
  /**
   * 聊天层自己要说的一句话（"这条会话被拒绝了，已经换新会话重发"之类）。
   *
   * **必须和 `error` 分开**：`error` 会被**原生快照整体覆写**（`error: snapshot.error`），
   * 而快照随活动状态一路推下来 —— 两者挤在同一个字段里时，聊天层的通知刚写进去就被下一条
   * 快照擦掉，用户只看到"顶上闪了一下"（实测）。分开之后快照永远碰不到它 ✓。
   * 显示优先级：`chatNotice` 在前（它更新、更针对当前这一刻），`error` 在后。
   */
  chatNotice?: string
}
