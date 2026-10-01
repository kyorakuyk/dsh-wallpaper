/**
 * 更新检测的**码 → 句子**（计划书 §四：原生只回码与数字，文案由界面按语言说）。
 *
 * 返回的是 `Message` 而不是句子：它们进的是气泡与设置卡片，**切语言时要跟着变**。键都在
 * `i18n/zh.full.ts` / `en.full.ts` 里 —— 更新检测只属于完整版（§七：Lite 本次不做），
 * 所以这些键**不进** `.shared.ts`（Lite 产物边界由 `scripts/verify-lite-bundle.ps1` 与
 * `tests/liteI18nBoundary.spec.ts` 拦着，加错地方会当场失败）。
 *
 * 这一层刻意只有函数、没有状态：气泡与设置页拿到的是同一份报告与同一条下载事件，于是它们说的
 * 也是同一句话。
 */
import { msg, sentenceOf, type Message } from '../../i18n/index.ts'
import type { UpdateCheckReport, UpdateDownloadFailure, UpdateFailure, UpdateSkipReason } from '../../native/runtime.ts'
import { downloadPercent, formatBytes, type UpdateDownloadState } from './updateState.ts'

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

/**
 * 下载或校验失败的原因码 → 一句人话（§六：网络、磁盘、路径、校验各有各的说法）。
 *
 * 网络与 HTTP 那两句与检查共用（同一个原因不该有两套说法）；`writeFailed` 与 `diskFull` 分得很开，
 * 因为后者用户自己能解决（清一下盘），界面必须说得出这一句。
 */
export function downloadFailureMessage(failure: UpdateDownloadFailure): Message {
  switch (failure.code) {
    case 'network':
      return msg('update.failure.network')
    case 'httpStatus':
      return typeof failure.httpStatus === 'number'
        ? msg('update.failure.http-status', { status: failure.httpStatus })
        : msg('update.failure.http-status-unknown')
    case 'destinationUnavailable':
      return msg('update.download.failure.destination-unavailable')
    case 'writeFailed':
      return msg('update.download.failure.write-failed')
    case 'diskFull':
      return msg('update.download.failure.disk-full')
    case 'sizeMismatch': {
      // 校验失败的两个数字原生会一起给；缺了就只说"没对上"，**不印**一个假数字。
      const { expectedBytes, actualBytes } = failure
      return typeof expectedBytes === 'number' && typeof actualBytes === 'number'
        ? msg('update.download.failure.size-mismatch', {
            expected: formatBytes(expectedBytes),
            actual: formatBytes(actualBytes),
          })
        : msg('update.download.failure.verification')
    }
    case 'digestMismatch':
      return msg('update.download.failure.digest-mismatch')
    default:
      // 原生加了码而界面还不认识：如实说"没下成"，而不是替它挑一个原因。
      return msg('update.download.failure.unknown')
  }
}

/** §四 的 `failed` 那句（气泡正文与设置卡片共用）：可读的原因；没有原因码就只说那半句。 */
export function downloadFailedMessage(download: UpdateDownloadState | undefined): Message {
  return download?.failure
    ? msg('update.download.failed', { reason: downloadFailureMessage(download.failure) })
    : msg('update.download.failed-unstated')
}

/**
 * `downloading` 那行话（§四 的「正在下载 42%」）。
 *
 * 总大小不知道时说的是"已下载多少"，而不是一个编出来的百分比 —— 进度条那一半同理
 * （[`downloadPercent`] 返回 `undefined`）。
 */
export function downloadProgressMessage(download: UpdateDownloadState): Message {
  const percent = downloadPercent(download)
  return percent === undefined
    ? msg('update.progress.downloaded', { downloaded: formatBytes(download.downloadedBytes) })
    : msg('update.progress.percent', { percent })
}

/**
 * 一次原生调用被拒时那句人话。
 *
 * 原生拒绝时给的是一个**对象**（`UpdateCommandError` 的 `{ code }`），`String(error)` 只会得到
 * `[object Object]`；所以这里按码翻译。认不出的码（原生加了码而界面还不认识）回落到原文，而不是
 * 假装知道原因。
 */
export function updateCallMessage(error: unknown): Message {
  const code =
    typeof error === 'object' && error !== null && 'code' in error
      ? (error as { code?: unknown }).code
      : undefined
  switch (code) {
    case 'forbidden':
      return msg('update.call.forbidden')
    case 'statePathUnavailable':
      return msg('update.call.state-path-unavailable')
    case 'invalidVersion':
      return msg('update.call.invalid-version')
    case 'destinationUnavailable':
      return msg('update.call.destination-unavailable')
    case 'untrustedAssetUrl':
      return msg('update.call.untrusted-asset-url')
    case 'nothingDownloaded':
      return msg('update.call.nothing-downloaded')
    case 'installerMissing':
      return msg('update.call.installer-missing')
    case 'unsupportedAsset':
      return msg('update.call.unsupported-asset')
    case 'openFailed':
      return msg('update.call.open-failed')
    default:
      // 认不出的码（原生加了码而界面还不认识）：把那**个码**原样说出来，而不是挑一个原因，也不是
      // `[object Object]` —— 这一串正是排查时要 grep 的东西。
      return msg('update.call.unknown', {
        error: typeof code === 'string' ? `code=${code}` : sentenceOf(error),
      })
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
