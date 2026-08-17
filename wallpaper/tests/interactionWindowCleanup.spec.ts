import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function readJson(relativePath: string): Promise<unknown> {
  const source = await readFile(resolve(wallpaperRoot, relativePath), 'utf8')
  return JSON.parse(source)
}

describe('legacy interaction window cleanup', () => {
  it('keeps the Tauri window configuration limited to the background host and settings', async () => {
    const config = await readJson('src-tauri/tauri.conf.json') as {
      app?: { windows?: Array<{ label?: unknown }> }
    }

    const labels = config.app?.windows?.map((window) => window.label)
    expect(labels).toStrictEqual(['background', 'settings'])
    expect(labels).not.toContain('interaction')
  })

  it('keeps the default capability scoped to the remaining native windows', async () => {
    const capability = await readJson('src-tauri/capabilities/default.json') as { windows?: unknown }

    expect(capability.windows).toStrictEqual(['background', 'settings'])
    expect(capability.windows).not.toContain('interaction')
  })

  it('does not recreate or look up an independent interaction WebView window', async () => {
    const rustSources = await Promise.all([
      readFile(resolve(wallpaperRoot, 'src-tauri/src/windows_integration.rs'), 'utf8'),
      readFile(resolve(wallpaperRoot, 'src-tauri/src/lib.rs'), 'utf8'),
    ])

    for (const source of rustSources) {
      expect(source).not.toMatch(/\bget_webview_window\s*\(\s*"interaction"\s*\)/)
      expect(source).not.toMatch(/\bWebviewWindowBuilder\s*::\s*new\s*\(\s*[^,]+,\s*"interaction"\s*,/)
    }
  })
})
