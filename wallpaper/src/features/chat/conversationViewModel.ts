import { t } from '../../i18n/index.ts'
import type { Activity, BackendMode, ChatMessage, TokenUsage } from '../../domain/types.ts'

export interface BackendPresentation {
  name: string
  shortName: string
  description: string
  experimental: boolean
}

/**
 * The wording is read through getters rather than filled in once at module load:
 * the language is restored from the settings document after startup, so a value
 * captured at import time would keep showing the wrong language. `shortName` is a
 * product name (`DeepSeek Web`, `Harness`) and is the same in both languages.
 */
export const BACKEND_PRESENTATION: Record<BackendMode, BackendPresentation> = {
  'deepseek-web': {
    get name() { return t('chat.backend.deepseek-web.name') },
    shortName: 'DeepSeek Web',
    get description() { return t('chat.backend.deepseek-web.description') },
    experimental: true,
  },
  'deepseek-api': {
    name: 'DeepSeek API',
    shortName: 'DeepSeek API',
    get description() { return t('chat.backend.deepseek-api.description') },
    experimental: false,
  },
  harness: {
    name: 'DeepSeek Harness',
    shortName: 'Harness',
    get description() { return t('chat.backend.harness.description') },
    experimental: false,
  },
}

/** Same reason as above: these are read while rendering, not once at import. */
export const ACTIVITY_LABEL: Record<Activity, string> = {
  get idle() { return t('chat.activity.idle') },
  get sending() { return t('chat.activity.sending') },
  get thinking() { return t('chat.activity.thinking') },
  get streaming() { return t('chat.activity.streaming') },
  get tool() { return t('chat.activity.tool') },
  get done() { return t('chat.activity.done') },
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
  return `${estimated ? t('chat.cost.approx') : ''}¥${cost.toFixed(4)}`
}

/**
 * The compact footer is deliberately complete even when a backend did not
 * return metering data. A blank footer made an unmetered reply look like a
 * free reply, which is especially misleading for API mode. Zero remains a
 * real value at every field and must never be formatted as unavailable.
 */
export interface TurnUsageSummary {
  available: boolean
  input: string
  output: string
  cacheRead: string
  cost: string
}

export function turnUsageSummary(
  usage: TokenUsage | undefined,
  backend: BackendMode,
  apiPricingConfigured: boolean,
): TurnUsageSummary {
  const unavailable = t('chat.usage.unavailable')
  if (!usage) {
    return {
      available: false,
      input: unavailable,
      output: unavailable,
      cacheRead: unavailable,
      cost: backend === 'deepseek-api' && !apiPricingConfigured
        ? t('chat.usage.price-unconfigured')
        : t('chat.usage.cost-unavailable'),
    }
  }
  return {
    available: true,
    input: String(usage.input),
    output: String(usage.output),
    cacheRead: usage.cacheRead === undefined ? unavailable : String(usage.cacheRead),
    cost: usage.cost === undefined
      ? backend === 'deepseek-api' && !apiPricingConfigured
        ? t('chat.usage.price-unconfigured')
        : t('chat.usage.cost-unavailable')
      : formatCost(usage.cost, usage.estimated),
  }
}

export function composerPlaceholder(disabled: boolean, activity: Activity): string {
  if (disabled) return t('chat.composer.disabled')
  if (isBusyActivity(activity)) return t('chat.composer.busy')
  return t('chat.composer.placeholder')
}
