import type { Message, TokenUsage } from '@deepseek-ai/dsh-llm'
import type { SessionEvent } from '@deepseek-ai/dsh-session'
import packageManifest from '../package.json' with { type: 'json' }

type PackageManifest = {
  version?: string
  peerDependencies?: Record<string, string>
  devDependencies?: Record<string, string>
}

/**
 * The package the Bridge is compiled against. `dsh-agent` carries the agent and
 * session API surface the route handlers use, so its declared range is the
 * honest answer to "which DSH API does this build expect".
 */
const AUTHORITATIVE_HOST_PACKAGE = '@deepseek-ai/dsh-agent'

function authoredAgainst(): string {
  const manifest = packageManifest as PackageManifest
  const declared = manifest.peerDependencies?.[AUTHORITATIVE_HOST_PACKAGE]
    ?? manifest.devDependencies?.[AUTHORITATIVE_HOST_PACKAGE]
  // `*`, `latest`, and a missing entry say nothing about the expected surface,
  // so they report `unknown` instead of implying a verified range.
  if (typeof declared !== 'string') return 'unknown'
  const trimmed = declared.trim()
  return /^[\^~]?\d+\.\d+\.\d+/.test(trimmed) ? trimmed : 'unknown'
}

/**
 * The wallpaper↔Bridge REST/SSE contract version. It is bumped only for a
 * breaking change to `/api/wallpaper/v1` field meanings, and is deliberately
 * *not* the Bridge package version: the wallpaper depends on this stable
 * boundary, while the Bridge's own release is free to move independently
 * (field added: `bridgeVersion` now reports the package version instead of a
 * second hardcoded number, which is what made an installed copy impossible to
 * identify).
 */
export const BRIDGE_PROTOCOL_VERSION = 1
/**
 * Release version of this Bridge build, read from `package.json` so it cannot
 * drift from the published package (it was previously a second hardcoded
 * `1.1.0` next to a `0.1.1` manifest). The status endpoint reports it so a stale
 * copy installed in a DSH profile can be recognised without reading the
 * profile's `node_modules`.
 */
export const BRIDGE_VERSION: string = packageManifest.version ?? '0.0.0'
/**
 * The DSH release whose API definitions this Bridge build was compiled against.
 *
 * DSH exposes no runtime version: there is no `DSH_VERSION` environment
 * variable and no version service, only a `--version` CLI flag. Deriving the
 * host version from the plugin's own dependency declaration is therefore the
 * only claim this code can actually prove, and it is the one a support report
 * needs — it names the API surface the compiled code expects, which is also
 * what the verified matrix in `bridge/README.md` is keyed on.
 *
 * Report a range or a concrete version, whichever the dependency carries. A
 * wildcard or missing entry yields `unknown` rather than a guess.
 */
export const BRIDGE_AUTHORED_AGAINST: string = authoredAgainst()
/**
 * Non-sensitive build provenance. `DSH_WALLPAPER_BRIDGE_BUILD` is set by the
 * release build; `dev` is the honest answer for a local build, and it is never
 * used to decide compatibility.
 */
export const BRIDGE_BUILD = typeof process !== 'undefined' && process.env?.DSH_WALLPAPER_BRIDGE_BUILD
  ? String(process.env.DSH_WALLPAPER_BRIDGE_BUILD).slice(0, 40)
  : 'dev'
export const API_PREFIX = '/api/wallpaper/v1'

export interface BridgeQuestionOption {
  label: string
  description?: string
}

export interface BridgeQuestion {
  id: string
  question: string
  detail?: string
  header?: string
  options?: BridgeQuestionOption[]
  multiSelect?: boolean
}

export type BridgeEvent =
  | { type: 'status'; activity: 'idle' | 'sending' | 'thinking' | 'streaming' | 'tool' | 'done' }
  | { type: 'delta'; text: string }
  | { type: 'message'; role: 'user' | 'assistant'; content: string }
  | { type: 'usage'; input: number; output: number; cacheRead?: number; cost?: number }
  | { type: 'model'; provider?: string; model: string; effort?: string }
  | { type: 'question-required'; sessionId: string; questions: BridgeQuestion[] }
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

/**
 * The DSH durable transcript includes plugin-injected user-shaped context
 * (agent instructions, time context, workspace facts, and similar runtime
 * material). It is model input, not a user-visible chat turn. The wallpaper
 * must show only an explicit human user message or a final assistant message.
 */
export function isVisibleWallpaperMessage(
  message: Pick<Message, 'role'> & { source?: { kind?: string } },
): boolean {
  return message.role === 'assistant'
    || (message.role === 'user' && message.source?.kind === 'user')
}

export function usageEvent(usage: TokenUsage): BridgeEvent {
  return {
    type: 'usage',
    input: usage.inputTokens,
    output: usage.outputTokens,
    ...(usage.cacheReadTokens === undefined ? {} : { cacheRead: usage.cacheReadTokens }),
  }
}

/**
 * Whether a usage payload can be put on the stream at all.
 *
 * DSH does not guarantee every token counter is present: a response can carry
 * `inputTokens`/`outputTokens` as `undefined` (the wallpaper then renders "未提供"),
 * and a provider can report a cache read larger than the input, which cannot be
 * true. The stream has to survive both. Emitting an unrepresentable usage event
 * used to take the protocol's oversize-failure path, which closed the whole SSE
 * connection — so one missing counter destroyed the subscriber that had just been
 * handed the assistant's reply, and every later turn was lost with it.
 */
export function representableUsage(usage: TokenUsage): boolean {
  const valid = (value: unknown): boolean =>
    typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
  if (!valid(usage.inputTokens) || !valid(usage.outputTokens)) return false
  if (usage.cacheReadTokens === undefined) return true
  return valid(usage.cacheReadTokens) && usage.cacheReadTokens <= usage.inputTokens
}

function questionFromToolCall(event: Extract<SessionEvent, { type: 'tool/call' }>, sessionId?: string): BridgeEvent | undefined {
  if (event.data.name !== 'ask_user_question' || !sessionId || event.data.arguments.length > 100_000) return undefined
  let payload: unknown
  try { payload = JSON.parse(event.data.arguments) } catch { return undefined }
  if (!payload || typeof payload !== 'object' || !Array.isArray((payload as { questions?: unknown }).questions)) return undefined
  const questions: BridgeQuestion[] = []
  for (const candidate of (payload as { questions: unknown[] }).questions.slice(0, 8)) {
    if (!candidate || typeof candidate !== 'object') continue
    const item = candidate as Record<string, unknown>
    if (typeof item.id !== 'string' || typeof item.question !== 'string') continue
    const options = Array.isArray(item.options)
      ? item.options.slice(0, 12).flatMap((option): BridgeQuestionOption[] => {
        if (!option || typeof option !== 'object' || typeof (option as Record<string, unknown>).label !== 'string') return []
        const value = option as Record<string, unknown>
        return [{ label: value.label as string, ...(typeof value.description === 'string' ? { description: value.description } : {}) }]
      })
      : undefined
    questions.push({
      id: item.id,
      question: item.question,
      ...(typeof item.detail === 'string' ? { detail: item.detail } : {}),
      ...(typeof item.header === 'string' ? { header: item.header } : {}),
      ...(options?.length ? { options } : {}),
      ...(typeof (item.multiSelect ?? item.multi_select) === 'boolean' ? { multiSelect: (item.multiSelect ?? item.multi_select) as boolean } : {}),
    })
  }
  return questions.length ? { type: 'question-required', sessionId, questions } : undefined
}

export function mapSessionEvent(event: SessionEvent, sessionId?: string): BridgeEvent[] {
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
      // Only attach usage when it can actually be represented. An absent counter
      // means "not provided" (the wallpaper renders 未提供), not a protocol
      // violation, and must never close the stream that just delivered the reply.
      if (event.data.usage && representableUsage(event.data.usage)) result.push(usageEvent(event.data.usage))
      return result
    }
    case 'user/message': {
      return event.data.source.kind === 'user'
        ? [{ type: 'message', role: 'user', content: contentText(event.data) }]
        : []
    }
    case 'tool/call': {
      const question = questionFromToolCall(event, sessionId)
      return question ? [{ type: 'status', activity: 'tool' }, question] : [{ type: 'status', activity: 'tool' }]
    }
    // 一轮以非 `completed` 的原因结束，是**宿主拒绝了它**（实测：归档掉的会话，`turn/end` 的
    // `reason.kind` 就是 `blocked`，几毫秒内结束、既没有 step 也没有回答）。以前这里把它和
    // "正常跑完但没说话"一起翻成 `idle`，壁纸于是看到"结束了、没事"——用户看到的就是"输入被吞、
    // 灯还是绿的"。异常被说成正常比异常本身危险，所以非 `completed` 一律额外报一条可识别的错。
    case 'turn/end': {
      const reason = event.data.reason.kind
      const status: BridgeEvent = { type: 'status', activity: reason === 'completed' ? 'done' : 'idle' }
      if (reason === 'completed') return [status]
      return [status, {
        type: 'error',
        code: 'turn-blocked',
        recoverable: true,
        message: `DSH 拒绝了这一轮对话（原因：${reason}）；这条会话可能已被归档或不再可写。`,
      }]
    }
    case 'request/context': return [{
      type: 'model',
      provider: event.data.provider,
      model: event.data.model,
    }]
    default: return []
  }
}
