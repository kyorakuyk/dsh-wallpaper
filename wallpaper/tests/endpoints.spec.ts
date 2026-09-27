import { describe, expect, it, vi, afterEach } from 'vitest'
import {
  CHECKOUT_ENDPOINT_PORT,
  DEFAULT_ENDPOINT_PORTS,
  OFFICIAL_SHELL_SUBJECT_ID,
  SHELL_SUBJECT_PREFIX,
  clientRaiseAction,
  endpointPriority,
  endpointScopeConfigured,
  endpointScopeOf,
  orderCandidates,
  raiseOutcomeNotice,
  scanSummary,
  selectEndpoint,
  subjectClientKind,
  subjectEndpointPorts,
  scanEndpoints,
  unsupportedShellSubjectFallback,
  type EndpointScope,
  type HarnessEndpointCandidate,
} from '../src/connect/endpoints.ts'
import { isEmbeddedShellSubject } from '../src/connect/harnessSubjects.ts'
import { monitorHarnessEndpoint, type EndpointUpdate } from '../src/connect/harnessEndpoint.ts'

const OFFICIAL_SHELL = `${SHELL_SUBJECT_PREFIX}com.deepseek.dsh`
/** 一个**不是**已知主体的 AUMID：第三方客户端 2026-09-27 起不再受支持，用它验证"未知主体不给端口"。 */
const UNKNOWN_SHELL = `${SHELL_SUBJECT_PREFIX}ai.deepseek.dsh.desktop`
const CHECKOUT = 'D:\\Family\\DeepSeekHarness\\deepseek-harness'

function candidate(
  port: number,
  availability: HarnessEndpointCandidate['status']['availability'],
  kind: HarnessEndpointCandidate['kind'] = 'official-web',
): HarnessEndpointCandidate {
  return {
    port,
    kind,
    source: 'default',
    status: { availability },
    bridgeFound: availability !== 'offline' && availability !== 'web-only',
  }
}

afterEach(() => vi.unstubAllGlobals())

describe('reaching a client interface', () => {
  it('opens a browser only for the windowless CLI/webui shape', () => {
    // The official desktop client owns a Windows window; the CLI/webui shape has none,
    // and its interface is the browser at the port it listens on. Getting this
    // backwards would try to raise a window that does not exist, or open a
    // browser for a client that already has a window.
    expect(clientRaiseAction('official-web')).toBe('browser')
    expect(clientRaiseAction('official-desktop')).toBe('window')
    // The windowless shape is the one on DSH's own default port.
    expect(DEFAULT_ENDPOINT_PORTS.find((entry) => entry.port === 3080)?.kind).toBe('official-web')
  })

  it('stays silent on success and on a refused foreground change', () => {
    // `raise-refused` means Windows declined the foreground change, which is the
    // normal situation for a desktop wallpaper. The window was still restored, so
    // reporting an error would be noise on a working path.
    expect(raiseOutcomeNotice('raised', 'official-desktop')).toBeNull()
    expect(raiseOutcomeNotice('raise-refused', 'official-desktop')).toBeNull()
  })

  it('names the client and the next step for the two real failures', () => {
    const noWindow = raiseOutcomeNotice('no-window', 'official-desktop')
    expect(noWindow).toContain('桌面客户端')
    expect(noWindow).toContain('浏览器')

    const notRunning = raiseOutcomeNotice('not-running', 'official-desktop')
    expect(notRunning).toContain('桌面客户端')
    // Not-running must say to start it, not imply the wallpaper will.
    expect(notRunning).toContain('请先启动')

    for (const notice of [noWindow, notRunning]) {
      expect(notice).not.toMatch(/[A-Za-z]:\\/)
      expect(notice).not.toMatch(/Bearer|token/i)
    }
  })
})

describe('endpoint priority', () => {
  it('ships the official desktop first, then the web/CLI shape', () => {
    // The user's stated default order. This is also why the two known ports are
    // listed in this sequence rather than numerically.
    expect(endpointPriority('official-desktop')).toBeLessThan(endpointPriority('official-web'))
    expect(DEFAULT_ENDPOINT_PORTS.map((entry) => entry.port)).toEqual([19387, 3080])
  })

  it('orders candidates by kind and then by port, without mutating the input', () => {
    const input = [
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(43120, 'bridge-ready', 'official-web'),
      candidate(19387, 'bridge-ready', 'official-desktop'),
    ]
    // 同 kind 内按端口升序 ⇒ 3080 在 43120 前；官方桌面整体优先。
    expect(orderCandidates(input).map((entry) => entry.port)).toEqual([19387, 3080, 43120])
    // The caller's array keeps its original order.
    expect(input.map((entry) => entry.port)).toEqual([3080, 43120, 19387])
  })

  it('breaks ties inside one kind by port so the list is stable', () => {
    const ordered = orderCandidates([
      candidate(5000, 'bridge-ready', 'official-web'),
      candidate(4000, 'bridge-ready', 'official-web'),
    ])
    expect(ordered.map((entry) => entry.port)).toEqual([4000, 5000])
  })
})

describe('endpoint selection', () => {
  it('prefers the highest-priority ready endpoint when nothing is configured', () => {
    const chosen = selectEndpoint([
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(19387, 'bridge-ready', 'official-desktop'),
    ], {})
    expect(chosen?.port).toBe(19387)
  })

  it('falls back to a Bridge that is present but not ready, over nothing', () => {
    // A Bridge that answers `bridge-auth-unavailable` is a different problem from
    // no Bridge at all, and the user needs to see which one it is.
    const chosen = selectEndpoint([
      candidate(19387, 'bridge-auth-unavailable', 'official-desktop'),
    ], {})
    expect(chosen?.port).toBe(19387)
    expect(chosen?.status.availability).toBe('bridge-auth-unavailable')
  })

  it('never selects a port that only hosts a plain web service', () => {
    expect(selectEndpoint([candidate(19387, 'web-only', 'official-desktop')], {})).toBeUndefined()
    expect(selectEndpoint([candidate(3080, 'offline')], {})).toBeUndefined()
  })

  it('honours a pinned port even when it is not ready and another one is', () => {
    // Silently switching would answer a *different* client than the user picked,
    // which is worse than reporting the chosen one as unavailable.
    const chosen = selectEndpoint([
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(19387, 'bridge-loading', 'official-desktop'),
    ], { endpointPort: 19387 })
    expect(chosen?.port).toBe(19387)
    expect(chosen?.status.availability).toBe('bridge-loading')
  })

  it('reports the pinned port as unreachable instead of substituting another one', () => {
    // The regression this pins: a pin that the scan did not see used to fall back
    // to "auto", which handed the session to whichever client happened to answer —
    // the exact substitution the frozen rule forbids.
    expect(selectEndpoint([candidate(3080, 'bridge-ready', 'official-web')], { endpointPort: 19387 })).toBeUndefined()
  })
})

/**
 * The frozen rule, from the user's own words: 「无论A是怎么死的，都不允许静默用B
 * 来替换A，除非用户手动替换」/「设置里设置的是什么就是什么」.
 *
 * Everything below is one statement of that rule at a different level: which ports
 * a subject owns, which candidate may be used, and what happens when the subject
 * the user chose is the one that is down.
 */
describe('the configured subject decides the endpoint', () => {
  it('gives a shell the one port compiled into it', () => {
    expect(subjectEndpointPorts({ subjectId: OFFICIAL_SHELL })).toEqual([19387])
    // 第三方桌面客户端 2026-09-27 起不再受支持：它的 AUMID 现在与任何陌生 AUMID 一样
    // **不给出任何端口**（而不是"给出 43120 然后探测失败"）。
    expect(subjectEndpointPorts({ subjectId: UNKNOWN_SHELL })).toEqual([])
    // The AUMID is matched case-insensitively, like the native table does.
    expect(subjectEndpointPorts({ subjectId: `${SHELL_SUBJECT_PREFIX}COM.DeepSeek.DSH` })).toEqual([19387])
  })

  it('gives a source tree its own default plus the ports the user added for it', () => {
    expect(subjectEndpointPorts({ subjectId: CHECKOUT })).toEqual([CHECKOUT_ENDPOINT_PORT])
    expect(subjectEndpointPorts({ subjectId: CHECKOUT, extraPorts: [3081, 3081, 0, 70000] }))
      .toEqual([CHECKOUT_ENDPOINT_PORT, 3081])
  })

  it('lets an explicit pin outrank the subject, and reads as configured either way', () => {
    // A pin is the user saying where their DSH is, which is not a substitution.
    expect(subjectEndpointPorts({ subjectId: OFFICIAL_SHELL, endpointPort: 3080 })).toEqual([3080])
    expect(endpointScopeConfigured({ subjectId: OFFICIAL_SHELL })).toBe(true)
    expect(endpointScopeConfigured({ endpointPort: 3080 })).toBe(true)
    // Nothing chosen yet is the one case that keeps the shipped priority order.
    expect(subjectEndpointPorts({})).toBeUndefined()
    expect(subjectEndpointPorts({ subjectId: '   ' })).toBeUndefined()
    expect(endpointScopeConfigured({})).toBe(false)
  })

  it('admits no port for a shell this build cannot place', () => {
    // Configured, but nowhere to look: reporting it as "nothing configured" would
    // put the wallpaper back on the priority order — another client, by accident.
    expect(subjectEndpointPorts({ subjectId: `${SHELL_SUBJECT_PREFIX}com.unknown.client` })).toEqual([])
    expect(endpointScopeConfigured({ subjectId: `${SHELL_SUBJECT_PREFIX}com.unknown.client` })).toBe(true)
  })

  it('never uses another client\u2019s ready Bridge while the chosen one is down', () => {
    const scan = [
      candidate(43120, 'bridge-ready', 'official-web'),
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(19387, 'offline', 'official-desktop'),
    ]
    const chosen = selectEndpoint(scan, { subjectId: OFFICIAL_SHELL })
    // 端口 43120 上有个现成可用的 Bridge，而它依然不是答案：在**被选中的主体**应答之前，
    // 灯就应当是不亮的。
    expect(chosen?.port).toBe(19387)
    expect(chosen?.status.availability).toBe('offline')
  })

  it('keeps a live conversation on the endpoint it started on', () => {
    // 一个主体可以有多个许可端口（源码树默认 3080 + 用户给它加的端口）：点在谁身上就留在谁身上，
    // 即使另一个端口优先级更高。
    const scan = [
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(3081, 'bridge-ready', 'official-web'),
    ]
    const scope = { subjectId: CHECKOUT, extraPorts: [3081] }
    expect(selectEndpoint(scan, scope, 3081)?.port).toBe(3081)
    // 而主体之外的粘滞端口被忽略，不会被"恢复"回来。
    expect(selectEndpoint(scan, scope, 9999)?.port).toBe(3080)
    // A sticky port that stopped answering hands the choice to the next permitted
    // one rather than freezing the wallpaper on a dead port.
    expect(selectEndpoint([
      candidate(3080, 'bridge-ready', 'official-web'),
      candidate(3081, 'offline', 'official-web'),
    ], { subjectId: CHECKOUT, extraPorts: [3081] }, 3081)?.port).toBe(3080)
  })

  it('uses the subject\u2019s own port order for a tree with an added port', () => {
    const scan = [
      candidate(3081, 'bridge-ready', 'official-web'),
      candidate(3080, 'bridge-ready', 'official-web'),
    ]
    // The tree's default leads its added port, so the same configuration always
    // resolves to the same instance.
    expect(selectEndpoint(scan, { subjectId: CHECKOUT, extraPorts: [3081] })?.port).toBe(3080)
  })

  it('reads the stored launch fields as one scope', () => {
    // The one place the settings become a scope: the background surface and the
    // settings window both go through it, so neither can mean something different.
    expect(endpointScopeOf({ subjectId: OFFICIAL_SHELL, endpointPort: 3080, extraEndpointPorts: [3081] }))
      .toEqual({ subjectId: OFFICIAL_SHELL, endpointPort: 3080, extraPorts: [3081] })
    // A profile written before `subjectId` existed still names its checkout.
    expect(endpointScopeOf({ rootPath: CHECKOUT }).subjectId).toBe(CHECKOUT)
    // And the current field wins over the legacy one, as the launcher resolves it.
    expect(endpointScopeOf({ subjectId: OFFICIAL_SHELL, rootPath: CHECKOUT }).subjectId).toBe(OFFICIAL_SHELL)
  })

  it('names the shape of a subject without a scan', () => {
    // The shape decides how the interface is reached, and it must not depend on
    // something being live in order to be known.
    expect(subjectClientKind(OFFICIAL_SHELL)).toBe('official-desktop')
    expect(subjectClientKind(CHECKOUT)).toBe('official-web')
    // 第三方客户端的 AUMID 现在与陌生 AUMID 一样：**没有形状**（因为它不再受支持）。
    expect(subjectClientKind(UNKNOWN_SHELL)).toBeUndefined()
    expect(subjectClientKind(`${SHELL_SUBJECT_PREFIX}com.unknown.client`)).toBeUndefined()
    expect(subjectClientKind(undefined)).toBeUndefined()
  })

  it('falls back to the official client when the stored subject is unsupported', () => {
    // 设置里存着一个本 build 已不支持的壳主体（第三方客户端被移除前的遗留）：不静默、也不
    // 卡在"未知主体"，而是落回官方桌面客户端并把原因说清楚。
    const fallback = unsupportedShellSubjectFallback(UNKNOWN_SHELL)
    expect(fallback?.subjectId).toBe(OFFICIAL_SHELL_SUBJECT_ID)
    expect(fallback?.notice).toContain('已不再受支持')
    expect(fallback?.notice).toContain('已切回官方桌面客户端')
    // 落回去的那个 id 必须真的是本 build 认识的主体，否则等于换了个看不到的灯。
    expect(subjectClientKind(OFFICIAL_SHELL_SUBJECT_ID)).toBe('official-desktop')
    expect(subjectEndpointPorts({ subjectId: OFFICIAL_SHELL_SUBJECT_ID })).toEqual([19387])
    // 反例：没有存值、存的是已知主体、或存的是源码目录路径 ⇒ **不动**（路径永远是合法身份）。
    expect(unsupportedShellSubjectFallback(undefined)).toBeNull()
    expect(unsupportedShellSubjectFallback('   ')).toBeNull()
    expect(unsupportedShellSubjectFallback(OFFICIAL_SHELL)).toBeNull()
    expect(unsupportedShellSubjectFallback(CHECKOUT)).toBeNull()
  })

  it('agrees with the shell table the rest of the bridge uses', () => {
    // 同一份 AUMID↔端口表在这里与 `harness_targets.rs` 各存一份。端口不许漂移：主体被绑到一个
    // 没人监听的端口上，就是"灯不亮而看不到原因"。
    for (const aumid of ['com.deepseek.dsh']) {
      const ports = subjectEndpointPorts({ subjectId: `${SHELL_SUBJECT_PREFIX}${aumid}` })
      expect(ports).toHaveLength(1)
      const known = DEFAULT_ENDPOINT_PORTS.find((entry) => entry.port === ports?.[0])
      expect(known, `port for ${aumid} must be a client port this build scans`).toBeDefined()
      expect(known?.kind).toBe(subjectClientKind(`${SHELL_SUBJECT_PREFIX}${aumid}`))
    }
    // 曾经也在表里的第三方客户端：现在既不给端口，也不给形状（§ 用户 2026-09-27 的要求）。
    expect(subjectEndpointPorts({ subjectId: UNKNOWN_SHELL })).toEqual([])
    expect(subjectClientKind(UNKNOWN_SHELL)).toBeUndefined()
    // The subject prefix is one namespace, not two spellings of it.
    expect(isEmbeddedShellSubject(OFFICIAL_SHELL)).toBe(true)
    expect(isEmbeddedShellSubject(CHECKOUT)).toBe(false)
  })
})

describe('scan summary', () => {
  it('distinguishes "found none" from "found some but none usable"', () => {
    const none = scanSummary([candidate(19387, 'offline'), candidate(3080, 'offline')])
    expect(none.bridges).toBe(0)
    expect(none.summary).toContain('未发现可接入')
    // The count of scanned ports is always reported, so a scan that found nothing
    // still proves it looked somewhere.
    expect(none.summary).toContain('2')

    const present = scanSummary([
      candidate(19387, 'bridge-loading', 'official-desktop'),
      candidate(3080, 'offline'),
    ])
    expect(present.bridges).toBe(1)
    expect(present.ready).toBe(0)
    expect(present.summary).toContain('发现 1 个')

    const ready = scanSummary([
      candidate(19387, 'bridge-ready', 'official-desktop'),
      candidate(43120, 'bridge-ready', 'official-web'),
    ])
    expect(ready.ready).toBe(2)
    expect(ready.summary).toContain('2 个可用')
  })
})

describe('scanning', () => {
  it('identifies a Bridge and ignores a non-Bridge service on the same path', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.includes(':19387/api/')) {
        return {
          ok: true,
          json: async () => ({
            protocolVersion: 1,
            dsh: 'online',
            state: 'bridge-ready',
            reasonCode: 'ready',
            authentication: 'ready',
            capabilities: ['status', 'control', 'sessions', 'history', 'sse', 'cancel', 'approval-handoff'],
          }),
        }
      }
      if (url.includes(':43120/api/')) {
        // Answers the path, but with some other service's document.
        return { ok: true, json: async () => ({ status: 'unrelated-service' }) }
      }
      throw new Error('connection refused')
    })
    vi.stubGlobal('fetch', fetchMock)

    // 43120 不再是默认端口（第三方客户端已移除）⇒ 作为**用户自己填的端口**探测：壁纸仍然要能
    // 诚实回答"那儿有个别的服务"，而不是把它当成会话端点。
    const found = await scanEndpoints([43120])
    expect(found.map((entry) => entry.port)).toEqual([19387, 3080, 43120])
    expect(found[0]).toMatchObject({ port: 19387, bridgeFound: true })
    expect(found[0]?.status.availability).toBe('bridge-ready')
    expect(found[1]).toMatchObject({ port: 3080, bridgeFound: false })
    expect(found[1]?.status.availability).toBe('offline')
    // A non-Bridge document must not be promoted to an endpoint.
    expect(found[2]).toMatchObject({ port: 43120, bridgeFound: false })
    expect(found[2]?.status.availability).toBe('web-only')
  })

  it('accepts user-supplied ports without letting duplicates or junk in', async () => {
    const seen: string[] = []
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      seen.push(url)
      throw new Error('refused')
    }))
    const found = await scanEndpoints([19387, 6000, 0, 70000, 6000, 1.5])
    expect(found.map((entry) => entry.port)).toEqual([19387, 3080, 6000])
    // A duplicate of a default port is not probed twice.
    expect(seen.filter((url) => url.includes(':19387/'))).toHaveLength(1)
    // Invalid values never become candidates.
    for (const bad of ['0', '70000', '1.5']) {
      expect(seen.some((url) => url.includes(`:${bad}/`))).toBe(false)
    }
  })

  it('reports web-only when the Bridge path fails but the root page answers', async () => {
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url.includes('/api/')) return { ok: false, status: 401 }
      return { ok: true }
    }))
    const found = await scanEndpoints()
    for (const entry of found) {
      expect(entry.bridgeFound).toBe(false)
      expect(entry.status.availability).toBe('web-only')
      expect(entry.status.reasonCode).toBe('bridge-status-missing')
    }
  })
})

/**
 * The same frozen rule, through the loop the desktop actually runs: the browser
 * preview owns the probe loop, and the native monitor mirrors it.
 */
describe('the endpoint monitor', () => {
  const bridgeDocument = {
    protocolVersion: 1,
    dsh: 'online',
    state: 'bridge-ready',
    authentication: 'ready',
    capabilities: ['status', 'control', 'sessions', 'history', 'sse', 'cancel', 'approval-handoff'],
  }

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  /** A fake loopback: only `answering` ports host a usable Bridge. */
  function stubDialled(answering: () => readonly number[], seen?: string[]) {
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      seen?.push(url)
      const port = Number(url.split(':')[2]?.split('/')[0])
      return answering().includes(port)
        ? { ok: true, json: async () => bridgeDocument }
        : Promise.reject(new Error('connection refused'))
    }))
  }

  /** Run the monitor for a few poll intervals and collect what it published. */
  async function published(scope: () => EndpointScope, ticks = 4): Promise<EndpointUpdate[]> {
    const updates: EndpointUpdate[] = []
    const monitor = monitorHarnessEndpoint({
      scope,
      onChange: (update) => updates.push(update),
      readyIntervalMs: 5,
      idleIntervalMs: 5,
    })
    await vi.advanceTimersByTimeAsync(ticks * 5)
    monitor.stop()
    return updates
  }

  it('keeps the light off while another client answers instead of the chosen one', async () => {
    vi.useFakeTimers()
    // 另一个端口上有个现成可用的服务，而被选中的是官方客户端：拿那个来应答就是"替用户换了主体"。
    stubDialled(() => [43120])
    const updates = await published(() => ({ subjectId: OFFICIAL_SHELL }))
    expect(updates.length).toBeGreaterThan(0)
    for (const update of updates) {
      expect(update.status.availability).not.toBe('bridge-ready')
      expect(update.port).toBe(19387)
    }
    expect(updates.at(-1)?.status.reasonCode).toBe('subject-offline')
  })

  it('never even asks a port outside the configured subject', async () => {
    vi.useFakeTimers()
    const seen: string[] = []
    stubDialled(() => [], seen)
    await published(() => ({ subjectId: OFFICIAL_SHELL }), 2)
    // Probing another client's port on the way would be harmless but dishonest:
    // the settings say which subject this wallpaper talks to.
    expect(seen.length).toBeGreaterThan(0)
    expect(seen.every((url) => url.includes(':19387'))).toBe(true)
  })

  it('publishes the configured subject as soon as it answers', async () => {
    vi.useFakeTimers()
    stubDialled(() => [19387])
    const updates = await published(() => ({ subjectId: OFFICIAL_SHELL }))
    expect(updates.at(-1)?.status.availability).toBe('bridge-ready')
    expect(updates.at(-1)?.port).toBe(19387)
  })

  it('stays on the client it connected to when nothing is configured', async () => {
    vi.useFakeTimers()
    // Nothing chosen yet, so any running client will do — but the wallpaper must
    // then *stay* with the one it picked: a higher-priority client appearing must
    // not pull a live conversation off it.
    let answering: number[] = [3080]
    stubDialled(() => answering)
    const updates: EndpointUpdate[] = []
    const monitor = monitorHarnessEndpoint({
      scope: () => ({}),
      onChange: (update) => updates.push(update),
      readyIntervalMs: 5,
      idleIntervalMs: 5,
    })
    await vi.advanceTimersByTimeAsync(20)
    expect(updates.at(-1)?.port).toBe(3080)
    answering = [3080, 19387, 43120]
    await vi.advanceTimersByTimeAsync(20)
    monitor.stop()
    expect(updates.at(-1)?.port).toBe(3080)
  })

  it('hands over only when the settings change the subject', async () => {
    vi.useFakeTimers()
    stubDialled(() => [19387, 3080])
    let scope: EndpointScope = { subjectId: OFFICIAL_SHELL }
    const updates: EndpointUpdate[] = []
    const monitor = monitorHarnessEndpoint({
      scope: () => scope,
      onChange: (update) => updates.push(update),
      readyIntervalMs: 5,
      idleIntervalMs: 5,
    })
    await vi.advanceTimersByTimeAsync(20)
    expect(updates.at(-1)?.port).toBe(19387)
    // A real change of subject is the one thing that may move the endpoint.
    scope = { subjectId: CHECKOUT }
    await vi.advanceTimersByTimeAsync(20)
    monitor.stop()
    expect(updates.at(-1)?.port).toBe(3080)
  })
})
