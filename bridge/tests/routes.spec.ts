import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Context } from '@deepseek-ai/cordis'
import { API_PREFIX } from '../src/protocol.ts'
import { apply } from '../src/index.ts'

interface CapturedResponse {
  status: number
  headers: Record<string, string>
  body: string
  chunks: string[]
  response: ServerResponse
}

interface RouteHarness {
  root: string
  tokenFile: string
  routes: Map<string, (req: IncomingMessage, res: ServerResponse) => void | Promise<void>>
  listeners: Map<string, (...args: never[]) => unknown>
  agent: {
    session: { id: string; deriveMessages(): [] }
    status: 'idle'
    options: { provider: string; model: string }
    followup: ReturnType<typeof vi.fn>
    cancel: ReturnType<typeof vi.fn>
  }
  create: ReturnType<typeof vi.fn>
  logger: { warn: ReturnType<typeof vi.fn> }
}

const cleanups: string[] = []
afterEach(async () => {
  await Promise.all(cleanups.splice(0).map((path) => rm(path, { recursive: true, force: true })))
})

function request(method: string, path: string, body?: unknown, authorization?: string): IncomingMessage {
  const payload = body === undefined
    ? []
    : [Buffer.from(typeof body === 'string' ? body : JSON.stringify(body), 'utf8')]
  return {
    method,
    url: path,
    headers: authorization === undefined ? {} : { authorization },
    on: () => undefined,
    async *[Symbol.asyncIterator](): AsyncGenerator<Buffer> {
      yield* payload
    },
  } as unknown as IncomingMessage
}

function response(): CapturedResponse {
  const captured: Omit<CapturedResponse, 'response'> = {
    status: 200,
    headers: {},
    body: '',
    chunks: [],
  }
  let ended = false
  const native = {
    get writableEnded(): boolean { return ended },
    destroyed: false,
    statusCode: 200,
    setHeader(name: string, value: string): void { captured.headers[name.toLowerCase()] = value },
    flushHeaders: () => undefined,
    write(chunk: string): boolean { captured.chunks.push(String(chunk)); return true },
    end(body?: string): void { if (body !== undefined) captured.body += body; ended = true },
  } as unknown as ServerResponse
  return {
    get body() { return captured.body },
    get headers() { return captured.headers },
    get chunks() { return captured.chunks },
    get status() { return native.statusCode },
    response: native,
  }
}

async function createHarness(): Promise<RouteHarness> {
  const root = await mkdtemp(join(tmpdir(), 'dsh-wallpaper-bridge-'))
  cleanups.push(root)
  const routes = new Map<string, (req: IncomingMessage, res: ServerResponse) => void | Promise<void>>()
  const listeners = new Map<string, (...args: never[]) => unknown>()
  const agent = {
    session: { id: '', deriveMessages: () => [] as [] },
    status: 'idle' as const,
    options: { provider: 'mock', model: 'deepseek-chat' },
    followup: vi.fn(),
    cancel: vi.fn(),
  }
  const create = vi.fn(async (options: { sessionId: string }) => {
    agent.session.id = options.sessionId
    return { agent, dispose: async () => undefined }
  })
  const logger = { warn: vi.fn() }
  const webServer = {
    host: '127.0.0.1' as const,
    register: (route: { path: string; handler: (req: IncomingMessage, res: ServerResponse) => void | Promise<void> }) => {
      routes.set(route.path, route.handler)
      return () => { routes.delete(route.path) }
    },
  }
  const context = {
    on: (event: string, listener: (...args: never[]) => unknown) => {
      listeners.set(event, listener)
      return () => { listeners.delete(event) }
    },
    effect: () => undefined,
    inject: (_dependencies: string[], callback: (scope: unknown) => void) => callback({ webServer, agents: { create, resume: create }, logger, effect: () => undefined }),
  } as unknown as Context
  const tokenFile = join(root, 'bridge-token')
  apply(context, { tokenFile })
  return { root, tokenFile, routes, listeners, agent, create, logger }
}

async function call(
  handler: ((req: IncomingMessage, res: ServerResponse) => void | Promise<void>) | undefined,
  req: IncomingMessage,
): Promise<CapturedResponse> {
  expect(handler).toBeDefined()
  const captured = response()
  await handler?.(req, captured.response)
  return captured
}

describe('wallpaper bridge HTTP routes', () => {
  it('keeps status public while protecting standard session operations with the generated token', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)

    const status = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    expect(status.status).toBe(200)
    expect(JSON.parse(status.body)).toMatchObject({
      bridgeVersion: '1.0.0',
      protocolVersion: 1,
      dsh: 'online',
      authentication: 'ready',
      capabilities: expect.arrayContaining([
        'sessions',
        'resume',
        'history',
        'sse',
        'cancel',
        'approval-handoff',
      ]),
    })

    const token = (await readFile(harness.tokenFile, 'utf8')).trim()
    expect(token).toHaveLength(43)
    expect(status.body).not.toContain(token)

    const rejected = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, {}))
    expect(rejected.status).toBe(401)
    expect(JSON.parse(rejected.body)).toEqual({ error: 'unauthorized' })

    const created = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'wallpaper-test' }, `Bearer ${token}`))
    expect(created.status).toBe(201)
    expect(JSON.parse(created.body)).toMatchObject({ sessionId: 'wallpaper-test', provider: 'mock', model: 'deepseek-chat' })
    expect(harness.create).toHaveBeenCalledOnce()

    const accepted = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions/wallpaper-test/messages`, { text: 'hello' }, `Bearer ${token}`))
    expect(accepted.status).toBe(202)
    expect(harness.agent.followup).toHaveBeenCalledOnce()

    const cancelled = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions/wallpaper-test/cancel`, {}, `Bearer ${token}`))
    expect(cancelled.status).toBe(202)
    expect(harness.agent.cancel).toHaveBeenCalledWith({ kind: 'user' })
  })

  it('rejects malformed or unsafe client input without returning request content', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const unsafeId = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: '../not-a-session' }, `Bearer ${token}`))
    expect(unsafeId.status).toBe(400)
    expect(JSON.parse(unsafeId.body)).toEqual({ error: 'invalid-session-id' })

    const secret = 'do-not-return-this-request-content'
    const malformed = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, `{ "secret": "${secret}`, `Bearer ${token}`))
    expect(malformed.status).toBe(400)
    expect(malformed.body).toContain('invalid-request')
    expect(malformed.body).not.toContain(secret)
    expect(harness.logger.warn.mock.calls.flat().join(' ')).not.toContain(secret)
  })

  it('enforces the message limit in UTF-8 bytes at the HTTP boundary', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()
    await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'unicode-limit' }, `Bearer ${token}`))

    // These are 100,001 UTF-8 bytes but only 33,334 JavaScript characters.
    const rejected = await call(
      sessionsRoute,
      request('POST', `${API_PREFIX}/sessions/unicode-limit/messages`, { text: '界'.repeat(33_334) }, `Bearer ${token}`),
    )
    expect(rejected.status).toBe(413)
    expect(JSON.parse(rejected.body)).toEqual({ error: 'text-too-large' })
    expect(harness.agent.followup).not.toHaveBeenCalled()
  })
})
