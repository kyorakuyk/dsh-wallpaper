/**
 * 把"装桥"的原生结果变成用户能读的一句话。
 *
 * 这里刻意是**纯函数**：三种语气与它们的优先级是这个功能里唯一会被用户看见的决策，
 * 所以它值得被测，而不是散在界面的三元表达式里。
 *
 * 优先级：**失败 > 需要确认 > 装好了**。一次装多个档案时，最坏的那条决定语气 ——
 * 一次失败不该被另一条成功掩盖。
 */

import { msg, type Message } from '../i18n/index.ts'

/** 与 Rust 侧 `BridgeInstallOutcome` 字段一一对应（都是单词，不涉及重命名）。 */
export interface BridgeInstallOutcome {
  profile: string
  /** `installed` / `needs-confirmation` / `failed` */
  status: string
  detail: string
  command: string
}

export interface BridgeInstallSummary {
  tone: 'ok' | 'attention' | 'error'
  /**
   * 要显示的那句话，**没求值**（`Message`）：它会进设置窗口的通知状态，而语言可能在它挂着的时候
   * 换过。`detail` 是原生包管理器的自由文本（本批不翻译），它是这句话的一个参数。
   */
  text: Message
}

/** 原文太长时截断，界面不该被包管理日志淹没。 */
const MAX_DETAIL_CHARS = 160

function clip(value: string): string {
  const text = value.trim().replace(/\s+/g, ' ')
  return text.length <= MAX_DETAIL_CHARS ? text : `${text.slice(0, MAX_DETAIL_CHARS)}…`
}

function profileNames(outcomes: BridgeInstallOutcome[]): string {
  return outcomes.map((outcome) => `「${outcome.profile}」`).join('、')
}

export function summarizeBridgeInstall(outcomes: BridgeInstallOutcome[]): BridgeInstallSummary | null {
  if (outcomes.length === 0) return null

  const failed = outcomes.filter((outcome) => outcome.status === 'failed')
  if (failed.length > 0) {
    const first = failed[0]
    // `detail` 是**原生**的自由文本（包管理器的原话），没得译；"还没有细节"那句才是我们的词条。
    const detail = clip(first.detail) || msg('connect.bridge.no-detail')
    return {
      tone: 'error',
      text: msg('connect.bridge.failed', { profile: first.profile, detail }),
    }
  }

  const needsConfirmation = outcomes.filter((outcome) => outcome.status === 'needs-confirmation')
  if (needsConfirmation.length > 0) {
    // 这一步**只能由用户做**：DSH 要求不兼容插件由人明确确认精确版本豁免。
    const first = needsConfirmation[0]
    return {
      tone: 'attention',
      text: msg('connect.bridge.needs-confirmation', { profile: first.profile, detail: clip(first.detail) }),
    }
  }

  // 全部"已经是这一版"时（原生侧连包管理都没跑）不出一声：这是每次启动的常态。
  const installed = outcomes.filter((outcome) => outcome.status === 'installed')
  if (installed.length === 0) return null
  return {
    tone: 'ok',
    text: msg('connect.bridge.installed', { profiles: profileNames(outcomes) }),
  }
}
