/**
 * 更新气泡：状态（计划书 §四）与"什么时候提示"的规则。
 *
 * 这一层是纯的 —— 没有 React、没有 IPC、不求值任何句子 —— 于是"表桌面阶段不出现""已忽略的版本
 * 不再提示""失败不弹气泡"这些能被单测直接钉住，而不是靠盯着屏幕看。
 *
 * §四 那张图里的六个状态**全部可达**（第一、二片只有前三个，第三片接上了下载与安装）：
 *
 * ```
 * idle ──检测到新版本──▶ available ──按「下载」──▶ downloading ──成功──▶ ready ──点「安装」──▶ handing-off
 *                          │                        │
 *                      按「忽略」                 下载失败
 *                          ▼                        ▼
 *                      dismissed(记版本)          failed ──可重试──▶ downloading
 * ```
 *
 * `downloading` / `ready` / `failed` 的**唯一来源是原生的 `update-download` 事件**
 * （[`applyDownloadEvent`]）：界面不自己编这三个状态。曾经这里钉着一条相反的测试（"不许假装
 * 下载"），那是因为第二片还没有下载可接；第三片把断言翻了过来，理由写在 `tests/updateUi.spec.tsx`
 * 里那一条的注释上。
 */
import type { DesktopWorkspace } from '../../runtime/desktopWorkspace.ts'
import type { RuntimeState } from '../../domain/types.ts'
import type {
  UpdateAsset,
  UpdateCheckReport,
  UpdateDownloadEvent,
  UpdateDownloadFailure,
} from '../../native/runtime.ts'

/**
 * §四 的六个状态。六个都真的会出现；`handing-off` 不是其中一个：点「安装」之后交给系统的是
 * 安装程序自己的界面，壁纸这边停在 `ready` 并给一句"已交给 Windows"（见 `useUpdate`）。
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

/**
 * 界面手上的下载状态。
 *
 * **只由 `update-download` 事件产生**（[`applyDownloadEvent`]）：`downloading` 的进度、`ready` 的
 * 落盘位置、`failed` 的原因码都来自那一条事件，界面不自己编一个出来。
 */
export interface UpdateDownloadState {
  /** 这条事件说的是哪个版本；只有与手上这一枚相同时才交给状态机（见 [`updatePhase`]）。 */
  version: string
  phase: 'downloading' | 'ready' | 'failed'
  downloadedBytes: number
  /** API 给的总大小；没给时是 `undefined`（那时说"已下载多少"，不编百分比）。 */
  totalBytes?: number
  /** `ready` 时的落盘位置。 */
  path?: string
  /** `ready` 时算出来的 sha256（`sha256:<hex>`）。 */
  sha256?: string
  /** `failed` 时的原因码。 */
  failure?: UpdateDownloadFailure
}

/**
 * 一条原生事件 → 界面状态。**纯函数**，而且是这三个状态的唯一来源。
 *
 * 事件自带全部字段（版本、相位、字节数、终局的原因码），所以这里不需要"上一份状态"作参数：
 * 后到的事件整份替换前一份。这样也就不会出现"上一轮的失败原因挂在这一轮的进度上"。
 */
export function applyDownloadEvent(event: UpdateDownloadEvent): UpdateDownloadState {
  const total = typeof event.totalBytes === 'number' && event.totalBytes > 0 ? event.totalBytes : undefined
  return {
    version: event.version,
    phase: event.phase,
    downloadedBytes: Math.max(0, event.downloadedBytes),
    totalBytes: total,
    path: event.path ?? undefined,
    sha256: event.sha256 ?? undefined,
    failure: event.failure ?? undefined,
  }
}

/**
 * 进度百分比（0–100 的整数）；**总大小不知道时是 `undefined`**。
 *
 * 三种情况刻意分得很开：不知道总大小 ⇒ 不印百分比（编一个出来就是假数据）；已经超过总大小（不该
 * 发生，但网络与服务端都可能给出意外的字节数）⇒ 封在 100，而不是印出 130%。
 */
export function downloadPercent(download: UpdateDownloadState | undefined): number | undefined {
  const total = download?.totalBytes
  if (download === undefined || total === undefined || total <= 0) return undefined
  return Math.floor((Math.min(download.downloadedBytes, total) / total) * 100)
}

/** 字节数 → 一行给用户看的数字（总大小不知道时用它说"已下载多少"）。 */
export function formatBytes(bytes: number): string {
  const value = Math.max(0, bytes)
  if (value < 1024) return `${Math.round(value)} B`
  const kilobytes = value / 1024
  if (kilobytes < 1024) return `${kilobytes.toFixed(1)} KB`
  const megabytes = kilobytes / 1024
  if (megabytes < 1024) return `${megabytes.toFixed(1)} MB`
  return `${(megabytes / 1024).toFixed(2)} GB`
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

/**
 * 这一份报告与手上这条下载事件处在 §四 的哪个状态。
 *
 * 六个状态的来路分得很清：`available` / `dismissed` / `idle` 来自**检查报告**，
 * `downloading` / `ready` / `failed` 来自**下载事件**（[`applyDownloadEvent`]）。事件缺席时
 * 后面三个不会出现 —— 这正是第三片翻转那条"不许假装下载"测试之后仍然成立的一半。
 *
 * 事件只有在说的是**手上这一枚版本**时才算数：另一个版本的旧事件（比如上一轮下过 0.4.2，
 * 现在提示的是 0.4.3）不该把这一枚气泡改成别的状态。
 */
export function updatePhase(report: UpdateCheckReport | undefined, download?: UpdateDownloadState): UpdatePhase {
  const offer = offeredUpdate(report)
  if (offer) return downloadPhaseFor(offer, download) ?? 'available'
  if (!report) return 'idle'
  const hasVersion = (report.latestVersion?.trim()?.length ?? 0) > 0
  const isNewerVersion = report.outcome === 'updateAvailable' || report.outcome === 'noInstallableAsset'
  if (hasVersion && isNewerVersion && report.dismissed) return 'dismissed'
  return 'idle'
}

/**
 * 这条事件属于这一枚更新吗（属于就给它的相位）。
 *
 * 比的是**原生原样回给我们的那个版本串**：界面把 `offer.version` 递过去、原生把它写回事件里
 * （只去掉了 `v` 前缀与首尾空白，见 `update/download.rs`），所以这里做同样的规范化就够了 ——
 * 不再实现一遍"补齐四段"的版本比较（那是原生的事，界面各算一遍就是两处可能不一致）。
 */
function downloadPhaseFor(
  offer: UpdateOffer,
  download: UpdateDownloadState | undefined,
): UpdateDownloadState['phase'] | undefined {
  if (!download) return undefined
  const normalize = (version: string) => version.trim().replace(/^[vV]/, '')
  if (normalize(download.version) !== normalize(offer.version)) return undefined
  return download.phase
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
