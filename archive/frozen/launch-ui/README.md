# 归档：「启动参数」前端（launch-ui）

## 来源
- 来源 commit：`3c92772`（迁移前 tag：`pre-freeze-isolation`）；下文"原 N-M 行"均指该 commit 中的文件。
- 迁出批次：冻结代码归档隔离 B6（2026-10）。功能本身由 `a8e2e91` 加入、`07c7050` 冻结（入口注释掉）；B6 只把那些注释掉的入口移出现役源码，不改任何现役逻辑。

## 归档原因
「启动参数」在这个 build 里整体冻结：三个启动入口都不带参数，设置窗口里没有输入框，相关代码只以注释形式留在现役文件里。B6 把这些注释移到这里，现役 `wallpaper/src` 只保留真正运行的代码。剥掉注释后，被改的现役文件与迁移前逐字相同。

## 原路径与行范围 → 归档片段
片段是普通代码（去掉了 `//` 与 `{/* */}` 外壳），每段前有 `// ---- 原 N-M 行：位置 ----`；FREEZE 说明保留为注释。为便于定位，`args:` 那几处连同所在调用一起列出，调用里其余各行是上下文（仍在现役文件里）。

| 原位置（3c92772） | 内容 | 片段 |
|---|---|---|
| `wallpaper/src/App.tsx` 12-20 | `parseLaunchArgs` import 及其 FREEZE 说明 | `wallpaper/src/App.launch-args.tsx` |
| `wallpaper/src/App.tsx` 1044-1047 | 「打开界面」`ensureHarnessUi` 的 `args: parseLaunchArgs(launch.args)` | 同上 |
| `wallpaper/src/App.tsx` 1500-1502 | 随壁纸自动启动 `autostartHarnessTarget` 的 `args: parseLaunchArgs(settings.dshLaunch.args)` | 同上 |
| `wallpaper/src/App.tsx` 2118-2120 | 手动「启动」`launchHarnessTarget` 的 `args: parseLaunchArgs(settings.dshLaunch.args)` | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 27-33 | `parseLaunchArgs` import 及其 FREEZE 说明 | `wallpaper/src/settings/SettingsWindow.launch-args.tsx` |
| `wallpaper/src/settings/SettingsWindow.tsx` 461-463 | 「打开界面」`ensureHarnessUi` 的 `args: parseLaunchArgs(current.args)` | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 1095-1098、1116-1128 | 两个 handler 共用的 FREEZE 说明（两份归档各留一份）与 `onSelectLaunchArgs` | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 1130-1132 | TUI 不带参数的 FREEZE 说明；片段里附上 `a8e2e91` 中的恢复形态 `openSubjectTui(parseLaunchArgs(settingsRef.current.dshLaunch.args))` | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 25-31、34 | 三个 import 共用的 FREEZE 说明（两份归档各留一份）与 `launchArgsIssue` import | `wallpaper/src/settings/SettingsPanel.launch-args.tsx` |
| `wallpaper/src/settings/SettingsPanel.tsx` 153-166 | 四个 prop 共用的说明（两份归档各留一份）与 `onSelectLaunchArgs` prop | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 660-670 | `argsIssue` 计算 | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 902-905 | 连接卡片里那段 JSX 说明的后半段：「启动参数」为什么取代「启动命令」（只有说明） | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 930-952 | 「启动参数」`Field` | 同上 |
| `wallpaper/src/connect/harnessSubjects.ts` 341-344 | `port-occupied-external` 那句提示被删半句的 FREEZE 说明 | 只在本 README 留档（见下） |

`harnessSubjects.ts` 341-344 原文（现役已删，提示文案本身未改）：

> FREEZE（临时冻结，不是删除）：这句话原来还有半句"在「启动参数」里换一个端口可以并行再起一个实例"。本 build 里「启动参数」不在界面上（它随这次冻结一起关掉了），把用户指向一个不存在的入口比不说更坏 —— 指向不存在的入口正是这个项目一直在修的那种失败。恢复办法：把原来那半句加回来（它随「启动参数」一起复活）。

这半句现在住在 i18n 词条 `harness.subject.launch.port-occupied-external` 里（zh / en 两份）；恢复时把它加回这两条词条，B6 没有动任何词条。

## 迁出内容
- import：`App.tsx` 与 `SettingsWindow.tsx` 的 `parseLaunchArgs`，`SettingsPanel.tsx` 的 `launchArgsIssue`。
- 调用实参：三个启动入口与「打开界面」的 `args:`，TUI 的 `parseLaunchArgs(...)` 实参。
- props：`SettingsPanelProps.onSelectLaunchArgs`；handler：`SettingsWindow` 里的 `onSelectLaunchArgs`（改参数时一并清掉 `endpointPort` pin）。
- 局部量：`argsIssue`；界面：「启动参数」`Field`（`title="启动参数"`、`aria-label="启动参数"`）。
- CSS：无（「启动参数」行没有专用样式）。

## 迁出的测试（共 2 条，均为 `it.skip`）
- `wallpaper/tests/launchArgsAndInstances.spec.ts` 原 165-173 行：`is cleared of the explicit pin when the args change, …` → `wallpaper/tests/launchArgsAndInstances.spec.ts`
- `wallpaper/tests/dshAutostart.spec.ts` 原 186-206 行：`gives the automatic path the same launcher and the same args as the button` → `wallpaper/tests/dshAutostart.spec.ts`

用例逐字保留（含 `it.skip` 与上方的冻结说明），文件里只补了 import、`source` 帮手与外层 `describe`；import 路径按放回 `wallpaper/tests/` 写。

现役测试里改写的断言（`wallpaper/tests/launchArgsAndInstances.spec.ts` 的「启动参数冻结之后：没有参数流出去」一组）：现役 `App.tsx` / `SettingsWindow.tsx` 不再出现 `parseLaunchArgs` 与 `args:`；可恢复的 `args: parseLaunchArgs(...)` 各行与 TUI 恢复形态改为读本归档的片段来钉。

## 留在现役的共享能力（不要随本归档恢复或删除）
- `wallpaper/src/connect/launchArgs.ts`：`parseLaunchArgs`、`launchPortFromArgs`、`launchSettingsPort`，以及本轮仍留在原处的 `launchArgsIssue`（连同它们的测试）。
- `wallpaper/src/native/runtime.ts` 对 `parseLaunchArgs` 的 import 与 `setHarnessEndpointScope` 里的调用（端点镜子：探针与「打开界面」盯同一个端口）。
- `wallpaper/src/connect/endpoints.ts` 的 `launchSettingsPort` import 与 `subjectEndpointPorts` 里的调用（存下的 `args` 中声明的端口排在主体自身端口最前）。
- `wallpaper/src/settings/store.ts` 的 `dshLaunch.args?`（持久化兼容，已存的值照常读写）。
- `wallpaper/src/App.tsx` 里 `dshAutostartNotice` 的 `case 'launch-args-invalid'` 及其 FREEZE 留档说明（原生结果枚举里仍有这条码）。
- `wallpaper/src/settings/SettingsWindow.tsx` 的 `stopManagedInstance` 与 `onStopAllManagedDsh`（属于 instance-ui 一侧，同样留在现役）。

## 恢复步骤
1. 把片段按"原 N-M 行"放回对应位置：`App.tsx` 加回 `parseLaunchArgs` import 与三处 `args:`；`SettingsWindow.tsx` 加回 import、「打开界面」的 `args:`、`onSelectLaunchArgs` handler，并把 `openSubjectTui()` 改成片段里的恢复形态；`SettingsPanel.tsx` 加回 `launchArgsIssue` import、`onSelectLaunchArgs` prop（建议改为必填）与「启动参数」`Field`。FREEZE 说明按需删去。
2. `argsIssue` 与 `Field` 二选一：3c92772 里冻结的 `Field` 直接写 `launchArgsIssue(settings.dshLaunch.args) ?? …`；若同时恢复 `const argsIssue = …`，就把 `detail` 改回 `argsIssue ?? …`（`a8e2e91` 的写法），否则 `noUnusedLocals` 会报 `argsIssue` 未使用。
3. 若一并恢复「起别名」与实例下拉，见 `archive/frozen/instance-ui/README.md`（两边共用的 FREEZE 说明各留了一份）。
4. 恢复测试：把两份归档 spec 中的用例移回原文件对应位置，按恢复后的源码把 `it.skip` 改为 `it` 并更新断言；同时调整 `launchArgsAndInstances.spec.ts` 中改读归档的那两条（改回检查现役源码里的 `args:`）。
5. 在 `wallpaper/` 下运行 `pnpm typecheck && pnpm test && pnpm build && pnpm build:lite`。

## 未验证范围
- 归档片段不参与 typecheck、测试与构建，接口可能随现役代码演进而失配。
- B6 在无法运行 tsc / vitest 的环境里完成：现役文件的"剥注释后逐字相同"、括号配对与测试断言都用 node 脚本核对过，最终以 CI 为准。

## 注意
- 片段是从调用、props、JSX 中间截出来的，单独打开时 IDE 报语法错误或未解析的符号属于预期。
- CI 的 `paths-ignore` 包含 `archive/**`，只改归档不会触发流水线。
- Vitest 的发现根是 `wallpaper/`，仓库根的 `archive/` 不会被默认测试发现，所以这里的 spec 不会运行。
