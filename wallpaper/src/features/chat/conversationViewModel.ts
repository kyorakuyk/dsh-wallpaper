import type { Activity, BackendMode, ChatMessage, TokenUsage } from '../../domain/types.ts'

export interface BackendPresentation {
  name: string
  shortName: string
  description: string
  experimental: boolean
}

export const BACKEND_PRESENTATION: Record<BackendMode, BackendPresentation> = {
  'deepseek-web': { name: 'DeepSeek 网页桥接', shortName: 'DeepSeek Web', description: '免费 · 实验能力', experimental: true },
  'deepseek-api': { name: 'DeepSeek API', shortName: 'DeepSeek API', description: '按量计费', experimental: false },
  harness: { name: 'DeepSeek Harness', shortName: 'Harness', description: '本地工具会话', experimental: false },
}

export const ACTIVITY_LABEL: Record<Activity, string> = {
  idle: '待命',
  sending: '正在发送',
  thinking: '正在思考',
  streaming: '正在回复',
  tool: '正在使用工具',
  done: '已完成',
}

export function isBusyActivity(activity: Activity): boolean {
  return activity === 'sending' || activity === 'thinking' || activity === 'streaming' || activity === 'tool'
}

export function sessionCost(messages: ChatMessage[]): number {
  return sessionCostSummary(messages)?.cost ?? 0
}

/**
 * Distinguish an actual zero-price transcript from one whose price table or
 * provider usage is unavailable.  `0` is a valid user-entered price, so a
 * numeric accumulator alone would make the two states indistinguishable.
 */
export function sessionCostSummary(messages: ChatMessage[]): { cost: number; estimated: boolean } | undefined {
  const billed = messages
    .map((message) => message.usage)
    .filter((usage): usage is TokenUsage & { cost: number } => usage?.cost !== undefined)
  if (billed.length === 0) return undefined
  return {
    cost: billed.reduce((sum, usage) => sum + usage.cost, 0),
    estimated: billed.some((usage) => usage.estimated === true),
  }
}

export function usageTokenCount(usage?: TokenUsage): number | undefined {
  if (!usage) return undefined
  return usage.input + usage.output
}

export function formatCost(cost: number, estimated = false): string {
  return `${estimated ? '约 ' : ''}¥${cost.toFixed(4)}`
}

export function composerPlaceholder(disabled: boolean, activity: Activity): string {
  if (disabled) return '当前模式暂不可用'
  if (isBusyActivity(activity)) return '大肥鱼正在处理上一条消息…'
  return '今天要一起处理什么？'
}
