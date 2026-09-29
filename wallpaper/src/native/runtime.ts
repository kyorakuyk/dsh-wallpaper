import type { BackendMode, ChatMessage, ScopedChatEvent } from '../domain/types.ts'
import type { HarnessStatus } from '../connect/harness.ts'
import { parseLaunchArgs } from '../connect/launchArgs.ts'

export interface NativeSendOptions {
  conversationId?: string
  requestId?: string
  newConversation?: boolean
  baseUrl?: string
  model?: string
  /**
   * Harness 专用：新建会话时装载哪个 agent 预设（`minimal` / `standard` / …）。
   *
   * 壁纸默认给 `minimal`（用户要求："工作区的预设先默认为'极简模式'试试，应该能省不少上下文"）
   * ——预设决定这条会话装载多少指令与工具，而壁纸是每轮都要重建上下文的常驻场景，装得少就是省。
   */
  preset?: string
  /** CNY per million input tokens. Omitted means pricing is not configured. */
  priceInputPerMillion?: number
  /** CNY per million output tokens. Omitted means pricing is not configured. */
  priceOutputPerMillion?: number
}

export interface TranslucentTbStatus { installed: boolean; running: boolean; source?: string }
export interface LockScreenDiagnostics { supported: boolean; packageIdentity: boolean; takeoverAvailable: boolean; originalImageUri?: string; backupExists: boolean; backupValid: boolean; staleBackup: boolean; managedImageReady: boolean; managedImageActive: boolean; developmentBuild: boolean; warnings: string[] }
/**
 * 本应用启动的**一个** DSH 实例。
 *
 * `instanceKey` 是它的地址（主体 id + 「启动参数」），也是停止时要指名的那一个：同一个主体可以
 * 在 3080 与 3081 上各起一个，只有键能分清"停哪一个"。
 */
export interface ManagedDshInstance {
  instanceKey: string
  subjectId: string
  /** 它在哪个端口服务。读不到时为 undefined —— 界面写「端口未确认」，不写 0。 */
  port?: number
  pid: number
  /** 只有本进程启动的实例知道这两项；落盘记录（壁纸重启过）里没有。 */
  rootPath?: string
  profile?: string
  /** 启动它时用的「启动参数」。 */
  args: string[]
}
/**
 * 本应用启动的全部实例，外加两个给启动监督用的汇总字段。
 *
 * `instances` 是权威答案；`managed` / `running` 留着，因为启动监督问的是另一个问题
 * ——"我这次启动的那个孩子还在不在"。传了 `subjectId` 时它们只看那个主体的实例。
 */
export interface ManagedDshStatus {
  instances: ManagedDshInstance[]
  managed: boolean
  running: boolean
}
/**
 * 凭据管理器愿意交代的全部内容：有没有 Key，以及脱敏形态（`sk-••••••••abcd`）。
 *
 * 没有明文——原生侧**不存在**把 Key 交出来的命令；脱敏也是那边算好的（`mask_api_key`），
 * 渲染端连"取中间几位自己拼"的机会都没有。
 */
export interface ApiKeyStatus {
  present: boolean
  masked?: string
}
/**
 * 桌面会话工作区落在哪儿（只读自检）。
 *
 * 位置规则与桥一致：**壁纸自己的数据目录**（`%LOCALAPPDATA%\com.dsh.wallpaper`）下的「桌面会话」。
 * 安装目录不能用——MSIX 每次升级会整体替换它，写在那里的东西必丢。
 */
export interface DesktopWorkspaceStatus {
  dataDirectory: string
  workspaceDirectory: string
  workspaceExists: boolean
  /** 助手维护的「项目记忆」（说话人格等长期要求落在这里）；桌面会话里不贴路径，设置里给入口。 */
  memoryFile: string
  memoryExists: boolean
  /** 「清除全部用户数据」时要一并删掉的那条凭据（在凭据管理器里，不在文件系统上）。 */
  credentialTarget: string
}
/**
 * Outcome of the one automatic DSH start this process is allowed to attempt.
 * `outcome` is a closed, non-sensitive code; `external` means the instance's port
 * was already owned by someone else's DSH and was deliberately left alone.
 *
 * `command-not-confirmed` 已经不在这张表里了：它存在的前提（用户填一个自定义启动命令、自动
 * 启动要不要执行它）随那个设置一起消失。现在自动启动与手动启动跑的是同一个启动器。
 */
export interface ManagedDshAutostart {
  outcome: 'started' | 'started-unconfirmed' | 'already-attempted' | 'already-running' | 'root-path-missing'
    | 'root-path-invalid' | 'launcher-missing' | 'profile-invalid' | 'port-occupied-external'
    | 'launch-args-invalid' | 'unknown-target' | 'spawn-failed'
  pid?: number
  external: boolean
}
export interface AutostartStatus {
  enabled: boolean
  source: 'startup-task' | 'run' | 'none' | 'disabled-by-user' | 'disabled-by-policy' | 'unsupported'
  /**
   * Why the state is what it is, from the side that knows (Windows refused the
   * startup task, the recorded entry names another build, the registry could
   * not be read). `null` when nothing needs explaining — for example when a
   * packaged build's startup task is simply on.
   */
  reason: string | null
}
/**
 * The state a browser preview reports: there is no Windows process to ask, so
 * the page must say that instead of showing an unchecked switch as "off".
 */
export function unsupportedAutostart(): AutostartStatus {
  return { enabled: false, source: 'unsupported', reason: '当前系统不支持本应用的开机自启。' }
}
/**
 * One endpoint from the native scan. `kind` is the expected client shape for a
 * known port; `bridgeFound` is true only when a wallpaper Bridge answered, so a
 * port hosting some other HTTP service is not offered as an endpoint.
 */
export interface HarnessEndpointScan {
  port: number
  kind: 'official-desktop' | 'official-web'
  source: 'default' | 'user'
  bridgeFound: boolean
  status: HarnessStatus
}

/**
 * What the settings say the wallpaper may talk to.
 *
 * `subjectId` is the user's real choice (`shell:<aumid>` for a client that carries
 * its own checkout, else a source tree's path); `port` is their explicit endpoint
 * override; `extraPorts` are the ports they added for a checkout that does not
 * listen on the default. Native derives the admissible ports from the subject alone
 * — see `subject_endpoint_ports` — so this is a statement about the subject, never
 * about which client happens to answer.
 */
export interface HarnessEndpointScope {
  port?: number | null
  subjectId?: string
  extraPorts?: readonly number[]
  /**
   * 「启动参数」的原文。
   *
   * 原生用它读出这个主体被要求在哪个端口上服务（`--port 3081`），于是**探针**与「打开界面」
   * 盯着同一个端口。少了它，并行实例的第二个会永远显示成离线 —— 桥明明在隔壁一个端口上应答。
   * 这里传原文而不是解析后的端口，是因为分词与"怎么读 `--port`"的规则只有一份（`launchArgs.ts`），
   * 原生只做它自己那一份形状检查。
   */
  args?: string
}

/** What native made of that scope, for logging and for the settings card. */
export interface HarnessEndpointScopeResult {
  port?: number | null
  subjectId?: string
  /** The ports the subject may be reached on; empty when nothing is configured. */
  subjectPorts: number[]
}

/**
 * What the shim did when asked to start a chosen execution subject.
 *
 * `started-unconfirmed` is not a failure: the Windows shell accepted the request
 * but nothing answered before the launch timeout, which a slow first start of an
 * Electron client can produce. `already-running` means the subject's port was
 * already answering, so its live instance was reported and left untouched.
 */
export interface HarnessLaunchOutcome {
  outcome: 'started' | 'started-unconfirmed' | 'already-running' | 'unknown-target'
    | 'root-path-invalid' | 'launcher-missing' | 'profile-invalid'
    | 'port-occupied-external' | 'launch-args-invalid' | 'spawn-failed'
  kind: 'embedded-shell' | 'checkout'
  /** Present only when this application started and owns a child (a checkout). */
  pid?: number
  /** True when the subject's window was put out of sight after starting. */
  hidden: boolean
}

/**
 * What 「拉起 UI」 found, and what it had to do about it.
 *
 * Idempotent over the three states a subject can be in, so the caller does not
 * have to know which one it is looking at: it starts the subject when nothing
 * answers, shows a window the wallpaper hid at startup, and otherwise just brings
 * the window forward.
 */
export interface HarnessUiOutcome {
  outcome: 'raised' | 'raise-refused' | 'no-window' | 'not-running' | 'unknown-target'
  kind: 'embedded-shell' | 'checkout'
  /** True when this call had to start the subject first. */
  started: boolean
  /** The start's own code (word it with `launchOutcomeNotice`) when it had to. */
  startOutcome?: string
}

/**
 * What the last scan confirmed, and when.
 *
 * A scan is manual and expensive, so the settings surface shows this instead of
 * walking the disk on every open — with `verifiedAtMs` visible, because a cached
 * list must never look current: a client can be uninstalled between two scans.
 */
export interface HarnessTargetCatalog {
  schemaVersion: number
  /** Milliseconds since the epoch, from the scan that produced this list. */
  verifiedAtMs: number
  targets: HarnessTarget[]
  requiresSubjectChoice: boolean
}

/**
 * One harness execution subject, in the two classes the design froze
 * (`docs/design/harness-subject-and-ui-design.md`, §3).
 *
 * An `embedded-shell` carries its own checkout, so it fixes the service and the
 * window together and is identified by AUMID — never by a path, which is what
 * lets a client update leave a stored choice valid. A `checkout` is a source
 * tree: its path *is* its identity, it has no window of its own, and it is the
 * only class that uses the `profile` setting.
 */
export interface HarnessTarget {
  /** Stable key to store and later resolve back to a subject. */
  id: string
  /**
   * `embedded-shell` 自带检出、按 AUMID 寻址；`checkout` 是一棵源码树、路径就是身份；
   * `installed-cli` 是本机**全局安装**的 DSH CLI（npm 全局装的那种）—— 它没有 AUMID、也没有
   * 源码树，所以它与 checkout 的差别只有"没有树"：命令从 `node <tree>/apps/cli/lib/bin.js`
   * 换成 `dsh`，服务仍是它自举的 profile（默认 `web`，端口 3080）。
   */
  kind: 'embedded-shell' | 'checkout' | 'installed-cli'
  /** Which client shape this subject answers as, reusing the endpoint scan's vocabulary. */
  client: 'official-desktop' | 'official-web'
  label: string
  /**
   * 该主体自己声明的版本号，由扫描读出；读不到就没有这个字段。
   *
   * 三种来源不同（客户端读 exe 的 VERSIONINFO、源码目录读它自己的 package.json、已安装 CLI 读
   * npm 全局包清单），但含义只有一个：**那个东西自己说自己是什么版本**。缺失时前缀一条都不加，
   * 而不是写"未知"：我们没读到和我们读到了"未知"是两件事，占位符抹掉了这个区别，还会让用户以为
   * 这一条被检查过。
   */
  version?: string
  /**
   * 自带检出的壳，它的窗口属于哪个可执行文件 —— 扫描从注册它的那个快捷方式读到。
   *
   * 只在原生侧使用（后台启动后藏窗口、显式动作里把窗口找回来），渲染层不拿它做判断。缺失就是
   * "这次扫描没读到"：原生那时改问正在应答的那台客户端的可执行文件，而不是猜一条路径。
   */
  executable?: string
  /** Where the scan found it: a checkout's scan origin, or a shell's shortcut directory. */
  source: string
  identity: {
    /** The shell's AppUserModelID; set for shells only. */
    aumid?: string
    /** The checkout's root; set for checkouts only. */
    rootPath?: string
    /** Probe hints only. A user may move any port. */
    defaultPorts: number[]
  }
  launch: {
    /** `apps-folder` hands the shell a location-independent alias; `managed-command` spawns the tree. */
    kind: 'apps-folder' | 'managed-command'
    alias?: string
  }
  capabilities: {
    /** Re-launching reuses the running shell instead of opening a second window. */
    singleInstance: boolean
    /** It owns a Windows window, so 「拉起 UI」 can raise it. */
    ownsWindow: boolean
    /** It can be started with its window kept out of sight. */
    canStartHidden: boolean
    /** It needs the `profile` setting (checkouts only). */
    needsProfile: boolean
  }
}

/**
 * One subject scan. `requiresSubjectChoice` is set when more than one source
 * tree exists: the wallpaper must not choose for the user, so the settings
 * surface asks which tree is the default subject (§4.3).
 */
export interface HarnessTargetScan {
  targets: HarnessTarget[]
  requiresSubjectChoice: boolean
}

/**
 * Result of asking for a client's own Windows window.
 *
 * `raised` is the only success. `no-window` means something is listening on that
 * endpoint but owns no visible window — the CLI/webui shape, whose interface is a
 * browser URL instead — and `raise-refused` means Windows declined the foreground
 * change, which is normal for a desktop wallpaper and not a failure to report as
 * one. The codes are a contract with the wording table below.
 */
export interface RaiseClientOutcome {
  outcome: 'raised' | 'no-window' | 'not-running' | 'raise-refused'
  raised: boolean
}
export interface DeepSeekWebStatus { state: 'loading' | 'logged-out' | 'ready' | 'generating' | 'unsupported'; conversationId?: string; model?: string; signature: string }
export interface DeepSeekWebHistory { messages: ChatMessage[]; conversationId?: string; model?: string; state: DeepSeekWebStatus['state'] | 'loading' }
export interface DeepSeekWebAdapterConfigStatus { schemaVersion: number; adapterVersion: string; source: 'builtin' | 'local'; path: string; warning?: string }
/**
 * A bounded window of one durable API transcript. `hasMore` means older
 * messages exist on disk but were not cloned across IPC; the caller asks for a
 * larger `limit` when the user wants to read further back.
 */
export interface ApiHistoryPage {
  messages: ChatMessage[]
  totalMessages: number
  hasMore: boolean
  bytes: number
  limit: number
}
/**
 * One durable API transcript, as listed for the settings history page. Message
 * bodies are intentionally absent: this is a management view, not a reader.
 */
export interface ApiConversationSummary {
  id: string
  messageCount: number
  /** Serialized size of this transcript — what the persistence budget acts on. */
  bytes: number
  updatedAt: number
  firstMessageAt: number
  lastMessageAt: number
  /** True for the transcript this process is currently reading. */
  active: boolean
}

export interface ApiConversationListing {
  conversations: ApiConversationSummary[]
  totalBytes: number
  totalMessages: number
  /** Size the application trims to before writing. */
  budgetBytes: number
  /** Hard ceiling above which a save is refused. */
  maxBytes: number
}

export interface DesktopRect { x: number; y: number; width: number; height: number }
export interface DesktopDisplayInfo { id: string; name: string; bounds: DesktopRect; workArea: DesktopRect; scaleFactor: number; primary: boolean }

export interface NativeRuntime {
  isNative: boolean
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   setLockScreen(enabled: boolean): Promise<string>
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   clearStaleLockScreenBackup(confirmed: boolean): Promise<string>
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   lockScreenDiagnostics(): Promise<LockScreenDiagnostics>
  setAutostart(enabled: boolean): Promise<AutostartStatus>
  autostartStatus(): Promise<AutostartStatus>
  translucentTbStatus(): Promise<TranslucentTbStatus>
  launchTranslucentTb(): Promise<void>
  openTranslucentTbInstall(): Promise<void>
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   openWindowsLockScreenSettings(): Promise<void>
  /**
   * Store the DeepSeek API key the user typed in Settings.
   *
   * This is the one place a plaintext key crosses the Tauri IPC boundary, and only on the
   * way **in** — it replaced the Windows credential dialog, which the user found impossible
   * to read (it is built for username+password credentials). Nothing hands a key back:
   * `apiKeyStatus` answers with `maskApiKey`'s shape only, and the API client reads the
   * credential natively in Rust.
   */
  saveApiKey(key: string): Promise<void>
  /**
   * Whether a key is stored, and the masked form to show. `masked` is produced natively
   * (`mask_api_key`) and never contains the middle of the key.
   */
  apiKeyStatus(): Promise<ApiKeyStatus>
  /** 桌面会话工作区落在哪儿（只读自检）——与桥用的是同一条规则。 */
  desktopWorkspaceStatus(): Promise<DesktopWorkspaceStatus>
  openProjectMemory(): Promise<{ opened: string; memoryFile: string; memoryExists: boolean }>
  /**
   * 「打开 TUI」：在一个**新的终端窗口**里拉起本机的 TUI 命令（`dst`）。
   *
   * 契约与原生一致，而且调用方必须遵守：找不到 TUI 时返回 `opened: false` 与一句 `message`
   * 说明**怎么办** —— 那句话要显示出来，**不得**静默改成打开浏览器（那等于替用户换了一条
   * 他没选的路，而这正是这次改动要根除的失败模式）。
   *
   * 「启动参数」照常带上：它加的是启动器后面的话，而这条路的启动器就是 TUI 自己。
   */
  openSubjectTui(args?: string[]): Promise<{ opened: boolean; reason?: string; message?: string; launcher?: string }>
  requestDeepSeekLogin(): Promise<void>
  nativeBootstrapGeneration(): Promise<number>
  releaseNativeBootstrap(generation: number): Promise<boolean>
  ensureDeepSeekWeb(conversationId?: string, newConversation?: boolean): Promise<void>
  deepseekWebStatus(): Promise<DeepSeekWebStatus>
  deepseekWebHistory(conversationId?: string, newConversation?: boolean): Promise<DeepSeekWebHistory>
  deepseekWebAdapterConfig(): Promise<DeepSeekWebAdapterConfigStatus>
  openDeepSeekWebAdapterConfig(): Promise<DeepSeekWebAdapterConfigStatus>
  resetDeepSeekWebAdapterConfig(): Promise<DeepSeekWebAdapterConfigStatus>
  openSettingsWindow(): Promise<void>
  listenSystem(listener: (event: 'locked' | 'unlocked' | 'suspend' | 'resume') => void): Promise<() => void>
  listenChat(listener: (event: ScopedChatEvent) => void): Promise<() => void>
  sendChat(mode: BackendMode, text: string, options?: NativeSendOptions): Promise<string | undefined>
  cancelChat(mode: BackendMode): Promise<void>
  /**
   * 建立 Harness 会话。
   *
   * **端点端口不由调用方给**：`connect_harness` 命令自己按主体范围解析（源码里的原话是
   * "resolved here rather than trusted from the caller"——渲染端不许指定任意端口，而监视器读
   * 同一个值，于是状态与会话不可能指着两个不同的客户端）。这里只传模型与预设。
   */
  connectHarness(resumeSessionId: string | undefined, connectionId: string, model?: string, preset?: string): Promise<string>
  harnessHistory(): Promise<ChatMessage[]>
  harnessPresets(): Promise<Array<{ id: string; name?: string; description?: string; trust: 'system' | 'user'; broken?: string; isDefault: boolean }>>
  setHarnessPreset(preset: string): Promise<void>
  harnessControls(): Promise<{ permission: { current: string; options: string[] }; commands: Array<{ name: string; description: string; input?: { hint: string } }> }>
  /**
   * 当前 Harness 主体可用的模型目录（宿主提供、经桥接转发）。
   * `supported: false` = 宿主不具备枚举能力，不是"没有模型"。
   */
  harnessModels(): Promise<{ supported: boolean; provider?: string; current?: { provider: string; model: string }; models?: Array<{ id: string; name: string }> }>
  /**
   * 把选定的模型推给 Harness 宿主（宿主存进自己的设置，跨宿主重启生效）。
   * 宿主不具备该能力时拒绝——壁纸自己那次会话仍然用选定模型。
   */
  harnessSetModel(model: string): Promise<{ provider: string; model: string }>
  /** DeepSeek API 兼容端点自报的模型目录（`/models`）。 */
  apiModels(baseUrl: string): Promise<{ supported: boolean; models?: Array<{ id: string; name: string }> }>
  setHarnessPermission(permission: string): Promise<void>
  apiHistory(conversationId: string, limit?: number): Promise<ApiHistoryPage>
  listApiConversations(): Promise<ApiConversationListing>
  deleteApiConversation(conversationId: string): Promise<boolean>
  clearApiHistory(): Promise<number>
  listenTray(listener: (event: { type: 'backend'; backend: BackendMode }) => void): Promise<() => void>
  probeHarness(): Promise<HarnessStatus>
  /**
   * Publish which endpoints the wallpaper may talk to.
   *
   * The settings are the authority, and the native monitor owns the probe loop, so
   * the whole scope has to reach native state: the subject the user chose, the
   * endpoint they pinned, and the ports they added for a checkout. It travels as one
   * call because it is one decision — a partial update would leave the monitor
   * probing a port belonging to the subject the user has just left, which is the
   * silent substitution the connection path forbids.
   */
  setHarnessEndpointScope(scope: HarnessEndpointScope): Promise<HarnessEndpointScopeResult>
  /**
   * Leave the inner desktop — the island's 「X」.
   *
   * Native owns the fact "are we in the inner desktop": it hides and restores Explorer's
   * icon layer, and the floating ball's own pop-out rule keys off it. So the island asks
   * native instead of moving the interface by itself; the transition then arrives as the
   * same `desktop-workspace-toggle: leave` event the desktop double-click produces.
   */
  leaveInnerWorkspace(): Promise<void>
  /** Which local ports host a wallpaper Bridge, in the native layer's order. */
  scanHarnessEndpoints(extraPorts?: readonly number[]): Promise<HarnessEndpointScan[]>
  /**
   * Bring a desktop client's own window forward. Never launches anything, so a
   * client that is not running reports `not-running` rather than starting up.
   */
  /**
   * Verify that a renderer island `pointerdown` really is a user click.
   *
   * The native click route is closed, so the island event has to be reported from
   * here - and the native side deliberately does not trust that report: it re-checks
   * the physical left button and the window under the cursor, and rejects the call
   * otherwise. Returns the verdict for logging; it performs no activation.
   */
  verifyIslandClick(): Promise<string>
  raiseClientWindow(port: number): Promise<RaiseClientOutcome>
  /**
   * Open a windowless client's web UI in the default browser. Only a loopback
   * endpoint is accepted, so this cannot open an arbitrary destination.
   */
  openClientInBrowser(port: number, path?: string): Promise<void>
  /**
   * Open a link from the transcript in the default browser.
   *
   * The address comes from model output, so the native side validates it again
   * (`external_link::validate`): http/https only, ASCII only, no `user@` in the
   * authority. The renderer refuses to even style anything else as a link.
   */
  openExternalLink(url: string): Promise<void>
  /** Whether anything is listening, without raising it. */
  harnessEndpointListening(port: number): Promise<boolean>
  desktopDisplays(): Promise<DesktopDisplayInfo[]>
  desktopLayoutMetrics(displayId?: string): Promise<{ expandedBottomInset: number; taskbarVisible: boolean }>
  scanDshPaths(hintPath?: string, deepScan?: boolean): Promise<Array<{ rootPath: string; source: string }>>
  /**
   * List the harness execution subjects this machine offers: shells that carry
   * their own checkout, and source trees. Discovery only — it starts nothing and
   * takes over nothing.
   */
  scanHarnessTargets(hintPath?: string, deepScan?: boolean): Promise<HarnessTargetScan>
  /** The last confirmed subject list, or null when nothing has been scanned yet. */
  harnessTargetCatalog(): Promise<HarnessTargetCatalog | null>
  /**
   * Start the chosen execution subject. The class decides the mechanism — a shell
   * alias, or the managed checkout chain — so the caller passes an id and reads a
   * closed outcome code back.
   *
   * `args` is 「启动参数」, already tokenized: native appends it to whichever launcher this
   * build picked, so the runnable identity stays ours. It is honoured identically by this
   * path and the unattended one.
   */
  launchHarnessTarget(options: {
    targetId: string
    profile?: string
    args?: string[]
  }): Promise<HarnessLaunchOutcome>
  /**
   * The unattended counterpart, at most once per wallpaper process (native state
   * is the single-flight authority, exactly as for `autostartManagedDsh`). The one
   * difference that remains is real: only this path may keep a shell's window out of
   * sight. 「启动参数」跟着一起走 —— 自动启动与手动启动跑的是同一个启动器。
   */
  autostartHarnessTarget(options: {
    targetId?: string
    profile: string
    args?: string[]
  }): Promise<ManagedDshAutostart>
  /**
   * Make the chosen subject's interface available and foreground, whatever state it
   * is in: not running, running with a window the wallpaper hid, or behind another
   * window. Starts it when nothing answers, which is why this is native policy
   * rather than a caller's branch.
   */
  ensureHarnessUi(options: {
    targetId?: string
    port: number
    profile?: string
    args?: string[]
  }): Promise<HarnessUiOutcome>
  launchDsh(rootPath: string, profile: string, args?: string[]): Promise<number>
  /**
   * One automatic start attempt per process, with the outcome remembered even
   * when it fails so a bad configuration cannot become a retry loop.
   */
  autostartManagedDsh(options: {
    rootPath?: string
    profile: string
    args?: string[]
  }): Promise<ManagedDshAutostart>
  managedDshAutostartStatus(): Promise<ManagedDshAutostart | null>
  /**
   * 本应用启动着哪些 DSH。
   *
   * 传 `subjectId` 只影响 `managed` / `running` 两个汇总字段（"我这次启动的孩子还在不在"）；
   * `instances` 永远是全部，而且**官壳永远不在里面** —— 它不是本应用的实例，停它会当场关掉
   * 用户自己的客户端、并弹一条"宿主意外退出"的报错框。停止清单与这个列表是同一处，所以
   * 界面显示什么就能停什么。
   */
  managedDshStatus(subjectId?: string): Promise<ManagedDshStatus>
  /**
   * 停止本应用启动的 DSH：`instanceKey` 指名一个实例，不给就停**全部**。
   *
   * 一个实现、一个动作：「实例下拉里某一行的 ×」与「全部停止」走的是同一条命令，只是参数不同。
   */
  stopManagedDsh(instanceKey?: string): Promise<void>
}

async function tauriAvailable(): Promise<boolean> {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

const nativeWindowAvailable = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const nativeRuntime: NativeRuntime = {
  isNative: nativeWindowAvailable,
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   async setLockScreen(enabled) {
  //     if (!await tauriAvailable()) return '浏览器预览不支持设置系统锁屏。'
  //     const { invoke } = await import('@tauri-apps/api/core')
  //     return invoke<string>('set_lock_screen_enabled', { enabled })
  //   },
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   async clearStaleLockScreenBackup(confirmed) {
  //     if (!await tauriAvailable()) return '浏览器预览不支持清理系统锁屏恢复点。'
  //     const { invoke } = await import('@tauri-apps/api/core')
  //     return invoke<string>('clear_stale_lock_screen_backup', { confirmed })
  //   },
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   async lockScreenDiagnostics() {
  //     if (!await tauriAvailable()) return { supported: false, packageIdentity: false, takeoverAvailable: false, backupExists: false, backupValid: false, staleBackup: false, managedImageReady: false, managedImageActive: false, developmentBuild: false, warnings: ['浏览器预览不支持系统锁屏诊断。'] }
  //     const { invoke } = await import('@tauri-apps/api/core')
  //     return invoke<LockScreenDiagnostics>('get_lock_screen_diagnostics')
  //   },
  async setAutostart(enabled) {
    if (!await tauriAvailable()) return unsupportedAutostart()
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AutostartStatus>('set_autostart', { enabled })
  },
  async autostartStatus() {
    if (!await tauriAvailable()) return unsupportedAutostart()
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<AutostartStatus>('autostart_status')
  },
  async translucentTbStatus() {
    if (!await tauriAvailable()) return { installed: false, running: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<TranslucentTbStatus>('translucent_tb_status')
  },
  async launchTranslucentTb() {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('launch_translucent_tb')
  },
  async openTranslucentTbInstall() {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_translucent_tb_install')
  },
  // FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30，见 docs/plans/release-scope-cleanup-plan.md 第一节）。恢复办法：取消注释。
  //   async openWindowsLockScreenSettings() {
  //     if (!await tauriAvailable()) return
  //     const { invoke } = await import('@tauri-apps/api/core')
  //     await invoke('open_windows_lock_screen_settings')
  //   },
  async saveApiKey(key) {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持 Windows 凭据管理器')
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('save_api_key', { key })
  },
  async apiKeyStatus() {
    // 浏览器预览里没有凭据管理器：如实回答"没有 Key"，而不是抛错让设置页显示故障。
    if (!await tauriAvailable()) return { present: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ApiKeyStatus>('api_key_status')
  },
  async desktopWorkspaceStatus() {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持桌面会话工作区自检')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DesktopWorkspaceStatus>('desktop_workspace_status')
  },
  async openProjectMemory() {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持打开项目记忆')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ opened: string; memoryFile: string; memoryExists: boolean }>('open_project_memory')
  },
  async openSubjectTui(args?: string[]) {
    if (!await tauriAvailable()) throw new Error('仅桌面版支持打开 TUI')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ opened: boolean; reason?: string; message?: string; launcher?: string }>('open_subject_tui', { args })
  },  async requestDeepSeekLogin() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('show_deepseek_login')
  },
  async nativeBootstrapGeneration() {
    if (!await tauriAvailable()) return 0
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('native_bootstrap_generation')
  },
  async releaseNativeBootstrap(generation) {
    if (!await tauriAvailable()) return true
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('release_native_bootstrap', { generation })
  },
  async ensureDeepSeekWeb(conversationId, newConversation = false) {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('deepseek_web_ensure', { conversationId, newConversation })
  },
  async deepseekWebStatus() {
    if (!await tauriAvailable()) return { state: 'loading', signature: 'deepseek-chat-dom-v2' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebStatus>('deepseek_web_status')
  },
  async deepseekWebHistory(conversationId, newConversation = false) {
    if (!await tauriAvailable()) return { messages: [], state: 'loading' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebHistory>('deepseek_web_history', { conversationId, newConversation })
  },
  async deepseekWebAdapterConfig() {
    if (!await tauriAvailable()) return { schemaVersion: 1, adapterVersion: 'preview', source: 'builtin', path: '浏览器预览不支持本地网页适配器配置' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebAdapterConfigStatus>('deepseek_web_adapter_config_status')
  },
  async openDeepSeekWebAdapterConfig() {
    if (!await tauriAvailable()) throw new Error('浏览器预览不支持打开网页适配器配置')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebAdapterConfigStatus>('open_deepseek_web_adapter_config')
  },
  async resetDeepSeekWebAdapterConfig() {
    if (!await tauriAvailable()) throw new Error('浏览器预览不支持恢复网页适配器配置')
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DeepSeekWebAdapterConfigStatus>('reset_deepseek_web_adapter_config')
  },
  async openSettingsWindow() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_settings_window')
  },
  async listenSystem(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    return listen<'locked' | 'unlocked' | 'suspend' | 'resume'>('system-session', (event) => listener(event.payload))
  },
  async listenChat(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    return listen<ScopedChatEvent>('chat-event', (event) => listener(event.payload))
  },
  async sendChat(mode, text, options) {
    const { invoke } = await import('@tauri-apps/api/core')
    return (await invoke<string | null>('send_chat', {
      mode,
      text,
      conversationId: options?.conversationId,
      requestId: options?.requestId,
      newConversation: options?.newConversation,
      baseUrl: options?.baseUrl,
      model: options?.model,
      priceInputPerMillion: options?.priceInputPerMillion,
      priceOutputPerMillion: options?.priceOutputPerMillion,
    })) ?? undefined
  },
  async cancelChat(mode) {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('cancel_chat', { mode })
  },
  async connectHarness(resumeSessionId, connectionId, model, preset) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string>('connect_harness', { resumeSessionId, connectionId, model, preset })
  },
  async harnessHistory() {
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ messages: Array<{ id: string; role: 'user' | 'assistant'; content: string }> }>('harness_history')
    return result.messages.map((message) => ({ ...message, createdAt: Date.now() }))
  },
  async harnessPresets() {
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ presets: Array<{ id: string; name?: string; description?: string; trust: 'system' | 'user'; broken?: string; isDefault: boolean }> }>('harness_presets')
    return result.presets
  },
  async setHarnessPreset(preset) {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('harness_set_preset', { preset })
  },
  async harnessControls() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ permission: { current: string; options: string[] }; commands: Array<{ name: string; description: string; input?: { hint: string } }> }>('harness_controls')
  },
  async harnessModels() {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ supported: boolean; provider?: string; current?: { provider: string; model: string }; models?: Array<{ id: string; name: string }> }>('harness_models')
  },
  async apiModels(baseUrl) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ supported: boolean; models?: Array<{ id: string; name: string }> }>('api_models', { baseUrl })
  },
  async harnessSetModel(model) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ provider: string; model: string }>('harness_set_model', { model })
  },
  async setHarnessPermission(permission) {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('harness_set_permission', { permission })
  },
  async apiHistory(conversationId, limit) {
    if (!await tauriAvailable()) return { messages: [], totalMessages: 0, hasMore: false, bytes: 0, limit: limit ?? 0 }
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{
      messages: Array<{
        id: string
        role: 'user' | 'assistant'
        content: string
        createdAt: number
        usage?: ChatMessage['usage']
      }>
      totalMessages?: number
      hasMore?: boolean
      bytes?: number
      limit?: number
    }>('api_history', { conversationId, limit })
    // Message ids, timestamps and final usage are persisted by the native
    // DPAPI archive. Preserve them on resume so React keys and session cost
    // remain stable instead of fabricating a fresh zero-cost transcript.
    const messages = result.messages.map((message) => ({
      id: message.id,
      role: message.role,
      content: message.content,
      createdAt: message.createdAt,
      usage: message.usage,
    }))
    return {
      messages,
      totalMessages: result.totalMessages ?? messages.length,
      hasMore: result.hasMore === true,
      bytes: result.bytes ?? 0,
      limit: result.limit ?? messages.length,
    }
  },
  async listApiConversations() {
    if (!await tauriAvailable()) return { conversations: [], totalBytes: 0, totalMessages: 0, budgetBytes: 0, maxBytes: 0 }
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<Partial<ApiConversationListing>>('list_api_conversations')
    return {
      conversations: result.conversations ?? [],
      totalBytes: result.totalBytes ?? 0,
      totalMessages: result.totalMessages ?? 0,
      budgetBytes: result.budgetBytes ?? 0,
      maxBytes: result.maxBytes ?? 0,
    }
  },
  async deleteApiConversation(conversationId) {
    if (!await tauriAvailable()) return false
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('delete_api_conversation', { conversationId })
  },
  async clearApiHistory() {
    if (!await tauriAvailable()) return 0
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('clear_api_history')
  },
  async listenTray(listener) {
    if (!await tauriAvailable()) return () => undefined
    const { listen } = await import('@tauri-apps/api/event')
    const backendDispose = await listen<BackendMode>('tray-backend', (event) => listener({ type: 'backend', backend: event.payload }))
    return () => backendDispose()
  },
  async probeHarness() {
    if (!await tauriAvailable()) return { availability: 'offline' }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessStatus>('probe_harness')
  },
  /**
   * Publish the endpoint scope the native monitor probes from.
   *
   * The monitor owns the probe loop, so the choice must reach native state: a
   * per-request port would leave the rendered status coming from the shipped
   * priority order while the settings named a subject. `subjectId` is the important
   * half — without it native only knew ports, and connected to whichever client
   * answered.
   */
  async setHarnessEndpointScope(scope: HarnessEndpointScope) {
    if (!await tauriAvailable()) {
      return { port: scope.port ?? null, subjectId: scope.subjectId, subjectPorts: [] }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessEndpointScopeResult>('set_harness_endpoint', {
      // Explicit nulls rather than omitted keys: the command's own boundary is
      // strict about what it received, and `None` is well defined on that side.
      port: scope.port ?? null,
      subjectId: scope.subjectId ?? null,
      extraPorts: scope.extraPorts ? [...scope.extraPorts] : [],
      // 已分好词的 argv：原生只做形状检查，不再分一次词（两次解释就是注入）。
      args: parseLaunchArgs(scope.args),
    })
  },
  /**
   * Ask native to leave the inner desktop, restoring Explorer's icon layer.
   *
   * A failure here is worth showing: without it the desktop stays in "inner" state and
   * the floating ball will not pop out again.
   */
  async leaveInnerWorkspace() {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('leave_inner_workspace')
  },
  /**
   * Ask the native side to bring a desktop client's window forward.
   *
   * Native resolution matters here: the window is found from the port the
   * wallpaper is connected to, so it is the same client whose session the user is
   * talking to, and no stored executable path can go stale.
   */
  /** Returns the native verification verdict, or an empty string off Tauri. */
  async verifyIslandClick() {
      if (!await tauriAvailable()) return ''
      const { invoke } = await import('@tauri-apps/api/core')
      return invoke<string>('verify_island_click')
  },
  async raiseClientWindow(port: number) {
    if (!await tauriAvailable()) return { outcome: 'not-running' as const, raised: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<RaiseClientOutcome>('raise_client_window', { port })
  },
  /**
   * Open a windowless client's web UI in the default browser. Used only for the
   * CLI/webui shape, which has no Windows window to raise.
   */
  async openClientInBrowser(port: number, path = '/') {
    if (!await tauriAvailable()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_client_in_browser', { port, path })
  },
  async openExternalLink(url: string) {
    // 这里**不吞错误**：打开失败要能浮到界面上（调用方负责显示），静默失败等于"点了没反应"。
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_external_link', { url })
  },
  async harnessEndpointListening(port: number) {
    if (!await tauriAvailable()) return false
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('harness_endpoint_listening', { port })
  },
  /**
   * Ask the native side which local ports host a wallpaper Bridge.
   *
   * The renderer can scan too, but the status the desktop actually renders comes
   * from this process, so the native scan is the authoritative one. `extraPorts`
   * carries the user's own additions on top of the three known client shapes.
   */
  async scanHarnessEndpoints(extraPorts: readonly number[] = []) {
    if (!await tauriAvailable()) return []
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessEndpointScan[]>('scan_harness_endpoints_command', { extraPorts: [...extraPorts] })
  },
  async desktopDisplays() {
    if (!await tauriAvailable()) return [{ id: 'preview', name: '预览屏幕', bounds: { x: 0, y: 0, width: 1920, height: 1080 }, workArea: { x: 0, y: 0, width: 1920, height: 1080 }, scaleFactor: 1, primary: true }]
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<DesktopDisplayInfo[]>('desktop_displays')
  },
  async desktopLayoutMetrics(displayId) {
    if (!await tauriAvailable()) return { expandedBottomInset: 48, taskbarVisible: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<{ expandedBottomInset: number; taskbarVisible: boolean }>('desktop_layout_metrics', { displayId })
  },
  async scanDshPaths(hintPath, deepScan = false) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<Array<{ rootPath: string; source: string }>>('scan_dsh_paths', { hintPath, deepScan })
  },
  async scanHarnessTargets(hintPath, deepScan = false) {
    if (!await tauriAvailable()) return { targets: [], requiresSubjectChoice: false }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessTargetScan>('scan_harness_targets', { hintPath, deepScan })
  },
  async harnessTargetCatalog() {
    if (!await tauriAvailable()) return null
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessTargetCatalog | null>('harness_target_catalog')
  },
  async launchHarnessTarget(options) {
    if (!await tauriAvailable()) {
      return { outcome: 'spawn-failed' as const, kind: 'checkout' as const, hidden: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessLaunchOutcome>('launch_harness_target', {
      targetId: options.targetId,
      profile: options.profile,
      args: options.args,
    })
  },
  async autostartHarnessTarget(options) {
    if (!await tauriAvailable()) {
      // A browser preview has no native shell to own the process, so this reports
      // a terminal outcome instead of throwing into a boot path.
      return { outcome: 'spawn-failed' as const, external: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshAutostart>('autostart_harness_target', {
      targetId: options.targetId,
      profile: options.profile,
      args: options.args,
    })
  },
  async ensureHarnessUi(options) {
    if (!await tauriAvailable()) {
      return { outcome: 'not-running' as const, kind: 'checkout' as const, started: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<HarnessUiOutcome>('ensure_harness_ui', {
      targetId: options.targetId,
      port: options.port,
      profile: options.profile,
      args: options.args,
    })
  },
  async launchDsh(rootPath, profile, args) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('launch_dsh', { rootPath, profile, args })
  },
  /**
   * Ask the native side to start the configured DSH once per process.
   *
   * Native state is the single-flight authority: the caller may be a remounted
   * React tree, a second WebView, or an unlock broadcast, and all of them must
   * observe the same first-and-only attempt. A `false` gate here is a shortcut,
   * not the guarantee.
   */
  async autostartManagedDsh(options) {
    if (!await tauriAvailable()) {
      // A browser preview has no native shell to own the process. Report a
      // terminal outcome instead of throwing into a boot path.
      return { outcome: 'spawn-failed' as const, external: false }
    }
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshAutostart>('autostart_managed_dsh', {
      rootPath: options.rootPath,
      profile: options.profile,
      args: options.args,
    })
  },
  async managedDshAutostartStatus() {
    if (!await tauriAvailable()) return null
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshAutostart | null>('managed_dsh_autostart_status')
  },
  async managedDshStatus(subjectId?: string) {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<ManagedDshStatus>('managed_dsh_status', { subjectId })
  },
  async stopManagedDsh(instanceKey?: string) {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('stop_managed_dsh', { instanceKey })
  },
}
