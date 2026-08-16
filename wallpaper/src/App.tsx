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
import { SettingsPanel } from './settings/SettingsPanel.tsx'
import { BACKGROUND_OPTIONS, applyBubbleOverrides, assetUrl, loadSettings, resumeConversationId, saveConversationPointer, saveSettings, type WallpaperSettings } from './settings/store.ts'
import { nativeRuntime } from './native/runtime.ts'
import type { AppSurface } from './surface.ts'
import { collectInteractionRegions, publishInteractionRegions } from './runtime/interactionRegions.ts'
import { AppearanceDrawer } from './features/appearance/AppearanceDrawer.tsx'
import type { AppearanceAssetSummary, AppearanceThemeSummary, AssetClassificationRequest } from './features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from './appearance/theme/index.ts'
import { chooseAppearanceImportFolder, chooseAppearanceImportPaths, nativeAppearance } from './native/appearance.ts'
import { appCoreClient } from './runtime/appCoreClient.ts'
import { useInteractionLayout } from './runtime/useInteractionLayout.ts'
import type { InteractionState } from './runtime/interactionLayout.ts'

const registry = new PersonaRegistry()

export interface AppProps { surface?: AppSurface }

export function App({ surface = 'combined' }: AppProps) {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [runtime, baseDispatch] = useReducer(reduceRuntime, { ...INITIAL_RUNTIME_STATE, backend: settings.defaultBackend })
  const [showSettings, setShowSettings] = useState(false)
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
  const [interactionState, setInteractionState] = useState<InteractionState>('collapsed')
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
  const sceneSurface = surface !== 'interaction'
  const interactionSurface = surface !== 'background'
  const interactionDirection = useInteractionLayout({
    enabled: surface === 'interaction',
    layout: settings.interactionLayout,
    state: interactionState,
    anchor: settings.floatingAnchor,
    refreshKey: `${showSettings}:${showAppearance}:${runtime.historyExpanded}:${runtime.phase}`,
  })

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
      if (!snapshot.interaction.desktopForeground || snapshot.privacyScreen) {
        setShowSettings(false)
        setShowAppearance(false)
        setInteractionState('collapsed')
      }
    }
    void Promise.all([
      appCoreClient.snapshot().then(applySnapshot),
      appCoreClient.subscribe(applySnapshot).then((dispose) => { unsubscribe = dispose }),
    ])
    const timer = surface === 'background'
      ? setTimeout(() => dispatchCore('boot-ready', { playWake: settings.animationsEnabled && !settings.skipWakeAnimation }), 120)
      : undefined
    return () => {
      if (timer !== undefined) clearTimeout(timer)
      unsubscribe()
    }
  }, [settings.animationsEnabled, settings.skipWakeAnimation, surface])

  useEffect(() => {
    if (!interactionSurface) return
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
  }, [interactionSurface, runtime.backend, settings.conversationPolicy, settings.deepseekApi.baseUrl, settings.deepseekApi.model, conversationGeneration])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let unsubscribe: () => void = () => undefined
    void nativeRuntime.listenSystem((event) => {
      if (!appCoreClient.native && (event === 'locked' || event === 'suspend')) baseDispatch({ type: 'LOCK' })
      if (event === 'unlocked' || event === 'resume') {
        setInteractionState('collapsed')
        if (settings.conversationPolicy === 'new-on-unlock') setConversationGeneration((value) => value + 1)
        if (!appCoreClient.native) baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation })
      }
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [settings.animationsEnabled, settings.conversationPolicy, settings.playWakeOnEveryUnlock, settings.skipWakeAnimation])

  useEffect(() => {
    if (!nativeRuntime.isNative || !interactionSurface) return
    let unsubscribe: () => void = () => undefined
    void nativeRuntime.listenTray((event) => {
      if (event.type === 'backend') changeBackend(event.backend)
      else { setInteractionState('expanded'); setShowAppearance(false); setShowSettings(true) }
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [interactionSurface])

  useEffect(() => {
    if (!nativeAppearance.isNative) return
    void refreshAppearance()
  }, [surface])

  useEffect(() => {
    if (!nativeRuntime.isNative || !interactionSurface) return
    let revision = 0
    let frame = 0
    const publish = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        void publishInteractionRegions({
          revision: ++revision,
          scaleFactor: window.devicePixelRatio || 1,
          regions: collectInteractionRegions(),
        })
      })
    }
    const observer = new MutationObserver(publish)
    observer.observe(document.body, { attributes: true, childList: true, subtree: true })
    window.addEventListener('resize', publish)
    window.addEventListener('dsh-interaction-placement', publish)
    publish()
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', publish)
      window.removeEventListener('dsh-interaction-placement', publish)
      cancelAnimationFrame(frame)
      void publishInteractionRegions({ revision: ++revision, scaleFactor: window.devicePixelRatio || 1, regions: [] })
    }
  }, [interactionSurface])

  useEffect(() => {
    if (!interactionSurface) return
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
  }, [interactionSurface, settings.autoSwitchHarness])

  useEffect(() => {
    if (!appCoreClient.native || !interactionSurface) return
    if (runtime.harness === 'bridge-ready' && runtime.backend !== 'harness') {
      if (settings.autoSwitchHarness) changeBackend('harness')
      else setShowHarnessPrompt(true)
    }
    if (runtime.harness !== 'bridge-ready' && runtime.backend === 'harness') changeBackend('deepseek-web')
  }, [interactionSurface, runtime.harness, runtime.backend, settings.autoSwitchHarness])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === 'w') { baseDispatch({ type: 'LOCK' }); dispatchCore('lock') }
      if (event.key === 'Escape') {
        if (runtime.phase === 'locked') { baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }); dispatchCore('unlock', { playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }) }
        else if (showSettings || showAppearance || interactionState === 'expanded') {
          setShowSettings(false); setShowAppearance(false); setInteractionState('collapsed')
          baseDispatch({ type: 'CLOSE_CHAT' }); dispatchCore('close-chat')
        }
      }
    }
    window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey)
  }, [runtime.phase, settings])

  const changeBackend = (backend: WallpaperSettings['defaultBackend']) => {
    patchRuntime({ backend })
    setShowHarnessPrompt(false)
    baseDispatch({ type: 'RECOVER' })
    if (appCoreClient.native) void appCoreClient.selectBackend(backend).catch((error) => patchRuntime({ error: String(error) }))
  }
  const scene = useMemo(() => {
    if (runtime.phase === 'booting' || runtime.phase === 'locked') return <SleepScene persona={persona} mode="system" />
    if (runtime.phase === 'waking') return <WakeScene persona={persona} enabled={settings.animationsEnabled && !settings.skipWakeAnimation} speed={settings.animationSpeed} onWakeDone={() => { baseDispatch({ type: 'WAKE_DONE' }); dispatchCore('wake-done') }} />
    return <IdleScene persona={{ ...persona, bubbles, assets: { ...persona.assets, portrait: resolvedPersona ?? persona.assets.portrait } }} bubbleText={runtime.activity === 'thinking' ? '正在认真思考…' : bubbles.morning} showHarnessPrompt={interactionSurface && showHarnessPrompt} harnessOnline={runtime.harness !== 'offline'} backgroundUrl={resolvedBackground ?? (background?.path ? assetUrl(background.path) : undefined)} onOpenChat={() => { if (interactionSurface) { baseDispatch({ type: 'OPEN_CHAT' }); dispatchCore('open-chat') } }} onSwitchToHarness={() => changeBackend('harness')} onDismissHarnessPrompt={() => setShowHarnessPrompt(false)} />
  }, [background?.path, bubbles, persona, resolvedBackground, resolvedPersona, runtime, settings, showHarnessPrompt])

  return <div className={`wallpaper-root surface-${surface} effort-${runtime.reasoningEffort ?? 'normal'}`}>
    {sceneSurface && scene}
    {interactionSurface && <>
      <ConversationBubble backend={runtime.backend} activity={runtime.activity} modelLabel={modelLabel} messages={messages} streamingText={streamingText} historyExpanded={runtime.historyExpanded} usage={usage} collapsed={interactionState === 'collapsed'} layout={settings.interactionLayout} expandDirection={interactionDirection} onExpand={() => { setInteractionState('expanded'); baseDispatch({ type: 'OPEN_CHAT' }); dispatchCore('open-chat') }} disabled={runtime.backend === 'deepseek-web' || (runtime.backend === 'harness' && runtime.harness !== 'bridge-ready')} onToggleHistory={() => { baseDispatch({ type: 'TOGGLE_HISTORY' }); dispatchCore('toggle-history') }} onSend={(text) => { void adapterRef.current.send(text).then(() => { if (adapterRef.current instanceof NativeChatAdapter) { const id = adapterRef.current.conversationId(); if (id) saveConversationPointer(runtime.backend, id) } }) }} onStop={() => void adapterRef.current.stop()} onClose={() => { setShowSettings(false); setShowAppearance(false); setInteractionState('collapsed'); baseDispatch({ type: 'CLOSE_CHAT' }); dispatchCore('close-chat') }} />
      {runtime.error && runtime.phase !== 'error' && <div className="runtime-notice" role="status">{runtime.error}<button onClick={() => patchRuntime({ error: undefined })}>×</button></div>}
      {interactionState === 'expanded' && <button className="tray-zone" data-interaction-region="settings-trigger" onClick={() => { setShowAppearance(false); setShowSettings((value) => !value) }} title="设置" aria-label="设置" />}
      {interactionState === 'expanded' && <button className="tray-zone appearance-trigger" data-interaction-region="appearance-trigger" onClick={() => { setShowSettings(false); setShowAppearance((value) => !value); void refreshAppearance() }} title="外观" aria-label="外观">◈</button>}
      {showSettings && <div data-interaction-region="settings-panel"><SettingsPanel settings={settings} harnessStatus={runtime.harness} onChange={(next) => { setSettings(next); saveSettings(next); if (nativeRuntime.isNative && next.lockScreenEnabled !== settings.lockScreenEnabled) void nativeRuntime.setLockScreen(next.lockScreenEnabled).catch((error) => patchRuntime({ error: String(error) })); if (nativeRuntime.isNative && next.autostart !== settings.autostart) void nativeRuntime.setAutostart(next.autostart).catch((error) => patchRuntime({ error: String(error) })) }} onRequestDeepSeekLogin={() => { baseDispatch({ type: 'AUTH_REQUIRED' }); void nativeRuntime.requestDeepSeekLogin() }} onConfigureApiKey={() => { const key = window.prompt('输入 DeepSeek API Key。密钥只会写入 Windows 凭据管理器，不进入前端存储。'); if (key) void nativeRuntime.saveApiKey(key).catch((error) => patchRuntime({ error: String(error) })) }} onClose={() => setShowSettings(false)} /></div>}
      <AppearanceDrawer open={showAppearance} themes={appearanceThemes} assets={appearanceAssets} activeThemeId={appearanceTheme?.id ?? ''} activeThemeVersion={appearanceTheme?.version ?? ''} overrides={appearanceOverrides} busy={appearanceBusy} notice={appearanceNotice} onClose={() => setShowAppearance(false)} onImport={() => { void importAppearance(chooseAppearanceImportPaths) }} onImportFolder={() => { void importAppearance(chooseAppearanceImportFolder) }} onExport={() => setAppearanceNotice({ tone: 'info', message: '主题导出需要名称与版本信息，完整导出表单将在下一步接入。' })} onReviewInbox={() => undefined} onClassify={(request) => { void classifyAppearance(request) }} onActivateTheme={(themeId, version) => { void mutateAppearance(() => nativeAppearance.activateTheme(themeId, version)) }} onSetOverride={(slot, assetId) => { void mutateAppearance(() => nativeAppearance.setOverride(slot, assetId)) }} onClearOverride={(slot) => { void mutateAppearance(() => nativeAppearance.clearOverride(slot)) }} />
      {runtime.phase === 'auth-required' && <div className="auth-overlay" data-interaction-region="auth"><div className="auth-card"><h2>DeepSeek 网页登录</h2><p>桌面版会显示 DeepSeek 官方登录窗口，登录态由 WebView2 保存。</p><button onClick={() => { baseDispatch({ type: 'AUTH_READY' }); dispatchCore('auth-ready') }}>我已完成登录</button></div></div>}
    </>}
  </div>
}
