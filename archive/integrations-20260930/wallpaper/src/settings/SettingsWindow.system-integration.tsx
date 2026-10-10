// 归档片段（B4，2026-10）：`wallpaper/src/settings/SettingsWindow.tsx` 中系统集成（1B：TranslucentTB / 登录过渡底图）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/integrations-20260930/README.md` 的 B4 小节。

// ---- 原 9-10 行：import 区，锁屏的 import 注释（原 7-8）之后 ----
// FREEZE(1B)：透明任务栏退出，这个类型随之冻结（单行 import 列表里不能用 // 注释单项）。
import { nativeRuntime, type AutostartStatus, type ApiConversationListing, type ApiKeyStatus, type DeepSeekWebAdapterConfigStatus, type DesktopDisplayInfo, type DesktopWorkspaceStatus, type HarnessEndpointScan, type HarnessTarget, type ManagedDshStatus, type TranslucentTbStatus } from '../native/runtime.ts'

// ---- 原 123-124 行：`interactionEnabled` 之后、`harnessTargets` 之前 ----
  // FREEZE(1B)：探针冻结后没人再写它，界面也不再读它（卡片已冻结），保留初始值不动。
  const [translucentTb, setTranslucentTb] = useState<TranslucentTbStatus>({ installed: false, running: false })

// ---- 原 588-592 行：`probeRunners` 的第一项（`managedDsh` 之前），与 settingsProbes 片段里的 `translucentTb` 探针一起恢复 ----
    // FREEZE(1B)：TranslucentTB 探针冻结（状态仍可由用户点「刷新」手动读取）。
    translucentTb: async () => {
      const status = await nativeRuntime.translucentTbStatus()
      if (mountedRef.current) setTranslucentTb(status)
    },

// ---- 原 639-640 行：`refreshProbe` 之后、`refreshManagedDsh` 之前 ----
  // FREEZE(1B)：这条探针已退出。
  const refreshTranslucentTb = () => refreshProbe('translucentTb')

// ---- 原 1048-1049 行：`<SettingsPanel>` 的 props，`harnessStatus` 之后、`harnessTargets` 之前 ----
      /* FREEZE(1B)：透明任务栏退出，这几个 prop 随之冻结。 */
      translucentTb={translucentTb}

// ---- 原 1146-1149 行：`<SettingsPanel>` 的 props，`onChange` 之后、`appearanceAssets` 之前 ----
      /* FREEZE(1B)：同上。 */
      onRefreshTranslucentTb={refreshTranslucentTb}
      onLaunchTranslucentTb={() => void nativeRuntime.launchTranslucentTb().then(refreshTranslucentTb).catch((error) => setNotice(String(error)))}
      onInstallTranslucentTb={() => void nativeRuntime.openTranslucentTbInstall().catch((error) => setNotice(String(error)))}
