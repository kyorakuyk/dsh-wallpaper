import type { Sentence } from '../i18n/index.ts'

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
  | { type: 'conversation-reset'; reason: 'session-archived' | 'turn-blocked'; message: Sentence }
  | { type: 'error'; code: string; recoverable: boolean; message: Sentence }

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

/**
 * 通知条上那句**我们自己写的**提示属于哪一类。
 *
 * 与语言无关：以前判"这句是不是我们写的那条掉线提示"靠**文字前缀**（`startsWith`），
 * 切到英文之后那句话不再以中文前缀开头，于是掉线提示永远清不掉 —— 一个只在英文界面里出现的
 * 故障。现在判的是这个标志位。
 *
 * 只有一类，而且掉线提示与"Harness 模式现在不能切"共用同一个前缀：改动前两者都会被同一条
 * `startsWith` 规则清掉，所以它们共用同一个标志位，行为逐字不变。
 */
export type RuntimeNoticeKind = 'bridge-unavailable'

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
  /**
   * 通知条上的一句话。
   *
   * 我们自己的句子存成 `Message`（键 + 参数）——**渲染期**才求值，所以会话期间切语言它会跟着
   * 重译，而不是留在写进状态那一刻的语言里。原生（Rust）返回的自由文本保持字符串原样：
   * 它是运行时的系统文字，本批不翻译（`docs/plans/i18n-plan.md` 第一节第 6 条）。
   */
  error?: Sentence
  /** `error` 里那句**我们自己写的**提示是哪一类；原生文本与"没有提示"都留空。 */
  errorKind?: RuntimeNoticeKind
  /**
   * 聊天层自己要说的一句话（"这条会话被拒绝了，已经换新会话重发"之类）。
   *
   * **必须和 `error` 分开**：`error` 会被**原生快照整体覆写**（`error: snapshot.error`），
   * 而快照随活动状态一路推下来 —— 两者挤在同一个字段里时，聊天层的通知刚写进去就被下一条
   * 快照擦掉，用户只看到"顶上闪了一下"（实测）。分开之后快照永远碰不到它 ✓。
   * 显示优先级：`chatNotice` 在前（它更新、更针对当前这一刻），`error` 在后。
   */
  chatNotice?: Sentence
}
