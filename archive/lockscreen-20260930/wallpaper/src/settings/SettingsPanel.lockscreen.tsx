// 归档片段（B4，2026-10）：`wallpaper/src/settings/SettingsPanel.tsx` 中锁屏（1A）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/lockscreen-20260930/README.md` 的 B4 小节。

// ---- 原 11-12 行：import 区，`appearance/theme` 之后。恢复时用它替换现役的 `native/runtime.ts` 类型 import，并补回现役行里有、这行没有的 `UpdateCheckReport` ----
// FREEZE(1A)：锁屏退出，LockScreenDiagnostics 一并冻结（单行 import 列表里不能用 // 注释单项，所以整行注释、旁边写出不含它的版本）。
import type { DeepSeekWebAdapterConfigStatus, DesktopDisplayInfo, DesktopWorkspaceStatus, LockScreenDiagnostics, ManagedDshStatus, ApiConversationListing, ApiKeyStatus } from '../native/runtime.ts'

// ---- 原 192-198 行：`SettingsPanelProps` 内，`onClearAppearance` 之后、`autostartBusy` 之前 ----
  // FREEZE(1A)：锁屏退出，这 6 个 prop 随之冻结（见 docs/plans/release-scope-cleanup-plan.md 第一节）。
  lockScreenDiagnostics?: LockScreenDiagnostics
  onRefreshLockScreenDiagnostics: () => void
  onRestoreLockScreen: () => void
  onClearStaleLockScreenBackup: () => void
  onSetLockScreenEnabled: (enabled: boolean) => void
  lockScreenBusy: boolean

// ---- 原 1283-1290 行：系统页 `settings.system.windows.title` 卡片内，自动启动 `Field` 之后、`</Card>` 之前 ----
          {/* FREEZE(1A)：壁纸不再触碰锁屏（2026-09-30）。恢复办法：还原这段，并恢复接口里的 6 个 prop 与 SettingsWindow 的传参。 */}
          <Field title="接管锁屏图片" detail={props.lockScreenBusy ? '正在应用系统锁屏设置，请稍候。' : '使用内置且已审计的熟睡画面；密码界面仍由 Windows 原生安全桌面处理。正式版需要 MSIX 包身份。'}><Toggle label="接管锁屏图片" checked={settings.lockScreenEnabled} onChange={props.onSetLockScreenEnabled} disabled={props.lockScreenBusy} /></Field>
          <div className="lockscreen-diagnostics">
            <div className="lockscreen-diagnostics__row"><div><strong>接管状态</strong><small>{props.lockScreenDiagnostics?.managedImageActive ? '正在使用大肥鱼的熟睡画面' : '未检测到本应用的锁屏图片'}</small></div>{props.lockScreenDiagnostics?.managedImageActive ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRestoreLockScreen}>打开 Windows 锁屏设置</button> : props.lockScreenDiagnostics?.staleBackup ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onClearStaleLockScreenBackup}>{props.lockScreenBusy ? '正在清理…' : '清理旧恢复点（删除原图副本）'}</button> : null}</div>
            <div className="lockscreen-diagnostics__row"><div><strong>接管前检查</strong><small>{props.lockScreenDiagnostics ? props.lockScreenDiagnostics.takeoverAvailable ? 'Windows 与当前应用身份允许尝试设置锁屏图片' : props.lockScreenDiagnostics.supported ? 'Windows 允许，但当前正式版需要 MSIX 包身份' : '当前系统不允许应用修改锁屏图片' : '正在读取系统状态…'}</small></div><button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRefreshLockScreenDiagnostics}>{props.lockScreenBusy ? '正在应用…' : '刷新检查'}</button></div>
            {props.lockScreenDiagnostics && <ul><li>备份：{props.lockScreenDiagnostics.staleBackup ? '已保留，但当前锁屏已被外部更改' : props.lockScreenDiagnostics.backupValid ? '原静态图片可恢复' : props.lockScreenDiagnostics.backupExists ? '备份失效' : '尚未创建（首次接管时保存）'}</li><li>托管睡眠图：{props.lockScreenDiagnostics.managedImageReady ? '已准备' : '首次接管时准备'}</li>{props.lockScreenDiagnostics.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>}
          </div>
