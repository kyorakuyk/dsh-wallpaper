/**
 * 悬浮球窗口（surface `ball`）：一个极简的小圆球。
 *
 * 它是折叠态胶囊的新家——壁纸场景画在 Explorer 图标层之下，表桌面收不到任何鼠标消息
 * （见 `docs/evidence/input-model-desktop-hit-testing.md`），所以折叠态的 UI 必须搬到一个
 * 独立顶层窗口里才点得着。原生侧的窗口策略（圆形区域、Z 槽夹在图标层与普通应用之间、
 * 永不抢前台、鼠标靠近才滑入）见 `src-tauri/src/floating_ball.rs`。
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
import { suppressNativeContextMenu } from '../runtime/contextMenu.ts'

export function BallWindow() {
  // 球也是壁纸的一部分：右键它不该弹出"图像另存为/更多工具"那一套网页菜单。
  useEffect(() => suppressNativeContextMenu(), [])
  return (
    <button
      type="button"
      className="ball"
      aria-label="打开 AI 输入岛"
      title="打开 AI 输入岛"
      onClick={() => {
        // 失败不弹任何界面：球的窗口没有能力显示错误，原生侧会留下日志。
        void invoke('enter_inner_workspace_from_ball').catch(() => undefined)
      }}
    />
  )
}
