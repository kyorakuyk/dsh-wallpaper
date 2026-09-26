import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function source(relativePath: string) {
  return readFile(resolve(wallpaperRoot, relativePath), 'utf8')
}

/**
 * API Key 的输入路径，以及它留下/丢掉了什么。
 *
 * 这里记着一次**有意的性质变更**：最初 Key 由 Windows 自己的凭据对话框（CredUI）收集，明文
 * 从不经过渲染端与 IPC。用户实测反馈那个对话框读不懂（它是给"用户名 + 密码"的网络凭据设计的，
 * 用户名那一栏一定会出现），明确要求"在我们的设置窗内输入 apikey"。于是改成本窗口输入，代价是
 * 明文会**经 IPC 进来一次**——所以下面这些断言守的就是"换掉的那一半之外还剩什么"。
 */
describe('API key entry in the settings window', () => {
  it('collects the key in our own window and never lets it come back out', async () => {
    const [settings, panel, runtime] = await Promise.all([
      source('src/settings/SettingsWindow.tsx'),
      source('src/settings/SettingsPanel.tsx'),
      source('src/native/runtime.ts'),
    ])

    // 设置窗自己收：密码型输入框 + 保存入口。
    expect(panel).toContain('type="password"')
    expect(panel).toContain('apiKeyDraft')
    expect(settings).toContain('nativeRuntime.saveApiKey(')
    expect(runtime).toContain("invoke('save_api_key', { key })")
    // 系统凭据对话框那条路已经撤掉（连同它的权限与绑定），不留一条没人走的旧路。
    expect(settings).not.toContain('promptForApiKeyCredential')
    expect(runtime).not.toContain('prompt_for_api_key')
    expect(runtime).not.toContain('promptForApiKeyCredential')

    // 反向永远不成立：渲染端拿不到明文，只有脱敏串。
    expect(runtime).toContain('apiKeyStatus(): Promise<ApiKeyStatus>')
    expect(runtime).toContain("invoke<ApiKeyStatus>('api_key_status')")
    expect(runtime).not.toMatch(/getApiKey|readApiKey|apiKeyPlain/)
  })

  it('masks the stored key natively, and never shows the middle of it', async () => {
    const [native, panel] = await Promise.all([
      source('src-tauri/src/lib.rs'),
      source('src/settings/SettingsPanel.tsx'),
    ])

    // 脱敏在原生侧算（`mask_api_key`），设置窗只负责把它摆出来。
    expect(native).toContain('fn mask_api_key(key: &str) -> String')
    expect(native).toContain('Ok(key) => Ok(serde_json::json!({ "present": true, "masked": mask_api_key(&key) }))')
    expect(panel).toContain('props.apiKeyStatus.masked')
  })

  it('restricts the new commands to the settings surface', async () => {
    const [native, capability, build] = await Promise.all([
      source('src-tauri/src/lib.rs'),
      source('src-tauri/capabilities/settings.json'),
      source('src-tauri/build.rs'),
    ])

    expect(native).toContain('fn save_api_key(caller: tauri::WebviewWindow, key: String)')
    expect(native).toContain('fn api_key_status(caller: tauri::WebviewWindow)')
    expect(native).toContain('caller.label() != SETTINGS_WINDOW_LABEL')
    // 权限只给设置窗：读状态的命令同样不开放给壁纸/球/网页那些窗口。
    expect(capability).toContain('"allow-save-api-key"')
    expect(capability).toContain('"allow-api-key-status"')
    expect(capability).not.toContain('allow-prompt-for-api-key')
    // 模型目录要在设置窗里能拉（测试与刷新都靠它）。
    expect(capability).toContain('"allow-api-models"')
    // **光有权限还不够**：命令自己也会认调用者。原来 `api_models` 只认壁纸宿主，用户实测点
    // 「测试」直接报"该命令只允许壁纸宿主调用"。现在它认两个表面（球、网页窗口仍然不行）。
    expect(native).toMatch(/async fn api_models\(caller: tauri::WebviewWindow, base_url: String\) -> Result<serde_json::Value, String> \{[\s\S]{0,600}?require_wallpaper_surface\(&caller\)\?/)
    expect(native).toContain('BACKGROUND_WINDOW_LABEL | SETTINGS_WINDOW_LABEL => Ok(())')
    // 命令要进 ACL 白名单，否则调用会被 tauri 直接拒掉。
    expect(build).toContain('"save_api_key"')
    expect(build).toContain('"api_key_status"')
    expect(build).not.toContain('"prompt_for_api_key"')
  })

  it('tests the key by pulling the model catalogue, and offers a separate refresh', async () => {
    const [settings, panel] = await Promise.all([
      source('src/settings/SettingsWindow.tsx'),
      source('src/settings/SettingsPanel.tsx'),
    ])

    // 「测试」= 保存(如有草稿) + 拉模型目录 + 回读脱敏状态：一次请求同时回答"Key 能不能用"和
    // "现在有哪些模型"（`/models` 需要密钥，200 就等于密钥可用）。
    expect(settings).toContain('nativeRuntime.apiModels(')
    expect(settings).toMatch(/const testApiKey = async \(\) => \{[\s\S]*?nativeRuntime\.saveApiKey\(draft\)/)
    expect(settings).toMatch(/const testApiKey = async \(\) => \{[\s\S]*?refreshApiModelCatalog\(\)/)
    expect(settings).toMatch(/const testApiKey = async \(\) => \{[\s\S]*?readApiKeyStatus\(\)/)
    // 「刷新」只重拉目录，不动密钥。
    expect(settings).toMatch(/const refreshApiModels = async \(\) => \{[\s\S]*?refreshApiModelCatalog\(\)/)
    // 两个按钮在面板里，且「测试」在输入框右侧（用户明确要求"最右侧"）。
    expect(panel).toMatch(/type="password"[\s\S]{0,900}onClick=\{props\.onTestApiKey\}/)
    expect(panel).toContain('onClick={props.onRefreshApiModels}')
    // 模型那一栏由拉取到的目录喂候选。
    expect(panel).toContain('props.apiModelCatalog')
  })

  it('drops the API address field and keeps it as a default the API client still honours', async () => {
    const [panel, store, settings] = await Promise.all([
      source('src/settings/SettingsPanel.tsx'),
      source('src/settings/store.ts'),
      source('src/settings/SettingsWindow.tsx'),
    ])

    // 用户："我们只深耕 deepseek，所以 API 网址可以省略"。
    expect(panel).not.toContain('title="API 地址"')
    // 值还在（老配置里的自定义地址继续生效），只是不再让人手填。
    expect(store).toContain('https://api.deepseek.com')
    expect(settings).toContain('settingsRef.current.deepseekApi.baseUrl')
  })
})
