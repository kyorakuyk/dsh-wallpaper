// 归档片段（B6，2026-10）：`wallpaper/src/settings/SettingsPanel.tsx` 中「起别名」、实例下拉 `RunningInstances` 与每实例停止的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// `RunningInstances` 里原有三行 `// …` 说明写在 JSX 中间（注释外壳下面的又一层 `//`）；去掉外壳后它们会变成
// JSX 文本，所以这里按 a8e2e91 里的原样写回 `{/* … */}`，文字未改。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/frozen/instance-ui/README.md`。

// ---- 原 25-32 行：import 区，`./AppearancePreview.tsx` 之后（25-31 行的说明管三个 import，两份归档各留一份；
// 恢复时用 32 行替换现役那条不带 `instanceLabel` / `subjectAlias` 的 import；34 行的 `launchArgsIssue` 在 `archive/frozen/launch-ui/` 里） ----
// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：「起别名」与实例下拉被冻在这个 build 之外，所以它们要的两样东西
// 也一起冻住 —— `instanceLabel` 只给实例下拉的行文字用，`subjectAlias` 只给「起别名」输入框回显用，
// `launchArgsIssue` 只给「启动参数」那行的校验提示用。三个函数本身一行都没动（
// `connect/harnessSubjects.ts` / `connect/launchArgs.ts` 里的纯逻辑与它们的测试照常跑）。
// 怎么恢复：取消注释下面两个 import，再去掉本文件里对应的三处 FREEZE 注释。
// ---------------------------------------------------------------------------
import { catalogAgeLabel, displaySubjectPath, instanceLabel, sameSubject, subjectAlias, subjectOptionLabel } from '../connect/harnessSubjects.ts'
// 恢复 `RunningInstances` 时还要在 `../native/runtime.ts` 的类型 import 里补上 `ManagedDshInstance`（`HarnessTarget` 现役已有）。

// ---- 原 153-165 行与 167-168 行：`SettingsPanelProps` 内，`tuiAvailable` 之后、`reachAction` 之前（说明两份归档各留一份；
// 166 行的 `onSelectLaunchArgs` 在 `archive/frozen/launch-ui/` 里） ----
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
  onSelectSubjectAlias: (alias: string) => void
  onStopManagedInstance: (instanceKey: string) => void

// ---- 原 363-448 行：模块级，`harnessEndpointKindLabel` 之后、`displayLabel` 之前 ----
// ---------------------------------------------------------------------------
// FREEZE（临时冻结，不是删除）：「当前已启动实例」下拉（`RunningInstances`）。
//
// 为什么关：本 build 回到 `a8e2e91` 之前的行为 —— 停止入口只有一个，就是卡片底部那行
// 「本应用启动的 DSH」+「停止本应用启动的 DSH」（那个按钮已经恢复了，见连接卡片底部）。标题
// 右上角的下拉在这个世界里没有第二个实例可列，留着它只会与底部那个按钮做同一件事。
//
// 怎么恢复：把下面这个组件取消注释，恢复连接卡片上的 `action={<RunningInstances … />}`，并在
// `SettingsWindow.tsx` 里恢复 `onStopManagedInstance` 的传参（三处传参各有自己的 FREEZE 注释）；
// 再把底部那行改回注释（它就在那里留档）。原生侧一行都不用动：`managed_dsh_status` 仍然返回
// 列表、`stop_managed_dsh` 仍然接受 instanceKey、`managedDshBusy` 这个 prop 现在钉着底部那个
// 按钮的可用性。
//
// 卡片标题右上角的「当前已启动实例」。
//
// 每一行读作 `别名 · 端口`，行尾的 × 停掉**那一个**实例。同一个源码目录起了两个端口时，这是
// 唯一能分清"我要停的是哪一个"的地方 —— 所以行文字必须带端口，而不是只写一个名字。
//
// 行**不是**可选项：这里的下拉是一个清单，不是单选。点行不做任何事（没有"选中"这个状态），
// 要动就动行尾那个 ×。做成"可选"会让人以为选中它就会改掉「打开界面」的目标，而那条路由
// 「启动参数」里的端口决定（`endpoints.ts`），两个真相来源只会互相打架。
//
// 它复用了 `Choice` 的那套样式类，因为外观该与同一个窗口里的其他下拉一致。
//
function RunningInstances({ instances, targets, aliases, busy, onStopInstance, onStopAll, onRefresh }: {
  instances: readonly ManagedDshInstance[]
  targets: readonly HarnessTarget[]
  aliases: Readonly<Record<string, string>> | undefined
  busy: boolean
  onStopInstance: (instanceKey: string) => void
  onStopAll: () => void
  onRefresh: () => void
}) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const close = (event: MouseEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false) }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [])
  const labelFor = (instance: ManagedDshInstance) => instanceLabel(instance.subjectId, instance.port, targets, aliases)
  // 只有一行时直接把那一行写在按钮上：用户不必为了读到一个名字而先点开一次。
  const triggerText = instances.length === 0
    ? '当前已启动实例（无）'
    : instances.length === 1
      ? labelFor(instances[0]!)
      : `当前已启动实例（${instances.length} 个）`
  return <div className={`settings-choice settings-instances ${open ? 'is-open' : ''}`} ref={root}>
    <button
      type="button"
      className="settings-choice__trigger"
      aria-label="当前已启动实例"
      aria-expanded={open}
      onClick={() => setOpen((shown) => !shown)}
    >
      <span>{triggerText}</span><i>⌄</i>
    </button>
    {open && <div className="settings-choice__menu settings-instances__menu" role="list" aria-label="当前已启动实例">
      {instances.length === 0
        ? <span className="settings-choice__empty">本应用没有启动 DSH；其他人启动的实例不会被列在这里，也不会被停止。</span>
        : instances.map((instance) => {
            const label = labelFor(instance)
            return <div className="settings-instances__row" role="listitem" key={instance.instanceKey}>
              <span className="settings-instances__name" title={instance.subjectId}>{label}</span>
              <button
                type="button"
                className="settings-instances__stop"
                aria-label={`停止实例 ${label}`}
                disabled={busy}
                onClick={() => onStopInstance(instance.instanceKey)}
              >
                ×
              </button>
            </div>
          })}
      <div className="settings-instances__footer">
        <button type="button" className="settings-action secondary" disabled={busy} onClick={onRefresh}>刷新</button>
      </div>
    </div>}
    {/* 「全部停止」紧挨着下拉，因为它们是同一个动作的两个范围：一个是"停这一个"，一个是"都停"。
        原来卡片底部那个「停止本应用启动的 DSH」按钮已经被它取代 —— 两个控件做同一件事，
        用户就得猜它们有什么区别（而答案曾经是"没有区别"）。 */}
    <button className="settings-action secondary" disabled={busy || instances.length === 0} onClick={onStopAll}>全部停止</button>
  </div>
}
// ---------------------------------------------------------------------------

// ---- 原 835-847 行：连接卡片 `<Card title={t('settings.connections.harness.title')} …>` 的属性，`description` 之后 ----
          // FREEZE（临时冻结，不是删除）：卡片标题右上角的「当前已启动实例」下拉。它随
          // 「启动参数」一起冻住（本 build 回到单实例行为，停止入口只有下面那行）。恢复办法：
          // 取消注释下面这个 `action`，并恢复 `RunningInstances` 组件（本文件内，注释里留着）
          // 与 `SettingsWindow.tsx` 里那两处传参。
          action={<RunningInstances
            instances={props.managedDsh.instances}
            targets={props.harnessTargets}
            aliases={settings.dshLaunch.aliases}
            busy={props.managedDshBusy}
            onStopInstance={props.onStopManagedInstance}
            onStopAll={props.onStopAllManagedDsh}
            onRefresh={props.onRefreshManagedDsh}
          />}

// ---- 原 883-887 行：「运行方式」`Choice` 的 options 内，现役 `subjectOptionLabel(target, props.harnessTargets)` 那一行之前 ----
                    // FREEZE（临时冻结，不是删除）：这一行原来把别名表传进去（`subjectOptionLabel(
                    // target, props.harnessTargets, settings.dshLaunch.aliases)`）。不传就是"用目录名
                    // 区分两个同名克隆"——也就是这个功能之前的行文字（`源码目录 · DeepSeekHarness.old
                    // · 0.1.0-rc.5`）。恢复办法：把第三个实参加回去（`subjectOptionLabel` 的别名规则
                    // 一行都没动，`subjectOptionVersion.spec.ts` 仍然钉着它）。
// 恢复形态（3c92772 里这里只有上面的说明；下面这一行逐字取自冻结前的 commit a8e2e91）：
                    ...props.harnessTargets.map((target) => ({ value: target.id, label: subjectOptionLabel(target, props.harnessTargets, settings.dshLaunch.aliases) })),

// ---- 原 908-928 行：连接卡片 `{!shellSelected && <>…</>}` 内，「源码目录」Field（原 907 行）之后 ----
            {/* FREEZE（临时冻结，不是删除）：「起别名」输入框。
                为什么关：别名唯一的作用就是顶替「运行方式」和实例下拉里的"上级目录名"那一段，而
                那个下拉现在按目录名走、实例下拉整个冻住了 —— 留着这个框就是多一个改了也不影响
                任何显示的输入框。别名表本身没删：`dshLaunch.aliases` 与它的归一化测试照常跑，
                写进设置里的值只是暂时不显示。
                怎么恢复：取消下面这个 Field 的注释，并恢复 `props.onSelectSubjectAlias` 那个 prop
                与它在本文件顶部的 `subjectAlias` import。 */}
            {!cliSelected && <Field
              title="起别名"
              detail={`只在「运行方式」和实例下拉里显示，留空就用目录名（${selectedSubject?.label ?? '目录名'}）。两个同名目录原本靠上一级目录区分，别名会顶替那一段。`}
            >
              <input
                value={subjectAlias(settings.dshLaunch.subjectId, settings.dshLaunch.aliases)}
                placeholder="留空时使用目录名"
                aria-label="起别名"
                maxLength={64}
                onChange={(e) => props.onSelectSubjectAlias(e.target.value)}
              />
            </Field>}
