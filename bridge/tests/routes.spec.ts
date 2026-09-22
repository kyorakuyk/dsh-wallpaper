import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Context } from '@deepseek-ai/cordis'
import { API_PREFIX } from '../src/protocol.ts'
import { apply, desktopEntryPrompt, historyOf, isLoopbackWebServerHost, tokenFileForRoot, windowsTokenAclCommands, windowsTokenDirectoryAclCommands, LIVE_SESSION_IDLE_TTL_MS, LIVE_SESSION_SWEEP_INTERVAL_MS, MAX_HISTORY_BYTES, MAX_HISTORY_MESSAGES, MAX_LIVE_SESSIONS, MAX_PENDING_CREATIONS, MAX_SSE_CLIENTS_PER_SESSION } from '../src/index.ts'

interface CapturedResponse {
  status: number
  headers: Record<string, string>
  body: string
  chunks: string[]
  destroyed: boolean
  response: ServerResponse
}

interface ResponseOptions {
  writeResult?: boolean | ((chunk: string, writeNumber: number) => boolean)
}

interface RouteHarness {
  root: string
  tokenRoot: string
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
  persistence: { enabled: boolean }
  injectedDependencies: string[] | undefined
  routeDependencies: string[] | undefined
  workspace: { title: string; path: string; sessionIds: string[]; attachSession: ReturnType<typeof vi.fn> }
  workspaceRegistry: { list: () => unknown[]; create: ReturnType<typeof vi.fn> }
}
const cleanups: Array<() => Promise<void>> = []
afterEach(async () => {
  await Promise.allSettled(cleanups.splice(0).map(async (cleanup) => { await cleanup() }))
})

function request(
  method: string,
  path: string,
  body?: unknown,
  authorization?: string,
  options: { declaredLength?: number; chunks?: Buffer[] } = {},
): IncomingMessage {
  const payload = body === undefined
    ? []
    : options.chunks ?? [Buffer.from(typeof body === 'string' ? body : JSON.stringify(body), 'utf8')]
  return {
    method,
    url: path,
    headers: {
      ...(authorization === undefined ? {} : { authorization }),
      ...(options.declaredLength === undefined ? {} : { 'content-length': String(options.declaredLength) }),
    },
    on: () => undefined,
    async *[Symbol.asyncIterator](): AsyncGenerator<Buffer> {
      yield* payload
    },
  } as unknown as IncomingMessage
}

function response(options: ResponseOptions = {}): CapturedResponse {
  const captured: Omit<CapturedResponse, 'response'> = {
    status: 200,
  headers: {},
  body: '',
  chunks: [],
  // The returned test facade exposes the live getter below. Seed the backing
  // shape as well so it stays structurally complete under `tsc --noEmit`.
  destroyed: false,
  }
  let ended = false
  let destroyed = false
  let writeNumber = 0
  const native = {
    get writableEnded(): boolean { return ended },
    get destroyed(): boolean { return destroyed },
    statusCode: 200,
    setHeader(name: string, value: string): void { captured.headers[name.toLowerCase()] = value },
    flushHeaders: () => undefined,
    write(chunk: string): boolean {
      captured.chunks.push(String(chunk))
      writeNumber += 1
      return typeof options.writeResult === 'function'
        ? options.writeResult(chunk, writeNumber)
        : options.writeResult ?? true
    },
    destroy(): void { destroyed = true },
    end(body?: string): void { if (body !== undefined) captured.body += body; ended = true },
  } as unknown as ServerResponse
  return {
    get body() { return captured.body },
    get headers() { return captured.headers },
    get chunks() { return captured.chunks },
    get destroyed() { return destroyed },
    get status() { return native.statusCode },
    response: native,
  }
}

async function createHarness(
  persistenceEnabled = false,
  prepareTokenRoot?: (tokenRoot: string) => Promise<void>,
  webServerHost: string = '127.0.0.1',
): Promise<RouteHarness> {
  const root = await mkdtemp(join(tmpdir(), 'dsh-wallpaper-bridge-'))
  const tokenRoot = join(root, 'host-owned-dsh-root')
  const effects: Array<() => unknown> = []
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
  const persistence = { enabled: persistenceEnabled }
  let workspaceCreated = false
  const workspace = {
    title: '桌面会话',
    path: join(tokenRoot, 'workspace', 'dsh-wallpaper-desktop'),
    sessionIds: [] as string[],
    attachSession: vi.fn(async (sessionId: string) => { workspace.sessionIds.unshift(sessionId) }),
  }
  const workspaceRegistry = {
    list: () => workspaceCreated ? [workspace] : [],
    create: vi.fn(async (path: string, title?: string) => {
      workspaceCreated = true
      workspace.path = path
      workspace.title = title ?? workspace.title
      return workspace
    }),
  }
  const webServer = {
    host: webServerHost,
    register: (route: { path: string; handler: (req: IncomingMessage, res: ServerResponse) => void | Promise<void> }) => {
      routes.set(route.path, route.handler)
      return () => { routes.delete(route.path) }
    },
  }
  let injectedDependencies: string[] | undefined
  /** The mutating session scope's declared dependencies, if it was composed. */
  let routeDependencies: string[] | undefined
  const context = {
    on: (event: string, listener: (...args: never[]) => unknown) => {
      listeners.set(event, listener)
      return () => { listeners.delete(event) }
    },
    effect: (callback: () => unknown) => { effects.push(callback) },
    get: (name: string) => name === 'sessionPersistence' && persistence.enabled ? {} : undefined,
    inject: (dependencies: string[], callback: (scope: unknown) => void) => {
      injectedDependencies = dependencies
      if (dependencies.includes('agents')) routeDependencies = dependencies
      callback({
        webServer,
        agents: { create, resume: create },
        agentDefaultModel: { currentSelection: () => ({ provider: 'default-provider', model: 'default-model' }) },
        agentPresets: {
          defaultId: 'standard',
          list: async () => [{ id: 'standard', trust: 'system' as const }],
          mount: async () => undefined,
        },
        workspaceRegistry,
        permissionPresets: { names: ['workspace-write', 'danger-full-access'], current: () => 'workspace-write', set: vi.fn() },
        commands: { list: () => [], execute: vi.fn(async () => undefined) },
        logger,
        effect: (callback: () => unknown) => { effects.push(callback) },
      })
    },
  } as unknown as Context
  // This is deliberately a host-owned root, not a token path. The bridge can
  // only touch its dedicated `wallpaper` child beneath it.
  const tokenFile = tokenFileForRoot(tokenRoot)
  await prepareTokenRoot?.(tokenRoot)
  apply(context, { tokenRoot })
  cleanups.push(async () => {
    // Token provisioning starts before the first HTTP request. Run the Cordis
    // effect disposers before removing the fixture root so whoami/icacls and
    // the idle sweep cannot still hold a handle below it.
    for (const effect of effects) {
      const disposer = effect()
      if (typeof disposer === 'function') await (disposer as () => unknown)()
    }
    await rm(root, { recursive: true, force: true })
  })
  return {
    root,
    tokenRoot,
    tokenFile,
    routes,
    listeners,
    agent,
    create,
    logger,
    persistence,
    // Read lazily: `inject` is called once per scope, so a snapshot taken here
    // would capture whichever scope happened to compose last.
    get injectedDependencies() { return injectedDependencies },
    get routeDependencies() { return routeDependencies },
    workspace,
    workspaceRegistry,
  }
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
  it('describes the desktop entry and its default capability boundary to DSH', () => {
    const prompt = desktopEntryPrompt('C:\\workspace\\dsh-wallpaper-desktop', '桌面会话', 'workspace-write')
    expect(prompt).toContain('dsh-wallpaper desktop interaction entry')
    expect(prompt).toContain('桌面会话')
    expect(prompt).toContain('workspace-write')
    expect(prompt).toContain('not the full Harness Web UI')
  })

  it('declares both agent lifecycle and web-server dependencies for HTTP routes', async () => {
    const harness = await createHarness()
    // The mutating session scope declares — and is gated on — the full service
    // set, not just the web server needed by the public status route.
    expect(harness.routeDependencies).toEqual(['agentDefaultModel', 'agentPresets', 'agents', 'webServer', 'workspaceRegistry', 'permissionPresets', 'commands'])
  })

  it('registers no route when the host is not the exact loopback address', async () => {
    expect(isLoopbackWebServerHost('127.0.0.1')).toBe(true)
    expect(isLoopbackWebServerHost('localhost')).toBe(false)
    expect(isLoopbackWebServerHost('0.0.0.0')).toBe(false)
    const harness = await createHarness(false, undefined, '0.0.0.0')
    expect(harness.routes.size).toBe(0)
  })

  it('uses only its fixed token slot beneath the host-owned root', () => {
    const root = 'C:\\Users\\whale\\.dsh'
    expect(tokenFileForRoot(root)).toBe('C:\\Users\\whale\\.dsh\\wallpaper\\bridge-token')
    expect(tokenFileForRoot(`${root}\\custom-token.txt`)).toBe(
      'C:\\Users\\whale\\.dsh\\custom-token.txt\\wallpaper\\bridge-token',
    )
  })

  it('fails closed instead of treating a configured root as an arbitrary token filename', async () => {
    const unrelatedContent = 'this file is not a bridge token'
    const harness = await createHarness(false, async (tokenRoot) => {
      // Simulates an old `tokenFile`-style value being supplied as the new
      // root. The bridge must not reset this file or its parent ACL.
      await writeFile(tokenRoot, unrelatedContent, 'utf8')
    })
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)

    const status = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    expect(status.status).toBe(200)
    expect(JSON.parse(status.body)).toMatchObject({ authentication: 'unavailable' })
    expect(await readFile(harness.tokenRoot, 'utf8')).toBe(unrelatedContent)
  })

  it('replaces every previous Windows token ACL grant before allowing the current user', () => {
    expect(windowsTokenAclCommands('C:\\Users\\whale\\.dsh\\wallpaper\\bridge-token', 'S-1-5-21-42')).toEqual([
      ['C:\\Users\\whale\\.dsh\\wallpaper\\bridge-token', '/setowner', '*S-1-5-21-42'],
      ['C:\\Users\\whale\\.dsh\\wallpaper\\bridge-token', '/reset'],
      ['C:\\Users\\whale\\.dsh\\wallpaper\\bridge-token', '/grant:r', '*S-1-5-21-42:(F)'],
      ['C:\\Users\\whale\\.dsh\\wallpaper\\bridge-token', '/inheritance:r'],
    ])
    expect(windowsTokenAclCommands('token', 'S-1-5-21-42').flat()).not.toContain('/inheritance:e')
    expect(windowsTokenDirectoryAclCommands('C:\\Users\\whale\\.dsh\\wallpaper', 'S-1-5-21-42')).toEqual([
      ['C:\\Users\\whale\\.dsh\\wallpaper', '/setowner', '*S-1-5-21-42'],
      ['C:\\Users\\whale\\.dsh\\wallpaper', '/reset'],
      ['C:\\Users\\whale\\.dsh\\wallpaper', '/grant:r', '*S-1-5-21-42:(OI)(CI)(F)'],
      ['C:\\Users\\whale\\.dsh\\wallpaper', '/inheritance:r'],
    ])
  })

  it('keeps status public while protecting standard session operations with the generated token', async () => {
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)

    const status = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    expect(status.status).toBe(200)
    expect(JSON.parse(status.body)).toMatchObject({
      bridgeVersion: '1.1.0',
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
    expect(harness.create).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: 'wallpaper-test',
      meta: { cwd: harness.workspace.path, agentPreset: 'standard' },
      agentOptions: { provider: 'default-provider', model: 'default-model' },
    }))

    const accepted = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions/wallpaper-test/messages`, { text: 'hello' }, `Bearer ${token}`))
    expect(accepted.status).toBe(202)
    expect(harness.agent.followup).toHaveBeenCalledOnce()

    const cancelled = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions/wallpaper-test/cancel`, {}, `Bearer ${token}`))
    expect(cancelled.status).toBe(202)
    expect(harness.agent.cancel).toHaveBeenCalledWith({ kind: 'user' })
  })

  it('lists preset metadata only for an authenticated local wallpaper client', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const controlRoute = harness.routes.get(`${API_PREFIX}/control`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const rejected = await call(controlRoute, request('GET', `${API_PREFIX}/control/presets`))
    expect(rejected.status).toBe(401)
    const listed = await call(controlRoute, request('GET', `${API_PREFIX}/control/presets`, undefined, `Bearer ${token}`))
    expect(listed.status).toBe(200)
    expect(JSON.parse(listed.body)).toEqual({
      presets: [expect.objectContaining({ id: 'standard', trust: 'system', isDefault: true })],
    })
  })

  it('owns one dated session inside the desktop workspace and resumes it while live', async () => {
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const first = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, {}, `Bearer ${token}`))
    const sessionId = JSON.parse(first.body).sessionId as string
    expect(first.status).toBe(201)
    expect(sessionId).toMatch(/^wallpaper-\d{4}-\d{2}-\d{2}$/)
    expect(harness.workspaceRegistry.create).toHaveBeenCalledWith(harness.workspace.path, '桌面会话')
    expect(harness.create).toHaveBeenCalledWith(expect.objectContaining({
      sessionId,
      meta: { cwd: harness.workspace.path, agentPreset: 'standard' },
    }))
    expect(harness.workspace.attachSession).toHaveBeenCalledWith(sessionId)

    const second = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, {}, `Bearer ${token}`))
    expect(second.status).toBe(200)
    expect(JSON.parse(second.body).sessionId).toBe(sessionId)
    expect(harness.create).toHaveBeenCalledOnce()
  })

  it('rejects a resume ID outside the bridge-owned desktop workspace', async () => {
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const owned = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'desktop-owned' }, `Bearer ${token}`))
    expect(owned.status).toBe(201)
    expect(harness.workspace.sessionIds).toContain('desktop-owned')

    const foreign = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { resumeSessionId: 'foreign-dsh-session' }, `Bearer ${token}`))
    expect(foreign.status).toBe(409)
    expect(JSON.parse(foreign.body)).toEqual({ error: 'resume-unavailable' })
    expect(harness.create).toHaveBeenCalledOnce()
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

  it('does not let an authenticated HTTP request choose the DSH working directory', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const created = await call(
      sessionsRoute,
      request('POST', `${API_PREFIX}/sessions`, {
        sessionId: 'host-owned-cwd',
        cwd: 'C:\\sensitive\\not-authorized-by-the-host',
      }, `Bearer ${token}`),
    )

    expect(created.status).toBe(201)
    expect(harness.create).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: 'host-owned-cwd',
      meta: { cwd: harness.workspace.path, agentPreset: 'standard' },
    }))
  })

  it('single-flights concurrent creation of the same live session', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    let releaseCreate: (() => void) | undefined
    harness.create.mockImplementationOnce(async (options: { sessionId: string }) => {
      harness.agent.session.id = options.sessionId
      await new Promise<void>((resolve) => { releaseCreate = resolve })
      return { agent: harness.agent, dispose: async () => undefined }
    })

    const first = response()
    const firstRequest = sessionsRoute?.(
      request('POST', `${API_PREFIX}/sessions`, { sessionId: 'single-flight' }, `Bearer ${token}`),
      first.response,
    )
    await vi.waitFor(() => expect(harness.create).toHaveBeenCalledOnce())

    const second = response()
    const secondRequest = sessionsRoute?.(
      request('POST', `${API_PREFIX}/sessions`, { sessionId: 'single-flight' }, `Bearer ${token}`),
      second.response,
    )
    await vi.waitFor(() => expect(harness.create).toHaveBeenCalledOnce())

    expect(releaseCreate).toBeTypeOf('function')
    releaseCreate?.()
    await Promise.all([firstRequest, secondRequest])

    expect(harness.create).toHaveBeenCalledOnce()
    expect(first.status).toBe(201)
    expect(second.status).toBe(200)
    expect(JSON.parse(first.body)).toMatchObject({ sessionId: 'single-flight' })
    expect(JSON.parse(second.body)).toMatchObject({ sessionId: 'single-flight' })
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

  it('does not reveal live sessions publicly and declares resume only with persistence', async () => {
    const harness = await createHarness(false)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    const initial = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    expect(JSON.parse(initial.body).capabilities).not.toContain('resume')
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()
    const created = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'private-live' }, `Bearer ${token}`))
    expect(created.status).toBe(201)

    const publicStatus = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    expect(publicStatus.body).not.toContain('private-live')
    expect(publicStatus.body).not.toContain('deepseek-chat')

    const resume = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { resumeSessionId: 'previous-session' }, `Bearer ${token}`))
    expect(resume.status).toBe(409)
    expect(JSON.parse(resume.body)).toEqual({ error: 'resume-unavailable' })
  })

  it('uses precise 400/413 request-body errors and stops oversized input', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()
    const tooLarge = await call(
      sessionsRoute,
      request('POST', `${API_PREFIX}/sessions`, {}, `Bearer ${token}`, { declaredLength: 1_048_577 }),
    )
    expect(tooLarge.status).toBe(413)
    expect(JSON.parse(tooLarge.body)).toEqual({ error: 'request-too-large' })

    const nonObject = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, [], `Bearer ${token}`))
    expect(nonObject.status).toBe(400)
    expect(JSON.parse(nonObject.body).error).toBe('invalid-request')
  })

  it('refuses an extra subscriber instead of multiplying one session write fan-out', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()
    await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'bounded-subscribers' }, `Bearer ${token}`))

    const events = `${API_PREFIX}/sessions/bounded-subscribers/events`
    const accepted: CapturedResponse[] = []
    for (let index = 0; index < MAX_SSE_CLIENTS_PER_SESSION; index += 1) {
      const response_ = await call(sessionsRoute, request('GET', events, undefined, `Bearer ${token}`))
      expect(response_.status).toBe(200)
      expect(response_.headers['x-dsh-wallpaper-sse-ready']).toBe('1')
      accepted.push(response_)
    }

    const extra = await call(sessionsRoute, request('GET', events, undefined, `Bearer ${token}`))
    expect(extra.status).toBe(409)
    expect(JSON.parse(extra.body)).toEqual({ error: 'too-many-subscribers' })
    // The rejected subscriber never entered the publish set.
    for (const client of accepted) expect(client.destroyed).toBe(false)
  })

  it('bounds the history payload before it becomes a JSON response body', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()
    await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'bounded-history' }, `Bearer ${token}`))

    const hidden = { id: 'ctx-1', role: 'user' as const, content: [{ type: 'text' as const, text: 'injected context' }], source: { kind: 'plugin' } }
    const messages = Array.from({ length: 12 }, (_, index) => ({
      id: `m-${index}`,
      role: index % 2 === 0 ? 'user' as const : 'assistant' as const,
      content: [{ type: 'text' as const, text: `turn ${index}` }],
      ...(index % 2 === 0 ? { source: { kind: 'user' } } : {}),
    }))
    // The durable transcript contains plugin-injected context that the
    // wallpaper must never show; history filtering still applies.
    harness.agent.session.deriveMessages = () => [hidden, ...messages] as never

    const full = await call(sessionsRoute, request('GET', `${API_PREFIX}/sessions/bounded-history/history`, undefined, `Bearer ${token}`))
    expect(full.status).toBe(200)
    const fullBody = JSON.parse(full.body) as { messages: Array<{ id: string }>; truncated: boolean }
    expect(fullBody.messages.map((message) => message.id)).toEqual(messages.map((message) => message.id))
    expect(fullBody.truncated).toBe(false)
    expect(full.body).not.toContain('injected context')

    // A caller-chosen window keeps the newest messages in chronological order.
    const windowed = await call(sessionsRoute, request('GET', `${API_PREFIX}/sessions/bounded-history/history?limit=3`, undefined, `Bearer ${token}`))
    const windowedBody = JSON.parse(windowed.body) as { messages: Array<{ id: string; content: string }>; truncated: boolean }
    expect(windowedBody.messages.map((message) => message.id)).toEqual(['m-9', 'm-10', 'm-11'])
    expect(windowedBody.truncated).toBe(true)

    // The limit is clamped to the documented ceiling, never trusted as-is.
    const clamped = await call(sessionsRoute, request('GET', `${API_PREFIX}/sessions/bounded-history/history?limit=100000`, undefined, `Bearer ${token}`))
    expect((JSON.parse(clamped.body) as { messages: unknown[] }).messages).toHaveLength(12)
    expect(clamped.body).toContain(`"maxMessages":${MAX_HISTORY_MESSAGES}`)
  })

  it('drops whole messages to respect the history byte budget without cutting one body', () => {
    const content = (text: string) => [{ type: 'text' as const, text }]
    const session = {
      deriveMessages: () => [
        { id: 'old', role: 'user' as const, content: content('x'.repeat(400)), source: { kind: 'user' } },
        { id: 'newest', role: 'assistant' as const, content: content('y'.repeat(4_000)) },
      ],
    }
    const bounded = historyOf(session as never, MAX_HISTORY_MESSAGES, 1_000)
    // The newest message is always kept, even alone over the byte budget, so
    // the answer the user is reading cannot be dropped.
    expect(bounded.messages.map((message) => message.id)).toEqual(['newest'])
    expect((bounded.messages[0]?.content as string).length).toBe(4_000)
    expect(bounded.truncated).toBe(true)

    const both = historyOf(session as never, MAX_HISTORY_MESSAGES, MAX_HISTORY_BYTES)
    expect(both.messages.map((message) => message.id)).toEqual(['old', 'newest'])
    expect(both.truncated).toBe(false)
  })

  it('reuses the same live handle for a repeated connect instead of adding a session', async () => {
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    // Five reconnects of the desktop client must not create five live handles.
    const created: Array<{ status: number; sessionId?: string }> = []
    for (let index = 0; index < 5; index += 1) {
      const reply = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, {}, `Bearer ${token}`))
      const body = JSON.parse(reply.body) as { sessionId?: string }
      created.push({ status: reply.status, sessionId: body.sessionId })
    }
    expect(new Set(created.map((entry) => entry.sessionId)).size).toBe(1)
    expect(created[0]?.status).toBe(201)
    // Reconnects are 200s, not a second create and not a spurious 409.
    expect(created.slice(1).every((entry) => entry.status === 200)).toBe(true)
    expect(harness.create).toHaveBeenCalledOnce()
  })

  it('keeps the resource ceilings in one place and refuses new sessions at the cap', async () => {
    // The in-flight cap uses the same stable 429 contract as the live cap.
    expect(MAX_LIVE_SESSIONS).toBeGreaterThan(0)
    expect(MAX_PENDING_CREATIONS).toBeGreaterThan(0)
    expect(MAX_SSE_CLIENTS_PER_SESSION).toBeGreaterThan(0)
    expect(LIVE_SESSION_SWEEP_INTERVAL_MS).toBeGreaterThan(0)
    expect(LIVE_SESSION_IDLE_TTL_MS).toBeGreaterThan(LIVE_SESSION_SWEEP_INTERVAL_MS)
    expect(MAX_HISTORY_MESSAGES).toBe(256)
    expect(MAX_HISTORY_BYTES).toBe(4 * 1024 * 1024)

    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    for (let index = 0; index < MAX_LIVE_SESSIONS; index += 1) {
      const reply = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: `sess-${index}` }, `Bearer ${token}`))
      expect(reply.status).toBe(201)
    }
    const overCap = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'sess-over' }, `Bearer ${token}`))
    expect(overCap.status).toBe(429)
    expect(JSON.parse(overCap.body)).toEqual({ error: 'too-many-live-sessions' })
    expect(harness.create).toHaveBeenCalledTimes(MAX_LIVE_SESSIONS)

    // A reconnect to an already live session still succeeds at the cap: it
    // allocates nothing, so refusing it would break the desktop client.
    const reconnect = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, { sessionId: 'sess-0' }, `Bearer ${token}`))
    expect(reconnect.status).toBe(200)
    expect(harness.create).toHaveBeenCalledTimes(MAX_LIVE_SESSIONS)
  })
})
