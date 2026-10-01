/**
 * 更新气泡：立绘槽位里的第二枚气泡（计划书 §四）。
 *
 * 三条硬要求都在这个文件里落地：
 *
 * 1. **两个按钮**：「下载」与「忽略」。点气泡本体不做任何升级动作，也**不存在**"没点过就算忽略"
 *    —— 只有按下「忽略」才会写那条记录（规矩 3）。第三片起主按钮按 §四 的状态说话：
 *    `available` ⇒ 「下载」、`downloading` ⇒ 进度条（§十：不提供取消）、`ready` ⇒ 「点击安装」、
 *    `failed` ⇒ 「重试」+「打开发布页」（§六 的回落）；
 * 2. **点击必须 `stopPropagation`**：气泡挂在 `.portrait-slot` 里，而槽位自己的 `onClick` 是
 *    "进入里桌面/打开对话"—— 不拦住，点一下按钮会顺带开一次对话（§四 明确要求）；
 * 3. **带 `data-interaction-region`**：壁纸窗口只在热区里收鼠标消息（`runtime/interactionRegions.ts`），
 *    没有这个属性，按钮在真机上就是"点了没反应"，而浏览器预览里一切正常。
 *
 * 这一枚气泡**自己不算状态**：`phase` 与 `download` 都是外面（`useUpdate`，源头是原生事件）给的。
 * 它只负责画：进度条画多满、正文说哪句、按钮是哪几个。
 *
 * 文案全部是词条（`useLanguage()` 订阅语言变化，多数组件已有这一行，这里也必须有 —— 它渲染
 * `t()`）。
 */
import { useLanguage, formatMessage, msg, t, type Message } from '../../i18n/index.ts'
import type { PersonaTheme } from '../../persona/types.ts'
import { bubbleThemeStyle } from '../../ui/Bubble.tsx'
import { downloadFailedMessage, downloadProgressMessage } from './updateCopy.ts'
import { downloadPercent, type UpdateDownloadState, type UpdateOffer, type UpdatePhase } from './updateState.ts'
import './UpdateBubble.css'

export interface UpdateBubbleProps {
  offer: UpdateOffer
  /** §四 的状态机（`available` / `downloading` / `ready` / `failed`）。 */
  phase: UpdatePhase
  /** 手上这一条下载事件；`downloading` / `ready` / `failed` 时才有内容。 */
  download?: UpdateDownloadState
  theme: PersonaTheme
  /** 有原生调用在飞：按钮一起禁用，免得一次点击变成两次请求。 */
  busy?: boolean
  /** 下面那行提示（忽略没落盘、调用被拒、已经交给安装程序）。没有就不占地方。 */
  notice?: Message
  /** 主按钮：`available` 是「下载」、`failed` 是「重试」、`ready` 是「点击安装」。 */
  onDownload: () => void
  /** `ready` 的主按钮：把下载好的安装包交给 Windows（§六）。 */
  onInstall: () => void
  /** 「打开发布页」：这次发布没有可安装资产、或下载失败时的回落（§3.1、§六）。 */
  onOpenReleasePage: () => void
  /** 「忽略」：记下这个版本，之后不再提示它（更晚的版本仍会提示）。 */
  onDismiss: () => void
}

/**
 * 正文那句话：按 §四 的状态说。
 *
 * `available` 用的就是设置卡片那一句（`update.outcome.available`，同一组词条）；`ready` 说"已经
 * 下好了"，`failed` 说可读的原因。
 */
function headline(phase: UpdatePhase, offer: UpdateOffer, download: UpdateDownloadState | undefined): Message {
  switch (phase) {
    case 'ready':
      return msg('update.outcome.ready', { version: offer.version })
    case 'failed':
      return downloadFailedMessage(download)
    default:
      return msg('update.outcome.available', { version: offer.version })
  }
}

/** 主按钮那句话：按状态说，不按"有没有资产"猜（没有资产时 `available` 说的才是「打开发布页」）。 */
function primaryActionLabel(phase: UpdatePhase, offer: UpdateOffer): string {
  if (phase === 'ready') return t('update.action.install')
  if (phase === 'failed') return t('update.action.retry')
  return offer.asset ? t('update.action.download') : t('update.action.release-page')
}

export function UpdateBubble({
  offer,
  phase,
  download,
  theme,
  busy = false,
  notice,
  onDownload,
  onInstall,
  onOpenReleasePage,
  onDismiss,
}: UpdateBubbleProps) {
  // 这一枚气泡里的每一句话都是词条（正文、按钮、提示行）。
  useLanguage()
  const downloading = phase === 'downloading' && download
  const percent = downloading ? downloadPercent(download) : undefined
  const progress = downloading ? downloadProgressMessage(download) : undefined
  return (
    <div
      className="bubble bubble-top bubble-update"
      data-interaction-region="update-bubble"
      // 见文件头第 2 条：不拦住，点按钮会顺带打开对话。
      onClick={(event) => event.stopPropagation()}
      style={bubbleThemeStyle(theme)}
    >
      <div className="bubble-tail" />
      <span className="update-bubble__text" title={offer.version}>
        {formatMessage(progress ?? headline(phase, offer, download))}
      </span>
      {/* 进度条（§四 的 downloading）。总大小未知时它没有 `aria-valuenow`：那是不定态，不是 0%。 */}
      {progress && (
        <span
          className="update-bubble__progress"
          data-known={percent === undefined ? 'unknown' : 'known'}
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          aria-valuetext={formatMessage(progress)}
        >
          <span
            className="update-bubble__progress-fill"
            style={percent === undefined ? undefined : { width: `${percent}%` }}
          />
        </span>
      )}
      {notice && <span className="update-bubble__notice" role="status">{formatMessage(notice)}</span>}
      {/* 下载中一个按钮都没有（§十：不提供取消；进度条就是那一段的全部内容）。 */}
      {phase !== 'downloading' && (
        <span className="update-bubble__actions">
          <button
            type="button"
            className="update-bubble__action"
            disabled={busy}
            onClick={phase === 'ready' ? onInstall : onDownload}
          >
            {primaryActionLabel(phase, offer)}
          </button>
          {phase === 'failed' && (
            <button
              type="button"
              className="update-bubble__action update-bubble__action--quiet"
              disabled={busy}
              onClick={onOpenReleasePage}
            >
              {t('update.action.release-page')}
            </button>
          )}
          {phase !== 'failed' && (
            <button
              type="button"
              className="update-bubble__action update-bubble__action--quiet"
              disabled={busy}
              onClick={onDismiss}
            >
              {t('update.action.dismiss')}
            </button>
          )}
        </span>
      )}
    </div>
  )
}
