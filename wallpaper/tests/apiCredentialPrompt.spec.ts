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
})
