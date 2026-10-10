// 归档片段（B6，2026-10）：`wallpaper/src/App.tsx` 中实例清单按主体查询（`managedDshStatus(subjectId)` 的实参）的冻结代码。
// 来源 commit 3c92772（tag pre-freeze-isolation）；下文行号均指该 commit 中的原文件。
// 原文件里这些代码多是 `//` 或 `{/* */}` 注释；这里去掉注释外壳恢复成普通代码，FREEZE 说明保留为注释。
// 本文件不在任何构建里：单独编译不通过、IDE 报未解析符号属预期。
// 恢复办法见 `archive/frozen/instance-ui/README.md`。

// ---- 原 1439-1442 行：`harnessStarting` 那个 effect 的 `check` 内，`const managed = await nativeRuntime.managedDshStatus()` 之前 ----
        // FREEZE（临时冻结，不是删除）：这里原来把**主体**也传进去（`managedDshStatus(subjectId
        // ?? rootPath)`），因为并行实例之后"我这次启动的那个孩子还在不在"要按主体问。回到不带
        // 主体：单实例世界里两者答案相同，而这一版就该是那个世界的形状。恢复办法：把那个实参加
        // 回去（一行）。参数仍然在 `runtime.ts` 的签名里，`instances` / 每实例停止也照旧。
// 恢复形态（3c92772 里这里只有上面的说明；下面这一行逐字取自冻结前的 commit a8e2e91）：
        const managed = await nativeRuntime.managedDshStatus(settingsRef.current.dshLaunch.subjectId ?? settingsRef.current.dshLaunch.rootPath)
