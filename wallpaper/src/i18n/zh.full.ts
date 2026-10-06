/**
 * 中文词条 —— **只有完整版会说的那一半**（真源）。
 *
 * Lite 入口（`src/main-lite.tsx`）只登记 `zh.shared.ts`（见 `i18n/lite.ts`），所以这里的句子不会进
 * `dist-lite` 的产物；完整版入口登记两份合起来的 `zh.ts`（见 `i18n/full.ts`）。
 *
 * 如果一条键被 Lite 的可达模块用到了，它就该搬回 `zh.shared.ts` —— `tests/liteI18nBoundary.spec.ts`
 * 会把漏搬的那条指出来，而不是等产物里冒出完整版的字样。
 *
 * 规则与 `zh.shared.ts` 相同：中文是从界面上原样搬过来的，键按位置命名，需要插值用 `{name}`。
 */
export const zhFull = {
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
  'statusbar.saved': '已保存',

  // 语言卡片
  'language.card': '语言',

  // ---------------------------------------------------------------------------
  // 设置中心的六张页面（`settings/SettingsPanel.tsx`）。键按**位置**命名：`settings.<页面>.<项>`，
  // 与页签的 `nav.*` 一样。这里只放卡片与控件自己的话；从别处来的句子留在原来的键里
  // （比如「打开」的候选文字属于 `connect/openRoutes.ts`，槽位名字属于 `appearance.slot.*`）。
  // ---------------------------------------------------------------------------

  // 设置中心的外壳：品牌副标题、关闭按钮、页标题前缀（`设置 / 常规`）。
  'settings.brand.subtitle': '个性化控制中心',
  'settings.window.close': '关闭设置',
  'settings.page.heading': '设置 / {page}',
  // 通用动作词：刷新、打开（多处复用，所以放在页面这一层而不是某一页下面）。
  'settings.refresh': '刷新',
  'settings.open': '打开',
  // 下拉在没有候选时显示的那一格（`Choice` 的兜底文字）。
  'settings.choice.empty': '请选择',

  // 常规 · 交互方式
  'settings.general.interaction.title': '交互方式',
  'settings.general.interaction.description': '决定会话气泡如何出现在桌面上。',
  'settings.general.conversation-window.title': '中央会话窗',
  'settings.general.conversation-window.detail': '关闭后仅可通过托盘右键或此处重新打开',
  'settings.general.conversation-window.toggle': '显示中央会话窗',
  'settings.general.bubble-layout.title': '气泡布局',
  'settings.general.bubble-layout.detail': '中央悬浮始终展开；任务栏停靠以胶囊按钮唤起。',
  // 两个取值（`floating` / `taskbar-docked`）是内部标识，只有标签是词条。
  'settings.general.bubble-layout.floating': '中央玻璃悬浮',
  'settings.general.bubble-layout.taskbar-docked': '任务栏停靠胶囊',
  'settings.general.history-drawer.title': '历史抽屉默认展开',
  'settings.general.history-drawer.detail': '启动或解锁后直接显示最近的对话。',
  'settings.general.history-drawer.toggle': '历史抽屉默认展开',
  'settings.general.shortcut.title': '发送消息快捷键',
  'settings.general.shortcut.detail': '想防止误触发送的开发者可切换为 Ctrl+Enter 发送。',
  'settings.general.shortcut.enter': 'Enter 发送，Ctrl+Enter 换行',
  'settings.general.shortcut.ctrl-enter': 'Ctrl+Enter 发送，Enter 换行',

  // 常规 · 多屏桌面（每块屏幕一行，插值填显示器序号与分辨率）
  'settings.general.multi-screen.title': '多屏桌面 · 已检测 {count} 个屏幕',
  'settings.general.multi-screen.description': '每块屏幕独立铺满自己的背景；对话窗和立绘可以分别指定目标屏幕。未单独指定的屏幕跟随全局背景。',
  'settings.general.multi-screen.toggle': '启用独立多屏背景',
  'settings.general.multi-screen.on': '已按屏幕分别渲染；修改某一屏不会改变其他屏幕的背景选择。',
  'settings.general.multi-screen.off': '关闭时保持现有跨虚拟桌面的单一场景；开启后才显示逐屏选择。',
  // 显示器的名字与它那一行参数；`{number}` / `{index}` 是系统给的编号，只有句子随语言走。
  'settings.general.display.number': '显示器 {number}',
  'settings.general.display.fallback': '显示器 {index}',
  'settings.general.display.primary-name': '主屏',
  'settings.general.display.secondary-name': '副屏 {number}',
  'settings.general.display.metrics': '{width} × {height} 像素 · 缩放 {scale}%',
  'settings.general.display.primary': ' · 主显示器',
  'settings.general.display.background': '{display} 背景',
  'settings.general.display.follow-global': '跟随全局背景',
  'settings.general.display.conversation.title': '对话窗所在屏幕',
  'settings.general.display.conversation.detail': '只移动会话层，不重新加载其他屏幕的背景。',
  'settings.general.display.portrait.title': '立绘所在屏幕',
  'settings.general.display.portrait.detail': '立绘和头顶气泡只挂载到选中的屏幕。',
  'settings.general.display.refresh': '刷新显示器检测',
  'settings.general.display-map.label': '显示器布局',
  'settings.general.display-map.identify': '识别屏幕',
  'settings.general.display-map.hide-numbers': '隐藏编号',
  'settings.general.display-map.select': '选择{display}',
  'settings.general.display-map.portrait-tag': '立绘',
  'settings.general.display-map.chat-tag': '对话窗',
  'settings.general.display-map.background-of': '{display}的背景',
  'settings.general.display-map.background-hint': '只影响这一块屏幕',
  'settings.general.display-map.follow-global': '跟随全局',

  'settings.appearance.preview.label': '外观预览',
  'settings.appearance.preview.bubble': '早上好！今天要做什么呢？',
  'settings.appearance.preview.caption': '预览为近似效果，以桌面实际显示为准',

  // 常规 · 会话生命周期
  'settings.general.conversation-lifecycle.title': '会话生命周期',
  'settings.general.conversation-policy.title': '新会话策略',
  'settings.general.conversation-policy.detail': '网页模式会固定到同一个 DeepSeek 会话地址；其他后端分别保留自己的最近会话。',
  // 三个取值（`resume-last` / `new-on-unlock` / `daily`）同样是内部标识。
  'settings.general.conversation-policy.resume-last': '恢复最近会话',
  'settings.general.conversation-policy.new-on-unlock': '每次解锁新建',
  'settings.general.conversation-policy.daily': '每日新建',

  // 常规 · 高级外观（测试中）
  'settings.general.advanced.title': '高级外观（测试中）',
  'settings.general.advanced.description': '环境渐变只作用于立绘；会话窗使用独立的亚克力透明度。',
  'settings.general.ambient-length.title': '环境渐变长度',
  'settings.general.ambient-length.detail': '从暗侧向亮侧延伸至 {percent}%',
  'settings.general.ambient-strength.title': '环境渐变强度',
  'settings.general.conversation-opacity.title': '中央会话窗透明度',
  'settings.general.conversation-opacity.detail': '{percent}% · 仅影响亚克力底色，不影响文字可读性',
  'settings.general.conversation-blur.title': '中央会话窗磨砂',
  'settings.general.conversation-blur.detail': '{pixels}px · 0 为纯透明玻璃，数值越高背景越柔和',

  // 连接 · 聊天模式
  'settings.connections.chat-mode.title': '聊天模式',
  'settings.connections.chat-mode.description': '二选一。更改会话的通道，会记作下次启动的默认值；网页桥接不会在失败时自动切到付费 API。',
  'settings.connections.chat-mode.field-title': '当前使用',
  'settings.connections.chat-mode.field-detail': '滑槽在左边时，聊天走这里选的通道；改动会记作下次启动的默认值。',
  'settings.connections.chat-mode.auto-switch.title': 'DSH 就绪时自动切换',
  'settings.connections.chat-mode.auto-switch.detail': '当 harness 就绪时自动切换到 harness 模式。',
  'settings.connections.chat-mode.auto-switch.toggle': 'DSH 自动切换',
  // 后端 → 显示名（旧的 `BACKEND_MODE_LABELS` 表）。`harness` 那一格是产品名，两种语言里一样。
  'settings.connections.backend.deepseek-web': 'DeepSeek 网页入口（实验）',
  'settings.connections.backend.deepseek-api': 'DeepSeek API（付费）',

  // 连接 · DeepSeek Harness
  'settings.connections.harness.title': 'DeepSeek Harness 连接',
  'settings.connections.harness.description': '当第一次使用与本机含有多个不同dsh时使用',
  'settings.connections.subject.title': '运行方式',
  'settings.connections.subject.scanning': '正在后台搜索可识别的运行方式，请稍候。',
  'settings.connections.subject.none': '点「扫描」找出本机可以运行的 DSH。',
  'settings.connections.subject.found': '已发现 {count} 个可选项{age}。',
  'settings.connections.subject.current': '当前：{path}',
  'settings.connections.subject.scanning-button': '扫描中…',
  'settings.connections.subject.rescan': '重新扫描',
  'settings.connections.subject.scan': '扫描',
  'settings.connections.root-path.title': '源码目录',
  'settings.connections.root-path.detail': '这份源码的位置；扫描会用它作为下一次查找的提示路径。',
  'settings.connections.open.title': '打开界面',
  'settings.connections.open.detail.no-subject': '先在上面选定运行方式。',
  'settings.connections.open.detail.shell': '会把它自己的窗口调到前台；如果它没在运行，会先把它启动起来。',
  'settings.connections.open.detail.multi': '浏览器用它的网页界面；TUI 会在一个新的终端窗口里打开。没在运行时都会先把它启动起来。',
  'settings.connections.open.detail.single': '会用它的网页界面（默认浏览器）；如果它没在运行，会先把它启动起来。',
  'settings.connections.open.window-label': '拉起的窗口',
  'settings.connections.open.busy': '处理中…',
  'settings.connections.open.action': '打开',
  'settings.connections.launch-with-wallpaper.title': '随壁纸启动 DSH',
  'settings.connections.launch-with-wallpaper.shell': '壁纸启动时自动把该客户端跑起来（它不支持静默启动时会直接出现窗口）。要让它随登录生效，还需要在「系统」里开启壁纸开机自启。已经在运行的实例不会被接管或重启。',
  'settings.connections.launch-with-wallpaper.checkout': '壁纸启动时自动把该源码目录跑起来。要让它随登录生效，还需要在「系统」里开启壁纸开机自启。已经在运行的实例不会被接管或停止。',
  'settings.connections.autostart-warning.title': '壁纸开机自启未生效',
  'settings.connections.autostart-warning.detail': '开机后自动启动 DSH 依赖壁纸自身的开机自启。',
  'settings.connections.autostart-warning.disabled-by-user': 'Windows 任务管理器已禁用本应用的自启项，请在任务管理器中重新启用。',
  'settings.connections.autostart-warning.disabled-by-policy': '系统策略禁用了本应用的自启项，请联系管理员。',
  'settings.connections.autostart-warning.not-configured': '壁纸尚未设置开机自启，请在「系统」页开启壁纸开机自启。',
  'settings.connections.managed.title': '本应用启动的 DSH',
  'settings.connections.managed.yes': '该 DSH 由本应用启动，可以在这里停止它。',
  'settings.connections.managed.no': '本应用没有启动 DSH；其他人启动的实例不会被停止。',
  'settings.connections.managed.stop': '停止本应用启动的 DSH',

  // 连接 · DeepSeek 网页入口与应用内 API
  'settings.connections.web.title': 'DeepSeek 网页入口（实验）',
  'settings.connections.web.description': '在壁纸里用你的网页版账号对话；登录后直连。',
  'settings.connections.web.page.title': '页面',
  'settings.connections.web.page.detail': '页面和登录状态由独立 WebView2 配置目录保存；本应用不读取、复制或记录 Cookie。',
  'settings.connections.web.page.open': '打开应用内页面',
  'settings.connections.web.adapter.title': '网页适配（高级）',
  'settings.connections.web.adapter.detail': '网页结构变化时才需要动它，平常不用管。',
  'settings.connections.web.adapter.source-local': '本地 override',
  'settings.connections.web.adapter.source-bundled': '内置默认',
  'settings.connections.web.adapter.reading': '正在读取配置状态…',
  'settings.connections.web.adapter.open': '打开配置',
  'settings.connections.web.adapter.reset': '恢复默认',
  'settings.connections.api.title': 'DeepSeek API',
  'settings.connections.api.description': 'API 模式会产生实际费用，密钥只保存在 Windows 凭据管理器。',
  'settings.connections.api.key.title': '访问密钥',
  'settings.connections.api.key.detail': '在这里填入 DeepSeek API Key，按测试确认连通性，自动拉取可用模型。',
  'settings.connections.api.key.aria': 'DeepSeek API Key',
  'settings.connections.api.key.replace': '填入新的 Key 以替换',
  'settings.connections.api.key.placeholder': 'sk-…',
  'settings.connections.api.key.test': '测试',
  'settings.connections.api.saved.title': '已保存',
  'settings.connections.api.saved.present': '这是凭据管理器里那一条的脱敏形态。',
  'settings.connections.api.saved.absent': '还没有保存过 API Key。',
  'settings.connections.api.saved.aria': '已保存的 API Key（脱敏）',
  'settings.connections.api.saved.unconfigured': '未配置',
  'settings.connections.api.model.title': '模型',
  'settings.connections.api.model.available': '可用模型 {count} 项{age}。换了一批名字就按「刷新」。',
  'settings.connections.api.model.hint': '按「刷新」拉取可用模型；拉到的列表会记下来，下次打开设置直接显示。',
  'settings.connections.api.model.label': 'DeepSeek API 模型',
  'settings.connections.api.model.empty': '还没有拉取到模型列表，先按「刷新」。',
  'settings.connections.api.model.stale': '{model}（不在当前目录里）',
  // 模型列表那句说明里"什么时候拉的"（持久化缓存，可能是几天前拉的）。
  'settings.connections.api.model.age.just-now': '（刚刚拉取）',
  'settings.connections.api.model.age.minutes': '（{minutes} 分钟前拉取）',
  'settings.connections.api.model.age.hours': '（{hours} 小时前拉取）',
  'settings.connections.api.model.age.days': '（{days} 天前拉取）',
  // FREEZE（临时冻结，不是删除）：价格这一组词条的**显示**随两个价格输入一起冻结（见
  // `settings/SettingsPanel.tsx` 里那两行 Field 的 FREEZE）：标题／说明／标签／占位原文一字未改，
  // 恢复时把那两行取消注释即可。其中「输入价格」那句说明（"……才会显示本轮和会话估算费用"）
  // 说的显示面上一批已经冻结，所以它暂时不会渲染出来。
  'settings.connections.api.price-input.title': '输入价格',
  'settings.connections.api.price-input.detail': '人民币／每百万 input tokens。输入、输出价格都配置后，才会显示本轮和会话估算费用。',
  'settings.connections.api.price-input.label': '输入价格（人民币每百万 tokens）',
  'settings.connections.api.price-output.title': '输出价格',
  'settings.connections.api.price-output.detail': '人民币／每百万 output tokens。留空不会伪造零费用；缓存 token 没有单独价格时会标为估算。',
  'settings.connections.api.price-output.label': '输出价格（人民币每百万 tokens）',
  // 价格输入框的占位（`PriceInput`）。
  'settings.connections.api.price.placeholder': '未配置',

  // 外观 · 桌面背景与素材库
  'settings.appearance.background.title': '桌面背景',
  'settings.appearance.background.description': '内置背景与你的素材将保持独立。',
  'settings.appearance.background.current': '当前',
  'settings.appearance.library.title': '素材库（导入功能测试中）',
  'settings.appearance.library.description': '导入的单张素材先选择用途，再出现在对应组件的枚举菜单中。主题包和插件将在后续版本单独处理。',
  'settings.appearance.library.import': '导入图片素材',
  'settings.appearance.library.counts': '{inbox} 项待分类 · {usable} 项可用',
  'settings.appearance.asset.purpose': '{name} 的用途',
  'settings.appearance.asset.choose-purpose': '选择用途…',
  'settings.appearance.asset.image': '图片',
  'settings.appearance.asset.transparent': ' · 透明背景',
  'settings.appearance.component.detail': '{detail} · {count} 项可选',
  'settings.appearance.component.none': '暂无此类素材，请先导入并指定用途',
  'settings.appearance.component.use-default': '使用默认',
  // 素材库的组件行：只列五个立绘槽位与桌面背景，名字与说明与 `appearance.slot.*` 同义。
  // 键与 `componentSlots` 里那个 `slot` 一一对应（`persona.deepseek.flash` → `persona-deepseek-flash`）。
  'settings.appearance.component.desktop-background.label': '桌面背景',
  'settings.appearance.component.desktop-background.detail': '工作室场景的底图',
  'settings.appearance.component.persona-deepseek-flash.label': 'DeepSeek Flash 立绘',
  'settings.appearance.component.persona-deepseek-flash.detail': '蓝色幼年形态',
  'settings.appearance.component.persona-deepseek-pro.label': 'DeepSeek Pro 立绘',
  'settings.appearance.component.persona-deepseek-pro.detail': '蓝色成年形态',
  'settings.appearance.component.persona-harness-flash.label': 'Harness Flash 立绘',
  'settings.appearance.component.persona-harness-flash.detail': '黑红幼年形态',
  'settings.appearance.component.persona-harness-pro.label': 'Harness Pro 立绘',
  'settings.appearance.component.persona-harness-pro.detail': '黑红成年形态',
  // 外观 · 苏醒动画（开发中）
  'settings.appearance.wake.title': '苏醒动画（开发中）',
  'settings.appearance.wake.enabled.title': '启用动画',
  'settings.appearance.wake.enabled.toggle': '启用苏醒动画',
  'settings.appearance.wake.every-unlock.title': '每次解锁播放',
  'settings.appearance.wake.every-unlock.toggle': '每次解锁播放',
  'settings.appearance.wake.skip.title': '跳过苏醒过程',
  'settings.appearance.wake.skip.toggle': '跳过苏醒过程',

  // 形态 · 人物列表。一句话里那个"外观 → 素材库"是一条导航指向，所以整句不拆。
  'settings.personas.list.title': '人物列表',
  'settings.personas.list.description': '四张正式立绘是固定的后端／模型层级映射。此处只用于审阅；如需替换某张图，请到“外观 → 素材库”为对应槽位指定素材。',

  // 历史 · API 会话记录
  'settings.history.title': 'API 会话记录',
  'settings.history.description': 'DeepSeek API 模式的历史记录以当前 Windows 用户的加密档案保存在本机。删除只影响这份档案，不影响 DeepSeek 网页入口或 Harness 会话。',
  'settings.history.summary': '共 {conversations} 个会话 · {messages} 条消息 · {size}',
  'settings.history.budget': '（预算 {size}，超过后自动淘汰最旧记录）',
  'settings.history.refresh': '刷新',
  'settings.history.refreshing': '读取中…',
  'settings.history.clear': '清空全部 API 历史',
  'settings.history.usage': '已用 {percent}% 的应用预算；硬上限 {limit}，达到上限时本次运行会停止写入磁盘而不是覆盖已有记录。',
  'settings.history.loading.title': '正在读取 API 会话记录…',
  'settings.history.loading.detail': '首次读取需要解密本机档案。',
  'settings.history.empty.title': '没有可删除的 API 会话记录',
  'settings.history.empty.detail': '使用 DeepSeek API 模式发送过消息后，这里会出现可管理的会话。',
  'settings.history.active': '当前会话',
  'settings.history.row-meta': '{messages} 条消息 · {size} · 最后活动 {time}',
  'settings.history.row-delete': '删除 API 会话 {id}',
  'settings.history.delete': '删除',
  // 同一天的历史行只写时间，前面加"今天"；较早的那些写完整日期（那就是一串数字格式，不是句子）。
  'settings.history.today': '今天 {time}',

  // 系统 · 数据与目录
  'settings.system.data.title': '数据与目录',
  'settings.system.data.description': '「桌面会话」的工作区落在壁纸自己的数据目录里：升级安装会保留，卸载之后也留得下。若想彻底删除数据，在卸载前请先点击下面的“清除全部用户数据”。',
  'settings.system.workspace.title': '工作区',
  'settings.system.workspace.missing': '（还没创建；桥第一次用到时会在这里建出来）',
  'settings.system.memory.title': '项目记忆',
  'settings.system.memory.present': '（助手维护；说话人格等长期要求就写在这里）',
  'settings.system.memory.absent': '（还没有：你或助手第一次「记下来」时会出现）',
  'settings.system.reading': '正在读取…',
  'settings.system.memory.open': '打开项目记忆',
  'settings.system.memory.opening': '正在打开…',
  'settings.system.clear.title': '清除全部用户数据',
  'settings.system.clear.action': '清除全部用户数据',
  'settings.system.clear.clearing': '正在清除…',
  'settings.system.clear.detail': '删除本应用的桌面会话工作区，以及凭据管理器里保存的 API Key。设置与网页登录态在 WebView2 配置目录里，需要退出应用后手动删除（下面会给出路径）。',
  // 「清除全部用户数据」的确认框：一行一句，`{workspace}` 与 `{data}` 是原生报回来的真实路径。
  'settings.system.clear.confirm.intro': '将删除：',
  'settings.system.clear.confirm.workspace': '· 桌面会话工作区（{workspace}）',
  'settings.system.clear.confirm.workspace-fallback': '数据目录下的「桌面会话」',
  'settings.system.clear.confirm.credentials': '· 凭据管理器里保存的 DeepSeek API Key',
  'settings.system.clear.confirm.manual-heading': '需要你自己删（本应用正在运行，删不干净）：',
  'settings.system.clear.confirm.settings': '· 设置与网页登录态：{data}',
  'settings.system.clear.confirm.data-fallback': '%LOCALAPPDATA%\\com.dsh.wallpaper（退出后删除）',
  'settings.system.clear.confirm.bridge': '· 桥接凭据：~/.dsh/wallpaper',
  'settings.system.clear.confirm.continue': '继续？',
  // 清除结果那一行：原生的每一项都带上它的路径。
  'settings.system.clear.removed': '已删除 {path}',
  'settings.system.clear.credential-removed': '已删除凭据管理器里的 API Key',
  'settings.system.clear.credential-absent': '凭据管理器里没有这条 Key（无需删除）',
  'settings.system.clear.manual': '退出应用后手动删除：{what} → {path}',
  'settings.system.clear.join': '；',
  'settings.system.clear.failed': '清除失败：{error}',

  // 系统 · Windows 集成
  'settings.system.windows.title': 'Windows 集成',
  'settings.system.autostart.title': '登录后自动启动',
  'settings.system.autostart.toggle': '登录后自动启动',
  'settings.system.autostart.busy': '正在更新 Windows 启动任务，请稍候；设置中心仍可继续使用。',

  // 系统 · 更新（`features/update/*`）：卡片上的固定文案。结论、失败原因与两条动作的话在文件
  // 末尾的「更新检测」那一节，因为它们与立绘气泡**共用同一句话**。
  'settings.system.update.title': '更新',
  'settings.system.update.description': '更新来自 GitHub Releases。进入里桌面后会自动检查一次，之后每 6 小时最多一次；这里的「检查更新」不受这个限制。',
  'settings.system.update.current.title': '当前版本',
  'settings.system.update.current.unavailable': '读不到本机版本，因此不检查更新',
  'settings.system.update.last-check.title': '上次检查',
  'settings.system.update.last-check.never': '还没有检查过',
  'settings.system.update.check': '检查更新',
  'settings.system.update.checking': '正在检查…',

  // ---------------------------------------------------------------------------
  // 设置窗口自己说的话（`settings/SettingsWindow.tsx`）：提示条、通知、桥对齐反馈与自启提示。
  // 这些都是**运行时**组出来的句子（插值里是端口、主体名、错误对象），所以键按"哪一步"命名。
  // 提示条停留多久**不再**从这些字里反推（原来是拿 `/失败|错误|…/` 去猜），而由调用点给出
  // 语气 —— 句子一进词条，那种猜测在英文下就失效了。
  // ---------------------------------------------------------------------------

  // 提示条本身，以及三条跨页面的通用失败（设置同步、素材库、显示器列表）。
  'settings.window.notice.dismiss': '关闭通知',
  'settings.window.notice.sync-failed': '设置同步失败：{error}',
  'settings.window.notice.appearance-read-failed': '素材库读取失败：{error}',
  'settings.window.notice.displays-read-failed': '显示器列表读取失败：{error}',

  // 扫描执行主体。
  'settings.window.scan.done': '扫描完成，发现 {count} 个可选执行主体。',
  'settings.window.scan.none': '未发现 DSH 项目或已安装的客户端；可手动填写 DSH 项目根目录后再扫描。',
  'settings.window.scan.failed': '扫描 DSH 失败：{error}',

  // 端点扫描，以及"存着的端口已经不属于这个主体"那条自相矛盾的清理。
  'settings.window.endpoints.stale-port': '设置里选定的端口 {port} 不属于当前主体，已清除；「打开」会按该主体自己的端口来。',
  'settings.window.endpoints.none': '未发现可接入的 Harness。请先启动官方桌面客户端，或让本机的 DSH CLI 起来（dsh web）后重新扫描。',
  'settings.window.endpoints.unavailable': '发现 {count} 个 Harness，但当前都不可对话；详情见端点下拉。',
  'settings.window.endpoints.found': '发现 {count} 个可接入的 Harness。',
  'settings.window.endpoints.failed': '扫描接入端点失败：{error}',

  // 「打开界面」这一次动作的结果。`{label}` 填的是执行主体的类别词（也是词条）。
  'settings.window.reach.start-failed': '启动失败，请查看日志中的启动记录。',
  'settings.window.reach.no-port': '没有可打开的界面：主体没有在本机监听任何端口。',
  'settings.window.reach.browser-opened': '已在默认浏览器中打开 127.0.0.1:{port}。',
  'settings.window.reach.raised': '已把 {label} 的窗口拉到前台。',
  'settings.window.reach.restored': '{label} 的窗口已恢复；Windows 拒绝了前台切换，点一下它即可。',
  'settings.window.reach.failed': '打开客户端界面失败：{error}',

  // 停止本应用启动的 DSH。
  'settings.window.managed.stopped-all': '已停止本应用启动的全部 DSH。',
  'settings.window.managed.stopped-one': '已停止该 DSH 实例。',

  // 开机自启：Windows 那边的两种"不许"、读状态失败、以及这次改动没落地。
  'settings.window.autostart.disabled-by-user': 'Windows 已禁用 DSH Wallpaper 开机启动，请在系统设置中允许。',
  'settings.window.autostart.disabled-by-policy': 'Windows 策略禁止 DSH Wallpaper 开机启动。',
  'settings.window.autostart.read-failed': '读取开机自启状态失败：{error}',
  'settings.window.autostart.update-failed': '开机自启更新失败：{error}',

  // 桥对齐（`ensureProfileBridge`）。
  'settings.window.bridge.failed': '装桥失败：{error}',

  // 网页适配器配置：打开、恢复默认（含确认框）与各自的失败。
  'settings.window.adapter.opened': '已打开网页适配器配置；保存后下一次网页状态、历史或发送操作会读取新配置。',
  'settings.window.adapter.open-failed': '网页适配器配置打开失败：{error}',
  'settings.window.adapter.reset-confirm': '恢复默认网页适配器配置会覆盖当前本地 override 文件。确定继续吗？',
  'settings.window.adapter.reset-done': '网页适配器配置已恢复默认。',
  'settings.window.adapter.reset-failed': '网页适配器配置恢复失败：{error}',

  // 访问密钥与模型目录：读状态、地址不支持目录、缺 Key、保存/测试的结果、只刷新目录。
  'settings.window.api-key.read-failed': '读取访问密钥状态失败：{error}',
  'settings.window.api-key.models-unsupported': '该 API 地址不提供模型列表（HTTP 404/405）。',
  'settings.window.api-key.models-read-failed': '读取模型列表失败：{error}',
  'settings.window.api-key.missing': '请先填入 DeepSeek API Key。',
  'settings.window.api-key.saved': 'API Key 已保存到 Windows 凭据管理器，模型列表已更新。',
  'settings.window.api-key.usable': '已保存的 API Key 可用，模型列表已更新。',
  'settings.window.api-key.save-failed': 'API Key 保存失败：{error}',
  'settings.window.api-key.models-refreshed': '模型列表已刷新。',

  // 「聊天模式」这一栏的切换结果（`{label}` 是后端显示名，也是词条）。
  'settings.window.backend.switched': '已切换为{label}，正在运行的壁纸立即生效。',
  'settings.window.backend.failed': '切换后端失败：{error}',

  // API 会话记录：删除一条（含确认框）、清空全部（含确认框），以及它们的失败。
  'settings.window.history.delete-confirm': '删除 API 会话 {id} 的本地记录？此操作无法撤销。',
  'settings.window.history.deleted': '已删除 API 会话 {id} 的本地记录。',
  'settings.window.history.gone': '该会话已不存在，列表已刷新。',
  'settings.window.history.delete-failed': '删除 API 会话失败：{error}',
  'settings.window.history.clear-confirm': '清空全部 {count} 个 API 会话的本地记录？此操作无法撤销，但不会影响 DeepSeek 网页入口或 Harness 会话。',
  'settings.window.history.cleared': '已清空 {count} 个 API 会话的本地记录。',
  'settings.window.history.nothing-to-clear': '没有可清空的 API 会话记录。',
  'settings.window.history.clear-failed': '清空 API 历史失败：{error}',

  // 素材库的导入、分类、应用与恢复默认。
  'settings.window.appearance.import-failed': '导入失败：{error}',
  'settings.window.appearance.classify-failed': '素材分类失败：{error}',
  'settings.window.appearance.sync-failed': '外观同步失败：{error}',
  'settings.window.appearance.apply-failed': '应用素材失败：{error}',
  'settings.window.appearance.reset-failed': '恢复默认失败：{error}',

  // 「拉起 TUI」：没装 TUI 时**显示原生那句"怎么办"**，只有连原生都问不到时才用这里的兜底。
  'settings.window.tui.missing': '本机没有找到 TUI（dst）。',
  'settings.window.tui.launched': '已在新终端窗口中拉起 TUI。',
  'settings.window.tui.failed': '打开 TUI 失败：{error}',

  // DeepSeek 应用内页面。
  'settings.window.deepseek-web.open-failed': '无法打开 DeepSeek 应用内页面：{error}',

  // 「打开项目记忆」的两种结果。文件名是磁盘上那个文件的名字（原生建的），两种语言里都不译。
  'settings.window.memory.selected': '已在资源管理器中选中「项目记忆.md」。',
  'settings.window.memory.opened-folder': '还没有「项目记忆.md」：已打开桌面会话目录，你或助手第一次“记下来”时它会出现在这里。',

  // ---------------------------------------------------------------------------
  // 「打开」那张卡片给的路线的名字（`connect/openRoutes.ts`）。
  // 这三个名字由那个**纯函数**在每次调用时取，所以函数体里用 `t()`；**不能**提到模块级常量里
  // —— 那会在 import 时把语言定死，之后切语言就再也不变了。
  // ---------------------------------------------------------------------------
  'open.route.shell-window': '官方客户端窗口',
  'open.route.browser': '浏览器',
  'open.route.tui': '终端里的 TUI',

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
  // FREEZE（临时冻结，不是删除）：只去掉"花钱"的那半句（用户要求界面上不再出现"计价"），
  // 保留"这条通道是谁"的身份说明。删掉的原文逐字抄在下面，恢复时不必翻 git 历史：
  //   'chat.host.deepseek-api': '你自己的 DeepSeek API key，按 token 计费。'
  // 为什么关：这半句渲染成页脚那枚宿主徽章的 `title`（见 `connect/conversationHost.ts`），
  // 而它报的"计费"读数已经在上一批冻结了（`ConversationBubble.tsx` 的用量与费用两处 FREEZE）
  // —— 悬停里再提一次"按 token 计费"，说的是一个界面上已经看不到的东西。
  // 关掉之后：悬停只说明这条通道是谁，不再提钱。
  // 怎么恢复：把上面那句原文接回下面这行的句尾（中文用全角逗号，见那句原文的标点）。
  'chat.host.deepseek-api': '你自己的 DeepSeek API key。',
  // FREEZE（临时冻结，不是删除）：同上，去掉的是"不产生 API 费用"那半句。删掉的原文逐字：
  //   'chat.host.deepseek-web': 'DeepSeek 网页额度，不产生 API 费用。'
  // 关掉之后：悬停只说这条通道是谁（网页额度）。怎么恢复：把原文接回下面这行的句尾。
  'chat.host.deepseek-web': 'DeepSeek 网页额度。',
  'chat.host.embedded-shell': '本机 DeepSeek Harness 客户端，它带自己的窗口。',
  'chat.host.terminal': '{label}，界面在终端里。',
  'chat.host.browser': '{label}，界面在浏览器里。',

  // ---------------------------------------------------------------------------
  // 原生运行时**返回给界面**的兜底与错误句（`native/runtime.ts`）。它们是浏览器预览（没有 Tauri、
  // 没有 Windows）时的答案，用 `t()` 在**调用时**求值 —— 与 `connect/*` 里的句子同一条理由。
  //
  // 注意这些句子是**数据**：一旦被组进一个对象（`AutostartStatus.reason`、适配器状态的 `path`），
  // 它就定在那一刻的语言上，界面之后切语言不会把它重译。这正是计划里"结构化状态改由界面本地化"
  // 要逐批收拾的东西 —— 本批先让它们**说当前语言**，而不是写死中文。
  // ---------------------------------------------------------------------------
  'runtime.autostart.unsupported': '当前系统不支持本应用的开机自启。',
  // 四条"只有桌面版才有"的拒绝（浏览器预览里调用会抛错）。
  'runtime.credentials.desktop-only': '仅桌面版支持 Windows 凭据管理器',
  'runtime.workspace.desktop-only': '仅桌面版支持桌面会话工作区自检',
  'runtime.memory.desktop-only': '仅桌面版支持打开项目记忆',
  'runtime.tui.desktop-only': '仅桌面版支持打开 TUI',
  // 网页适配器配置的三条浏览器预览答案（`path` 那一格是给界面显示的一句话，不是一个路径）。
  'runtime.web-adapter.preview-config-path': '浏览器预览不支持本地网页适配器配置',
  'runtime.web-adapter.preview-open': '浏览器预览不支持打开网页适配器配置',
  'runtime.web-adapter.preview-reset': '浏览器预览不支持恢复网页适配器配置',
  // 浏览器预览没有真实显示器，这是那块占位屏幕的名字。
  'runtime.display.preview-name': '预览屏幕',

  // ---------------------------------------------------------------------------
  // 本批（计划第 4 步）：桌面侧的三处界面文案。
  //
  // 键按**位置**命名：`app.bubble.*` 是 `App.tsx`（壁纸外壳：立绘气泡、通知条、会话窗、登录浮层），
  // `chat.bubble.*` 是 `features/chat/ConversationBubble.tsx`（会话气泡本体），
  // `lite.settings.*` 是 `lite/LiteSettingsWindow.tsx`（Lite 设置窗口）。
  //
  // 三处都有**带前导空格**的词条：它们是拼在别的句子后面的半句（中文不加空格，英文要加），
  // 与 `chat.cost.approx` 同一条理由 —— 空格是句子的一部分，所以它也在词条里。
  // ---------------------------------------------------------------------------

  // 掉线提示的**共同前缀**。它不是一个完整句子：前缀 + 状态说明（`connect/harnessLabels.ts`
  // 的 `harness.detail.*`）+ 下面那半句拼成一句。
  'app.bubble.harness.disconnected-prefix': 'DSH 壁纸 Bridge 当前不可用。',
  // 上面那两句的续写（前导空格接在状态说明后面）。
  'app.bubble.harness.selection-unavailable': ' Harness 模式只能在兼容 Bridge 就绪后切换。',
  'app.bubble.harness.session-preserved': ' 已保留当前 Harness 会话和对话记录；Bridge 恢复后可继续，或由你手动切换后端。',
  // 把上面三段**排成一句**的框。整句会作为 `Message`（键 + 参数）存进运行状态，所以三个 `{...}`
  // 都是词条而不是句子：切语言时三段一起重译。这里没有空格 —— 该有空格的那两半自己带着（见上）。
  // 「这句是我们自己写的」不再靠文字认（原来靠前缀 `startsWith`），改成状态里的 `errorKind`。
  'app.bubble.harness.disconnected-frame': '{prefix}{detail}{preserved}',
  'app.bubble.harness.selection-frame': '{prefix}{detail}{unavailable}',

  // 随壁纸自动启动 DSH 的失败码，一条一句（`dshAutostartNotice`）。`dshAutostart.spec.ts`
  // 逐字钉着其中几个词，所以这些句子不能顺手改写。
  'app.bubble.autostart.root-path-missing': '已开启「随壁纸启动 DSH」，但尚未选择执行主体；请在设置中心扫描并选择一个。',
  'app.bubble.autostart.unknown-target': '已开启「随壁纸启动 DSH」，但所选执行主体不可用；请在设置中心重新扫描后选择。',
  'app.bubble.autostart.started-unconfirmed': '已请求启动所选客户端，但它在超时时间内没有应答；若界面始终没有出现，请确认该客户端仍已安装。',
  'app.bubble.autostart.root-path-invalid': '已开启「随壁纸启动 DSH」，但配置的根目录不是可识别的 DSH 项目；请在设置中心修正。',
  'app.bubble.autostart.launcher-missing': '已开启「随壁纸启动 DSH」，但未找到 Node.js 或 pnpm。请在设置中心确认这两个程序已安装，并能在命令提示符里直接运行。',
  'app.bubble.autostart.profile-invalid': '已开启「随壁纸启动 DSH」，但配置的 profile 名称无效（只能包含字母、数字、连字符或下划线）。请在设置中心修正。',
  'app.bubble.autostart.launch-args-invalid': '已开启「随壁纸启动 DSH」，但「启动参数」无效。请在设置中心修正后重试。',
  'app.bubble.autostart.spawn-failed': '已开启「随壁纸启动 DSH」，但进程启动失败。请在设置中心检查根目录与启动参数。',
  'app.bubble.autostart.failed': '自动启动 DSH 失败：{error}',

  // 启动监督（`harnessLaunchOutcome`）：宽限期后的"退出了"、超时后按最具体成因给的四句。
  'app.bubble.launch.exited-early': 'DSH 启动后很快退出；请检查 DSH 配置或启动日志。',
  'app.bubble.launch.token-unavailable': 'DSH 已启动，但 Bridge 本机令牌不可用；请重启壁纸应用或检查令牌目录权限。',
  'app.bubble.launch.incompatible': 'DSH 已启动，但 Bridge 版本或能力不兼容；请更新 Bridge 后重试。',
  'app.bubble.launch.bridge-loading': 'DSH 已启动，Bridge 仍在装载会话服务；若长期停留，请检查 DSH 日志。',
  'app.bubble.launch.timeout': 'DSH 启动超时；进程仍在运行但 Bridge 尚未上线。',
  'app.bubble.launch.subject-failed': 'DSH 主体启动失败，请查看日志中的启动记录。',

  // 模型目录"还没查"与"查不到"的原因（`connect/modelDirectory.ts` 的 `reason`）。
  'app.bubble.model.harness-reading': '正在读取 Harness 模型目录…',
  'app.bubble.model.endpoint-reading': '正在读取端点模型目录…',
  'app.bubble.model.web-decided': '网页入口的模型由 DeepSeek 页面决定',
  'app.bubble.model.harness-not-running': 'Harness 未运行',
  'app.bubble.model.harness-read-failed': 'Harness 模型目录读取失败：{error}',
  'app.bubble.model.endpoint-read-failed': '端点模型列表读取失败：{error}',

  // 通知条与浮层：失败都要说出来（一条一句，`{error}` 填异常原文）。
  'app.bubble.notice.raise-window-failed': '打开可视化窗口失败：{error}',
  'app.bubble.notice.display-read-failed': '读取显示器布局失败：{error}',
  'app.bubble.notice.display-subscribe-failed': '显示器事件订阅失败：{error}',
  'app.bubble.notice.system-subscribe-failed': '系统会话事件订阅失败：{error}',
  'app.bubble.notice.appearance-subscribe-failed': '外观变更订阅失败：{error}',
  'app.bubble.notice.leave-inner-failed': '离开里桌面失败：{error}',
  'app.bubble.notice.open-link-failed': '打开链接失败：{error}',
  // 宿主发来的"需要批准"摘要后面接的那半句（摘要本身是运行时的字，不在词条里）。
  'app.bubble.notice.approval': '{summary}；请打开 Harness 处理。',

  // 立绘头顶那颗气泡里的话。
  'app.bubble.question.prompt': '想听听你的意见：{question}',
  'app.bubble.question.options': '（{options}）',
  'app.bubble.thinking': '正在认真思考…',

  // 需要登录网页入口时的浮层。
  'app.bubble.auth.title': '需要登录 DeepSeek 网页入口',
  'app.bubble.auth.body': '应用内官方页面已经打开，请在其中完成登录。登录状态只保存在独立 WebView2 配置目录，本应用不会读取或复制 Cookie；登录完成后回到桌面即可继续发送。',
  'app.bubble.auth.confirm': '我已完成登录',

  // ---------------------------------------------------------------------------
  // 会话气泡本体（`features/chat/ConversationBubble.tsx`）。标签、提示、按钮的无障碍名，
  // 以及页脚那一行用量 —— 用法与中文界面上的字逐字一致。
  // ---------------------------------------------------------------------------
  'chat.bubble.surface.label': 'AI 对话',
  'chat.bubble.session-title': '桌面会话',
  'chat.bubble.raise-window': '打开可视化窗口',
  'chat.bubble.close': '收起对话',
  'chat.bubble.history.label': '当前会话记录',
  'chat.bubble.history.kept-note': '上次的 Harness 会话（后端已退出）。它重新上线后会自动回到这段记录；你也可以现在就在左侧继续对话。',
  'chat.bubble.history.load-earlier': '加载更早的 {hidden} 条记录',
  // 同一枚按钮的两个方向：展开时它说"收起记录"（含 `chevron-up`），收起时说"会话记录"。
  'chat.bubble.history.toggle-expanded': '收起记录',
  'chat.bubble.history.toggle-collapsed': '会话记录',
  // 状态点与滑槽：失败、启动中，以及滑槽两个方向上的标签/提示（`{...}` 之外的都是键）。
  'chat.bubble.harness.failed': '连接失败',
  'chat.bubble.harness.starting': 'DSH 正在启动',
  'chat.bubble.mode-switch.to-deepseek': '切换至 DeepSeek 模式',
  'chat.bubble.mode-switch.to-harness': '切换至 Harness 模式',
  'chat.bubble.mode-switch.start': '启动 DSH',
  'chat.bubble.mode-switch.configure': '配置 DSH',
  'chat.bubble.mode-switch.title-to-deepseek': '当前：Harness，点击切回 DeepSeek',
  'chat.bubble.mode-switch.title-to-harness': '当前：DeepSeek，点击切换 Harness',
  'chat.bubble.mode-switch.start-title': '启动已配置的 DSH 后端',
  'chat.bubble.mode-switch.configure-title': '先配置 DSH 根目录与 profile',
  // agent 预设下拉：控件名、没有预设时的兜底项、宿主说某个预设坏了时的后缀。
  'chat.bubble.preset.label': '选择 DSH 模式',
  'chat.bubble.preset.standard': '标准模式',
  'chat.bubble.preset.unavailable': '（不可用）',
  // 输入框与命令/权限菜单。
  'chat.bubble.composer.label': '输入消息',
  'chat.bubble.composer.send': '发送消息',
  'chat.bubble.composer.stop': '停止生成',
  'chat.bubble.command-menu': '命令',
  'chat.bubble.command-menu.label': '选择命令',
  'chat.bubble.permission.label': '选择权限',
  // 模型选择器（岛内自绘的下拉）。
  'chat.bubble.model.label': '选择模型',
  'chat.bubble.model.switch': '切换模型',
  'chat.bubble.model.unsupported': '当前后端不支持在壁纸中切换模型',
  'chat.bubble.model.unavailable': '模型不可切换',
  // 页脚那一行用量：每条消息下面那一小行（` · 缓存 N` 含前导空格），以及本轮/会话两格。
  'chat.bubble.usage.tokens': '输入 {input} · 输出 {output}',
  'chat.bubble.usage.cache': ' · 缓存 {cacheRead}',
  'chat.bubble.turn-usage.label': '本轮用量',
  'chat.bubble.turn-usage.label-unavailable': '本轮用量未提供',
  'chat.bubble.turn-usage.input': '本轮 入 {input}',
  'chat.bubble.turn-usage.output': '出 {output}',
  'chat.bubble.turn-usage.cache': '缓存 {cacheRead}',
  'chat.bubble.turn-usage.cost': '费用 {cost}',
  'chat.bubble.session-cost': '会话 {cost}',
  'chat.bubble.session-cost.unavailable': '会话费用未提供',

  // ---------------------------------------------------------------------------
  // 内置背景（`settings/store.ts` 的 `BACKGROUND_OPTIONS`）与内置形态（`persona/registry.ts`）。
  // 名字都是**画出来的东西**，所以按位置命名；`default` 是纯 CSS 的空白渐变。
  // ---------------------------------------------------------------------------
  'settings.appearance.background.workspace': '深夜工作室',
  'settings.appearance.background.deepsea-2': '深海穹顶舱',
  'settings.appearance.background.deepsea-3': '深海书房',
  'settings.appearance.background.default': '默认渐变主题',
  'persona.builtin.blue-child': '蓝色幼年鲸鱼娘',
  'persona.builtin.blue-adult': '蓝色成年鲸鱼娘',
  'persona.builtin.black-adult': '黑红成年鲸鱼娘',
  'persona.builtin.black-child': '黑红幼年鲸鱼娘',

  // 内置形态在 manifest 没给气泡文案时的兜底（`persona/types.ts`）。
  'persona.bubble.morning': '早上好！今天要做什么呢？',
  'persona.bubble.done': '搞定啦～还有别的吗？',
  'persona.bubble.harness-online': '检测到 DeepSeek Harness，切换形态？',
  'persona.bubble.harness-offline': 'Harness 已下线，切回网页模式。',
  'persona.bubble.chat-open': '想聊点什么呀？',

  // 四张官方形态卡（`persona/officialCatalog.ts`）与卡片上的两段标签（`OfficialPersonaCards.tsx`）。
  // `DeepSeek` / `DeepSeek Harness` 是产品名，两种语言里一样，所以不在这里。
  'persona.official.deepseek.flash.name': 'DeepSeek Flash · 蓝色幼年',
  'persona.official.deepseek.pro.name': 'DeepSeek Pro · 蓝色成年',
  'persona.official.harness.flash.name': 'Harness Flash · 黑红幼年',
  'persona.official.harness.pro.name': 'Harness Pro · 黑红成年',
  'persona.official.tier.flash': 'Flash · 幼年',
  'persona.official.tier.pro': 'Pro · 成年',
  'persona.official.list.label': '人物列表',
  'persona.official.replaced': '已替换：{name}',
  'persona.official.replaced-custom': '已替换：自定义素材',
  'persona.official.baseline': '基础立绘',

  // ---------------------------------------------------------------------------
  // 连接面：端点、启动参数、模型目录、装桥（`connect/*`）。
  // ---------------------------------------------------------------------------
  'connect.endpoint.kind.desktop': '桌面客户端',
  'connect.endpoint.kind.web': 'Web / CLI',
  // 存着一个本 build 已不支持的壳主体时说的话（两段拼成一句）。
  'connect.endpoint.unsupported-shell':
    '原先选定的桌面客户端已不再受支持：它把本地接口锁在自己的授权后面，壁纸请求一律被拒绝；'
    + '已切回官方桌面客户端。',
  // 扫描结果（`scanSummary`）。
  'connect.scan.none': '已扫描 {count} 个端口，未发现可接入的 Harness',
  'connect.scan.found': '已扫描 {count} 个端口，发现 {bridges} 个可接入的 Harness（其中 {ready} 个可用）',
  // 「打开界面」的两种落空说法；`{kind}` 是上面那两个字。
  'connect.raise.no-window': '{kind} 没有可拉起的窗口；它的界面可能在浏览器里，请改用「在浏览器中打开」。',
  'connect.raise.not-running': '{kind} 未在运行。请先启动它，然后重新扫描。',
  // 「启动参数」的校验与读端口（`launchArgs.ts`）。
  'connect.launch-args.too-many': '启动参数最多 {max} 个。',
  'connect.launch-args.too-long': '单个启动参数不能超过 {max} 个字符。',
  'connect.launch-args.control-chars': '启动参数里不能包含控制字符。',
  // 模型目录问不到时的原因（`modelDirectory.ts`）：禁用选择器时给用户的解释。
  'connect.model.no-options': '当前 Harness 未提供可选模型',
  'connect.model.bridge-empty': '桥接未返回模型目录',
  'connect.model.endpoint-empty': '端点未返回模型目录',
  // 装桥结果（`bridgeInstall.ts`）：失败、需要确认、装好了三种语气。
  'connect.bridge.failed': '装桥失败（{profile}）：{detail}',
  'connect.bridge.no-detail': '没有更多信息',
  'connect.bridge.needs-confirmation': '桥已装好，但 {profile} 里的插件需要你确认版本豁免：在终端执行 dsh plugin allow-version（原文：{detail}）',
  'connect.bridge.installed': '已为 {profiles} 装好桥。',

  // ---------------------------------------------------------------------------
  // 外观素材（`native/appearance.ts`、`features/appearance/*`）。
  // ---------------------------------------------------------------------------
  'appearance.native.desktop-only': '仅桌面版支持外观存储',
  'appearance.native.import-desktop-only': '仅桌面版支持导入外观素材',
  'appearance.native.classify-desktop-only': '仅桌面版支持素材分类',
  // 导入素材的文件对话框：标题与两组过滤器名。
  'appearance.import.title': '导入主题或独立素材',
  'appearance.import.filter.content': '外观内容',
  'appearance.import.filter.all': '所有文件',
  'appearance.import.folder-title': '导入素材文件夹',
  // 素材分类面板（`AssetClassificationPanel.tsx`）。
  'appearance.classify.label': '素材分类',
  'appearance.classify.title': '整理独立素材',
  'appearance.classify.description': '先选择素材，再指定一个或多个用途。',
  'appearance.classify.close': '关闭分类',
  'appearance.classify.slots': '可用位置',
  'appearance.classify.later': '稍后整理',
  'appearance.classify.confirm': '确认分类',

  // ---------------------------------------------------------------------------
  // 设置中心与桌面壳里剩下的那几句：探针失败、场景 alt、抽屉与提示条的无障碍名字。
  // ---------------------------------------------------------------------------
  // 探针失败的前缀（`settingsProbes.ts`），后面接 `：` 与异常原文。
  'settings.probe.error': '{message}：{error}',
  'settings.probe.managed-dsh': '读取受管 DSH 状态失败',
  'settings.probe.web-adapter-config': '网页适配器配置读取失败',
  'settings.probe.autostart-status': '读取开机自启状态失败',
  'settings.probe.desktop-displays': '显示器列表读取失败',
  'settings.probe.api-history': '读取 API 会话记录失败',
  'settings.probe.update': '更新检查没有完成',
  // 待机背景与立绘那一下点击（睡眠、苏醒那两个 Lite 也用，在 `zh.shared.ts`）。
  'scene.idle.background': '背景',
  'scene.idle.portrait-title': '点击开始对话',
  // 悬浮球（`floating/BallWindow.tsx`）上的可访问名字与悬停提示。
  'ball.open-island': '打开 AI 输入岛',
  // 抽屉的关闭按钮与提示条（`ui/primitives/*`）。
  'ui.drawer.close': '关闭{title}',
  'ui.toast.close': '关闭提示',
  'widgets.host.label': '桌面组件',
  // 组件清单的校验理由（`widgets/sdk.ts`）：`{apiVersion}` / `{hostVersion}` 是宿主报的版本号。
  'widgets.manifest.id-invalid': '组件 ID 只能使用小写字母、数字、点、短横线或下划线。',
  'widgets.manifest.api-version': '组件需要 API v{apiVersion}，当前宿主仅支持 v{hostVersion}。',
  'widgets.manifest.display-name-required': '组件需要显示名称。',
  'widgets.manifest.default-size': '组件默认尺寸必须大于零。',
  'widgets.manifest.min-size': '组件最小尺寸必须大于零。',
  'widgets.manifest.min-exceeds-default': '组件最小尺寸不能超过默认尺寸。',
  'widgets.manifest.max-below-default': '组件最大尺寸不能小于默认尺寸。',
  'widgets.manifest.workspaces': '组件必须声明可用工作区。',

  // ---------------------------------------------------------------------------
  // 聊天后端里剩下的句子：适配器错误、换会话的凭据、预览回复，以及调试量尺与链接提示。
  // ---------------------------------------------------------------------------
  'chat.native.archived-notice': '这条会话已被归档，已在今天的新会话里重新发送。',
  'chat.native.blocked-notice': 'DSH 拒绝了这一轮（这条会话已被归档）；已在今天的新会话里重新发送。',
  'chat.native.new-session-failed': '换一条新会话也没有成功：{error}',
  'chat.native.reconnect-failed': '{error} 重新连接这条端点也没有成功：{connectError}',
  'chat.native.archived-failed': '{error} 换一条新会话也没有成功：{failure}',
  'chat.native.closed': '连接已经关闭',
  'chat.native.no-new-session': '桥没有给出新的会话',
  'chat.web.unrecognised': 'DeepSeek 网页结构无法识别，网页桥接需要更新。',
  'chat.web.not-connected': 'DeepSeek 网页实验入口尚未连接',
  'chat.web.empty-message': '消息不能为空。',
  'chat.web.busy': 'DeepSeek 网页上一条消息仍在处理中，请等待完成或点击停止。',
  'chat.preview.harness-reply': 'Harness 已接通。正式桌面应用会把这条消息交给标准 DSH 会话。',
  'chat.preview.browser-reply': '这是浏览器预览回复。正式应用会连接 DeepSeek 网页桥接或用户启用的 API。',
  // 调试量尺：两个内容边缘之差（`LayoutProbe.tsx`，只在注释掉的那两行打开时才会出现）。
  'chat.layout-probe.delta': 'Δ {a}→{b}: 左 {left}  右 {right}',
  // 链接的 title：地址后面跟着手势说明（`MarkdownBody.tsx`）。
  'chat.markdown.link-hint': '{href}（中键或回车打开）',

  // ---------------------------------------------------------------------------
  // 更新检测（`features/update/*`）：立绘气泡与设置中心系统页**共用**的句子。
  //
  // 原生侧只回码与数字（`src-tauri/src/update/commands.rs`），所以这里每一句话都对应一个码：
  // `update.outcome.*` 是五种结论，`update.skip.*` / `update.failure.*` 是嵌进结论里的原因，
  // `update.notice.*` 是"点下去之后没成"的那些话。
  //
  // 更新检测只属于完整版（计划书 §七：Lite 本次不做），所以这些键在 `.full.ts` 这一半 ——
  // 放进 `.shared.ts` 会让 Lite 的产物多出一整节用不上的文案（`verify-lite-bundle.ps1` 拦的就是它）。
  // ---------------------------------------------------------------------------
  // 结论（设置卡片的「上次结果」、气泡的正文、手动检查后那句话，都用这一组）。
  'update.outcome.available': '有新版本 {version}',
  // 结论说"有更新"、报告里却没有版本号：这是原生不该出现的状态，但宁可说出来也不印一个空版本号。
  'update.outcome.available-unversioned': '有新版本，但版本号读不出来',
  'update.outcome.up-to-date': '已是最新',
  'update.outcome.no-asset': '有新版本 {version}，但这次发布没有可安装的安装包',
  // `{reason}` 填下面 `update.skip.*` / `update.failure.*` 里的某一句（整句随语言一起变）。
  'update.outcome.skipped': '本次没有检查：{reason}',
  'update.outcome.failed': '检查失败：{reason}',
  // 报告说"跳过了/失败了"却没带原因码（原生不该出现）：只说不带原因的那半句，
  // **不替它挑一个原因** —— 那会是一句看起来很像事实的假话。
  'update.outcome.skipped-unstated': '本次没有检查',
  'update.outcome.failed-unstated': '检查失败',
  // 没检查的原因。
  'update.skip.throttled': '距上次检查不足 6 小时',
  'update.skip.version-unavailable': '读不到本机版本',
  // 检查失败的原因（`{status}` 是 HTTP 状态码）。
  'update.failure.network': '网络不可用或请求超时',
  'update.failure.http-status': '服务器返回 {status}',
  'update.failure.http-status-unknown': '服务器返回了一个错误状态',
  'update.failure.malformed-response': '发布信息读不懂',
  // 两个动作：气泡上、以及设置卡片上的那两个按钮，说的是同一句话。
  'update.action.download': '下载',
  'update.action.dismiss': '忽略',
  // 这次发布没有可安装资产时，主按钮说的就是它真正做的事（§3.1 的回落）。
  'update.action.release-page': '打开发布页',
  // 第三片：下载完的主按钮、下载失败之后的那一枚（§四 的状态机）。
  'update.action.install': '点击安装',
  'update.action.retry': '重试',
  // 点下去之后没成：写不进状态文件、调用本身失败、发布页打不开。
  'update.notice.dismiss-unpersisted': '这条忽略记录没有写进状态文件；下次启动还会提示这个版本。',
  'update.notice.dismiss-failed': '忽略失败：{error}',
  'update.notice.check-failed': '更新检查没有完成：{error}',
  'update.notice.open-release-failed': '打开发布页失败：{error}',
  // 第三片（下载与安装）的那几句。
  'update.notice.listen-failed': '订阅下载进度失败：{error}（进度不会更新）',
  'update.notice.download-failed': '下载没有开始：{error}',
  'update.notice.update-unavailable': '这个环境里没有原生的下载与安装（浏览器预览）：请打开发布页获取安装包。',
  'update.notice.install-failed': '安装没有起来：{error}',
  // 助手已经就位：应用**正在退出**，安装器由那个助手在退出之后以 passive 模式启动
  // （`/P /UPDATE /R`：没有维护页、没有向导页、没有目录选择，只有一条进度，装完自动打开新版）。
  // 这一句要在退出之前就说出来 —— 应用一关，界面就没有下一次说话的机会了。
  'update.notice.install-exiting': '正在退出以便安装：应用关闭后自动装好（没有要点的页面，只有一条进度），装完会自动打开新版本。',
  // 助手起不来时的回落：安装包已经交给 Windows 了，但应用**不会退出** —— 那条路上安装器是
  // 普通窗口（应用还在跑，它会先问要不要关掉），所以这一句不能照着上一句说。
  'update.notice.install-fallback-opened': '没能安排"退出后再安装"，已直接打开安装包；这一次应用不会退出，安装器会先问要不要关掉它。',
  // `ready` 的正文：下载完、还没装（§四 的「点击安装」）。
  'update.outcome.ready': '新版本 {version} 已下载',
  // `downloading` 的正文（§四 的「正在下载 42%」）。总大小未知时说的是"已下载多少"：
  // 百分比要有个分母，编一个出来就是假数据。
  'update.progress.percent': '正在下载 {percent}%',
  'update.progress.downloaded': '正在下载（已下载 {downloaded}）',
  // `failed` 的正文：可读的原因（`{reason}` 填下面 `update.download.failure.*` 里的某一句）。
  'update.download.failed': '下载失败：{reason}',
  'update.download.failed-unstated': '下载失败',
  // 下载/校验失败的原因（§六：网络、磁盘、路径、校验各有各的说法）。
  'update.download.failure.destination-unavailable': '保存位置不可用（目录建不出来）',
  'update.download.failure.write-failed': '文件写不下去（可能是权限或磁盘问题）',
  'update.download.failure.disk-full': '磁盘空间不足',
  'update.download.failure.size-mismatch': '文件大小与发布信息不符（应为 {expected}，实际 {actual}）',
  'update.download.failure.digest-mismatch': '文件校验和不符（已删除这个文件）',
  'update.download.failure.verification': '文件没有通过校验',
  'update.download.failure.unknown': '下载没有完成',
  // 原生**拒绝一次调用**时的那些码（`UpdateCommandError`）：文案必须是人话，不能是 `[object Object]`。
  'update.call.forbidden': '这个窗口不能做这件事（只有壁纸与设置中心可以）',
  'update.call.state-path-unavailable': '找不到更新的数据目录',
  'update.call.invalid-version': '版本号不可比较，已拒绝',
  'update.call.destination-unavailable': '保存位置不可用（版本号或文件名不合格）',
  'update.call.untrusted-asset-url': '下载地址不是发布仓库（已拒绝）',
  'update.call.nothing-downloaded': '没有找到下载好的安装包（没下过，或者下载记录没写进状态文件）',
  'update.call.installer-missing': '下载好的安装包已经不在原来的位置了',
  'update.call.unsupported-asset': '这个文件不是可安装的安装包（只认 .exe 与 .msix）',
  'update.call.open-failed': 'Windows 没有打开这个安装包',
  'update.call.unknown': '调用没有成功：{error}',
}

/** 这一份的形状（`en.full.ts` 用它标注）。 */
export type FullOnlyDict = typeof zhFull
