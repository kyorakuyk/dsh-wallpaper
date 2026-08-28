import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { cp, mkdir, rename, rm } from 'node:fs/promises'
import { resolve } from 'node:path'

const fullRuntimeAssets = [
  'personas/portrait-black-adult.png',
  'personas/portrait-black-child.png',
  'personas/portrait-blue-adult.png',
  'personas/portrait-blue-child.png',
  'personas/wake.jpg',
  'personas/deepsea-bg/deepsea-studio.png',
  'personas/deepsea-bg/deepsea-dome.png',
  'personas/deepsea-bg/deepsea-study.png',
  'personas/wake-frames/variant-anima/sleep.png',
  'personas/wake-frames/variant-anima/frame-2-eyes.png',
  'personas/wake-frames/variant-anima/frame-3-situp.png',
  'personas/wake-frames/variant-anima/frame-4-yawn.png',
]

const liteRuntimeAssets = [
  'personas/portrait-black-adult.png',
  'personas/portrait-black-child.png',
  'personas/portrait-blue-adult.png',
  'personas/portrait-blue-child.png',
  'personas/deepsea-bg/deepsea-studio.png',
  'personas/deepsea-bg/deepsea-dome.png',
  'personas/deepsea-bg/deepsea-study.png',
  'personas/wake-frames/variant-anima/sleep.png',
  'personas/wake-frames/variant-anima/frame-2-eyes.png',
  'personas/wake-frames/variant-anima/frame-3-situp.png',
  'personas/wake-frames/variant-anima/frame-4-yawn.png',
]

function runtimeAssetsOnly(outDir: string, assets: string[]) {
  return {
    name: 'runtime-assets-only',
    apply: 'build' as const,
    async closeBundle() {
      const root = resolve(import.meta.dirname)
      if (outDir === 'dist-lite') {
        const liteIndex = resolve(root, outDir, 'index-lite.html')
        const publicIndex = resolve(root, outDir, 'index.html')
        await rm(publicIndex, { force: true })
        await rename(liteIndex, publicIndex)
      }
      await rm(resolve(root, outDir, 'personas'), { recursive: true, force: true })
      for (const asset of assets) {
        const destination = resolve(root, outDir, asset)
        await mkdir(resolve(destination, '..'), { recursive: true })
        await cp(resolve(root, 'public', asset), destination)
      }
    },
  }
}

function liteDevEntry(lite: boolean) {
  return {
    name: 'lite-dev-entry',
    apply: 'serve' as const,
    transformIndexHtml(html: string) {
      return lite ? html.replace('/src/main.tsx', '/src/main-lite.tsx') : html
    },
  }
}

// 壁纸前端构建配置：
// - base '/' 让产物用绝对路径（Tauri tauri://localhost 下相对路径 './' 解析有问题）
// - 全屏沉浸式，无边框
export default defineConfig(({ mode }) => {
  const lite = mode === 'lite'
  const outDir = lite ? 'dist-lite' : 'dist'
  const assets = lite ? liteRuntimeAssets : fullRuntimeAssets
  return {
    base: '/',
    // Keep the curated built-in assets available during `tauri dev`. The build
    // plugin below still replaces the output's personas directory with the
    // explicit runtime list, so candidate/source artwork is not shipped.
    publicDir: 'public',
    plugins: [liteDevEntry(lite), react(), runtimeAssetsOnly(outDir, assets)],
    build: {
      outDir,
      target: 'es2020',
      sourcemap: false,
      rollupOptions: {
        // Separate HTML entry points keep the unused edition out of the
        // release graph entirely; a transform-only switch would still emit
        // the other product as a static chunk.
        input: resolve(import.meta.dirname, lite ? 'index-lite.html' : 'index.html'),
      },
    },
    server: {
      port: lite ? 5188 : 5187,
      strictPort: true,
      host: '127.0.0.1',
      // Windows 下编辑工具的临时文件/Tauri 构建产物会触发 EBUSY 崩溃，忽略它们
      watch: {
        ignored: ['**/*.tmp', '**/.tmpdir/**', '**/*.tsx.*', '**/src-tauri/**', '**/target/**'],
      },
    },
  }
})
