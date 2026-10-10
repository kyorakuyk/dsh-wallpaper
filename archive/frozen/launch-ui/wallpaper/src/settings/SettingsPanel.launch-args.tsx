// 归档片段（B6，2026-10）：`wallpaper/src/settings/SettingsPanel.tsx` 中「启动参数」（`launchArgsIssue` import、`onSelectLaunchArgs` prop、`argsIssue`、「启动参数」Field）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/frozen/launch-ui/README.md`。

// ---- 原 25-31 行与 34 行：import 区，`./AppearancePreview.tsx` 之后（25-31 行的说明管三个 import，两份归档各留一份；
// 32 行那条带 `instanceLabel` / `subjectAlias` 的 import 在 `archive/frozen/instance-ui/` 里） ----
// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：「起别名」与实例下拉被冻在这个 build 之外，所以它们要的两样东西
// 也一起冻住 —— `instanceLabel` 只给实例下拉的行文字用，`subjectAlias` 只给「起别名」输入框回显用，
// `launchArgsIssue` 只给「启动参数」那行的校验提示用。三个函数本身一行都没动（
// `connect/harnessSubjects.ts` / `connect/launchArgs.ts` 里的纯逻辑与它们的测试照常跑）。
// 怎么恢复：取消注释下面两个 import，再去掉本文件里对应的三处 FREEZE 注释。
// ---------------------------------------------------------------------------
import { launchArgsIssue } from '../connect/launchArgs.ts'

// ---- 原 153-166 行：`SettingsPanelProps` 内，`tuiAvailable` 之后、`reachAction` 之前（153-165 行的说明管四个 prop，两份归档各留一份；
// 167-168 行的 `onSelectSubjectAlias` / `onStopManagedInstance` 在 `archive/frozen/instance-ui/` 里） ----
  /**
   * FREEZE（临时冻结，不是删除）：「启动参数」的输入框不在这一版里，所以它的三个 prop 与
   * 「起别名」的那一个也一起冻住。
   *
   * 为什么关：本 build 有意回到 a8e2e91 之前的行为 —— 界面上没有「启动参数」行、没有「起别名」
   * 行，也没有标题右上角的实例下拉，启动链不接受任何参数（`App.tsx` / `SettingsWindow.tsx` 里
   * 三个入口都冻结了）。留着一个改了没用的输入框，比它不在更坏。
   *
   * 为什么标成可选而不是删掉：这样 `SettingsWindow` 给不给都不算类型错误，而恢复时两边一起取消
   * 注释就行 —— 这也是 `LayoutProbe` 那套"冻结就注释掉、复活就打开"的做法。
   *
   * 怎么恢复：取消注释这四个 prop，并在 `SettingsWindow.tsx` 里恢复对应的两个 handler 与两处传参。
   */
  onSelectLaunchArgs: (args: string) => void

// ---- 原 660-670 行：`SettingsPanel` 函数体内，`cliSelected` 之后、`openRoutes` 之前 ----
  /**
   * FREEZE（临时冻结，不是删除）：「启动参数」那一串现在能不能用。
   *
   * 判据是"启动器会不会收到一个它理解不了的词"，而不是"这串字好不好看"：条数、长度、控制字符。
   * 有意见时那句话**顶掉**用法说明 —— 一行同时说两件事，用户只会读到第一件。
   *
   * 为什么关：这一版没有「启动参数」输入框（见下面那条 Field 的 FREEZE 注释），所以没有东西
   * 可以把校验结果显示出来；一个算了没人看的变量在本项目里会被 `noUnusedLocals` 拦下。
   * 恢复办法：取消注释这一行，并把下面的 Field 一起打开（`launchArgsIssue` 的规则一行都没动）。
   */
  const argsIssue = launchArgsIssue(settings.dshLaunch.args)

// ---- 原 902-905 行：连接卡片 `{!shellSelected && <>` 开头那段 JSX 说明（原 898-906 行）的后半段，只有说明、没有代码 ----
// （前半段讲"源码目录"那几项，仍在现役；这后半段讲「启动参数」为什么取代「启动命令」，随输入框一起迁出。）
            {/*
              关于「启动参数」：它取代了原来的「启动命令」——不是换了措辞，而是**收掉了一项能力**
              （原来的框里可以填任意一个程序、由壁纸去执行它；「启动参数」只能往我们自己选定的那个
              启动器后面加词，于是"自动启动要不要用这个自定义命令"那一次授权也不再需要）。本 build
              把这一项整个冻住，见下面那个输入框上的 FREEZE 注释。
            */}

// ---- 原 930-952 行：连接卡片 `{!shellSelected && <>…</>}` 内，「数据档案」隐藏说明（原 929 行）之后、`</>}` 之前 ----
            {/* FREEZE（临时冻结，不是删除）：「启动参数」行。
                为什么关：本 build 有意回到这个功能之前的行为 —— 没有参数这一项，启动链也不接受
                参数（`App.tsx` 里三个入口、`SettingsWindow.tsx` 里两个入口都冻结了）。界面上留着
                一个改了没用的框，比它不在更坏；而它旁边的说明还在讲端口与并行实例，那些话在
                单实例的世界里只会让人以为改得动。
                怎么恢复：取消下面这个 Field 的注释，并恢复 `props.onSelectLaunchArgs` 那个 prop、
                本文件顶部的 `launchArgsIssue` import，以及下面那条 `argsIssue` 计算
                （`connect/launchArgs.ts` 本身一行都没动，分词与读端口的测试照常跑）。
                校验逻辑的落点也留档在这里：`launchArgsIssue(settings.dshLaunch.args)`。 */}
            <Field
              title="启动参数"
              detail={launchArgsIssue(settings.dshLaunch.args) ?? `追加到启动器后面的参数，例如 --port 3081。留空就用默认端口；参数按你写的原样传递，不经过命令行解释器（引号只在这里解释一次）。TUI 没有端口概念。目前支持的组合是官方桌面客户端加一个实例；换端口不隔离会话与工作区（隔离单位是 DSH_HOME，不是端口），再起第二个实例会与它共用同一份会话与工作区记录。`}
            >
              <input
                value={settings.dshLaunch.args ?? ''}
                placeholder="留空时使用默认启动方式"
                aria-label="启动参数"
                maxLength={512}
                onChange={(e) => props.onSelectLaunchArgs(e.target.value)}
              />
            </Field>
