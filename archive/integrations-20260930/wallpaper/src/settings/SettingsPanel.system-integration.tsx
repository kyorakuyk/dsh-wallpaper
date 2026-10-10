// 归档片段（B4，2026-10）：`wallpaper/src/settings/SettingsPanel.tsx` 中系统集成（1B：TranslucentTB / 登录过渡底图）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/integrations-20260930/README.md` 的 B4 小节。

// ---- 原 94-98 行：`SettingsPanelProps` 内，`onSetInteractionEnabled` 之后 ----
  // FREEZE(1B)：系统集成暂时只留开机自启（2026-09-30），透明任务栏这块随之冻结。恢复办法：取消注释。
  translucentTb: { installed: boolean; running: boolean; source?: string }
  onRefreshTranslucentTb: () => void
  onLaunchTranslucentTb: () => void
  onInstallTranslucentTb: () => void

// ---- 原 583-584 行：`SettingsPanel` 函数体第一行（现役解构 `const { settings, harnessStatus, onChange, onClose, page } = props` 之前） ----
  // FREEZE(1B)：解构里去掉已冻结的 translucentTb。
  const { settings, harnessStatus, onChange, onClose, translucentTb, page } = props

// ---- 原 1292-1296 行：系统页，`settings.system.windows.title` 卡片之后、`</>}` 之前 ----
        {/* FREEZE(1B)：透明任务栏卡片随系统集成一起冻结。恢复办法：去掉这对注释，并恢复接口与传参。 */}
        <Card title="透明任务栏" description="通过松耦合方式连接独立安装的 TranslucentTB，本应用不会修改其配置。">
          <div className="integration-status"><div><i className={translucentTb.running ? 'is-online' : ''} /><span><strong>{translucentTb.running ? 'TranslucentTB 正在运行' : translucentTb.installed ? 'TranslucentTB 已安装' : 'TranslucentTB 未安装'}</strong><small>{translucentTb.source ?? '由用户独立安装和管理'}</small></span></div><div className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshTranslucentTb}>刷新</button><button className="settings-action" onClick={translucentTb.installed ? props.onLaunchTranslucentTb : props.onInstallTranslucentTb}>{translucentTb.installed ? '启动' : '前往商店'}</button></div></div>
        </Card>
