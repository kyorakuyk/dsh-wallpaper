import type { Message, TokenUsage } from '@deepseek-ai/dsh-llm'
import type { SessionEvent } from '@deepseek-ai/dsh-session'

export const BRIDGE_VERSION = '1.0.0'
export const API_PREFIX = '/api/wallpaper/v1'

export type BridgeEvent =
  | { type: 'status'; activity: 'idle' | 'sending' | 'thinking' | 'streaming' | 'tool' | 'done' }
  | { type: 'delta'; text: string }
  | { type: 'message'; role: 'user' | 'assistant'; content: string }
  | { type: 'usage'; input: number; output: number; cacheRead?: number; cost?: number }
  | { type: 'model'; provider?: string; model: string; effort?: string }
  | { type: 'approval-required'; sessionId: string; summary: string }
  | { type: 'error'; code: string; recoverable: boolean; message: string }
  | { type: 'disconnected'; recoverable: true }

export type SessionItemRouteKind = 'messages' | 'history' | 'events' | 'cancel'

export type SessionRoute =
  | { kind: 'collection' }
  | { kind: SessionItemRouteKind; sessionId: string }
  | null

const MAX_SESSION_ID_LENGTH = 200

export function parseSessionRoute(pathname: string): SessionRoute {
  if (pathname === `${API_PREFIX}/sessions`) return { kind: 'collection' }
  const match = pathname.match(new RegExp(`^${API_PREFIX}/sessions/([^/]+)/(messages|history|events|cancel)$`))
  if (!match?.[1] || !match[2]) return null
  let sessionId: string
  try {
    sessionId = decodeURIComponent(match[1])
  } catch {
    return null
  }
  return isSafeSessionId(sessionId) ? { kind: match[2] as SessionItemRouteKind, sessionId } : null
}

/**
 * Session IDs cross the local HTTP boundary and are ultimately handed to DSH.
 * Keep that boundary deliberately boring: no control characters, no oversized
 * values, and no path separators even after URL decoding.
 */
export function isSafeSessionId(value: string): boolean {
  return value.length > 0
    && value.length <= MAX_SESSION_ID_LENGTH
    && !/[\u0000-\u001f\u007f/\\]/.test(value)
}

/** A stable, non-sensitive reference suitable for a local HTTP error body. */
export function errorReference(error: unknown): string {
  const message = error instanceof Error ? `${error.name}:${error.message}` : String(error)
  // Keep the original failure (which can contain a model response or a path)
  // out of both response bodies and ordinary bridge logs.
  return createHash('sha256').update(message).digest('hex').slice(0, 12)
}

export function bearerAuthorized(header: string | undefined, token: string): boolean {
  // Token creation deliberately uses 32 random bytes. Treat a missing or
  // truncated token as an authentication setup failure, never as a valid empty
  // secret (timingSafeEqual accepts two empty buffers).
  if (token.length < 32 || !header?.startsWith('Bearer ')) return false
  const candidate = Buffer.from(header.slice(7))
  const expected = Buffer.from(token)
  return candidate.length === expected.length && BunSafeTimingEqual(candidate, expected)
}

import { createHash, timingSafeEqual } from 'node:crypto'

function BunSafeTimingEqual(left: Buffer, right: Buffer): boolean {
  return timingSafeEqual(left, right)
}

export function contentText(message: Pick<Message, 'content'>): string {
  return message.content
    .filter((block): block is Extract<(typeof message.content)[number], { type: 'text' }> => block.type === 'text')
    .map((block) => block.text)
    .join('')
}

export function usageEvent(usage: TokenUsage): BridgeEvent {
  return {
    type: 'usage',
    input: usage.inputTokens,
    output: usage.outputTokens,
    ...(usage.cacheReadTokens === undefined ? {} : { cacheRead: usage.cacheReadTokens }),
  }
}

export function mapSessionEvent(event: SessionEvent): BridgeEvent[] {
  switch (event.type) {
    case 'turn/start': return [{ type: 'status', activity: 'sending' }]
    case 'step/start': return [{ type: 'status', activity: 'thinking' }]
    case 'assistant/chunk': {
      const chunk = event.data.chunk
      if (chunk.type === 'text-delta' && chunk.text) return [{ type: 'status', activity: 'streaming' }, { type: 'delta', text: chunk.text }]
      if (chunk.type === 'reasoning-delta' && chunk.text) return [{ type: 'status', activity: 'thinking' }]
      if (chunk.type === 'tool-call-delta') return [{ type: 'status', activity: 'tool' }]
      return []
    }
    case 'assistant/message': {
      const result: BridgeEvent[] = [{ type: 'message', role: 'assistant', content: contentText(event.data.message) }]
      if (event.data.usage) result.push(usageEvent(event.data.usage))
      return result
    }
    case 'user/message': return [{ type: 'message', role: 'user', content: contentText(event.data) }]
    case 'tool/call': return [{ type: 'status', activity: 'tool' }]
    case 'turn/end': return [{ type: 'status', activity: event.data.reason.kind === 'completed' ? 'done' : 'idle' }]
    case 'request/context': return [{
      type: 'model',
      provider: event.data.provider,
      model: event.data.model,
    }]
    default: return []
  }
}
