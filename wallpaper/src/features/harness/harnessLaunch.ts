import type { RuntimeState } from '../../domain/types.ts'
import { msg, type Message } from '../../i18n/index.ts'
import type { HarnessLaunchKind } from './harnessConnection.ts'

/**
 * Reconcile an in-flight automatic (or manual) launch with what the Bridge
 * reports. Kept pure and exported so the reason a user sees is decided by one
 * tested function rather than by nested conditions inside an interval.
 *
 * Returns `null` while the launch is still legitimately pending.
 */
/** 启动后多久才允许下"它退出了"这个结论（宽限期内不下结论，见函数内注释）。
 *
 * 各类别不一样长，因为它们的启动成本不一样：源码目录直接跑本机二进制，已安装的 CLI 要先经过
 * 一层 npm 批处理（cmd）再拉起 node，冷启动明显更慢。同一条时限套在所有人身上，就是"刚报错
 * 就连上"的来源 —— 报错早于事实。
 */
export const IMMEDIATE_EXIT_GRACE_MS = {
  checkout: 8_000,
  'installed-cli': 20_000,
} as const

/** 只有非壳主体才是本进程能观察"退出"的孩子，所以只有它们有宽限。 */
function exitGraceMs(launchedKind: HarnessLaunchKind): number {
  return launchedKind === 'embedded-shell' ? 0 : IMMEDIATE_EXIT_GRACE_MS[launchedKind]
}

export function harnessLaunchOutcome(
  status: { availability: RuntimeState['harness']; reasonCode?: string },
  elapsedMs: number,
  managed: { managed: boolean; running: boolean },
  timeoutMs = 45_000,
  /**
   * Which class was started. Only a checkout is a child this process owns, so only
   * a checkout can be observed *exiting*: for a shell the process that answers is
   * not ours, and "not managed" would otherwise be reported as "started and
   * immediately exited" for a client that is running perfectly well.
   */
  launchedKind: HarnessLaunchKind = 'checkout',
): { message: Message } | null {
  if (status.availability === 'bridge-ready') return null
  // "还没被我管起来"不等于"已经退出了"：宿主起来要几秒（实测：启动 22:27:54、端口与门票
  // 22:27:56；已安装 CLI 还要先经过一层 cmd 与批处理）。各类别给各自长度的宽限期，期内不下
  // 结论 —— 否则用户会先看到"启动后很快退出"，两秒后指示灯又变绿：一次假警报，比不说更糟。
  if (
    launchedKind !== 'embedded-shell'
    && elapsedMs > exitGraceMs(launchedKind)
    && (!managed.managed || !managed.running)
  ) {
    return { message: msg('app.bubble.launch.exited-early') }
  }
  if (elapsedMs <= timeoutMs) return null
  // Past the deadline the most specific available cause wins: a Bridge that
  // answered but cannot be used is a different fix from one that never appeared.
  if (status.availability === 'bridge-auth-unavailable') {
    return { message: msg('app.bubble.launch.token-unavailable') }
  }
  if (status.availability === 'bridge-incompatible') {
    return { message: msg('app.bubble.launch.incompatible') }
  }
  if (status.availability === 'bridge-loading') {
    return { message: msg('app.bubble.launch.bridge-loading') }
  }
  return { message: msg('app.bubble.launch.timeout') }
}

/**
 * 黄灯：连接与断开之间的**中间态**。
 *
 * 用户要的三段语义是"连上（绿）／正在连（黄呼吸）／不在了（熄灭）"，所以中间态覆盖三种真实
 * 情形：正在把 harness 后台拉起来（点滑槽或开机自启）、宿主已经应答但 Bridge 仍在装载、
 * 以及已经连上却暂时不应答（还没判死）。少了前两种，用户在"启动中"看到的是一盏熄灭的灯，
 * 那和"后端已经死了"看起来一模一样——而这两件事要让用户做的动作完全不同。
 *
 * 纯函数，因为这条规则是**灯的语言**，不该埋在三元表达式里：它连着"复位滑槽"和"自动回切"，
 * 判断错了用户就会在错误的时刻被搬走。
 */
export function isHarnessTransitioning(state: {
  /** 壁纸正在启动（或等待）已配置的主体。 */
  starting: boolean
  availability: RuntimeState['harness']
  /** 就绪过的桥接正在失联，但还没到"确认掉线"。 */
  probing: boolean
}): boolean {
  return state.starting || state.probing || state.availability === 'bridge-loading'
}

/**
 * 切换主体之后，黄灯（"连接中"）至少要亮这么久。
 *
 * 用户的要求是"进入固定 1–2s 的黄灯缓冲后**立刻载入**"：答案往往一次性就回来了，如果灯跟着答案
 * 一闪而过，用户根本看不到"它在连"，只会觉得界面抖了一下。所以这是**最短停留**，不是超时。
 *
 * 顺带说清"缓存"在这里指什么：**不是缓存会话 id**。桥的规矩是"只有没有 resume id 时才去建
 * '今天这条桌面会话'"（每日边界 04:00，你们自己定的），把昨天的 id 存下来再恢复就等于悄悄破了
 * 那条边界。所以切回来时该做的是"端点保持热 + 这一小段缓冲"，让宿主用一次往返把今天的会话交回来。
 */
export const HARNESS_SWITCH_BUFFER_MS = 1_200
