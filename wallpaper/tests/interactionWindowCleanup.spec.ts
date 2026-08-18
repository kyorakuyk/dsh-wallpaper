import { readdir, readFile } from 'node:fs/promises'
import { dirname, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function readJson(relativePath: string): Promise<unknown> {
  const source = await readFile(resolve(wallpaperRoot, relativePath), 'utf8')
  return JSON.parse(source)
}

interface RustSource {
  relativePath: string
  source: string
}

async function readRustSources(directory: string): Promise<RustSource[]> {
  const entries = await readdir(directory, { withFileTypes: true })
  const nestedSources = await Promise.all(entries.map(async (entry): Promise<RustSource[]> => {
    const path = resolve(directory, entry.name)

    if (entry.isDirectory()) return readRustSources(path)
    if (!entry.isFile() || !entry.name.endsWith('.rs')) return []

    return [{
      relativePath: relative(wallpaperRoot, path),
      source: await readFile(path, 'utf8'),
    }]
  }))

  return nestedSources.flat()
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
    const rustSources = await readRustSources(resolve(wallpaperRoot, 'src-tauri/src'))

    // `interaction` is now state and hit-region terminology inside the shared
    // background host, never a native Tauri window label. Keep the check
    // narrow enough that those valid terms remain available to Rust code.
    const interactionLabel = String.raw`(?:\"interaction\"|r(?:#*)\"interaction\"(?:#*))`
    const interactionWindowLookup = new RegExp(
      String.raw`\b(?:get_webview_window|get_window)\s*\(\s*${interactionLabel}\s*\)`,
    )
    const interactionWindowBuilder = new RegExp(
      String.raw`\b(?:WebviewWindowBuilder|WindowBuilder)\s*::\s*new\s*\(\s*[^,]+,\s*${interactionLabel}\s*,`,
    )
    const interactionLabelConstant = new RegExp(
      String.raw`\b(?:const|static)\s+\w*INTERACTION\w*[^=;]*=\s*${interactionLabel}\s*;`,
      'i',
    )

    expect(rustSources).not.toHaveLength(0)
    for (const { relativePath, source } of rustSources) {
      expect(source, relativePath).not.toMatch(interactionWindowLookup)
      expect(source, relativePath).not.toMatch(interactionWindowBuilder)
      expect(source, relativePath).not.toMatch(interactionLabelConstant)
    }
  })

  it('does not retain the obsolete separate ChatWindow scene', async () => {
    await expect(readFile(resolve(wallpaperRoot, 'src/scenes/ChatWindow.tsx'), 'utf8'))
      .rejects
      .toMatchObject({ code: 'ENOENT' })
  })
})
