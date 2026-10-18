import { t } from '../../i18n/index.ts'
import { Card } from '../components/SettingsControls.tsx'
import type { SettingsPanelProps } from '../SettingsPanel.tsx'

/** `18.4 MB`-style size for the history rows. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${Math.round(bytes)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/**
 * Local timestamp for one history row; `—` when the value is unknown.
 *
 * 同一天那半句是词条（`今天 09:05`），比较早的那些直接写完整日期 —— 那是一串数字格式，不是句子。
 */
export function formatHistoryTime(timestamp: number, now: Date = new Date()): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '—'
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return '—'
  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate()
  const time = `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
  return sameDay
    ? t('settings.history.today', { time })
    : `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')} ${time}`
}

/**
 * How close the archive is to the ceiling that stops persistence, expressed as
 * a percentage of the budget the application trims to. The UI must never imply
 * "full" at exactly the budget: trimming happens silently at that point, and
 * only the hard ceiling causes a refusal.
 */
export function historyPressure(totalBytes: number, budgetBytes: number): number {
  if (!Number.isFinite(totalBytes) || !Number.isFinite(budgetBytes) || budgetBytes <= 0) return 0
  return Math.max(0, Math.min(100, Math.round(totalBytes / budgetBytes * 100)))
}

export function HistoryTab(props: SettingsPanelProps) {
  return <>
        <Card
          title={t('settings.history.title')}
          description={t('settings.history.description')}
        >
          <div className="asset-library-toolbar">
            <span>
              {t('settings.history.summary', {
                conversations: props.apiHistory?.conversations.length ?? 0,
                messages: props.apiHistory?.totalMessages ?? 0,
                size: formatBytes(props.apiHistory?.totalBytes ?? 0),
              })}
              {props.apiHistory && props.apiHistory.budgetBytes > 0
                ? t('settings.history.budget', { size: formatBytes(props.apiHistory.budgetBytes) })
                : ''}
            </span>
            <span className="integration-actions">
              <button className="settings-action secondary" disabled={props.apiHistoryBusy} onClick={props.onRefreshApiHistory}>
                {props.apiHistoryBusy ? t('settings.history.refreshing') : t('settings.refresh')}
              </button>
              <button
                className="settings-action secondary"
                disabled={props.apiHistoryBusy || (props.apiHistory?.conversations.length ?? 0) === 0}
                onClick={props.onClearApiHistory}
              >
                {t('settings.history.clear')}
              </button>
            </span>
          </div>

          {props.apiHistory && props.apiHistory.budgetBytes > 0 && <div className="history-usage" role="presentation">
            <div className="history-usage__bar"><i style={{ width: `${historyPressure(props.apiHistory.totalBytes, props.apiHistory.budgetBytes)}%` }} /></div>
            <small>
              {t('settings.history.usage', {
                percent: historyPressure(props.apiHistory.totalBytes, props.apiHistory.budgetBytes),
                limit: formatBytes(props.apiHistory.maxBytes),
              })}
            </small>
          </div>}

          {props.apiHistory === undefined && <div className="settings-empty"><strong>{t('settings.history.loading.title')}</strong><span>{t('settings.history.loading.detail')}</span></div>}

          {props.apiHistory?.conversations.length === 0 && <div className="settings-empty">
            <strong>{t('settings.history.empty.title')}</strong>
            <span>{t('settings.history.empty.detail')}</span>
          </div>}

          {(props.apiHistory?.conversations.length ?? 0) > 0 && <div className="history-list">
            {props.apiHistory?.conversations.map((conversation) => <div className="history-row" key={conversation.id}>
              <span className="history-row__main">
                <strong title={conversation.id}>
                  {conversation.id}
                  {conversation.active && <i className="history-row__badge">{t('settings.history.active')}</i>}
                </strong>
                <small>
                  {t('settings.history.row-meta', { messages: conversation.messageCount, size: formatBytes(conversation.bytes), time: formatHistoryTime(conversation.lastMessageAt) })}
                </small>
              </span>
              <button
                className="settings-action secondary history-row__delete"
                disabled={props.apiHistoryBusy}
                aria-label={t('settings.history.row-delete', { id: conversation.id })}
                onClick={() => props.onDeleteApiConversation(conversation.id)}
              >
                {t('settings.history.delete')}
              </button>
            </div>)}
          </div>}
        </Card>
  </>
}
