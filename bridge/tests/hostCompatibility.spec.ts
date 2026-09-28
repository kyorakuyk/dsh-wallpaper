import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * 宿主会**拒绝** peer 范围不覆盖自己版本的插件：0.2.0-rc.1 的 DSH 直接把桥跳过，
 * 原话是 `skipping profile bundle "dsh-wallpaper-bridge": … incompatible with dsh
 * 0.2.0-rc.1`，于是桥的路由从未注册，壁纸连不上（2026-09-28 的实况）。
 *
 * 这个测试把"范围必须覆盖到哪一代"钉住，免得以后有人顺手收窄：
 *
 *  - 同一 minor 元组的预发布版（0.2.0-rc.2）与整个 0.2.x **必须**被接受 —— 这是 semver
 *    的既有语义，不需要为新 rc 改代码；
 *  - 换 minor 元组（0.3.x）**必须**被拒绝 —— 那是一次需要人确认的适配，不该靠范围自动放行。
 *
 * ⚠ 这不是权威判据。实测反例：`0.1.7-rc.2` 在严格 semver 下**不满足** `^0.1.0-rc.5`
 * （预发布版只匹配同一元组上的比较子），可它当时照样被宿主加载了 —— 说明宿主自己的检查比严格
 * semver 宽松。因此"新 rc 能不能跑"最终由 `tests/realDshSmoke.spec.ts`（真宿主 + 真 profile +
 * 本仓库的桥构建）来证，这个文件只保证范围不会悄悄变窄。
 *
 * 用 semver 而不是手写比较：宿主与 npm 生态用的就是它，自己实现一遍只会引入新的语义差别。
 * 走 createRequire 是因为 semver 6 不带类型声明，而 import 一个无类型的 CJS 包会破坏 typecheck。
 */
const require_ = createRequire(import.meta.url)
const semver = require_('semver') as { satisfies(version: string, range: string): boolean }

const bridgeManifest = JSON.parse(
  readFileSync(resolve(import.meta.dirname, '..', 'package.json'), 'utf8'),
) as { peerDependencies?: Record<string, string> }

/** 宿主侧被检查的那五个包（其余 peer 是 cordis / schemastery，宿主不参与判定）。 */
const HOST_PEERS = [
  '@deepseek-ai/dsh-agent',
  '@deepseek-ai/dsh-host-webserver',
  '@deepseek-ai/dsh-llm',
  '@deepseek-ai/dsh-session',
  '@deepseek-ai/dsh-user-approval',
] as const

/** 必须接受：已经跑过的、以及同一 minor 元组上的后续版本。 */
const MUST_ACCEPT = [
  '0.1.0-rc.5', // 声明里最初验证过的
  '0.1.5',
  '0.2.0-rc.1', // 2026-09-28 实测：桥在新宿主上 bridge-ready
  '0.2.0-rc.2', // 下一次预发布：同一元组，自动覆盖
  '0.2.0',
  '0.2.9',
]

/** 必须拒绝：换 minor 元组要靠人验证，不能靠范围放行。 */
const MUST_REJECT = ['0.3.0-rc.1', '0.3.0']

describe('the host peer range', () => {
  it('declares every host peer', () => {
    for (const peer of HOST_PEERS) {
      expect(bridgeManifest.peerDependencies?.[peer], peer).toBeTruthy()
    }
  })

  it('accepts the runtimes we run on, including the next rc of the same minor', () => {
    for (const peer of HOST_PEERS) {
      const range = bridgeManifest.peerDependencies![peer]!
      for (const version of MUST_ACCEPT) {
        expect(semver.satisfies(version, range), `${peer} vs ${version} (range ${range})`).toBe(true)
      }
    }
  })

  it('refuses a new minor tuple, which is an adaptation rather than a range tweak', () => {
    for (const peer of HOST_PEERS) {
      const range = bridgeManifest.peerDependencies![peer]!
      for (const version of MUST_REJECT) {
        expect(semver.satisfies(version, range), `${peer} vs ${version} (range ${range})`).toBe(false)
      }
    }
  })

  it('records that strict semver is stricter than the host', () => {
    // 这一条不是"要求"，是实测记录：0.1.7-rc.2 曾被宿主接受，但严格 semver 判否。
    // 谁要是想用 semver 推断"新版本一定能跑"，先看这里。
    const range = bridgeManifest.peerDependencies!['@deepseek-ai/dsh-agent']!
    expect(semver.satisfies('0.1.7-rc.2', range)).toBe(false)
  })
})
