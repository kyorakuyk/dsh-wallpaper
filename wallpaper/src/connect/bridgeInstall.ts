/**
 * 把"装桥"的原生结果变成用户能读的一句话。
 *
 * 这里刻意是**纯函数**：三种语气与它们的优先级是这个功能里唯一会被用户看见的决策，
 * 所以它值得被测，而不是散在界面的三元表达式里。
 *
 * 优先级：**失败 > 需要确认 > 装好了**。一次装多个档案时，最坏的那条决定语气 ——
 * 一次失败不该被另一条成功掩盖。
 */

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
  text: string
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
    const detail = clip(first.detail) || '没有更多信息'
    return {
      tone: 'error',
      text: `装桥失败（${first.profile}）：${detail}`,
    }
  }

  const needsConfirmation = outcomes.filter((outcome) => outcome.status === 'needs-confirmation')
  if (needsConfirmation.length > 0) {
    // 这一步**只能由用户做**：DSH 要求不兼容插件由人明确确认精确版本豁免。
    const first = needsConfirmation[0]
    return {
      tone: 'attention',
      text: `桥已装好，但 ${first.profile} 里的插件需要你确认版本豁免：在终端执行 dsh plugin allow-version（原文：${clip(first.detail)}）`,
    }
  }

  return {
    tone: 'ok',
    text: `已为 ${profileNames(outcomes)} 装好桥。`,
  }
}
