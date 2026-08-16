import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { cp, mkdir, rm } from 'node:fs/promises'
import { resolve } from 'node:path'

const runtimeAssets = [
  'personas/portrait-black-adult.png',
  'personas/portrait-black-child.png',
  'personas/portrait-blue-adult.png',
  'personas/portrait-blue-child.png',
  'personas/wake.jpg',
  'personas/deepsea-bg/bg-cand1.png',
  'personas/deepsea-bg/bg-cand2.png',
  'personas/deepsea-bg/bg-cand3.png',
  'personas/wake-frames/variant-anima/sleep.png',
  'personas/wake-frames/variant-anima/frame-2-eyes.png',
  'personas/wake-frames/variant-anima/frame-3-situp.png',
  'personas/wake-frames/variant-anima/frame-4-yawn.png',
]

function runtimeAssetsOnly() {
  return {
    name: 'runtime-assets-only',
    apply: 'build' as const,
    async closeBundle() {
      const root = resolve(import.meta.dirname)
      await rm(resolve(root, 'dist/personas'), { recursive: true, force: true })
      for (const asset of runtimeAssets) {
        const destination = resolve(root, 'dist', asset)
        await mkdir(resolve(destination, '..'), { recursive: true })
        await cp(resolve(root, 'public', asset), destination)
      }
    },
  }
}

// 壁纸前端构建配置：
// - base '/' 让产物用绝对路径（Tauri tauri://localhost 下相对路径 './' 解析有问题）
// - 全屏沉浸式，无边框
export default defineConfig({
  base: '/',
  // Keep the curated built-in assets available during `tauri dev`. The build
  // plugin below still replaces dist/personas with the explicit runtime list,
  // so candidate/source artwork is not shipped in release bundles.
  publicDir: 'public',
  plugins: [react(), runtimeAssetsOnly()],
  build: {
    outDir: 'dist',
    target: 'es2020',
    sourcemap: false,
  },
  server: {
    port: 5187,
    strictPort: true,
    host: '127.0.0.1',
    // Windows 下编辑工具的临时文件/Tauri 构建产物会触发 EBUSY 崩溃，忽略它们
    watch: {
      ignored: ['**/*.tmp', '**/.tmpdir/**', '**/*.tsx.*', '**/src-tauri/**', '**/target/**'],
    },
  },
})
