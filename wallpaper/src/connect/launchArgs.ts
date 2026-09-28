/**
 * 「启动参数」：追加到启动器后面的额外参数。
 *
 * 这一项取代了原来的「启动命令」。区别不是措辞，而是**能力**：`command` 允许用户指定任意一个
 * 程序，壁纸会去执行它；`args` 只能往**我们自己选定的**那个启动器后面加词，启动器身份始终是
 * 扫描决定的（`harness_launch.rs` 的 `installed_cli_command` / 受管链）。用户的要求正是如此：
 * "从那个框里跑任意程序，是项目决定不要的能力；追加参数则让启动器身份仍然是我们的。"
 *
 * 因此这里的两个函数只做**分词**与**读端口**，绝不是命令行解释器：
 *
 * * 没有变量展开、没有通配符、没有子 shell、没有 `;` `|` `&` 这些控制符的语义 —— 它们只是
 *   普通的字符串，会被原样交给启动器；
 * * 结果是一份 `argv` 数组（原生侧 `Command::args` 收的那种），不是一个命令行字符串，所以
 *   引号与空格的解释只发生在这里这一次，不会再有第二次（两次解释就是注入）。
 *
 * 分词放在渲染层，是为了让"用户写的这一段到底是什么"只有一个答案：原生收到的已经是数组。
 * 「读端口」在原生侧还有一份（`harness_launch::port_from_args`），因为那条端口占用检查必须在
 * 原生自己做 —— 两份都由测试用同一组例子钉住，和 `SHELL_SUBJECTS` 与 `SHELL_APPS` 那对
 * 必须一致的常量是同一个理由。
 */

/** 参数条数上限。够写端口、host、路径，又不足以拼出一条完整的命令行。 */
export const MAX_LAUNCH_ARGS = 32
/** 单个参数长度上限。超过这个长度的"参数"更可能是误粘贴。 */
export const MAX_LAUNCH_ARG_LENGTH = 512

/**
 * 把一个动词串切成 argv。
 *
 * 规则刻意只有三条，因为每多一条就多一种"用户以为会这样、其实那样"的写法：
 * 1. 空白（空格、制表、换行）分隔参数；
 * 2. 双引号包住的一段算一个参数，其中 `\"` 表示一个字面双引号、`\\` 表示一个字面反斜杠；
 * 3. 单引号包住的一段算一个参数，内部**不做任何转义**（与双引号区分开，这样 Windows 路径里的
 *    反斜杠不必写两遍）。
 *
 * 未闭合的引号**不报错、也不吞掉整行**：它按已读到的那一段结束。理由是这个字段是随打随存的，
 * 每敲一个引号就弹一次错，用户会在打字中途看到一次假的失败；而"少了一个收尾引号"这种输入，
 * 按已读内容执行与按报错拒绝相比，前者至少是用户看得懂的行为。
 */
export function parseLaunchArgs(raw: string | undefined): string[] {
  const text = raw ?? ''
  const args: string[] = []
  let current = ''
  let started = false
  let quote: '"' | "'" | undefined
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index]!
    if (quote === "'") {
      if (character === "'") quote = undefined
      else current += character
      started = true
      continue
    }
    if (quote === '"') {
      if (character === '\\' && (text[index + 1] === '"' || text[index + 1] === '\\')) {
        current += text[index + 1]
        index += 1
      } else if (character === '"') {
        quote = undefined
      } else {
        current += character
      }
      started = true
      continue
    }
    if (character === '"' || character === "'") {
      quote = character
      started = true
      continue
    }
    if (/\s/.test(character)) {
      if (started) args.push(current)
      current = ''
      started = false
      continue
    }
    current += character
    started = true
  }
  if (started) args.push(current)
  // 空串参数没有任何用处，却会变成启动器上一个莫名其妙的空词，所以滤掉。
  return args.filter((argument) => argument.length > 0)
}

/**
 * 这一串参数能不能用；能用就是 `null`。
 *
 * 控制字符被拒绝是**功能需要**，不是洁癖：原生侧用 `\u{1f}` 把主体 id 与参数拼成"实例键"，
 * 参数里再出现同一个字符就会让两个不同的实例撞成一个键 —— 那正好是"并行实例"这个功能的反面。
 */
export function launchArgsIssue(raw: string | undefined): string | null {
  const args = parseLaunchArgs(raw)
  if (args.length > MAX_LAUNCH_ARGS) return `启动参数最多 ${MAX_LAUNCH_ARGS} 个。`
  for (const argument of args) {
    if (argument.length > MAX_LAUNCH_ARG_LENGTH) return `单个启动参数不能超过 ${MAX_LAUNCH_ARG_LENGTH} 个字符。`
    // eslint-disable-next-line no-control-regex
    if (/[\u0000-\u001f\u007f]/.test(argument)) return '启动参数里不能包含控制字符。'
  }
  return null
}

/**
 * 参数里写的监听端口，或者 `undefined`。
 *
 * 只认 `--port 3081` 与 `--port=3081` 两种写法，因为这是 DSH 自己的 web 应用真正接受的形状
 * （实测 `dsh web --help`：`--port <port>  listen port; pass 0 to let the OS pick a free one`）。
 * Verbatim 的可信度靠这一条：**读不到就说读不到**，绝不去猜一个端口 —— 猜出来的端口会让
 * 「打开界面」去敲一扇没人应门的窗。
 *
 * 刻意只认**第一个** `--port`：重复给同一个旗标时，用户想要的是哪一个已经无法从这里判断，
 * 而把最后一个当作答案正是"参数解析器替你猜"的那种行为。
 */
export function launchPortFromArgs(args: readonly string[]): number | undefined {
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index]!
    const inline = /^--port=(.+)$/.exec(argument)
    const value = inline ? inline[1]! : argument === '--port' ? args[index + 1] : undefined
    if (value === undefined) continue
    return usablePort(value)
  }
  return undefined
}

/** 字符串形式的端口，只有在真的是一个可用端口时才返回数字。 */
function usablePort(value: string): number | undefined {
  if (!/^\d+$/.test(value)) return undefined
  const port = Number(value)
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : undefined
}

/** 用户设置的这一段参数里声明的端口。 */
export function launchSettingsPort(rawArgs: string | undefined): number | undefined {
  return launchPortFromArgs(parseLaunchArgs(rawArgs))
}
