// 归档片段（B6，2026-10）：`wallpaper/src/settings/SettingsWindow.tsx` 中「启动参数」（`parseLaunchArgs` import、「打开界面」与 TUI 的参数、`onSelectLaunchArgs`）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 为便于定位，`args:` 那几处连同所在调用一起列出：调用里不带 FREEZE 说明的其余各行是上下文，仍在现役文件里，不是迁出内容。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/frozen/launch-ui/README.md`。

// ---- 原 27-33 行：import 区，`connect/harnessProfiles.ts` 的 import 之后、`./SettingsPanel.tsx` 之前 ----
// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：本窗口里「启动参数」的分词只服务两处 —— 传给 `ensureHarnessUi` 的
// `args`，以及 `openSubjectTui` 的 `args`。两处都冻住了（各有一处 FREEZE 注释），所以这一行也随之
// 冻住。`connect/launchArgs.ts` 本身一行都没动；原生侧 `args` 都是可选参数，不传就是空参数列表。
// 怎么恢复：取消这一行，并取消那两处 `args:` 的注释。
// ---------------------------------------------------------------------------
import { parseLaunchArgs } from '../connect/launchArgs.ts'

// ---- 原 461-463 行：「打开界面」那条路的 `ensureHarnessUi` 调用内，`profile: profileForLaunch(),` 之后（上下文：原 457-464 行） ----
      const ensured = await nativeRuntime.ensureHarnessUi({
        targetId: subjectId,
        port,
        profile: profileForLaunch(),
        // FREEZE（临时冻结，不是删除）：「打开界面」这条路不带任何启动参数。恢复办法：取消下面
        // 这一行，并恢复本文件顶部的 `parseLaunchArgs` import。
        args: parseLaunchArgs(current.args),
      })

// ---- 原 1095-1098 行与 1116-1128 行：`<SettingsPanel …>` 的 props 内，`onStopAllManagedDsh` 之后、`onOpenTui` 之前 ----
// （1095-1098 行那段 FREEZE 说明同时管「起别名」与「启动参数」两个 handler，两份归档各留一份；
// 1099-1115 行的 `onSelectSubjectAlias` 在 `archive/frozen/instance-ui/` 里。）
      // FREEZE（临时冻结，不是删除）：「起别名」与「启动参数」两个 handler。它们的输入控件冻住了
      // （`SettingsPanel.tsx` 里对应的 Field 都注释了），所以这里也一起冻 —— 留着就是两段永远
      // 不会跑的回调，而"改了没反应"是比"没有这个入口"更难懂的状态。
      // 这一段里的规则本身一行都没改，恢复办法就是取消这一整块的注释。
      onSelectLaunchArgs={(value) => {
        const args = value.trim() ? value : undefined
        change({
          ...settingsRef.current,
          dshLaunch: {
            ...settingsRef.current.dshLaunch,
            args,
            // 换参数就清掉显式端口 pin：那条 pin 是"上一次启动选的那个端口"，参数已经把它推翻了。
            // 留着它，「打开界面」会去敲上一代端口（与"换主体就清 pin"是同一条理由）。
            endpointPort: undefined,
          },
        })
      }}

// ---- 原 1130-1132 行：`onOpenTui` 回调开头的 FREEZE 说明（原 1129 行的无参调用 `openSubjectTui()` 仍在现役） ----
        // FREEZE（临时冻结，不是删除）：这里原来把「启动参数」分好词再交给 TUI（`openSubjectTui(
        // parseLaunchArgs(settingsRef.current.dshLaunch.args))`）。这一版不带参数。恢复办法：把那个
        // 实参加回去，并恢复本文件顶部的 `parseLaunchArgs` import。
// 恢复形态（3c92772 里这里只有上面的说明；下面这一行逐字取自冻结前的 commit a8e2e91）：
      onOpenTui={() => void nativeRuntime.openSubjectTui(parseLaunchArgs(settingsRef.current.dshLaunch.args)).then((result) => {
