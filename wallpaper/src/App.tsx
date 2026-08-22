import { useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { PreviewAdapter } from './chat/mockAdapter.ts'
import { NativeChatAdapter } from './chat/nativeAdapter.ts'
import { DeepSeekWebAdapter } from './chat/deepseekWebAdapter.ts'
import type { ChatAdapter } from './chat/adapter.ts'
import { ConversationBubble } from './chat/ConversationBubble.tsx'
import type { BackendMode, ChatMessage, RuntimeState, TokenUsage } from './domain/types.ts'
import { personaIdFor, resolveModelTier } from './domain/modelTier.ts'
import { isHarnessReady, monitorHarness } from './connect/harness.ts'
import { PersonaRegistry } from './persona/registry.ts'
import { IdleScene } from './scenes/IdleScene.tsx'
import { SleepScene } from './scenes/SleepScene.tsx'
import { WakeScene } from './scenes/WakeScene.tsx'
import { INITIAL_RUNTIME_STATE, reduceRuntime } from './scenes/stateMachine.ts'
import { BACKGROUND_OPTIONS, applyBubbleOverrides, assetUrl, loadSettings, localCalendarDay, resumeConversationId, saveConversationPointer, type WallpaperSettings } from './settings/store.ts'
import { nativeRuntime, type NativeSendOptions } from './native/runtime.ts'
import { listen } from '@tauri-apps/api/event'
import type { AppSurface } from './surface.ts'
import { beginInteractionRegionSession, collectInteractionRegions, publishInteractionRegions } from './runtime/interactionRegions.ts'
import type { AppearanceSlot } from './appearance/theme/index.ts'
import { nativeAppearance } from './native/appearance.ts'
import { appCoreClient } from './runtime/appCoreClient.ts'
import type { DesktopWorkspace } from './runtime/desktopWorkspace.ts'
import { WidgetHost } from './widgets/WidgetHost.tsx'

const registry = new PersonaRegistry()

/**
 * A chat adapter can finish connecting, loading history, or sending a message
 * after React has already selected another backend.  Treat the adapter object
 * and the backend it was created for as one identity; neither is sufficient on
 * its own because an old native call may complete after a new adapter mounts.
 */
export function isCurrentChatOperation(
  activeAdapter: ChatAdapter,
  activeBackend: BackendMode,
  candidateAdapter: ChatAdapter,
  candidateBackend: BackendMode,
  disposed: boolean,
): boolean {
  return !disposed
    && activeAdapter === candidateAdapter
    && activeBackend === candidateBackend
    && candidateAdapter.mode === candidateBackend
}

export const HARNESS_DISCONNECTED_ERROR_PREFIX = 'DSH 壁纸 Bridge 当前不可用。'

export function canAutoSelectHarness(
  availability: RuntimeState['harness'],
  backend: BackendMode,
  autoSwitchHarness: boolean,
): boolean {
  return isHarnessReady(availability) && backend !== 'harness' && autoSwitchHarness
}

/**
 * A 3080 response alone is not a usable Harness transport. Keep every
 * renderer-side selection path behind the same compatible-Bridge predicate.
 */
export function canSelectBackend(
  availability: RuntimeState['harness'],
  backend: BackendMode,
): boolean {
  return backend !== 'harness' || isHarnessReady(availability)
}

export function harnessSelectionUnavailableError(availability: RuntimeState['harness']): string {
  const detail = availability === 'web-only'
    ? '检测到 DSH 服务，但壁纸 Bridge 未安装、未启动或不兼容。'
    : '未能连接到本机的 DSH 壁纸 Bridge。'
  return `${HARNESS_DISCONNECTED_ERROR_PREFIX}${detail} Harness 模式只能在兼容 Bridge 就绪后切换。`
}

/**
 * `NativeChatAdapter` retains the options object it is constructed with and
 * takes a value snapshot only when it starts a request.  Keep that object
 * stable for the lifetime of the API adapter: editing an API endpoint, model,
 * or price must affect the *next* request, never sever the event subscription
 * for a response that is already streaming.
 */
export function apiAdapterOptionsFromSettings(
  settings: Pick<WallpaperSettings, 'deepseekApi'>,
): NativeSendOptions {
  return {
    baseUrl: settings.deepseekApi.baseUrl,
    model: settings.deepseekApi.model,
    priceInputPerMillion: settings.deepseekApi.priceInputPerMillion,
    priceOutputPerMillion: settings.deepseekApi.priceOutputPerMillion,
  }
}

/** Mutates the stable API options holder used by the mounted adapter. */
export function updateApiAdapterOptions(
  options: NativeSendOptions,
  settings: Pick<WallpaperSettings, 'deepseekApi'>,
): NativeSendOptions {
  Object.assign(options, apiAdapterOptionsFromSettings(settings))
  return options
}

type ConversationPointerAdapter = ChatAdapter & {
  conversationId?: () => string | undefined
}

/**
 * Native API sessions receive their UUID before the native async request is
 * awaited.  Persist it at every lifecycle boundary instead of waiting for a
 * successful response, otherwise a backend switch can orphan a just-started
 * transcript from the resume pointer.
 */
export function persistConversationPointerWhenAvailable(
  adapter: ConversationPointerAdapter,
  backend: BackendMode,
  savePointer: (backend: BackendMode, id: string) => void = saveConversationPointer,
): string | undefined {
  if (adapter.mode !== backend || typeof adapter.conversationId !== 'function') return undefined
  const id = adapter.conversationId()
  if (!id) return undefined
  savePointer(backend, id)
  return id
}

/**
 * Keep teardown ordering explicit and testable: an adapter that has already
 * allocated its API UUID gets a resume pointer before its event listener is
 * removed.  This also covers an effect replacement caused by a backend change.
 */
export function disposeChatAdapter(
  adapter: ConversationPointerAdapter,
  backend: BackendMode,
  unsubscribe: () => void,
  savePointer: (backend: BackendMode, id: string) => void = saveConversationPointer,
): void {
  persistConversationPointerWhenAvailable(adapter, backend, savePointer)
  unsubscribe()
  adapter.disconnect()
}

/**
 * Adapter replacement is deliberately a much narrower event than a settings
 * update. A live API request owns its listener and request ID until a backend
 * switch or an explicit new-conversation generation replaces it. In
 * particular, editing the conversation policy only affects a later unlock;
 * it must not disconnect a response currently streaming.
 */
export function chatAdapterLifecycleKey(
  backend: BackendMode,
  conversationGeneration: number,
): string {
  return `${backend}:${conversationGeneration}`
}

/**
 * Daily sessions roll over on a real session return, rather than on an
 * arbitrary timer. This is important for a resident wallpaper: it may stay
 * alive across midnight with yesterday's adapter still mounted.
 */
export function shouldStartNewConversationOnUnlock(
  policy: WallpaperSettings['conversationPolicy'],
  previousUnlockDay: string,
  now: Date = new Date(),
): boolean {
  return policy === 'new-on-unlock'
    || (policy === 'daily' && previousUnlockDay !== localCalendarDay(now))
}

/**
 * `register_session_events` emits an initial `resume` while the background
 * WebView is starting. A resume is a policy boundary only after this process
 * has observed the matching suspend; otherwise it may be that synthetic
 * startup signal and must not create a duplicate fresh session at boot.
 */
export function shouldIgnoreUnpairedResume(
  observedLockOrSuspend: boolean,
  event: 'locked' | 'unlocked' | 'suspend' | 'resume',
): boolean {
  return event === 'resume' && !observedLockOrSuspend
}

/**
 * Dropping the bridge must never silently move a user to another backend: the
 * current transcript and session pointer remain meaningful when DSH returns.
 * The Bubble is disabled from the existing availability prop while this
 * controlled notice tells the user how to proceed.
 */
export function harnessAvailabilityPatch(
  backend: BackendMode,
  availability: RuntimeState['harness'],
  currentError?: string,
): Pick<RuntimeState, 'activity' | 'error'> | undefined {
  if (backend !== 'harness') return undefined
  if (isHarnessReady(availability)) {
    return currentError?.startsWith(HARNESS_DISCONNECTED_ERROR_PREFIX)
      ? { activity: 'idle', error: undefined }
      : undefined
  }
  if (currentError?.startsWith(HARNESS_DISCONNECTED_ERROR_PREFIX)) return undefined

  const detail = availability === 'web-only'
    ? '检测到 DSH 服务，但壁纸 Bridge 未安装、未启动或不兼容。'
    : '未能连接到本机的 DSH 壁纸 Bridge。'
  return {
    activity: 'idle',
    error: `${HARNESS_DISCONNECTED_ERROR_PREFIX}${detail} 已保留当前 Harness 会话和对话记录；Bridge 恢复后可继续，或由你手动切换后端。`,
  }
}

export interface AppProps { surface?: AppSurface }

export function App({ surface = 'combined' }: AppProps) {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [runtime, baseDispatch] = useReducer(reduceRuntime, { ...INITIAL_RUNTIME_STATE, backend: settings.defaultBackend })
  const [resolvedAssets, setResolvedAssets] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [streamingText, setStreamingText] = useState('')
  const [usage, setUsage] = useState<TokenUsage>()
  const [conversationGeneration, setConversationGeneration] = useState(0)
  const [apiModelChoice, setApiModelChoice] = useState(settings.deepseekApi.model)
  const [harnessModelChoice, setHarnessModelChoice] = useState<string | undefined>()
  const [interactionState, setInteractionState] = useState<'collapsed' | 'expanded'>('collapsed')
  const [interactionEnabled, setInteractionEnabled] = useState(true)
  const [workspace, setWorkspace] = useState<DesktopWorkspace>('front')
  const [innerHistoryExpanded, setInnerHistoryExpanded] = useState(false)
  const [expandedBottomInset, setExpandedBottomInset] = useState(48)
  const [presetOptions, setPresetOptions] = useState<Array<{ id: string; name?: string; broken?: string; isDefault: boolean }>>([])
  const [selectedPreset, setSelectedPreset] = useState<string>()
  const adapterRef = useRef<ChatAdapter>(new PreviewAdapter(settings.defaultBackend))
  // Do not put mutable API request settings in the adapter lifecycle effect.
  // The adapter captures this object by reference and snapshots it only when
  // sending, so a settings edit changes the next request without disconnecting
  // an in-flight stream or dropping its scoped events.
  const apiAdapterOptionsRef = useRef<NativeSendOptions>(apiAdapterOptionsFromSettings(settings))
  const conversationPolicyRef = useRef(settings.conversationPolicy)
  // Keep the day from the last session return, not from the last render. A
  // long-running process therefore notices local midnight when the user
  // returns to the desktop and asks for a daily conversation.
  const previousUnlockDayRef = useRef(localCalendarDay())
  // This ref is updated synchronously by user/backend actions. React state is
  // intentionally asynchronous, so runtimeRef alone would leave a short gap
  // in which an old adapter could finish and persist its pointer under a new
  // backend selection.
  const activeBackendRef = useRef<BackendMode>(runtime.backend)
  activeBackendRef.current = runtime.backend
  const runtimeRef = useRef(runtime)
  runtimeRef.current = runtime
  // Resolving library data URLs can finish out of order.  Each refresh gets a
  // monotonic epoch, so a slower pre-change resolve can never repaint the
  // previous background/persona after a user has selected a new one.
  const appearanceRefreshEpochRef = useRef(0)
  const patchRuntime = (patch: Partial<RuntimeState>) => baseDispatch({ type: 'PATCH', patch })
  const dispatchCore = (action: Parameters<typeof appCoreClient.dispatch>[0], options?: Parameters<typeof appCoreClient.dispatch>[1]) => {
    if (!appCoreClient.native) return
    void appCoreClient.dispatch(action, options).catch((error) => patchRuntime({ error: String(error) }))
  }

  const tier = resolveModelTier(runtime.backend, runtime.provider, runtime.model, settings.modelTierRules, runtime.modelTier)
  const persona = registry.get(personaIdFor(runtime.backend, tier))
  const bubbles = applyBubbleOverrides(persona.bubbles, settings.bubbleOverrides)
  const background = BACKGROUND_OPTIONS.find((item) => item.id === settings.background)
  const personaSlot: AppearanceSlot = runtime.backend === 'harness'
    ? tier === 'pro' ? 'persona.harness.pro' : 'persona.harness.flash'
    : tier === 'pro' ? 'persona.deepseek.pro' : 'persona.deepseek.flash'
  const resolvedPersona = resolvedAssets[personaSlot]
  const resolvedBackground = resolvedAssets['desktop.background']
  const modelLabel = runtime.model ?? (tier === 'pro' ? 'Pro · 成年形态' : 'Flash · 幼年形态')
  const modelOptions = useMemo(() => {
    const unique = (models: Array<string | undefined>) => [...new Set(models.filter((model): model is string => Boolean(model?.trim())))]
    if (runtime.backend === 'deepseek-api') return unique([apiModelChoice, settings.deepseekApi.model, 'deepseek-chat', 'deepseek-reasoner'])
    if (runtime.backend === 'harness') return unique([harnessModelChoice, runtime.model, 'deepseek-v4-flash-vision-exp', 'deepseek-v4-flash', 'deepseek-v4-pro'])
    return unique([runtime.model])
  }, [apiModelChoice, harnessModelChoice, runtime.backend, runtime.model, settings.deepseekApi.model])
  const selectedModel = runtime.backend === 'deepseek-api'
    ? apiModelChoice
    : runtime.backend === 'harness'
      ? harnessModelChoice ?? runtime.model ?? modelOptions[0]
      : runtime.model
  // The WorkerW host is permanently desktop-sized. Both the floating window
  // and the taskbar capsule now use CSS placement inside that one viewport.
  const interactionDirection = 'center' as const
  const adapterLifecycleKey = chatAdapterLifecycleKey(runtime.backend, conversationGeneration)

  // Commit settings into the stable holder after React commits the matching
  // render.  Mutating the ref during render could leak a discarded concurrent
  // render's configuration into an in-flight request.
  useEffect(() => {
    updateApiAdapterOptions(apiAdapterOptionsRef.current, settings)
    setApiModelChoice(settings.deepseekApi.model)
    conversationPolicyRef.current = settings.conversationPolicy
  }, [
    settings.conversationPolicy,
    settings.deepseekApi.baseUrl,
    settings.deepseekApi.model,
    settings.deepseekApi.priceInputPerMillion,
    settings.deepseekApi.priceOutputPerMillion,
  ])

  const enterInnerWorkspace = () => {
    // The existing drawer already has its own state and visual treatment. The
    // preference therefore only chooses its initial state as a workspace is
    // entered; it does not add another surface or force a transcript open.
    setInnerHistoryExpanded(settings.historyStartsExpanded)
    setWorkspace('entering-inner')
    setInteractionState('expanded')
    baseDispatch({ type: 'OPEN_CHAT' })
    dispatchCore('open-chat')
    window.setTimeout(() => setWorkspace('inner'), 280)
  }

  const leaveInnerWorkspace = () => {
    setInnerHistoryExpanded(false)
    if (settings.interactionLayout === 'floating') {
      // A floating surface is either fully present or absent. Resizing its native
      // HWND during a CSS exit animation exposes partially clipped WebView frames.
      setWorkspace('front')
      setInteractionState('expanded')
      baseDispatch({ type: 'CLOSE_CHAT' })
      dispatchCore('close-chat')
      return
    }
    setWorkspace('leaving-inner')
    window.setTimeout(() => {
      setWorkspace('front')
      setInteractionState('collapsed')
      baseDispatch({ type: 'CLOSE_CHAT' })
      dispatchCore('close-chat')
    }, 220)
  }

  const refreshAppearance = async () => {
    const refreshEpoch = ++appearanceRefreshEpochRef.current
    try {
      const slots: AppearanceSlot[] = [
        'desktop.background',
        'persona.deepseek.flash', 'persona.deepseek.pro',
        'persona.harness.flash', 'persona.harness.pro',
      ]
      const resolved = await Promise.all(slots.map(async (slot) => [slot, await nativeAppearance.resolveAsset(slot)] as const))
      if (refreshEpoch !== appearanceRefreshEpochRef.current) return
      setResolvedAssets(Object.fromEntries(resolved.filter((entry) => Boolean(entry[1]))))
    } catch (error) {
      patchRuntime({ error: String(error) })
    }
  }

  useEffect(() => {
    if (!appCoreClient.native) {
      const timer = setTimeout(() => baseDispatch({ type: 'BOOT_READY', playWake: settings.animationsEnabled && !settings.skipWakeAnimation }), 120)
      return () => clearTimeout(timer)
    }
    let unsubscribe: () => void = () => undefined
    const applySnapshot = (snapshot: Awaited<ReturnType<typeof appCoreClient.snapshot>>) => {
      // Native tray/system actions can change the selected backend without
      // going through this WebView's changeBackend callback. Advance the
      // synchronous identity first so an old async adapter cannot win during
      // React's next render/cleanup boundary.
      activeBackendRef.current = snapshot.backend
      patchRuntime({
        phase: snapshot.phase,
        backend: snapshot.backend,
        activity: snapshot.activity,
        harness: snapshot.harness,
        historyExpanded: snapshot.interaction.historyExpanded,
        error: snapshot.error,
      })
      setInteractionEnabled(snapshot.interaction.enabled)
      if (snapshot.phase === 'chatting') {
        setWorkspace((current) => current === 'front' || current === 'leaving-inner' ? 'inner' : current)
        setInteractionState('expanded')
      }
      if (!snapshot.interaction.desktopForeground || snapshot.privacyScreen) {
        if (settings.interactionLayout === 'taskbar-docked') setInteractionState('collapsed')
      }
    }
    void Promise.all([
      appCoreClient.snapshot().then(applySnapshot),
      appCoreClient.subscribe(applySnapshot).then((dispose) => { unsubscribe = dispose }),
    ])
    const timer = setTimeout(() => dispatchCore('boot-ready', { playWake: settings.animationsEnabled && !settings.skipWakeAnimation }), 120)
    return () => {
      clearTimeout(timer)
      unsubscribe()
    }
  }, [settings.animationsEnabled, settings.interactionLayout, settings.skipWakeAnimation, surface])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let dispose: () => void = () => undefined
    void listen<WallpaperSettings>('settings-changed', (event) => {
      // Settings are authored in a separate WebView. The payload is the
      // source of truth; localStorage here belongs only to this WebView.
      setSettings(event.payload)
    }).then((unlisten) => { dispose = unlisten })
    return () => dispose()
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative || runtime.harness !== 'bridge-ready') return
    void nativeRuntime.harnessPresets().then((presets) => {
      setPresetOptions(presets)
      setSelectedPreset((current) => current ?? presets.find((preset) => preset.isDefault)?.id ?? presets[0]?.id)
    }).catch(() => setPresetOptions([]))
  }, [runtime.harness])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    const refresh = () => { void nativeRuntime.desktopLayoutMetrics().then((metrics) => { if (!disposed) setExpandedBottomInset(metrics.expandedBottomInset) }) }
    refresh()
    window.addEventListener('resize', refresh)
    const timer = window.setInterval(refresh, 1000)
    return () => { disposed = true; window.removeEventListener('resize', refresh); window.clearInterval(timer) }
  }, [])

  useEffect(() => {
    const adapterBackend = runtime.backend
    const conversationPolicy = conversationPolicyRef.current
    const adapter: ChatAdapter = nativeRuntime.isNative
      ? adapterBackend === 'deepseek-web'
        ? new DeepSeekWebAdapter()
        : new NativeChatAdapter(
            adapterBackend,
            adapterBackend === 'deepseek-api'
              ? apiAdapterOptionsRef.current
              : adapterBackend === 'harness' && harnessModelChoice
                ? { model: harnessModelChoice }
                : {},
            // Harness owns its own daily workspace/session lifecycle. Never
            // feed it a renderer-local resume pointer, which could belong to
            // an unrelated DSH project or an old bridge contract.
            adapterBackend === 'harness'
              ? undefined
              : adapterBackend === 'deepseek-api'
                ? resumeConversationId('deepseek-api', conversationPolicy)
                : undefined,
          )
      : new PreviewAdapter(adapterBackend)
    adapterRef.current.disconnect(); adapterRef.current = adapter; setMessages([]); setStreamingText(''); setUsage(undefined)
    let disposed = false
    const isCurrent = () => isCurrentChatOperation(
      adapterRef.current,
      activeBackendRef.current,
      adapter,
      adapterBackend,
      disposed,
    )
    const unsubscribe = adapter.subscribe((event) => {
      if (!isCurrent()) return
      if (event.type === 'status') { patchRuntime({ activity: event.activity }); dispatchCore('set-activity', { value: event.activity }) }
      if (event.type === 'delta') { patchRuntime({ activity: 'streaming' }); dispatchCore('set-activity', { value: 'streaming' }); setStreamingText((value) => value + event.text) }
      if (event.type === 'message') {
        setMessages((items) => [...items, { id: crypto.randomUUID(), role: event.role, content: event.content, createdAt: Date.now(), usage: event.usage }])
        // Harness and future providers may attach final usage directly to the
        // final message instead of publishing a separate usage event.
        if (event.usage) setUsage(event.usage)
        if (event.role === 'assistant') setStreamingText('')
      }
      if (event.type === 'usage') setUsage(event)
      if (event.type === 'model') patchRuntime({ model: event.model, provider: event.provider, modelTier: event.tier, reasoningEffort: event.effort })
      if (event.type === 'auth-required') { baseDispatch({ type: 'AUTH_REQUIRED' }); dispatchCore('auth-required') }
      if (event.type === 'approval-required') { patchRuntime({ activity: 'tool', error: `${event.summary}；请打开 Harness 处理。` }); dispatchCore('set-activity', { value: 'tool' }) }
      if (event.type === 'error') patchRuntime({ activity: 'idle', error: event.message })
    })
    void (async () => {
      try {
        await adapter.connect()
        if (!isCurrent()) return
        persistConversationPointerWhenAvailable(adapter, adapterBackend)
        const history = await adapter.history()
        if (isCurrent() && history.length) {
          setMessages(history)
          // A restored transcript has no live `usage` event. Recover the
          // newest provider usage so the footer still describes the current
          // conversation until the next user turn clears it.
          const latestUsage = [...history].reverse().find((message) => message.usage)?.usage
          setUsage(latestUsage)
        }
      } catch (error) {
        if (isCurrent()) patchRuntime({ activity: 'idle', error: String(error) })
      }
    })()
    return () => {
      // `send()` creates an API session UUID synchronously, before its native
      // Promise resolves. Persist it before disconnecting so a backend switch,
      // unlock reset, or React effect teardown cannot orphan that transcript.
      disposed = true
      disposeChatAdapter(adapter, adapterBackend, unsubscribe)
    }
  }, [
    adapterLifecycleKey,
    harnessModelChoice,
  ])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let unsubscribe: () => void = () => undefined
    let observedLockOrSuspend = false
    void nativeRuntime.listenSystem((event) => {
      // The native host emits one synthetic `resume` on registration so a
      // current desktop can initialize its visual state. Do not mistake that
      // boot-time signal for an unlock policy boundary.
      if (shouldIgnoreUnpairedResume(observedLockOrSuspend, event)) {
        return
      }
      if (event === 'locked' || event === 'suspend') observedLockOrSuspend = true
      if (!appCoreClient.native && (event === 'locked' || event === 'suspend')) baseDispatch({ type: 'LOCK' })
      if (event === 'unlocked' || event === 'resume') {
        if (settings.interactionLayout === 'taskbar-docked') setInteractionState('collapsed')
        const now = new Date()
        const policy = conversationPolicyRef.current
        if (shouldStartNewConversationOnUnlock(policy, previousUnlockDayRef.current, now)) {
          setConversationGeneration((value) => value + 1)
        }
        previousUnlockDayRef.current = localCalendarDay(now)
        observedLockOrSuspend = false
        if (!appCoreClient.native) baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation })
      }
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [settings.animationsEnabled, settings.interactionLayout, settings.playWakeOnEveryUnlock, settings.skipWakeAnimation])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let unsubscribe: () => void = () => undefined
    void nativeRuntime.listenTray((event) => {
      changeBackend(event.backend)
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let dispose: () => void = () => undefined
    void import('@tauri-apps/api/event').then(({ listen }) => listen<'enter' | 'leave'>('desktop-workspace-toggle', (event) => {
      if (event.payload === 'enter') enterInnerWorkspace()
      else leaveInnerWorkspace()
    })).then((unlisten) => { dispose = unlisten })
    return () => dispose()
  }, [settings.interactionLayout])

  useEffect(() => {
    if (!nativeAppearance.isNative) return
    void refreshAppearance()
  }, [surface])

  useEffect(() => {
    if (!nativeAppearance.isNative) return
    let dispose: () => void = () => undefined
    void listen('appearance-changed', () => { void refreshAppearance() }).then((unlisten) => { dispose = unlisten })
    return () => dispose()
  }, [surface])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    let session: number | undefined
    let revision = 0
    let frame = 0
    const publish = () => {
      if (session === undefined) return
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        void publishInteractionRegions({
          session: session!,
          revision: ++revision,
          scaleFactor: window.devicePixelRatio || 1,
          regions: collectInteractionRegions(),
        })
      })
    }
    const observer = new MutationObserver(publish)
    void beginInteractionRegionSession().then((value) => {
      if (disposed) return
      session = value
      observer.observe(document.body, { attributes: true, childList: true, subtree: true })
      window.addEventListener('resize', publish)
      publish()
    }).catch((error) => patchRuntime({ error: String(error) }))
    return () => {
      disposed = true
      observer.disconnect()
      window.removeEventListener('resize', publish)
      cancelAnimationFrame(frame)
      if (session !== undefined) {
        void publishInteractionRegions({ session, revision: ++revision, scaleFactor: window.devicePixelRatio || 1, regions: [] })
      }
    }
  }, [])

  useEffect(() => {
    // Native builds receive the debounced Harness availability from the single Rust monitor
    // through AppSnapshot. Starting a second browser-side monitor here would duplicate every
    // 3080 probe for each WebView.
    if (appCoreClient.native) return
    const monitor = monitorHarness((status) => {
      patchRuntime({ harness: status.availability, model: status.model ?? runtimeRef.current.model, provider: status.provider ?? runtimeRef.current.provider, reasoningEffort: status.reasoningEffort })
      if (isHarnessReady(status.availability) && runtimeRef.current.backend !== 'harness') {
        if (canAutoSelectHarness(status.availability, runtimeRef.current.backend, settings.autoSwitchHarness)) changeBackend('harness')
      }
      const disconnected = harnessAvailabilityPatch(runtimeRef.current.backend, status.availability, runtimeRef.current.error)
      if (disconnected) patchRuntime(disconnected)
    })
    return () => monitor.stop()
  }, [settings.autoSwitchHarness])

  useEffect(() => {
    if (!appCoreClient.native) return
    if (isHarnessReady(runtime.harness) && runtime.backend !== 'harness') {
      if (canAutoSelectHarness(runtime.harness, runtime.backend, settings.autoSwitchHarness)) changeBackend('harness')
    }
    const disconnected = harnessAvailabilityPatch(runtime.backend, runtime.harness, runtime.error)
    if (disconnected) patchRuntime(disconnected)
  }, [runtime.harness, runtime.backend, runtime.error, settings.autoSwitchHarness])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === 'w') { baseDispatch({ type: 'LOCK' }); dispatchCore('lock') }
      if (event.key === 'Escape') {
        if (runtime.phase === 'locked') { baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }); dispatchCore('unlock', { playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }) }
      }
    }
    window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey)
  }, [runtime.phase, settings])

  const changeBackend = (backend: WallpaperSettings['defaultBackend']) => {
    if (!canSelectBackend(runtimeRef.current.harness, backend)) {
      // Do not construct a native Harness adapter or issue a Tauri selection
      // for a bare port-3080 observation. Leave the current transcript and
      // backend intact until the Bridge contract is actually ready.
      patchRuntime({ activity: 'idle', error: harnessSelectionUnavailableError(runtimeRef.current.harness) })
      return
    }
    // Clear backend-scoped UI immediately. The effect below repeats this while
    // creating the next adapter, which prevents one paint of API usage or a
    // partial answer under the newly selected backend label.
    setMessages([])
    setStreamingText('')
    setUsage(undefined)
    activeBackendRef.current = backend
    patchRuntime({ backend })
    baseDispatch({ type: 'RECOVER' })
    if (appCoreClient.native) void appCoreClient.selectBackend(backend).catch((error) => patchRuntime({ error: String(error) }))
  }
  const scene = useMemo(() => {
    if (runtime.phase === 'booting' || runtime.phase === 'locked') return <SleepScene persona={persona} mode="system" />
    if (runtime.phase === 'waking') return <WakeScene persona={persona} enabled={settings.animationsEnabled && !settings.skipWakeAnimation} speed={settings.animationSpeed} onWakeDone={() => { baseDispatch({ type: 'WAKE_DONE' }); dispatchCore('wake-done') }} />
    return <IdleScene persona={{ ...persona, bubbles, assets: { ...persona.assets, portrait: resolvedPersona ?? persona.assets.portrait } }} bubbleText={runtime.activity === 'thinking' ? '正在认真思考…' : bubbles.morning} backgroundUrl={resolvedBackground ?? (background?.path ? assetUrl(background.path) : undefined)} portraitAmbientLength={settings.portraitAmbientLength} portraitAmbientStrength={settings.portraitAmbientStrength} hideBubble={workspace !== 'front'} onOpenChat={enterInnerWorkspace} />
  }, [background?.path, bubbles, persona, resolvedBackground, resolvedPersona, runtime, settings, workspace])

  return <div className={`wallpaper-root surface-${surface} effort-${runtime.reasoningEffort ?? 'normal'} workspace-${workspace}`} data-workspace={workspace}>
    {scene}
    <>
      <WidgetHost workspace={workspace} widgets={[]} />
      {interactionEnabled && runtime.phase !== 'booting' && runtime.phase !== 'locked' && (settings.interactionLayout === 'taskbar-docked' || workspace !== 'front') && <ConversationBubble backend={runtime.backend} activity={runtime.activity} modelLabel={modelLabel} messages={messages} streamingText={streamingText} historyExpanded={workspace === 'front' ? runtime.historyExpanded : innerHistoryExpanded} usage={usage} collapsed={settings.interactionLayout === 'taskbar-docked' && interactionState === 'collapsed'} layout={settings.interactionLayout} expandDirection={interactionDirection} persistent={settings.interactionLayout === 'floating'} acrylicOpacity={settings.conversationOpacity} acrylicBlur={settings.conversationBlur} expandedBottomInset={expandedBottomInset} apiPricingConfigured={settings.deepseekApi.priceInputPerMillion !== undefined && settings.deepseekApi.priceOutputPerMillion !== undefined} harnessAvailability={runtime.harness} onSelectBackend={changeBackend} presetOptions={presetOptions} selectedPreset={selectedPreset} onSelectPreset={messages.length === 0 ? setSelectedPreset : undefined} modelOptions={modelOptions} selectedModel={selectedModel} onSelectModel={runtime.backend === 'deepseek-web' ? undefined : (model) => { if (runtime.backend === 'deepseek-api') { setApiModelChoice(model); apiAdapterOptionsRef.current.model = model; patchRuntime({ model }); } else { setHarnessModelChoice(model); setConversationGeneration((value) => value + 1); patchRuntime({ model, provider: 'deepseek-official', activity: 'idle' }); } }} onExpand={() => { setInteractionState('expanded'); if (workspace === 'front') enterInnerWorkspace(); else { baseDispatch({ type: 'OPEN_CHAT' }); dispatchCore('open-chat') } }} disabled={runtime.backend === 'harness' && runtime.harness !== 'bridge-ready'} onToggleHistory={() => { if (workspace !== 'front') setInnerHistoryExpanded((value) => !value); else { baseDispatch({ type: 'TOGGLE_HISTORY' }); dispatchCore('toggle-history') } }} onSend={(text) => {
        const adapter = adapterRef.current
        const adapterBackend = adapter.mode
        const isCurrent = () => isCurrentChatOperation(adapterRef.current, activeBackendRef.current, adapter, adapterBackend, false)
        if (!isCurrent()) return
        // The footer describes the turn being sent, never stale metrics from
        // its predecessor. It changes to an explicit waiting/unavailable
        // state until a provider supplies fresh usage.
        setUsage(undefined)
        const sending = adapter.send(text)
        // NativeChatAdapter allocates an API conversation ID before its first
        // await. Saving immediately survives a settings update or backend
        // switch while the request is still pending.
        persistConversationPointerWhenAvailable(adapter, adapterBackend)
        void sending.then(() => {
          if (!isCurrent()) return
          persistConversationPointerWhenAvailable(adapter, adapterBackend)
        }).catch((error) => {
          if (isCurrent()) patchRuntime({ activity: 'idle', error: String(error) })
        })
      }} onStop={() => {
        const adapter = adapterRef.current
        const adapterBackend = adapter.mode
        const isCurrent = () => isCurrentChatOperation(adapterRef.current, activeBackendRef.current, adapter, adapterBackend, false)
        if (!isCurrent()) return
        void adapter.stop().catch((error) => {
          if (isCurrent()) patchRuntime({ activity: 'idle', error: String(error) })
        })
      }} onClose={() => undefined} />}
      {runtime.error && runtime.phase !== 'error' && <div className="runtime-notice" role="status">{runtime.error}<button onClick={() => patchRuntime({ error: undefined })}>×</button></div>}
      {runtime.phase === 'auth-required' && <div className="auth-overlay" data-interaction-region="auth"><div className="auth-card"><h2>DeepSeek 网页模式（实验入口）</h2><p>已通过默认浏览器打开 DeepSeek 官方页面。当前尚未启用持久 WebView2 或 DOM 消息桥接：本应用不读取 Cookie，不能使用或保存官方页面的登录状态，也不会自动切换到付费 API。</p><button onClick={() => { baseDispatch({ type: 'AUTH_READY' }); dispatchCore('auth-ready') }}>关闭提示</button></div></div>}
    </>
  </div>
}
