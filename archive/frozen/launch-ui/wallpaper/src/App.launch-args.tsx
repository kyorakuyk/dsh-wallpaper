// 归档片段（B6，2026-10）：`wallpaper/src/App.tsx` 中「启动参数」（三个启动入口的 `args:` 与 `parseLaunchArgs` import）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 为便于定位，`args:` 那几处连同所在调用一起列出：调用里不带 FREEZE 说明的其余各行是上下文，仍在现役文件里，不是迁出内容。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/frozen/launch-ui/README.md`。

// ---- 原 12-20 行：import 区，`connect/endpoints.ts` 的 import 之后、`connect/harnessLabels.ts` 之前 ----
// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：「启动参数」把分词结果交给启动链，所以这一行随那个功能一起冻住。
// 为什么关：本 build 有意回到该功能之前的行为 —— 三个启动入口都不再携带任何参数（见下面三处
// FREEZE 注释）。参数为空时原生侧的行为与加这个功能之前逐字相同，所以只关调用处、不动原生。
// 怎么恢复：取消注释这一行与那三处 `args:`，再取消 SettingsPanel / SettingsWindow 里同名的
// 冻结块（「启动参数」行、实例下拉、每实例停止）。`connect/launchArgs.ts` 本身一行都没动，
// 它仍然在 `runtime.ts` 里为端点镜子供词（探针与「打开界面」盯同一个端口），分词测试也照常跑。
// ---------------------------------------------------------------------------
import { parseLaunchArgs } from './connect/launchArgs.ts'

// ---- 原 1044-1047 行：手动「打开界面」那条路的 `ensureHarnessUi` 调用内，`profile: launch.profile,` 之后（上下文：原 1040-1048 行） ----
        const ensured = await nativeRuntime.ensureHarnessUi({
          targetId: subjectId,
          port,
          profile: launch.profile,
          // FREEZE（临时冻结，不是删除）：「启动参数」不在这条路上传。恢复办法：取消注释下面
          // 这一行，并恢复本文件顶部的 `parseLaunchArgs` import。原生侧 `args` 是可选参数，
          // 不传等价于空参数列表 —— 也就是这个功能之前的行为。
          args: parseLaunchArgs(launch.args),
        })

// ---- 原 1500-1502 行：随壁纸自动启动的 `autostartHarnessTarget` 调用内，`profile: profileForLaunch(),` 之后（上下文：原 1497-1503 行） ----
        const result = await nativeRuntime.autostartHarnessTarget({
          targetId: subjectId,
          profile: profileForLaunch(),
          // FREEZE（临时冻结，不是删除）：随壁纸自动启动这条路上也不带任何参数 —— 它和手动
          // 「启动」跑的是同一个启动器，所以两条路一起冻结。恢复办法：取消注释这一行。
          args: parseLaunchArgs(settings.dshLaunch.args),
        })

// ---- 原 2118-2120 行：手动「启动」的 `launchHarnessTarget` 调用内，`profile: profileForLaunch(),` 之后（上下文：原 2115-2121 行） ----
          await nativeRuntime.launchHarnessTarget({
            targetId: subjectId,
            profile: profileForLaunch(),
            // FREEZE（临时冻结，不是删除）：手动「启动」这条路同样不带参数。恢复办法：取消
            // 注释这一行。原生侧 `args?` 是可选参数，缺省就是空参数列表。
            args: parseLaunchArgs(settings.dshLaunch.args),
          })
