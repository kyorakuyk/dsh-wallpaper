# 交互层交接：待办、已拍板决定与雷区

状态：**接手用**。生成于 `4c6041f`（分支 `codex/startup-render-handoff`），当前安装 **0.2.0.85**，回滚点 `artifacts/msix-test/rollback-0.2.0.84`。目的：让新的工作上下文**不必重读整场对话**就能继续施工。

相关：[`../design/harness-subject-and-ui-design.md`](../design/harness-subject-and-ui-design.md)（执行主体垫片，四块已完成）、[`input-island-keyboard-focus-repair-plan.md`](input-island-keyboard-focus-repair-plan.md)（键盘焦点，早前计划）。

---

## 1. 用户已拍板的决定（不得擅自改回）

1. **表桌面上单击胶囊 = 进入里桌面 + 弹出输入岛。**
   说明：这是不可逆动作，用户明确接受；此前我方建议"单击只弹岛"已被否决。**表/里切换保留在原有手势里，但落在交互控件上的点击/双击不得触发它**（见待办 ②）。实现时必须把这条写进胶囊处理函数的注释，避免以后被"顺手简化"回只弹岛。
2. **气泡布局的界面选择维持冻结**：只提供默认浏览器，不做 GUI/TUI 选择器。
3. **不做手动添加源码目录**：主体列表只来自扫描。
4. **面向用户处不得出现"官方"字样**（本壁纸不是 DSH 官方）；组件名与 CSS 类名内部的 `official-*` 保持不动。
5. **`profile` 设置项只属于"源码目录"这一路**；选中客户端时隐藏。
6. **最小披露**：设置页只显示用户能操作或能决定的东西，不显示端口、Bridge 状态码等内部信息。

---

## 2. 五个待办问题（按建议顺序）

### ① 底部停靠胶囊既点不到、也不能穿透（最优先，与 ⑤ 同源）
现象：表桌面底部那枚胶囊**收不到点击**，同时**挡住了本该给桌面的点击**；输入岛右上角「X」也不可用。
已知事实（已核实，不要再重新怀疑）：
- `wallpaper/src/runtime/interactionRegions.ts`（`beginInteractionRegionSession` / `collectInteractionRegions` / `publishInteractionRegions`）**没有任何调用者**；原生 `begin_interaction_region_session` / `update_interaction_regions` 同理。**那套"交互区域"机制是死代码**，不是本次元凶（上一轮已排除，勿再改它）。
- 因此现在**没有一套可用的"点击该落到哪一层"的模型**。取证问题是：命中测试究竟归谁——原生前台/区域监控线程、窗口样式（`WS_EX_TRANSPARENT` / `set_ignore_cursor_events`）、还是 Tauri 窗口的透明与鼠标设置？为什么胶囊既收不到点击、又没把点击放过去？
**动手前提：先给出可复述的输入模型 + 证据（代码位置/日志/实测），再修。**

### ② 双击交互控件会误触表/里桌面切换
现象：快速双击「会话记录」与输入岛右下角「切换模型」，会触发表/里桌面切换。
方向：双击判定大概率挂在桌面根节点上，未排除落在交互控件内的事件（`event.target.closest(...)`）。属小修，注释里写清"为什么控件上的双击不算桌面手势"。

### ③ 隐藏任务栏时底部没有适配
理想（用户原话）：**输入岛下边界始终距屏幕下边缘 1.5 个任务栏高度**。
已有基础设施：原生 `desktopLayoutMetrics` → `{ expandedBottomInset, taskbarVisible }`（CSS 默认 48px）。用它实现，**不要新造第二份常量**。

### ④ Harness 模型切换失效（"似乎并没有匹配到可用模型"）
**先取真实数据**：桥接上报的模型/控制项（`harness_controls` / `harness_presets` / 桥接 `/api/...` 返回）。再判断是映射错（Flash/Pro ↔ 幼年/成年 与桥接的模型 id 对不上）还是桥接根本没给可用模型。**没拿到数据不许猜着改**；若结论在桥接侧，如实报告，不要伪造一个"看起来能切"的列表。

### ⑤ 「X」收缩输入岛不可用
与 ① 同源的概率高（岛上的点击被吞）。修法应让 X 把**输入岛连同会话记录轨道收回胶囊**（用户要的行为）。注意：本轮只做到"可用且语义正确"，视觉重塑见 §4。

---

## 3. 用户提出的形态重设计（**尚未动手，需先完成 §2 的 ①②③④**）

- 胶囊改为**球状**；鼠标靠近屏幕下端时**从屏幕下边缘外侧弹出**。
- 表桌面单击 = 进里桌面 + 弹岛（§1.1）；里桌面单击 = 直接弹岛。
- 点按输入岛右上角「X」→ 输入岛连带会话记录轨道**缩回胶囊**。
- 会话记录轨道**挪用"中央会话窗"的样式**（用户认为后者更好看）。
- 我方补充的风险：屏幕外那部分在壁纸窗口之外，只有上浮后可见的部分能接收点击，**热点区域必须比可见部分略大**，否则贴着边缘的上浮永远触发不了。

---

## 4. 已完成的施工（一句话各一条，细节见对应提交与文档）

- **执行主体垫片四块全部落地**：靶点模型与静态发现（`60aacc4`）、启动链 + 设置里的执行主体选择（`9adeb12`）、「拉起 UI」三态幂等（`042ae29`）、扫描结果持久化含最后验证时间（`97cb060`）。设计文档见 [`../design/harness-subject-and-ui-design.md`](../design/harness-subject-and-ui-design.md)。
- **官壳「拉起窗口」的真实 bug 已修并验证**（`22a7e63`）：窗口归属链在打包形态下断链——"祖先名字不同就停止"这条规则在**读不到**进程名时也会停，而读另一个进程的名字正是打包进程不总能做到的。
- **设置页最小披露 + 用户语言**（`0307194` / `e52a0b5` / `a7d68b9` / `43a0818`）：主体列表收敛成一个下拉；删掉「接入的客户端」「启动执行主体」（并入「打开界面」）；根目录改为只读且仅选中源码目录时出现；说明文字全面去术语。
- **受管 DSH 输出与退出码落盘**（`b2eed29`）：`%LOCALAPPDATA%\com.dsh.wallpaper\logs\managed-dsh.log`。
- **默认渐变主题（纯 CSS 背景）**：`5ec4472` → `6a9e2f1`（缩略图与实物同一值）→ `d0507ea`（vmax 尺寸 + 两团水母光 + 去掉全屏 `backdrop-filter` 造成的 hover 卡顿）→ `0ae888f`（暗角退到 52%，不再吃掉亮部）。独立预览：[`../design/blank-background-preview.html`](../design/blank-background-preview.html)（规则由脚本从 `styles.css` 抽取注入，非另抄）。
- **入场动画重放 + 胶囊无几何**（`4c6041f`，上一轮子代理）：改任意设置不再重放 `waking`（根因：订阅 effect 依赖里带了设置，而同一 effect 里挂着一次性的 `boot-ready`）；`.dsh-chat-collapsed` 自带 190×44 几何，单屏路径不再把它撑成整屏。
- **文档目录按用途收纳**（`1424f5c`）：`plans/ design/ evidence/ guides/ career/(ignored) resume/ public-overview/`。

### 未提交但已完成结论的事

- **新背景只在待机屏生效**：`.scene-wake` 与 `.multi-screen-background` 各自写着硬编码渐变，**不消费** `settings.background` / 逐屏背景。用户明确说过"不要动过去的代码"，因此**保持原样**；要做需要单独征得同意（唯一需要动旧架构的地方）。多屏的"逐屏指定 → 否则跟随全局"逻辑**已经存在于 `App.tsx`**（`settings.multiScreen.backgrounds[display.id] ?? settings.background`），缺口只在那两个绘制层。

---

## 5. 雷区与硬约束

1. **工作区里有另一条工作流的未提交改动，绝不可卷进提交**：`wallpaper/src/App.tsx`（其 hunk）、`wallpaper/tests/nativeChatBoundary.spec.ts`、`wallpaper/tests/residentPolling.spec.ts`、未跟踪的 `wallpaper/tests/bootstrapHandoff.spec.ts`、`wallpaper/src-tauri/src/native_handoff.rs`、`wallpaper/src/native/bootstrapHandoff.ts`、以及 `ci.yml` / `README.md` / `packaging/` / `scripts/` / 立绘 PNG / `native_bootstrap.rs` / `lite/*` / `scenes/*`。只用 `git add -- <具体文件>` 或 `git apply --cached <patch>` 提交自己的 hunk。
   **教训**：曾用 `git add -- wallpaper/tests` 把他们的 WIP 卷进一个提交，已用 `reset --soft` 撤销重做；别重犯。
2. **中文一律用 `edit` 工具改**。用 pwsh 拼字符串写文件会**损坏 CJK**——`store.ts` 里出现过 `'认认主题渐变'` 这种乱码（已修）。提交信息用英文可完全避开这条。
3. **先过门禁再提交**（曾出现过"测试红着提交"）：`pnpm typecheck`、`pnpm test`（266 + bridge 69）、`cargo test --locked --all-targets`（168/0，5 ignored）、lite lib 与 lite bin check。
4. **官壳 19387 是本次会话宿主，绝不能重启/杀掉**。第三方桌面客户端 `shell:AppsFolder\ai.deepseek.dsh.desktop`（默认 43120）可用于实测，用完按**应答端口的 PID** 干净停止（`client_window::endpoint_process_id`）。
5. **端口不是契约**（3080/19387/43120 只是默认值）；**绝不接管或停止他人已运行的实例**。
6. 打包：`pwsh -NoProfile -File scripts\publish-local-msix.ps1 -PackageVersion <x.y.z.N>`（自带 Rust 门禁、签名、安装、拉起）。它**必须先绿**；`cargo fmt --check` 不是门禁（整个 crate 有偏差）。

---

## 6. 需要用户亲眼验证的清单（我方无法自证）

1. 切一次「气泡布局」：不应再重放入场动画。
2. 选「任务栏停靠胶囊」：底部只剩一枚小胶囊；**但用户最新实测仍不可点、且不能穿透** → 见 §2 ①。
3. 官壳「拉起窗口」（已在 0.2.0.78 由用户验证通过，勿回退）。
4. 官壳的**静默启动**（§5.1 - 先起服务、窗口隐藏）**从未验证**：把「随壁纸启动 DSH」打开、主体选客户端，重启壁纸观察。
5. 鼠标滑过输入岛是否仍然卡顿（0.2.0.83 去掉了全屏 `backdrop-filter` 那一对，机制上应已解决，需用户确认）。

---

## 7. 已知的"账"（未清，均不阻塞）

- **死代码**：`interactionRegions.ts` 与原生 `begin_interaction_region_session` / `update_interaction_regions`；`scan_dsh_paths` 与 `launch_dsh` 两个命令（渲染层已无调用者，目录指纹由主体扫描复用）；`set_harness_endpoint` / `dshLaunch.endpointPort`（UI 已不再暴露）。清理需连权限、capability 与 `gen/schemas` 一起动，面积较大，**一次只清一项**。
- **§7 遗留根因**："壁纸拉起 DSH 后立即退出"仍未定位。`managed-dsh.log` 已就绪（记录确切的 program/args/cwd、退出码与最后 20 行输出），复现一次即可给出结论。
- `blank-background-preview.html` 的自检里 `mix-blend-mode` 计数与预期差 1，未复核（可能是抽取正则合并了一处；若真漏，预览里会有一团光"不发光"，实物是发的）。
- 两个注释里曾残留旧名"空白背景渐变"，已改为"默认渐变主题"。
