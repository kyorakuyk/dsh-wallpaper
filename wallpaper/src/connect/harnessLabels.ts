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

/**
 * 失败标记该不该显示。
 *
 * `harnessFailed` 只在**启动握手失败**时置真，而清除它的那条 effect 开头就有前置条件（"握手中"），
 * 而失败时那个标记已经被清掉了 —— 于是这条清除路径此后再也不执行。实测症状：上一次尝试失败后标记
 * 粘住，等桥由常驻监视器自己连上来时没人清它，界面上就是**绿灯配"连接失败"**。
 *
 * 已就绪时一律不显示失败：灯的绿是当下的事实，失败标记只是对上一次尝试的记录。
 */
export function harnessFailureVisible(
  failed: boolean | undefined,
  availability: HarnessAvailability,
): boolean {
  return failed === true && availability !== 'bridge-ready'
}

/** Short label for the status dot in the bubble and the settings sidebar. */
export function harnessStateLabel(
  state: HarnessAvailability | { availability: HarnessAvailability; probing?: boolean },
): string {
  const availability = typeof state === 'string' ? state : state.availability
  const probing = typeof state === 'string' ? false : state.probing === true
  // 黄灯只有一种含义：**还没定**。所以凡是黄灯（正在连、正在装载、就绪过的桥接暂时失联还没判死）
  // 一律说"连接中"，绝不在呼吸着的同时写着"已连接" —— 那句话属于上一条连接，用户读到的却是
  // "能用了"，于是发消息才发现会话根本没建立。这是实测过的症状，不是假想。
  if (probing || availability === 'bridge-loading') return '连接中'
  switch (availability) {
    case 'bridge-ready': return 'DSH Bridge 已连接'
    case 'bridge-auth-unavailable': return 'DSH Bridge 令牌不可用'
    case 'bridge-incompatible': return 'DSH Bridge 版本不兼容'
    case 'web-only': return 'DSH 在线，缺少 Bridge'
    default: return 'DSH 当前离线'
  }
}
