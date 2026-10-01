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
import { t } from '../i18n/index.ts'
import type { HarnessAvailability } from '../connect/harness.ts'

/**
 * One sentence per non-ready state: what is missing and what fixes it.
 *
 * Read through getters rather than filling the record once at module load: the
 * language is restored from the settings document *after* startup, so a value
 * captured at import time would still be the Chinese one in an English window.
 * `bridge-ready` is the absence of a problem, so it has no sentence at all - and
 * the dictionaries do not take an empty entry, which is why that `''` stays here.
 */
export const HARNESS_STATE_DETAILS: Record<HarnessAvailability, string> = {
  get offline() { return t('harness.detail.offline') },
  get 'web-only'() { return t('harness.detail.web-only') },
  get 'bridge-loading'() { return t('harness.detail.bridge-loading') },
  get 'bridge-auth-unavailable'() { return t('harness.detail.bridge-auth-unavailable') },
  // A Bridge that is present but answers an older or narrower contract lands
  // here, which is the common "stale copy in the profile" case rather than a
  // hypothetical future version. Name the installation, not just "version
  // mismatch", so the user knows the fix is to update the Bridge plugin.
  get 'bridge-incompatible'() { return t('harness.detail.bridge-incompatible') },
  get 'bridge-ready'() { return '' },
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
  if (probing || availability === 'bridge-loading') return t('harness.label.connecting')
  switch (availability) {
    case 'bridge-ready': return t('harness.label.bridge-ready')
    case 'bridge-auth-unavailable': return t('harness.label.bridge-auth-unavailable')
    case 'bridge-incompatible': return t('harness.label.bridge-incompatible')
    case 'web-only': return t('harness.label.web-only')
    default: return t('harness.label.offline')
  }
}
