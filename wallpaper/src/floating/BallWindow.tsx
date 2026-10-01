/**
 * 悬浮球窗口（surface `ball`）：**玻璃碟 + 黑笔圈 + 黑笔"消息"**。
 *
 * 它是折叠态胶囊的新家——壁纸场景画在 Explorer 图标层之下，表桌面收不到任何鼠标消息
 * （见 `docs/evidence/input-model-desktop-hit-testing.md`），所以折叠态的 UI 必须搬到一个
 * 独立顶层窗口里才点得着。原生侧的窗口策略（圆形区域、Z 槽夹在图标层与普通应用之间、
 * 永不抢前台、鼠标靠近才滑入）见 `src-tauri/src/floating_ball.rs`。
 *
 * 视觉由**一张 SVG** 画完（玻璃面、墨圈、字形），三条理由：
 *
 * 1. **一律画在窗口里侧**。原生 `SetWindowRgn` 的圆形区域是 1bit 掩码、**没有抗锯齿**，
 *    只要图形画到窗口边缘，那道阶梯状硬边就会露出来（用户实测报的"毛边"）。所以整个球
 *    的可见轮廓最大半径 25.9（viewBox 56），四周留 2px 透明边距：区域裁掉的只有透明像素，
 *    真正的边界是 SVG 自己的抗锯齿圆边。`ballAppearance.spec.tsx` 逐点解析路径来钉住这条。
 * 2. **没有透镜**。没有径向渐变、没有内发光、没有"玻璃球"式的明暗过渡——用户明确要求
 *    这次不要鱼眼效果。玻璃只是一层**均匀的**半透明色。
 * 3. **墨圈是一笔画的**。外/内两条轮廓都由同一条手绘感的闭合曲线生成（`artifacts/ring-gen.py`），
 *    抖动 <0.35px，用 `fill-rule="evenodd"` 填成实心圈；"消息"字形复用的是设计系统那一份
 *    `iconPaths.message`，只是换成了黑墨描边。两者都不描边到窗口边缘。
 *
 * 单击的语义是 §1.1 拍板的**不可逆动作**：
 * **单击球 = 进入里桌面 + 弹出输入岛**（不是"只弹岛"）。实现上只调用一条原生命令，
 * 由它与桌面空白双击共用同一个入场实现；输入岛本身由 `App.tsx` 收到
 * `desktop-workspace-toggle` 的 `"enter"` 后展开。**不要把这里改成"只展开输入岛"**：
 * 表桌面下壁纸场景收不到鼠标，输入岛在进入里桌面之前根本点不着。
 *
 * 该命令只授权给这一扇窗口（`capabilities/floating-ball.json`）。
 */

/** 引入设计令牌，让球的配色与产品同一套变量。 */
import '../ui/tokens/tokens.css'
import './BallWindow.css'
import { invoke } from '@tauri-apps/api/core'
import { useEffect } from 'react'
import { t, useLanguage } from '../i18n/index.ts'
import { suppressNativeContextMenu } from '../runtime/contextMenu.ts'
import { iconPaths } from '../ui/primitives/Icon.tsx'

/** 玻璃碟的外轮廓：同一条手绘闭合曲线（见 `artifacts/ring-gen.py`）。 */
export const BALL_GLASS_PATH =
  'M53.19 32.58C52.41 36.75 50.13 41.21 47.35 44.42C44.58 47.63 40.51 50.39 36.52 51.82C32.54 53.26 27.64 53.73 23.45 53.03C19.25 52.33 14.66 50.33 11.36 47.61C8.06 44.89 5.11 40.76 3.63 36.72C2.16 32.68 1.73 27.60 2.52 23.36C3.30 19.13 5.53 14.56 8.34 11.32C11.16 8.09 15.36 5.33 19.40 3.95C23.43 2.58 28.37 2.28 32.54 3.07C36.70 3.86 41.14 5.97 44.38 8.69C47.63 11.42 50.54 15.43 52.01 19.41C53.48 23.39 53.96 28.41 53.19 32.58Z'

/** 同一支笔画出的**内**轮廓；与上面那段一起构成 `evenodd` 的实心墨圈。 */
export const BALL_RING_PATH =
  `${BALL_GLASS_PATH}M50.20 32.04C49.57 35.74 47.74 39.81 45.31 42.69C42.88 45.57 39.18 48.08 35.63 49.32C32.07 50.55 27.65 50.82 23.98 50.09C20.31 49.36 16.42 47.38 13.62 44.94C10.83 42.50 8.43 38.93 7.19 35.44C5.95 31.96 5.55 27.68 6.18 24.03C6.80 20.38 8.57 16.41 10.93 13.52C13.30 10.64 16.87 8.03 20.39 6.72C23.91 5.41 28.36 4.99 32.06 5.67C35.77 6.34 39.78 8.30 42.62 10.77C45.46 13.23 47.83 16.91 49.09 20.45C50.35 24.00 50.83 28.33 50.20 32.04Z`

export function BallWindow() {
  // 球上的可访问名字与 title 来自词条。
  useLanguage()
  // 球也是壁纸的一部分：右键它不该弹出"图像另存为/更多工具"那一套网页菜单。
  useEffect(() => suppressNativeContextMenu(), [])
  return (
    <button
      type="button"
      className="ball"
      aria-label={t('ball.open-island')}
      title={t('ball.open-island')}
      onClick={() => {
        // 失败不弹任何界面：球的窗口没有能力显示错误，原生侧会留下日志。
        void invoke('enter_inner_workspace_from_ball').catch(() => undefined)
      }}
    >
      {/* 玻璃碟 → 墨圈 → 墨字，三层都在这一张 SVG 里，且**都不画到窗口边缘**
          （四周留 2px：原生圆形区域没有抗锯齿，画到边上就是毛边）。 */}
      <svg className="ball__ink" viewBox="0 0 56 56" aria-hidden="true">
        <path className="ball__glass" d={BALL_GLASS_PATH} />
        <path className="ball__ring" fillRule="evenodd" d={BALL_RING_PATH} />
        {/* 「消息」：复用设计系统的字形，居中放进 56×56（24 格居中 = (56-24)/2 = 16）。 */}
        <path className="ball__message" d={iconPaths.message} transform="translate(16 16)" />
      </svg>
    </button>
  )
}
