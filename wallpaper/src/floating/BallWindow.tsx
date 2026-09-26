/**
 * 悬浮球窗口（surface `ball`）的占位视觉。
 *
 * 增量 1 只做窗口本体与弹出/收回，所以这里**没有**任何交互：
 * 不引 `./App.tsx`、不 `invoke`、不订阅事件、不申请 capability。
 * 真正的胶囊内容——折叠态 `ConversationBubble`，以及 §1.1 拍板的
 * 「表桌面单击 = 进里桌面 + 弹输入岛」——在**增量 2** 搬进来。
 *
 * 原生侧的窗口策略（胶囊区域、Z 槽、永不激活）见 `src-tauri/src/floating_ball.rs`。
 */

/** 引入设计令牌，让占位胶囊在增量 2 之前就与产品同一套配色。
 *  它只是 CSS 变量，不牵任何组件（`ui/primitives` 才是有依赖的桶文件）。 */
import '../ui/tokens/tokens.css'
import './BallWindow.css'

export function BallWindow() {
  return (
    <div className="ball-surface">
      {/* 原生窗口区域已经裁成胶囊：圆角之外既画不出东西，也收不到鼠标。
          所以这里只在窗口范围内画，不外扩阴影（会被区域裁掉）。 */}
      <div className="ball-capsule" role="presentation" aria-hidden="true">
        <span className="ball-capsule__sigil" />
        <span className="ball-capsule__label">DSH</span>
      </div>
    </div>
  )
}
