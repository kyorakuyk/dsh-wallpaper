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

  // 开机自启（`settings/autostartCopy.ts`）：当前由哪条路承载，以及"这次变更没有生效"的说明。
  // `reason` 是原生读回来的原因原文，照旧接在句子后面（它是运行时的系统文字，不在词条里）。
  'autostart.detail.startup-task': '登录后由 Windows 启动任务启动本应用。',
  'autostart.detail.startup-task.disabled': 'Windows 启动任务未启用。',
  'autostart.detail.run': '登录后由当前用户启动项启动本应用。',
  'autostart.detail.disabled-by-user': 'Windows 已禁用本应用的开机启动。',
  'autostart.detail.disabled-by-policy': 'Windows 策略禁止本应用开机启动。',
  'autostart.detail.unsupported': '当前系统不支持本应用的开机自启。',
  'autostart.detail.reading': '正在读取 Windows 的启动状态…',
  // 用户这次想要的方向，填进下面两句的 `{wanted}`。
  'autostart.refusal.wanted-on': '打开',
  'autostart.refusal.wanted-off': '关闭',
  'autostart.refusal.with-reason': '开机自启没有{wanted}：{reason}',
  'autostart.refusal.generic': '开机自启没有{wanted}；Windows 没有接受这次变更，请在系统设置的「启动应用」里检查本应用。',

  // Harness 连接状态（`connect/harnessLabels.ts`）。
  // 第一组是每个"没就绪"状态的一句说明：缺什么、怎么修。`bridge-ready` 是"没有问题"，
  // 所以它没有句子 —— 空串留在代码里，因为字典不收空词条（`tests/i18n.spec.ts` 钉着这一条）。
  'harness.detail.offline': '未能连接到本机的 DSH 壁纸 Bridge。',
  'harness.detail.web-only': '检测到 DSH 服务，但壁纸 Bridge 未安装、未启动或不兼容。',
  'harness.detail.bridge-loading': 'DSH 壁纸 Bridge 已启动，正在装载会话服务。',
  'harness.detail.bridge-auth-unavailable': 'DSH 壁纸 Bridge 已启动，但本机访问令牌不可用，请重启壁纸应用。',
  'harness.detail.bridge-incompatible': 'DSH 壁纸 Bridge 的版本或能力与本壁纸不兼容（profile 内可能是过旧的副本），请更新 Bridge 后重试。',
  // 第二组是气泡与设置侧栏上那枚状态点的短标签（黄灯一律说"连接中"）。
  'harness.label.connecting': '连接中',
  'harness.label.bridge-ready': 'DSH Bridge 已连接',
  'harness.label.bridge-auth-unavailable': 'DSH Bridge 令牌不可用',
  'harness.label.bridge-incompatible': 'DSH Bridge 版本不兼容',
  'harness.label.web-only': 'DSH 在线，缺少 Bridge',
  'harness.label.offline': 'DSH 当前离线',

  // 执行主体（`connect/harnessSubjects.ts`）：类别词、别名那一段名字、扫描时间与启动结果。
  // 类别词只有三个，而启动结果那句话把类别词填进 `{label}`，于是"结果行"与"它指的那一行"
  // 不可能读成两件事。
  'harness.subject.kind.embedded-shell': '客户端',
  'harness.subject.kind.installed-cli': '已安装的 CLI',
  'harness.subject.kind.checkout': '源码目录',
  'harness.subject.detail.shell': '客户端自带运行环境，不需要填路径。',
  'harness.subject.choice-prompt': '检测到您电脑上安装了 {count} 个 deepseek harness 源码树，请选择默认主体。',
  'harness.subject.age.just-now': '刚刚验证',
  'harness.subject.age.minutes': '{minutes} 分钟前验证',
  'harness.subject.age.hours': '{hours} 小时前验证',
  'harness.subject.age.days': '{days} 天前验证',
  // 源码目录在下拉里的一整行：`类别词 · 名字` + 有版本时才有的 ` · 版本` 那一段。
  'harness.subject.option.checkout': '源码目录 · {name}{version}',
  'harness.subject.instance.port-unknown': '端口未确认',
  'harness.subject.launch.started': '已启动{label}。',
  'harness.subject.launch.started-hidden': '已启动{label}，窗口已在后台；需要用「拉起窗口」把它调出来。',
  'harness.subject.launch.started-unconfirmed': '{label}接受了启动请求，但在超时时间内没有应答；它可能仍在启动，稍后刷新即可。',
  'harness.subject.launch.already-running': '{label}已在运行；本应用不会接管、重启或停止它。',
  'harness.subject.launch.unknown-target': '还没有选好要启动谁，请先扫描并选择一个。',
  'harness.subject.launch.root-path-invalid': '这个目录不是可用的 DSH 源码目录，请重新扫描后选择。',
  'harness.subject.launch.launcher-missing': '找不到用来启动它的程序：请确认 Node.js 或 pnpm 已安装，并能在命令提示符里直接运行。',
  'harness.subject.launch.profile-invalid': 'profile 无效：只能包含字母、数字、连字符或下划线。',
  'harness.subject.launch.port-occupied-external': '本机已有别的程序占用该端口（不是本应用启动的），因此没有重复启动，也不会去接管或停止它。',
  'harness.subject.launch.failed': '启动没有成功。日志里有这次启动的完整记录，可用于排查。',

  // 会话气泡（`features/chat/conversationViewModel.ts`）：后端说明、活动状态、用量与费用、输入框。
  'chat.backend.deepseek-web.name': 'DeepSeek 网页桥接',
  'chat.backend.deepseek-web.description': '免费 · 实验能力',
  'chat.backend.deepseek-api.description': '按量计费',
  'chat.backend.harness.description': '本地工具会话',
  'chat.activity.idle': '待命',
  'chat.activity.sending': '正在发送',
  'chat.activity.thinking': '正在思考',
  'chat.activity.streaming': '正在回复',
  'chat.activity.tool': '正在使用工具',
  'chat.activity.done': '已完成',
  // 估算是"约"，所以金额前面那一段也随语言走（中文是"约 "，含尾随空格）。
  'chat.cost.approx': '约 ',
  'chat.usage.unavailable': '未提供',
  'chat.usage.price-unconfigured': '价格未配置',
  'chat.usage.cost-unavailable': '费用未提供',
  'chat.composer.disabled': '当前模式暂不可用',
  'chat.composer.busy': '大肥鱼正在处理上一条消息…',
  'chat.composer.placeholder': '今天要一起处理什么？',
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
