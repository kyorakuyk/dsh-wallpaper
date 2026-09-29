/**
 * 启动一个主体时用它哪个档案 —— 由我们决定，不由用户填。
 *
 * 为什么不再给用户填（2026-09-30 用户决定隐藏该设置项）：
 *
 * * **官壳**用它自己独占的 `desktop`，我们给什么都没用（它靠 AUMID 激活，启动函数里根本没有档案参数）；
 * * **TUI** 用它自己的 `dsh-tui`，我们传反而是在替它做决定（它的 bin 里连 argv 都不读端口）；
 * * **已安装 CLI 与源码检出**必须显式给一个档案（实测：不写就是 `error: --profile <name> is required`），
 *   而在 CLI 出货的档案里，**只有 `web` 是"能起来并且提供 HTTP"的那一个**：
 *   `desktop` 被壳独占（实测报 `profile "desktop" is managed exclusively by the Electron application`），
 *   其余 `acp`/`headless`/`sdk`/`sdk-minimal` 都是 stdio 形态、**不监听 HTTP**，壁纸的桥就没有宿主可挂。
 *
 * 也就是说这个值本来就不是"用户的偏好"，而是"我们选哪条启动链"的结果 —— 既然只有一个正确答案，
 * 把它放在这里，而不是放在一个会被填错的输入框里。
 *
 * 顺带说明它与两处耦合的关系（都在原生侧）：`--no-open` 只在 web 上加（那是 web 应用自己的旗标），
 * 网页交接也只在这条路上做（只有 web 会打印 `http://127.0.0.1:<port>/?token=…`）。既然我们现在总是
 * 走 web，这两条永远成立。
 */
export const SUBJECT_PROFILE = 'web' as const

/** 启动某个主体时传给启动器的档案名。壳与 TUI 忽略它，但我们仍然显式给一个值，方便日志对照。 */
export function profileForLaunch(): string {
  return SUBJECT_PROFILE
}
