import { mkdtemp, rm, stat, writeFile } from 'node:fs/promises'
import type { FileHandle } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import type { Context } from '@deepseek-ai/cordis'
import { afterEach, describe, expect, it, vi } from 'vitest'

/**
 * `node:child_process` and `node:fs/promises` are ESM builtins whose exports
 * cannot be spied on per-test, so these hoisted factories intercept the calls
 * token provisioning makes and let a test hold an external process open. That
 * open process is the whole point: while `icacls` runs, it holds a handle on
 * the token directory, and a caller that removes that directory as soon as the
 * plugin is disposed used to get EBUSY.
 */
const control = vi.hoisted(() => ({
  /** External ACL processes currently open. */
  aclActive: 0,
  /** Highest `aclActive` observed; proves the race window was reached. */
  aclPeak: 0,
  /** Every external command the bridge ran, in order. */
  commands: [] as string[],
  /** When set, matching commands wait for this before calling back. */
  gate: undefined as Promise<void> | undefined,
  /** When set, only this command is gated; every other one answers at once. */
  gateOnly: undefined as string | undefined,
  /** When set, `writeFile` waits for this before writing. */
  writeGate: undefined as Promise<void> | undefined,
  writeStarted: undefined as (() => void) | undefined,
  /** When set, every external command fails, so provisioning fails closed. */
  failCommands: false,
}))

vi.mock('node:child_process', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:child_process')>()
  const mockExecFile = (file: string, args: readonly string[], options: unknown, callback?: unknown) => {
    control.commands.push(file)
    if (typeof callback !== 'function') {
      return (actual.execFile as unknown as (...a: unknown[]) => unknown)(file, args, options, callback)
    }
    const done = callback as (error: unknown, stdout: string, stderr: string) => void
    const isAcl = file === 'icacls.exe'
    if (isAcl) {
      control.aclActive += 1
      control.aclPeak = Math.max(control.aclPeak, control.aclActive)
    }
    const finish = (error: unknown, stdout: string, stderr: string) => {
      if (isAcl) control.aclActive -= 1
      done(error, stdout, stderr)
    }
    // Answer directly instead of spawning the real tool: this test is about the
    // bridge's lifecycle, not about `whoami` or `icacls` output.
    const stdout = file === 'whoami.exe' ? '"user","S-1-5-21-1-2-3-4"\n' : ''
    if (control.failCommands) {
      queueMicrotask(() => finish(new Error(`simulated ${file} failure`), '', ''))
      return undefined
    }
    if (control.gate && (control.gateOnly === undefined || control.gateOnly === file)) {
      void control.gate.then(() => finish(null, stdout, ''))
      return undefined
    }
    queueMicrotask(() => finish(null, stdout, ''))
    return undefined
  }
  // `promisify()` prefers `util.promisify.custom` when the target defines it,
  // and the real `execFile` does. Without copying that symbol onto the mock,
  // `promisify` calls this wrapper with `this === undefined`, the call throws,
  // and the provisioning chain dies silently before its first callback — which
  // would make every test here pass for the wrong reason.
  Object.defineProperty(mockExecFile, promisify.custom, {
    value: (file: string, args: readonly string[], options: unknown) => new Promise((resolve, reject) => {
      mockExecFile(file, args, options, (error: unknown, stdout: string, stderr: string) => {
        if (error) reject(error)
        else resolve({ stdout, stderr })
      })
    }),
  })
  return { ...actual, execFile: mockExecFile }
})

vi.mock('node:fs/promises', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:fs/promises')>()
  return {
    ...actual,
    // The token write goes through `open()` + `FileHandle.writeFile()`, and
    // `FileHandle` resolves that method internally rather than through this
    // module. Intercepting the handle is the only way to hold the write open.
    open: async (...args: unknown[]) => {
      const handle = await (actual.open as unknown as (...a: unknown[]) => Promise<FileHandle>)(...args)
      const realWriteFile = handle.writeFile.bind(handle)
      handle.writeFile = (async (data: unknown, options?: unknown) => {
        if (control.writeGate) {
          control.writeStarted?.()
          await control.writeGate
        }
        return realWriteFile(data as never, options as never)
      }) as typeof handle.writeFile
      return handle
    },
  }
})

const { apply, AsyncWorkTracker, tokenFileForRoot } = await import('../src/index.ts')

const cleanups: Array<() => Promise<void>> = []
afterEach(async () => {
  control.aclActive = 0
  control.aclPeak = 0
  control.commands = []
  control.gate = undefined
  control.gateOnly = undefined
  control.writeGate = undefined
  control.writeStarted = undefined
  control.failCommands = false
  await Promise.allSettled(cleanups.splice(0).map(async (cleanup) => { await cleanup() }))
})

function deferred() {
  let resolve: (() => void) | undefined
  const promise = new Promise<void>((settle) => { resolve = settle })
  return { promise, resolve: () => resolve?.() }
}

/** Let pending microtasks run. */
async function settleMicrotasks(): Promise<void> {
  for (let index = 0; index < 5; index += 1) await Promise.resolve()
}

describe('AsyncWorkTracker', () => {
  it('waits for every tracked task before settling', async () => {
    const tracker = new AsyncWorkTracker()
    const first = deferred()
    const second = deferred()
    tracker.track(first.promise)
    tracker.track(second.promise)

    let settled = false
    const settle = tracker.settle().then(() => { settled = true })

    first.resolve()
    await settleMicrotasks()
    // One task is still running, so teardown must not have completed.
    expect(settled).toBe(false)

    second.resolve()
    await settle
    expect(settled).toBe(true)
    expect(tracker.size).toBe(0)
  })

  it('refuses new work once draining, so teardown cannot be outrun', async () => {
    const tracker = new AsyncWorkTracker()
    expect(tracker.isDraining).toBe(false)
    const settle = tracker.settle()
    expect(tracker.isDraining).toBe(true)
    expect(tracker.track(Promise.resolve())).toBe(false)
    expect(tracker.run(async () => undefined)).toBe(false)
    await settle
  })

  it('is idempotent: repeated settle calls share one drain', async () => {
    const tracker = new AsyncWorkTracker()
    const work = deferred()
    tracker.track(work.promise)
    const first = tracker.settle()
    const second = tracker.settle()
    expect(second).toBe(first)
    work.resolve()
    await Promise.all([first, second])
  })

  it('awaits a follow-up task registered by a task that is still running', async () => {
    const tracker = new AsyncWorkTracker()
    const inner = deferred()
    tracker.track((async () => {
      // A task may register more work before it resolves; that work must also
      // be awaited or teardown could return while filesystem work continues.
      expect(tracker.track(inner.promise)).toBe(true)
    })())
    let settled = false
    const settle = tracker.settle().then(() => { settled = true })
    await settleMicrotasks()
    expect(settled).toBe(false)
    inner.resolve()
    await settle
    expect(settled).toBe(true)
  })

  it('absorbs a failure instead of turning it into an unhandled rejection', async () => {
    const tracker = new AsyncWorkTracker()
    const onError = vi.fn()
    tracker.onError = onError
    tracker.track(Promise.reject(new Error('provisioning failed')))
    await tracker.settle()
    expect(onError).toHaveBeenCalledTimes(1)
    expect(String(onError.mock.calls[0]?.[0])).toContain('provisioning failed')
  })
})

interface LifecycleHarness {
  root: string
  tokenRoot: string
  teardown(): Promise<void>
  effectCount(): number
}

/**
 * A Cordis context that *keeps* the effect disposers. The route fixture passes
 * `effect: () => undefined`, which is exactly why a teardown race against
 * `ensureToken()` could never fail in a test before.
 */
async function createLifecycleHarness(): Promise<LifecycleHarness> {
  const root = await mkdtemp(join(tmpdir(), 'dsh-wallpaper-bridge-lifecycle-'))
  cleanups.push(async () => { await rm(root, { recursive: true, force: true }) })
  const tokenRoot = join(root, 'host-owned-dsh-root')
  const effects: Array<() => unknown> = []
  const scope = {
    webServer: { host: '127.0.0.1', register: () => () => undefined },
    agents: { create: vi.fn(), resume: vi.fn() },
    agentDefaultModel: { currentSelection: () => ({ provider: 'p', model: 'm' }) },
    agentPresets: { defaultId: 'standard', list: async () => [], mount: async () => undefined },
    workspaceRegistry: { list: () => [], create: vi.fn() },
    permissionPresets: { names: ['workspace-write'], current: () => 'workspace-write', set: vi.fn() },
    commands: { list: () => [], execute: vi.fn() },
    logger: { warn: vi.fn(), error: vi.fn() },
    effect: (callback: () => unknown) => { effects.push(callback) },
  }
  const context = {
    on: () => () => undefined,
    get: () => undefined,
    effect: (callback: () => unknown) => { effects.push(callback) },
    inject: (_dependencies: string[], callback: (inner: unknown) => void) => callback(scope),
  } as unknown as Context
  apply(context, { tokenRoot })
  return {
    root,
    tokenRoot,
    effectCount: () => effects.length,
    async teardown() {
      // Every disposer runs, oldest first: the plugin lifecycle's own effect
      // (which owns bootstrap settling) and the web-server scope's effects.
      for (const effect of effects) {
        const disposer = effect()
        if (typeof disposer === 'function') await (disposer as () => unknown)()
      }
    },
  }
}

describe('bridge teardown versus token provisioning', () => {
  it('waits for an open external ACL process before completing teardown', async () => {
    const gate = deferred()
    control.gate = gate.promise
    control.gateOnly = 'icacls.exe'

    const harness = await createLifecycleHarness()
    const aclReached = (async () => {
      for (let attempt = 0; attempt < 200 && control.aclPeak === 0; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 5))
      }
    })()
    await Promise.race([aclReached, new Promise((resolve) => setTimeout(resolve, 1_000))])
    // Provisioning must genuinely have reached the ACL pass, otherwise this
    // test would pass without exercising the race it exists to cover.
    expect(control.commands).toContain('icacls.exe')
    expect(control.aclActive).toBeGreaterThan(0)

    const teardown = harness.teardown()
    let finished = false
    void teardown.then(() => { finished = true })
    await settleMicrotasks()
    // Teardown must still be waiting: an external ACL process is holding the
    // token directory, which is the handle a caller's directory removal raced.
    expect(finished).toBe(false)

    gate.resolve()
    await teardown

    // Teardown returned only after every external process had exited, so a
    // caller deleting the whole root now cannot collide with a live handle.
    expect(control.aclActive).toBe(0)
    await expect(stat(harness.root)).resolves.toBeDefined()
    await expect(rm(harness.root, { recursive: true, force: true })).resolves.toBeUndefined()
    await expect(stat(tokenFileForRoot(harness.tokenRoot))).rejects.toMatchObject({ code: 'ENOENT' })
  })

  it('waits for an in-flight token write, not just the ACL pass', async () => {
    const gate = deferred()
    let writeStarted: (() => void) | undefined
    const started = new Promise<void>((resolve) => { writeStarted = resolve })
    control.writeGate = gate.promise
    control.writeStarted = writeStarted

    const harness = await createLifecycleHarness()
    await Promise.race([started, new Promise((resolve) => setTimeout(resolve, 1_000))])
    // The gate must genuinely have been reached: without this the test would
    // pass by never intercepting the write at all.
    expect(control.commands).toContain('icacls.exe')
    expect(control.writeStarted).toBeTypeOf('function')

    const teardown = harness.teardown()
    let finished = false
    void teardown.then(() => { finished = true })
    await settleMicrotasks()
    expect(finished).toBe(false)

    gate.resolve()
    await teardown
    // The token was written before teardown completed, then survives it: this
    // process never deletes the host's credential.
    await expect(stat(tokenFileForRoot(harness.tokenRoot))).resolves.toBeDefined()
  })

  it('completes teardown after provisioning failed closed', async () => {
    // Every external command fails, so `ensureToken` fails closed. Teardown
    // must still settle: a failed bootstrap is not a reason to hang.
    control.failCommands = true
    const harness = await createLifecycleHarness()
    await expect(harness.teardown()).resolves.toBeUndefined()
    expect(control.commands).toContain('whoami.exe')
    // Nothing was left open, so the caller can remove the root immediately.
    await expect(rm(harness.root, { recursive: true, force: true })).resolves.toBeUndefined()
  })

  it('is idempotent when teardown runs twice', async () => {
    const harness = await createLifecycleHarness()
    await harness.teardown()
    await expect(harness.teardown()).resolves.toBeUndefined()
  })
})

describe('bridge request gate and teardown ordering', () => {
  it('refuses new session work once the bridge is shutting down', async () => {
    const source = await (await import('node:fs/promises')).readFile(new URL('../src/index.ts', import.meta.url), 'utf8')
    // The gate must exist and be checked after authentication, so shutdown
    // never becomes an unauthenticated oracle.
    const gateIndex = source.indexOf("if (stopping) return json(res, 503, { error: 'bridge-shutting-down' })")
    const authIndex = source.indexOf('if (!bearerAuthorized(req.headers.authorization, token))')
    expect(gateIndex).toBeGreaterThan(-1)
    expect(authIndex).toBeGreaterThan(-1)
    expect(gateIndex).toBeGreaterThan(authIndex)
    // ...and it must precede everything that could touch a session. Checking
    // position is what makes this a gate rather than a late failure: after the
    // gate the handler parses the route, reads the body, and can create or
    // resume an agent, all of which are wrong once teardown has begun. The
    // search is anchored to the `/sessions` registration, because the control
    // scope also parses bodies and an unanchored match would find that one.
    const sessionsStart = source.indexOf(`path: \`\${API_PREFIX}/sessions\``)
    expect(sessionsStart, 'the sessions route must be registered').toBeGreaterThan(-1)
    const sessionsHandler = source.slice(sessionsStart)
    const gateInSessions = sessionsHandler.indexOf("if (stopping) return json(res, 503, { error: 'bridge-shutting-down' })")
    expect(gateInSessions).toBeGreaterThan(-1)
    for (const later of ['parseSessionRoute(url.pathname)', 'const body = await readJson(req)', 'agent.followup(', 'executeCommand(']) {
      const index = sessionsHandler.indexOf(later)
      if (index === -1) continue
      expect(index, `${later} must run after the shutdown gate`).toBeGreaterThan(gateInSessions)
    }
  })

  it('makes teardown await bootstrap work through one idempotent path', async () => {
    const source = await (await import('node:fs/promises')).readFile(new URL('../src/index.ts', import.meta.url), 'utf8')
    // The whole point of the fix: teardown awaits the detached provisioning
    // task instead of returning while `icacls` still holds the directory.
    expect(source).toMatch(/await bootstrap\.settle\(\)/)
    expect(source).toContain('bootstrap.track(tokenReady)')
    expect(source).toMatch(/ctx\.effect\(\(\) => teardownOnce\)/)
    // The idle sweep is cleared from the same teardown, not a separate effect
    // whose ordering could drift.
    expect(source).toMatch(/let teardown: Promise<void> \| undefined[\s\S]*?clearInterval\(sweepTimer\)/)
  })
})
