/**
 * 两个面共用的更新检测状态：壁纸的立绘气泡与设置中心的系统页（计划书 §四、§五）。
 *
 * 「同一份状态」落在**原生状态文件**上（`%LOCALAPPDATA%\com.dsh.wallpaper\updates\state.json`）。
 * 这不是实现细节而是前提：壁纸窗口与设置窗口是两个 WebView、两份 JS 运行时，谁也看不见谁的
 * `useState` —— 它们唯一的共识只能来自那次检查往 `state.json` 里写下的东西。所以"检查"与
 * "忽略"都从原生进出，本模块只负责把报告翻译成界面要的东西。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { msg, sentenceOf, type Message } from '../../i18n/index.ts'
import { nativeRuntime, type UpdateCheckReport } from '../../native/runtime.ts'
import { offeredUpdate, type UpdateOffer } from './updateState.ts'

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
  /** 有原生调用在飞：按钮据此禁用，免得一次点击变成两次请求。 */
  busy: boolean
  /** 气泡/卡片下面那行提示（忽略没落盘、打开发布页失败、调用被拒）。 */
  notice?: Message
  /** 查一次。`manual` 为真时不受原生侧 6 小时节流限制（设置页的按钮与打开系统页那一次）。 */
  check(manual: boolean): Promise<UpdateCheckReport | undefined>
  /** 进入里桌面之后那一次自动检查（每进程一次；原生侧另有 6 小时节流）。 */
  checkIfDue(): void
  /** 记下某个版本已忽略（气泡与设置页共用的那一条）。 */
  dismiss(version: string): Promise<void>
  /** 本片的「下载」：打开发布页（第三片换成真下载，见函数内注释）。 */
  openReleasePage(): Promise<void>
}

export function useUpdate(): UpdateController {
  const [report, setReport] = useState<UpdateCheckReport>()
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
       * **本片是临时实现**：把这次 release 的发布页交给默认浏览器，用户自己去下。
       *
       * TODO（第三片）：换成原生侧的流式下载 + 进度事件 + 校验 + 交付安装器 —— 那时这里调
       * `nativeRuntime.updateDownload(...)`、气泡进入 `downloading`（进度条）→ `ready`
       * （「点击安装」）。**这条"打开发布页"要留着**：§六 规定资产缺席或下载失败时正是它
       * 兜底，不是被删掉。
       */
      await nativeRuntime.openExternalLink(offer.releaseUrl)
    } catch (error) {
      setNotice(msg('update.notice.open-release-failed', { error: sentenceOf(error) }))
    } finally {
      endWork()
    }
  }, [])

  return useMemo(
    () => ({ report, offer, busy, notice, check, checkIfDue, dismiss, openReleasePage }),
    [report, offer, busy, notice, check, checkIfDue, dismiss, openReleasePage],
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
