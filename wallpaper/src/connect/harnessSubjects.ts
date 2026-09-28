/**
 * Wording for the harness execution subjects a user can choose.
 *
 * Kept beside `endpoints.ts` rather than inside the settings panel, because the
 * subject list, the launch result and the island's route switch all need the same
 * sentences and must not drift into three explanations of one outcome. The design
 * they implement is frozen in `docs/design/harness-subject-and-ui-design.md` §3–§5.
 *
 * Nothing here may carry a token or a raw exception. Paths are shown by the row
 * that owns them: a checkout's path *is* its identity, and the user asked for it
 * by adopting that directory.
 */
import type { HarnessLaunchOutcome, HarnessTarget } from '../native/runtime.ts'

/** The user-facing class name, which is also the reason the classes behave
 * differently: one carries its own checkout, another is a tree of your own, and the
 * third is a CLI installed on this machine. */
export function subjectKindLabel(kind: HarnessTarget['kind']): string {
  // The design's own terms are 自带检出 / 源码检出, and they are exact: the classes
  // differ in whether the client carries its *own* checkout. Measured against what a
  // user can see, though, "检出" names an implementation detail — what they are
  // choosing between is a client that brings its own runtime, a source directory of
  // their own, and (2026-09-27) a DSH CLI installed on this machine with npm. The
  // labels say that, and the word 检出 never appears in the settings surface.
  if (kind === 'embedded-shell') return '客户端'
  if (kind === 'installed-cli') return '已安装的 CLI'
  return '源码目录'
}

/**
 * Whether a stored subject id names a shell rather than a source tree.
 *
 * The id namespace is the native model's (`shell:<AUMID>`), and this is the one
 * place the renderer reads it, so a caller that needs the class without a scan
 * does not invent its own prefix test.
 */
export function isEmbeddedShellSubject(subjectId: string | undefined): boolean {
  return (subjectId ?? '').startsWith('shell:')
}

/**
 * Whether a stored subject id names the globally installed CLI.
 *
 * 这一类不是"另一种源码目录"：它启动时要经过一层 npm 的批处理外壳再拉起 node，冷启动明显比
 * 直接跑二进制慢，所以等待它的时间不该和别的类别一样长（见 `harnessLaunchOutcome` 的宽限）。
 * 前缀同样属于原生模型（`cli:<启动器路径>`），沿用上面那条"只在这里读一次"的规矩。
 */
export function isInstalledCliSubject(subjectId: string | undefined): boolean {
  return (subjectId ?? '').startsWith('cli:')
}

/**
 * The one line under a subject's name.
 *
 * A shell has nothing to configure — it brings its own checkout and its own data
 * — so saying that is more useful than repeating where its shortcut was found. A
 * checkout has exactly one useful line, and it is its path.
 */
export function subjectDetail(target: HarnessTarget): string {
  return target.kind === 'embedded-shell'
    // A shell needs no configuration at all, and saying so is the useful part: the
    // user would otherwise look for the path field we deliberately hid for it.
    ? '客户端自带运行环境，不需要填路径。'
    : target.identity.rootPath ?? target.source
}

/**
 * Whether a 「拉起 UI」 result means the browser is the only interface left.
 *
 * Decided from what native *did*, never from the subject's shape. The raise answers
 * `no-window` for exactly the clients that own no resolvable window, which is the
 * same question the shape was being asked — and the shape is *unknown* in the one
 * case that matters: a wallpaper with no subject stored has no shape to read, so the
 * renderer fell back to "windowless", opened a browser, and never asked for the
 * window that was there. That is what made the small icon do nothing for a wallpaper
 * whose subject was not configured (or configured as a tree).
 *
 * Everything else means a window was found: `raised`, or `raise-refused` when Windows
 * declined the foreground change while the window was still restored. Both are a
 * success for the user's purpose — "show me that client" — and neither needs a browser.
 */
export function reachNeedsBrowser(outcome: string): boolean {
  return outcome === 'no-window'
}

/**
 * The prompt for §4.3, or `null` when there is nothing to ask.
 *
 * More than one source tree means the wallpaper cannot know which one the user
 * means, and guessing would quietly connect the wallpaper to the wrong tree.
 */
export function subjectChoicePrompt(targets: readonly HarnessTarget[]): string | null {
  const checkouts = targets.filter((target) => target.kind === 'checkout')
  if (checkouts.length < 2) return null
  return `检测到您电脑上安装了 ${checkouts.length} 个 deepseek harness 源码树，请选择默认主体。`
}

/**
 * What to say after a launch attempt, or `null` when saying nothing is better.
 *
 * Takes a subset rather than a whole `HarnessLaunchOutcome` because 「拉起 UI」
 * reports the same start codes in its own shape: the words for "no profile", "no
 * Node", "port taken" are the same whichever action discovered them.
 */
/**
 * How long ago a scan confirmed the list, in words.
 *
 * The renderer owns this because it is the only side that knows "now": a persisted
 * timestamp shown without its age is exactly the "looks current" failure the
 * catalogue exists to prevent.
 */
export function catalogAgeLabel(verifiedAtMs: number, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - verifiedAtMs) / 60_000))
  if (minutes < 1) return '刚刚验证'
  if (minutes < 60) return `${minutes} 分钟前验证`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} 小时前验证`
  return `${Math.floor(hours / 24)} 天前验证`
}

/**
 * The Win32 extended-length prefix, as a canonical path comes back from native.
 *
 * It is a transport detail: the same directory is the same subject with or without
 * it, and showing it to the user only makes a path look broken.
 */
const VERBATIM_PREFIX = '\\\\?\\'

function withoutVerbatimPrefix(path: string): string {
  const trimmed = path.trim()
  return trimmed.startsWith(VERBATIM_PREFIX) ? trimmed.slice(VERBATIM_PREFIX.length) : trimmed
}

/** A path as the user should see it: no extended-length prefix, no trailing slash. */
export function displaySubjectPath(path: string | undefined): string {
  return withoutVerbatimPrefix(path ?? '').replace(/[\\/]+$/, '')
}

/**
 * Whether two stored subject ids name the same subject.
 *
 * Comparison is deliberately loose because a stored id outlives the scan that
 * produced it: an id written before the prefix above was stripped still names the
 * same directory, and Windows paths are case-insensitive. A loose comparison here is
 * what keeps a re-scan from silently un-selecting the user's choice.
 */
export function sameSubject(left: string | undefined, right: string | undefined): boolean {
  const a = displaySubjectPath(left).toLowerCase()
  const b = displaySubjectPath(right).toLowerCase()
  return a.length > 0 && a === b
}

/** The directory that contains a path, for telling two same-named trees apart. */
function parentName(path: string): string {
  const parts = withoutVerbatimPrefix(path).split(/[\\/]+/).filter(Boolean)
  return parts.length >= 2 ? parts[parts.length - 2] : ''
}

/** The last path segment — a directory's own name, which is what 起别名 defaults to. */
function baseName(path: string): string {
  const parts = withoutVerbatimPrefix(path).split(/[\\/]+/).filter(Boolean)
  return parts.length > 0 ? parts[parts.length - 1]! : ''
}

/**
 * 用户给这个主体起的别名，或空串。
 *
 * 只看存下来的那一条：**不做任何"猜一个更好听的名字"**。别名要么是用户写的，要么就是空 ——
 * 空的时候显示什么，由调用处按它知道的上下文决定（下拉里是上级目录名或仓库名）。
 */
export function subjectAlias(
  subjectId: string | undefined,
  aliases: Readonly<Record<string, string>> | undefined,
): string {
  const id = (subjectId ?? '').trim()
  if (id.length === 0) return ''
  return (aliases?.[id] ?? '').trim()
}

/** 主体 id 用来显示时的那一段名字（不含类别词、不含版本）。 */
function subjectName(
  target: HarnessTarget,
  all: readonly HarnessTarget[],
  aliases: Readonly<Record<string, string>> | undefined,
): string {
  if (target.kind !== 'checkout') return target.label
  // 别名的位置就是原来那一段"上级目录名"的位置：**顶掉**它，而不是加在它后面。两个同名克隆
  // 靠这一段区分，所以别名的作用正是"用我认得出来的名字来区分"，多留一段反而更长、更难认。
  const alias = subjectAlias(target.id, aliases)
  if (alias.length > 0) return alias
  // 同名是常态而不是边角：那时用上一级目录区分，并省掉恒定的仓库名，
  // 于是"源码目录 · deepseek-harness（DeepSeekHarness.old）"缩成"源码目录 · DeepSeekHarness.old"。
  const duplicated = all.filter((other) => other.label === target.label).length > 1
  const parent = duplicated && target.identity.rootPath ? parentName(target.identity.rootPath) : ''
  return parent || target.label
}

/**
 * The name segment of a subject's option label.
 *
 * Exported because the running-instance dropdown shows the *same* name: one subject must not read as
 * two different things depending on which control is looking at it.
 */
export function subjectDisplayName(
  target: HarnessTarget,
  all: readonly HarnessTarget[],
  aliases?: Readonly<Record<string, string>>,
): string {
  return subjectName(target, all, aliases)
}

/**
 * One option's text in the subject select.
 *
 * Two clones of the same project share their last path segment, which is the normal
 * case rather than an edge case — so a name that appears twice is qualified with its
 * parent directory. Without that, a user cannot tell the entries apart, which is what
 * a select is for. A stored 别名 sits in exactly that position (`源码目录 · <别名> · <版本>`),
 * so the *shape* of the line is unchanged: one name segment, then the version.
 */
export function subjectOptionLabel(
  target: HarnessTarget,
  all: readonly HarnessTarget[],
  aliases?: Readonly<Record<string, string>>,
): string {
  // 版本号跟在名字后面，因为它回答的是同一类问题 —— "这一条是谁"：0.2.0-rc.1 的客户端与
  // 0.1.0-rc.5 的源码树能不能接上同一个 bridge，答案并不相同。读不到版本时**一个字都不加**：
  // 写"未知"会让用户以为我们查过这一条（见 `HarnessTarget.version`）。
  const version = target.version?.trim()
  const suffix = version ? ` · ${version}` : ''
  // 客户端与 CLI 的名字本身已经说清自己是什么（"官方桌面客户端"/"DSH CLI"），再冠一次类别词就是
  // 同一件事说两遍 —— 用户实测点名了这一点。只有源码目录的名字是个仓库名，必须带类别词。
  if (target.kind !== 'checkout') return `${target.label}${suffix}`
  return `源码目录 · ${subjectName(target, all, aliases)}${suffix}`
}

/**
 * 一个**正在运行的实例**在下拉里那一段名字。
 *
 * 与「运行方式」用同一套名字规则（`subjectDisplayName`），差别只在兜底：这里的实例可能来自上一次
 * 扫描之后就已经消失的目录，那时没有任何 `HarnessTarget` 可以问，只能用 id 自己最后一段 ——
 * "叫得出名字"是这一行的全部用途，而一个读不出名字的实例对用户毫无帮助。
 */
export function instanceDisplayName(
  subjectId: string,
  targets: readonly HarnessTarget[],
  aliases?: Readonly<Record<string, string>>,
): string {
  const id = subjectId.trim()
  const target = targets.find((candidate) => sameSubject(candidate.id, id))
  if (target) return subjectDisplayName(target, targets, aliases)
  const alias = subjectAlias(id, aliases)
  if (alias.length > 0) return alias
  if (isInstalledCliSubject(id)) return 'DSH CLI'
  return baseName(id) || id
}

/**
 * 实例下拉里的一整行：`别名 · 端口`。
 *
 * 端口那一段读不出来时写「端口未确认」，而不是写 0 或干脆省掉：省掉会让这一行看起来像"这个实例
 * 没有端口"（`--port 0` 让系统挑一个，真的是这样），而大多数时候只是**还没确认到**（宿主起得比
 * 端口登记早）。两种情况的下一步并不相同，所以它们不能长得一样。
 */
export function instanceLabel(
  subjectId: string,
  port: number | undefined,
  targets: readonly HarnessTarget[],
  aliases?: Readonly<Record<string, string>>,
): string {
  return `${instanceDisplayName(subjectId, targets, aliases)} · ${port ?? '端口未确认'}`
}
export function launchOutcomeNotice(outcome: {
  outcome: string
  kind: HarnessLaunchOutcome['kind']
  hidden?: boolean
}): string | null {
  // One noun per class, the same two the subject list uses, so a result line and the
  // row it refers to cannot read as two different things.
  const label = outcome.kind === 'embedded-shell' ? '客户端' : '源码目录'
  switch (outcome.outcome) {
    case 'started':
      // A hidden start is the one case where "started" is not the whole story:
      // the user would otherwise go looking for a window that is deliberately not
      // on screen yet.
      return outcome.hidden
        ? `已启动${label}，窗口已在后台；需要用「拉起窗口」把它调出来。`
        : `已启动${label}。`
    case 'started-unconfirmed':
      return `${label}接受了启动请求，但在超时时间内没有应答；它可能仍在启动，稍后刷新即可。`
    case 'already-running':
      return `${label}已在运行；本应用不会接管、重启或停止它。`
    case 'unknown-target':
      // "执行主体" is this plan's word for what runs the Harness; a user has no
      // reason to learn it, so the sentence names the missing act instead.
      return '还没有选好要启动谁，请先扫描并选择一个。'
    case 'root-path-invalid':
      // Names what is wrong with the *thing the user picked*, and the one action
      // that fixes it. A path is deliberately not repeated here: the row they
      // clicked already shows it.
      return '这个目录不是可用的 DSH 源码目录，请重新扫描后选择。'
    case 'launcher-missing':
      // The only entry that cannot avoid a system name: the fix is to install one of
      // two programs or point at their location, so naming them is the actionable
      // half, while PATH stays out of the sentence. 这一项**不再**把用户引向「启动命令」——
      // 那个设置已经不在了（现在只有「启动参数」，而它加不了参数以外的任何东西），
      // 指向一个不存在的入口比不说更坏。
      return '找不到用来启动它的程序：请确认 Node.js 或 pnpm 已安装，并能在命令提示符里直接运行。'
    case 'profile-invalid':
      return 'profile 无效：只能包含字母、数字、连字符或下划线。'
    case 'port-occupied-external':
      // The port number is an implementation fact; the user's situation is that
      // something is already running and this application is leaving it alone.
      return '本机已有别的程序占用该端口（不是本应用启动的），因此没有重复启动，也不会去接管或停止它。在「启动参数」里换一个端口可以并行再起一个实例。'
    default:
      // No code, no path: the log does name both, and that is where a bug report
      // should come from rather than from a dialog.
      return '启动没有成功。日志里有这次启动的完整记录，可用于排查。'
  }
}
