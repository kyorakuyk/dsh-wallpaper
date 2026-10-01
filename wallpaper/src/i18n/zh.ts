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
  'settings.general.display.metrics': '{width} × {height} 像素 · 缩放 {scale}%',
  'settings.general.display.primary': ' · 主显示器',
  'settings.general.display.background': '{display} 背景',
  'settings.general.display.follow-global': '跟随全局背景',
  'settings.general.display.conversation.title': '对话窗所在屏幕',
  'settings.general.display.conversation.detail': '只移动会话层，不重新加载其他屏幕的背景。',
  'settings.general.display.portrait.title': '立绘所在屏幕',
  'settings.general.display.portrait.detail': '立绘和头顶气泡只挂载到选中的屏幕。',
  'settings.general.display.refresh': '刷新显示器检测',

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
  'settings.connections.launch-with-wallpaper.shell': '壁纸启动时自动把该客户端跑起来（它不支持静默启动时会直接出现窗口）。要让它随登录生效，还需要在「常规」里开启壁纸开机自启。已经在运行的实例不会被接管或重启。',
  'settings.connections.launch-with-wallpaper.checkout': '壁纸启动时自动把该源码目录跑起来。要让它随登录生效，还需要在「常规」里开启壁纸开机自启。已经在运行的实例不会被接管或停止。',
  'settings.connections.autostart-warning.title': '壁纸开机自启未生效',
  'settings.connections.autostart-warning.detail': '开机后自动启动 DSH 依赖壁纸自身的开机自启。',
  'settings.connections.autostart-warning.disabled-by-user': 'Windows 任务管理器已禁用本应用的自启项，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。',
  'settings.connections.autostart-warning.disabled-by-policy': '系统策略禁用了本应用的自启项，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。',
  'settings.connections.autostart-warning.not-configured': '壁纸自身尚未设置开机自启，因此「随壁纸启动 DSH」只会在你手动打开壁纸后生效。请在「常规」中开启壁纸自启。',
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
  // 的 `harness.detail.*`）+ 下面那半句拼成一句，而"这是不是我们自己写的那句提示"也靠它认。
  // 因此它在 `App.tsx` 里由 `harnessDisconnectedErrorPrefix()` 在**调用时**取，不做模块级常量。
  'app.bubble.harness.disconnected-prefix': 'DSH 壁纸 Bridge 当前不可用。',
  // 上面那两句的续写（前导空格接在状态说明后面）。
  'app.bubble.harness.selection-unavailable': ' Harness 模式只能在兼容 Bridge 就绪后切换。',
  'app.bubble.harness.session-preserved': ' 已保留当前 Harness 会话和对话记录；Bridge 恢复后可继续，或由你手动切换后端。',

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
  // Lite 设置窗口（`lite/LiteSettingsWindow.tsx`）：品牌下一行、三张卡片、选项格子与提示条。
  // `Wallpaper Lite` / `DSH` / `Windows 11` 是产品名，两种语言里一样，所以不在这里。
  // FREEZE 冻结的那几张卡（锁屏、登录过渡底图、TranslucentTB）整块在注释里，不占词条。
  // ---------------------------------------------------------------------------
  'lite.settings.brand.tagline': '轻量桌面壁纸',
  'lite.settings.window.close': '关闭设置',
  'lite.settings.notice.close': '关闭通知',
  'lite.settings.hero.title': '让桌面安静地醒来',
  'lite.settings.hero.description': '只保留锁屏、苏醒动画、壁纸与立绘。Windows 密码页仍由系统负责。',
  'lite.settings.system.title': '锁屏与启动',
  'lite.settings.autostart.title': '登录后自动启动',
  'lite.settings.autostart.busy': '正在更新启动任务。',
  'lite.settings.wake.title': '苏醒动画',
  'lite.settings.wake.enabled.title': '启用苏醒动画',
  'lite.settings.wake.enabled.detail': '解锁后播放正式四帧素材。',
  'lite.settings.wake.every-unlock.title': '每次解锁播放',
  'lite.settings.wake.every-unlock.detail': '关闭后只在应用启动时播放一次。',
  'lite.settings.wake.skip.title': '跳过动画',
  'lite.settings.wake.skip.detail': '直接进入静态壁纸与立绘。',
  'lite.settings.wake.speed.title': '动画速度',
  'lite.settings.scene.title': '壁纸与立绘',
  'lite.settings.scene.background': '壁纸背景',
  'lite.settings.scene.portrait': '右侧立绘',
  'lite.settings.scene.custom-background': '自定义背景 · 选择文件',
  'lite.settings.scene.custom-portrait': '自定义立绘 · 选择文件',
  'lite.settings.scene.current': '当前',
  'lite.settings.scene.footnote': '首发版默认使用正式内置素材，也可分别导入一张背景和一张立绘；主题包、插件和逐项替换会在完整版中提供。',
  'lite.settings.footer.autosave': '设置会自动保存',
  // 提示条里那几句（失败都带上 `{error}`；导入成功按槽位分两句）。
  'lite.settings.save-failed': '设置保存失败：{error}',
  'lite.settings.load-failed': '读取本地设置失败：{error}',
  'lite.settings.autostart.read-failed': '读取开机自启状态失败：{error}',
  'lite.settings.autostart.update-failed': '开机自启更新失败：{error}',
  'lite.settings.custom-image.read-failed': '读取自定义素材失败：{error}',
  'lite.settings.custom-image.import-failed': '导入图片失败：{error}',
  'lite.settings.custom-image.background-imported': '已导入自定义壁纸背景。',
  'lite.settings.custom-image.portrait-imported': '已导入自定义立绘。',
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
