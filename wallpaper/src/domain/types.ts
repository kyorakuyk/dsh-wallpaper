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

export type ChatEvent =
  | { type: 'status'; activity: Activity }
  | { type: 'delta'; text: string }
  | { type: 'message'; role: 'user' | 'assistant'; content: string; usage?: TokenUsage }
  | ({ type: 'usage' } & TokenUsage)
  | { type: 'model'; provider?: string; model: string; tier: ModelTier; effort?: string }
  | { type: 'approval-required'; sessionId: string; summary: string }
  | { type: 'auth-required' }
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
  harness: 'offline' | 'web-only' | 'bridge-ready'
  error?: string
}
