import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { BACKEND_MODE_OPTIONS, backendModeLabel } from '../src/settings/SettingsPanel.tsx'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/**
 * 设置中心里的 chat 模式开关。
 *
 * 用户实测的问题："我们目前没有给运行中的壁纸设置切换 chat 模式的开关（web 桥接和 api）"。
 * 输入岛那个开关只做 Harness↔网页，而设置里那一栏原来叫「启动时使用」——**只影响下次启动**，
 * 点了它对正在跑的壁纸没有任何作用。这个开关现在两侧都管：切当下，也记住下次启动用哪个。
 */
describe('the chat-mode switch in the settings centre', () => {
  it('offers all three modes from one place', () => {
    expect(BACKEND_MODE_OPTIONS.map((option) => option.value)).toEqual(['deepseek-web', 'deepseek-api', 'harness'])
    // 提示语与下拉共用同一份文案。
    expect(backendModeLabel('deepseek-api')).toBe(BACKEND_MODE_OPTIONS[1]!.label)
    expect(backendModeLabel('harness')).toBe('DeepSeek Harness')
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
    // 面板把这一栏接到那个 handler 上，并且**显示运行中的值**（不是启动默认值）。
    expect(panel).toContain('onChange={(value) => props.onSelectBackend(value as BackendMode)}')
    expect(panel).toContain('value={props.liveBackend ?? settings.defaultBackend}')
    // 文案要直说"立即生效"，不能再写"启动时使用"。
    expect(panel).toContain('立即切换正在运行的壁纸')
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
