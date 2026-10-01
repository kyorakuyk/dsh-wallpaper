/**
 * 中文词条 —— **真源**。
 *
 * 规则（见 `docs/plans/i18n-plan.md`）：
 * 1. 这里的中文是**从界面上原样搬过来的**，不是重写的 —— 所以"中文界面没有变化"是可验证的；
 * 2. 键按界面位置命名（`settings.general.title`、`language.label`），不按句意命名；
 * 3. 新增一个键，`en.ts` 不同步就会**编译不过**（`Dict` 由这里推导）；
 * 4. 需要插值的地方用 `{name}`，由 `t(key, { name })` 填。
 */
export const zh = {
  // 界面语言（设置中心）
  'language.label': '界面语言',
  'language.hint': '切换后立即生效，选择会保存。',
  'language.zh': '简体中文',
  'language.en': 'English',

  // 设置中心：六个页签。label 是页签上的字，hint 是它下面那行小字（也是页标题右侧的说明）。
  'nav.general.label': '常规',
  'nav.general.hint': '启动与使用方式',
  'nav.connections.label': '连接',
  'nav.connections.hint': 'DeepSeek 与 DSH',
  'nav.appearance.label': '外观',
  'nav.appearance.hint': '背景与动画',
  'nav.personas.label': '形态',
  'nav.personas.hint': '模型映射规则',
  'nav.history.label': '历史',
  'nav.history.hint': 'API 会话记录',
  'nav.system.label': '系统',
  'nav.system.hint': 'Windows 集成',

  // 状态栏
  'statusbar.autosave': '设置会自动保存',

  // 语言卡片
  'language.card': '语言',
}

/** 所有可用键。写错键名在编译期就会被挡住。 */
export type MessageKey = keyof typeof zh

/**
 * 一份完整词条的形状。
 *
 * `en.ts` 用它标注，于是"少一个键"或"多一个键"都会在 `pnpm typecheck` 里失败 ——
 * 比"跑测试才发现漏了"更早，而且不需要为完整性写测试。
 */
export type Dict = Record<MessageKey, string>
