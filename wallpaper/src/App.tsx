import { useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { PreviewAdapter } from './chat/mockAdapter.ts'
import { NativeChatAdapter } from './chat/nativeAdapter.ts'
import { DeepSeekWebAdapter } from './chat/deepseekWebAdapter.ts'
import type { ChatAdapter } from './chat/adapter.ts'
import { ConversationBubble } from './chat/ConversationBubble.tsx'
import type { ChatMessage, RuntimeState, TokenUsage } from './domain/types.ts'
import { personaIdFor, resolveModelTier } from './domain/modelTier.ts'
import { monitorHarness } from './connect/harness.ts'
import { PersonaRegistry } from './persona/registry.ts'
import { IdleScene } from './scenes/IdleScene.tsx'
import { SleepScene } from './scenes/SleepScene.tsx'
import { WakeScene } from './scenes/WakeScene.tsx'
import { INITIAL_RUNTIME_STATE, reduceRuntime } from './scenes/stateMachine.ts'
import { BACKGROUND_OPTIONS, applyBubbleOverrides, assetUrl, loadSettings, resumeConversationId, saveConversationPointer, type WallpaperSettings } from './settings/store.ts'
import { nativeRuntime } from './native/runtime.ts'
import { listen } from '@tauri-apps/api/event'
import type { AppSurface } from './surface.ts'
import { beginInteractionRegionSession, collectInteractionRegions, publishInteractionRegions } from './runtime/interactionRegions.ts'
import { AppearanceDrawer } from './features/appearance/AppearanceDrawer.tsx'
import type { AppearanceAssetSummary, AppearanceThemeSummary, AssetClassificationRequest } from './features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from './appearance/theme/index.ts'
import { chooseAppearanceImportFolder, chooseAppearanceImportPaths, nativeAppearance } from './native/appearance.ts'
import { appCoreClient } from './runtime/appCoreClient.ts'
import type { DesktopWorkspace } from './runtime/desktopWorkspace.ts'
import { WidgetHost } from './widgets/WidgetHost.tsx'

const registry = new PersonaRegistry()

export interface AppProps { surface?: AppSurface }

export function App({ surface = 'combined' }: AppProps) {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [runtime, baseDispatch] = useReducer(reduceRuntime, { ...INITIAL_RUNTIME_STATE, backend: settings.defaultBackend })
  const [showAppearance, setShowAppearance] = useState(false)
  const [appearanceThemes, setAppearanceThemes] = useState<AppearanceThemeSummary[]>([])
  const [appearanceAssets, setAppearanceAssets] = useState<AppearanceAssetSummary[]>([])
  const [appearanceTheme, setAppearanceTheme] = useState<{ id: string; version: string }>()
  const [appearanceOverrides, setAppearanceOverrides] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [appearanceBusy, setAppearanceBusy] = useState(false)
  const [resolvedAssets, setResolvedAssets] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [appearanceNotice, setAppearanceNotice] = useState<{ tone: 'success' | 'info' | 'warning' | 'error'; message: string }>()
  const [showHarnessPrompt, setShowHarnessPrompt] = useState(false)
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [streamingText, setStreamingText] = useState('')
  const [usage, setUsage] = useState<TokenUsage>()
  const [conversationGeneration, setConversationGeneration] = useState(0)
  const [interactionState, setInteractionState] = useState<'collapsed' | 'expanded'>('collapsed')
  const [interactionEnabled, setInteractionEnabled] = useState(true)
  const [workspace, setWorkspace] = useState<DesktopWorkspace>('front')
  const [innerHistoryExpanded, setInnerHistoryExpanded] = useState(false)
  const adapterRef = useRef<ChatAdapter>(new PreviewAdapter(settings.defaultBackend))
  const runtimeRef = useRef(runtime)
  runtimeRef.current = runtime
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
  // The WorkerW host is permanently desktop-sized. Both the floating window
  // and the taskbar capsule now use CSS placement inside that one viewport.
  const interactionDirection = 'center' as const

  const enterInnerWorkspace = () => {
    // A workspace entry is an input-focused starting point, not a pre-opened
    // empty transcript drawer. History is deliberately user-triggered here.
    setInnerHistoryExpanded(false)
    setWorkspace('entering-inner')
    setInteractionState('expanded')
    baseDispatch({ type: 'OPEN_CHAT' })
    dispatchCore('open-chat')
    window.setTimeout(() => setWorkspace('inner'), 280)
  }

  const leaveInnerWorkspace = () => {
    setShowAppearance(false)
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
    try {
      const [snapshot, themes, assets] = await Promise.all([
        nativeAppearance.getState(),
        nativeAppearance.listThemes(),
        nativeAppearance.listAssets(),
      ])
      setAppearanceTheme(snapshot.activeTheme)
      setAppearanceOverrides(snapshot.overrides)
      setAppearanceThemes(themes)
      setAppearanceAssets(assets)
      const slots: AppearanceSlot[] = [
        'desktop.background',
        'persona.deepseek.flash', 'persona.deepseek.pro',
        'persona.harness.flash', 'persona.harness.pro',
      ]
      const resolved = await Promise.all(slots.map(async (slot) => [slot, await nativeAppearance.resolveAsset(slot)] as const))
      setResolvedAssets(Object.fromEntries(resolved.filter((entry) => Boolean(entry[1]))))
    } catch (error) {
      patchRuntime({ error: String(error) })
    }
  }

  const applyAppearanceSnapshot = (snapshot: { activeTheme?: { id: string; version: string }; overrides: Partial<Record<AppearanceSlot, string>> }) => {
    setAppearanceTheme(snapshot.activeTheme)
    setAppearanceOverrides(snapshot.overrides)
  }

  const mutateAppearance = async (operation: () => Promise<{ activeTheme?: { id: string; version: string }; overrides: Partial<Record<AppearanceSlot, string>> }>) => {
    setAppearanceBusy(true)
    try {
      applyAppearanceSnapshot(await operation())
    } catch (error) {
      patchRuntime({ error: String(error) })
    } finally {
      setAppearanceBusy(false)
    }
  }

  const importAppearance = async (choose: () => Promise<string[]>) => {
    setAppearanceBusy(true)
    try {
      const paths = await choose()
      if (paths.length === 0) return
      const batch = await nativeAppearance.importPaths(paths)
      applyAppearanceSnapshot(batch.snapshot)
      await refreshAppearance()
      const imported = batch.results.reduce((sum, result) => sum + result.imported, 0)
      const deduplicated = batch.results.reduce((sum, result) => sum + result.deduplicated, 0)
      const themeCount = batch.results.filter((result) => result.kind === 'theme').length
      const inboxCount = batch.results.filter((result) => result.kind === 'inbox').reduce((sum, result) => sum + result.imported, 0)
      const parts = [`已导入 ${imported} 项`]
      if (themeCount > 0) parts.push(`${themeCount} 个主题包`)
      if (inboxCount > 0) parts.push(`${inboxCount} 项等待分类`)
      if (deduplicated > 0) parts.push(`${deduplicated} 项已去重`)
      setAppearanceNotice({ tone: 'success', message: parts.join(' · ') })
    } catch (error) {
      setAppearanceNotice({ tone: 'error', message: `导入失败：${String(error)}` })
    } finally {
      setAppearanceBusy(false)
    }
  }

  const classifyAppearance = async (request: AssetClassificationRequest) => {
    setAppearanceBusy(true)
    try {
      await Promise.all(request.assetIds.map((assetId) => nativeAppearance.classifyAsset(assetId, request.slots)))
      await refreshAppearance()
      setAppearanceNotice({ tone: 'success', message: `已整理 ${request.assetIds.length} 项素材，可在对应组件菜单中选择。` })
    } catch (error) {
      setAppearanceNotice({ tone: 'error', message: `整理失败：${String(error)}` })
    } finally {
      setAppearanceBusy(false)
    }
  }

  useEffect(() => {
    if (!appCoreClient.native) {
      const timer = setTimeout(() => baseDispatch({ type: 'BOOT_READY', playWake: settings.animationsEnabled && !settings.skipWakeAnimation }), 120)
      return () => clearTimeout(timer)
    }
    let unsubscribe: () => void = () => undefined
    const applySnapshot = (snapshot: Awaited<ReturnType<typeof appCoreClient.snapshot>>) => {
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
        setShowAppearance(false)
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
    const adapter: ChatAdapter = nativeRuntime.isNative
      ? runtime.backend === 'deepseek-web'
        ? new DeepSeekWebAdapter()
        : new NativeChatAdapter(
            runtime.backend,
            runtime.backend === 'deepseek-api' ? { baseUrl: settings.deepseekApi.baseUrl, model: settings.deepseekApi.model } : {},
            runtime.backend === 'harness'
              ? resumeConversationId('harness', settings.conversationPolicy)
              : runtime.backend === 'deepseek-api'
                ? resumeConversationId('deepseek-api', settings.conversationPolicy)
                : undefined,
          )
      : new PreviewAdapter(runtime.backend)
    adapterRef.current.disconnect(); adapterRef.current = adapter; setMessages([]); setStreamingText('')
    const unsubscribe = adapter.subscribe((event) => {
      if (event.type === 'status') { patchRuntime({ activity: event.activity }); dispatchCore('set-activity', { value: event.activity }) }
      if (event.type === 'delta') { patchRuntime({ activity: 'streaming' }); dispatchCore('set-activity', { value: 'streaming' }); setStreamingText((value) => value + event.text) }
      if (event.type === 'message') { setMessages((items) => [...items, { id: crypto.randomUUID(), role: event.role, content: event.content, createdAt: Date.now(), usage: event.usage }]); if (event.role === 'assistant') setStreamingText('') }
      if (event.type === 'usage') setUsage(event)
      if (event.type === 'model') patchRuntime({ model: event.model, provider: event.provider, modelTier: event.tier, reasoningEffort: event.effort })
      if (event.type === 'auth-required') { baseDispatch({ type: 'AUTH_REQUIRED' }); dispatchCore('auth-required') }
      if (event.type === 'approval-required') { patchRuntime({ activity: 'tool', error: `${event.summary}；请打开 Harness 处理。` }); dispatchCore('set-activity', { value: 'tool' }) }
      if (event.type === 'error') patchRuntime({ activity: 'idle', error: event.message })
    })
    void adapter.connect().then(async () => {
      if (adapter instanceof NativeChatAdapter) {
        const id = adapter.conversationId()
        if (id) saveConversationPointer(runtime.backend, id)
      }
      const history = await adapter.history()
      if (history.length) setMessages(history)
    }).catch((error) => patchRuntime({ activity: 'idle', error: String(error) }))
    return () => { unsubscribe(); adapter.disconnect() }
  }, [runtime.backend, settings.conversationPolicy, settings.deepseekApi.baseUrl, settings.deepseekApi.model, conversationGeneration])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let unsubscribe: () => void = () => undefined
    void nativeRuntime.listenSystem((event) => {
      if (!appCoreClient.native && (event === 'locked' || event === 'suspend')) baseDispatch({ type: 'LOCK' })
      if (event === 'unlocked' || event === 'resume') {
        if (settings.interactionLayout === 'taskbar-docked') setInteractionState('collapsed')
        if (settings.conversationPolicy === 'new-on-unlock') setConversationGeneration((value) => value + 1)
        if (!appCoreClient.native) baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation })
      }
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [settings.animationsEnabled, settings.conversationPolicy, settings.interactionLayout, settings.playWakeOnEveryUnlock, settings.skipWakeAnimation])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let unsubscribe: () => void = () => undefined
    void nativeRuntime.listenTray((event) => {
      if (event.type === 'backend') changeBackend(event.backend)
      else { setInteractionState('expanded'); setShowAppearance(true) }
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
      if (status.availability === 'bridge-ready' && runtimeRef.current.backend !== 'harness') {
        if (settings.autoSwitchHarness) patchRuntime({ backend: 'harness' }); else setShowHarnessPrompt(true)
      }
      if (status.availability !== 'bridge-ready' && runtimeRef.current.backend === 'harness') patchRuntime({ backend: 'deepseek-web' })
    })
    return () => monitor.stop()
  }, [settings.autoSwitchHarness])

  useEffect(() => {
    if (!appCoreClient.native) return
    if (runtime.harness === 'bridge-ready' && runtime.backend !== 'harness') {
      if (settings.autoSwitchHarness) changeBackend('harness')
      else setShowHarnessPrompt(true)
    }
    if (runtime.harness !== 'bridge-ready' && runtime.backend === 'harness') changeBackend('deepseek-web')
  }, [runtime.harness, runtime.backend, settings.autoSwitchHarness])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === 'w') { baseDispatch({ type: 'LOCK' }); dispatchCore('lock') }
      if (event.key === 'Escape') {
        if (runtime.phase === 'locked') { baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }); dispatchCore('unlock', { playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }) }
      }
    }
    window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey)
  }, [runtime.phase, settings, workspace, showAppearance, interactionState])

  const changeBackend = (backend: WallpaperSettings['defaultBackend']) => {
    patchRuntime({ backend })
    setShowHarnessPrompt(false)
    baseDispatch({ type: 'RECOVER' })
    if (appCoreClient.native) void appCoreClient.selectBackend(backend).catch((error) => patchRuntime({ error: String(error) }))
  }
  const scene = useMemo(() => {
    if (runtime.phase === 'booting' || runtime.phase === 'locked') return <SleepScene persona={persona} mode="system" />
    if (runtime.phase === 'waking') return <WakeScene persona={persona} enabled={settings.animationsEnabled && !settings.skipWakeAnimation} speed={settings.animationSpeed} onWakeDone={() => { baseDispatch({ type: 'WAKE_DONE' }); dispatchCore('wake-done') }} />
    return <IdleScene persona={{ ...persona, bubbles, assets: { ...persona.assets, portrait: resolvedPersona ?? persona.assets.portrait } }} bubbleText={runtime.activity === 'thinking' ? '正在认真思考…' : bubbles.morning} showHarnessPrompt={showHarnessPrompt} harnessOnline={runtime.harness !== 'offline'} backgroundUrl={resolvedBackground ?? (background?.path ? assetUrl(background.path) : undefined)} portraitAmbientLength={settings.portraitAmbientLength} portraitAmbientStrength={settings.portraitAmbientStrength} hideBubble={workspace !== 'front'} onOpenChat={enterInnerWorkspace} onSwitchToHarness={() => changeBackend('harness')} onDismissHarnessPrompt={() => setShowHarnessPrompt(false)} />
  }, [background?.path, bubbles, persona, resolvedBackground, resolvedPersona, runtime, settings, showHarnessPrompt, workspace])

  return <div className={`wallpaper-root surface-${surface} effort-${runtime.reasoningEffort ?? 'normal'} workspace-${workspace}`} data-workspace={workspace}>
    {scene}
    <>
      <WidgetHost workspace={workspace} widgets={[]} />
      {interactionEnabled && runtime.phase !== 'booting' && runtime.phase !== 'locked' && (settings.interactionLayout === 'taskbar-docked' || workspace !== 'front') && <ConversationBubble backend={runtime.backend} activity={runtime.activity} modelLabel={modelLabel} messages={messages} streamingText={streamingText} historyExpanded={workspace === 'front' ? runtime.historyExpanded : innerHistoryExpanded} usage={usage} collapsed={settings.interactionLayout === 'taskbar-docked' && interactionState === 'collapsed'} layout={settings.interactionLayout} expandDirection={interactionDirection} persistent={settings.interactionLayout === 'floating'} acrylicOpacity={settings.conversationOpacity} acrylicBlur={settings.conversationBlur} onExpand={() => { setInteractionState('expanded'); if (workspace === 'front') enterInnerWorkspace(); else { baseDispatch({ type: 'OPEN_CHAT' }); dispatchCore('open-chat') } }} disabled={runtime.backend === 'harness' && runtime.harness !== 'bridge-ready'} onToggleHistory={() => { if (workspace !== 'front') setInnerHistoryExpanded((value) => !value); else { baseDispatch({ type: 'TOGGLE_HISTORY' }); dispatchCore('toggle-history') } }} onSend={(text) => { void adapterRef.current.send(text).then(() => { if (adapterRef.current instanceof NativeChatAdapter) { const id = adapterRef.current.conversationId(); if (id) saveConversationPointer(runtime.backend, id) } }) }} onStop={() => void adapterRef.current.stop()} onClose={() => undefined} />}
      {runtime.error && runtime.phase !== 'error' && <div className="runtime-notice" role="status">{runtime.error}<button onClick={() => patchRuntime({ error: undefined })}>×</button></div>}
      <AppearanceDrawer open={showAppearance} themes={appearanceThemes} assets={appearanceAssets} activeThemeId={appearanceTheme?.id ?? ''} activeThemeVersion={appearanceTheme?.version ?? ''} overrides={appearanceOverrides} busy={appearanceBusy} notice={appearanceNotice} onClose={() => setShowAppearance(false)} onImport={() => { void importAppearance(chooseAppearanceImportPaths) }} onImportFolder={() => { void importAppearance(chooseAppearanceImportFolder) }} onExport={() => setAppearanceNotice({ tone: 'info', message: '主题导出需要名称与版本信息，完整导出表单将在下一步接入。' })} onReviewInbox={() => undefined} onClassify={(request) => { void classifyAppearance(request) }} onActivateTheme={(themeId, version) => { void mutateAppearance(() => nativeAppearance.activateTheme(themeId, version)) }} onSetOverride={(slot, assetId) => { void mutateAppearance(() => nativeAppearance.setOverride(slot, assetId)) }} onClearOverride={(slot) => { void mutateAppearance(() => nativeAppearance.clearOverride(slot)) }} />
      {runtime.phase === 'auth-required' && <div className="auth-overlay" data-interaction-region="auth"><div className="auth-card"><h2>DeepSeek 网页登录</h2><p>桌面版会显示 DeepSeek 官方登录窗口，登录态由 WebView2 保存。</p><button onClick={() => { baseDispatch({ type: 'AUTH_READY' }); dispatchCore('auth-ready') }}>我已完成登录</button></div></div>}
    </>
  </div>
}
