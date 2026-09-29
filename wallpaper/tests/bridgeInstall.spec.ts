import { describe, expect, it } from 'vitest'
import { summarizeBridgeInstall } from '../src/connect/bridgeInstall.ts'

function outcome(profile: string, status: string, detail = ''): {
  profile: string
  status: string
  detail: string
  command: string
} {
  return { profile, status, detail, command: `dsh plugin --profile ${profile} add dsh-wallpaper-bridge@0.1.5` }
}

describe('bridge install summary', () => {
  it('says nothing when the native side had nothing to do', () => {
    expect(summarizeBridgeInstall([])).toBeNull()
    // 那个"要不要说给用户听"的函数是废话，已删除；走到这里就代表原生侧确实做过事。
  })

  it('says nothing when every profile already had the pinned version', () => {
    // 原生侧在这种情况下连包管理都不跑，界面也不该每次都报一遍。
    expect(summarizeBridgeInstall([outcome('web', 'already-present')])).toBeNull()
  })

  it('names every profile it installed into', () => {
    const summary = summarizeBridgeInstall([outcome('web', 'installed'), outcome('desktop', 'installed')])
    expect(summary?.tone).toBe('ok')
    expect(summary?.text).toContain('「web」')
    expect(summary?.text).toContain('「desktop」')
  })

  it('hands a version exemption to the user instead of reporting success', () => {
    const summary = summarizeBridgeInstall([
      outcome('web', 'installed'),
      outcome('desktop', 'needs-confirmation', 'is incompatible with dsh 0.2.0-rc.1 … dsh plugin allow-version'),
    ])
    expect(summary?.tone).toBe('attention')
    expect(summary?.text).toContain('allow-version')
    expect(summary?.text).toContain('desktop')
  })

  it('lets a failure outrank a success, and keeps the reason', () => {
    const summary = summarizeBridgeInstall([
      outcome('web', 'installed'),
      outcome('desktop', 'failed', 'ERR_PNPM_FETCH_404 未找到该版本'),
    ])
    expect(summary?.tone).toBe('error')
    expect(summary?.text).toContain('ERR_PNPM_FETCH_404')
    expect(summary?.text).toContain('desktop')
  })

  it('clips a long log and collapses its newlines before it reaches the surface', () => {
    const long = 'x'.repeat(400) + '\n\n第二个段落'
    const summary = summarizeBridgeInstall([outcome('web', 'failed', long)])
    expect(summary?.text).not.toContain('\n')
    expect((summary?.text ?? '').length).toBeLessThan(400)
    expect(summary?.text).toContain('…')
  })
})
