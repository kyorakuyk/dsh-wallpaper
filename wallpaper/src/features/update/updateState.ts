/**
 * 更新气泡：状态（计划书 §四）与"什么时候提示"的规则。
 *
 * 这一层是纯的 —— 没有 React、没有 IPC、不求值任何句子 —— 于是"表桌面阶段不出现""已忽略的版本
 * 不再提示""失败不弹气泡"这些能被单测直接钉住，而不是靠盯着屏幕看。
 *
 * **第二片（界面）只实现状态机里能实现的那几个**，其余留着手（计划书 §四 那张图）：
 *
 * ```
 * idle ──检测到新版本──▶ available ──按「下载」──▶ downloading ──成功──▶ ready ──点「安装」──▶ handing-off
 *                          │                        │
 *                      按「忽略」                 下载失败
 *                          ▼                        ▼
 *                      dismissed(记版本)          failed ──可重试──▶ downloading
 * ```
 *
 * - `idle` / `available` / `dismissed`：本片实现；
 * - `downloading` / `ready` / `failed`：**第三片**（原生流式下载 + 进度事件 + 交付安装器）才会
 *   产生。本片**不假装实现**它们：没有下载就没有进度，也没有"下载失败"。本片「下载」按钮走的是
 *   §六 的回落路径（打开发布页），它失败时落在气泡下面那行提示上，而不是这个状态机里。
 */
import type { DesktopWorkspace } from '../../runtime/desktopWorkspace.ts'
import type { RuntimeState } from '../../domain/types.ts'
import type { UpdateAsset, UpdateCheckReport } from '../../native/runtime.ts'

/**
 * §四 的六个状态。**本片只会出现前三个**；后三个是第三片要接的手。
 *
 * 单独写出来而不是"有没有 offer"的两个布尔，是为了让第三片接进度条时不必重新定义一套词汇：
 * 那时 `downloading` / `ready` / `failed` 会有各自的来源（原生事件），而界面分支已经写好了。
 */
export type UpdatePhase = 'idle' | 'available' | 'dismissed' | 'downloading' | 'ready' | 'failed'

/**
 * 仓库的 releases 页：报告里没给 release 地址时的回落（§六）。
 *
 * 与原生侧 `update::RELEASE_REPOSITORY`（`src-tauri/src/update/mod.rs`）是同一个仓库，所以两边
 * 不许各写一份还各说各的 —— `tests/updateUi.spec.ts` 直接对着原生那一行核对。
 */
export const RELEASES_PAGE_URL = 'https://github.com/kyorakuyk/dsh-wallpaper/releases'

/** 手上这一枚要提示的更新。 */
export interface UpdateOffer {
  /** 要提示的版本（原生给的写法：`0.4.2` 不写成 `0.4.2.0`）。 */
  version: string
  /**
   * 选中的可安装资产。`undefined` 表示这次发布**没有**可安装的安装包（§3.1）——那时界面给的是
   * 「打开发布页」而不是「下载」，而不是报错。
   */
  asset?: UpdateAsset
  /** 发布页地址：报告里的 `releaseUrl`，没有就退到仓库的 releases 页。 */
  releaseUrl: string
}

/** 这次 release 的发布页（报告没给就用仓库的 releases 页）。 */
export function releasePageUrl(report: UpdateCheckReport | undefined): string {
  const url = report?.releaseUrl?.trim()
  return url ? url : RELEASES_PAGE_URL
}

/**
 * 现在有哪一枚更新值得提示（没有就是 `undefined`）。
 *
 * 只认两种结论：`updateAvailable`（有新版本 + 有可安装资产）与 `noInstallableAsset`（有新版本、
 * 但这次发布没挂安装包）。其余一律不提示：
 *  - `failed`：检查失败不弹气泡，只在设置里留一行结果（§五 "不打扰是设计目标"）；
 *  - `upToDate` / `skipped`：本来就没有"新版本"这回事。
 *
 * `report.dismissed` 由**原生**算好（版本等价比较，`0.4.1` 与 `0.4.1.0` 是同一个版本），界面不再
 * 实现第二遍 —— 两处各算一遍就是两处可能不一致。
 */
export function offeredUpdate(report: UpdateCheckReport | undefined): UpdateOffer | undefined {
  if (!report) return undefined
  if (report.outcome !== 'updateAvailable' && report.outcome !== 'noInstallableAsset') return undefined
  const version = report.latestVersion?.trim()
  // 没有版本号就没有可忽略、也没有可下载的对象：宁可不提示，也不提示一个空版本。
  if (!version) return undefined
  if (report.dismissed) return undefined
  return {
    version,
    asset: report.asset ?? undefined,
    releaseUrl: releasePageUrl(report),
  }
}

/** 这一份报告处在 §四 的哪个状态（本片只有 `idle` / `available` / `dismissed` 会出现）。 */
export function updatePhase(report: UpdateCheckReport | undefined): UpdatePhase {
  if (offeredUpdate(report)) return 'available'
  if (!report) return 'idle'
  const hasVersion = (report.latestVersion?.trim()?.length ?? 0) > 0
  const isNewerVersion = report.outcome === 'updateAvailable' || report.outcome === 'noInstallableAsset'
  if (hasVersion && isNewerVersion && report.dismissed) return 'dismissed'
  return 'idle'
}

/**
 * 气泡什么时候出现（§四、§八 1）。
 *
 * 三条同时成立才出现：
 *
 * 1. **已经进入里桌面**（`workspace === 'inner'`）。表桌面阶段（含「早上好…」那枚气泡的整段时间）
 *    不出现 —— 这正是验收 1；`entering-inner` 那 280 毫秒也只是"正在进"，还不算"进入之后"，
 *    `leaving-inner` 同理；
 * 2. **唤醒动画已经走完**：`phase` 是 `idle` 或 `chatting`。锁屏、苏醒、启动中都不出现。进入里
 *    桌面本身就是"打开会话"，相位因此是 `chatting`，两者都算过完动画；
 * 3. 手上有一枚**没被忽略**的更新（`offeredUpdate`）—— 失败与"已是最新"都不弹（§五）。
 */
export function updateBubbleVisible(options: {
  workspace: DesktopWorkspace
  phase: RuntimeState['phase']
  offer?: UpdateOffer
}): boolean {
  if (!options.offer) return false
  if (options.workspace !== 'inner') return false
  return options.phase === 'idle' || options.phase === 'chatting'
}
