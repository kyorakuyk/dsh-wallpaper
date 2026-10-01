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

  // 外观（`appearance/theme/validation.ts`）：主题包体检报告里的每一句。槽位、版本号与路径来自被检查
  // 的那个包本身，所以用插值填进去 —— 句子是这里的，包的名字不是。
  'appearance.validation.component-invalid': '{slot} 的组件声明无效',
  'appearance.validation.single-asset-required': '{slot} 必须声明为单文件 asset',
  'appearance.validation.wake-sequence-frames-required': 'wake.sequence 必须包含至少一帧',
  'appearance.validation.wake-sequence-frame-invalid': 'wake.sequence 包含无效帧或时长',
  'appearance.validation.skin-required': 'chat.skin 必须声明为 skin',
  'appearance.validation.skin-textures': 'chat.skin textures 必须是路径数组',
  'appearance.validation.manifest-object': 'theme.json 必须是对象',
  'appearance.validation.schema-version': '不支持 schemaVersion {version}',
  'appearance.validation.kind-theme': 'kind 必须为 theme',
  'appearance.validation.id-format': '主题 id 格式无效',
  'appearance.validation.version-format': '主题 version 必须是语义版本',
  'appearance.validation.name-empty': '主题 name 不能为空',
  'appearance.validation.optional-text-type': '可选文本字段类型无效',
  'appearance.validation.min-app-version': 'compatibility.minAppVersion 格式无效',
  'appearance.validation.baseline-lock': 'baseline 必须锁定有效的 id 和版本',
  'appearance.validation.components-object': 'components 必须是对象',
  'appearance.validation.unknown-slot': '未知外观槽位 {slot}',
  'appearance.validation.files-array': 'files 必须是文件清单数组',
  'appearance.validation.file-entry': 'files 中存在无效条目',
  'appearance.validation.ui-declaration': 'ui 声明无效',
  'appearance.validation.listed-path-unsafe': '文件清单路径不安全：{path}',
  'appearance.validation.listed-hash-format': 'SHA-256 格式无效：{path}',
  'appearance.validation.listed-duplicate': '文件清单重复：{path}',
  'appearance.validation.package-path-unsafe': '包内路径不安全：{path}',
  'appearance.validation.package-symlink': '主题包不允许符号链接：{path}',
  'appearance.validation.package-duplicate': '包内文件重复：{path}',
  'appearance.validation.reference-path-unsafe': '资源引用路径不安全：{path}',
  'appearance.validation.reference-unlisted': '资源引用未列入 files：{path}',
  'appearance.validation.missing-file': '主题包缺少文件：{path}',
  'appearance.validation.size-mismatch': '文件大小不匹配：{path}',
  'appearance.validation.hash-mismatch': '文件哈希不匹配：{path}',
  'appearance.validation.unexpected-file': '主题包包含未声明文件：{path}',

  // 外观槽位与素材那一小行（`features/appearance/appearanceViewModel.ts`）：单品菜单与分类面板里的
  // 名字和说明，加上素材卡片上"这是什么媒体"的短词。
  'appearance.slot.desktop.background.label': '桌面背景',
  'appearance.slot.desktop.background.short': '背景',
  'appearance.slot.desktop.background.description': '桌面场景的底图',
  'appearance.slot.lockscreen.image.label': '锁屏图片（预留）',
  'appearance.slot.lockscreen.image.short': '锁屏',
  'appearance.slot.lockscreen.image.description': '素材库可预先归类；当前锁屏接管固定使用内置熟睡画面',
  'appearance.slot.wake.sequence.label': '苏醒动画',
  'appearance.slot.wake.sequence.short': '苏醒',
  'appearance.slot.wake.sequence.description': '解锁后播放的有序帧组',
  'appearance.slot.persona.deepseek.flash.label': 'DeepSeek Flash 立绘',
  'appearance.slot.persona.deepseek.flash.short': '蓝色幼年',
  'appearance.slot.persona.deepseek.flash.description': 'DeepSeek Flash 模型形态',
  'appearance.slot.persona.deepseek.pro.label': 'DeepSeek Pro 立绘',
  'appearance.slot.persona.deepseek.pro.short': '蓝色成年',
  'appearance.slot.persona.deepseek.pro.description': 'DeepSeek Pro 模型形态',
  'appearance.slot.persona.harness.flash.label': 'Harness Flash 立绘',
  'appearance.slot.persona.harness.flash.short': '黑红幼年',
  'appearance.slot.persona.harness.flash.description': 'Harness Flash 模型形态',
  'appearance.slot.persona.harness.pro.label': 'Harness Pro 立绘',
  'appearance.slot.persona.harness.pro.short': '黑红成年',
  'appearance.slot.persona.harness.pro.description': 'Harness Pro 模型形态',
  'appearance.slot.chat.skin.label': '对话气泡皮肤',
  'appearance.slot.chat.skin.short': '气泡',
  'appearance.slot.chat.skin.description': '声明式玻璃材质与纹理',
  'appearance.slot.ui.font.label': '界面字体',
  'appearance.slot.ui.font.short': '字体',
  'appearance.slot.ui.font.description': '聊天和菜单使用的字体',
  'appearance.asset.meta.font': '字体',
  'appearance.asset.meta.sequence': '动画序列',
  'appearance.asset.meta.skin': '气泡皮肤',
  'appearance.asset.meta.image': '图片',
  // 有透明通道时接在尺寸后面那半句（中文含前导空格，与 `chat.cost.approx` 同一个理由）。
  'appearance.asset.meta.transparent': ' · 透明背景',

  // 外观抽屉（`features/appearance/AppearanceDrawer.tsx`）。继承那一行拆成"前缀 + 分隔符"两段：
  // 中文的顿号与英文的逗号不是同一个字符，所以分隔符也得随语言走。
  'appearance.drawer.title': '外观',
  'appearance.drawer.description': '主题决定整体风格，独立素材可以覆盖其中一个组件。',
  'appearance.drawer.import': '导入',
  'appearance.drawer.export': '导出当前搭配',
  'appearance.drawer.themes-title': '主题',
  'appearance.drawer.themes-hint': '切换主题会清除当前的单项替换。',
  'appearance.drawer.source-official': '官方',
  'appearance.drawer.source-user': '用户',
  'appearance.drawer.current': '当前主题',
  'appearance.drawer.inherited-count': '项继承官方基线',
  'appearance.drawer.override-count': '项独立素材覆盖',
  'appearance.drawer.inheritance': '继承：{slots}',
  'appearance.drawer.inheritance-separator': '、',
  'appearance.drawer.reset': '恢复主题默认',
  'appearance.drawer.components-title': '单项组件',
  'appearance.drawer.components-hint': '只显示已分类的独立素材，不会拆开主题包。',
  'appearance.drawer.use-default': '使用主题默认',
  'appearance.drawer.slot-menu': '{slot}素材',
  'appearance.drawer.menu-empty': '还没有适用于此组件的独立素材',
  'appearance.drawer.inbox-title': '待分类区',
  'appearance.drawer.inbox-hint': '确认用途后，素材才会进入对应组件菜单。',
  'appearance.drawer.review-all': '全部整理',
  'appearance.drawer.inbox-empty': '待分类区是空的',
  'appearance.drawer.inbox-empty-hint': '导入图片、字体、文件夹或主题包',
  'appearance.drawer.classify': '分类',
  'appearance.drawer.show-more': '查看其余 {count} 项',
  'appearance.drawer.choose-file': '选择文件',
  'appearance.drawer.choose-folder': '选择文件夹',

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

  // 指示器悬停时的解释（`connect/conversationHost.ts`）。`{label}` 填的是执行主体的类别词，
  // 它自己也是词条（见上面 harness.subject.kind.*）；`{alias}` 是用户给主体起的名字。
  // 元素上那四个短词（`Web` / `API` / `Desktop` / `TUI`）**不是**词条：它们是两种语言里都一样的
  // 产品与形态名，见那个文件顶部的说明。
  'chat.host.alias': '（别名：{alias}）',
  'chat.host.deepseek-api': '你自己的 DeepSeek API key，按 token 计费。',
  'chat.host.deepseek-web': 'DeepSeek 网页额度，不产生 API 费用。',
  'chat.host.embedded-shell': '本机 DeepSeek Harness 客户端，它带自己的窗口。',
  'chat.host.terminal': '{label}，界面在终端里。',
  'chat.host.browser': '{label}，界面在浏览器里。',
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
