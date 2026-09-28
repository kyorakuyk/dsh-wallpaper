# docs/ 目录导览

本目录按**用途**分文件夹收纳。新增文档请放进对应目录；根目录只保留本导览文件。

## 目录结构

| 目录 | 内容 | 是否进仓库 |
| --- | --- | --- |
| `plans/` | 施工文档：待实施的需求、施工顺序与验收条件（含已完成的） | ✅ 进仓库 |
| `evidence/` | 真机取证与结论记录：采样数据、验证结果、已修正的错误结论 | ✅ 进仓库 |
| `design/` | 机制与设计说明：架构决定、行为定义、跨模块约定 | ✅ 进仓库 |
| `guides/` | 操作与专题说明：环境搭建、发布流程、锁屏/多屏等单点主题 | ✅ 进仓库 |
| `career/` | 求职、职业规划、面试材料、学习路线 | ❌ **本地专用** |
| `resume/` | 简历（含真实姓名/电话/邮箱） | ❌ **本地专用** |
| `public-overview/` | 对外概览仓的本地暂存稿 | ❌ **本地专用** |

`career/`、`resume/`、`public-overview/` 由 `.gitignore` **按整目录忽略**。这三处含个人隐私或对外仓内容，**绝不入库**；因此它们在本机随时可读写，但 `git status` 里不会出现。

## 现有文档

**plans/**

| 文档 | 主题 |
| --- | --- |
| `autostart-recovery-plan.md` | 登录自启项恢复 |
| `dsh-harness-connection-autostart-compatibility-plan.md` | Harness 连接与开机自启的兼容性修复 |
| `git-recovery-plan.md` | 仓库恢复 |
| `input-island-keyboard-focus-repair-plan.md` | 输入岛单击后无法输入（Windows 焦点交接） |
| `stability-performance-remediation-plan.md` | 稳定性与性能整改（9 项） |
| `startup-render-handoff-implementation-plan.md` | 启动渲染交接 |
| `theme-and-asset-customization-plan.md` | 主题与素材自定义 |
| `v0.3-product-rewrite-plan.md` | v0.3 产品化重写 |

**evidence/**

| 文档 | 主题 |
| --- | --- |
| `build-and-publish-timings.md` | 构建与发布耗时实测：本地发布 103.5s→65.5s 的做法、CI 缓存的坑、以及量耗时的方法 |
| `input-island-focus-phase-a-evidence.md` | 输入岛焦点问题的 A 阶段取证 |
| `input-island-focus-native-route-closed.md` | 同问题的结论记录（含一处被推翻的结论及其更正） |
| `input-model-desktop-hit-testing.md` | 桌面上的一次点击归谁（前台桌面收不到 DOM 鼠标事件的取证） |
| `settings-window-frame-and-corners.md` | 设置中心窗口的圆角与系统边框 |

**design/**

| 文档 | 主题 |
| --- | --- |
| `harness-subject-and-ui-design.md` | Harness 执行主体与「拉起 UI」：两类模型与已冻结决定 |
| `deepseek-web-adapter.md` | DeepSeek 网页模式适配器 |
| `multi-screen.md` | 多屏与逐屏背景 |

**guides/**

| 文档 | 主题 |
| --- | --- |
| `engine-setup.md` | 引擎/环境搭建 |
| `lite-release.md` | Lite 版发布 |
| `lockscreen-minimal-probe.md` | 锁屏最小化探针 |
| `lockscreen-msix-test.md` | 锁屏 MSIX 测试包 |

## 约定

1. **跨目录引用必须写相对路径**，例如从 `plans/` 指向 `design/`：`[多屏说明](../design/multi-screen.md)`。搬家后请顺手修链接。
2. **`career/` 是整目录忽略的**，因此往里面新增任何材料都不需要再补 `.gitignore` 规则。
3. **历史文件里的旧路径不要改写**：`.workbuddy/` 下的会话记录，以及早期文档里出现的 `docs/xxx.md` 旧路径，都是当时的真实记录；改写会伪造历史。以本文件的映射为准即可。
4. 代码注释里引用文档时，请写**当前路径**（例如 `docs/evidence/...`）。本目录结构变更后，已同步更新过 `lib.rs` 与 `windows_integration.rs` 中的三处注释。
