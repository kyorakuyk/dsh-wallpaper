import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { BACKEND_MODE_LABELS, CHAT_MODE_OPTIONS, backendModeLabel, catalogAgeSuffix, chatModeOptions } from '../src/settings/SettingsPanel.tsx'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/**
 * 设置中心里的 chat 模式开关。
 *
 * 用户实测的问题："我们目前没有给运行中的壁纸设置切换 chat 模式的开关（web 桥接和 api）"。
 * 输入岛那个开关只做 Harness↔网页，而设置里那一栏原来叫「启动时使用」——**只影响下次启动**，
 * 点了它对正在跑的壁纸没有任何作用。这个开关现在两侧都管：切当下，也记住下次启动用哪个，
 * 而且**只管两种聊天后端**：Harness 由桌面上的那个开关负责（用户要求从这个下拉里删掉它）。
 */
describe('the chat-mode switch in the settings centre', () => {
  it('lists exactly the two chat backends, and never Harness', () => {
    // 下拉里只有两种聊天后端：它回答的是"滑槽在左边时聊天走哪个通道"。滑槽在右端时一定是
    // Harness —— 那是**滑槽自己的含义**，不由这里描述，也不留任何"不可选但可见"的项。
    expect(CHAT_MODE_OPTIONS.map((option) => option.value)).toEqual(['deepseek-web', 'deepseek-api'])
    expect(chatModeOptions()).toEqual([
      { value: 'deepseek-web', label: BACKEND_MODE_LABELS['deepseek-web'] },
      { value: 'deepseek-api', label: BACKEND_MODE_LABELS['deepseek-api'] },
    ])
    expect(chatModeOptions().some((option) => option.value === 'harness')).toBe(false)
    // 标签表仍然覆盖三种（提示语里会遇到 Harness）。
    expect(backendModeLabel('harness')).toBe('DeepSeek Harness')
    expect(backendModeLabel('deepseek-api')).toBe(BACKEND_MODE_LABELS['deepseek-api'])
  })

  it('switches the running wallpaper, not just the next launch', async () => {
    const [settings, panel] = await Promise.all([
      source('src/settings/SettingsWindow.tsx'),
      source('src/settings/SettingsPanel.tsx'),
    ])

    // 走原生的 AppCore：背景端就是靠 `app-snapshot` 改 `runtime.backend` 的（托盘同一条路）。
    expect(settings).toMatch(/const selectBackend = async \(backend: BackendMode\) => \{[\s\S]*?appCoreClient\.selectBackend\(backend\)/)
    // 同时写进 defaultBackend：它是"下次启动"的默认值，两件事本来就是一件事的两面。
    expect(settings).toMatch(/const selectBackend = async \(backend: BackendMode\) => \{[\s\S]*?defaultBackend: backend/)
    // 面板把这一栏接到那个 handler 上，显示并编辑**启动默认值**（不是运行中的值：这一栏给
    // "滑槽在左边"赋予含义，此刻在不在 Harness 由滑槽表达）。
    expect(panel).toContain('onChange={(value) => props.onSelectBackend(value as BackendMode)}')
    expect(panel).toContain('value={settings.defaultBackend}')
    // 候选恒为两项，且调用时不传"当前值"——没有"必要时追加一项"这回事了。
    expect(panel).toContain('chatModeOptions()')
    // 文案只说这一栏自己管什么，不再解释滑槽（那句解释会制造一个用户本来没有的问题）。
    expect(panel).toContain('滑槽在左边时，聊天走这里选的通道')
    expect(panel).not.toContain('Harness 由桌面上的那个开关切换')
    expect(panel).not.toContain('Harness 不在这里切换')
    expect(panel).not.toContain('title="启动时使用"')
  })

  it('is allowed for the settings surface in the native gate and the capability', async () => {
    const [native, capability] = await Promise.all([
      source('src-tauri/src/lib.rs'),
      source('src-tauri/capabilities/settings.json'),
    ])

    // 光有权限不够：命令自己也会认调用者。`select_backend` 原来只认壁纸宿主。
    expect(native).toMatch(/fn select_backend\([\s\S]{0,900}?require_wallpaper_surface\(&caller\)\?/)
    expect(capability).toContain('"allow-select-backend"')
    // 而且切换后仍要把快照发给背景端，否则改了状态、壁纸却不知道。
    expect(native).toMatch(/fn select_backend\([\s\S]{0,1200}?emit_app_snapshot\(&app, &snapshot\)/)
  })

  it('returns to the configured chat backend when leaving Harness', async () => {
    const [app, bubble] = await Promise.all([
      source('src/App.tsx'),
      source('src/features/chat/ConversationBubble.tsx'),
    ])

    // 用户实测："从 harness 模式退回来仍旧默认进入 web 端桥"——因为这个开关写死回到了网页。
    // 现在它回到了**设置里选的那个**聊天后端。
    expect(bubble).toContain("props.backend === 'harness' ? (props.nonHarnessBackend ?? 'deepseek-web') : 'harness'")
    expect(bubble).not.toContain("props.backend === 'harness' ? 'deepseek-web' : 'harness'")
    expect(app).toContain("nonHarnessBackend={settings.defaultBackend === 'harness' ? 'deepseek-web' : settings.defaultBackend}")
  })

  it('renders how old the persisted model list is', () => {
    const now = Date.parse('2026-09-27T12:00:00.000Z')
    expect(catalogAgeSuffix(undefined, now)).toBe('')
    expect(catalogAgeSuffix('not a date', now)).toBe('')
    expect(catalogAgeSuffix('2026-09-27T11:59:40.000Z', now)).toBe('（刚刚拉取）')
    expect(catalogAgeSuffix('2026-09-27T11:30:00.000Z', now)).toBe('（30 分钟前拉取）')
    expect(catalogAgeSuffix('2026-09-27T06:00:00.000Z', now)).toBe('（6 小时前拉取）')
    expect(catalogAgeSuffix('2026-09-20T12:00:00.000Z', now)).toBe('（7 天前拉取）')
  })

  it('persists the pulled model list, keyed by the address it came from', async () => {
    const [settings, store, panel] = await Promise.all([
      source('src/settings/SettingsWindow.tsx'),
      source('src/settings/store.ts'),
      source('src/settings/SettingsPanel.tsx'),
    ])

    // 拉取成功后写进设置（用户实测："api 的刷新结果也没有持久化，下次仍旧需要重新刷新"）。
    expect(settings).toMatch(/const refreshApiModelCatalog = async \(\): Promise<boolean> => \{[\s\S]*?modelCatalog: \{ baseUrl, models, fetchedAt: new Date\(\)\.toISOString\(\) \}/)
    // 打开窗口时用缓存填充，但**地址不匹配就不认**：旧地址的列表不是新地址的模型清单。
    expect(settings).toMatch(/const cached = settingsRef\.current\.deepseekApi\.modelCatalog[\s\S]{0,200}?cached\.baseUrl === settingsRef\.current\.deepseekApi\.baseUrl[\s\S]{0,120}?setApiModelCatalog\(cached\.models\)/)
    // 存储层要能持久化并校验它（响应是外部数据，一律当不可信 JSON 处理）。
    expect(store).toContain('modelCatalog?: ApiModelCatalog')
    expect(store).toContain('function normalizeApiModelCatalog(value: unknown): ApiModelCatalog | undefined')
    expect(store).toMatch(/const modelCatalog = normalizeApiModelCatalog\(raw\.modelCatalog\)/)
    // 界面上要说明这份列表是什么时候拉的。
    expect(panel).toContain('catalogAgeSuffix(props.apiModelCatalogFetchedAt)')
  })

  it('does not subscribe the settings window to the background-only snapshot event', async () => {
    const settings = await source('src/settings/SettingsWindow.tsx')

    // `app-snapshot` 是定向发给背景端的（`emit_to(BACKGROUND_WINDOW_LABEL)`），
    // "系统事件不进设置 WebView"这条边界由 nativeChatBoundary.spec.ts 钉着：为了一个显示值
    // 放宽它不值得，所以本窗口只在打开时读一次快照。
    expect(settings).not.toContain('appCoreClient.subscribe')
    expect(settings).toMatch(/const \[liveBackend, setLiveBackend\] = useState<BackendMode>\(\)/)
    expect(settings).toMatch(/void appCoreClient\.snapshot\(\)\.then\(\(snapshot\) => \{[\s\S]{0,400}?setLiveBackend\(snapshot\.backend\)/)
  })
})
