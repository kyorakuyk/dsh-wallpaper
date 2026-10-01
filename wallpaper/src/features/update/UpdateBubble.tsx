/**
 * 更新气泡：立绘槽位里的第二枚气泡（计划书 §四）。
 *
 * 三条硬要求都在这个文件里落地：
 *
 * 1. **两个按钮**：「下载」与「忽略」。点气泡本体不做任何升级动作，也**不存在**"没点过就算忽略"
 *    —— 只有按下「忽略」才会写那条记录（规矩 3）。按「下载」在本片是"打开发布页"（见 `useUpdate`
 *    里的 TODO），但按钮形态与状态机位置都已经按第三片的"下载 → 进度 → 安装"定好；
 * 2. **点击必须 `stopPropagation`**：气泡挂在 `.portrait-slot` 里，而槽位自己的 `onClick` 是
 *    "进入里桌面/打开对话"—— 不拦住，点一下按钮会顺带开一次对话（§四 明确要求）；
 * 3. **带 `data-interaction-region`**：壁纸窗口只在热区里收鼠标消息（`runtime/interactionRegions.ts`），
 *    没有这个属性，按钮在真机上就是"点了没反应"，而浏览器预览里一切正常。
 *
 * 文案全部是词条（`useLanguage()` 订阅语言变化，多数组件已有这一行，这里也必须有 —— 它渲染
 * `t()`）。
 */
import { useLanguage, formatMessage, t, type Message } from '../../i18n/index.ts'
import type { PersonaTheme } from '../../persona/types.ts'
import { bubbleThemeStyle } from '../../ui/Bubble.tsx'
import type { UpdateOffer } from './updateState.ts'
import './UpdateBubble.css'

export interface UpdateBubbleProps {
  offer: UpdateOffer
  theme: PersonaTheme
  /** 有原生调用在飞：两个按钮一起禁用，免得一次点击变成两次请求。 */
  busy?: boolean
  /** 下面那行提示（忽略没落盘、打开发布页失败）。没有就不占地方。 */
  notice?: Message
  /** 主按钮：本片是"打开发布页"（有资产时标签是「下载」，没有时是「打开发布页」）。 */
  onDownload: () => void
  /** 「忽略」：记下这个版本，之后不再提示它（更晚的版本仍会提示）。 */
  onDismiss: () => void
}

export function UpdateBubble({ offer, theme, busy = false, notice, onDownload, onDismiss }: UpdateBubbleProps) {
  // 这一枚气泡里的每一句话都是词条（正文、两个按钮、提示行）。
  useLanguage()
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
        {t('update.outcome.available', { version: offer.version })}
      </span>
      {notice && <span className="update-bubble__notice" role="status">{formatMessage(notice)}</span>}
      <span className="update-bubble__actions">
        <button type="button" className="update-bubble__action" disabled={busy} onClick={onDownload}>
          {offer.asset ? t('update.action.download') : t('update.action.release-page')}
        </button>
        <button type="button" className="update-bubble__action update-bubble__action--quiet" disabled={busy} onClick={onDismiss}>
          {t('update.action.dismiss')}
        </button>
      </span>
    </div>
  )
}
