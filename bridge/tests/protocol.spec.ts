import { describe, expect, it } from 'vitest'
import { SessionId, Session } from '@deepseek-ai/dsh-session'
import { createAssistantMessage } from '@deepseek-ai/dsh-llm'
import { bearerAuthorized, mapSessionEvent, parseSessionRoute } from '../src/protocol.ts'

describe('wallpaper bridge protocol', () => {
  it('parses only versioned session routes', () => {
    expect(parseSessionRoute('/api/wallpaper/v1/sessions')).toEqual({ kind: 'collection' })
    expect(parseSessionRoute('/api/wallpaper/v1/sessions/a%20b/events')).toEqual({ kind: 'events', sessionId: 'a b' })
    expect(parseSessionRoute('/api/wallpaper/v2/sessions')).toBeNull()
    expect(parseSessionRoute('/api/wallpaper/v1/sessions/a/private')).toBeNull()
  })

  it('requires an exact bearer token', () => {
    expect(bearerAuthorized('Bearer secret-token', 'secret-token')).toBe(true)
    expect(bearerAuthorized('Bearer secret-token-x', 'secret-token')).toBe(false)
    expect(bearerAuthorized(undefined, 'secret-token')).toBe(false)
  })

  it('maps text deltas and usage into stable wallpaper events', () => {
    const session = Session.create(SessionId('bridge-test'))
    const message = createAssistantMessage({
      content: [{ type: 'text', text: 'done' }],
      source: { provider: 'mock', model: 'flash' },
    })
    const event = session.append('assistant/message', {
      turn: 1,
      step: 1,
      message,
      usage: { inputTokens: 12, outputTokens: 3, cacheReadTokens: 4 },
    }, { surfaceOp: 'append', sourceEventSeqs: [] })
    expect(mapSessionEvent(event)).toEqual([
      { type: 'message', role: 'assistant', content: 'done' },
      { type: 'usage', input: 12, output: 3, cacheRead: 4 },
    ])
  })
})
