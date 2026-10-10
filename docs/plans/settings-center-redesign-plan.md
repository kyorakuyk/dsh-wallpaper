# 设置中心视觉改版 · 实施计划

> 给执行者（另一个模型）看的施工单。**先通读第 0–2 节再动手**，然后按阶段顺序做，一个阶段一个提交，不要跨阶段合并。
> 方向稿（交互效果图）：https://claude.ai/artifact/Wdjuq6PjJz7FenEXpKRCkz（执行者可能打不开，所以下面所有视觉要求都写成了具体数值，以本文为准）。

---

## 0. 目标与范围

**要解决的问题**（现状）：

1. 风格是通用的"深蓝玻璃 + 天蓝强调色 + 每张卡片都有渐变和阴影"，和壁纸里的角色无关。
2. 字太小、对比太低：说明文字 10px、颜色 `#6f8598`；导航副标题 10px；状态栏 9px。
3. 导航图标用的是 Unicode 符号（`⌂ ⌁ ◐ ◇ ☰ ⚙`），粗细、大小、基线都不统一。
4. 层级太多：渐变背景 → 侧栏 → 带渐变阴影的卡片 → 带边框的行，没有重点。
5. 多屏设置靠"显示器 1 / 显示器 5"下拉框，用户对不上实际屏幕。

**改完的样子**：

- 强调色跟随当前角色：Harness 模式是暗红，DeepSeek 模式是海蓝。
- 一层不透明底色，分区之间只用细线隔开，不再有卡片。
- 侧栏顶部是当前角色的小立绘和连接状态。
- 正文 13–14px，说明文字 12px，对比度 ≥ 4.5:1。
- 统一的线性 SVG 图标。
- 多屏页用按真实排列画出的显示器示意图，外观页顶部有实时预览。

**范围**：只改**完整版**的设置中心（`wallpaper/src/settings/` 下的 `SettingsPanel.tsx`、`SettingsPanel.css`，新增文件也放在这个目录），以及 `wallpaper/src/i18n/zh.full.ts`、`en.full.ts`。

---

## 1. 硬性约束（违反任何一条就算失败）

1. **不碰这些地方**：`src-tauri/`（Rust）、`lite/`（Lite 版设置窗）、`settings/store.ts` 的设置结构与版本号、`settings/SettingsWindow.tsx` 的业务逻辑、所有 `FREEZE` 注释块（原样保留，不要"顺手清理"）。
2. **只改呈现，不改行为**：所有回调（`set`、`onChange`、`props.onXxx`）的调用方式与参数保持不变。保存逻辑不变。
3. **界面文字一律走 i18n**：TSX 里不得出现中文字面量（注释除外），`tests/noHardcodedCopy.spec.ts` 会拦。新增的每个键必须**同时**加到 `i18n/zh.full.ts` 和 `i18n/en.full.ts`（少一边 `pnpm typecheck` 会失败）。键按界面位置命名，例如 `settings.general.display-map.identify`。
4. **不加 npm 依赖**。图标手写成 SVG 组件（第 3 节给了全部路径）。
5. **不加载网络字体**。这是离线桌面应用，字体继续用 `--dsh-font-sans`（`ui/tokens/tokens.css` 里已定义）。
6. **保留 `Field` 的两列规则**（`tests/settingsFieldLayout.spec.tsx` 钉着）：
   - `.settings-field__copy` 必须是 `flex: 1 1 auto; min-width: 0`；
   - `.settings-field__control` 必须是 `flex: 0 1 auto; flex-wrap: wrap; min-width: 0`；
   - `Field` 的 `children` 只放控件，说明文字走 `detail` / `note`。
7. **保留窗口圆角的约定**：`.settings-app` 的 `border-radius: 8px` 不改，`SettingsWindow.css` 里根节点透明那几条不改（文件里有注释解释原因）。
8. **下拉菜单不能被裁掉**：现在靠 `.settings-card:has(.settings-choice.is-open){overflow:visible}` 解决。改版后分区不再 `overflow:hidden`，这条规则可以保留也可以删，但必须实测下拉菜单在页面最底部一行时仍完整可见。
9. **每个阶段结束**都要在仓库根目录跑：
   ```
   pnpm typecheck
   pnpm test
   ```
   两条全绿才能提交。不许为了让测试通过去改测试的断言；如果某条测试确实和新设计冲突，停下来，把测试名和冲突点写进提交说明，交给人决定。
10. 在分支 `feat/settings-redesign` 上工作，一个阶段一个提交。每个阶段提交后用 `pnpm desktop:dev` 打开设置中心截图（六个页面各一张），放进提交说明或 PR。

---

## 2. 设计规范（所有数值以这里为准）

### 2.1 颜色变量

在 `SettingsPanel.css` 的 `.settings-app` 上定义（替换现有的 `--panel`、`--line`）：

| 变量 | 值 | 用途 |
|---|---|---|
| `--s-bg` | `rgba(11, 20, 34, .97)` | 唯一一层底色（整个窗口） |
| `--s-line` | `#1C2A3B` | 分区分隔线、标题栏/侧栏边线 |
| `--s-control-bg` | `#08101B` | 输入框、下拉触发条、分段按钮容器的底 |
| `--s-control-line` | `#2A3A4E` | 控件边框 |
| `--s-hover` | `#132033` | 悬停底色 |
| `--s-text` | `#E8EFF6` | 正文、标题 |
| `--s-text-2` | `#A3B4C4` | 次要文字（页面副标题、未选中的导航） |
| `--s-text-3` | `#8094A7` | 说明文字、状态栏（在 `--s-bg` 上约 5.6:1，不得再调暗） |
| `--s-accent` | 见 2.2 | 强调色：文字、图标、描边 |
| `--s-accent-fill` | 见 2.2 | 强调色实底：开关打开时的底、主按钮 |
| `--s-accent-soft` | 见 2.2 | 强调色浅底：选中导航、选中分段 |
| `--s-on-accent` | `#0B1422` | 压在 `--s-accent-fill` 上的字/开关圆点（深色，保证对比） |

### 2.2 强调色跟随角色

由当前模式决定：`const backend = props.liveBackend ?? settings.defaultBackend`。

| 模式 | `--s-accent` | `--s-accent-fill` | `--s-accent-soft` |
|---|---|---|---|
| `harness` | `#FF6B81` | `#E03050` | `rgba(224, 48, 80, .16)` |
| `deepseek-web` / `deepseek-api` | `#7FC4FF` | `#4DA6FF` | `rgba(77, 166, 255, .16)` |

实现方式：在 `.settings-app` 根节点加一个类，`settings-app--harness` 或 `settings-app--deepseek`，两个类各自定义这三个变量。**不要**用内联 style 写颜色。这些值来自 `persona/officialCatalog.ts` 里的角色主题，改版后如果角色主题色变了，这里要同步。

### 2.3 字号

| 元素 | 字号 / 字重 / 行高 |
|---|---|
| 页面大标题 `h1` | 26px / 700 / 1.2，`letter-spacing: -.01em` |
| 页面副标题 | 13px / 400，颜色 `--s-text-2` |
| 分区标题 `h2` | 15px / 700 |
| 分区说明 | 12px / 1.55，颜色 `--s-text-3` |
| 行标题（`.settings-field__copy strong`） | 14px / 500 |
| 行说明（`.settings-field__copy span`） | 12px / 1.55，颜色 `--s-text-3`，`max-width: 460px` |
| 行备注（`.settings-field__note`） | 12px，颜色保持现在的 `#d9b478` |
| 导航项 | 14px；选中时字重 500 |
| 控件内文字（下拉、输入框、按钮） | 13px |
| 状态栏 | 12px，颜色 `--s-text-3` |
| 数字/分辨率/版本号 | `--dsh-font-mono`，11–12px |

**全文件不得再出现小于 11px 的字号。** 改完后搜 `font-size:9px`、`font-size:10px`、`font-size:10.5px`、`font-size:8px`，确保为零。

### 2.4 尺寸与圆角

- 可点击控件高度 ≥ 36px；导航项 42px；分段按钮 38px。
- 开关（`.settings-toggle`）：44 × 26，圆点 18 × 18，内边距 3px。关：底 `#1A2636`，边 `--s-control-line`，圆点 `--s-text-3`；开：底和边都是 `--s-accent-fill`，圆点 `--s-on-accent`。
- 圆角：控件 8px，分段按钮内的项 7px，缩略图 10px（图片本身 6px），窗口 8px（不改）。
- 内容区内边距：`26px 36px 30px`。

### 2.5 焦点与动效

- 所有可交互元素加 `:focus-visible { outline: 2px solid var(--s-accent); outline-offset: 2px; }`。
- 过渡统一 `.16s ease`，只用在 `background`、`border-color`、`color`、开关圆点的 `left` 上。
- 加 `@media (prefers-reduced-motion: reduce) { .settings-app * { transition: none !important; animation: none !important; } }`。

---

## 3. 阶段 1：颜色、字号、对比度、图标（纯样式 + 一个图标组件）

**目标**：不动布局结构，只把颜色、字号、图标换掉。完成后整体观感应该已经明显不同。

### 3.1 颜色变量与强调色

1. 在 `SettingsPanel.css` 的 `.settings-app` 规则里删掉 `--panel`、`--line`，加入 2.1 的全部变量。
2. 新增 `.settings-app--harness` 和 `.settings-app--deepseek` 两条规则，各定义 2.2 的三个变量。
3. `.settings-app` 的 `background` 从渐变改为 `var(--s-bg)`；删掉 `backdrop-filter`（一层不透明底，不需要玻璃）。
4. 全文件把写死的强调色换成变量：
   - `#208cc8`、`#238fc8`、`#176ca5`、`#2b9de5`、`#42b7f4`、`#3eaeea`、`#4bbcff` → `var(--s-accent-fill)` 或 `var(--s-accent)`（文字/描边用 `--s-accent`，实底用 `--s-accent-fill`）；
   - `rgba(37,143,214,…)`、`rgba(58,163,225,…)`、`rgba(43,157,229,…)`、`rgba(76,185,255,…)`、`rgba(81,186,255,…)` 这类浅蓝底/描边 → `var(--s-accent-soft)` 或 `var(--s-accent)`；
   - `rgba(157,210,255,.13)`、`rgba(158,205,239,.075)` 这类分隔线 → `var(--s-line)`；
   - `#6f8598`、`#71879a`、`#73899c`、`#617689`、`#7890a4` 这类灰字 → `var(--s-text-3)`；`#aebdca`、`#9fb2c4` → `var(--s-text-2)`。
   - **例外**：`.official-persona-card--deepseek / --harness` 里的颜色是各自角色的固定色，不要换成变量。
5. 在 `SettingsPanel.tsx` 渲染根节点处：
   ```tsx
   const backend = props.liveBackend ?? settings.defaultBackend
   const themeClass = backend === 'harness' ? 'settings-app--harness' : 'settings-app--deepseek'
   return <div className={`settings-app ${themeClass}`}>
   ```

### 3.2 字号与对比度

按 2.3 逐条改 `SettingsPanel.css`。注意这个文件很多规则写在同一行，改之前先把要改的那一行整体读一遍，别把相邻规则改坏。

### 3.3 开关、按钮、输入框

- 开关按 2.4 重写 `.settings-toggle` 和 `.settings-toggle span`。
- `.settings-action`（主按钮）：底 `var(--s-accent-fill)`，字 `var(--s-on-accent)`，字重 500，去掉渐变和阴影，高度 36px，圆角 8px。
- `.settings-action.secondary`：底透明，边 `var(--s-control-line)`，字 `var(--s-text)`，悬停底 `var(--s-hover)`。
- 输入框、下拉触发条：底 `var(--s-control-bg)`，边 `var(--s-control-line)`，高 36px，字号 13px；聚焦时边 `var(--s-accent)`，外圈 `0 0 0 3px var(--s-accent-soft)`。
- 下拉菜单 `.settings-choice__menu`：底 `#0E1928`，边 `var(--s-control-line)`；选中/悬停项底 `var(--s-accent-soft)`。

### 3.4 图标组件

新建 `wallpaper/src/settings/SettingsIcon.tsx`：

```tsx
export type SettingsIconName = 'general' | 'connections' | 'appearance' | 'personas' | 'history' | 'system' | 'close' | 'monitor' | 'person' | 'chat' | 'check'

const PATHS: Record<SettingsIconName, JSX.Element> = {
  general: <><path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" /><circle cx="15" cy="6" r="2" /><circle cx="9" cy="12" r="2" /><circle cx="17" cy="18" r="2" /></>,
  connections: <><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" /><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" /></>,
  appearance: <><circle cx="12" cy="12" r="8" /><path d="M12 4a8 8 0 0 0 0 16z" fill="currentColor" /></>,
  personas: <><circle cx="12" cy="8" r="4" /><path d="M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6" /></>,
  history: <><circle cx="12" cy="12" r="8" /><path d="M12 8v4l3 2" /></>,
  system: <><rect x="5" y="5" width="14" height="14" rx="2" /><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" /></>,
  close: <path d="M6 6l12 12M18 6L6 18" />,
  monitor: <><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></>,
  person: <><circle cx="12" cy="8" r="4" /><path d="M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6" /></>,
  chat: <path d="M5 5h14v10H9l-4 4z" />,
  check: <path d="M5 12l5 5 9-10" />,
}

export function SettingsIcon({ name, size = 18 }: { name: SettingsIconName; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{PATHS[name]}</svg>
}
```

然后在 `SettingsPanel.tsx`：

- `pages` 数组的 `icon: string` 改成 `icon: SettingsIconName`，六项分别是 `'general' | 'connections' | 'appearance' | 'personas' | 'history' | 'system'`；
- 导航按钮里 `<span className="settings-nav__icon">{item.icon}</span>` 改为 `<span className="settings-nav__icon"><SettingsIcon name={item.icon} /></span>`；
- 标题栏关闭按钮的 `×` 换成 `<SettingsIcon name="close" />`（`aria-label` 保留）；
- 下拉触发条里的 `⌄` 可以保留，也可以换成一个 12px 的向下箭头 SVG（如果换，自己加一个 `chevron` 图标：`<path d="M6 9l6 6 6-6" />`）。

`.settings-nav__icon` 的样式：去掉方形底色，只留 `display:grid; place-items:center; width:20px; height:20px; color: var(--s-text-3)`；选中项里图标颜色 `var(--s-accent)`。

### 3.5 导航样式

- 导航项：`height: 42px; padding: 0 12px; border-radius: 8px; gap: 12px; color: var(--s-text-2); font-size: 14px`。
- 悬停：底 `var(--s-hover)`，字 `var(--s-text)`。
- 选中：底 `var(--s-accent-soft)`，字 `var(--s-text)`，字重 500，**去掉**现在左侧那道 `inset 3px 0` 竖线和边框。
- **导航项里的副标题（`<small>`）去掉**：把 `<span><strong>…</strong><small>…</small></span>` 改成只渲染 `t(item.labelKey)`。`nav.*.hint` 这些键继续在页面标题下方作为副标题使用，不要删键。

### 阶段 1 验收

- [ ] `pnpm typecheck`、`pnpm test` 全绿。
- [ ] 全文件没有小于 11px 的字号。
- [ ] 在设置的"连接"页把聊天模式切到 Harness，强调色（选中导航、开关、按钮、焦点框）立刻变成红色系；切回 DeepSeek 变回蓝色系。
- [ ] 六个导航图标粗细、大小一致，与文字垂直居中。
- [ ] 下拉菜单在页面最后一行打开时完整可见。
- [ ] 按 Tab 键能看到每个控件的焦点框。

---

## 4. 阶段 2：去掉卡片、侧栏角色卡、保存反馈

### 4.1 卡片 → 平铺分区

**只改 CSS，不改 `Card` 组件的类名**（测试和其他选择器依赖 `settings-card`）：

```css
.settings-card{margin:0;padding:22px 0 6px;border:0;border-top:1px solid var(--s-line);border-radius:0;background:none;box-shadow:none;overflow:visible}
.settings-card:first-of-type{border-top:0;padding-top:4px}
.settings-card>header{padding:0 0 10px}
.settings-card__body{padding:0}
```

- `.settings-field` 的 `border-top` 改成 `1px solid var(--s-line)`，`min-height: 60px`，`gap: 24px` 保持。
- 内容区 `.settings-content` 加 `max-width: 860px`，避免大窗口下一行拉得太长。
- `.asset-inbox-row`、`.asset-component-row`、`.history-row`：去掉背景色，只保留 `border: 1px solid var(--s-line)`、`border-radius: 8px`。

### 4.2 页面标题区

- `.settings-page-heading`：改成纵向排列，`h1` 在上（2.3 的大标题样式），`nav.*.hint` 那句（现在在右侧的 `<p>`）移到 `h1` 下方作为副标题。
- 现在 `h1` 上方那个大写小字（`settings.page.heading`）保留 DOM，样式改为 12px、颜色 `var(--s-accent)`、不加 `text-transform`。

### 4.3 侧栏角色卡

新建 `wallpaper/src/settings/SettingsPersonaBadge.tsx`，放在侧栏 `<nav>` **上方**，替代侧栏底部的 `.settings-sidebar__status`（删掉那块，但它显示的 `harnessStateLabel(harnessStatus)` 文字要搬进角色卡）。

数据来源：

- 角色：`officialPersonaCardFor(backend, 'pro')`（`persona/officialCatalog.ts` 已导出）。用它的 `portraitPath` 和名字。
- 背景：当前全局背景 `BACKGROUND_OPTIONS.find(b => b.id === settings.background)?.path`，没有 path（纯色背景）时用 `var(--blank-background)`。
- 路径都要过 `assetUrl()`（`runtime/assets.ts`），和 `OfficialPersonaCards.tsx` 的写法一致。

结构与样式：

```
<div class="settings-persona-badge">           高 150px，圆角 10px，overflow:hidden，相对定位
  <img class="__bg" />                          绝对铺满，object-fit:cover，opacity:.55，alt=""
  <img class="__portrait" />                    绝对定位 right:-6px top:-8px，宽 150px，alt=""
  <div class="__caption">                       绝对定位贴底，上方 26px 的渐变遮罩：
                                                linear-gradient(180deg, rgba(11,20,34,0), rgba(11,20,34,.92))
    <strong>{角色名}</strong>                    14px / 700
    <span><i class="__dot" />{连接状态文字}</span>  12px，--s-text-2；圆点 7px，在线 #52D8A0，其余 #657484
  </div>
</div>
```

- 立绘加 `filter: brightness(.9)`，让它和暗色侧栏融在一起。
- 图片加载失败时（`onError`）隐藏该 `<img>`，卡片仍显示文字。
- 标题栏的品牌区：删掉 `settings-brand__mark` 那张图，只留文字（"Wallpaper" 和副标题，沿用现有 i18n 键），字号 14px / 700 和 12px。

### 4.4 保存反馈

`SettingsPanel.tsx` 里 `const set = (patch) => onChange({ ...settings, ...patch })` 改为：调用 `onChange` 之后，把一个本地状态 `savedAt` 设为 `Date.now()`，1600ms 后清空（用 `useRef` 存定时器，组件卸载时清掉）。

状态栏右侧在 `savedAt` 有值时显示新键 `statusbar.saved`（中文 `已保存`，英文 `Saved`），前面加 `<SettingsIcon name="check" size={14} />`，颜色 `var(--s-accent)`；否则显示原来的 `statusbar.autosave`。

不要为每一行单独做"已保存"标记，那需要改 `Field` 的接口，风险大。

### 阶段 2 验收

- [ ] `pnpm typecheck`、`pnpm test` 全绿。
- [ ] 页面上看不到任何卡片背景、阴影；分区之间只有一条细线。
- [ ] 侧栏顶部显示当前角色立绘与连接状态；切换聊天模式后立绘与强调色一起变。
- [ ] 改任意一项设置，状态栏出现约 1.6 秒的"已保存 ✓"。
- [ ] 窗口缩到最小宽度（780px 以下有响应式规则）时，侧栏角色卡不溢出、文字不重叠。

---

## 5. 阶段 3a：多屏示意图（新组件）

位置：常规页的"多屏桌面"那张卡（`SettingsPanel.tsx` 里 `props.desktopDisplays.length > 1 &&` 那一段）。

### 5.1 纯函数：先写、先测

新建 `wallpaper/src/settings/displayLayoutMap.ts`：

```ts
import type { DesktopDisplayInfo } from '../native/runtime.ts'

export interface DisplayTile { id: string; left: number; top: number; width: number; height: number }

/**
 * 把各显示器的 bounds 按真实相对位置等比缩放进 maxWidth × maxHeight 的框里。
 * bounds 可能有负坐标（副屏在主屏左边或上面），先减去最小值。
 * 返回的整体在框内水平居中、底部对齐。
 */
export function layoutDisplays(displays: DesktopDisplayInfo[], maxWidth: number, maxHeight: number, gap = 12): { tiles: DisplayTile[]; width: number; height: number }
```

算法：

1. 取所有 `bounds` 的 `minX, minY, maxX(x+width), maxY(y+height)`；
2. `scale = Math.min(maxWidth / (maxX - minX), maxHeight / (maxY - minY))`；
3. 每块：`left = (x - minX) * scale`，`top = (y - minY) * scale`，`width = w * scale`，`height = h * scale`，四个值都 `Math.round`；
4. 相邻屏幕之间留 `gap` 像素的视觉间隔：每块宽高各减去 `gap`，left/top 各加 `gap / 2`；
5. `displays` 为空时返回 `{ tiles: [], width: 0, height: 0 }`。

新建测试 `wallpaper/tests/displayLayoutMap.spec.ts`，至少覆盖：

- 两块屏左右并排（2560×1600 在 x=0，1920×1080 在 x=2560）：两块的宽度比 ≈ 2560:1920，第二块在第一块右边；
- 副屏在主屏左边（x 为负）：结果里不出现负的 left；
- 上下排列；
- 单块屏；
- 空数组。

### 5.2 组件

新建 `wallpaper/src/settings/DisplayLayoutMap.tsx`，props：

```ts
{
  displays: DesktopDisplayInfo[]
  labels: string[]                          // 与 displays 一一对应，用现有的 displayLabel()
  backgroundUrlFor: (displayId: string) => string | undefined   // 该屏实际使用的背景（逐屏设置优先，否则全局）
  portraitDisplayId?: string                // 用 preferredDisplayId() 解析后的值
  conversationDisplayId?: string
  selectedId: string
  onSelect: (displayId: string) => void
  showNumbers: boolean
}
```

渲染：

- 外框：底 `var(--s-control-bg)`，边 `1px solid var(--s-line)`，圆角 12px，内边距 24px；内部一个 `position: relative` 的容器，宽高取 `layoutDisplays(displays, 640, 240)` 的结果。
- 每块屏是一个 `<button type="button" aria-pressed={selected} aria-label={…}>`，绝对定位到 tile 的位置：
  - 底图是该屏背景（`<img alt="">`，`object-fit: cover`，`opacity: .8`）；没有背景时用 `var(--blank-background)`；
  - 边框 2px：选中 `var(--s-accent)` 并加 `box-shadow: 0 0 0 4px var(--s-accent-soft)`，未选中 `var(--s-control-line)`；圆角 8px；
  - 左上角放标签：立绘在这块屏时显示 `[person 图标] 立绘`，对话窗在这块屏时显示 `[chat 图标] 对话窗`。标签高 22px，圆角 11px，底 `rgba(5,9,15,.82)`，字 `var(--s-accent)`，11px / 500；
  - 左下角显示屏幕名（`labels[i]`，13px / 500）和分辨率与缩放（mono 11px，`--s-text-2`）；块太小放不下时（tile 宽 < 160px）只显示屏幕名；
  - `showNumbers` 为 true 时，整块盖一层 `rgba(5,9,15,.55)`，正中显示序号（`i + 1`），64px / 700。
- 示意图下方右侧放"识别屏幕"按钮（次要按钮样式，左侧 `monitor` 图标），点击切换 `showNumbers`。**只在示意图上显示编号**，不要试图在真实屏幕上弹窗（那需要原生支持，不在本阶段范围）。

### 5.3 接到设置页

在多屏卡片里，`settings.multiScreen.enabled` 为 true 时：

1. **删掉**现在逐屏的那几行 `Field`（每块屏一个背景下拉）以及"对话窗所在屏幕""立绘所在屏幕"两行下拉；
2. 依次放：
   - `DisplayLayoutMap`，`selectedId` 是本地状态，默认取立绘所在的屏；
   - "**{所选屏幕名}的背景**"：一排缩略图按钮，第一个是"跟随全局"（虚线框 + 文字），后面是 `BACKGROUND_OPTIONS` 的每一项（复用 `.background-grid` 的缩略图样式）。点击调用现有的 `setDisplayBackground(selectedId, value)`，"跟随全局"传 `''`；
   - "立绘和头顶气泡"与"对话窗"：显示器数量 ≤ 4 时用新的分段按钮（见 5.4），选项是每块屏的名字；超过 4 块时继续用原来的 `Choice` 下拉。写入方式与原来一致：`set({ multiScreen: { ...settings.multiScreen, portraitDisplayId: value || undefined } })`，对话窗同理。
3. "启用独立多屏背景"那行开关保持原样。

### 5.4 分段按钮组件

在 `SettingsPanel.tsx` 里和 `Toggle`、`Choice` 放在一起，新增：

```tsx
function Segmented({ value, options, onChange, label }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string }) {
  return <div className="settings-segmented" role="radiogroup" aria-label={label}>{options.map((option) =>
    <button type="button" key={option.value} role="radio" aria-checked={option.value === value} className={option.value === value ? 'is-selected' : ''} onClick={() => onChange(option.value)}>{option.label}</button>)}</div>
}
```

样式：容器 `display:grid; grid-auto-flow:column; grid-auto-columns:minmax(0,1fr); padding:3px; border-radius:9px; background:var(--s-control-bg); border:1px solid var(--s-line)`；按钮高 38px、圆角 7px、13px、透明底、`--s-text-2`；选中项底 `var(--s-accent-soft)`、字 `var(--s-text)`、字重 500、`box-shadow: inset 0 0 0 1px var(--s-accent)`。

### 5.5 新增 i18n 键（中英都要加）

| 键 | 中文 | English |
|---|---|---|
| `settings.general.display-map.label` | 显示器布局 | Display layout |
| `settings.general.display-map.identify` | 识别屏幕 | Identify displays |
| `settings.general.display-map.hide-numbers` | 隐藏编号 | Hide numbers |
| `settings.general.display-map.select` | 选择{display} | Select {display} |
| `settings.general.display-map.portrait-tag` | 立绘 | Portrait |
| `settings.general.display-map.chat-tag` | 对话窗 | Chat |
| `settings.general.display-map.background-of` | {display}的背景 | Background for {display} |
| `settings.general.display-map.background-hint` | 只影响这一块屏幕 | Applies to this display only |
| `settings.general.display-map.follow-global` | 跟随全局 | Follow global |

"立绘和头顶气泡""对话窗"两行的标题沿用现有的 `settings.general.display.portrait.title`、`settings.general.display.conversation.title`。

### 阶段 3a 验收

- [ ] `pnpm typecheck`、`pnpm test` 全绿，新测试 `displayLayoutMap.spec.ts` 通过。
- [ ] 示意图里两块屏的相对位置、大小比例与 Windows"显示设置"里的排列一致（在真机上对照）。
- [ ] 点一块屏 → 它高亮，下方"背景"那排切换到这块屏的设置；选一张图后真实壁纸对应那块屏的背景变化。
- [ ] 切换"立绘""对话窗"所在屏后，示意图上的标签立即移动，真实壁纸也跟着移动。
- [ ] 只有一块屏时，整张多屏卡片不显示（现有行为，不能改坏）。

---

## 6. 阶段 3b：外观页实时预览

位置：外观页（`page === 'appearance'`）最上方，在"背景"分区之前。

### 6.1 组件

新建 `wallpaper/src/settings/AppearancePreview.tsx`，props：

```ts
{
  backgroundUrl?: string     // 当前全局背景，经过 assetUrl()；纯色背景时为 undefined
  portraitUrl: string        // 与侧栏角色卡同一个立绘
  ambientStrength: number    // settings.portraitAmbientStrength，0–1
  bubbleText: string         // 新键 settings.appearance.preview.bubble
}
```

渲染：

- 外框 16:10，宽 `min(100%, 480px)`，圆角 10px，`overflow:hidden`，`box-shadow: 0 18px 40px rgba(0,0,0,.45)`；
- 底图：`backgroundUrl` 有值用 `<img alt="">` 铺满，否则底色 `var(--blank-background)`；
- 立绘：绝对定位 `right: 10%; bottom: 5%; height: 89%`，`filter` 由 `ambientStrength`（记为 s）计算：
  ```
  brightness(${1 - 0.24 * s}) saturate(${1 - 0.18 * s}) drop-shadow(-3px 0 ${4 + 6 * s}px rgba(90, 184, 245, ${0.25 + 0.4 * s}))
  ```
- 脚底阴影：立绘下方一个 120×18 的椭圆，`background: radial-gradient(closest-side, rgba(0,0,0,.6), rgba(0,0,0,0))`；
- 气泡：绝对定位在立绘头部左上方，最大宽 170px，内边距 `8px 12px`，圆角 `12px 12px 4px 12px`，底 `rgba(9,16,28,.86)`，边 `1px solid var(--s-accent)`，12px / 1.5；
- 下方说明文字 12px `--s-text-3`：新键 `settings.appearance.preview.caption`（中文"预览为近似效果，以桌面实际显示为准"）。

**说明**：这是近似效果，不复用壁纸里 `usePortraitEnvironmentBlend` 的真实算法（它依赖背景取色，放进设置窗代价太大）。只要求强度变化时预览有可见的对应变化。

### 6.2 接入

- 在外观页最上方渲染 `<AppearancePreview />`。
- `portraitAmbientLength` / `portraitAmbientStrength` 两个滑块**保持在常规页原位置不动**（挪位置可能撞上测试）。预览读的是同一份 `settings`，在常规页调完回到外观页能看到变化即可。
- 背景缩略图网格（`.background-grid`）样式按 4.1 的思路收紧：按钮底 `#0E1928`，边 `1.5px solid var(--s-line)`，选中时边 `var(--s-accent)` 并加 `box-shadow: 0 0 0 3px var(--s-accent-soft)`；角标"当前"改为 `var(--s-accent-fill)` 底、`var(--s-on-accent)` 字、11px。

### 6.3 新增 i18n 键

| 键 | 中文 | English |
|---|---|---|
| `settings.appearance.preview.label` | 外观预览 | Appearance preview |
| `settings.appearance.preview.bubble` | 早上好！今天要做什么呢？ | Good morning! What shall we do today? |
| `settings.appearance.preview.caption` | 预览为近似效果，以桌面实际显示为准 | Approximate preview; the desktop is the reference |

预览外框加 `role="img"` 和 `aria-label={t('settings.appearance.preview.label')}`。

### 阶段 3b 验收

- [ ] `pnpm typecheck`、`pnpm test` 全绿。
- [ ] 在外观页切换背景，预览底图立即变化。
- [ ] 在常规页把环境强度从 0 拉到 100%，回到外观页，预览里立绘明显变暗、偏冷、出现蓝色轮廓光。
- [ ] 切换聊天模式，预览里的立绘和气泡描边颜色跟着变。
- [ ] 窗口变窄时预览按比例缩小，不溢出。

---

## 7. 本计划**不做**的事

以下内容出现在效果图里，但需要原生侧或壁纸侧的配合，留待以后单独立项，本次不要做：

- 外观页的"脚底阴影""呼吸动效"开关：壁纸本身还没有这两个功能，需要新增设置字段并做 `store.ts` 版本迁移。
- 在真实屏幕上弹出编号的"识别屏幕"：需要原生创建临时窗口。
- 更换页面标题字体（例如 MiSans）：需要把字体文件打进安装包，并确认授权。
- Lite 版设置窗（`lite/LiteSettingsWindow.tsx`）的改版。
- 立绘读取用户在外观库里替换过的自定义立绘：本次侧栏和预览只用官方默认立绘。

---

## 8. 常见坑

1. **`SettingsPanel.css` 一行写了很多条规则**。用查找替换批量改颜色时，先确认匹配到的不是 `.official-persona-card` 那几条（它们有自己的角色色）。
2. **不要删 i18n 键**。导航副标题不再显示在导航里，但 `nav.*.hint` 仍是页面副标题；`settings.page.heading` 仍在 DOM 里。删键可能让别的测试失败。
3. **下拉菜单**：卡片去掉 `overflow:hidden` 后，旧的 `.settings-card:has(.settings-choice.is-open){overflow:visible}` 不再需要，但确认 `.settings-content` 自己的滚动容器不会把菜单裁掉。页面最后一行的下拉要实测。
4. **`officialPersonaCardFor` 的第二个参数是模型档位**，设置中心拿不到实时档位，固定传 `'pro'`。
5. **背景路径**：`BACKGROUND_OPTIONS[i].path` 是相对路径，要过 `assetUrl()`；纯色背景（id 为 `default`）没有 path，用 `var(--blank-background)`。
6. **焦点样式**：去掉旧样式时不要顺手把 `outline: 0` 留在可交互元素上，否则键盘用户看不到焦点。
7. **一个阶段一个提交**。某个阶段做不完就停在那里交付，不要把半成品混进下一个阶段。
