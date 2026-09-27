import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Context } from '@deepseek-ai/cordis'
import { API_PREFIX, BRIDGE_VERSION } from '../src/protocol.ts'
import { apply, desktopEntryPrompt, historyOf, isLoopbackWebServerHost, tokenFileForRoot, windowsTokenAclCommands, windowsTokenDirectoryAclCommands, LIVE_SESSION_IDLE_TTL_MS, LIVE_SESSION_SWEEP_INTERVAL_MS, MAX_HISTORY_BYTES, MAX_HISTORY_MESSAGES, MAX_LIVE_SESSIONS, MAX_PENDING_CREATIONS, MAX_SSE_CLIENTS_PER_SESSION } from '../src/index.ts'

/**
 * Mirrors the bridge's private constant. A file placed here makes token
 * provisioning fail the way an ACL or permission problem does.
 */
const TOKEN_DIRECTORY_NAME_FOR_TEST = 'wallpaper'

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
  logger: { warn: ReturnType<typeof vi.fn>; error: ReturnType<typeof vi.fn> }
  persistence: { enabled: boolean }
  injectedDependencies: string[] | undefined
  routeDependencies: string[] | undefined
  workspace: { title: string; path: string; sessionIds: string[]; attachSession: ReturnType<typeof vi.fn> }
  workspaceRegistry: { list: () => unknown[]; create: ReturnType<typeof vi.fn> }
  /** Run the session/control scope's disposers, as Cordis does on unprovide. */
  disposeSessionScope(): Promise<void>
  /** Re-compose the session/control scope, as Cordis does on reprovide. */
  composeSessionScope(): void
  /** Make the next `/sessions` registration throw once. */
  failNextSessionRegistration(): void
  /** Omit a host member (`service.member`) from the composed scope. */
  withholdHostMember(member: string): void
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
  options: { withhold?: string; breakTokenRoot?: boolean; deferSessionScope?: boolean } = {},
): Promise<RouteHarness> {
  const root = await mkdtemp(join(tmpdir(), 'dsh-wallpaper-bridge-'))
  const tokenRoot = join(root, 'host-owned-dsh-root')
  /**
   * Effect bodies tagged with the scope that registered them. Cordis disposes
   * one scope at a time when a providing service disappears, so a test that
   * wants to model "the session routes were unmounted" must be able to run
   * exactly that scope's disposers.
   */
  const effects: Array<{ scope: 'lifecycle' | 'status' | 'routes'; run: () => unknown }> = []
  const routes = new Map<string, (req: IncomingMessage, res: ServerResponse) => void | Promise<void>>()
  const listeners = new Map<string, (...args: never[]) => unknown>()
  /** When set, the next `/sessions` registration throws once. */
  let failNextSessionRegistration = false
  /** Host members to omit from the composed scope, as `service.member`. */
  let withheldHostMembers: string[] = options.withhold ? [options.withhold] : []
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
  const logger = { warn: vi.fn(), error: vi.fn() }
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
      if (failNextSessionRegistration && route.path === `${API_PREFIX}/sessions`) {
        failNextSessionRegistration = false
        throw new Error('simulated sessions registration failure')
      }
      routes.set(route.path, route.handler)
      return () => { routes.delete(route.path) }
    },
  }
  let injectedDependencies: string[] | undefined
  /** The mutating session scope's declared dependencies, if it was composed. */
  let routeDependencies: string[] | undefined
  /** The session scope callback, so a test can compose it on demand. */
  let composeSessionScope: (() => void) | undefined
  /**
   * Model a host whose session services arrive *after* the status route. Cordis
   * invokes an `inject` callback only once its dependencies are live, so this is
   * a real startup ordering, not a synthetic state.
   */
  let deferSessionScope = options.deferSessionScope === true
  const context = {
    on: (event: string, listener: (...args: never[]) => unknown) => {
      listeners.set(event, listener)
      return () => { listeners.delete(event) }
    },
    effect: (callback: () => unknown) => { effects.push({ scope: 'lifecycle', run: callback }) },
    get: (name: string) => name === 'sessionPersistence' && persistence.enabled ? {} : undefined,
    inject: (dependencies: string[], callback: (scope: unknown) => void) => {
      injectedDependencies = dependencies
      const kind = dependencies.includes('agents') ? 'routes' : 'status'
      if (kind === 'routes') routeDependencies = dependencies
      const scope = {
        // Spread so a test that withholds a member cannot mutate the shared
        // web-server mock and leak into the next harness.
        webServer: { ...webServer },
        agents: { create, resume: create },
        agentDefaultModel: { currentSelection: () => ({ provider: 'default-provider', model: 'default-model' }) },
        agentPresets: {
          defaultId: 'standard',
          list: async () => [{ id: 'standard', trust: 'system' as const }],
          mount: async () => undefined,
          // The host adapter validates the whole consumed surface, so a fixture
          // that omits a member the routes use is now rejected at mount time
          // (as `bridge-incompatible`) instead of failing inside a request.
          recompose: async () => ({ id: 'standard', trust: 'system' as const }),
        },
        workspaceRegistry,
        permissionPresets: {
          names: ['workspace-write', 'danger-full-access'],
          current: () => 'workspace-write',
          set: vi.fn(),
        },
        commands: { list: () => [], execute: vi.fn(async () => undefined) },
        logger,
        effect: (callback: () => unknown) => { effects.push({ scope: kind, run: callback }) },
      }
      // Model a host that is missing a member the adapter validates, so the
      // incompatibility path can be driven through the real route table.
      for (const member of withheldHostMembers) {
        const [service, property] = member.split('.')
        const target = (scope as unknown as Record<string, unknown>)[service as string]
        if (target && typeof target === 'object') {
          delete (target as Record<string, unknown>)[property as string]
        }
      }
      const compose = () => callback(scope)
      if (kind === 'routes') {
        composeSessionScope = compose
        // Hold the session scope back to model late-arriving services.
        if (deferSessionScope) return
      }
      compose()
    },
  } as unknown as Context
  // This is deliberately a host-owned root, not a token path. The bridge can
  // only touch its dedicated `wallpaper` child beneath it.
  const tokenFile = tokenFileForRoot(tokenRoot)
  if (options.breakTokenRoot) {
    // Make provisioning fail for a reason the bridge can only report: a file
    // where its token directory must go. This is how a real ACL or permission
    // failure presents itself, and it is the only way to reach the 503 paths
    // (`bridge-token-unavailable`) behaviourally instead of by reading source.
    await mkdir(tokenRoot, { recursive: true })
    await writeFile(join(tokenRoot, TOKEN_DIRECTORY_NAME_FOR_TEST), 'not a directory')
  }
  await prepareTokenRoot?.(tokenRoot)
  apply(context, { tokenRoot })
  /** Run the disposers of one scope, the way Cordis unloads a fiber. */
  const disposeScope = async (scope: 'lifecycle' | 'status' | 'routes'): Promise<void> => {
    for (const effect of effects.filter((entry) => entry.scope === scope)) {
      const disposer = effect.run()
      if (typeof disposer === 'function') await (disposer as () => unknown)()
    }
  }
  cleanups.push(async () => {
    // Token provisioning starts before the first HTTP request. Run the Cordis
    // effect disposers before removing the fixture root so whoami/icacls and
    // the idle sweep cannot still hold a handle below it.
    for (const scope of ['routes', 'status', 'lifecycle'] as const) await disposeScope(scope)
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
    disposeSessionScope: () => disposeScope('routes'),
    composeSessionScope: () => composeSessionScope?.(),
    failNextSessionRegistration: () => { failNextSessionRegistration = true },
    withholdHostMember: (member: string) => { withheldHostMembers = [member] },
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

/**
 * Drive one status request so token provisioning completes, then read the token.
 * The status route `await`s the provisioning task, which is why it is the
 * supported way to get a token without duplicating the fixture's setup.
 */
async function provisionToken(
  tokenFile: string,
  statusRoute: ((req: IncomingMessage, res: ServerResponse) => void | Promise<void>) | undefined,
): Promise<string> {
  const status = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
  expect(status.status).toBe(200)
  return (await readFile(tokenFile, 'utf8')).trim()
}

describe('wallpaper bridge HTTP routes', () => {
  it('describes the desktop entry and its default capability boundary to DSH', () => {
    const prompt = desktopEntryPrompt('C:\\workspace\\dsh-wallpaper-desktop', '桌面会话', 'workspace-write')
    // 简报是说给助手听的产品事实：工作区、权限、以及"用户看不到 Harness 界面"这个场景。
    expect(prompt).toContain('桌面会话')
    expect(prompt).toContain('C:\\workspace\\dsh-wallpaper-desktop')
    expect(prompt).toContain('workspace-write')
    expect(prompt).toContain('他看不到 Harness 的完整界面')
    // 能力边界只说真话：读放行、写只在工作区内、越界被拒时不要重试。
    expect(prompt).toContain('越界写会被沙箱直接拒绝')
    expect(prompt).toContain('不要反复重试')
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

  it('withholds session capabilities whenever the session routes are not mounted', async () => {
    // Cordis never invokes an `inject` callback partially, so the reachable
    // "loading" window is one where the session scope has been *unmounted*
    // again (a providing service disappeared or was replaced). Drive that
    // transition by running the host's own disposers, which is exactly what
    // Cordis does on unprovide.
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)

    const ready = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as {
      state: string
      capabilities: string[]
    }
    expect(ready.state).toBe('bridge-ready')
    for (const capability of ['sessions', 'history', 'sse', 'cancel', 'approval-handoff', 'control']) {
      expect(ready.capabilities).toContain(capability)
    }

    harness.disposeSessionScope()

    const loading = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as {
      state: string
      reasonCode: string
      capabilities: string[]
    }
    // The Bridge still answers and still identifies itself, so the user learns
    // it exists and is not ready — instead of seeing a missing Bridge.
    expect(loading.state).toBe('bridge-loading')
    expect(loading.reasonCode).toBe('services-pending')
    expect(loading.capabilities).toContain('status')
    for (const capability of ['sessions', 'history', 'sse', 'cancel', 'approval-handoff', 'control']) {
      expect(loading.capabilities).not.toContain(capability)
    }
    // The session endpoints really are gone, so the withheld capability is
    // truthful rather than merely conservative.
    expect(harness.routes.has(`${API_PREFIX}/sessions`)).toBe(false)
    expect(harness.routes.has(`${API_PREFIX}/control`)).toBe(false)
  })

  it('reports an incompatible host by name and revokes it when the scope unloads', async () => {
    // A host missing a member the adapter validates must be *named*, not left as
    // an unexplained failure inside the first request that touches it.
    const harness = await createHarness(true, undefined, '127.0.0.1', { withhold: 'agentPresets.recompose' })
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)

    const incompatible = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as {
      state: string
      reasonCode: string
      capabilities: string[]
    }
    expect(incompatible.state).toBe('bridge-incompatible')
    // The reason code names the exact member, because that is the only
    // actionable part: a bare `host-shape-mismatch` would let the wallpaper say
    // "incompatible" without saying which service to report.
    expect(incompatible.reasonCode).toBe('host-shape-mismatch:agentPresets.recompose')
    // Only the diagnostic surface survives; nothing claims to be driveable.
    expect(incompatible.capabilities).toEqual(['status'])
    expect(harness.routes.has(`${API_PREFIX}/sessions`)).toBe(false)
    expect(harness.routes.has(`${API_PREFIX}/control`)).toBe(false)
    // The reason is recorded for diagnosis without leaking host internals.
    expect(harness.logger.error).toHaveBeenCalled()
    expect(String((harness.logger as unknown as { error: { mock: { calls: unknown[][] } } }).error.mock.calls[0]?.[0] ?? ''))
      .toContain('agentPresets.recompose')

    // Unloading the scope must clear the incompatibility: a stale one would keep
    // reporting `bridge-incompatible` for a host that is now fine.
    await harness.disposeSessionScope()
    const after = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as { state: string }
    expect(after.state).toBe('bridge-loading')
  })

  it('never announces capabilities for a partially registered route table', async () => {    // The second registration throws. The first must be rolled back, otherwise
    // the status endpoint would advertise `control` for a route table that
    // failed to compose.
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    harness.failNextSessionRegistration()

    // Re-composing the scope is what the host does when its service set is
    // replaced; the second `/sessions` registration is the one that fails.
    expect(() => harness.composeSessionScope()).toThrow(/simulated sessions registration failure/)

    const body = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as {
      state: string
      capabilities: string[]
    }
    expect(body.state).toBe('bridge-loading')
    expect(body.capabilities).not.toContain('control')
    expect(body.capabilities).not.toContain('sessions')
    expect(harness.routes.has(`${API_PREFIX}/control`)).toBe(false)
  })

  it('keeps status public while protecting standard session operations with the generated token', async () => {
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)

    const status = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    expect(status.status).toBe(200)
    expect(JSON.parse(status.body)).toMatchObject({
      // The Bridge's own release version, taken from package.json rather than a
      // second hardcoded contract number. `protocolVersion` below is the stable
      // v1 boundary.
      bridgeVersion: BRIDGE_VERSION,
      protocolVersion: 1,
      // DSH exposes no runtime version, so this is the only version claim the
      // Bridge can prove: the API range it was compiled against.
      authoredAgainst: expect.stringMatching(/^\^?\d+\.\d+\.\d+/),
      dsh: 'online',
      authentication: 'ready',
      state: 'bridge-ready',
      reasonCode: 'ready',
      capabilities: expect.arrayContaining([
        'status',
        'control',
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

    // DSH exposes no runtime version, so the status must not claim to know one.
    // These keys existed in an earlier draft and would always have read
    // `unknown`; keeping them out prevents a compatibility field that never
    // performs a check from reappearing.
    const statusBody = JSON.parse(status.body) as Record<string, unknown>
    expect(statusBody).not.toHaveProperty('hostVersion')
    expect(statusBody).not.toHaveProperty('hostVerified')
    expect(statusBody).not.toHaveProperty('supportedDshVersions')

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

  it('lists the host model catalog for an authenticated local wallpaper client', async () => {
    // The catalog is the host's own answer. The route exists to hand it to the
    // wallpaper, which stops guessing model ids: a hardcoded list is exactly how
    // the picker ended up offering ids the host would reject.
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const controlRoute = harness.routes.get(`${API_PREFIX}/control`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const rejected = await call(controlRoute, request('GET', `${API_PREFIX}/control/models`))
    expect(rejected.status).toBe(401)
    const wrongVerb = await call(controlRoute, request('PUT', `${API_PREFIX}/control/models`, {}, `Bearer ${token}`))
    expect(wrongVerb.status).toBe(405)
    const listed = await call(controlRoute, request('GET', `${API_PREFIX}/control/models`, undefined, `Bearer ${token}`))
    expect(listed.status).toBe(200)
    const payload = JSON.parse(listed.body) as { supported: boolean; provider: string; current: { model: string }; models: Array<{ id: string }> }
    expect(payload).toMatchObject({ provider: 'default-provider', current: { model: 'default-model' } })
    // The fixture host exposes no `llm` service, so this must be the explicit
    // "cannot be asked" answer rather than a fabricated empty catalog.
    expect(payload.supported).toBe(false)
    expect(payload.models).toEqual([])
  })

  it('pushes the selected model to the host, and says so when it cannot', async () => {
    // 同步（壁纸端为源）：这条写入路由让宿主的默认模型跟随壁纸的选择，跨宿主重启生效。
    // 宿主没有持久化能力时必须回 501，而不是 200——客户端不能以为"已经同步了"。
    const harness = await createHarness()
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const controlRoute = harness.routes.get(`${API_PREFIX}/control`)
    await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const token = (await readFile(harness.tokenFile, 'utf8')).trim()

    const unauthenticated = await call(controlRoute, request('POST', `${API_PREFIX}/control/models`, { model: 'deepseek-v4-pro' }))
    expect(unauthenticated.status).toBe(401)
    const empty = await call(controlRoute, request('POST', `${API_PREFIX}/control/models`, { model: '   ' }, `Bearer ${token}`))
    expect(empty.status).toBe(400)
    expect(JSON.parse(empty.body).error).toBe('model-required')
    const unsupported = await call(controlRoute, request('POST', `${API_PREFIX}/control/models`, { model: 'deepseek-v4-pro' }, `Bearer ${token}`))
    expect(unsupported.status).toBe(501)
    expect(JSON.parse(unsupported.body).error).toBe('model-selection-unsupported')
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

  it('turns from loading to ready when the session services arrive late', async () => {
    // Plan §3 requires "status 先于完整服务" and "服务迟到后转 ready". The
    // literal first state is unreachable in this architecture (the status route
    // mounts from `webServer` alone while the session scope waits for seven
    // services), so this models the reachable ordering: the status route answers
    // first, the session scope composes later.
    const harness = await createHarness(true, undefined, '127.0.0.1', { deferSessionScope: true })
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    expect(harness.routes.has(`${API_PREFIX}/sessions`)).toBe(false)

    const loading = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as {
      state: string
      reasonCode: string
      capabilities: string[]
      authentication: string
    }
    expect(loading.state).toBe('bridge-loading')
    expect(loading.reasonCode).toBe('services-pending')
    expect(loading.capabilities).toEqual(['status'])
    // The token is provisioned even while the session services are pending, so
    // `authentication` must not be what makes the Bridge unusable here.
    expect(loading.authentication).toBe('ready')

    // The services arrive: nothing else changes.
    harness.composeSessionScope()
    const ready = JSON.parse((await call(statusRoute, request('GET', `${API_PREFIX}/status`))).body) as {
      state: string
      reasonCode: string
      capabilities: string[]
    }
    expect(ready.state).toBe('bridge-ready')
    expect(ready.reasonCode).toBe('ready')
    expect(ready.capabilities).toContain('sessions')
    // ...and the session route really exists now, so the flip is not cosmetic.
    expect(harness.routes.has(`${API_PREFIX}/sessions`)).toBe(true)
  })

  it('answers 503 for an unusable token and never exposes why', async () => {
    // Plan §3 requires "创建会话 404/409/503" to be covered behaviourally. The
    // 503 paths previously had only a source-text assertion, which proves the
    // gate exists but not that a client receives it.
    const harness = await createHarness(true, undefined, '127.0.0.1', { breakTokenRoot: true })
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const sessionsRoute = harness.routes.get(`${API_PREFIX}/sessions`)
    const controlRoute = harness.routes.get(`${API_PREFIX}/control`)

    const status = await call(statusRoute, request('GET', `${API_PREFIX}/status`))
    const statusBody = JSON.parse(status.body) as { authentication: string; state: string; reasonCode: string }
    expect(status.status).toBe(200)
    expect(statusBody.authentication).toBe('unavailable')
    expect(statusBody.state).toBe('bridge-auth-unavailable')
    expect(statusBody.reasonCode).toBe('token-unavailable')
    // The reference is a non-sensitive digest, and the status route is public.
    expect(status.body).not.toContain(TOKEN_DIRECTORY_NAME_FOR_TEST + '\\')
    expect(status.body).not.toMatch(/[A-Za-z]:\\/)

    // The session routes answer 503 rather than creating anything. No token can
    // exist here, so this also proves the token gate runs before route work.
    const created = await call(sessionsRoute, request('POST', `${API_PREFIX}/sessions`, {}))
    expect(created.status).toBe(503)
    const body = JSON.parse(created.body) as { error: string; reference?: string }
    expect(body.error).toBe('bridge-token-unavailable')
    expect(body.reference).toMatch(/^[a-f0-9]{12}$/)
    // The failure reason stays out of the response, and so does the path.
    expect(created.body).not.toMatch(/[A-Za-z]:\\/)
    expect(created.body).not.toContain('not a directory')

    const listed = await call(controlRoute, request('GET', `${API_PREFIX}/control/presets`))
    expect(listed.status).toBe(503)
    expect(JSON.parse(listed.body).error).toBe('bridge-token-unavailable')

    // The routes ARE mounted here: the difference from an incompatible host is
    // that a token problem is recoverable, so the routes stay registered and
    // refuse per request. That is also why `/status` reports
    // `bridge-auth-unavailable` rather than `bridge-loading`.
    expect(harness.routes.has(`${API_PREFIX}/sessions`)).toBe(true)

    // `capabilities` describes what is *mounted*, not what is safe to use, so it
    // may still list `sessions` here. The property that must hold is that the
    // same document cannot be read as ready: a consumer that checks
    // `authentication` first, as both the Rust and renderer interpreters do,
    // must reach `bridge-auth-unavailable` and refuse to send.
    const announced = JSON.parse(status.body) as { capabilities: string[]; authentication: string; state: string }
    expect(announced.authentication).toBe('unavailable')
    expect(announced.state).not.toBe('bridge-ready')
  })

  it('distinguishes a wrong method from a missing route across the whole interface', async () => {
    // The session scope answered 405 for a known path with the wrong verb while
    // the control scope answered 404, so the same client mistake produced two
    // different codes depending on which half of `/control` vs `/sessions` it
    // hit. Both halves must agree: 404 means "no such route", 405 means "not
    // with that verb".
    const harness = await createHarness(true)
    const statusRoute = harness.routes.get(`${API_PREFIX}/status`)
    const token = await provisionToken(harness.tokenFile, statusRoute)
    const auth = `Bearer ${token}`

    const cases: Array<[string, ReturnType<typeof request>, string, number]> = [
      // Known session routes, wrong verb -> 405.
      ['sessions collection', request('GET', `${API_PREFIX}/sessions`, undefined, auth), `${API_PREFIX}/sessions`, 405],
      // Known control routes, wrong verb -> 405.
      ['presets', request('POST', `${API_PREFIX}/control/presets`, {}, auth), `${API_PREFIX}/control`, 405],
      // Unknown paths under a registered prefix -> 404.
      ['unknown control path', request('GET', `${API_PREFIX}/control/nope`, undefined, auth), `${API_PREFIX}/control`, 404],
      ['unknown session path', request('GET', `${API_PREFIX}/sessions/abc/unknown`, undefined, auth), `${API_PREFIX}/sessions`, 404],
      // A live session's knowable routes, wrong verb -> 405 rather than 404.
      ['session preset', request('GET', `${API_PREFIX}/control/sessions/wallpaper-test/preset`, undefined, auth), `${API_PREFIX}/control`, 405],
      ['session permission', request('GET', `${API_PREFIX}/control/sessions/wallpaper-test/permission`, undefined, auth), `${API_PREFIX}/control`, 405],
    ]
    for (const [label, req, path, expected] of cases) {
      const response = await call(harness.routes.get(path), req)
      expect(response.status, `${label}: ${response.body}`).toBe(expected)
      expect(JSON.parse(response.body).error, label).toBe(expected === 405 ? 'method-not-allowed' : 'not-found')
    }
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
