/**
 * 输入岛左下角那枚「这段对话跑在谁身上」的指示器。
 *
 * 它回答用户提出的两个分不清：
 *
 *  1. 滑槽在最左端时两种后端的界面长得一样，用户看不出这次花的是 DeepSeek 网页额度还是自己的 API key；
 *  2. 切到 harness 时，装了多个 DSH 的用户看不出这一次拉起的是哪个界面。
 *
 * 所以取值只有 `Web` / `API` / `Desktop` / `TUI` 四个短词，解释放进 `title`：元素本身不可点，
 * 能承载的只有"悬停看一眼"。短词沿用用户自己的说法（`Web`/`API`、`Desktop`/`Web`/`TUI`），
 * 不翻译成中文 —— 这一个位置只有几个字符宽，而词汇表越短越不会被读错。
 *
 * harness 那一侧的判据与设置中心「打开」那张卡片同源（`SettingsPanel` 的 `openRoutes`）：
 * 客户端带来自己的窗口，只有已安装的 CLI 真有浏览器与终端两条路，源码目录只有浏览器一条。所以
 * 它问的其实是"这次会拉起哪个界面"。**已定案（2026-09-30，施工文档 §7.6 走法 A）**：这里继续
 * 按**主体的 DSH** 判，不改成按"谁在服务这段对话"判 —— 壳主体的宿主无论是它自带的 CLI
 * （壁纸在后台起的）还是壳自己（用户点「打开」之后），服务的都是"客户端那个 DSH"，
 * 所以 `Desktop` 两种情况都对。
 */
import type { BackendMode } from '../domain/types.ts'
import { subjectKindLabel, subjectKindOf } from './harnessSubjects.ts'

/** 四个短词，也是这个元素全部可能显示的内容。 */
export type ConversationHostText = 'Web' | 'API' | 'Desktop' | 'TUI'

export interface ConversationHostChip {
  text: ConversationHostText
  /** 悬停时的解释。只在这里出现，元素上不显示。 */
  title: string
}

export interface ConversationHostInput {
  backend: BackendMode
  /** 存下来的主体 id（`shell:<AUMID>` / `cli:<启动器路径>` / 源码目录）。 */
  subjectId: string | undefined
  /** 设置里「打开」选的路线；只有在真的存在两条路时它才作数，判据见 `SettingsPanel`。 */
  window: 'browser' | 'tui' | undefined
  /** 用户给这个主体起的别名；没有就不写进解释里。 */
  alias?: string
}

/**
 * 一枚指示器，或它的解释文本。
 *
 * `alias` 只对 harness 有意义，而且只在用户真的起过名字时出现：装了多个同类主体时，
 * 类别词分不开它们，别名才是用户认得出的那一句。
 */
export function conversationHostChip(input: ConversationHostInput): ConversationHostChip {
  const alias = (input.alias ?? '').trim()
  const withAlias = (sentence: string): string =>
    alias.length > 0 ? `${sentence}（别名：${alias}）` : sentence

  if (input.backend === 'deepseek-api') {
    return { text: 'API', title: '你自己的 DeepSeek API key，按 token 计费。' }
  }
  if (input.backend !== 'harness') {
    return { text: 'Web', title: 'DeepSeek 网页额度，不产生 API 费用。' }
  }

  const kind = subjectKindOf(input.subjectId)
  if (kind === 'embedded-shell') {
    // 客户端自带窗口，所以它没有"界面在浏览器里"这一层可讲。
    return { text: 'Desktop', title: withAlias('本机 DeepSeek Harness 客户端，它带自己的窗口。') }
  }
  if (kind === 'installed-cli' && input.window === 'tui') {
    return { text: 'TUI', title: withAlias(`${subjectKindLabel(kind)}，界面在终端里。`) }
  }
  return { text: 'Web', title: withAlias(`${subjectKindLabel(kind)}，界面在浏览器里。`) }
}
