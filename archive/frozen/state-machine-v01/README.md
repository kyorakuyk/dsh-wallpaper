# 归档：v0.1 旧状态机

## 原路径
- `wallpaper/src/scenes/stateMachine.ts` 中的 v0.1 部分（原第 5-39 行与第 108-130 行）：`WallpaperState`、`WallpaperEvent`、`StateTransition`、`TRANSITIONS`、`nextState`、`WallpaperStateMachine`
- `wallpaper/tests/stateMachine.spec.ts`

## 来源
- 来源 commit：`3c92772`（迁移前 tag：`pre-freeze-isolation`）

## 归档原因
v0.1 遗留实现，生产代码中没有调用方；现役状态机使用同文件中保留的 `reduceRuntime`（`RuntimeEvent`、`INITIAL_RUNTIME_STATE`、`reduceRuntime` 未改动）。

## 迁出内容
- 符号：`WallpaperState`、`WallpaperEvent`、`StateTransition`、`TRANSITIONS`、`nextState`、`WallpaperStateMachine`（逐字保留，全部 export）
- 文件：`wallpaper/src/scenes/stateMachine.v01.ts`
- 测试：`wallpaper/tests/stateMachine.spec.ts`，4 条用例；import 已改为 `../src/scenes/stateMachine.v01.ts`，用例内容未变

## 依赖
只依赖本文件内的类型，不依赖 `RuntimeState` 或其他项目模块（仅测试依赖 `vitest`）。

## 恢复方法
1. 把 `stateMachine.v01.ts` 移回 `wallpaper/src/scenes/`，或把其中符号并回 `wallpaper/src/scenes/stateMachine.ts`（也可单独 import）。
2. 把 spec 移回 `wallpaper/tests/stateMachine.spec.ts`，并把 import 路径改回实际所在文件（如 `../src/scenes/stateMachine.ts`）。
3. 在 `wallpaper/` 下运行 `pnpm typecheck` 和 `pnpm test`。

## 未验证范围
归档文件不参与默认 typecheck 与测试，接口可能随现役代码演进而失配。

## 注意
- 归档文件在 IDE 中出现无法解析的 import 属于预期。
- CI 的 `paths-ignore` 包含 `archive/**`，只改归档不会触发流水线。
- Vitest 发现根是 `wallpaper/`，仓库根的 `archive/` 不会被默认测试发现。
