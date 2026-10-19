import { formatMessage, t, type Message } from '../../i18n/index.ts'
import { downloadPercent, offeredUpdate, updatePhase } from '../../features/update/updateState.ts'
import { downloadFailedMessage, downloadProgressMessage, updateOutcomeMessage } from '../../features/update/updateCopy.ts'
import { formatHistoryTime } from './HistoryTab.tsx'
import { autostartDetail } from '../autostartCopy.ts'
import { Card, Field, Toggle } from '../components/SettingsControls.tsx'
import type { SettingsPanelProps } from '../SettingsPanel.tsx'

interface SystemTabProps extends SettingsPanelProps {
  set: (patch: Partial<SettingsPanelProps['settings']>) => void
  clearDetail: Message[] | undefined
  clearing: boolean
  clearUserData: () => Promise<void>
}

export function SystemTab(props: SystemTabProps) {
  const { settings, set, clearDetail, clearing, clearUserData } = props
  /**
   * 更新卡片上那几个派生值（计划书 §四）。
   *
   * 都在这儿算，是因为它们**要在渲染期求值**：`updateOutcomeMessage`/`downloadProgressMessage`
   * 返回的是没求值的 `Message`（切语言时整句跟着变），而时间戳那句沿用了历史页那个"当天只写时刻"
   * 的格式。
   *
   * `updateNow` 是**与气泡同一个状态机**（`updatePhase`）：报告给前三个状态、全局的下载事件给
   * 后三个，所以卡片与气泡不会各说各的。
   */
  const updateReport = props.updateReport
  const updateOffer = offeredUpdate(updateReport)
  const updateNow = updatePhase(updateReport, props.updateDownload)
  const updateCurrentVersion = updateReport
    ? updateReport.currentVersion ?? t('settings.system.update.current.unavailable')
    : t('settings.system.reading')
  const updateLastChecked = updateReport?.checkedAtMs
    ? formatHistoryTime(updateReport.checkedAtMs)
    : t('settings.system.update.last-check.never')

  return <>
        {/* 更新（计划书 §四）：当前版本、上次检查（时间 + 结论）、手动「检查更新」；有可用更新时
            同一张卡片上给出「下载」与「忽略」—— 与立绘气泡同一状态机、同一份原生状态。 */}
        <Card title={t('settings.system.update.title')} description={t('settings.system.update.description')}>
          <Field title={t('settings.system.update.current.title')}>
            <span className="settings-static">{updateCurrentVersion}</span>
          </Field>
          <Field
            title={t('settings.system.update.last-check.title')}
            detail={updateReport
              ? `${updateLastChecked} · ${formatMessage(updateOutcomeMessage(updateReport))}`
              : t('settings.system.reading')}
          >
            <span className="integration-actions">
              <button className="settings-action" disabled={props.updateBusy} onClick={props.onCheckForUpdates}>
                {props.updateBusy ? t('settings.system.update.checking') : t('settings.system.update.check')}
              </button>
              {updateOffer && <>
                {/* 与立绘气泡同一个状态机（`updateNow`）与同一条下载事件：这里给的是同样的动作。
                    `downloading` 只有进度（§十：不提供取消），`ready` 是「点击安装」，
                    `failed` 是「重试」+「打开发布页」（§六 的回落）。 */}
                {updateNow === 'downloading' && props.updateDownload
                  ? <span className="settings-static" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={downloadPercent(props.updateDownload)} aria-valuetext={formatMessage(downloadProgressMessage(props.updateDownload))}>
                      {formatMessage(downloadProgressMessage(props.updateDownload))}
                    </span>
                  : <>
                      <button className="settings-action secondary" disabled={props.updateBusy} onClick={updateNow === 'ready' ? props.onInstallUpdate : props.onDownloadUpdate}>
                        {updateNow === 'ready'
                          ? t('update.action.install')
                          : updateNow === 'failed'
                            ? t('update.action.retry')
                            : updateOffer.asset ? t('update.action.download') : t('update.action.release-page')}
                      </button>
                      {(updateNow === 'failed' || !updateOffer.asset) && (
                        <button className="settings-action secondary" disabled={props.updateBusy} onClick={props.onOpenUpdatePage}>
                          {t('update.action.release-page')}
                        </button>
                      )}
                      {updateNow !== 'failed' && (
                        <button className="settings-action secondary" disabled={props.updateBusy} onClick={() => props.onDismissUpdate(updateOffer.version)}>
                          {t('update.action.dismiss')}
                        </button>
                      )}
                    </>}
              </>}
            </span>
          </Field>
          {/* 失败原因就显示在卡片上（§四 的 `failed`、§八 7：断网、资产缺失、校验不符都要看得见）。 */}
          {updateNow === 'failed' && <p className="settings-hint" role="status">{formatMessage(downloadFailedMessage(props.updateDownload))}</p>}
          {props.updateNotice && <p className="settings-hint" role="status">{formatMessage(props.updateNotice)}</p>}
        </Card>
        <Card title={t('settings.system.data.title')} description={t('settings.system.data.description')}>
          <Field title={t('settings.system.workspace.title')} detail={props.desktopWorkspace ? (props.desktopWorkspace.workspaceExists ? props.desktopWorkspace.workspaceDirectory : props.desktopWorkspace.workspaceDirectory + t('settings.system.workspace.missing')) : t('settings.system.reading')}><span /></Field>
          {/* 卸载时问不了（MSIX 没有自定义卸载界面），所以"想清干净的时候能清干净"这个入口放在这里。
              原生只做它能证明做完的两件：工作区目录 + 凭据管理器里那条 Key；WebView2 配置目录正被
              运行中的进程占用，删不干净，所以如实回报路径让用户退出后自己删。 */}
          <Field title={t('settings.system.memory.title')} detail={props.desktopWorkspace ? props.desktopWorkspace.memoryFile + (props.desktopWorkspace.memoryExists ? t('settings.system.memory.present') : t('settings.system.memory.absent')) : t('settings.system.reading')}>
            <button
              className="settings-action secondary"
              disabled={props.openingMemory}
              onClick={() => { void props.onOpenProjectMemory?.() }}
            >{props.openingMemory ? t('settings.system.memory.opening') : t('settings.system.memory.open')}</button>
          </Field>
          <Field title={t('settings.system.clear.title')} detail={clearDetail ? clearDetail.map(formatMessage).join(t('settings.system.clear.join')) : t('settings.system.clear.detail')}>
            <button
              className="settings-action secondary"
              disabled={clearing}
              onClick={() => { void clearUserData() }}
            >{clearing ? t('settings.system.clear.clearing') : t('settings.system.clear.action')}</button>
          </Field>
        </Card>
        <Card title={t('settings.system.windows.title')}>
          <Field title={t('settings.system.autostart.title')} detail={props.autostartBusy ? t('settings.system.autostart.busy') : autostartDetail(props.autostart)}><Toggle label={t('settings.system.autostart.toggle')} checked={settings.autostart} onChange={(value) => set({ autostart: value })} disabled={props.autostartBusy} /></Field>
        </Card>
  </>
}
