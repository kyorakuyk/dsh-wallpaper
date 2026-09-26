import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function source(relativePath: string) {
  return readFile(resolve(wallpaperRoot, relativePath), 'utf8')
}

describe('native API credential prompt boundary', () => {
  it('keeps API-key plaintext out of the settings WebView and IPC arguments', async () => {
    const [settings, runtime] = await Promise.all([
      source('src/settings/SettingsWindow.tsx'),
      source('src/native/runtime.ts'),
    ])

    expect(settings).not.toContain('window.prompt')
    expect(settings).toContain('nativeRuntime.promptForApiKeyCredential()')
    expect(runtime).toContain('promptForApiKeyCredential(): Promise<boolean>')
    expect(runtime).toContain("invoke<boolean>('prompt_for_api_key')")
    expect(runtime).not.toContain('saveApiKey(key: string)')
    expect(runtime).not.toContain("invoke('save_api_key'")
  })

  it('uses a native generic CredUI prompt and restricts it to Settings', async () => {
    const native = await source('src-tauri/src/lib.rs')

    expect(native).toContain('fn prompt_for_api_key(caller: tauri::WebviewWindow)')
    expect(native).toContain('caller.label() != SETTINGS_WINDOW_LABEL')
    expect(native).toContain('CREDUI_FLAGS_GENERIC_CREDENTIALS')
    expect(native).toContain('CREDUI_FLAGS_ALWAYS_SHOW_UI')
    expect(native).toContain('CREDUI_FLAGS_PASSWORD_ONLY_OK')
    expect(native).toContain('CREDUI_FLAGS_DO_NOT_PERSIST')
    expect(native).toContain('CredWriteW')
    expect(native).not.toContain('fn save_api_key(key: String)')
  })

  /**
   * 用户实测问过："为什么点『更新 API key』会弹出这玩意？"（截图里是 CredUI 的"用户名 +
   * 密码"对话框）。答：它**就是 Windows 自己的凭据对话框**，用它是为了那条边界——明文不经过
   * WebView 与 IPC。但 CredUI 的字段是固定的，用户名那一栏一定会出现，所以不能让它空着，
   * 也不能让用户以为那栏有用。
   */
  it('fills the username CredUI insists on showing, and says where the key goes', async () => {
    const [native, panel] = await Promise.all([
      source('src-tauri/src/lib.rs'),
      source('src/settings/SettingsPanel.tsx'),
    ])

    // 预填用户名：空的下拉 + 浏览按钮正是让人看不懂的那个画面。
    expect(native).toContain('let mut credential_username = [0u16; CREDUI_MAX_USERNAME_LENGTH as usize + 1]')
    expect(native).toMatch(/let seed: Vec<u16> = CREDENTIAL_USERNAME\.encode_utf16\(\)\.collect\(\)/)
    expect(native).toContain('credential_username[..seed_len].copy_from_slice(&seed[..seed_len])')
    // 我们写死用户名，就不该让用户改它——否则他会以为自己填的名字生效了。
    expect(native).toContain('CREDUI_FLAGS_KEEP_USERNAME')
    // 对话框自己的提示要说"填在密码一栏"，不是笼统的"请输入"。
    expect(native).toContain('填到「密码」一栏')
    // 设置卡片也要先说明会弹一个 Windows 窗口，别让系统对话框突然出现。
    expect(panel).toContain('弹出 Windows')
  })
})
