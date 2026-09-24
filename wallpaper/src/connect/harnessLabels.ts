/**
 * User-facing wording for every Harness state.
 *
 * Kept out of `connect/harness.ts` (which `domain/types.ts` imports) to avoid a
 * cycle, and out of `App.tsx` so the composer notice, the settings sidebar and
 * the bubble badge cannot drift into three different explanations of one
 * failure.
 *
 * Nothing here may carry a token, a filesystem path, or a raw exception.
 */
import type { HarnessAvailability } from '../connect/harness.ts'

/** One sentence per non-ready state: what is missing and what fixes it. */
export const HARNESS_STATE_DETAILS: Record<HarnessAvailability, string> = {
  offline: '未能连接到本机的 DSH 壁纸 Bridge。',
  'web-only': '检测到 DSH 服务，但壁纸 Bridge 未安装、未启动或不兼容。',
  'bridge-loading': 'DSH 壁纸 Bridge 已启动，正在装载会话服务。',
  'bridge-auth-unavailable': 'DSH 壁纸 Bridge 已启动，但本机访问令牌不可用，请重启壁纸应用。',
  // A Bridge that is present but answers an older or narrower contract lands
  // here, which is the common "stale copy in the profile" case rather than a
  // hypothetical future version. Name the installation, not just "version
  // mismatch", so the user knows the fix is to update the Bridge plugin.
  'bridge-incompatible': 'DSH 壁纸 Bridge 的版本或能力与本壁纸不兼容（profile 内可能是过旧的副本），请更新 Bridge 后重试。',
  'bridge-ready': '',
}

/** Short label for the status dot in the bubble and the settings sidebar. */
export function harnessStateLabel(availability: HarnessAvailability): string {
  switch (availability) {
    case 'bridge-ready': return 'DSH Bridge 已连接'
    case 'bridge-loading': return 'DSH Bridge 正在装载'
    case 'bridge-auth-unavailable': return 'DSH Bridge 令牌不可用'
    case 'bridge-incompatible': return 'DSH Bridge 版本不兼容'
    case 'web-only': return 'DSH 在线，缺少 Bridge'
    default: return 'DSH 当前离线'
  }
}
