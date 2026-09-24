/**
 * The single place where this Bridge touches the DSH host.
 *
 * Every host service the Bridge uses is reached through `HostAdapter` instead of
 * from the route handlers. Two problems motivated the boundary:
 *
 *  1. The route handlers used `wctx as unknown as {...}` in several places, so a
 *     host whose service shape had changed produced an unexplained `TypeError`
 *     in the middle of a request (or, worse, a half-completed session creation).
 *     Shape validation now happens once, when the adapter is built, and a
 *     mismatch becomes a named state instead of a runtime surprise.
 *  2. Version drift had no single place to live. Documented, verified
 *     differences between DSH builds belong here, next to the compatibility
 *     matrix in `bridge/README.md`, rather than being scattered through
 *     handlers.
 *
 * The local interfaces below describe only what the Bridge actually consumes.
 * They are deliberately narrower than the DSH ones: a host may add members
 * freely, and an absent member that the Bridge does not use is not a mismatch.
 */
import type { Context } from '@deepseek-ai/cordis'
import type { AgentHandle } from '@deepseek-ai/dsh-agent'
import type { SessionId, Session, SessionEvent } from '@deepseek-ai/dsh-session'

/**
 * The host does not expose a shape this Bridge can drive.
 *
 * Thrown while building the adapter (so a broken host fails closed before any
 * route can run) and never carries host internals: `code` is a stable contract
 * value and `detail` names the one service that did not match.
 */
export class HostIncompatibleError extends Error {
  constructor(readonly code: string, readonly detail: string) {
    super(`${code}: ${detail}`)
    this.name = 'HostIncompatibleError'
  }
}

export interface DefaultModelSelection {
  provider: string
  model: string
}

export interface AgentPresetDirectory {
  readonly id: string
  readonly name?: string
  readonly description?: string
  readonly trust: 'system' | 'user'
  readonly broken?: string
}

export interface DesktopWorkspace {
  readonly title: string
  readonly path: string
  readonly sessionIds: readonly SessionId[]
  attachSession(sessionId: SessionId): Promise<void>
  detachSession(sessionId: SessionId): Promise<void>
}

export interface CreateAgentInput {
  sessionId: SessionId
  cwd: string
  agentPreset: string
  provider: string
  model: string
  setup: (agentContext: Context) => Promise<void>
}

export interface ResumeAgentInput {
  resumeSessionId: SessionId
  provider: string
  model: string
  setup: (agentContext: Context) => Promise<void>
}

export interface CommandDescriptor {
  readonly name: string
  readonly description: string
  readonly input?: { readonly hint: string }
}

/** Host events the Bridge subscribes to. */
export type HostEventName = 'session/event' | 'agent/error' | 'agent/disposed' | 'approval/request'

/**
 * The verified operation set. Each member is one thing the Bridge needs, named
 * after the intent rather than after the DSH service that happens to implement
 * it, so a future host can satisfy the same intent differently.
 */
export interface HostAdapter {
  /** DSH release this adapter was validated against, for `/status`. */
  readonly hostVersion: string
  defaultModel(): DefaultModelSelection
  presetDirectory(): Promise<readonly AgentPresetDirectory[]>
  defaultPresetId(): string
  mountPreset(agentContext: Context, preset: string): Promise<void>
  recomposePreset(agentContext: Context, preset: string): Promise<AgentPresetDirectory>
  workspaces(): readonly DesktopWorkspace[]
  createWorkspace(path: string, title?: string): Promise<DesktopWorkspace>
  permissionNames(): readonly string[]
  currentPermission(events: readonly SessionEvent[]): string
  setPermission(session: Session, name: string): void
  commandsFor(agent: { id: unknown }): readonly CommandDescriptor[]
  executeCommand(agent: { id: unknown }, text: string, signal: AbortSignal): Promise<unknown>
  createAgent(input: CreateAgentInput): Promise<AgentHandle>
  resumeAgent(input: ResumeAgentInput): Promise<AgentHandle>
  on(name: HostEventName, listener: (...args: never[]) => unknown): () => void
}

/** Minimal structural guards. `undefined` means "not the shape we need". */
function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' ? value as Record<string, unknown> : undefined
}

function hasFunction(value: unknown, ...path: string[]): boolean {
  let current: unknown = value
  for (const key of path) {
    const record = asRecord(current)
    if (!record) return false
    current = record[key]
  }
  return typeof current === 'function'
}

/**
 * Services the Bridge requires, in the order the status diagnostic reports them.
 * Kept next to the adapter so the injected list and the validated list cannot
 * disagree.
 */
export const HOST_ADAPTER_SERVICES = [
  'agentDefaultModel',
  'agentPresets',
  'agents',
  'webServer',
  'workspaceRegistry',
  'permissionPresets',
  'commands',
] as const

/**
 * Verified DSH builds. `rc.5` is the only one proven end to end against a real
 * profile; the peers in `package.json` must not claim more than this list, and
 * a host outside it is still attempted but reported honestly.
 */
export const VERIFIED_HOST_VERSIONS = ['0.1.0-rc.5'] as const

function requireShape(condition: boolean, detail: string): void {
  if (!condition) throw new HostIncompatibleError('host-shape-mismatch', detail)
}

/**
 * Build the adapter for one composed Cordis scope.
 *
 * Every required member is checked here, so a host that is missing one fails at
 * mount time with a named reason instead of throwing inside a request. The
 * checks are intentionally about *shape*, not identity: a host may reimplement
 * these services as long as the members the Bridge calls behave as declared.
 */
export function createHostAdapter(ctx: Context): HostAdapter {
  const scope = ctx as unknown as Record<string, unknown>
  requireShape(hasFunction(scope.agentDefaultModel, 'currentSelection'), 'agentDefaultModel.currentSelection')
  requireShape(hasFunction(scope.agentPresets, 'list'), 'agentPresets.list')
  requireShape(hasFunction(scope.agentPresets, 'mount'), 'agentPresets.mount')
  requireShape(hasFunction(scope.agents, 'create'), 'agents.create')
  requireShape(hasFunction(scope.agents, 'resume'), 'agents.resume')
  requireShape(hasFunction(scope.workspaceRegistry, 'list'), 'workspaceRegistry.list')
  requireShape(hasFunction(scope.workspaceRegistry, 'create'), 'workspaceRegistry.create')
  requireShape(hasFunction(scope.permissionPresets, 'set'), 'permissionPresets.set')
  requireShape(hasFunction(scope.commands, 'list'), 'commands.list')
  requireShape(hasFunction(scope.commands, 'execute'), 'commands.execute')
  // The reachability policy is read from the host rather than assumed, so a
  // host that stops exposing `host` fails here instead of silently registering
  // routes on an unverified address.
  requireShape(typeof asRecord(scope.webServer)?.host === 'string', 'webServer.host')
  requireShape(hasFunction(scope.webServer, 'register'), 'webServer.register')

  const agentDefaultModel = scope.agentDefaultModel as {
    currentSelection(): DefaultModelSelection
  }
  const agentPresets = scope.agentPresets as {
    readonly defaultId?: string
    list(): Promise<readonly AgentPresetDirectory[]>
    mount(agentContext: Context, preset?: string): Promise<unknown>
    recompose(agentContext: Context, preset: string): Promise<AgentPresetDirectory>
  }
  const agents = scope.agents as {
    create(options: Record<string, unknown>): Promise<AgentHandle>
    resume(options: Record<string, unknown>): Promise<AgentHandle>
  }
  const workspaceRegistry = scope.workspaceRegistry as {
    list(): readonly DesktopWorkspace[]
    create(path: string, title?: string): Promise<DesktopWorkspace>
  }
  const permissionPresets = scope.permissionPresets as {
    readonly names: readonly string[]
    current(events: readonly SessionEvent[]): string
    set(session: Session, name: string): void
  }
  const commands = scope.commands as {
    list(agent: { id: unknown }): readonly CommandDescriptor[]
    execute(agent: { id: unknown }, text: string, signal: AbortSignal): Promise<unknown>
  }

  return {
    hostVersion: typeof process !== 'undefined' ? process.env?.DSH_VERSION ?? 'unknown' : 'unknown',

    defaultModel: () => {
      const selection = agentDefaultModel.currentSelection()
      const record = asRecord(selection)
      // A host that returns a selection without usable strings would otherwise
      // create a session with the literal text `undefined` in its persona.
      requireShape(
        typeof record?.provider === 'string' && typeof record?.model === 'string'
          && record.provider.length > 0 && record.model.length > 0,
        'agentDefaultModel.currentSelection() result',
      )
      return { provider: selection.provider, model: selection.model }
    },

    presetDirectory: () => agentPresets.list(),

    defaultPresetId: () => {
      // `defaultId` is a getter that reads live settings, so it is read on every
      // call rather than captured at mount. A host that exposes it as a plain
      // property keeps working; one that omits it fails here, not later.
      const id = asRecord(agentPresets)?.defaultId
      requireShape(typeof id === 'string' && id.length > 0, 'agentPresets.defaultId')
      return id as string
    },

    mountPreset: async (agentContext, preset) => {
      await agentPresets.mount(agentContext, preset)
    },

    recomposePreset: (agentContext, preset) => agentPresets.recompose(agentContext, preset),

    workspaces: () => workspaceRegistry.list(),

    createWorkspace: (path, title) => workspaceRegistry.create(path, title),

    permissionNames: () => {
      const names = permissionPresets.names
      return Array.isArray(names) ? names : []
    },

    // DSH's `current()` takes the session's *events*, not the session. Passing a
    // Session object used to be a documented shape mismatch that only avoided
    // failing because the call site cast it; routing it here removes the cast.
    currentPermission: (events) => permissionPresets.current(events),

    setPermission: (session, name) => {
      permissionPresets.set(session, name)
    },

    commandsFor: (agent) => {
      const listed = commands.list(agent)
      return Array.isArray(listed) ? listed : []
    },

    executeCommand: (agent, text, signal) => commands.execute(agent, text, signal),

    createAgent: (input) => agents.create({
      sessionId: input.sessionId,
      // The workspace is host policy, never HTTP input: the wallpaper never
      // needs to choose a filesystem boundary, and `cwd` here comes from the
      // registered workspace rather than from a request body.
      meta: { cwd: input.cwd, agentPreset: input.agentPreset },
      agentOptions: { provider: input.provider, model: input.model },
      setup: input.setup,
    }),

    resumeAgent: (input) => agents.resume({
      resumeSessionId: input.resumeSessionId,
      agentOptions: { provider: input.provider, model: input.model },
      setup: input.setup,
    }),

    /**
     * Event subscription for a *service-shaped* scope. `apply()` itself
     * subscribes through `ctx.on` directly, because it runs before any scope
     * exists: subscribing there is what lets the Bridge observe a session that
     * a host creates outside this plugin's routes.
     */
    on: (name, listener) => {
      const subscribe = asRecord(scope)?.on
      if (typeof subscribe !== 'function') {
        throw new HostIncompatibleError('host-shape-mismatch', 'on')
      }
      return (subscribe as (name: HostEventName, listener: unknown) => () => void).call(scope, name, listener)
    },
  }
}
