/**
 * 屏蔽 WebView 的默认右键菜单。
 *
 * 壁纸不是网页：桌面上（尤其里桌面）右键会弹出 Chromium 给"网页图片"准备的那一套
 * ——图像另存为 / 复制图像 / 复制图像链接 / 更多工具——它把**背景插画**当成一张可保存的网图，
 * 而这既不是用户要做的事，也破坏了"这是一个桌面"的观感。用户明确要求冻结它。
 *
 * 两处例外，都是有意的：
 *
 * * **输入框里的右键菜单保留**：输入区的复制/粘贴在这里是真的有用，一起关掉是在惩罚用户；
 * * 只挂在壁纸表面上（背景宿主与悬浮球），设置中心是普通应用窗口，不动它。
 */

/** 哪些地方右键仍按普通输入控件处理。 */
const TEXT_ENTRY_SELECTOR = 'input, textarea, [contenteditable="true"]'

/**
 * 这次右键该不该被拦掉。
 *
 * 单独抽出来是因为它是一条**判断**，而 DOM 事件本身不好在测试里造：这里只要一个能回答
 * `closest` 的对象，于是"输入框里不拦、别处都拦"可以被直接钉住。
 */
export function shouldSuppressContextMenu(target: unknown): boolean {
  const element = target as { closest?: (selector: string) => unknown } | null | undefined
  if (element && typeof element.closest === 'function') {
    return element.closest(TEXT_ENTRY_SELECTOR) === null
  }
  // 没有可判断的目标（比如右键落在文档本身）时按"拦"处理：壁纸上没有任何地方需要那个菜单。
  return true
}

export function suppressNativeContextMenu(): () => void {
  const onContextMenu = (event: MouseEvent) => {
    if (!shouldSuppressContextMenu(event.target)) return
    event.preventDefault()
  }
  document.addEventListener('contextmenu', onContextMenu)
  return () => document.removeEventListener('contextmenu', onContextMenu)
}
