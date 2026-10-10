# 归档：「起别名」、实例下拉与每实例停止（instance-ui）

## 来源
- 来源 commit：`3c92772`（迁移前 tag：`pre-freeze-isolation`）；下文"原 N-M 行"均指该 commit 中的文件。
- 迁出批次：冻结代码归档隔离 B6（2026-10）。这些界面由 `a8e2e91` 加入、`07c7050` 冻结（注释掉，并恢复卡片底部的「停止本应用启动的 DSH」）；B6 只把注释掉的部分移出现役源码，不改任何现役逻辑。

## 归档原因
这个 build 回到单实例行为：设置窗口里没有「起别名」输入框，连接卡片标题右上角没有「当前已启动实例」下拉，也没有按实例停止的入口；停止只走卡片底部那一行（停全部）。这些代码以前以注释形式留在现役文件里，B6 把它们移到这里。剥掉注释后，被改的现役文件与迁移前逐字相同。

## 原路径与行范围 → 归档片段
片段是普通代码（去掉了 `//` 与 `{/* */}` 外壳），每段前有 `// ---- 原 N-M 行：位置 ----`；FREEZE 说明保留为注释。几处 3c92772 里只有说明、没有代码的位置，片段在说明后附上 `a8e2e91` 中的原行作为恢复形态，并注明来源。

| 原位置（3c92772） | 内容 | 片段 |
|---|---|---|
| `wallpaper/src/App.tsx` 1439-1442 | `managedDshStatus` 不再按主体查询的说明；附恢复形态 `managedDshStatus(settingsRef.current.dshLaunch.subjectId ?? settingsRef.current.dshLaunch.rootPath)` | `wallpaper/src/App.running-instances.tsx` |
| `wallpaper/src/settings/SettingsWindow.tsx` 142-144 | `managedDshBusy` JSDoc 里的 FREEZE 段（现役改写为不带 FREEZE 的现状说明） | `wallpaper/src/settings/SettingsWindow.running-instances.tsx` |
| `wallpaper/src/settings/SettingsWindow.tsx` 485-487 | 「打开界面」打开浏览器后刷新实例清单 `refreshManagedDsh()` | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 509-512 | `stopManagedInstance` JSDoc 里的 FREEZE 段（现役改写；函数本身留在现役） | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 594-596 | 探针 `managedDsh` 不再按主体查询的说明；附恢复形态 `managedDshStatus(settingsRef.current.dshLaunch.subjectId)` | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 1090-1093 | `onStopManagedInstance` 传参 | 同上 |
| `wallpaper/src/settings/SettingsWindow.tsx` 1095-1115 | 两个 handler 共用的 FREEZE 说明（两份归档各留一份）与 `onSelectSubjectAlias` | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 25-32 | 三个 import 共用的 FREEZE 说明（两份归档各留一份）与带 `instanceLabel` / `subjectAlias` 的 import | `wallpaper/src/settings/SettingsPanel.running-instances.tsx` |
| `wallpaper/src/settings/SettingsPanel.tsx` 153-165、167-168 | 四个 prop 共用的说明（两份归档各留一份）与 `onSelectSubjectAlias`、`onStopManagedInstance` | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 363-448 | `RunningInstances` 组件及其 FREEZE 说明 | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 835-847 | 连接卡片的 `action={<RunningInstances … />}` | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 883-887 | 「运行方式」选项不再传别名表的说明；附恢复形态 `subjectOptionLabel(target, props.harnessTargets, settings.dshLaunch.aliases)` | 同上 |
| `wallpaper/src/settings/SettingsPanel.tsx` 908-928 | 「起别名」`Field` | 同上 |
| `wallpaper/src/settings/SettingsPanel.css` 1083-1108 | 卡片 header 两列布局与 `.settings-instances*` 规则 | `wallpaper/src/settings/SettingsPanel.running-instances.css` |

`RunningInstances` 里有三行说明在 3c92772 中是 JSX 中间的 `// …`（注释外壳下又一层 `//`）；去壳后会变成 JSX 文本，所以片段按 `a8e2e91` 的原样写成 `{/* … */}`，文字未改。

## 迁出内容
- import：`instanceLabel`、`subjectAlias`（`SettingsPanel.tsx`）。恢复 `RunningInstances` 还需在 `../native/runtime.ts` 的类型 import 里补 `ManagedDshInstance`。
- 组件：`RunningInstances`；Card 属性：`action={<RunningInstances … />}`。
- props：`SettingsPanelProps.onSelectSubjectAlias`、`onStopManagedInstance`；handler / 传参：`SettingsWindow` 的 `onSelectSubjectAlias`（清空即删键）、`onStopManagedInstance`。
- 调用：「打开界面」后的 `refreshManagedDsh()`；`managedDshStatus(...)` 的主体实参（App 与设置窗口两处）；`subjectOptionLabel` 的别名实参。
- 界面：「起别名」`Field`（`title="起别名"`、`aria-label="起别名"`）。
- CSS：`.settings-card>header` 两列布局、`.settings-card__heading`、`.settings-card__action`、`.settings-instances`、`.settings-instances .settings-choice__trigger`、`.settings-instances__menu`、`__row`、`__row:hover`、`__name`、`__stop`、`__stop:hover:not(:disabled)`、`__stop:disabled`、`__footer`。`Card` 组件本身的 `action` 插槽与 `settings-card__heading` / `settings-card__action` 类名仍在现役 JSX 里（只是无人传 `action`）。

## 迁出的测试（共 3 条，均为 `it.skip`）
- `wallpaper/tests/launchArgsAndInstances.spec.ts` 原 217-227 行：`keeps one alias per subject, and drops a cleared one instead of storing an empty string`
- 原 267-280 行：`renders the dropdown in the card header, with 全部停止 beside it`
- 原 282-297 行：`consolidated the two stop controls into one action`

均迁到 `wallpaper/tests/launchArgsAndInstances.spec.ts`。用例逐字保留（含 `it.skip` 与上方的冻结说明），文件里只补了 import（含只供第一条使用的 `normalizeSettings`）、`source` 帮手与外层 `describe`；import 路径按放回 `wallpaper/tests/` 写。

## 留在现役的共享能力（不要随本归档恢复或删除）
- `wallpaper/src/connect/harnessSubjects.ts`：`subjectAlias`（App.tsx 仍用它给宿主标签取名）、`subjectOptionLabel`（别名参数仍在签名里），以及本轮留在原处的 `instanceLabel`（连同测试）。
- `wallpaper/src/settings/store.ts`：`dshLaunch.aliases?` 与 `normalizeSubjectAliases`（持久化兼容，已存的别名照常读写）。
- `wallpaper/src/native/runtime.ts`：`ManagedDshInstance` / `ManagedDshStatus.instances`、`managedDshStatus(subjectId?)`、`stopManagedDsh(instanceKey?)` 原生契约。
- `wallpaper/src/settings/SettingsWindow.tsx`：`stopManagedInstance(instanceKey?)`（被 `onStopAllManagedDsh` 使用）、`managedDshBusy`、`refreshManagedDsh`；`SettingsPanel.tsx` 卡片底部的「停止本应用启动的 DSH」与「刷新」。
- 依赖：无专属 npm/crate 依赖；依赖的共享能力见本节上面的列表。

## 恢复步骤
1. 把片段按"原 N-M 行"放回：`SettingsPanel.tsx` 换回带 `instanceLabel` / `subjectAlias` 的 import、补 `ManagedDshInstance` 类型 import、加回两个 prop（建议改为必填）、`RunningInstances` 组件、Card 的 `action`、「起别名」`Field`，并按需给 `subjectOptionLabel` 加回别名实参；`SettingsWindow.tsx` 加回 `onSelectSubjectAlias`、`onStopManagedInstance`、`refreshManagedDsh()` 与 `managedDshStatus` 的主体实参；`App.tsx` 加回 `managedDshStatus` 的主体实参。
2. 把 CSS 片段的规则放回 `SettingsPanel.css`（原位置在 `.settings-hint` 之后）。
3. 决定卡片底部「停止本应用启动的 DSH」那一行的去留：`a8e2e91` 用下拉里的「全部停止」取代了它，冻结时又把它恢复了；与下拉并存就是两个控件做同一件事。
4. 若一并恢复「启动参数」，见 `archive/frozen/launch-ui/README.md`。
5. 恢复测试：把归档 spec 中的三条用例移回原文件对应位置，按恢复后的界面把 `it.skip` 改为 `it` 并更新断言（例如第三条里"底部按钮不在"的断言与第 3 步的决定一致）。
6. 在 `wallpaper/` 下运行 `pnpm typecheck && pnpm test && pnpm build && pnpm build:lite`。

## 未验证范围
- 归档片段不参与 typecheck、测试与构建，接口可能随现役代码演进而失配。
- B6 在无法运行 tsc / vitest 的环境里完成：现役文件的"剥注释后逐字相同"、括号配对与测试断言都用 node 脚本核对过，最终以 CI 为准。

## 注意
- 片段是从 props、JSX、函数体中间截出来的，单独打开时 IDE 报语法错误或未解析的符号属于预期。
- 现役测试会读取部分归档文件做完整性断言，因此修改归档（除纯 .md 外）会触发 CI。
- Vitest 的发现根是 `wallpaper/`，仓库根的 `archive/` 不会被默认测试发现，所以这里的 spec 不会运行。
