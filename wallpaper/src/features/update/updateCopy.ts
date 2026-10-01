/**
 * 更新检测的**码 → 句子**（计划书 §四：原生只回码与数字，文案由界面按语言说）。
 *
 * 返回的是 `Message` 而不是句子：它们进的是气泡与设置卡片，**切语言时要跟着变**。键都在
 * `i18n/zh.full.ts` / `en.full.ts` 里 —— 更新检测只属于完整版（§七：Lite 本次不做），
 * 所以这些键**不进** `.shared.ts`（Lite 产物边界由 `scripts/verify-lite-bundle.ps1` 与
 * `tests/liteI18nBoundary.spec.ts` 拦着，加错地方会当场失败）。
 *
 * 这一层刻意只有函数、没有状态：气泡与设置页拿到的是同一份报告，于是它们说的也是同一句话。
 */
import { msg, type Message } from '../../i18n/index.ts'
import type { UpdateCheckReport, UpdateFailure, UpdateSkipReason } from '../../native/runtime.ts'

/** 检查失败的原因码 → 一句人话（§八 7：断网、资产缺失、被拒都要能看见原因）。 */
export function updateFailureMessage(failure: UpdateFailure): Message {
  switch (failure.code) {
    case 'network':
      return msg('update.failure.network')
    case 'httpStatus':
      // `httpStatus` 是随码一起来的那个数字；缺了就说"服务器给了个错误状态"，
      // 而不是印一个假的状态码。
      return typeof failure.httpStatus === 'number'
        ? msg('update.failure.http-status', { status: failure.httpStatus })
        : msg('update.failure.http-status-unknown')
    case 'malformedResponse':
      return msg('update.failure.malformed-response')
  }
}

/** 没检查的原因码 → 一句人话。 */
export function updateSkipMessage(reason: UpdateSkipReason): Message {
  switch (reason) {
    case 'throttled':
      return msg('update.skip.throttled')
    case 'versionUnavailable':
      return msg('update.skip.version-unavailable')
  }
}

/**
 * 一次检查的结论 → 一句人话（设置卡片上那行「上次结果」）。
 *
 * 五种结论各有说法，而**跳过与失败都把原因嵌在句子里**（`Message` 的参数可以是一条 `Message`，
 * 渲染期按当前语言递归求值）：所以切到英文之后，整句连原因一起换语言。
 *
 * 报告说"跳过了"却没给原因（原生不该出现）时，说的就是"没有检查"这句本身 —— **不替它挑一个
 * 原因**：那会是一句看起来很像事实的假话。
 */
export function updateOutcomeMessage(report: UpdateCheckReport): Message {
  const version = report.latestVersion?.trim()
  switch (report.outcome) {
    case 'updateAvailable':
      return version ? msg('update.outcome.available', { version }) : msg('update.outcome.available-unversioned')
    case 'upToDate':
      return msg('update.outcome.up-to-date')
    case 'noInstallableAsset':
      return version ? msg('update.outcome.no-asset', { version }) : msg('update.outcome.available-unversioned')
    case 'skipped':
      return report.skipReason
        ? msg('update.outcome.skipped', { reason: updateSkipMessage(report.skipReason) })
        : msg('update.outcome.skipped-unstated')
    case 'failed':
      return report.failure
        ? msg('update.outcome.failed', { reason: updateFailureMessage(report.failure) })
        : msg('update.outcome.failed-unstated')
  }
}
