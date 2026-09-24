import { spawn, type ChildProcess } from 'node:child_process'
import { existsSync } from 'node:fs'
import { cp, mkdir, mkdtemp, readFile, rm, symlink } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { afterAll, describe, expect, it } from 'vitest'
import { API_PREFIX } from '../src/protocol.ts'

/**
 * The route fixtures use a mock Cordis context, so they prove the *contract*
 * but never that a real host mounts anything. This smoke test boots a real DSH
 * CLI with a real composed profile that loads this repository's Bridge build,
 * and asserts that every announced capability corresponds to a reachable route.
 *
 * It is deliberately opt-in and self-contained:
 *  - it `skip`s unless a DSH checkout with a built CLI and a real profile exist;
 *  - it clones the profile into the OS temp directory (a few MB of bundle
 *    dependencies) and runs `DSH_HOME` pointed there, so the user's sessions,
 *    costs, credentials and token file are never read or written;
 *  - the cloned profile's Bridge is a junction to this working tree, so it
 *    tests the current code, not whatever copy an installed profile holds;
 *  - it never prints the bearer token.
 */

const DSH_ROOT = process.env.DSH_WALLPAPER_SMOKE_DSH_ROOT ?? 'D:\\Family\\DeepSeekHarness\\deepseek-harness'
const DSH_CLI = join(DSH_ROOT, 'apps', 'cli', 'lib', 'bin.js')
const REAL_PROFILE = process.env.DSH_WALLPAPER_SMOKE_PROFILE
  ?? join(process.env.USERPROFILE ?? '', '.dsh', 'profiles', 'desktop')
const BRIDGE_DIR = resolve(import.meta.dirname, '..')
const BASE_URL = 'http://127.0.0.1:3080'
const STATUS_URL = `${BASE_URL}${API_PREFIX}/status`
/** Cold DSH boot plus Bridge composition. */
const READY_TIMEOUT_MS = 90_000
const POLL_INTERVAL_MS = 1_000

interface SmokeContext {
  child: ChildProcess
  home: string
  tokenFile: string
  output: string[]
}

let context: SmokeContext | undefined

/** The profile resolves every bundle from its own directory, so the clone must
 * carry the composed dependency set, not just the manifest. */
const PROFILE_FILES = ['package.json', 'cordis.yml', 'cordis.patch.yml', 'pnpm-workspace.yaml', 'pnpm-lock.yaml']

const smokeReady = existsSync(DSH_CLI)
  && existsSync(join(REAL_PROFILE, 'package.json'))
  && existsSync(join(REAL_PROFILE, 'node_modules'))
  && existsSync(join(BRIDGE_DIR, 'cordis.patch.yml'))

async function stopTree(child: ChildProcess): Promise<void> {
  if (child.pid === undefined || child.exitCode !== null) return
  // `taskkill /T` mirrors the wallpaper's own exit path: DSH spawns children and
  // killing only the parent would leave them holding port 3080.
  await new Promise<void>((settle) => {
    const kill = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' })
    kill.on('close', () => settle())
    kill.on('error', () => settle())
  })
}

async function bootProfile(): Promise<SmokeContext> {
  const home = await mkdtemp(join(tmpdir(), 'dsh-wallpaper-smoke-'))
  const profileDir = join(home, 'profiles', 'desktop')
  await mkdir(profileDir, { recursive: true })
  for (const file of PROFILE_FILES) {
    const source = join(REAL_PROFILE, file)
    if (existsSync(source)) await cp(source, join(profileDir, file))
  }
  await cp(join(REAL_PROFILE, 'node_modules'), join(profileDir, 'node_modules'), { recursive: true })
  // Point the plugin at this working tree instead of an installed copy. A
  // junction needs no privileges on Windows and keeps the tested code identical
  // to what the build produces.
  const installedBridge = join(profileDir, 'node_modules', 'dsh-wallpaper-bridge')
  await rm(installedBridge, { recursive: true, force: true })
  await symlink(BRIDGE_DIR, installedBridge, 'junction')

  const output: string[] = []
  const child = spawn(process.execPath, [DSH_CLI, '--profile', 'desktop'], {
    env: { ...process.env, DSH_HOME: home },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  child.stdout?.on('data', (chunk: Buffer) => output.push(chunk.toString()))
  child.stderr?.on('data', (chunk: Buffer) => output.push(chunk.toString()))
  return { child, home, tokenFile: join(home, 'wallpaper', 'bridge-token'), output }
}

interface BridgeStatus {
  state: string
  reasonCode: string
  capabilities: string[]
  protocolVersion: number
  authentication: string
}

async function waitForStatus(): Promise<BridgeStatus> {
  const deadline = Date.now() + READY_TIMEOUT_MS
  let lastError = 'no attempt made'
  while (Date.now() < deadline) {
    const active = context
    if (active && active.child.exitCode !== null) {
      throw new Error(`DSH exited early with code ${active.child.exitCode}:\n${active.output.join('')}`)
    }
    try {
      const response = await fetch(STATUS_URL, { signal: AbortSignal.timeout(2_000) })
      if (response.ok) {
        const body = await response.json() as Record<string, unknown>
        if (body.dsh === 'online') {
          return {
            state: String(body.state ?? ''),
            reasonCode: String(body.reasonCode ?? ''),
            capabilities: Array.isArray(body.capabilities) ? body.capabilities as string[] : [],
            protocolVersion: Number(body.protocolVersion ?? 0),
            authentication: String(body.authentication ?? ''),
          }
        }
      }
      lastError = `http ${response.status}`
    } catch (error) {
      lastError = String(error)
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
  }
  throw new Error(`the Bridge never answered /status within ${READY_TIMEOUT_MS} ms (last: ${lastError})`)
}

async function authorizedFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const token = (await readFile(context!.tokenFile, 'utf8')).trim()
  expect(token.length).toBeGreaterThan(0)
  return fetch(`${BASE_URL}${path}`, {
    ...init,
    headers: { ...init.headers, Authorization: `Bearer ${token}`, Accept: 'application/json' },
  })
}

afterAll(async () => {
  const active = context
  context = undefined
  if (!active) return
  await stopTree(active.child)
  await rm(active.home, { recursive: true, force: true })
})

describe.skipIf(!smokeReady)('real DSH desktop profile smoke test', () => {
  it('mounts every capability it announces and serves a session end to end', async () => {
    context = await bootProfile()
    const status = await waitForStatus()

    // The host really composed every service this Bridge needs, so the route
    // table exists and the announced capabilities can be trusted.
    expect(status.protocolVersion).toBe(1)
    expect(status.state).toBe('bridge-ready')
    expect(status.reasonCode).toBe('ready')
    expect(status.authentication).toBe('ready')
    for (const capability of ['status', 'control', 'sessions', 'history', 'sse', 'cancel', 'approval-handoff']) {
      expect(status.capabilities, `capabilities: ${status.capabilities.join(',')}`).toContain(capability)
    }

    // Telemetry identities the wallpaper can use to detect a stale install. DSH
    // exposes no runtime version, so `authoredAgainst` reports the API range this
    // build was compiled against rather than pretending to know the host.
    const raw = await (await fetch(STATUS_URL)).json() as Record<string, unknown>
    expect(String(raw.bridgeVersion ?? '')).toMatch(/^\d+\.\d+\.\d+/)
    expect(typeof raw.bridgeBuild).toBe('string')
    expect(String(raw.authoredAgainst ?? '')).toMatch(/^\^?\d+\.\d+\.\d+/)

    // The token must never appear in the status document.
    const token = (await readFile(context.tokenFile, 'utf8')).trim()
    expect(JSON.stringify(raw)).not.toContain(token)

    // An unauthenticated caller must not reach the route table.
    const unauthorized = await fetch(`${BASE_URL}${API_PREFIX}/sessions`, { method: 'POST', body: '{}' })
    expect(unauthorized.status).toBe(401)

    // The announced `sessions` capability must correspond to a real route: this
    // is exactly the 404 the previous contract could not distinguish itself from.
    const created = await authorizedFetch(`${API_PREFIX}/sessions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ sessionId: 'wallpaper-smoke' }),
    })
    expect(created.status, `body: ${await created.clone().text()}`).toBe(201)
    await expect(created.json()).resolves.toMatchObject({ sessionId: 'wallpaper-smoke' })

    // A resume of a session this profile does not own must be an explicit
    // conflict, never a silent new transcript.
    const foreign = await authorizedFetch(`${API_PREFIX}/sessions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ resumeSessionId: 'not-owned-by-this-profile' }),
    })
    expect(foreign.status).toBe(409)
    await expect(foreign.json()).resolves.toMatchObject({ error: 'resume-unavailable' })

    // `sse` must likewise be a real route with the ready handshake.
    const sse = await authorizedFetch(`${API_PREFIX}/sessions/wallpaper-smoke/events`, {
      signal: AbortSignal.timeout(10_000),
    })
    expect(sse.status).toBe(200)
    expect(sse.headers.get('x-dsh-wallpaper-sse-ready')).toBe('1')
    await sse.body?.cancel()

    // `history` and `cancel` round out the announced set.
    const history = await authorizedFetch(`${API_PREFIX}/sessions/wallpaper-smoke/history`)
    expect(history.status).toBe(200)
    await expect(history.json()).resolves.toMatchObject({ sessionId: 'wallpaper-smoke' })

    const cancelled = await authorizedFetch(`${API_PREFIX}/sessions/wallpaper-smoke/cancel`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{}',
    })
    expect(cancelled.status).toBe(202)
  }, READY_TIMEOUT_MS + 60_000)
})
