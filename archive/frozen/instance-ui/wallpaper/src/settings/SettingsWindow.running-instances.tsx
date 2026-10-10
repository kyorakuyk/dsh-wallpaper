// 归档片段（B6，2026-10）：`wallpaper/src/settings/SettingsWindow.tsx` 中「起别名」、实例下拉与每实例停止（`onSelectSubjectAlias`、`onStopManagedInstance`、实例清单刷新、按主体查询）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/frozen/instance-ui/README.md`。

// ---- 原 142-144 行：`managedDshBusy` 那条 JSDoc 的最后一段（现役改写为不带 FREEZE 的现状说明） ----
   * FREEZE（临时冻结，不是删除）：它原来只喂给标题右上角那个实例下拉，现在改喂卡片底部的
   * 「停止本应用启动的 DSH」与它旁边的「刷新」（同一个 `stopManagedInstance`）。所以它没有跟着
   * 下拉一起冻住 —— 底部那个按钮需要它来挡住重复点击。恢复办法：什么都不用做。

// ---- 原 485-487 行：`reach` 处理里 `openClientInBrowser(live)` 之后、`showNotice(… browser-opened …)` 之前 ----
        // FREEZE（临时冻结，不是删除）：这里原来刷新标题右上角那份实例清单（"刚才可能启动了一个
        // 新实例，免得它还停在上一秒的样子"）。下拉冻住了，没有清单可刷。恢复办法：取消下面这一行。
        refreshManagedDsh()

// ---- 原 509-512 行：`stopManagedInstance` 那条 JSDoc 的最后一段（现役改写为不带 FREEZE 的现状说明；函数本身留在现役） ----
   * FREEZE（临时冻结，不是删除）：`instanceKey` 那一条路（下拉里某一行的 ×）冻住了，但"不给
   * instanceKey"这条路**正在用** —— 卡片底部恢复的「停止本应用启动的 DSH」走的就是它，语义与
   * 从前逐字相同（停全部）。恢复办法：把 `onStopManagedInstance` 的传参加回来（取消注释下方
   * 那一处 FREEZE），这个函数不用改。

// ---- 原 594-596 行：`probeRunners.managedDsh` 内，`const status = await nativeRuntime.managedDshStatus()` 之前 ----
      // FREEZE（临时冻结，不是删除）：这里原来带上主体 id（`managedDshStatus(subjectId)`），
      // 问的是"我这次启动的那个孩子还在不在"。不带主体问的是同一件事的单实例形态。
      // 恢复办法：把那个实参加回去（一行）。
// 恢复形态（3c92772 里这里只有上面的说明；下面这一行逐字取自冻结前的 commit a8e2e91）：
      const status = await nativeRuntime.managedDshStatus(settingsRef.current.dshLaunch.subjectId)

// ---- 原 1090-1093 行：`<SettingsPanel …>` 的 props 内，`onRefreshManagedDsh` 之后、`onStopAllManagedDsh` 之前 ----
      // FREEZE（临时冻结，不是删除）：标题右上角那个按实例停止的入口（下拉里某一行的 ×）。
      // 它随下拉一起冻住；不丢动作 —— 没有 instanceKey 的那一条路由 `onStopAllManagedDsh` 承担，
      // 也就是卡片底部恢复的「停止本应用启动的 DSH」。恢复办法：取消下面这一行。
      onStopManagedInstance={(instanceKey) => { void stopManagedInstance(instanceKey) }}

// ---- 原 1095-1115 行：`<SettingsPanel …>` 的 props 内，`onStopAllManagedDsh` 之后（1095-1098 行那段 FREEZE 说明同时管两个 handler，
// 两份归档各留一份；1116-1128 行的 `onSelectLaunchArgs` 在 `archive/frozen/launch-ui/` 里） ----
      // FREEZE（临时冻结，不是删除）：「起别名」与「启动参数」两个 handler。它们的输入控件冻住了
      // （`SettingsPanel.tsx` 里对应的 Field 都注释了），所以这里也一起冻 —— 留着就是两段永远
      // 不会跑的回调，而"改了没反应"是比"没有这个入口"更难懂的状态。
      // 这一段里的规则本身一行都没改，恢复办法就是取消这一整块的注释。
      onSelectSubjectAlias={(alias) => {
        // 别名按主体 id 存：用户可能在两棵树之间来回切，名字必须跟着树走。空串表示"用目录名"，
        // 所以它**删掉**那个键，而不是存一个空值 —— 让"没起别名"只有一种表示。
        const subjectId = settingsRef.current.dshLaunch.subjectId
        if (!subjectId) return
        const aliases = { ...(settingsRef.current.dshLaunch.aliases ?? {}) }
        const name = alias.trim()
        if (name) aliases[subjectId] = name
        else delete aliases[subjectId]
        change({
          ...settingsRef.current,
          dshLaunch: {
            ...settingsRef.current.dshLaunch,
            ...(Object.keys(aliases).length > 0 ? { aliases } : { aliases: undefined }),
          },
        })
      }}
