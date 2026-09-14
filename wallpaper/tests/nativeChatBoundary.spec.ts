import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const nativeRoot = resolve(wallpaperRoot, 'src-tauri')

async function readNative(relativePath: string): Promise<string> {
  // Git for Windows may check the same source out as CRLF. Normalize before
  // applying source-boundary assertions so CI verifies the contract rather
  // than the runner's checkout line-ending policy.
  return (await readFile(resolve(nativeRoot, relativePath), 'utf8')).replace(/\r\n?/g, '\n')
}

async function readCapability(name: 'background' | 'settings'): Promise<{ windows?: unknown; permissions?: unknown }> {
  return JSON.parse(await readNative(`capabilities/${name}.json`)) as { windows?: unknown; permissions?: unknown }
}

function permissions(capability: { permissions?: unknown }): string[] {
  expect(capability.permissions).toBeInstanceOf(Array)
  return capability.permissions as string[]
}

describe('native chat boundary', () => {
  const sensitiveChatCommands = [
    'allow-send-chat',
    'allow-cancel-chat',
    'allow-connect-harness',
    'allow-harness-history',
    'allow-api-history',
    'allow-deepseek-web-history',
  ]

  it('creates explicit application-command ACL entries at build time', async () => {
    const build = await readNative('build.rs')

    expect(build).toContain('tauri_build::try_build')
    expect(build).toContain('tauri_build::AppManifest::new().commands')
    for (const command of [
      'send_chat', 'cancel_chat', 'connect_harness', 'harness_history', 'api_history',
    ]) expect(build).toContain(`"${command}"`)
  })

  it('does not grant chat commands to the settings WebView', async () => {
    const [background, settings] = await Promise.all([readCapability('background'), readCapability('settings')])

    expect(background.windows).toStrictEqual(['background'])
    expect(settings.windows).toStrictEqual(['settings'])
    for (const permission of sensitiveChatCommands) {
      expect(permissions(background)).toContain(permission)
      expect(permissions(settings)).not.toContain(permission)
    }
  })

  it('uses explicit least-privilege core permissions for both WebViews', async () => {
    const [background, settings] = await Promise.all([readCapability('background'), readCapability('settings')])

    for (const capability of [background, settings]) {
      expect(permissions(capability)).not.toContain('core:default')
      expect(permissions(capability)).not.toContain('core:event:default')
      expect(permissions(capability)).not.toContain('core:window:default')
      expect(permissions(capability)).not.toContain('core:event:allow-emit')
      expect(permissions(capability)).not.toContain('core:event:allow-emit-to')
    }
    expect(permissions(background)).toEqual(expect.arrayContaining([
      'core:event:allow-listen',
      'core:event:allow-unlisten',
    ]))
    expect(permissions(settings)).toEqual(expect.arrayContaining([
      'core:event:allow-listen',
      'core:event:allow-unlisten',
      'allow-publish-settings',
      'allow-notify-appearance-changed',
    ]))
  })

  it('declares an MSIX StartupTask while retaining the native autostart fallback', async () => {
    const [manifest, lib] = await Promise.all([
      readFile(resolve(wallpaperRoot, '..', 'packaging/msix/AppxManifest.xml'), 'utf8'),
      readNative('src/lib.rs'),
    ])
    expect(manifest).toContain('windows.startupTask')
    expect(manifest).toContain('DshWallpaperStartup')
    expect(lib).toContain('windows_integration::set_startup_task')
    expect(lib).toContain('HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run')
  })

  it('keeps the DeepSeek web transport in a dedicated persistent WebView', async () => {
    const [lib, web, background] = await Promise.all([
      readNative('src/lib.rs'),
      readNative('src/deepseek_web.rs'),
      readCapability('background'),
    ])
    expect(lib).toContain('mod deepseek_web;')
    expect(lib).toContain('deepseek_web::DeepSeekWebState::default()')
    expect(web).toContain('deepseek-webview2')
    expect(web).toContain('WebviewUrl::External')
    expect(web).toContain('deepseek-chat-dom-v2')
    expect(web).toContain('MutationObserver')
    expect(web).toContain('does not read cookies')
    expect(web).toContain('DEEPSEEK_WEB_UNSUPPORTED')
    expect(permissions(background)).toEqual(expect.arrayContaining([
      'allow-deepseek-web-ensure',
      'allow-deepseek-web-status',
      'allow-deepseek-web-history',
    ]))
  })

  it('keeps the native first-frame handoff bounded and recoverable', async () => {
    const [bootstrap, integration, readme] = await Promise.all([
      readNative('src/native_bootstrap.rs'),
      readNative('src/windows_integration.rs'),
      readFile(resolve(wallpaperRoot, '..', 'README.md'), 'utf8'),
    ])

    expect(integration).toContain('WORKERW_RETRY_WINDOW')
    expect(integration).toContain('wait_for_visible_wallpaper_worker')
    expect(integration).toContain('native_bootstrap::reattach_to_workerw()')
    expect(bootstrap).toContain('startup-diagnostic.log')
    expect(bootstrap).toContain('event=reattach parent=WorkerW')
    expect(readme).toContain('startup-diagnostic.log')
    expect(readme).toContain('DeepSeek 网页 DOM 桥接')
  })

  it('requires explicit confirmation before deleting a saved lock-screen original', async () => {
    const [runtime, liteNative, settings, liteSettings, lib, integration] = await Promise.all([
      readFile(resolve(wallpaperRoot, 'src/native/runtime.ts'), 'utf8'),
      readFile(resolve(wallpaperRoot, 'src/lite/native.ts'), 'utf8'),
      readFile(resolve(wallpaperRoot, 'src/settings/SettingsWindow.tsx'), 'utf8'),
      readFile(resolve(wallpaperRoot, 'src/lite/LiteSettingsWindow.tsx'), 'utf8'),
      readNative('src/lib.rs'),
      readNative('src/windows_integration.rs'),
    ])
    expect(runtime).toContain('clearStaleLockScreenBackup(confirmed: boolean)')
    expect(runtime).toContain("{ confirmed }")
    expect(liteNative).toContain('clearStaleLockScreenBackup(confirmed: boolean)')
    expect(settings).toContain('window.confirm(')
    expect(liteSettings).toContain('window.confirm(')
    expect(lib).toContain('confirmed: bool')
    expect(integration).toContain('永久删除已保存的原锁屏图片副本')
  })

  it('bounds appearance assets before they cross the IPC base64 boundary', async () => {
    const [commands, importer] = await Promise.all([
      readNative('src/appearance/commands.rs'),
      readNative('src/appearance/importer.rs'),
    ])
    expect(commands).toContain('read_resolved_asset')
    expect(commands).toContain('DEFAULT_MAX_ASSET_BYTES')
    expect(importer).toContain('DEFAULT_MAX_ASSET_BYTES')
    expect(importer).toContain('MAX_ZIP_EXPANSION_RATIO')
    expect(importer).toContain('total_size')
  })

  it('does not treat a quiet generating page as complete without a terminal hint', async () => {
    const web = await readNative('src/deepseek_web.rs')
    expect(web).toContain('completion_hint')
    expect(web).toContain('terminalAction')
    expect(web).toContain('QUIET_COMPLETION_POLLS: u8 = 20')
    expect(web).toContain('completion_hint && quiet_polls')
  })

  it('keeps autostart changes off the settings renderer thread and migrates old installs', async () => {
    const [lib, settings] = await Promise.all([
      readNative('src/lib.rs'),
      readFile(resolve(wallpaperRoot, 'src/settings/SettingsWindow.tsx'), 'utf8'),
    ])
    expect(lib).toMatch(/async\s+fn\s+set_autostart[\s\S]*?spawn_blocking\(move \|\| set_autostart_blocking\(enabled\)\)/)
    expect(lib).toContain('migrate_legacy_autostart')
    expect(settings).toContain("nativeRuntime.autostartStatus()")
    expect(settings).toContain('autostartOperationRef')
    expect(settings).toContain('autostartBusy')
  })

  it('uses the built DSH CLI for managed launches when it is available', async () => {
    const lib = await readNative('src/lib.rs')

    expect(lib).toContain('.join("apps")')
    expect(lib).toContain('.join("cli")')
    expect(lib).toContain('.join("lib")')
    expect(lib).toContain('.join("bin.js")')
    expect(lib).toContain('node.exe')
    expect(lib).toContain('pnpm.cmd')
    expect(lib).toContain('tsx/esm')
  })

  it('uses Rust as the only settings-to-background event router', async () => {
    const [settings, lib] = await Promise.all([
      readFile(resolve(wallpaperRoot, 'src/settings/SettingsWindow.tsx'), 'utf8'),
      readNative('src/lib.rs'),
    ])

    expect(settings).not.toContain("from '@tauri-apps/api/event'")
    expect(settings).toContain("invoke('publish_settings', { settings: next })")
    expect(settings).toContain("invoke('notify_appearance_changed')")
    expect(lib).toMatch(/fn publish_settings\s*\([\s\S]*?require_settings\(&caller\)\?;[\s\S]*?EventTarget::webview_window\(BACKGROUND_WINDOW_LABEL\)/)
    expect(lib).toMatch(/fn notify_appearance_changed\s*\([\s\S]*?require_settings\(&caller\)\?;[\s\S]*?EventTarget::webview_window\(BACKGROUND_WINDOW_LABEL\)/)
  })

  it('defends the sensitive commands against a future ACL mistake', async () => {
    const lib = await readNative('src/lib.rs')

    expect(lib).toMatch(/const BACKGROUND_WINDOW_LABEL:\s*&str\s*=\s*"background"/)
    expect(lib).toMatch(/fn require_background\s*\(caller:\s*&tauri::WebviewWindow\)/)
    for (const command of ['send_chat', 'cancel_chat', 'connect_harness', 'harness_history', 'api_history']) {
      const signature = new RegExp(`(?:async\\s+)?fn\\s+${command}\\s*\\([\\s\\S]*?\\)\\s*->[^\\{]*\\{[\\s\\S]*?require_background\\(&caller\\)\\?;`)
      expect(lib, command).toMatch(signature)
    }
  })

  it('keeps system-setting and desktop-input commands bound to their owning surface', async () => {
    const lib = await readNative('src/lib.rs')

    for (const command of [
      'set_lock_screen_enabled', 'get_lock_screen_diagnostics', 'set_autostart', 'autostart_status',
      'translucent_tb_status', 'launch_translucent_tb', 'open_translucent_tb_install',
      'start_settings_drag', 'hide_settings_window',
    ]) {
      const signature = new RegExp(`(?:async\\s+)?fn\\s+${command}\\s*\\([\\s\\S]*?\\)\\s*->[^\\{]*\\{[\\s\\S]*?require_settings\\(&caller\\)\\?;`)
      expect(lib, command).toMatch(signature)
    }
    for (const command of ['begin_interaction_region_session', 'update_interaction_regions', 'probe_harness']) {
      const signature = new RegExp(`(?:async\\s+)?fn\\s+${command}\\s*\\([\\s\\S]*?\\)\\s*->[^\\{]*\\{[\\s\\S]*?require_background\\(&caller\\)\\?;`)
      expect(lib, command).toMatch(signature)
    }
    expect(lib).toMatch(/fn show_deepseek_login[\s\S]*?require_wallpaper_surface\(&caller\)\?;/)
    expect(lib).toMatch(/fn get_app_snapshot[\s\S]*?require_wallpaper_surface\(&caller\)\?;/)
  })

  it('keeps appearance library management in settings while the background can only resolve active assets', async () => {
    const [background, settings, commands] = await Promise.all([
      readCapability('background'),
      readCapability('settings'),
      readNative('src/appearance/commands.rs'),
    ])

    const backgroundPermissions = permissions(background)
    const settingsPermissions = permissions(settings)
    const settingsOnlyPermissions = [
      'allow-appearance-list-themes',
      'allow-appearance-list-assets',
      'allow-appearance-activate-theme',
      'allow-appearance-set-override',
      'allow-appearance-clear-override',
      'allow-appearance-import-paths',
      'allow-appearance-classify-asset',
      'allow-appearance-export-current-theme',
    ]

    expect(backgroundPermissions).toContain('allow-appearance-resolve-asset')
    for (const permission of [
      'allow-appearance-get-state',
      'allow-appearance-list-themes',
      'allow-appearance-list-assets',
      'allow-appearance-resolve-library-asset',
      ...settingsOnlyPermissions,
    ]) expect(backgroundPermissions).not.toContain(permission)
    for (const permission of settingsOnlyPermissions) expect(settingsPermissions).toContain(permission)

    for (const command of [
      'appearance_list_themes',
      'appearance_list_assets',
      'appearance_activate_theme',
      'appearance_set_override',
      'appearance_clear_override',
      'appearance_import_paths',
      'appearance_classify_asset',
      'appearance_export_current_theme',
    ]) {
      const signature = new RegExp(`fn\\s+${command}\\s*\\([\\s\\S]*?\\)\\s*->[^\\{]*\\{[\\s\\S]*?require_appearance_settings\\(&caller\\)`)
      expect(commands, command).toMatch(signature)
    }
    for (const command of ['appearance_get_state', 'appearance_resolve_asset', 'appearance_resolve_library_asset']) {
      const signature = new RegExp(`fn\\s+${command}\\s*\\([\\s\\S]*?\\)\\s*->[^\\{]*\\{[\\s\\S]*?require_appearance_reader\\(&caller\\)`)
      expect(commands, command).toMatch(signature)
    }
  })

  it('targets conversation bodies only at the wallpaper WebView', async () => {
    const chat = await readNative('src/chat.rs')

    expect(chat).toContain('EventTarget::webview_window("background")')
    expect(chat).not.toMatch(/\.emit\(\s*"chat-event"/)
  })

  it('keeps system, workspace, and tray backend events out of the settings WebView', async () => {
    const [lib, windows] = await Promise.all([
      readNative('src/lib.rs'),
      readNative('src/windows_integration.rs'),
    ])

    expect(lib).toMatch(/EventTarget::webview_window\(BACKGROUND_WINDOW_LABEL\)[\s\S]*?"tray-backend"/)
    expect(windows).toContain('fn emit_to_background')
    for (const event of ['system-session', 'desktop-workspace-toggle', 'app-snapshot']) {
      expect(windows).not.toMatch(new RegExp(`app\\.emit\\(\\s*"${event}"`))
    }
  })

  it('validates Bridge SSE events against a closed native protocol before emitting them', async () => {
    const chat = await readNative('src/chat.rs')

    expect(chat).toContain('enum BridgeEvent')
    expect(chat).toContain('deny_unknown_fields')
    expect(chat).toContain('fn parse_bridge_event')
    expect(chat).toContain('parse_bridge_event(&line, session_id)')
    expect(chat).not.toContain('scoped_harness_event')
  })

  it('starts the single-instance guard before resident plugins', async () => {
    const [cargo, lib] = await Promise.all([readNative('Cargo.toml'), readNative('src/lib.rs')])

    expect(cargo).toMatch(/^rust-version\s*=\s*"1\.77\.2"/m)
    expect(cargo).toMatch(/^tauri-plugin-single-instance\s*=\s*"2\.4\.3"/m)
    const singleInstance = lib.indexOf('.plugin(tauri_plugin_single_instance::init')
    const logPlugin = lib.indexOf('tauri_plugin_log::Builder::new()')
    const dialogPlugin = lib.indexOf('.plugin(tauri_plugin_dialog::init())')
    expect(singleInstance).toBeGreaterThan(-1)
    expect(singleInstance).toBeLessThan(logPlugin)
    expect(singleInstance).toBeLessThan(dialogPlugin)
  })

  it('pins the Windows single-instance race fix to the reviewed local vendor copy', async () => {
    const [cargo, notice, windows] = await Promise.all([
      readNative('Cargo.toml'),
      readNative('vendor/tauri-plugin-single-instance/NOTICE.md'),
      readNative('vendor/tauri-plugin-single-instance/src/platform_impl/windows.rs'),
    ])

    expect(cargo).toMatch(/\[patch\.crates-io\][\s\S]*tauri-plugin-single-instance\s*=\s*\{\s*path\s*=\s*"vendor\/tauri-plugin-single-instance"\s*\}/)
    expect(notice).toContain('tauri-plugin-single-instance` 2.4.3')
    expect(notice).toContain('Apache-2.0 OR MIT')
    expect(windows).toContain('WAIT_SLICE')
    expect(windows).toContain('STARTUP_TIMEOUT')
    expect(windows).toContain('WaitForSingleObject')
    expect(windows).toContain('SendMessageTimeoutW')
    expect(windows).toContain('SMTO_ABORTIFHUNG')
    expect(windows).toContain('ACTIVATION_PAYLOAD')
    expect(windows).toContain('is_valid_copydata_activation')
    expect(windows).toContain('MAX_COPYDATA_BYTES')
    expect(windows).toContain('existing instance did not publish its IPC target in time')
  })

  it('never routes local Harness probes or bearer requests through a system proxy', async () => {
    const [lib, chat] = await Promise.all([
      readNative('src/lib.rs'),
      readNative('src/chat.rs'),
    ])

    const probe = lib.match(/fn harness_probe_client\(\)[\s\S]*?\n}\n/)
    expect(probe?.[0]).toContain('.no_proxy()')
    expect(chat.match(/fn bridge_request_client\(\)[\s\S]*?\n}\n/)?.[0]).toContain('.no_proxy()')
    expect(chat.match(/fn bridge_stream_client\(\)[\s\S]*?\n}\n/)?.[0]).toContain('.no_proxy()')
  })
})
