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

/** The user-facing class name, which is also the reason the two classes behave
 * differently: one carries its own checkout, the other does not. */
export function subjectKindLabel(kind: HarnessTarget['kind']): string {
  return kind === 'embedded-shell' ? '自带检出（客户端）' : '源码检出'
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
 * The one line under a subject's name.
 *
 * A shell has nothing to configure — it brings its own checkout and its own data
 * — so saying that is more useful than repeating where its shortcut was found. A
 * checkout has exactly one useful line, and it is its path.
 */
export function subjectDetail(target: HarnessTarget): string {
  return target.kind === 'embedded-shell'
    ? '自带检出与服务，不需要配置路径或 profile。'
    : target.identity.rootPath ?? target.source
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

/** What to say after a launch attempt, or `null` when saying nothing is better. */
export function launchOutcomeNotice(outcome: HarnessLaunchOutcome): string | null {
  const label = outcome.kind === 'embedded-shell' ? '客户端' : '源码检出'
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
      return '没有可启动的执行主体，请先扫描并选择一个。'
    case 'root-path-invalid':
      return '源码检出目录不可识别，请重新扫描后选择。'
    case 'launcher-missing':
      return '未找到启动器：Node.js / pnpm 不在 PATH 中，或自定义启动器路径不存在。'
    case 'profile-invalid':
      return 'profile 无效：只能包含字母、数字、连字符或下划线。'
    case 'port-occupied-external':
      return '3080 端口已被其他 DSH 占用；本应用不会接管或停止它。'
    case 'command-not-confirmed':
      return '自定义启动命令尚未获得自动启动授权，因此没有执行。'
    default:
      return '启动失败，请查看日志中的启动记录。'
  }
}
