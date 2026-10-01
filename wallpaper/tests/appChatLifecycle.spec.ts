import { beforeAll, describe, expect, it } from 'vitest'
import type { ChatAdapter } from '../src/chat/adapter.ts'
import { NativeChatAdapter } from '../src/chat/nativeAdapter.ts'
import type { BackendMode } from '../src/domain/types.ts'
import { nativeRuntime } from '../src/native/runtime.ts'

// AppCore's browser preview fallback is evaluated when App.tsx is imported.
// These are pure lifecycle tests, so a minimal window is enough and avoids
// mounting a full desktop surface just to verify stale-operation guards.
beforeAll(() => {
  if (!('window' in globalThis)) Object.assign(globalThis, { window: {} })
})

function adapter(mode: BackendMode): ChatAdapter {
  return {
    mode,
    async connect() {},
    disconnect() {},
    async send() {},
    async stop() {},
    async history() { return [] },
    subscribe() { return () => undefined },
  }
}

describe('App chat lifecycle isolation', () => {
  it('accepts only the mounted adapter for the backend it was created for', async () => {
    const { isCurrentChatOperation } = await import('../src/App.tsx')
    const api = adapter('deepseek-api')
    const harness = adapter('harness')

    expect(isCurrentChatOperation(api, 'deepseek-api', api, 'deepseek-api', false)).toBe(true)
    expect(isCurrentChatOperation(harness, 'harness', api, 'deepseek-api', false)).toBe(false)
    expect(isCurrentChatOperation(api, 'harness', api, 'deepseek-api', false)).toBe(false)
    expect(isCurrentChatOperation(api, 'deepseek-api', api, 'deepseek-api', true)).toBe(false)
  })

  it('keeps Harness selected and supplies a controlled disconnected state', async () => {
    const {
      canAutoSelectHarness,
      canSelectBackend,
      harnessAvailabilityPatch,
      harnessDisconnectedErrorPrefix,
      harnessFallbackBackend,
      harnessSelectionUnavailableError,
    } = await import('../src/App.tsx')
    const disconnected = harnessAvailabilityPatch('harness', 'offline')

    expect(disconnected).toMatchObject({ activity: 'idle' })
    expect(disconnected?.error).toContain(harnessDisconnectedErrorPrefix())
    expect(disconnected?.error).toContain('已保留当前 Harness 会话和对话记录')
    expect(harnessAvailabilityPatch('deepseek-web', 'offline')).toBeUndefined()
    expect(harnessAvailabilityPatch('harness', 'bridge-ready', disconnected?.error)).toEqual({ activity: 'idle', error: undefined })
    expect(canAutoSelectHarness('bridge-ready', 'deepseek-web', true)).toBe(true)
    expect(canAutoSelectHarness('web-only', 'deepseek-web', true)).toBe(false)
    expect(canAutoSelectHarness('offline', 'harness', true)).toBe(false)
    expect(canAutoSelectHarness('offline', 'harness', false)).toBe(false)
    expect(canSelectBackend('bridge-ready', 'harness')).toBe(true)
    expect(canSelectBackend('web-only', 'harness')).toBe(false)
    expect(canSelectBackend('offline', 'harness')).toBe(false)
    expect(canSelectBackend('web-only', 'deepseek-api')).toBe(true)
    expect(harnessSelectionUnavailableError('web-only')).toContain('未安装、未启动或不兼容')
    expect(harnessSelectionUnavailableError('offline')).toContain('未能连接')
    // 主体**彻底退出**后滑槽要复位到左侧（拉起 harness 的入口就在壁纸里，停在死掉的一侧
    // 会让用户不得不再手动切一次）。只在 offline 生效：bridge-loading 是"正在起来"。
    expect(harnessFallbackBackend('offline', 'harness', 'deepseek-web')).toBe('deepseek-web')
    expect(harnessFallbackBackend('offline', 'harness', 'harness')).toBe('deepseek-web')
    expect(harnessFallbackBackend('bridge-loading', 'harness', 'deepseek-web')).toBeUndefined()
    expect(harnessFallbackBackend('bridge-ready', 'harness', 'deepseek-web')).toBeUndefined()
    expect(harnessFallbackBackend('offline', 'deepseek-web', 'deepseek-web')).toBeUndefined()
  })

  it('moves the switch back only when the wallpaper itself moved it away', async () => {
    const { shouldReturnToHarness, harnessResetClaimed } = await import('../src/App.tsx')

    // 主体退出后壁纸自己复位过：它回来时，滑槽要自己拨回去（用户原话：再次联通之后要以
    // harness 后端为准）。复位期间保留的那段转写属于同一个会话——会话按日期命名、转写留在
    // 宿主那边，所以拨回去接上的就是同一段记录。
    expect(shouldReturnToHarness('bridge-ready', 'deepseek-web', true)).toBe(true)
    // 用户自己拨到左侧的：壁纸没有打断他，也就不该反过来打断他。
    expect(shouldReturnToHarness('bridge-ready', 'deepseek-web', false)).toBe(false)
    // 还没就绪、或者本来就在 Harness 上：无事可做。
    expect(shouldReturnToHarness('bridge-loading', 'deepseek-web', true)).toBe(false)
    expect(shouldReturnToHarness('offline', 'deepseek-web', true)).toBe(false)
    expect(shouldReturnToHarness('bridge-ready', 'harness', true)).toBe(false)

    // 这次复位是**壁纸自己**做的，而壁纸装一次新版就重启一次：权利写进设置，活过重启。少了这
    // 一半，桥回来了、灯是绿的，滑槽却停在左侧——用户以为还在跟 DSH 说话，输入进了另一个后端
    // （用户实测报的"输入被吞"）。用户手动拨过一次就会清掉它，他选的那一侧永远优先。
    expect(harnessResetClaimed(false, undefined)).toBe(false)
    expect(harnessResetClaimed(true, undefined)).toBe(true)
    expect(harnessResetClaimed(false, Date.now())).toBe(true)
    expect(harnessResetClaimed(true, Date.now())).toBe(true)
    // 坏值不算凭据。
    expect(harnessResetClaimed(false, Number.NaN)).toBe(false)
    expect(shouldReturnToHarness('bridge-ready', 'deepseek-web', harnessResetClaimed(false, 1))).toBe(true)
    expect(shouldReturnToHarness('bridge-ready', 'deepseek-web', harnessResetClaimed(false, undefined))).toBe(false)
  })

  /**
   * 聊天层的通知必须和原生宿主的 rror **分开存**：后者会被原生快照整体覆写，挤在一起时
   * 通知刚写进去就被下一条快照擦掉（用户实测："顶上弹了一下，消失得极快"）。
   */
  it('keeps a chat-layer notice visible even though snapshots own the host error', async () => {
    const { visibleNotice } = await import('../src/App.tsx')
    const afterTurnBlocked = { chatNotice: 'DSH 拒绝了这一轮（这条会话已被归档）', error: undefined }
    // 快照随后把宿主那句话写进来（或写空）：聊天层那句仍然在屏幕上。
    expect(visibleNotice({ ...afterTurnBlocked, error: undefined })).toBe(afterTurnBlocked.chatNotice)
    expect(visibleNotice({ ...afterTurnBlocked, error: 'Bridge 已断开' })).toBe(afterTurnBlocked.chatNotice)
    // 没有聊天层通知时，宿主那句话照旧显示；两者都没有则不显示。
    expect(visibleNotice({ error: 'Bridge 已断开' })).toBe('Bridge 已断开')
    expect(visibleNotice({})).toBeUndefined()
  })

  it('treats connecting and reconnecting as the amber state', async () => {
    const { isHarnessTransitioning } = await import('../src/App.tsx')
    const state = (over: Partial<Parameters<typeof isHarnessTransitioning>[0]>) => ({
      starting: false,
      probing: false,
      availability: 'offline' as const,
      ...over,
    })

    // 用户要的三段语义：连上（绿）／正在连（黄呼吸）／不在了（熄灭）。
    expect(isHarnessTransitioning(state({ starting: true }))).toBe(true)                 // 正在拉起 harness 后台
    expect(isHarnessTransitioning(state({ availability: 'bridge-loading' }))).toBe(true) // 宿主应答了，Bridge 还在装
    expect(isHarnessTransitioning(state({ availability: 'bridge-ready', probing: true }))).toBe(true) // 已连上但暂时失联
    // 已连接、一切正常：绿灯。
    expect(isHarnessTransitioning(state({ availability: 'bridge-ready' }))).toBe(false)
    // 后端确实不在了：这是**结论**，不是中间态，灯该熄灭——两者的后续动作完全不同。
    expect(isHarnessTransitioning(state({}))).toBe(false)
  })

  it('keeps an API adapter lifecycle stable while committing later request settings', async () => {
    const { apiAdapterOptionsFromSettings, chatAdapterLifecycleKey, updateApiAdapterOptions, subjectScopeKey } = await import('../src/App.tsx')
    const first = {
      deepseekApi: {
        baseUrl: 'https://api.deepseek.com',
        model: 'deepseek-chat',
        priceInputPerMillion: 1,
        priceOutputPerMillion: 2,
      },
    }
    const edited = {
      deepseekApi: {
        baseUrl: 'https://proxy.example.test/v1',
        model: 'deepseek-reasoner',
        priceInputPerMillion: 3,
        priceOutputPerMillion: 4,
      },
    }
    const stableOptions = apiAdapterOptionsFromSettings(first)
    const scope = subjectScopeKey({ subjectId: 'cli:C:/npm/dsh.cmd', endpointPort: 3080 })
    const mountedLifecycle = chatAdapterLifecycleKey('deepseek-api', 7, scope)

    // The mounted adapter holds this exact object. Updating it for the next
    // request must not manufacture a new lifecycle/effect identity.
    expect(updateApiAdapterOptions(stableOptions, edited)).toBe(stableOptions)
    expect(stableOptions).toEqual({
      baseUrl: 'https://proxy.example.test/v1',
      model: 'deepseek-reasoner',
      priceInputPerMillion: 3,
      priceOutputPerMillion: 4,
    })
    expect(chatAdapterLifecycleKey('deepseek-api', 7, scope)).toBe(mountedLifecycle)
    expect(chatAdapterLifecycleKey('deepseek-api', 8, scope)).not.toBe(mountedLifecycle)
    // 主体（端点范围）也必须进这个键：切换主体不会重建适配器，而适配器持有的是"连着哪个端点、
    // 哪条会话"—— 换了主体却留着旧端点的会话，就是"灯说已连接、一发却说会话尚未建立"。
    const shellScope = subjectScopeKey({ subjectId: 'shell:com.deepseek.dsh', endpointPort: 19387 })
    expect(chatAdapterLifecycleKey('deepseek-api', 7, shellScope)).not.toBe(mountedLifecycle)
  })

  it('keeps the amber light on for a fixed moment after a subject switch', async () => {
    const { HARNESS_SWITCH_BUFFER_MS } = await import('../src/App.tsx')
    // 用户要的是"固定 1–2s 的黄灯缓冲后立刻载入"：答案常常一次往返就回来，灯跟着一闪而过，
    // 用户看不到"它在连"，只觉得界面抖了一下。所以这是**最短停留**，不是超时。
    expect(HARNESS_SWITCH_BUFFER_MS).toBeGreaterThanOrEqual(1_000)
    expect(HARNESS_SWITCH_BUFFER_MS).toBeLessThanOrEqual(2_000)
  })

  it('loads the minimal preset by default, and passes a chosen one through', async () => {
    const { DEFAULT_HARNESS_PRESET, harnessAdapterOptionsFromSettings } = await import('../src/App.tsx')

    // 端点**不在这里**：`connect_harness` 命令自己按主体范围解析端口（"不信任调用方"），渲染端
    // 曾经算过一个端口交出去，命令根本不看它 —— 死参数已删，所以这里也不该再出现它。
    const fallback = harnessAdapterOptionsFromSettings({}, 'deepseek-chat')
    expect(fallback.preset).toBe('minimal')
    expect(DEFAULT_HARNESS_PRESET).toBe('minimal')
    expect(fallback.model).toBe('deepseek-chat')
    expect('endpointPort' in fallback).toBe(false)

    // 设置里写了就用写的那个（"设置里是什么就是什么"）。
    expect(harnessAdapterOptionsFromSettings({ harnessPreset: 'standard' }, undefined).preset).toBe('standard')
  })

  it('applies daily policy only when unlocking after a local calendar rollover', async () => {
    const { chatAdapterLifecycleKey, shouldIgnoreUnpairedResume, shouldStartNewConversationOnUnlock, subjectScopeKey } = await import('../src/App.tsx')
    const scope = subjectScopeKey({ subjectId: 'cli:C:/npm/dsh.cmd', endpointPort: 3080 })
    const beforeMidnight = new Date(2026, 7, 18, 23, 59, 0)
    const afterMidnight = new Date(2026, 7, 19, 0, 1, 0)
    const afterBoundary = new Date(2026, 7, 19, 4, 1, 0)

    // Changing a policy in Settings changes neither adapter identity nor an
    // in-flight stream. The daily decision belongs to the later unlock event.
    expect(chatAdapterLifecycleKey('deepseek-api', 7, scope)).toBe(chatAdapterLifecycleKey('deepseek-api', 7, scope))
    expect(shouldStartNewConversationOnUnlock('daily', '2026-08-18', beforeMidnight)).toBe(false)
    // 「助手日」的边界是 04:00（用户定的规则）：00:01 仍属**前一天** —— 深夜还在做的事不该被零点
    // 从中间切走；到 04:01 才真的换新的一天 ✓。边界小时可配，0 = 退回"零点跨日"的旧行为。
    expect(shouldStartNewConversationOnUnlock('daily', '2026-08-18', afterMidnight)).toBe(false)
    expect(shouldStartNewConversationOnUnlock('daily', '2026-08-18', afterBoundary)).toBe(true)
    expect(shouldStartNewConversationOnUnlock('daily', '2026-08-18', afterMidnight, 0)).toBe(true)
    expect(shouldStartNewConversationOnUnlock('new-on-unlock', '2026-08-19', afterMidnight)).toBe(true)
    expect(shouldStartNewConversationOnUnlock('resume-last', '2026-08-18', afterBoundary)).toBe(false)
    expect(shouldIgnoreUnpairedResume(false, 'resume')).toBe(true)
    expect(shouldIgnoreUnpairedResume(false, 'unlocked')).toBe(false)
    expect(shouldIgnoreUnpairedResume(true, 'resume')).toBe(false)
  })

  it('persists an API session as soon as send has allocated its conversation ID', async () => {
    const { disposeChatAdapter, persistConversationPointerWhenAvailable } = await import('../src/App.tsx')
    const writes: Array<{ backend: BackendMode; id: string }> = []
    const apiWithAllocatedId = {
      ...adapter('deepseek-api'),
      conversationId: () => 'api-created-before-native-await',
    }

    // This is deliberately invoked without waiting for a send promise. It
    // models the narrow interval where NativeChatAdapter has synchronously
    // allocated its UUID but the native request is still pending.
    expect(persistConversationPointerWhenAvailable(
      apiWithAllocatedId,
      'deepseek-api',
      (backend, id) => writes.push({ backend, id }),
    )).toBe('api-created-before-native-await')
    expect(writes).toEqual([{ backend: 'deepseek-api', id: 'api-created-before-native-await' }])

    // Effect teardown uses the same helper; switching to another backend must
    // not write the pending API ID under that new backend's pointer.
    expect(persistConversationPointerWhenAvailable(
      apiWithAllocatedId,
      'harness',
      (backend, id) => writes.push({ backend, id }),
    )).toBeUndefined()
    expect(writes).toHaveLength(1)

    let unsubscribed = false
    let disconnected = false
    const teardownAdapter = {
      ...apiWithAllocatedId,
      disconnect: () => { disconnected = true },
    }
    disposeChatAdapter(
      teardownAdapter,
      'deepseek-api',
      () => { unsubscribed = true },
      (backend, id) => writes.push({ backend, id }),
    )
    expect(writes.at(-1)).toEqual({ backend: 'deepseek-api', id: 'api-created-before-native-await' })
    expect(unsubscribed).toBe(true)
    expect(disconnected).toBe(true)
  })

  it('makes the real native API session ID available before its native request settles', async () => {
    const originalSendChat = nativeRuntime.sendChat
    let settleRequest: ((id: string | undefined) => void) | undefined
    nativeRuntime.sendChat = async () => new Promise<string | undefined>((resolve) => { settleRequest = resolve })
    try {
      const adapter = new NativeChatAdapter('deepseek-api')
      const pending = adapter.send('persist this before a backend switch')
      const id = adapter.conversationId()
      const writes: Array<{ backend: BackendMode; id: string }> = []
      const { persistConversationPointerWhenAvailable } = await import('../src/App.tsx')

      expect(id).toMatch(/.+/)
      expect(persistConversationPointerWhenAvailable(
        adapter,
        'deepseek-api',
        (backend, conversationId) => writes.push({ backend, id: conversationId }),
      )).toBe(id)
      expect(writes).toEqual([{ backend: 'deepseek-api', id }])

      settleRequest?.(id)
      await pending
    } finally {
      nativeRuntime.sendChat = originalSendChat
    }
  })
})
