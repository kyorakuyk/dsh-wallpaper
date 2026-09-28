import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { PreviewAdapter } from './chat/mockAdapter.ts'
import { NativeChatAdapter } from './chat/nativeAdapter.ts'
import { DeepSeekWebAdapter } from './chat/deepseekWebAdapter.ts'
import type { ChatAdapter } from './chat/adapter.ts'
import { ConversationBubble } from './chat/ConversationBubble.tsx'
import type { BackendMode, ChatMessage, ChatQuestion, ModelTier, RuntimeState, TokenUsage } from './domain/types.ts'
import { personaIdFor, resolveModelTier } from './domain/modelTier.ts'
import { isHarnessReady } from './connect/harness.ts'
import { monitorHarnessEndpoint } from './connect/harnessEndpoint.ts'
import { endpointScopeOf, subjectEndpointPorts } from './connect/endpoints.ts'
import { HARNESS_STATE_DETAILS } from './connect/harnessLabels.ts'
import { isEmbeddedShellSubject, isInstalledCliSubject, reachNeedsBrowser } from './connect/harnessSubjects.ts'
import {
  apiModelDirectory,
  bridgeModelDirectory,
  modelIdsFor,
  modelUnavailableReason,
  selectedModelFor,
  unavailableDirectory,
  type ModelDirectory,
} from './connect/modelDirectory.ts'
import { PersonaRegistry } from './persona/registry.ts'
import { IdleScene } from './scenes/IdleScene.tsx'
import { MultiScreenIdleScene } from './scenes/MultiScreenIdleScene.tsx'
import { MultiScreenWakeScene } from './scenes/MultiScreenWakeScene.tsx'
import { SleepScene } from './scenes/SleepScene.tsx'
import { WakeScene } from './scenes/WakeScene.tsx'
import { INITIAL_RUNTIME_STATE, reduceRuntime } from './scenes/stateMachine.ts'
import { BACKGROUND_OPTIONS, DEFAULT_DAY_BOUNDARY_HOUR, applyBubbleOverrides, assetUrl, assistantDay, loadSettings, normalizeReceivedSettings, resumeConversationId, saveConversationPointer, saveSettings, type WallpaperSettings } from './settings/store.ts'
import { nativeRuntime, type DesktopDisplayInfo, type ManagedDshAutostart, type NativeSendOptions } from './native/runtime.ts'
import { reportNativeBootstrapReady } from './native/bootstrapHandoff.ts'
import { listen } from '@tauri-apps/api/event'
import type { AppSurface } from './surface.ts'
import { beginInteractionRegionSession, collectInteractionRegions, publishInteractionRegions } from './runtime/interactionRegions.ts'
import type { AppearanceSlot } from './appearance/theme/index.ts'
import { nativeAppearance } from './native/appearance.ts'
import { appCoreClient } from './runtime/appCoreClient.ts'
import { suppressNativeContextMenu } from './runtime/contextMenu.ts'
import { shouldApplyAppSnapshot } from './runtime/appSnapshot.ts'
import type { DesktopWorkspace } from './runtime/desktopWorkspace.ts'
import { WidgetHost } from './widgets/WidgetHost.tsx'
import { displayCssRect, displayTopologySignature, displayUiScale, preferredDisplayId, virtualDesktopBounds } from './runtime/displayLayout.ts'
import { listenUntilDisposed } from './runtime/lifecycle.ts'
import { frameSchedulerTarget, StreamTextBuffer } from './features/chat/streamRender.ts'

const registry = new PersonaRegistry()

/**
 * Fallback cadence for the expanded-layout inset. It is a safety net for a
 * taskbar/topology change that emits no event, not the primary trigger: the
 * measurement itself runs on display-changed, resize, and workspace
 * transitions.
 */
export const LAYOUT_METRICS_FALLBACK_INTERVAL_MS = 30_000
/** Fallback cadence for re-reading the display list; `display-changed` is the
 * real trigger. */
export const DISPLAY_TOPOLOGY_FALLBACK_INTERVAL_MS = 30_000

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

/**
 * Process-wide record that the automatic DSH start has been requested.
 *
 * Module scope rather than a ref on purpose: a React remount would reset a ref
 * and issue a second request. This is only a cheap short-circuit — the native
 * `ManagedDshAutostartState` is the guarantee that survives a reload, a second
 * WebView, or HMR, because a fresh JS realm starts with this flag false again.
 */
let dshAutostartRequestedInProcess = false

/**
 * Which class the last launch started, so the 45-second supervision knows whether
 * "not managed" means anything.
 *
 * A checkout is a child of this process and its exit is observable; a shell is an
 * application the Windows shell starts on our behalf, and nothing about it is
 * owned here. Same module-scope reasoning as above: this survives a remount, and
 * it is only a label — the native side decides what actually happens.
 */
let harnessLaunchedKind: 'embedded-shell' | 'checkout' | 'installed-cli' = 'checkout'

export function canAutoSelectHarness(
  availability: RuntimeState['harness'],
  backend: BackendMode,
  autoSwitchHarness: boolean,
): boolean {
  return isHarnessReady(availability) && backend !== 'harness' && autoSwitchHarness
}

/**
 * 主体**彻底退出**（availability 为 `offline`）时回到左侧。
 *
 * 用户明确要求复位，理由很实在：**拉起 harness 的入口就在壁纸里**——模式滑槽切到 Harness
 * 那一侧会走"没在运行就启动"的流程。不复位的话，用户想把它重新拉起来还得先手动切一次 ✗，
 * 多一步无谓的摩擦。
 *
 * 记录不会因此丢：Harness 会话 id 是按日期命名的（`wallpaper-YYYY-MM-DD`），转写留在宿主的
 * 会话存储里，切回 Harness 时会自动接上当天那个会话——"缓存最后一份记录"是结构性的。
 *
 * 只在 `offline` 生效：`bridge-loading` 是"正在起来"，那时把用户弹回左侧才是 bug。
 */
export function harnessFallbackBackend(
  availability: RuntimeState['harness'],
  backend: BackendMode,
  defaultBackend: WallpaperSettings['defaultBackend'],
): WallpaperSettings['defaultBackend'] | undefined {
  if (backend !== 'harness' || availability !== 'offline') return undefined
  // 配置的默认后端就是 harness 时落到网页入口，绝不停在一个已经不在运行的后端上。
  return defaultBackend === 'harness' ? 'deepseek-web' : defaultBackend
}

/**
 * 主体回来之后，壁纸是否可以自己把滑槽拨回 Harness。
 *
 * `autoResetByWallpaper` 是这条规则的全部要害：只有**壁纸自己**把滑槽复位过，它才可以把滑槽
 * 拨回去。用户手动拨过去的情况，壁纸既没打断他，也不该反过来打断他。
 *
 * 复位保留的是**转写**：转写留在宿主那边（会话按日期命名），所以拨回来时自动接上同一个会话，
 * 轨道里那段记录说的就是同一个会话。
 */
export function shouldReturnToHarness(
  availability: RuntimeState['harness'],
  backend: BackendMode,
  autoResetByWallpaper: boolean,
): boolean {
  return autoResetByWallpaper && backend !== 'harness' && isHarnessReady(availability)
}

/**
 * "上次是壁纸自己把滑槽拨离 Harness 的"这件事是否成立。
 *
 * 它存在两处，缺一不可：内存里的 `ref` 管**本次运行**，设置里的时间戳管**重启之后**——升级安装
 * 必然重启壁纸，而壁纸自己做过的事不会因为重启就不算数。只存内存的后果实测过：桥回来、灯是绿的，
 * 滑槽却停在左侧，用户以为还在跟 DSH 说话，而输入进的是另一个后端（"输入被吞"）。
 *
 * 这条判断只回答"壁纸有没有这个权利"，不回答"现在该不该拨"——后者还得看桥是否就绪
 * （`shouldReturnToHarness`）。用户手动拨动滑槽会同时清掉这两处凭据，他选的那一侧永远优先。
 */
export function harnessResetClaimed(inMemory: boolean, persistedAt: number | undefined): boolean {
  return inMemory || (typeof persistedAt === 'number' && Number.isFinite(persistedAt))
}

/**
 * 黄灯：连接与断开之间的**中间态**。
 *
 * 用户要的三段语义是"连上（绿）／正在连（黄呼吸）／不在了（熄灭）"，所以中间态覆盖三种真实
 * 情形：正在把 harness 后台拉起来（点滑槽或开机自启）、宿主已经应答但 Bridge 仍在装载、
 * 以及已经连上却暂时不应答（还没判死）。少了前两种，用户在"启动中"看到的是一盏熄灭的灯，
 * 那和"后端已经死了"看起来一模一样——而这两件事要让用户做的动作完全不同。
 *
 * 纯函数，因为这条规则是**灯的语言**，不该埋在三元表达式里：它连着"复位滑槽"和"自动回切"，
 * 判断错了用户就会在错误的时刻被搬走。
 */
export function isHarnessTransitioning(state: {
  /** 壁纸正在启动（或等待）已配置的主体。 */
  starting: boolean
  availability: RuntimeState['harness']
  /** 就绪过的桥接正在失联，但还没到"确认掉线"。 */
  probing: boolean
}): boolean {
  return state.starting || state.probing || state.availability === 'bridge-loading'
}

/**
 * 驱动立绘形态的那个模型。
 *
 * 用户实测 bug：Harness 里切到 pro → 立绘成年；切到网页再切回来 → 立绘变幼年，而选择器里
 * 仍然是 pro。原因是形态原来读的是 `runtime.model`，而它会被新后端的 `model` 事件覆盖：
 * Harness 重连时宿主报的是**它自己的**当前模型，未必是壁纸这次会话选定的那个。
 *
 * 所以形态跟**选择器显示的那个模型**走（用户原话："立绘切换应当和模型切换共享一个生命周期"）：
 * 它来自壁纸自己的选择与持久化，也正是这次会话真正要用的模型。网页入口是唯一例外——它的模型
 * 由 DeepSeek 页面决定，壁纸没有选择，只能读页面报回来的值。
 */
export function portraitTierModel(
  backend: BackendMode,
  pageModel: string | undefined,
  selectedModel: string | undefined,
): string | undefined {
  return backend === 'deepseek-web' ? pageModel : selectedModel
}

/**
 * 事件带来的层级，只有"知道的"才写回运行状态。
 *
 * 原生侧的 `model` 事件固定带 `unknown`（它不解释模型名，规则在渲染端），照抄会把
 * "上一次的层级"这个兜底归零——那是立绘在切后端时跳回幼年的另一半原因。
 */
export function tierPatchFromEvent(tier: ModelTier | undefined): Pick<RuntimeState, 'modelTier'> | Record<string, never> {
  return tier && tier !== 'unknown' ? { modelTier: tier } : {}
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

/**
 * One sentence per Harness state, so the composer notice and the bubble badge
 * cannot describe the same condition differently. Defined in
 * `connect/harnessLabels.ts`; re-exported here for existing importers.
 */
export { HARNESS_STATE_DETAILS, harnessStateLabel } from './connect/harnessLabels.ts'

export function harnessSelectionUnavailableError(availability: RuntimeState['harness']): string {
  return `${HARNESS_DISCONNECTED_ERROR_PREFIX}${HARNESS_STATE_DETAILS[availability]} Harness 模式只能在兼容 Bridge 就绪后切换。`
}

/**
 * Actionable text for a failed automatic DSH start.
 *
 * Each code names the concrete thing the user must fix, and `null` means there
 * is nothing to report (the launch is in progress, or the process was started).
 * No entry may include a path from the OS, a token, or an exception body.
 */
export function dshAutostartNotice(result: ManagedDshAutostart): string | null {
  switch (result.outcome) {
    case 'started':
    case 'already-attempted':
    case 'port-occupied-external':
      return null
    case 'root-path-missing':
      return '已开启「随壁纸启动 DSH」，但尚未选择执行主体；请在设置中心扫描并选择一个。'
    case 'unknown-target':
      return '已开启「随壁纸启动 DSH」，但所选执行主体不可用；请在设置中心重新扫描后选择。'
    case 'started-unconfirmed':
      return '已请求启动所选客户端，但它在超时时间内没有应答；若界面始终没有出现，请确认该客户端仍已安装。'
    case 'already-running':
      return null
    case 'root-path-invalid':
      return '已开启「随壁纸启动 DSH」，但配置的根目录不是可识别的 DSH 项目；请在设置中心修正。'
    case 'launcher-missing':
      return '已开启「随壁纸启动 DSH」，但未找到 Node.js 或 pnpm。请在设置中心填写启动命令的完整路径，或安装后重试。'
    case 'profile-invalid':
      return '已开启「随壁纸启动 DSH」，但配置的 profile 名称无效（只能包含字母、数字、连字符或下划线）。请在设置中心修正。'
    case 'command-not-confirmed':
      return '「随壁纸启动 DSH」不会在无人值守时执行自定义启动命令。请在设置中心确认使用该命令，或清空它改用内建启动器。'
    default:
      return '已开启「随壁纸启动 DSH」，但进程启动失败。请在设置中心检查根目录与启动命令。'
  }
}

/**
 * Reconcile an in-flight automatic (or manual) launch with what the Bridge
 * reports. Kept pure and exported so the reason a user sees is decided by one
 * tested function rather than by nested conditions inside an interval.
 *
 * Returns `null` while the launch is still legitimately pending.
 */
/** 启动后多久才允许下"它退出了"这个结论（宽限期内不下结论，见函数内注释）。
 *
 * 各类别不一样长，因为它们的启动成本不一样：源码目录直接跑本机二进制，已安装的 CLI 要先经过
 * 一层 npm 批处理（cmd）再拉起 node，冷启动明显更慢。同一条时限套在所有人身上，就是"刚报错
 * 就连上"的来源 —— 报错早于事实。
 */
export const IMMEDIATE_EXIT_GRACE_MS = {
  checkout: 8_000,
  'installed-cli': 20_000,
} as const

/** 只有非壳主体才是本进程能观察"退出"的孩子，所以只有它们有宽限。 */
function exitGraceMs(launchedKind: 'embedded-shell' | 'checkout' | 'installed-cli'): number {
  return launchedKind === 'embedded-shell' ? 0 : IMMEDIATE_EXIT_GRACE_MS[launchedKind]
}

export function harnessLaunchOutcome(
  status: { availability: RuntimeState['harness']; reasonCode?: string },
  elapsedMs: number,
  managed: { managed: boolean; running: boolean },
  timeoutMs = 45_000,
  /**
   * Which class was started. Only a checkout is a child this process owns, so only
   * a checkout can be observed *exiting*: for a shell the process that answers is
   * not ours, and "not managed" would otherwise be reported as "started and
   * immediately exited" for a client that is running perfectly well.
   */
  launchedKind: 'embedded-shell' | 'checkout' | 'installed-cli' = 'checkout',
): { message: string } | null {
  if (status.availability === 'bridge-ready') return null
  // "还没被我管起来"不等于"已经退出了"：宿主起来要几秒（实测：启动 22:27:54、端口与门票
  // 22:27:56；已安装 CLI 还要先经过一层 cmd 与批处理）。各类别给各自长度的宽限期，期内不下
  // 结论 —— 否则用户会先看到"启动后很快退出"，两秒后指示灯又变绿：一次假警报，比不说更糟。
  if (
    launchedKind !== 'embedded-shell'
    && elapsedMs > exitGraceMs(launchedKind)
    && (!managed.managed || !managed.running)
  ) {
    return { message: 'DSH 启动后很快退出；请检查 DSH 配置或启动日志。' }
  }
  if (elapsedMs <= timeoutMs) return null
  // Past the deadline the most specific available cause wins: a Bridge that
  // answered but cannot be used is a different fix from one that never appeared.
  if (status.availability === 'bridge-auth-unavailable') {
    return { message: 'DSH 已启动，但 Bridge 本机令牌不可用；请重启壁纸应用或检查令牌目录权限。' }
  }
  if (status.availability === 'bridge-incompatible') {
    return { message: 'DSH 已启动，但 Bridge 版本或能力不兼容；请更新 Bridge 后重试。' }
  }
  if (status.availability === 'bridge-loading') {
    return { message: 'DSH 已启动，Bridge 仍在装载会话服务；若长期停留，请检查 DSH 日志。' }
  }
  return { message: 'DSH 启动超时；进程仍在运行但 Bridge 尚未上线。' }
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

/**
 * 壁纸新建 Harness 会话时默认装载的 agent 预设。
 *
 * 用户要求："工作区的预设先默认为'极简模式'试试，应该能省不少上下文"。宿主那边的 id 实测是
 * `minimal`（预设表：`standard` 默认 / `minimal` / `ptc` / `cordis`）。
 */
export const DEFAULT_HARNESS_PRESET = 'minimal'

/**
 * 切换主体之后，黄灯（"连接中"）至少要亮这么久。
 *
 * 用户的要求是"进入固定 1–2s 的黄灯缓冲后**立刻载入**"：答案往往一次性就回来了，如果灯跟着答案
 * 一闪而过，用户根本看不到"它在连"，只会觉得界面抖了一下。所以这是**最短停留**，不是超时。
 *
 * 顺带说清"缓存"在这里指什么：**不是缓存会话 id**。桥的规矩是"只有没有 resume id 时才去建
 * '今天这条桌面会话'"（每日边界 04:00，你们自己定的），把昨天的 id 存下来再恢复就等于悄悄破了
 * 那条边界。所以切回来时该做的是"端点保持热 + 这一小段缓冲"，让宿主用一次往返把今天的会话交回来。
 */
export const HARNESS_SWITCH_BUFFER_MS = 1_200

/**
 * Harness 适配器的选项：模型 + 预设。
 *
 * **端点不在这里**：`connect_harness` 命令自己按主体范围解析端口（源码注释原话"resolved here
 * rather than trusted from the caller"——渲染端不许指定任意端口，监视器读同一个值，于是状态与
 * 会话不会指着两个不同的客户端）。曾经在这里算过一个 `endpointPort` 交出去，命令根本不看它：
 * 一个死参数，已删。
 */
export function harnessAdapterOptionsFromSettings(
  settings: Pick<WallpaperSettings, 'harnessPreset'>,
  model: string | undefined,
): NativeSendOptions {
  return { model, preset: settings.harnessPreset ?? DEFAULT_HARNESS_PRESET }
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
  subjectScope: string,
): string {
  return `${backend}:${conversationGeneration}:${subjectScope}`
}

/**
 * 主体范围在生命周期键里的形态。
 *
 * 主体**必须**进这个键：切换主体不会重建聊天适配器，而适配器持有的正是"连着哪一个端点、哪一条
 * 会话"。实测的边界情况是：从 CLI 切到官壳时，原生侧收到了新的探测范围（指示灯因此变绿），但事件
 * 流与那条会话仍留在旧端点上 —— 用户看到的就是"灯说已连接，一发消息却说会话尚未建立"。
 */
export function subjectScopeKey(launch: {
  subjectId?: string
  rootPath?: string
  endpointPort?: number
  extraEndpointPorts?: readonly number[]
}): string {
  const scope = endpointScopeOf(launch)
  return [scope.subjectId ?? '', scope.endpointPort ?? '', (scope.extraPorts ?? []).join(',')].join('|')
}

/**
 * Native actions are serialized in Rust, but their IPC responses can arrive
 * out of order in a busy WebView.  AppCore increments `revision` for every
 * state change; only snapshots at or after the last applied revision may
 * repaint the renderer.  Without this guard a late `sending` snapshot could
 * overwrite a newer `done` event and leave the composer stuck.
 */
/**
 * Daily sessions roll over on a real session return, rather than on an
 * arbitrary timer. This is important for a resident wallpaper: it may stay
 * alive across midnight with yesterday's adapter still mounted.
 */
export function shouldStartNewConversationOnUnlock(
  policy: WallpaperSettings['conversationPolicy'],
  previousUnlockDay: string,
  now: Date = new Date(),
  boundaryHour: number = DEFAULT_DAY_BOUNDARY_HOUR,
): boolean {
  return policy === 'new-on-unlock'
    || (policy === 'daily' && previousUnlockDay !== assistantDay(now, boundaryHour))
}

/**
 * WebView2 keeps cookies and page state, but the DeepSeek root route can still
 * select a different conversation after a reload. When there is no pointer
 * to resume, the non-resume policies must explicitly start from that root;
 * `resume-last` intentionally adopts the current page on first use.
 */
export function shouldStartNewWebConversation(
  policy: WallpaperSettings['conversationPolicy'],
  resumeId: string | undefined,
): boolean {
  return policy === 'new-on-unlock' || (policy === 'daily' && !resumeId)
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

  return {
    activity: 'idle',
    error: `${HARNESS_DISCONNECTED_ERROR_PREFIX}${HARNESS_STATE_DETAILS[availability]} 已保留当前 Harness 会话和对话记录；Bridge 恢复后可继续，或由你手动切换后端。`,
  }
}

/**
 * 通知条上该显示哪一句。
 *
 * 两句话来自两个不同的世界：`chatNotice` 是**聊天层**说的（"这条会话被拒绝了，已换新会话重发"），
 * `error` 是**原生宿主**说的（Bridge 断开等）。必须分开存，因为 `error` 会被**原生快照整体覆写**
 * （`error: snapshot.error`）——以前两者挤在同一个字段里，聊天层的通知刚写进去就被下一条快照擦掉，
 * 用户只看到"顶上闪了一下"（实测）。聊天层那句更新、更针对此刻，所以它在前面。
 */
export function visibleNotice(state: Pick<RuntimeState, 'chatNotice' | 'error'>): string | undefined {
  return state.chatNotice ?? state.error
}

export interface AppProps { surface?: AppSurface }

export function App({ surface = 'combined' }: AppProps) {
  const [settings, setSettings] = useState<WallpaperSettings>(() => loadSettings())
  const [runtime, baseDispatch] = useReducer(reduceRuntime, { ...INITIAL_RUNTIME_STATE, backend: settings.defaultBackend })
  const [resolvedAssets, setResolvedAssets] = useState<Partial<Record<AppearanceSlot, string>>>({})
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [streamingText, setStreamingText] = useState('')
  const [questionPrompt, setQuestionPrompt] = useState<ChatQuestion[]>()
  const [usage, setUsage] = useState<TokenUsage>()
  const [conversationGeneration, setConversationGeneration] = useState(0)
  const [apiModelChoice, setApiModelChoice] = useState(settings.deepseekApi.model)
  const [harnessModelChoice, setHarnessModelChoice] = useState<string | undefined>()
  // 枚举出来的模型目录。初始是"还没查"，不是"没有模型"——区别见 connect/modelDirectory.ts。
  const [harnessModelDir, setHarnessModelDir] = useState<ModelDirectory>({ kind: 'unavailable', reason: '正在读取 Harness 模型目录…' })
  /** 就绪过的桥接正在失联（黄灯），但还没到"确认掉线"。 */
  const [harnessProbing, setHarnessProbing] = useState(false)
  const [apiModelDir, setApiModelDir] = useState<ModelDirectory>({ kind: 'unavailable', reason: '正在读取端点模型目录…' })
  const [interactionState, setInteractionState] = useState<'collapsed' | 'expanded'>('collapsed')
  const [interactionEnabled, setInteractionEnabled] = useState(true)
  // The composer draft lives here rather than in the bubble because returning to
  // the desktop has to rebuild the input area (that rebuild is what restores the
  // keyboard channel), and component state would not survive it.
  const [chatDraft, setChatDraft] = useState('')

  // Report island pointer events to the native side, the only side that can act on
  // them: a real click never reaches the native window procedure, because the WebView2
  // child owns the mouse messages from another process.
  //
  // A report, not a trigger. The native command re-checks the physical left button and
  // the window under the cursor, so this cannot become an unverified activation path.
  useEffect(() => {
    if (!nativeRuntime.isNative) return
    // 桌面不是网页：背景插画上的右键会弹出 Chromium 的"图像另存为/复制图像链接/更多工具"，
    // 用户明确要求冻结它。输入框里的菜单保留（见 `runtime/contextMenu.ts`）。
    return suppressNativeContextMenu()
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    const reportIslandClick = (event: PointerEvent) => {
      if (event.button !== 0) return
      void nativeRuntime.verifyIslandClick().catch((error) => {
        if (import.meta.env.DEV) console.debug("verify island click rejected:", error)
      })
    }
    document.addEventListener("pointerdown", reportIslandClick, true)
    return () => document.removeEventListener("pointerdown", reportIslandClick, true)
  }, [])
  const [workspace, setWorkspace] = useState<DesktopWorkspace>('front')
  // Long-lived listeners read the workspace through a ref (same reason as
  // `settingsRef`): a captured value would be stale by the time the snapshot
  // arrives, and choosing front/inner is exactly what decides whether the
  // docked interaction surface may be collapsed.
  const workspaceRef = useRef<DesktopWorkspace>('front')
  workspaceRef.current = workspace
  const [innerHistoryExpanded, setInnerHistoryExpanded] = useState(false)
  const [expandedBottomInset, setExpandedBottomInset] = useState(48)
  const [desktopDisplays, setDesktopDisplays] = useState<DesktopDisplayInfo[]>([])
  const [presetOptions, setPresetOptions] = useState<Array<{ id: string; name?: string; broken?: string; isDefault: boolean }>>([])
  const [selectedPreset, setSelectedPreset] = useState<string>()
  const [harnessControls, setHarnessControls] = useState<{ permission: { current: string; options: string[] }; commands: Array<{ name: string; description: string; input?: { hint: string } }> }>()
  const [harnessStarting, setHarnessStarting] = useState(false)
  /**
   * 握手**失败**过（不是"还没连"）。
   *
   * 与黄灯（连接中）必须分开：黄灯是"还没定"，红灯是"试过了、没成"。用户要的动作也不同——
   * 红灯时点滑槽＝立刻拉起对应 harness 进程并发一次握手申请，而黄灯时只需要等。
   */
  const [harnessFailed, setHarnessFailed] = useState(false)
  /** 切换主体之后黄灯的**最短**停留（见 `HARNESS_SWITCH_BUFFER_MS`）。 */
  const [harnessBuffering, setHarnessBuffering] = useState(false)
  const [nativeHandoffGeneration, setNativeHandoffGeneration] = useState<number>()
  const harnessLaunchPendingRef = useRef(false)
  const harnessLaunchStartedAtRef = useRef<number>()
  /**
   * 上面那个失败分支要用的两样东西，用 ref 拿最新值。
   *
   * 那个 effect 只在 `[harnessStarting, runtime.harness]` 变化时重建，直接闭包捕获 `changeBackend`
   * 会拿到**过期的**那一份（它每次渲染都重建）。失败复位滑槽是一次性动作，必须用当下这一份。
   */
  const changeBackendRef = useRef<((backend: 'deepseek-web' | 'deepseek-api' | 'harness', options?: { keepTranscript?: boolean; automatic?: boolean }) => void) | undefined>(undefined)
  const nonHarnessBackendRef = useRef<'deepseek-web' | 'deepseek-api'>('deepseek-web')
  /**
   * 壁纸自己把滑槽复位过（主体退出），因此它有权在主体回来后自己拨回去。
   *
   * 只在**我们自己**复位时置位：用户自己拨到左侧时，壁纸没有打断他，也就不该反过来打断他。
   * 用户手动选后端会清掉它。ref 与 state 各司其职：ref 给事件回调读（浏览器预览那条路的闭包
   * 是旧的），state 给渲染读（轨道里那条"上次的 Harness 会话"要用它）。
   */
  const autoResetFromHarnessRef = useRef(false)
  const [autoResetFromHarness, setAutoResetFromHarness] = useState(false)
  const markAutoResetFromHarness = (value: boolean) => {
    autoResetFromHarnessRef.current = value
    setAutoResetFromHarness(value)
    // 同一件事写进设置，让它活过下一个进程：装一次新版就重启一次壁纸，只记在内存里的规则
    // 会在重启那一刻悄悄失效（见 `harnessResetClaimed`）。用户手动选后端会清掉它。
    const current = settingsRef.current
    if (value) {
      saveSettings({ ...current, harnessAutoResetAt: Date.now() })
    } else if (current.harnessAutoResetAt !== undefined) {
      saveSettings({ ...current, harnessAutoResetAt: undefined })
    }
  }
  const adapterRef = useRef<ChatAdapter>(new PreviewAdapter(settings.defaultBackend))
  const chatActivityRef = useRef<{ adapter: ChatAdapter; backend: BackendMode; activity: RuntimeState['activity'] }>()
  // Chat activity actions are emitted from event callbacks without awaiting
  // each IPC call. Serialize this queue so a late `streaming` dispatch cannot
  // be processed after the terminal `done` dispatch.
  const coreDispatchQueueRef = useRef<Promise<unknown>>(Promise.resolve())
  /**
   * 切换主体之后，黄灯**至少**亮满 `HARNESS_SWITCH_BUFFER_MS`。
   *
   * 依赖与下面那个"把主体范围交给原生"的 effect 相同 —— 同一件事触发两处：原生开始按新主体探测，
   * 界面则先进入"连接中"。启动时也会走一次，那是对的：那时确实还在连。
   */
  useEffect(() => {
    setHarnessBuffering(true)
    const timer = window.setTimeout(() => setHarnessBuffering(false), HARNESS_SWITCH_BUFFER_MS)
    return () => window.clearTimeout(timer)
  }, [
    settings.dshLaunch.subjectId,
    settings.dshLaunch.rootPath,
    settings.dshLaunch.endpointPort,
    settings.dshLaunch.extraEndpointPorts,
  ])
  // Do not put mutable API request settings in the adapter lifecycle effect.
  // The adapter captures this object by reference and snapshots it only when
  // sending, so a settings edit changes the next request without disconnecting
  // an in-flight stream or dropping its scoped events.
  const apiAdapterOptionsRef = useRef<NativeSendOptions>(apiAdapterOptionsFromSettings(settings))
  const conversationPolicyRef = useRef(settings.conversationPolicy)
  // Keep the day from the last session return, not from the last render. A
  // long-running process therefore notices the day boundary when the user
  // returns to the desktop and asks for a daily conversation. 「助手日」= 本地
  // 04:00 起算，深夜还在做的事不会被零点切走（`assistantDay`）。
  const previousUnlockDayRef = useRef(assistantDay(new Date(), settings.dayBoundaryHour))
  // This ref is updated synchronously by user/backend actions. React state is
  // intentionally asynchronous, so runtimeRef alone would leave a short gap
  // in which an old adapter could finish and persist its pointer under a new
  // backend selection.
  const activeBackendRef = useRef<BackendMode>(runtime.backend)
  activeBackendRef.current = runtime.backend
  const runtimeRef = useRef(runtime)
  runtimeRef.current = runtime
  /**
   * The latest settings, for callbacks and subscriptions that outlive one render.
   *
   * This exists so that a *subscription* never has to name a setting in its dependency
   * list. Naming one there looks harmless and is not: the boot effect below used to
   * depend on `settings.interactionLayout`, so changing that one preference tore the
   * effect down and re-ran it — and re-running it dispatched `boot-ready` again, which
   * is the event the state machine reads as "this desktop has just started waking up".
   * The wake animation therefore replayed on every layout change. A setting that only
   * decides *how* something is drawn must not be able to re-trigger an event that means
   * "this session just started".
   */
  const settingsRef = useRef(settings)
  settingsRef.current = settings

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void listen<number>('native-handoff-generation', (event) => {
      setNativeHandoffGeneration((current) => Math.max(current ?? 0, event.payload))
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch((error) => console.warn('native hand-off generation listener failed', error))
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    // Every Tauri WebView owns an isolated storage partition, so the saved
    // endpoint exists in this surface's storage but the native monitor — which
    // owns the probe loop and resolves the endpoint sessions use — has its own
    // state. Pushing it at startup is what makes a saved choice survive a
    // restart; relying on the settings window to push it meant a user who never
    // reopened settings got the default port instead.
    // The *subject* is the half that matters: it is what decides which ports may be
    // probed at all, so native can no longer connect to whichever client answers.
    void nativeRuntime.setHarnessEndpointScope(endpointScopeOf(settings.dshLaunch)).catch(() => null)
  }, [
    settings.dshLaunch.subjectId,
    settings.dshLaunch.rootPath,
    settings.dshLaunch.endpointPort,
    settings.dshLaunch.extraEndpointPorts,
  ])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    void nativeRuntime.nativeBootstrapGeneration().then((generation) => {
      if (!disposed) setNativeHandoffGeneration((current) => Math.max(current ?? 0, generation))
    }).catch((error) => console.warn('native hand-off generation query failed', error))
    return () => { disposed = true }
  }, [runtime.phase])
  const appSnapshotRevisionRef = useRef(-1)
  /**
   * `autoStartWithWallpaper` starts a resident service, so it must fire from the
   * background host exactly once per process.
   *
   * Two layers, deliberately. This module-scope marker survives a React remount
   * of the component (a ref would not, so a remount would issue a pointless IPC
   * round trip), and the native `ManagedDshAutostartState` is the actual
   * guarantee: it remembers the attempt for the process lifetime and refuses a
   * repeat, which also covers a second WebView, a reload, or an HMR pass.
   */
  const dshAutostartRequestedRef = useRef(false)
  /**
   * Streaming text arrives one delta per model chunk. Committing each of them
   * as its own React update (and its own auto-scroll) is what made a long
   * answer re-render hundreds of times, so the deltas are coalesced into one
   * commit per animation frame. Order is preserved exactly: appends
   * concatenate in arrival order.
   */
  const streamBufferRef = useRef<StreamTextBuffer>()
  if (!streamBufferRef.current) {
    streamBufferRef.current = new StreamTextBuffer(frameSchedulerTarget(), (text) => {
      setStreamingText((value) => value + text)
    })
  }
  const desktopDisplaysSignatureRef = useRef('')
  // Resolving library data URLs can finish out of order.  Each refresh gets a
  // monotonic epoch, so a slower pre-change resolve can never repaint the
  // previous background/persona after a user has selected a new one.
  const appearanceRefreshEpochRef = useRef(0)
  const patchRuntime = (patch: Partial<RuntimeState>) => baseDispatch({ type: 'PATCH', patch })
  const dispatchCore = (action: Parameters<typeof appCoreClient.dispatch>[0], options?: Parameters<typeof appCoreClient.dispatch>[1]) => {
    if (!appCoreClient.native) return
    coreDispatchQueueRef.current = coreDispatchQueueRef.current
      .catch(() => undefined)
      .then(() => appCoreClient.dispatch(action, options))
      .catch((error) => patchRuntime({ error: String(error) }))
  }

  // 背景与立绘形态无关，留在这里；**形态那一串（tier / persona / 立绘槽位）搬到了模型选择之后**，
  // 因为它现在读的是"选择器正在显示的那个模型"，而那个值要到后面才算得出来。
  const background = BACKGROUND_OPTIONS.find((item) => item.id === settings.background)
  const resolvedBackground = resolvedAssets['desktop.background']
  const multiScreenActive = settings.multiScreen.enabled && desktopDisplays.length > 1
  const displayVirtualBounds = useMemo(() => virtualDesktopBounds(desktopDisplays), [desktopDisplays])
  const conversationDisplayId = preferredDisplayId(desktopDisplays, settings.multiScreen.conversationDisplayId)
  const portraitDisplayId = preferredDisplayId(desktopDisplays, settings.multiScreen.portraitDisplayId)
  const screenBackgroundUrls = useMemo(() => Object.fromEntries(desktopDisplays.map((display) => {
    const selected = settings.multiScreen.backgrounds[display.id] ?? settings.background
    const option = BACKGROUND_OPTIONS.find((item) => item.id === selected)
    const url = selected === settings.background && resolvedBackground
      ? resolvedBackground
      : option?.path
        ? assetUrl(option.path)
        : undefined
    return [display.id, url] as const
  })), [desktopDisplays, resolvedBackground, settings.background, settings.multiScreen.backgrounds])
  // 可切换模型的来源是**枚举出来的**，不是硬编码的 id 列表。
  //
  // 以前这里写死了三个候选（`deepseek-v4-flash-vision-exp` / `deepseek-v4-flash` /
  // `deepseek-v4-pro`），而宿主真实目录是 `deepseek-flash`、`deepseek-v4-pro`——前两个
  // 根本不存在，用户选了也只会被拒绝，看起来就是"匹配不到模型列表"。现在名单来自
  // 宿主自己的目录（经桥接转发），三种执行主体各自问自己的宿主；问不到时（老版本宿主、
  // 端点没有 /models、或还没连上）退化成"只显示当前模型"，并且**把原因说出来**，
  // 不再拿一串假的 id 充数。三种情况的区别由 `modelDirectory.ts` 归一。
  // 记住的选择，但只在这台宿主**仍然提供它**时采用。
  //
  // 回显上次用的模型是用户要的行为；可宿主可能已经不再提供它（换了模型目录、换了主体），
  // 那种情况下按"没记住"处理，否则选择器会摆出一个宿主根本不认识的 id。
  const persistedHarnessModel = useMemo(() => {
    const saved = settings.harnessModel.model?.trim()
    if (!saved) return undefined
    if (harnessModelDir.kind === 'enumerated' && !harnessModelDir.models.some((model) => model.id === saved)) return undefined
    return saved
  }, [settings.harnessModel.model, harnessModelDir])
  // 选择器里能选的模型：**每个后端只认自己的来源**。
  //
  // 这里踩过一次真实的坑：`runtime.model` 是三个后端**共用**的一个字段，API 侧把它写成
  // `deepseek-chat`（那是 API 自己的模型 id，服务端会映射到 flash）之后，切到网页/Harness 时
  // 这个值还在，于是网页端右下角会显示一个 Harness 目录里根本不存在的 `deepseek-chat`。
  // 网页入口的模型由 DeepSeek 页面决定，壁纸这边没有可选项——给空列表比给一个错的 id 诚实。
  const modelOptions = useMemo(() => {
    if (runtime.backend === 'deepseek-api') return modelIdsFor(apiModelDir, settings.deepseekApi.model, apiModelChoice)
    if (runtime.backend === 'harness') {
      // 宿主自己的当前模型（枚举结果里的 `current`）优先于共用的 `runtime.model`：
      // 后者可能带着别的后端的值，而 `current` 一定来自这台 Harness 宿主。
      const hostCurrent = harnessModelDir.kind === 'unavailable' ? undefined : harnessModelDir.current
      return modelIdsFor(harnessModelDir, persistedHarnessModel ?? hostCurrent, harnessModelChoice)
    }
    return []
  }, [runtime.backend, harnessModelDir, apiModelDir, apiModelChoice, harnessModelChoice, persistedHarnessModel, settings.deepseekApi.model])
  // 枚举到的显示名（DeepSeek-Flash 之类）替掉裸 id；没枚举到就显示 id 本身。
  const modelLabels = useMemo(() => {
    const directory = runtime.backend === 'deepseek-api' ? apiModelDir : harnessModelDir
    if (directory.kind !== 'enumerated') return undefined
    return Object.fromEntries(directory.models.map((model) => [model.id, model.name]))
  }, [runtime.backend, harnessModelDir, apiModelDir])
  const modelSwitchDisabledReason = useMemo(() => {
    // 网页入口的模型由 DeepSeek 页面自己决定，与"枚举不到"是两件事，文案必须分开。
    if (runtime.backend === 'deepseek-web') return '网页入口的模型由 DeepSeek 页面决定'
    return modelUnavailableReason(runtime.backend === 'deepseek-api' ? apiModelDir : harnessModelDir)
  }, [runtime.backend, harnessModelDir, apiModelDir])

  // 枚举当前可切换的模型：Harness 侧问宿主（经桥接转发），API 侧问端点自己的 `/models`。
  // 读取失败不打扰用户（没有 toast）：目录进入 `unavailable`，选择器禁用并把原因写在选项里。
  // 依赖里带上 `runtime.harness`：宿主晚一点才连上时，名单要跟着刷新一次。
  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    if (runtime.backend === 'harness') {
      // 主体不在运行时**不保留旧列表**：选择器要立刻变成"Harness 未运行"并禁用，
      // 这就是"状态刷新之后实时更新模型下拉"的掉线那一半。
      if (!isHarnessReady(runtime.harness)) {
        setHarnessModelDir(unavailableDirectory('Harness 未运行'))
        return () => { disposed = true }
      }
      void nativeRuntime.harnessModels()
        .then((payload) => { if (!disposed) setHarnessModelDir(bridgeModelDirectory(payload)) })
        .catch((error) => { if (!disposed) setHarnessModelDir(unavailableDirectory(`Harness 模型目录读取失败：${String(error)}`)) })
    } else if (runtime.backend === 'deepseek-api') {
      void nativeRuntime.apiModels(settingsRef.current.deepseekApi.baseUrl)
        .then((payload) => { if (!disposed) setApiModelDir(apiModelDirectory(payload, settingsRef.current.deepseekApi.model)) })
        .catch((error) => { if (!disposed) setApiModelDir(unavailableDirectory(`端点模型列表读取失败：${String(error)}`)) })
    }
    return () => { disposed = true }
  }, [runtime.backend, runtime.harness, settings.deepseekApi.baseUrl])
  // 选择器当前值：**空字符串必须当作"没有值"**——这条规则连同上一条串值的教训一起搬进了
  // `connect/modelDirectory.ts::selectedModelFor`，那里也是它被回归测试钉住的地方。
  // 「桌面会话」左侧那枚小图标（点击行为见 ConversationBubble，视觉不变）：单击拉起
  // **当前主体的可视化窗口**。它在**每个后端**都一样可用（用户原话："把每个样式的
  // 「桌面会话」左边的小图标做成拉起对应可视化窗口的快捷键"）——之前只在 Harness 模式下接上
  // 了回调，于是网页模式下它是一枚纯图标：点了什么都不发生，看起来就是"按钮拉不起来窗口"。
  //
  // 规则与设置中心「打开」那套完全一致，关键是**不能只调 raise**：主体没在运行时先把它启动；
  // 官壳有自己的窗口 → 拉到前台；**检出/CLI（webui 形态）根本没有窗口** →
  // 交给默认浏览器打开。走哪条路由**原生的结果**决定（`no-window`），不由这里猜主体形态：
  // 什么都没配置时没有形态可读，猜出来的"无窗口"会让浏览器抢在窗口之前被打开。
  const openSubjectInterface = useCallback(() => {
    void (async () => {
      try {
        const launch = settings.dshLaunch
        const subjectId = launch.subjectId ?? launch.rootPath
        // The subject's own ports, in the subject's own order — never "whatever the
        // scan found answering", which would reach a client the user did not choose.
        const ports = subjectEndpointPorts(endpointScopeOf(launch)) ?? []
        // 0 lets native pick among the subject's own ports, preferring the one that
        // is actually listening: that is what keeps a checkout the user moved to an
        // added port from being started a second time.
        const port = launch.endpointPort ?? (ports.length === 1 ? ports[0]! : 0)
        const ensured = await nativeRuntime.ensureHarnessUi({
          targetId: subjectId,
          port,
          profile: launch.profile,
          command: launch.command,
        })
        if (reachNeedsBrowser(ensured.outcome)) {
          const live = port > 0
            ? port
            : (await nativeRuntime.scanHarnessEndpoints([...ports])).find((item) => item.bridgeFound)?.port
          if (live) await nativeRuntime.openClientInBrowser(live)
          return
        }
        if (ensured.started && ensured.outcome === 'not-running') {
          patchRuntime({ error: 'DSH 主体启动失败，请查看日志中的启动记录。' })
        }
      } catch (error) {
        patchRuntime({ error: `打开可视化窗口失败：${String(error)}` })
      }
    })()
  }, [settings.dshLaunch])
  // 选择器当前值由 `connect/modelDirectory.ts::selectedModelFor` 一处决定：那条规则连着两次
  // 被"后端之间串了值"咬到（共用的 `runtime.model`），所以它必须在能被测到的地方，而不是散
  // 在这里的嵌套三元里。空串按"没有值"处理的原因也写在那个函数上。
  const selectedModel = selectedModelFor({
    backend: runtime.backend,
    chosen: runtime.backend === 'deepseek-api' ? apiModelChoice : harnessModelChoice,
    persisted: persistedHarnessModel,
    configured: settings.deepseekApi.model,
    directory: runtime.backend === 'deepseek-api' ? apiModelDir : harnessModelDir,
    ids: modelOptions,
  })
  /**
   * 立绘形态要跟着**模型选择**走，而不是跟着某个后端"顺手报回来的模型"走。
   *
   * 实测 bug：Harness 里切到 pro → 立绘成年；切到网页再切回来 → 立绘变幼年，而下拉列表里
   * 仍然是 pro。原因是这条判断原来读的是 `runtime.model`，而它会被新后端的 `model` 事件覆盖
   * （Harness 重连时宿主报的是它自己的当前模型，未必是壁纸这次会话实际用的那个）。
   *
   * 用户的要求是"立绘切换和模型切换共享一个生命周期，做好持久化"。所以这里读的正是
   * **选择器显示的那个模型**（`selectedModel`）：它来自壁纸自己的选择与持久化
   * （`settings.harnessModel.model` / `settings.deepseekApi.model`），也正是这次会话真正要用的
   * 模型——重启后先按持久化的值立绘，不必等宿主回报。规则本身在 `portraitTierModel` 上。
   */
  const tier = resolveModelTier(
    runtime.backend,
    runtime.provider,
    portraitTierModel(runtime.backend, runtime.model, selectedModel),
    settings.modelTierRules,
    runtime.modelTier,
  )
  const persona = registry.get(personaIdFor(runtime.backend, tier))
  const bubbles = applyBubbleOverrides(persona.bubbles, settings.bubbleOverrides)
  const personaSlot: AppearanceSlot = runtime.backend === 'harness'
    ? tier === 'pro' ? 'persona.harness.pro' : 'persona.harness.flash'
    : tier === 'pro' ? 'persona.deepseek.pro' : 'persona.deepseek.flash'
  const resolvedPersona = resolvedAssets[personaSlot]
  const modelLabel = runtime.model ?? (tier === 'pro' ? 'Pro · 成年形态' : 'Flash · 幼年形态')
  /**
   * 黄灯：中间态，规则见 `isHarnessTransitioning`（纯函数，可测）。两个来源各自算：原生监控
   * 把 `harnessProbing` 随快照发下来（它能看到端口属主的进程是否还活着），浏览器预览那条路
   * 自己在本地探针里判断。
   */
  const harnessTransitioning = isHarnessTransitioning({
    starting: harnessStarting,
    availability: runtime.harness,
    probing: harnessProbing || runtime.harnessProbing === true,
  })
  // The WorkerW host is permanently desktop-sized. Both the floating window
  // and the taskbar capsule now use CSS placement inside that one viewport.
  const interactionDirection = 'center' as const
  const adapterLifecycleKey = chatAdapterLifecycleKey(runtime.backend, conversationGeneration, subjectScopeKey(settings.dshLaunch))

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
    // Read through the ref: these handlers are held by a long-lived listener, so a
    // captured value would go stale the moment the preference changed.
    setInnerHistoryExpanded(settingsRef.current.historyStartsExpanded)
    setWorkspace('entering-inner')
    setInteractionState('expanded')
    baseDispatch({ type: 'OPEN_CHAT' })
    dispatchCore('open-chat')
    window.setTimeout(() => setWorkspace('inner'), 280)
  }

  const leaveInnerWorkspace = () => {
    setInnerHistoryExpanded(false)
    if (settingsRef.current.interactionLayout === 'floating') {
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

  // The native cover is released only when the active idle scene's images
  // decode and Rust confirms the current WebView/cover host, geometry, and Z
  // order. A stale phase or host generation is discarded.
  useEffect(() => {
    if (!nativeRuntime.isNative || runtime.phase !== 'idle' || nativeHandoffGeneration === undefined) return
    const controller = new AbortController()
    void reportNativeBootstrapReady(nativeHandoffGeneration, nativeRuntime, { signal: controller.signal })
      .then((released) => {
        if (!released && !controller.signal.aborted) console.warn('native hand-off remains covered until its host or renderer is ready')
      })
      .catch((error) => console.warn('native hand-off readiness check failed', error))
    return () => controller.abort()
  }, [nativeHandoffGeneration, runtime.phase])

  useEffect(() => {
    if (!appCoreClient.native) return
    const applySnapshot = (snapshot: Awaited<ReturnType<typeof appCoreClient.snapshot>>) => {
      if (!shouldApplyAppSnapshot(appSnapshotRevisionRef.current, snapshot.revision)) return
      appSnapshotRevisionRef.current = snapshot.revision
      // Native tray/system actions can change the selected backend without
      // going through this WebView's changeBackend callback. Advance the
      // synchronous identity first so an old async adapter cannot win during
      // React's next render/cleanup boundary.
      activeBackendRef.current = snapshot.backend
      const liveChat = chatActivityRef.current
      patchRuntime({
        phase: snapshot.phase,
        backend: snapshot.backend,
        activity: liveChat?.adapter === adapterRef.current && liveChat.backend === snapshot.backend
          ? liveChat.activity
          : snapshot.activity,
        harness: snapshot.harness,
        // The reason and the amber light are part of the same diagnostic; dropping them
        // here while the availability came through is how a state with no explanation
        // reached the island.
        harnessReasonCode: snapshot.harnessReasonCode,
        harnessProbing: snapshot.harnessProbing === true,
        historyExpanded: snapshot.interaction.historyExpanded,
        error: snapshot.error,
      })
      setInteractionEnabled(snapshot.interaction.enabled)
      if (snapshot.phase === 'chatting') {
        setWorkspace((current) => current === 'front' || current === 'leaving-inner' ? 'inner' : current)
        setInteractionState('expanded')
      }
      if (!snapshot.interaction.desktopForeground || snapshot.privacyScreen) {
        // 只在**表桌面**把交互面收回胶囊。
        //
        // 里桌面里岛本身就是主界面。实测：点悬浮球进来后，这条判断紧接着把
        // `interactionState` 收回 `collapsed`，于是岛只出现一帧
        // （原生日志里 `island_visible` 立刻 true→false，DOM 里连问候语都消失），
        // 用户看到的就是"进了里桌面但输入岛没出现"。
        // 桌面是否前台本来就是为"表桌面上盖着应用"准备的条件，在里桌面并不成立。
        if (settingsRef.current.interactionLayout === 'taskbar-docked' && workspaceRef.current === 'front') {
          setInteractionState('collapsed')
        }
      }
    }
    // The subscription is owned by the helper: if this effect is torn down
    // before `subscribe()` resolves, the listener it returns is released
    // immediately instead of being stored in a component that is already gone.
    const listener = listenUntilDisposed(
      (onSnapshot) => appCoreClient.subscribe(onSnapshot),
      applySnapshot,
      { onError: (error) => patchRuntime({ error: String(error) }) },
    )
    void appCoreClient.snapshot()
      .then(applySnapshot)
      .catch((error) => patchRuntime({ error: String(error) }))
    return () => listener.dispose()
    // Deliberately keyed on the surface alone: this is a subscription, and every
    // setting it reads goes through `settingsRef`. Listing a setting here is what
    // re-issued the boot event — see the ref's own comment.
  }, [surface])

  /**
   * Boot is an event, not a setting.
   *
   * `boot-ready` is what the state machine reads as "this desktop has just started
   * waking up", so it is dispatched exactly once per surface — never again because a
   * preference changed. `animationsEnabled` and `skipWakeAnimation` decide whether
   * *this* wake animates; they do not decide that another wake happens, which is why
   * they are read once through the ref instead of being watched.
   */
  useEffect(() => {
    const playWake = settingsRef.current.animationsEnabled && !settingsRef.current.skipWakeAnimation
    const timer = setTimeout(() => {
      if (appCoreClient.native) dispatchCore('boot-ready', { playWake })
      else baseDispatch({ type: 'BOOT_READY', playWake })
    }, 120)
    return () => clearTimeout(timer)
  }, [surface])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    // Settings are authored in a separate WebView. The payload is the
    // source of truth; localStorage here belongs only to this WebView.
    // It still crosses a process boundary as untrusted JSON, so it goes
    // through the same whitelist rebuild as a local load: a malformed or
    // future payload must degrade to defaults per field instead of handing
    // the desktop renderer a value of the wrong type.
    return listenUntilDisposed<WallpaperSettings>(
      (emit) => listen<WallpaperSettings>('settings-changed', (event) => emit(event.payload)),
      (payload) => setSettings(normalizeReceivedSettings(payload)),
      { onError: (error) => patchRuntime({ error: String(error) }) },
    ).dispose
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    let scheduledRefresh: number | undefined
    const refresh = async () => {
      try {
        const next = await nativeRuntime.desktopDisplays()
        if (disposed) return
        const signature = displayTopologySignature(next)
        if (signature === desktopDisplaysSignatureRef.current) return
        desktopDisplaysSignatureRef.current = signature
        setDesktopDisplays(next)
      } catch (error) {
        if (!disposed) patchRuntime({ error: `读取显示器布局失败：${String(error)}` })
      }
    }
    const delayedRefresh = () => {
      if (scheduledRefresh !== undefined) return
      scheduledRefresh = window.setTimeout(() => {
        scheduledRefresh = undefined
        void refresh()
      }, 120)
    }
    void refresh()
    const displayListener = listenUntilDisposed<unknown>(
      (emit) => listen('display-changed', () => emit(undefined)),
      delayedRefresh,
      { onError: (error) => patchRuntime({ error: `显示器事件订阅失败：${String(error)}` }) },
    )
    window.addEventListener('resize', delayedRefresh)
    const timer = window.setInterval(() => { void refresh() }, DISPLAY_TOPOLOGY_FALLBACK_INTERVAL_MS)
    return () => {
      disposed = true
      window.removeEventListener('resize', delayedRefresh)
      window.clearInterval(timer)
      if (scheduledRefresh !== undefined) window.clearTimeout(scheduledRefresh)
      displayListener.dispose()
    }
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative || runtime.harness !== 'bridge-ready') return
    void nativeRuntime.harnessPresets().then((presets) => {
      setPresetOptions(presets)
      setSelectedPreset((current) => current ?? presets.find((preset) => preset.isDefault)?.id ?? presets[0]?.id)
    }).catch(() => setPresetOptions([]))
  }, [runtime.harness])

  useEffect(() => {
    if (!nativeRuntime.isNative || runtime.backend !== 'harness' || runtime.harness !== 'bridge-ready') return
    void nativeRuntime.harnessControls().then(setHarnessControls).catch(() => setHarnessControls(undefined))
  }, [runtime.backend, runtime.harness, conversationGeneration])

  // The launcher returns before DSH has finished booting. Keep the switch in
  // its starting state until the Bridge is genuinely ready, and surface an
  // early child exit instead of letting the old eight-second timer make the
  // button look available again while nothing is listening on 3080.
  useEffect(() => {
    if (harnessStarting && !harnessLaunchPendingRef.current) {
      harnessLaunchPendingRef.current = true
      harnessLaunchStartedAtRef.current = Date.now()
    }
  }, [harnessStarting])

  useEffect(() => {
    if (!nativeRuntime.isNative || !harnessLaunchPendingRef.current) return
    if (runtime.harness === 'bridge-ready') {
      harnessLaunchPendingRef.current = false
      harnessLaunchStartedAtRef.current = undefined
      setHarnessFailed(false)
      if (harnessStarting) setHarnessStarting(false)
      return
    }
    if (!harnessStarting) setHarnessStarting(true)
    let disposed = false
    const check = async () => {
      try {
        const managed = await nativeRuntime.managedDshStatus()
        if (disposed || !harnessLaunchPendingRef.current) return
        const outcome = harnessLaunchOutcome(
          { availability: runtime.harness, reasonCode: runtime.harnessReasonCode },
          Date.now() - (harnessLaunchStartedAtRef.current ?? Date.now()),
          managed,
          45_000,
          harnessLaunchedKind,
        )
        if (!outcome) return
        harnessLaunchPendingRef.current = false
        harnessLaunchStartedAtRef.current = undefined
        setHarnessStarting(false)
        // 握手失败：灯转红（"试过、没成"），滑槽拨回左边。旧的那条连接**不动** —— 连接层是
        // make-before-break，没有成功的新会话就不会拆旧的；它只是不再冒充"已连接"。
        setHarnessFailed(true)
        changeBackendRef.current?.(nonHarnessBackendRef.current)
        patchRuntime({ error: outcome.message })
      } catch (error) {
        if (!disposed) patchRuntime({ error: String(error) })
      }
    }
    void check()
    const timer = window.setInterval(() => { void check() }, 1000)
    return () => { disposed = true; window.clearInterval(timer) }
  }, [harnessStarting, runtime.harness])

  /**
   * Start the configured DSH once, when `autoStartWithWallpaper` is on.
   *
   * Deliberately narrow:
   *  - the background wallpaper surface is the only initiator, so opening or
   *    saving the settings window cannot spawn a second resident DSH;
   *  - the request is made once per process and the native side refuses a
   *    repeat, so a remount, an unlock, or an HMR pass cannot add another;
   *  - it runs from an effect and resolves asynchronously, so the native first
   *    frame and the WorkerW attach are never blocked by a process spawn;
   *  - an external DSH already on 3080 is reported as `external` and left
   *    running: this feature never takes over or stops someone else's service.
   */
  useEffect(() => {
    if (!nativeRuntime.isNative) return
    if (!settings.dshLaunch.autoStartWithWallpaper) return
    // Module scope, so a React remount of this component does not re-request.
    if (dshAutostartRequestedRef.current || dshAutostartRequestedInProcess) return
    dshAutostartRequestedRef.current = true
    dshAutostartRequestedInProcess = true
    let disposed = false
    void (async () => {
      try {
        // §5.1: the automatic start starts the chosen *subject*, whatever class it
        // belongs to. Native decides which mechanism that means, so this passes the
        // stored id and reads a closed outcome code back.
        const subjectId = settings.dshLaunch.subjectId ?? settings.dshLaunch.rootPath
        const result = await nativeRuntime.autostartHarnessTarget({
          targetId: subjectId,
          profile: settings.dshLaunch.profile,
          command: settings.dshLaunch.command,
          trustedCommand: settings.dshLaunch.trustedCommandForAutoStart,
        })
        if (disposed) return
        if (result.outcome === 'started' || result.outcome === 'started-unconfirmed') {
          // Reuse the manual launch's readiness window, so the same 45-second
          // supervision, timeout message and exit detection apply. A shell is
          // supervised on the bridge probe alone: nothing here owns its process.
          harnessLaunchedKind = isEmbeddedShellSubject(subjectId)
            ? 'embedded-shell'
            : isInstalledCliSubject(subjectId)
              ? 'installed-cli'
              : 'checkout'
          harnessLaunchStartedAtRef.current = Date.now()
          harnessLaunchPendingRef.current = true
          setHarnessStarting(true)
          // A start that was never confirmed still deserves its own sentence: the
          // user must be able to tell "still starting" from "nothing happened".
          if (result.outcome === 'started-unconfirmed') {
            const notice = dshAutostartNotice(result)
            if (notice) patchRuntime({ error: notice })
          }
          return
        }
        if (result.outcome === 'port-occupied-external') {
          // Someone else's DSH already serves 3080. Probe it and stay quiet:
          // there is no failure here for the user to act on.
          const status = await nativeRuntime.probeHarness().catch(() => undefined)
          if (!disposed && status) patchRuntime({ harness: status.availability, harnessReasonCode: status.reasonCode })
          return
        }
        const notice = dshAutostartNotice(result)
        if (notice && !disposed) patchRuntime({ error: notice })
      } catch (error) {
        if (!disposed) patchRuntime({ error: `自动启动 DSH 失败：${String(error)}` })
      }
    })()
    return () => { disposed = true }
    // Keyed to the setting alone on purpose: editing rootPath/profile after a
    // launch must not spawn a second DSH in the same process. The corrected
    // configuration takes effect on the next wallpaper start.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.dshLaunch.autoStartWithWallpaper])

  /**
   * The expanded bottom inset depends on the taskbar and display topology, not
   * on the clock. Polling it once per second was a permanent resident cost for
   * a value that only changes on a display event, a resize, a conversation
   * display switch, or a workspace transition. Measure on those events, and
   * keep only a slow fallback so a taskbar change that emits nothing still
   * converges.
   */
  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let disposed = false
    let frame = 0
    const refresh = () => {
      if (disposed) return
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        if (disposed) return
        void nativeRuntime.desktopLayoutMetrics(conversationDisplayId)
          .then((metrics) => { if (!disposed) setExpandedBottomInset(metrics.expandedBottomInset) })
          .catch(() => undefined)
      })
    }
    refresh()
    window.addEventListener('resize', refresh)
    const displayListener = listenUntilDisposed<unknown>(
      (emit) => listen('display-changed', () => emit(undefined)),
      refresh,
      { onError: () => undefined },
    )
    const timer = window.setInterval(refresh, LAYOUT_METRICS_FALLBACK_INTERVAL_MS)
    return () => {
      disposed = true
      cancelAnimationFrame(frame)
      window.removeEventListener('resize', refresh)
      window.clearInterval(timer)
      displayListener.dispose()
    }
  }, [conversationDisplayId, interactionState, runtime.historyExpanded, workspace])

  useEffect(() => {
    const adapterBackend = runtime.backend
    const conversationPolicy = conversationPolicyRef.current
    const webResumeId = adapterBackend === 'deepseek-web'
      ? resumeConversationId('deepseek-web', conversationPolicy)
      : undefined
    const adapter: ChatAdapter = nativeRuntime.isNative
      ? adapterBackend === 'deepseek-web'
        ? new DeepSeekWebAdapter(
            webResumeId,
            shouldStartNewWebConversation(conversationPolicy, webResumeId),
          )
        : new NativeChatAdapter(
            adapterBackend,
            adapterBackend === 'deepseek-api'
              ? apiAdapterOptionsRef.current
              : adapterBackend === 'harness'
                ? harnessAdapterOptionsFromSettings(settings, harnessModelChoice)
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
    adapterRef.current.disconnect()
    adapterRef.current = adapter
    chatActivityRef.current = { adapter, backend: adapterBackend, activity: 'idle' }
    setMessages([]); setStreamingText(''); setUsage(undefined)
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
      // Guaranteed by the ref initializer above; the local binding keeps the
      // narrowing visible to TypeScript inside this closure.
      const streamBuffer = streamBufferRef.current
      if (!streamBuffer) return
      if (event.type === 'status') {
        if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = event.activity
        patchRuntime({ activity: event.activity }); dispatchCore('set-activity', { value: event.activity }); if (event.activity === 'idle' || event.activity === 'done') setQuestionPrompt(undefined)
      }
      if (event.type === 'delta') {
        if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = 'streaming'
        // A streaming turn publishes one delta per model chunk. Committing a
        // React state update for every one of them is what made a long answer
        // re-render hundreds of times, so deltas are coalesced into a single
        // commit per animation frame and the order is preserved exactly.
        patchRuntime({ activity: 'streaming' }); dispatchCore('set-activity', { value: 'streaming' }); streamBuffer.append(event.text)
      }
      if (event.type === 'message') {
        setMessages((items) => [...items, { id: crypto.randomUUID(), role: event.role, content: event.content, createdAt: Date.now(), usage: event.usage }])
        // Harness and future providers may attach final usage directly to the
        // final message instead of publishing a separate usage event.
        if (event.usage) setUsage(event.usage)
        if (event.role === 'assistant') {
          // Any delta still queued for the next frame belongs to the answer
          // that just landed, so it is dropped rather than appended twice.
          streamBuffer.reset()
          setStreamingText('')
          setQuestionPrompt(undefined)
        }
      }
      if (event.type === 'usage') setUsage(event)
      if (event.type === 'model') {
        // 形态只由**知道的**层级改写（原生侧带的是 `unknown`），见 `tierPatchFromEvent`。
        patchRuntime({
          model: event.model,
          provider: event.provider,
          reasoningEffort: event.effort,
          ...tierPatchFromEvent(event.tier),
        })
      }
      if (event.type === 'auth-required') {
        if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = 'idle'
        baseDispatch({ type: 'AUTH_REQUIRED' }); dispatchCore('set-activity', { value: 'idle' }); dispatchCore('auth-required')
      }
      if (event.type === 'approval-required') { patchRuntime({ activity: 'tool', error: `${event.summary}；请打开 Harness 处理。` }); dispatchCore('set-activity', { value: 'tool' }) }
      if (event.type === 'question-required') { setQuestionPrompt(event.questions); patchRuntime({ activity: 'tool', error: undefined }); dispatchCore('set-activity', { value: 'tool' }) }
      if (event.type === 'conversation-reset') {
        // 这条会话不能用了（用户归档，或宿主拒绝了这一轮）：轨道上这段记录**立刻**停止看起来
        // 像活的（症状正是"输入被吞了、灯还是绿的"——转写看着正常，其实没有人在听）。
        // 同步清空而不是等新会话的历史对账回来：那个请求可能回空，而且它赢不了"重发那句话已经
        // 在路上"这件事；新会话的内容由随后的事件与历史对账填回来。通知走 `chatNotice` 而不是
        // `error`：原生快照会整体覆写 `error`，挤在一起就只能"闪一下"。
        setMessages([])
        setStreamingText('')
        setUsage(undefined)
        setQuestionPrompt(undefined)
        if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = 'sending'
        patchRuntime({ activity: 'sending', chatNotice: event.message })
        dispatchCore('set-activity', { value: 'sending' })
      }
      if (event.type === 'error') {
        if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = 'idle'
        // 同样是**聊天层**的话（"DSH 拒绝了这一轮…"、"发送失败…"），走 `chatNotice`：
        // 它在屏幕上留得住，而原生快照碰不到它。
        setStreamingText(''); patchRuntime({ activity: 'idle', chatNotice: event.message }); dispatchCore('set-activity', { value: 'idle' })
      }
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
        if (isCurrent()) {
          if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = 'idle'
          patchRuntime({ activity: 'idle', error: String(error) })
          dispatchCore('set-activity', { value: 'idle' })
        }
      }
    })()
    return () => {
      // `send()` creates an API session UUID synchronously, before its native
      // Promise resolves. Persist it before disconnecting so a backend switch,
      // unlock reset, or React effect teardown cannot orphan that transcript.
      disposed = true
      if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current = undefined
      disposeChatAdapter(adapter, adapterBackend, unsubscribe)
    }
  }, [
    adapterLifecycleKey,
    harnessModelChoice,
  ])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    let observedLockOrSuspend = false
    const listener = listenUntilDisposed<'locked' | 'unlocked' | 'suspend' | 'resume'>(
      (emit) => nativeRuntime.listenSystem(emit),
      (event) => {
        // The native host emits one synthetic `resume` on registration so a
        // current desktop can initialize its visual state. Do not mistake that
        // boot-time signal for an unlock policy boundary.
        if (shouldIgnoreUnpairedResume(observedLockOrSuspend, event)) {
          return
        }
        if (event === 'locked' || event === 'suspend') observedLockOrSuspend = true
        if (!appCoreClient.native && (event === 'locked' || event === 'suspend')) baseDispatch({ type: 'LOCK' })
        if (event === 'unlocked' || event === 'resume') {
          if (settingsRef.current.interactionLayout === 'taskbar-docked') setInteractionState('collapsed')
          const now = new Date()
          const policy = conversationPolicyRef.current
          const boundaryHour = settingsRef.current.dayBoundaryHour
          if (shouldStartNewConversationOnUnlock(policy, previousUnlockDayRef.current, now, boundaryHour)) {
            setConversationGeneration((value) => value + 1)
          }
          previousUnlockDayRef.current = assistantDay(now, boundaryHour)
          observedLockOrSuspend = false
          if (!appCoreClient.native) baseDispatch({ type: 'UNLOCK', playWake: settingsRef.current.playWakeOnEveryUnlock && settingsRef.current.animationsEnabled && !settingsRef.current.skipWakeAnimation })
        }
      },
      { onError: (error) => patchRuntime({ error: `系统会话事件订阅失败：${String(error)}` }) },
    )
    return () => listener.dispose()
    // Subscription, not a reaction to a preference: the values it reads are taken from
    // `settingsRef` when an unlock actually arrives. Re-subscribing on every preference
    // change only made the listener churn.
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    const listener = listenUntilDisposed<{ type: 'backend'; backend: BackendMode }>(
      (emit) => nativeRuntime.listenTray(emit),
      (event) => changeBackend(event.backend),
      { onError: (error) => patchRuntime({ error: String(error) }) },
    )
    return () => listener.dispose()
  }, [])

  useEffect(() => {
    if (!nativeRuntime.isNative) return
    const listener = listenUntilDisposed<'enter' | 'leave'>(
      async (emit) => {
        const { listen: listenEvent } = await import('@tauri-apps/api/event')
        return listenEvent<'enter' | 'leave'>('desktop-workspace-toggle', (event) => emit(event.payload))
      },
      (payload) => {
        if (payload === 'enter') enterInnerWorkspace()
        else leaveInnerWorkspace()
      },
      { onError: (error) => patchRuntime({ error: String(error) }) },
    )
    return () => listener.dispose()
    // One subscription for the surface's lifetime: the handlers read the layout they
    // need from `settingsRef`, so a toggle preference no longer tears the listener down
    // and rebuilds it. Keying a subscription on a preference is the same mistake that
    // re-issued the boot event.
  }, [])

  useEffect(() => {
    if (!nativeAppearance.isNative) return
    void refreshAppearance()
  }, [surface])

  useEffect(() => {
    if (!nativeAppearance.isNative) return
    const listener = listenUntilDisposed<unknown>(
      (emit) => listen('appearance-changed', () => emit(undefined)),
      () => { void refreshAppearance() },
      { onError: (error) => patchRuntime({ error: `外观变更订阅失败：${String(error)}` }) },
    )
    return () => listener.dispose()
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
    // Browser preview has no native probe cache, so it discovers the endpoint
    // itself. Same scope, same strict identification and the same "no substitution"
    // rule as the native scan, so the two cannot disagree about which client is in
    // use.
    const monitor = monitorHarnessEndpoint({
      // Read through the live state rather than a captured value, so changing the
      // subject takes effect on the next poll instead of requiring a reload.
      scope: () => ({
        subjectId: settings.dshLaunch.subjectId ?? settings.dshLaunch.rootPath,
        endpointPort: settings.dshLaunch.endpointPort,
        extraPorts: settings.dshLaunch.extraEndpointPorts,
      }),
      // Only consulted while nothing is configured: a configured subject's own
      // ports already include the ones the user added for it.
      extraPorts: () => settings.dshLaunch.extraEndpointPorts ?? [],
      onChange: ({ status }) => {
        patchRuntime({ harness: status.availability, model: status.model ?? runtimeRef.current.model, provider: status.provider ?? runtimeRef.current.provider, reasoningEffort: status.reasoningEffort })
        if (isHarnessReady(status.availability) && runtimeRef.current.backend !== 'harness') {
          if (canAutoSelectHarness(status.availability, runtimeRef.current.backend, settings.autoSwitchHarness)
            || shouldReturnToHarness(status.availability, runtimeRef.current.backend, harnessResetClaimed(autoResetFromHarnessRef.current, settingsRef.current.harnessAutoResetAt))) {
            markAutoResetFromHarness(false)
            changeBackend('harness', { automatic: true })
          }
        }
        // 黄灯：就绪过的桥接正在失联但还没判死（`probing`）。此时**不改** availability，
        // 所以滑槽、模型列表、会话都还按"它还在"处理 ✓——只有灯变色 ✓。
        setHarnessProbing(status.probing === true)
        const disconnected = harnessAvailabilityPatch(runtimeRef.current.backend, status.availability, runtimeRef.current.error)
        if (disconnected) patchRuntime(disconnected)
      },
    })
    return () => monitor.stop()
  }, [settings.autoSwitchHarness])

  useEffect(() => {
    if (!appCoreClient.native) return
    if (isHarnessReady(runtime.harness) && runtime.backend !== 'harness') {
      if (canAutoSelectHarness(runtime.harness, runtime.backend, settings.autoSwitchHarness)
        || shouldReturnToHarness(runtime.harness, runtime.backend, harnessResetClaimed(autoResetFromHarnessRef.current, settings.harnessAutoResetAt))) {
        markAutoResetFromHarness(false)
        changeBackend('harness', { automatic: true })
      }
    }
    // 主体退出后复位到左侧：拉起 harness 的入口就在壁纸里，停在死掉的一侧会让用户
    // 不得不再手动切一次 ✗。记录不会丢——会话按日期命名、转写留在宿主那边，切回去自动接上。
    // 复位时**保留轨道里的转写**：用户看到的是"上次的 Harness 会话"，而不是一片空白；
    // `autoResetFromHarnessRef` 与设置里的时间戳同时记下"这次是壁纸复位的"，主体回来后由它自己
    // 拨回去——包括壁纸重启之后（升级安装必然重启）。
    const fallback = harnessFallbackBackend(runtime.harness, runtime.backend, settings.defaultBackend)
    if (fallback) {
      markAutoResetFromHarness(true)
      changeBackend(fallback, { keepTranscript: true, automatic: true })
    }
    const disconnected = harnessAvailabilityPatch(runtime.backend, runtime.harness, runtime.error)
    if (disconnected) patchRuntime(disconnected)
  }, [runtime.harness, runtime.backend, runtime.error, settings.autoSwitchHarness, settings.defaultBackend])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === 'w') { baseDispatch({ type: 'LOCK' }); dispatchCore('lock') }
      if (event.key === 'Escape') {
        if (runtime.phase === 'locked') { baseDispatch({ type: 'UNLOCK', playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }); dispatchCore('unlock', { playWake: settings.playWakeOnEveryUnlock && settings.animationsEnabled && !settings.skipWakeAnimation }) }
      }
    }
    window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey)
  }, [runtime.phase, settings])

  /**
   * Switch the backend, and decide what happens to what is on screen.
   *
   * `keepTranscript` is for the one transition the user did not ask for: when the
   * subject exits and the wallpaper resets the switch, wiping the rail would take the
   * record away at the exact moment it is the only thing left of the session. The
   * transcript is not lost either way — the host keeps the day-named session — but
   * "it is still there somewhere" is not the same as still being on screen.
   *
   * `automatic` marks a move the wallpaper made by itself, which is what entitles it
   * to move back when the subject returns; a manual choice clears that claim.
   */
  const changeBackend = (
    backend: WallpaperSettings['defaultBackend'],
    options?: { keepTranscript?: boolean; automatic?: boolean },
  ) => {
    if (!canSelectBackend(runtimeRef.current.harness, backend)) {
      // Do not construct a native Harness adapter or issue a Tauri selection
      // for a bare port-3080 observation. Leave the current transcript and
      // backend intact until the Bridge contract is actually ready.
      patchRuntime({ activity: 'idle', error: harnessSelectionUnavailableError(runtimeRef.current.harness) })
      return
    }
    if (!options?.automatic) markAutoResetFromHarness(false)
    // Clear backend-scoped UI immediately. The effect below repeats this while
    // creating the next adapter, which prevents one paint of API usage or a
    // partial answer under the newly selected backend label.
    if (!options?.keepTranscript) setMessages([])
    setStreamingText('')
    setUsage(undefined)
    activeBackendRef.current = backend
    patchRuntime({ backend })
    baseDispatch({ type: 'RECOVER' })
    if (appCoreClient.native) void appCoreClient.selectBackend(backend).catch((error) => patchRuntime({ error: String(error) }))
  }
  const scene = useMemo(() => {
    if (runtime.phase === 'booting' || runtime.phase === 'locked') return <SleepScene persona={persona} mode="system" />
    if (runtime.phase === 'waking') {
      const wakeProps = {
        persona,
        startIndex: 1,
        handoffGeneration: nativeHandoffGeneration,
        enabled: settings.animationsEnabled && !settings.skipWakeAnimation,
        speed: settings.animationSpeed,
        onFirstWakeFrame: (generation: number) => reportNativeBootstrapReady(generation, nativeRuntime, { verifySceneImages: false }),
        onWakeDone: () => { baseDispatch({ type: 'WAKE_DONE' }); dispatchCore('wake-done') },
      }
      return multiScreenActive
        ? <MultiScreenWakeScene displays={desktopDisplays} {...wakeProps} />
        : <WakeScene {...wakeProps} />
    }
    const question = questionPrompt?.[0]
    const questionOptions = question?.options?.map((option) => option.label).join(' / ')
    const bubbleText = question
      ? `想听听你的意见：${question.question}${questionOptions ? `（${questionOptions}）` : ''}`
      : runtime.activity === 'thinking' ? '正在认真思考…' : bubbles.morning
    const scenePersona = { ...persona, bubbles, assets: { ...persona.assets, portrait: resolvedPersona ?? persona.assets.portrait } }
    if (multiScreenActive) {
      return <MultiScreenIdleScene displays={desktopDisplays} backgroundUrls={screenBackgroundUrls} portraitDisplayId={portraitDisplayId} persona={scenePersona} bubbleText={workspace === 'front' ? bubbleText : ''} portraitAmbientLength={settings.portraitAmbientLength} portraitAmbientStrength={settings.portraitAmbientStrength} onOpenChat={enterInnerWorkspace} />
    }
    return <IdleScene persona={scenePersona} bubbleText={bubbleText} backgroundUrl={resolvedBackground ?? (background?.path ? assetUrl(background.path) : undefined)} portraitAmbientLength={settings.portraitAmbientLength} portraitAmbientStrength={settings.portraitAmbientStrength} hideBubble={workspace !== 'front'} onOpenChat={enterInnerWorkspace} />
  }, [background?.path, bubbles, desktopDisplays, multiScreenActive, persona, portraitDisplayId, questionPrompt, resolvedBackground, resolvedPersona, runtime, screenBackgroundUrls, settings, workspace])

  const conversationBubble = interactionEnabled
    && runtime.phase !== 'booting'
    && runtime.phase !== 'locked'
    && (settings.interactionLayout === 'taskbar-docked' || workspace !== 'front')
    ? (() => {
      // 失败复位要用的两样东西取"当下这一份"（见上面 ref 的说明）。这里赋值而不是在 effect 里：
      // 切换后端与主体都发生在用户动作之后，任何一次渲染都比上一次更新。
      changeBackendRef.current = changeBackend as typeof changeBackendRef.current
      const fallback = (settings.defaultBackend === 'harness' ? 'deepseek-web' : settings.defaultBackend) as 'deepseek-web' | 'deepseek-api'
      nonHarnessBackendRef.current = fallback
      return <ConversationBubble
      backend={runtime.backend}
      activity={runtime.activity}
      modelLabel={modelLabel}
      messages={messages}
      streamingText={streamingText}
      historyExpanded={workspace === 'front' ? runtime.historyExpanded : innerHistoryExpanded}
      // Rebuilt when the desktop regains the foreground: only a rebuild
      // restores the WebView keyboard channel, and the draft is kept in App
      // so the rebuild does not discard a half-typed message.
      initialDraft={chatDraft}
      onDraftChange={setChatDraft}
      usage={usage}
      collapsed={settings.interactionLayout === 'taskbar-docked' && interactionState === 'collapsed'}
      layout={settings.interactionLayout}
      expandDirection={interactionDirection}
      persistent={settings.interactionLayout === 'floating'}
      acrylicOpacity={settings.conversationOpacity}
      acrylicBlur={settings.conversationBlur}
      expandedBottomInset={expandedBottomInset}
      apiPricingConfigured={settings.deepseekApi.priceInputPerMillion !== undefined && settings.deepseekApi.priceOutputPerMillion !== undefined}
      harnessAvailability={runtime.harness}
      harnessStarting={harnessStarting}
      harnessFailed={harnessFailed}
      harnessSuspect={harnessTransitioning || harnessBuffering}
      onStartHarness={async () => {
        if (harnessLaunchPendingRef.current) return
        // 红灯时点滑槽＝**动手再试一次**：拉起对应 harness 进程（这一步本身就是"握手申请"的前半，
        // 后半由切到 harness 后适配器连接时发出）。所以先清掉失败标记，灯回到"连接中"。
        setHarnessFailed(false)
        setHarnessStarting(true)
        harnessLaunchPendingRef.current = true
        harnessLaunchStartedAtRef.current = Date.now()
        try {
          // §5.3: the route switch does what the automatic start does — start the
          // chosen execution subject — and differs only in switching the backend
          // afterwards, which is the caller's job.
          const subjectId = settings.dshLaunch.subjectId ?? settings.dshLaunch.rootPath
          if (!subjectId) {
            harnessLaunchPendingRef.current = false
            harnessLaunchStartedAtRef.current = undefined
            setHarnessStarting(false)
            await nativeRuntime.openSettingsWindow()
            return
          }
          harnessLaunchedKind = isEmbeddedShellSubject(subjectId)
            ? 'embedded-shell'
            : isInstalledCliSubject(subjectId)
              ? 'installed-cli'
              : 'checkout'
          await nativeRuntime.launchHarnessTarget({
            targetId: subjectId,
            profile: settings.dshLaunch.profile,
            command: settings.dshLaunch.command,
          })
        } catch (error) {
          harnessLaunchPendingRef.current = false
          harnessLaunchStartedAtRef.current = undefined
          setHarnessStarting(false)
          patchRuntime({ error: String(error) })
        }
      }}
      onConfigureHarness={() => { void nativeRuntime.openSettingsWindow().catch((error) => patchRuntime({ error: String(error) })) }}
      onSelectBackend={changeBackend}
      // 离开 Harness 要回到**用户在设置里选的那个聊天后端**，而不是写死的网页桥：
      // 设置里选了 API 的人，用这个开关去 Harness 再切回来时必须回到 API（用户实测报过）。
      // 设置本身也可能就是 Harness（那时按"聊天后端"取网页），所以这里兜一层。
      nonHarnessBackend={settings.defaultBackend === 'harness' ? 'deepseek-web' : settings.defaultBackend}
      // 保留下来的是**上一个后端**的转写（壁纸因主体退出自己复位时才发生），所以它只在
      // 已经不在 Harness 上、而且确实有记录可看时才标注来历。
      keptTranscript={harnessResetClaimed(autoResetFromHarness, settings.harnessAutoResetAt) && runtime.backend !== 'harness' && messages.length > 0}
      presetOptions={presetOptions}
      selectedPreset={selectedPreset}
      onSelectPreset={messages.length === 0 ? (preset) => { void nativeRuntime.setHarnessPreset(preset).then(() => setSelectedPreset(preset)).catch((error) => patchRuntime({ error: String(error) })) } : undefined}
      permission={runtime.backend === 'harness' ? harnessControls?.permission : undefined}
      commands={runtime.backend === 'harness' ? harnessControls?.commands : undefined}
      onSelectPermission={(permission) => { void nativeRuntime.setHarnessPermission(permission).then(() => setHarnessControls((value) => value ? { ...value, permission: { ...value.permission, current: permission } } : value)).catch((error) => patchRuntime({ error: String(error) })) }}
      modelOptions={modelOptions}
      selectedModel={selectedModel}
      modelLabels={modelLabels}
      modelSwitchDisabledReason={modelSwitchDisabledReason}
      harnessReady={isHarnessReady(runtime.harness)}
      // 黄灯（`harnessSuspect`）在面板顶部那处统一给：原生的 `harnessProbing` + 刚才那段最短停留。
      // 每个后端都接上：它是"拉起当前主体可视化窗口"的快捷键，与岛上当前是哪个后端无关
      // （用户要求的是"每个样式"都有这个按钮）。没有主体可拉时，原生会退到"这个端点上
      // 应答的那台"，所以网页模式下点击也不会落空。
      onRaiseClientWindow={openSubjectInterface}
      onSelectModel={runtime.backend === 'deepseek-web' ? undefined : (model) => {
        if (runtime.backend === 'deepseek-api') {
          setApiModelChoice(model)
          apiAdapterOptionsRef.current.model = model
          patchRuntime({ model })
        } else {
          setHarnessModelChoice(model)
          // 持久化：下次启动要**回显这次选的模型**，而不是宿主报的当前值。
          // 写进设置后其它 WebView（设置中心等）会通过 `settings-changed` 收到同一份值。
          saveSettings({ ...settings, harnessModel: { model } })
          // 同步给宿主：宿主把默认模型改成这个（写进它自己的设置，跨宿主重启生效），
          // 于是从 DSH 界面开的新会话也用同一个模型。失败不打扰用户——壁纸这次会话已经用
          // 上了选定模型，这只是"没同步成"，原生日志里有原因。
          void nativeRuntime.harnessSetModel(model).catch(() => undefined)
          setConversationGeneration((value) => value + 1)
          patchRuntime({ model, provider: 'deepseek-official', activity: 'idle' })
        }
      }}
      onExpand={() => {
        setInteractionState('expanded')
        if (workspace === 'front') {
          enterInnerWorkspace()
        } else {
          baseDispatch({ type: 'OPEN_CHAT' })
          dispatchCore('open-chat')
        }
      }}
      disabled={runtime.backend === 'harness' && runtime.harness !== 'bridge-ready'}
      sendShortcut={settings.sendShortcut}
      onToggleHistory={() => {
        if (workspace !== 'front') {
          setInnerHistoryExpanded((value) => !value)
        } else {
          baseDispatch({ type: 'TOGGLE_HISTORY' })
          dispatchCore('toggle-history')
        }
      }}
      onSend={(text) => {
        const adapter = adapterRef.current
        const adapterBackend = adapter.mode
        const isCurrent = () => isCurrentChatOperation(adapterRef.current, activeBackendRef.current, adapter, adapterBackend, false)
        if (!isCurrent()) return
        setUsage(undefined)
        setStreamingText('')
        if (chatActivityRef.current?.adapter === adapter && chatActivityRef.current.backend === adapterBackend) chatActivityRef.current.activity = 'sending'
        // 一次新的发送清掉上一条聊天层通知（宿主的 `error` 由原生快照自己维护，不动它）。
        patchRuntime({ activity: 'sending', error: undefined, chatNotice: undefined })
        dispatchCore('set-activity', { value: 'sending' })
        const sending = adapter.send(text)
        persistConversationPointerWhenAvailable(adapter, adapterBackend)
        void sending.then(() => {
          if (!isCurrent()) return
          persistConversationPointerWhenAvailable(adapter, adapterBackend)
        }).catch((error) => {
          if (isCurrent()) {
            if (chatActivityRef.current?.adapter === adapter) chatActivityRef.current.activity = 'idle'
            patchRuntime({ activity: 'idle', error: String(error) })
            dispatchCore('set-activity', { value: 'idle' })
          }
        })
      }}
      onStop={() => {
        const adapter = adapterRef.current
        const adapterBackend = adapter.mode
        const isCurrent = () => isCurrentChatOperation(adapterRef.current, activeBackendRef.current, adapter, adapterBackend, false)
        if (!isCurrent()) return
        if (chatActivityRef.current?.adapter === adapter && chatActivityRef.current.backend === adapterBackend) chatActivityRef.current.activity = 'idle'
        patchRuntime({ activity: 'idle' })
        dispatchCore('set-activity', { value: 'idle' })
        void adapter.stop().catch((error) => {
          if (isCurrent()) patchRuntime({ activity: 'idle', error: String(error) })
        })
      }}
      // 「X」= 离开里桌面。**交给原生**（`leave_inner_workspace`），不在这里自己搬界面：
      // "现在在不在里桌面"是原生的事实（图标层、悬浮球的判据都看它），前端自己搬会让那个事实
      // 原地不动——实测后果是点完 X 之后悬浮球再也弹不出来、点球也唤不起输入岛（原生以为还
      // 在里桌面，`enter` 直接被幂等短路，事件根本不发）。真正的界面迁移由原生的
      // `desktop-workspace-toggle: leave` 事件驱动，与桌面空白双击**同一条路**。
      //
      // 表桌面上没有"离开"可言（原生那边本来就不在里桌面，也不会发事件），那时这个按钮就是
      // "把展开的岛收回胶囊"，由前端自己收。
      onClose={() => {
        if (workspace === 'front') {
          leaveInnerWorkspace()
          return
        }
        void nativeRuntime.leaveInnerWorkspace().catch((error) => patchRuntime({ error: `离开里桌面失败：${String(error)}` }))
      }}
    />
    })()
    : null

  const conversationDisplay = desktopDisplays.find((display) => display.id === conversationDisplayId) ?? desktopDisplays[0]
  const conversationUiScale = multiScreenActive && typeof window !== 'undefined' ? displayUiScale(window.devicePixelRatio || 1) : 1
  const conversationTransformOrigin = settings.interactionLayout === 'taskbar-docked'
    ? interactionState === 'collapsed'
      ? 'calc(100% - clamp(22px, 5vh, 58px))'
      : 'calc(100% - clamp(10px, 2.7vh, 34px))'
    : (workspace === 'front' ? runtime.historyExpanded : innerHistoryExpanded)
      ? `calc(100% - ${expandedBottomInset}px)`
      : 'calc(100% - clamp(22vh, 29vh, 34vh))'
  const conversationSurface = multiScreenActive && conversationDisplayId && conversationBubble && conversationDisplay
    ? <div
      className={`display-interaction-layer ${settings.interactionLayout === 'taskbar-docked' && interactionState === 'collapsed' ? 'is-collapsed' : ''}`}
      style={{
        ...displayCssRect(conversationDisplay, displayVirtualBounds),
        // Keep this in viewport-relative CSS units. A single WorkerW can span
        // monitors with different DPI scales, so converting the target screen
        // with its own scale factor would mix physical and logical spaces.
        ['--dsh-display-height' as string]: `${conversationDisplay.bounds.height / Math.max(1, displayVirtualBounds.height) * 100}vh`,
      }}
    >
      <div
        className="display-interaction-content"
        // Keep the horizontal anchor at the selected display's center. A
        // single-value transform-origin is interpreted as the X origin by
        // CSS, which previously nudged the island sideways after scaling.
        style={{ transform: `scale(${conversationUiScale})`, transformOrigin: `50% ${conversationTransformOrigin}` }}
      >
        {conversationBubble}
      </div>
    </div>
    : conversationBubble

  return <div className={`wallpaper-root surface-${surface} effort-${runtime.reasoningEffort ?? 'normal'} workspace-${workspace}`} data-workspace={workspace}>
    {scene}
    <>
      <WidgetHost workspace={workspace} widgets={[]} />
      {conversationSurface}
      {visibleNotice(runtime) && runtime.phase !== 'error' && <div className="runtime-notice" role="status">{visibleNotice(runtime)}<button onClick={() => patchRuntime({ error: undefined, chatNotice: undefined })}>×</button></div>}
      {runtime.phase === 'auth-required' && <div className="auth-overlay" data-interaction-region="auth"><div className="auth-card"><h2>需要登录 DeepSeek 网页入口</h2><p>应用内官方页面已经打开，请在其中完成登录。登录状态只保存在独立 WebView2 配置目录，本应用不会读取或复制 Cookie；登录完成后回到桌面即可继续发送。</p><button onClick={() => { baseDispatch({ type: 'AUTH_READY' }); dispatchCore('auth-ready') }}>我已完成登录</button></div></div>}
    </>
  </div>
}
