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

const registry = new PersonaRegistry()

export interface AppProps { surface?: AppSurface }

export function App({ surface = 'combined' }: AppProps) {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [runtime, baseDispatch] = useReducer(reduceRuntime, { ...INITIAL_RUNTIME_STATE, backend: settings.defaultBackend })
  const [showSettings, setShowSettings] = useState(false)
  const [showHarnessPrompt, setShowHarnessPrompt] = useState(false)
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [streamingText, setStreamingText] = useState('')
  const [usage, setUsage] = useState<TokenUsage>()
  const [conversationGeneration, setConversationGeneration] = useState(0)
  const adapterRef = useRef<ChatAdapter>(new PreviewAdapter(settings.defaultBackend))
  const runtimeRef = useRef(runtime)
  runtimeRef.current = runtime
  const patchRuntime = (patch: Partial<RuntimeState>) => baseDispatch({ type: 'PATCH', patch })

  const tier = resolveModelTier(runtime.backend, runtime.provider, runtime.model, settings.modelTierRules, runtime.modelTier)
  const persona = registry.get(personaIdFor(runtime.backend, tier))
  const bubbles = applyBubbleOverrides(persona.bubbles, settings.bubbleOverrides)
  const background = BACKGROUND_OPTIONS.find((item) => item.id === settings.background)
  const modelLabel = runtime.model ?? (tier === 'pro' ? 'Pro · 成年形态' : 'Flash · 幼年形态')
  const sceneSurface = surface !== 'interaction'
  const interactionSurface = surface !== 'background'

  useEffect(() => { const timer = setTimeout(() => baseDispatch({ type: 'BOOT_READY', playWake: settings.animationsEnabled && !settings.skipWakeAnimation }), 120); return () => clearTimeout(timer) }, [settings.animationsEnabled, settings.skipWakeAnimation])

  useEffect(() => {
    const adapter: ChatAdapter = nativeRuntime.isNative
      ? runtime.backend === 'deepseek-web'
        ? new DeepSeekWebAdapter()
        : new NativeChatAdapter(runtime.backend, runtime.backend === 'deepseek-api' ? { baseUrl: settings.deepseekApi.baseUrl, model: settings.deepseekApi.model } : {}, runtime.backend === 'harness' ? resumeConversationId('harness', settings.conversationPolicy) : undefined)
      : new PreviewAdapter(runtime.backend)
    adapterRef.current.disconnect(); adapterRef.current = adapter; setMessages([]); setStreamingText('')
    const unsubscribe = adapter.subscribe((event) => {
      if (event.type === 'status') patchRuntime({ activity: event.activity })
      if (event.type === 'delta') { patchRuntime({ activity: 'streaming' }); setStreamingText((value) => value + event.text) }
      if (event.type === 'message') { setMessages((items) => [...items, { id: crypto.randomUUID(), role: event.role, content: event.content, createdAt: Date.now(), usage: event.usage }]); if (event.role === 'assistant') setStreamingText('') }
      if (event.type === 'usage') setUsage(event)
      if (event.type === 'model') patchRuntime({ model: event.model, provider: event.provider, modelTier: event.tier, reasoningEffort: event.effort })
      if (event.type === 'auth-required') baseDispatch({ type: 'AUTH_REQUIRED' })
      if (event.type === 'approval-required') patchRuntime({ activity: 'tool', error: `${event.summary}；请打开 Harness 处理。` })
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
      if (event === 'locked' || event === 'suspend') baseDispatch({ type: 'LOCK' })
      if (event === 'unlocked' || event === 'resume') {
        if (settings.conversationPolicy === 'new-on-unlock') setConversationGeneration((value) => value + 1)
        baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation })
      }
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [settings.animationsEnabled, settings.conversationPolicy, settings.playWakeOnEveryUnlock, settings.skipWakeAnimation])

  useEffect(() => {
    if (!nativeRuntime.isNative || surface !== 'background') return
    const onOpen = () => { void nativeRuntime.showInteraction() }
    window.addEventListener('pointerup', onOpen)
    return () => window.removeEventListener('pointerup', onOpen)
  }, [surface])

  useEffect(() => {
    if (!nativeRuntime.isNative || !interactionSurface) return
    let unsubscribe: () => void = () => undefined
    void nativeRuntime.listenTray((event) => {
      if (event.type === 'backend') changeBackend(event.backend)
      else setShowSettings(true)
    }).then((dispose) => { unsubscribe = dispose })
    return () => unsubscribe()
  }, [interactionSurface])

  useEffect(() => {
    const monitor = monitorHarness((status) => {
      patchRuntime({ harness: status.availability, model: status.model ?? runtimeRef.current.model, provider: status.provider ?? runtimeRef.current.provider, reasoningEffort: status.reasoningEffort })
      if (status.availability === 'bridge-ready' && runtimeRef.current.backend !== 'harness') {
        if (settings.autoSwitchHarness) patchRuntime({ backend: 'harness' }); else setShowHarnessPrompt(true)
      }
      if (status.availability !== 'bridge-ready' && runtimeRef.current.backend === 'harness') patchRuntime({ backend: 'deepseek-web' })
    }, nativeRuntime.isNative ? () => nativeRuntime.probeHarness() : undefined)
    return () => monitor.stop()
  }, [settings.autoSwitchHarness])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === 'w') baseDispatch({ type: 'LOCK' })
      if (event.key === 'Escape') {
        if (runtime.phase === 'locked') baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation })
        else if (runtime.phase === 'chatting') baseDispatch({ type: 'CLOSE_CHAT' })
        else setShowSettings(false)
      }
    }
    window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey)
  }, [runtime.phase, settings])

  const changeBackend = (backend: WallpaperSettings['defaultBackend']) => { patchRuntime({ backend }); setShowHarnessPrompt(false); baseDispatch({ type: 'RECOVER' }) }
  const scene = useMemo(() => {
    if (runtime.phase === 'booting' || runtime.phase === 'locked') return <SleepScene persona={persona} mode="system" />
    if (runtime.phase === 'waking') return <WakeScene persona={persona} enabled={settings.animationsEnabled && !settings.skipWakeAnimation} speed={settings.animationSpeed} onWakeDone={() => baseDispatch({ type: 'WAKE_DONE' })} />
    return <IdleScene persona={{ ...persona, bubbles }} bubbleText={runtime.activity === 'thinking' ? '正在认真思考…' : bubbles.morning} showHarnessPrompt={showHarnessPrompt} harnessOnline={runtime.harness !== 'offline'} backgroundUrl={background?.path ? assetUrl(background.path) : undefined} onOpenChat={() => baseDispatch({ type: 'OPEN_CHAT' })} onSwitchToHarness={() => changeBackend('harness')} onDismissHarnessPrompt={() => setShowHarnessPrompt(false)} />
  }, [background?.path, bubbles, persona, runtime, settings, showHarnessPrompt])

  return <div className={`wallpaper-root surface-${surface} effort-${runtime.reasoningEffort ?? 'normal'}`}>
    {sceneSurface && scene}
    {interactionSurface && <>
      {(surface === 'interaction' || runtime.phase === 'chatting') && <ConversationBubble backend={runtime.backend} activity={runtime.activity} modelLabel={modelLabel} messages={messages} streamingText={streamingText} historyExpanded={runtime.historyExpanded} usage={usage} disabled={runtime.backend === 'harness' && runtime.harness !== 'bridge-ready'} onToggleHistory={() => baseDispatch({ type: 'TOGGLE_HISTORY' })} onSend={(text) => void adapterRef.current.send(text)} onStop={() => void adapterRef.current.stop()} onClose={() => { baseDispatch({ type: 'CLOSE_CHAT' }); if (surface === 'interaction') void nativeRuntime.hideInteraction() }} />}
      {runtime.error && runtime.phase !== 'error' && <div className="runtime-notice" role="status">{runtime.error}<button onClick={() => patchRuntime({ error: undefined })}>×</button></div>}
      <button className="tray-zone" onClick={() => setShowSettings((value) => !value)} title="设置" aria-label="设置" />
      {showSettings && <SettingsPanel settings={settings} harnessStatus={runtime.harness} onChange={(next) => { setSettings(next); saveSettings(next); if (nativeRuntime.isNative && next.lockScreenEnabled !== settings.lockScreenEnabled) void nativeRuntime.setLockScreen(next.lockScreenEnabled).catch((error) => patchRuntime({ error: String(error) })); if (nativeRuntime.isNative && next.autostart !== settings.autostart) void nativeRuntime.setAutostart(next.autostart).catch((error) => patchRuntime({ error: String(error) })) }} onRequestDeepSeekLogin={() => { baseDispatch({ type: 'AUTH_REQUIRED' }); void nativeRuntime.requestDeepSeekLogin() }} onConfigureApiKey={() => { const key = window.prompt('输入 DeepSeek API Key。密钥只会写入 Windows 凭据管理器，不进入前端存储。'); if (key) void nativeRuntime.saveApiKey(key).catch((error) => patchRuntime({ error: String(error) })) }} onClose={() => setShowSettings(false)} />}
      {runtime.phase === 'auth-required' && <div className="auth-overlay"><div className="auth-card"><h2>DeepSeek 网页登录</h2><p>桌面版会显示 DeepSeek 官方登录窗口，登录态由 WebView2 保存。</p><button onClick={() => baseDispatch({ type: 'AUTH_READY' })}>我已完成登录</button></div></div>}
    </>}
  </div>
}
