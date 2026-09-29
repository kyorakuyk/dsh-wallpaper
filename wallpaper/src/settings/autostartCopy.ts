import type { AutostartStatus } from '../native/runtime.ts'

/**
 * Copy for the wallpaper's own Windows autostart, driven by what Rust read back
 * from the system.
 *
 * The renderer is not allowed to decide *why* a change did not take effect: it
 * cannot see the startup task, the registry or the package identity, and when
 * it guessed, every refusal became the same sentence about 系统启动应用权限 — a
 * settings page the user had already checked. Rust knows the cause and sends it
 * as `reason`; these two helpers only decide how to show it.
 */

/**
 * Whether this is a state Windows reported, rather than this page's own
 * placeholder before the first read comes back.
 *
 * Rust never reports `none` without a reason: every "nothing is in effect" path
 * says what it looked at. So `none` with no reason is the renderer's "还没读到"
 * marker, and a warning must not be raised from it — the 常规 page warned
 * 「壁纸开机自启未生效」 for an autostart that was on, purely because nobody had
 * opened the 系统 page yet and its probe is the only thing that reads the state.
 */
export function autostartKnown(status: AutostartStatus): boolean {
  return status.source !== 'none' || status.reason !== null
}

/** Which path is carrying autostart right now, or why none is. */
export function autostartDetail(status: AutostartStatus): string {
  const cause = status.reason ? ` ${status.reason}` : ''
  switch (status.source) {
    case 'startup-task':
      return status.enabled
        ? '登录后由 Windows 启动任务启动本应用。'
        : `Windows 启动任务未启用。${cause}`.trim()
    case 'run':
      // The compatibility entry is the normal path for an unpackaged build and
      // the fallback for a package Windows refuses to register a task for; in
      // both cases it really is what starts the app at logon, so say so.
      return `登录后由当前用户启动项启动本应用。${cause}`.trim()
    case 'disabled-by-user':
      return status.reason ?? 'Windows 已禁用本应用的开机启动。'
    case 'disabled-by-policy':
      return status.reason ?? 'Windows 策略禁止本应用开机启动。'
    case 'unsupported':
      return status.reason ?? '当前系统不支持本应用的开机自启。'
    default:
      // Rust always names a cause when it reports "nothing is in effect"; a
      // missing one means this page has not heard back yet.
      return status.reason ?? '正在读取 Windows 的启动状态…'
  }
}

/**
 * What to say when Windows ended up in a different state than the user asked
 * for. `null` means the change took effect and nothing needs saying.
 *
 * The specific reason is always preferred over the generic instruction, and the
 * instruction survives only for the case where Rust genuinely had none.
 */
export function autostartRefusalNotice(status: AutostartStatus, requested: boolean): string | null {
  if (status.enabled === requested) return null
  const wanted = requested ? '打开' : '关闭'
  if (status.reason) return `开机自启没有${wanted}：${status.reason}`
  return `开机自启没有${wanted}；Windows 没有接受这次变更，请在系统设置的「启动应用」里检查本应用。`
}
