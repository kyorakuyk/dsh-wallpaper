import { readFile, readdir } from 'node:fs/promises'
import { describe, expect, it } from 'vitest'
import { SessionId, Session } from '@deepseek-ai/dsh-session'
import { createAssistantMessage, createUserMessage } from '@deepseek-ai/dsh-llm'
import { BRIDGE_VERSION, bearerAuthorized, errorReference, isSafeSessionId, isVisibleWallpaperMessage, mapSessionEvent, parseSessionRoute } from '../src/protocol.ts'

describe('wallpaper bridge protocol', () => {
  /**
   * 版本号是"这份拷贝是哪一版"的唯一凭据（`/status` 的 `bridgeVersion`），它被**烘焙进构建产物**。
   * 代价很具体：改了 `package.json` 却忘了 `build`，产物就停在上一个版本上 —— 我今天正是据此
   * **误判**"新代码没生效"，白费一轮。所以产物必须与清单对得上。
   */
  it('keeps the reported version, the manifest, and the built artifact in step', async () => {
    const manifest = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8')) as { version: string }
    expect(BRIDGE_VERSION).toBe(manifest.version)
    // 版本被烘焙进**共享 chunk**（不是 index.js），所以整个 lib/ 都要看。
    const directory = new URL('../lib/', import.meta.url)
    const artifacts = await readdir(directory).catch(() => [] as string[])
    if (artifacts.length === 0) return
    const contents = await Promise.all(artifacts.map((name) => readFile(new URL(name, directory), 'utf8')))
    expect(contents.join('\n')).toContain(manifest.version)
  })

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

  it('maps ask_user_question tool calls into a desktop question prompt', () => {
    const session = Session.create(SessionId('question-session'))
    const event = session.append('tool/call', {
      turn: 1,
      step: 1,
      callId: 'call-question' as never,
      name: 'ask_user_question',
      arguments: JSON.stringify({ questions: [{ id: 'choice', question: '继续吗？', options: [{ label: '继续' }] }] }),
    })
    expect(mapSessionEvent(event, 'question-session')).toEqual([
      { type: 'status', activity: 'tool' },
      { type: 'question-required', sessionId: 'question-session', questions: [{ id: 'choice', question: '继续吗？', options: [{ label: '继续' }] }] },
    ])
  })

  /**
   * 一轮以非 `completed` 的原因结束时，宿主是**拒绝了**这一轮（真机实测：归档掉的会话，
   * `turn/end` 的 `reason.kind` 就是 `blocked`，几毫秒内结束、没有 step、没有回答）。
   * 以前这里和"正常跑完但没说话"一样只回 `idle`，壁纸看到的是"结束了、没事"——用户看到的就是
   * "输入被吞、灯还是绿的"。异常被说成正常比异常本身危险，所以这里必须额外报出来。
   */
  it('reports a turn the host refused instead of mapping it to a silent idle', () => {
    const session = Session.create(SessionId('blocked-session'))
    // `turn/end` 不是 surface-eligible 事件，本来就不带 surfaceOp。
    const completed = session.append('turn/end', { turn: 1, reason: { kind: 'completed' } })
    const blocked = session.append('turn/end', { turn: 2, reason: { kind: 'blocked' } })

    expect(mapSessionEvent(completed)).toEqual([{ type: 'status', activity: 'done' }])
    expect(mapSessionEvent(blocked)).toEqual([
      { type: 'status', activity: 'idle' },
      {
        type: 'error',
        code: 'turn-blocked',
        recoverable: true,
        message: 'DSH 拒绝了这一轮对话（原因：blocked）；这条会话可能已被归档或不再可写。',
      },
    ])
  })

  it('never exposes plugin-injected runtime context as a user chat message', () => {    const session = Session.create(SessionId('bridge-context-filter'))
    const injected = createUserMessage({
      content: [{ type: 'text', text: 'internal runtime context' }],
      source: { kind: 'plugin', plugin: 'agent-instructions' },
    })
    const user = createUserMessage({
      content: [{ type: 'text', text: 'visible user message' }],
      source: { kind: 'user' },
    })
    const injectedEvent = session.append('user/message', injected, { surfaceOp: 'append' })
    const userEvent = session.append('user/message', user, { surfaceOp: 'append' })

    expect(isVisibleWallpaperMessage(injected)).toBe(false)
    expect(isVisibleWallpaperMessage(user)).toBe(true)
    expect(mapSessionEvent(injectedEvent)).toEqual([])
    expect(mapSessionEvent(userEvent)).toEqual([
      { type: 'message', role: 'user', content: 'visible user message' },
    ])
  })
})
