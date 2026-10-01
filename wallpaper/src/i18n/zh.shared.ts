/**
 * 中文词条 —— **两版都要的那一半**（真源）。
 *
 * 为什么词条分家：整本词典曾经只由一个模块持有，而 Lite 入口也 import 它，于是完整版的每条键名与
 * 文案都进了 `dist-lite` 的 bundle（`scripts/verify-lite-bundle.ps1` 就是拦这件事的）。所以词条按
 * **面**拆开：这里放完整版与 Lite 都要的句子，`zh.full.ts` 放只有完整版会说的句子，`zh.ts` 把两份
 * 合起来当全量真源。
 *
 * 一条键**该放哪一半**不是凭感觉：Lite 入口可达的模块里用到的键必须在这一份里，由
 * `tests/liteI18nBoundary.spec.ts` 钉住（它走一遍 import 图，扫出来的键都要能在这里找到）。
 *
 * 规则（与拆分前一致）：
 * 1. 这里的中文是**从界面上原样搬过来的**，不是重写的 —— 所以"中文界面没有变化"是可验证的；
 * 2. 键按界面位置命名（`settings.general.title`、`language.label`），不按句意命名；
 * 3. 新增一个键，`en.shared.ts` 不同步就会**编译不过**（`SharedDict` 由这里推导）；
 * 4. 需要插值的地方用 `{name}`，由 `t(key, { name })` 填。
 */
export const zhShared = {
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

  // ---------------------------------------------------------------------------
  // Lite 的壁纸与立绘候选（`lite/settings.ts`、`lite/persona.ts`）：选项格子上的字。
  // 这几个 id 是内部标识，只有 label 是词条；`custom` 那一格由导入的素材决定，没有名字。
  // ---------------------------------------------------------------------------
  'lite.option.background.workspace': '深夜工作室',
  'lite.option.background.deepsea-2': '深海穹顶舱',
  'lite.option.background.deepsea-3': '深海书房',
  'lite.option.portrait.blue-adult': '蓝色成年形态',
  'lite.option.portrait.blue-child': '蓝色幼年形态',
  'lite.option.portrait.black-adult': '黑红成年形态',
  'lite.option.portrait.black-child': '黑红幼年形态',
  // Lite 选图的文件对话框（`lite/native.ts`）。
  'lite.native.pick.background': '选择壁纸背景',
  'lite.native.pick.portrait': '选择立绘',
  'lite.native.image-filter': '图片',
  // 场景图里 Lite 也会用到的两个无障碍名字（睡眠与苏醒；待机背景与立绘那一下点击只有完整版
  // 渲染，所以那两条在 `zh.full.ts`）。
  'scene.sleep.alt': '睡着的鲸鱼娘',
  'scene.sleep.hint': '按 Esc 或输入密码唤醒',
  'scene.wake.frame': '苏醒 {index}',
  'scene.wake.alt': '苏醒的鲸鱼娘',
  'lite.scene.background.alt': '壁纸背景',
}

/** 这一份的形状（`en.shared.ts` 用它标注）。 */
export type SharedDict = typeof zhShared
