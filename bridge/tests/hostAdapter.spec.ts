import { describe, expect, it, vi } from 'vitest'
import { createHostAdapter, HostIncompatibleError, HOST_ADAPTER_SERVICES } from '../src/host.ts'

/**
 * The adapter is the one place the Bridge asserts what a DSH host looks like, so
 * these tests pin both halves of its contract: it must accept a host that
 * provides the consumed surface, and it must reject a host that does not with a
 * named reason instead of a `TypeError` mid-request.
 */

/** A host scope exposing exactly the surface the adapter requires. */
function validScope(): Record<string, unknown> {
  return {
    on: vi.fn(() => () => undefined),
    logger: { warn: vi.fn(), error: vi.fn() },
    // The adapter reads the loopback policy from the host, so the fixture must
    // expose it: `webServer` is part of the validated surface.
    webServer: { host: '127.0.0.1', register: vi.fn(() => () => undefined) },
    agentDefaultModel: { currentSelection: () => ({ provider: 'deepseek-official', model: 'deepseek-flash' }) },
    agents: { create: vi.fn(), resume: vi.fn() },
    agentPresets: {
      defaultId: 'standard',
      list: async () => [{ id: 'standard', trust: 'system' as const }],
      mount: async () => undefined,
      recompose: async () => ({ id: 'standard', trust: 'system' as const }),
    },
    workspaceRegistry: { list: () => [], create: vi.fn() },
    permissionPresets: { names: ['workspace-write'], current: () => 'workspace-write', set: vi.fn() },
    commands: { list: () => [], execute: vi.fn() },
  }
}

const asContext = (scope: Record<string, unknown>) => scope as never

describe('host adapter acceptance', () => {
  it('accepts a host that provides the consumed surface', () => {
    const adapter = createHostAdapter(asContext(validScope()))
    expect(adapter.defaultModel()).toEqual({ provider: 'deepseek-official', model: 'deepseek-flash' })
    expect(adapter.defaultPresetId()).toBe('standard')
    expect(adapter.permissionNames()).toEqual(['workspace-write'])
  })

  it('reads defaultId live rather than caching it at mount', () => {
    // DSH's `defaultId` is a getter over live settings; caching it would make a
    // user's preset change invisible until the plugin remounted.
    const scope = validScope()
    const adapter = createHostAdapter(asContext(scope))
    expect(adapter.defaultPresetId()).toBe('standard')
    ;(scope.agentPresets as { defaultId: string }).defaultId = 'careful'
    expect(adapter.defaultPresetId()).toBe('careful')
  })

  it('passes permission events through, not the Session object', () => {
    // DSH derives this value from the durable event stream. The old call site
    // cast a Session into an events-shaped hole; the adapter is where that
    // mismatch is settled.
    const set = vi.fn()
    const scope = validScope()
    scope.permissionPresets = { names: ['workspace-write'], current: vi.fn(() => 'read-only'), set }
    const adapter = createHostAdapter(asContext(scope))
    const events = [{ type: 'permission/preset', seq: 1, time: 0, data: { preset: 'read-only' } }]
    expect(adapter.currentPermission(events as never)).toBe('read-only')
    expect((scope.permissionPresets as { current: ReturnType<typeof vi.fn> }).current).toHaveBeenCalledWith(events)
  })

  it('keeps the session workspace a host policy choice', () => {
    // `cwd` reaches `agents.create` from the registered workspace, never from a
    // request body; a local caller must not choose the filesystem boundary.
    const create = vi.fn(async () => ({ agent: {}, dispose: async () => undefined }))
    const scope = validScope()
    scope.agents = { create, resume: vi.fn() }
    const adapter = createHostAdapter(asContext(scope))
    const setup = async () => undefined
    void adapter.createAgent({
      sessionId: 'wallpaper-test' as never,
      cwd: 'C:\\host\\workspace',
      agentPreset: 'standard',
      provider: 'p',
      model: 'm',
      setup,
    })
    expect(create).toHaveBeenCalledWith({
      sessionId: 'wallpaper-test',
      meta: { cwd: 'C:\\host\\workspace', agentPreset: 'standard' },
      agentOptions: { provider: 'p', model: 'm' },
      setup,
    })
  })
})

describe('host adapter rejection', () => {
  // Every member the adapters validate, so a new requirement cannot be added
  // without also giving it a rejection case here.
  const requiredMembers: Array<[string, string]> = [
    ['agentDefaultModel.currentSelection', 'agentDefaultModel'],
    ['agentPresets.list', 'agentPresets'],
    ['agentPresets.mount', 'agentPresets'],
    ['agents.create', 'agents'],
    ['agents.resume', 'agents'],
    ['workspaceRegistry.list', 'workspaceRegistry'],
    ['workspaceRegistry.create', 'workspaceRegistry'],
    ['permissionPresets.set', 'permissionPresets'],
    ['commands.list', 'commands'],
    ['commands.execute', 'commands'],
    ['webServer.register', 'webServer'],
  ]

  it.each(requiredMembers)('rejects a host missing %s', (detail) => {
    // Remove exactly the one member under test: a service-level deletion would
    // only ever surface that service's first failed check, which would hide
    // every later member in the same service.
    const scope = validScope()
    const [service, member] = detail.split('.')
    delete (scope[service as string] as Record<string, unknown>)[member as string]
    let thrown: unknown
    try {
      createHostAdapter(asContext(scope))
    } catch (error) {
      thrown = error
    }
    expect(thrown).toBeInstanceOf(HostIncompatibleError)
    expect((thrown as HostIncompatibleError).code).toBe('host-shape-mismatch')
    // The message names the *member* that would have thrown later, not just the
    // service, so a support report is actionable.
    expect(String((thrown as HostIncompatibleError).message)).toContain(detail)
  })

  it('rejects a host that does not compose a required service at all', () => {
    // The realistic failure: the service is absent, not merely incomplete.
    for (const service of HOST_ADAPTER_SERVICES) {
      const scope = validScope()
      delete scope[service]
      expect(() => createHostAdapter(asContext(scope)), service).toThrow(HostIncompatibleError)
    }
  })

  it('rejects a default-model selection without usable strings', () => {
    // A host returning `{}` would otherwise create a session whose persona
    // renders the literal text `undefined`.
    const scope = validScope()
    scope.agentDefaultModel = { currentSelection: () => ({}) }
    const adapter = createHostAdapter(asContext(scope))
    expect(() => adapter.defaultModel()).toThrow(HostIncompatibleError)
    expect(() => adapter.defaultModel()).toThrow(/currentSelection\(\) result/)
  })

  it('rejects a missing default preset id when it is asked for', () => {
    const scope = validScope()
    delete (scope.agentPresets as { defaultId?: string }).defaultId
    const adapter = createHostAdapter(asContext(scope))
    expect(() => adapter.defaultPresetId()).toThrow(/agentPresets\.defaultId/)
  })

  it('never carries host internals in the error', () => {
    const scope = validScope()
    delete scope.commands
    try {
      createHostAdapter(asContext(scope))
      expect.unreachable('expected a rejection')
    } catch (error) {
      const message = String((error as Error).message)
      expect(message).not.toMatch(/[A-Za-z]:\\/)
      expect(message).not.toMatch(/Bearer|token/i)
    }
  })
})

describe('single source of truth for the injected services', () => {
  it('lists every service the adapter validates', () => {
    // `inject` in index.ts and the validation above must not drift: a service
    // the adapter reads but the plugin does not require would be undefined at
    // runtime on a minimal host.
    expect([...HOST_ADAPTER_SERVICES]).toEqual([
      'agentDefaultModel', 'agentPresets', 'agents', 'webServer',
      'workspaceRegistry', 'permissionPresets', 'commands',
    ])
  })

  it('is the list the plugin actually injects', async () => {
    const index = await import('../src/index.ts')
    expect([...index.inject]).toEqual([...HOST_ADAPTER_SERVICES])
  })
})

describe('version reporting stays factual', () => {
  it('reads the reported release version from package.json', async () => {
    const manifest = (await import('../package.json', { with: { type: 'json' } })).default as { version: string }
    const protocol = await import('../src/protocol.ts')
    // A second hardcoded version is exactly how the status endpoint ended up
    // advertising `1.1.0` for a `0.1.1` package.
    expect(protocol.BRIDGE_VERSION).toBe(manifest.version)
  })

  it('reports the compiled-against range instead of an unprovable host version', async () => {
    const manifest = (await import('../package.json', { with: { type: 'json' } })).default as {
      peerDependencies: Record<string, string>
    }
    const protocol = await import('../src/protocol.ts')
    expect(protocol.BRIDGE_AUTHORED_AGAINST).toBe(manifest.peerDependencies['@deepseek-ai/dsh-agent'])
  })

  it('never reads a host version that DSH does not expose', async () => {
    // DSH sets no `DSH_VERSION`, so reading one would silently yield `unknown`
    // and ship a compatibility field that never performs a check.
    const fs = await import('node:fs/promises')
    for (const relative of ['../src/protocol.ts', '../src/host.ts', '../src/index.ts']) {
      const text = await fs.readFile(new URL(relative, import.meta.url), 'utf8')
      expect(text, relative).not.toMatch(/process\.env[^\n]*DSH_VERSION/)
      expect(text, relative).not.toMatch(/env\.DSH_VERSION/)
    }
  })
})

/**
 * The model catalog has three possible shapes in the wild, and the picker's
 * behaviour depends on telling them apart: the host enumerates its catalog, the
 * host exposes only the current selection, or the host cannot be asked at all.
 * `supported` is what carries that distinction — an empty list must never be
 * read as "nothing is available".
 */
describe('model catalog discovery', () => {
  const withLlm = (listModels: () => Promise<unknown>) => {
    const scope = validScope()
    scope.get = (name: string) => (name === 'llm' ? { listModels } : undefined)
    return scope
  }

  it('reports unsupported (not empty) when the host has no llm service', async () => {
    // The stock fixture has no `get`, i.e. a DSH build that predates the seam.
    const adapter = createHostAdapter(asContext(validScope()))
    expect(await adapter.modelDirectory()).toEqual({
      supported: false,
      provider: 'deepseek-official',
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [],
    })
  })

  it('enumerates the host catalog with the id and the display name', async () => {
    const adapter = createHostAdapter(asContext(withLlm(async () => [
      { provider: 'deepseek-official', id: 'deepseek-flash', name: 'DeepSeek-Flash' },
      { provider: 'deepseek-official', id: 'deepseek-v4-pro', name: 'DeepSeek-V4-Pro', description: 'Pro' },
    ])))
    expect(await adapter.modelDirectory()).toEqual({
      supported: true,
      provider: 'deepseek-official',
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [
        { id: 'deepseek-flash', name: 'DeepSeek-Flash' },
        { id: 'deepseek-v4-pro', name: 'DeepSeek-V4-Pro', description: 'Pro' },
      ],
    })
  })

  it('drops entries the picker could never submit, and falls back to the id as a name', async () => {
    const adapter = createHostAdapter(asContext(withLlm(async () => [
      { provider: 'deepseek-official', id: 'deepseek-flash' },
      { provider: 'deepseek-official', name: 'No id' },
      { provider: 'deepseek-official', id: '', name: 'Empty id' },
      'not-an-object',
    ])))
    const directory = await adapter.modelDirectory()
    // An option without an id would be rendered but rejected by the host later.
    expect(directory.models).toEqual([{ id: 'deepseek-flash', name: 'deepseek-flash' }])
  })

  it('degrades to unsupported when the host refuses to enumerate', async () => {
    // The LLM seam throws `INVALID_CATALOG` for an unregistered route; a picker
    // must fall back to "current model only" instead of surfacing that error.
    const adapter = createHostAdapter(asContext(withLlm(async () => { throw new Error('INVALID_CATALOG') })))
    expect(await adapter.modelDirectory()).toEqual({
      supported: false,
      provider: 'deepseek-official',
      current: { provider: 'deepseek-official', model: 'deepseek-flash' },
      models: [],
    })
  })

  it('still mounts when the host exposes an llm service without listModels', async () => {
    const scope = validScope()
    scope.get = (name: string) => (name === 'llm' ? {} : undefined)
    const adapter = createHostAdapter(asContext(scope))
    expect((await adapter.modelDirectory()).supported).toBe(false)
  })
})

/**
 * 同步的方向是"壁纸端为源、宿主跟随"：壁纸选了什么，宿主的默认模型就变成什么，
 * 这样从 DSH 界面开的新会话也用同一个模型。宿主没有 setter 时要**明说做不到**，
 * 而不是假装成功——壁纸自己那次会话的模型仍然生效，所以这是能力缺口，不是失败。
 */
describe('pushing the model choice to the host', () => {
  const withSave = () => {
    const saveSelection = vi.fn(async () => undefined)
    const scope = validScope()
    scope.agentDefaultModel = {
      currentSelection: () => ({ provider: 'deepseek-account', model: 'deepseek-flash' }),
      saveSelection,
    }
    return { scope, saveSelection }
  }

  it('writes the selection through the host so the host persists it', async () => {
    const { scope, saveSelection } = withSave()
    const adapter = createHostAdapter(asContext(scope))
    expect(await adapter.setDefaultModel({ provider: 'deepseek-account', model: 'deepseek-v4-pro' }, ['deepseek-flash', 'deepseek-v4-pro'])).toBe(true)
    expect(saveSelection).toHaveBeenCalledWith({ provider: 'deepseek-account', model: 'deepseek-v4-pro' })
  })

  it('refuses a model the host does not offer, without touching the host', async () => {
    // 接受一个宿主目录里没有的 id，会把宿主的默认模型推进它自己的选择器都退不出来的状态。
    const { scope, saveSelection } = withSave()
    const adapter = createHostAdapter(asContext(scope))
    expect(await adapter.setDefaultModel({ provider: 'deepseek-account', model: 'deepseek-v5' }, ['deepseek-flash'])).toBe(false)
    expect(saveSelection).not.toHaveBeenCalled()
  })

  it('reports that it cannot synchronise when the host has no setter', async () => {
    const adapter = createHostAdapter(asContext(validScope()))
    expect(await adapter.setDefaultModel({ provider: 'deepseek-account', model: 'deepseek-flash' }, [])).toBe(false)
  })

  it('accepts any id when the host cannot enumerate (nothing to validate against)', async () => {
    const { scope, saveSelection } = withSave()
    const adapter = createHostAdapter(asContext(scope))
    expect(await adapter.setDefaultModel({ provider: 'deepseek-account', model: 'anything' }, [])).toBe(true)
    expect(saveSelection).toHaveBeenCalledOnce()
  })
})
