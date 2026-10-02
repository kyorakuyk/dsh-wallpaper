/**
 * 两个面共用的更新检测状态：壁纸的立绘气泡与设置中心的系统页（计划书 §四、§五）。
 *
 * 「同一份状态」落在**原生状态文件**上（`%LOCALAPPDATA%\com.dsh.wallpaper\updates\state.json`）。
 * 这不是实现细节而是前提：壁纸窗口与设置窗口是两个 WebView、两份 JS 运行时，谁也看不见谁的
 * `useState` —— 它们唯一的共识只能来自那次检查往 `state.json` 里写下的东西。所以"检查"与
 * "忽略"都从原生进出，本模块只负责把报告翻译成界面要的东西。
 *
 * 第三片接上的三件事：
 *
 *  - 「下载」调原生 `update_download`（流式下载 + 校验），命令很快返回，只说"开工了没有"；
 *  - `downloading` / `ready` / `failed` 三个状态**只由原生的 `update-download` 事件产生**
 *    （[`applyDownloadEvent`]）—— 界面不自己编一个进度出来；
 *  - 那条事件是**全局事件**，两个窗口都订阅：设置页按下「下载」时壁纸上的气泡也会走同一条
 *    时间线（上一片发现"两个窗口之间没有推送"就是缺了它）。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { msg, sentenceOf, type Message } from '../../i18n/index.ts'
import { nativeRuntime, type UpdateCheckReport, type UpdateDownloadEvent } from '../../native/runtime.ts'
import { listenUntilDisposed } from '../../runtime/lifecycle.ts'
import { applyDownloadEvent, offeredUpdate, updatePhase, type UpdateDownloadState, type UpdateOffer, type UpdatePhase } from './updateState.ts'
import { updateCallMessage } from './updateCopy.ts'

/**
 * 一次性闸门：第一次返回 `true`，之后恒为 `false`。
 *
 * 抽成工厂是为了让"每进程一次"这条规则能被单测直接钉住（模块级的那个实例只创建一次，
 * 单测没法再要一个新的）。
 */
export function createOnceGate(): () => boolean {
  let claimed = false
  return () => {
    if (claimed) return false
    claimed = true
    return true
  }
}

/**
 * 自动检查的闸门：**每个进程最多发一次**。
 *
 * 模块级（不是 ref），理由与 `App.tsx` 里那条 `dshAutostart` 完全一样：重挂载会把 ref 清掉，
 * 而这里要的正是"活过一次重挂载"。真正的保证在原生侧 —— 6 小时节流（§五）才是"不该打网络的
 * 时候一次都不打"；这道闸门只是省掉重复的 IPC 与日志。
 */
const automaticCheckClaimed = createOnceGate()

export interface UpdateController {
  /** 最近一次报告；还没查过（或浏览器预览没有原生宿主）时是 `undefined`。 */
  report?: UpdateCheckReport
  /** 手上这一枚值得提示的更新（已忽略、失败、已是最新都没有）。 */
  offer?: UpdateOffer
  /**
   * §四 的状态机。`downloading` / `ready` / `failed` 来自下载事件；`dismissed` 来自报告里原生
   * 算好的 `dismissed`。气泡与设置卡片都按它分支。
   */
  phase: UpdatePhase
  /** 手上这一条下载事件（进度、落盘位置或失败原因）；没有下载过时是 `undefined`。 */
  download?: UpdateDownloadState
  /** 有原生调用在飞：按钮据此禁用，免得一次点击变成两次请求。 */
  busy: boolean
  /** 气泡/卡片下面那行提示（忽略没落盘、下载没起来、安装没起来、调用被拒）。 */
  notice?: Message
  /** 查一次。`manual` 为真时不受原生侧 6 小时节流限制（设置页的按钮与打开系统页那一次）。 */
  check(manual: boolean): Promise<UpdateCheckReport | undefined>
  /** 进入里桌面之后那一次自动检查（每进程一次；原生侧另有 6 小时节流）。 */
  checkIfDue(): void
  /** 记下某个版本已忽略（气泡与设置页共用的那一条）。 */
  dismiss(version: string): Promise<void>
  /**
   * 「下载」：把报告里选中的资产交给原生流式下载（§六）。进度与终局从 [`UpdateController.download`]
   * 回来 —— 也就是那条全局事件，不是这里的返回值。
   *
   * 这次发布没有可安装资产时按钮本来就是「打开发布页」（§3.1），所以这条路会直接打开它。
   */
  startDownload(): Promise<void>
  /** 「点击安装」（`ready` 状态）：把已经下好的安装包按后缀交给 Windows（§六）。 */
  install(): Promise<void>
  /** 回落：打开发布页（§六 规定资产缺失或下载失败时提供它）。 */
  openReleasePage(): Promise<void>
}

export function useUpdate(): UpdateController {
  const [report, setReport] = useState<UpdateCheckReport>()
  const [download, setDownload] = useState<UpdateDownloadState>()
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<Message>()
  /**
   * 请求代际：两次检查重叠时（自动那一次还在飞、用户又点了「检查更新」），只有最后一次回来
   * 的结果能写进状态 —— 否则先发的旧结果会盖掉新结果，界面上就是"点了按钮反而退回上一步"。
   *
   * 「忽略」也会把它 +1：那条记录比任何**在它之前发出**的检查都新。不加这一下，一次慢检查回来时
   * 报告里的 `dismissed` 还是 `false`（它读状态发生在忽略之前），刚被忽略的气泡会当场又冒出来。
   */
  const epochRef = useRef(0)
  /** 在飞的调用有几层。计数而不是布尔：检查与忽略可以重叠，谁最后结束谁把 `busy` 放下。 */
  const inFlightRef = useRef(0)
  const beginWork = () => {
    inFlightRef.current += 1
    setBusy(true)
  }
  const endWork = () => {
    inFlightRef.current = Math.max(0, inFlightRef.current - 1)
    if (inFlightRef.current === 0) setBusy(false)
  }

  /**
   * 下载进度与终局：**唯一**产生 `downloading` / `ready` / `failed` 的地方。
   *
   * 走 `listenUntilDisposed`（`runtime/lifecycle.ts`）而不是自己存一个 disposer：挂载后立刻卸载时
   * 那个 promise 还没 settle，自己存就会把原生监听器漏在 WebView 里。
   *
   * 事件整份替换状态（[`applyDownloadEvent`]）：它自带版本、相位、字节数与终局原因，所以不需要
   * 拿上一份状态做增量 —— 也就不会出现"上一轮的失败原因挂在这一轮的进度上"。
   */
  useEffect(() => {
    const listener = listenUntilDisposed<UpdateDownloadEvent>(
      (receive) => nativeRuntime.listenUpdateDownload(receive),
      (event) => setDownload(applyDownloadEvent(event)),
      {
        onError: (error) => {
          // 订阅不上就永远没有进度：如实说一句，而不是让进度条停在 0%（或干脆不动）。
          setNotice(msg('update.notice.listen-failed', { error: sentenceOf(error) }))
        },
      },
    )
    return () => listener.dispose()
  }, [])

  const check = useCallback(async (manual: boolean) => {
    const epoch = ++epochRef.current
    beginWork()
    try {
      const next = await nativeRuntime.updateCheck(manual)
      if (epochRef.current !== epoch) return next
      // 浏览器预览拿回来的是 `undefined`：**不动**手上这一份报告，也不假装查过了。
      if (next) setReport(next)
      return next
    } catch (error) {
      // 调用本身失败（被 ACL 拒、IPC 断了）也要看得见：不然按钮就是"点了没反应"。
      if (epochRef.current === epoch) setNotice(msg('update.notice.check-failed', { error: sentenceOf(error) }))
      return undefined
    } finally {
      endWork()
    }
  }, [])

  const checkIfDue = useCallback(() => {
    if (!automaticCheckClaimed()) return
    void check(false)
  }, [check])

  const dismiss = useCallback(async (version: string) => {
    // 见 `epochRef` 的说明：这条记录比先发出的检查新。
    epochRef.current += 1
    beginWork()
    setNotice(undefined)
    try {
      const result = await nativeRuntime.updateDismiss(version)
      if (!result.persisted) {
        // 状态写不进去 = **重启之后同一个版本还会提示**。如实说出来，别让用户以为按过了。
        setNotice(msg('update.notice.dismiss-unpersisted'))
        return
      }
      // 本地就把结论改成"这个版本已忽略"：与原生报告里的 `dismissed` 是同一个字段，所以界面只有
      // 一条判断（`offeredUpdate`），不需要第二套"本地忽略过"的记录 —— 两套记录迟早会各说各的。
      setReport((current) => current && {
        ...current,
        dismissed: true,
        dismissedVersion: result.dismissedVersion ?? version,
      })
    } catch (error) {
      setNotice(msg('update.notice.dismiss-failed', { error: sentenceOf(error) }))
    } finally {
      endWork()
    }
  }, [])

  const offer = useMemo(() => offeredUpdate(report), [report])
  /** 回调要读"当下这一枚"，不能读闭包当初捕获的那一个（报告会被下一次检查换掉）。 */
  const offerRef = useRef<UpdateOffer>()
  offerRef.current = offer
  /** §四 的状态机：报告给前三个状态，下载事件给后三个（`updatePhase` 里两处汇合）。 */
  const phase = useMemo(() => updatePhase(report, download), [report, download])

  const openReleasePage = useCallback(async () => {
    const offer = offerRef.current
    if (!offer) {
      // 没有可提示的更新时这个按钮根本不存在，所以这里到不了；真到了也不发一次注定失败的调用。
      console.warn('update: no offered release to open')
      return
    }
    beginWork()
    setNotice(undefined)
    try {
      /**
       * §六 的回落路径：把这次 release 的发布页交给默认浏览器。资产缺席、下载失败、或者用户自己
       * 想手动去下，走的都是它 —— 它不是"临时实现"，是设计里的兜底（第三片把它从主按钮挪到了
       * 回落按钮上）。
       */
      await nativeRuntime.openExternalLink(offer.releaseUrl)
    } catch (error) {
      setNotice(msg('update.notice.open-release-failed', { error: sentenceOf(error) }))
    } finally {
      endWork()
    }
  }, [])

  /**
   * 「下载」：交给原生流式下载（§六）。
   *
   * **这一颗按钮是唯一的下载入口**：检查那条路（`check` / `checkIfDue`）里没有它，命令的调用点
   * 也全在这一段里 —— "打开壁纸就自动下 30 MB"这件事在代码里不存在（原生侧另有单测钉着
   * `run_check` 里没有下载调用）。
   *
   * 这里**不预置 `downloading`**：进度条要等原生第一条进度事件回来才出现。界面自己先摆一个进度条
   * 出来就是"假装下载"（第二片那条测试的翻转版：三个状态必须由**真实事件**驱动）。
   */
  const downloadUpdate = useCallback(async () => {
    const offer = offerRef.current
    if (!offer) {
      console.warn('update: no offered release to download')
      return
    }
    if (!offer.asset) {
      // 这次发布没有可安装资产：主按钮本来就是「打开发布页」（§3.1 的回落），走同一条路。
      await openReleasePage()
      return
    }
    beginWork()
    setNotice(undefined)
    try {
      const report = await nativeRuntime.updateDownload(offer.version, offer.asset)
      if (!report) {
        // 浏览器预览没有原生宿主：如实说，而不是让按钮看起来"点了没反应"。
        setNotice(msg('update.notice.update-unavailable'))
      }
      // `started === false`：同一时刻已经有一次下载在跑（多半是另一个窗口按的）。什么都不用做
      // —— 那条事件是全局的，这一边的进度条照样会跟着走。
    } catch (error) {
      setNotice(msg('update.notice.download-failed', { error: updateCallMessage(error) }))
    } finally {
      endWork()
    }
  }, [openReleasePage])

  /**
   * 「点击安装」：把下载好的安装包按后缀交给 Windows（§六）。
   *
   * 原生那一步的顺序是"**先安排、后退出**"：它先起一个等本进程结束的助手，成功就让应用退出
   * （`nextStep === 'exiting'`）—— 那一步回来的报告是这次调用**最后**一次回话，所以这里必须
   * 当场把话说清楚（"正在退出以便安装"），而不是等一个不会来的事件。助手启动安装器时带的是
   * `/P /UPDATE /R`（原生 `INSTALLER_ARGUMENTS`）：passive 把维护页与向导页整片去掉，装完
   * 还负责把新版本起回来 —— 所以这一句话里没有"请在窗口里确认"这种事要做。
   *
   * 助手起不来时原生回落到"现在就交给 Windows"（`nextStep === 'opened'`）：应用**不退出**，
   * 界面如实说这一句 —— 回落不是失败（安装包照样交出去了），但用户要知道接下来该看哪儿：
   * 那条路上安装器是普通窗口，会先问要不要关掉还在跑的应用。
   */
  const install = useCallback(async () => {
    beginWork()
    setNotice(undefined)
    try {
      const result = await nativeRuntime.updateInstall()
      if (!result) {
        setNotice(msg('update.notice.update-unavailable'))
        return
      }
      setNotice(
        result.nextStep === 'exiting'
          ? msg('update.notice.install-exiting')
          : msg('update.notice.install-fallback-opened'),
      )
    } catch (error) {
      setNotice(msg('update.notice.install-failed', { error: updateCallMessage(error) }))
    } finally {
      endWork()
    }
  }, [])

  return useMemo(
    () => ({
      report,
      offer,
      phase,
      download,
      busy,
      notice,
      check,
      checkIfDue,
      dismiss,
      startDownload: downloadUpdate,
      install,
      openReleasePage,
    }),
    [report, offer, phase, download, busy, notice, check, checkIfDue, dismiss, downloadUpdate, install, openReleasePage],
  )
}

/**
 * 壁纸那一面：**进入里桌面之后**自动查一次，并把气泡要的三样东西交给立绘槽位。
 *
 * 触发点写在 `updateState.updateBubbleVisible` 旁边，两处条件必须一致 —— 这里只是把那两个条件
 * （"在里桌面"与"过完唤醒动画"）交给它，避免"查了却不显示"或"显示了却没查"。
 */
export function useUpdateBubble(options: { inInnerDesktop: boolean; awake: boolean }): UpdateController {
  const update = useUpdate()
  const { inInnerDesktop, awake } = options
  const { checkIfDue } = update
  useEffect(() => {
    // 表桌面阶段既不查也不显示（§四）：锁屏/苏醒/启动中同样不查。
    if (!inInnerDesktop || !awake) return
    checkIfDue()
  }, [inInnerDesktop, awake, checkIfDue])
  return update
}
