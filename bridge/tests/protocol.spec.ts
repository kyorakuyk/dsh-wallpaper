import { describe, expect, it } from 'vitest'
import { SessionId, Session } from '@deepseek-ai/dsh-session'
import { createAssistantMessage } from '@deepseek-ai/dsh-llm'
import { bearerAuthorized, errorReference, isSafeSessionId, mapSessionEvent, parseSessionRoute } from '../src/protocol.ts'

describe('wallpaper bridge protocol', () => {
  it('parses only versioned session routes', () => {
    expect(parseSessionRoute('/api/wallpaper/v1/sessions')).toEqual({ kind: 'collection' })
    expect(parseSessionRoute('/api/wallpaper/v1/sessions/a%20b/events')).toEqual({ kind: 'events', sessionId: 'a b' })
    expect(parseSessionRoute('/api/wallpaper/v2/sessions')).toBeNull()
    expect(parseSessionRoute('/api/wallpaper/v1/sessions/a/private')).toBeNull()
    expect(parseSessionRoute('/api/wallpaper/v1/sessions/%E0%A4%A/events')).toBeNull()
    expect(parseSessionRoute('/api/wallpaper/v1/sessions/a%2Fb/events')).toBeNull()
  })

  it('requires an exact bearer token', () => {
    const token = 'a'.repeat(43)
    expect(bearerAuthorized(`Bearer ${token}`, token)).toBe(true)
    expect(bearerAuthorized(`Bearer ${token}x`, token)).toBe(false)
    expect(bearerAuthorized('Bearer secret-token', 'secret-token')).toBe(false)
    expect(bearerAuthorized(undefined, 'secret-token')).toBe(false)
  })

  it('keeps session identifiers and public error references safe', () => {
    expect(isSafeSessionId('wallpaper-123')).toBe(true)
    expect(isSafeSessionId('../outside')).toBe(false)
    expect(isSafeSessionId(`a${String.fromCharCode(0)}b`)).toBe(false)
    expect(isSafeSessionId('a'.repeat(201))).toBe(false)

    const secret = 'Bearer this-must-not-leak'
    const reference = errorReference(new Error(secret))
    expect(reference).toMatch(/^[a-f0-9]{12}$/)
    expect(reference).not.toContain(secret)
    expect(errorReference(new Error(secret))).toBe(reference)
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
