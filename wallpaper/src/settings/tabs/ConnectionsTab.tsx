import { openRoutesFor, selectedOpenRoute } from '../../connect/openRoutes.ts'
import { catalogAgeLabel, displaySubjectPath, sameSubject, subjectOptionLabel } from '../../connect/harnessSubjects.ts'
import type { BackendMode } from '../../domain/types.ts'
import { formatMessage, formatSentence, msg, t, type Message } from '../../i18n/index.ts'
import type { HarnessEndpointScan } from '../../native/runtime.ts'
import { autostartKnown } from '../autostartCopy.ts'
import { Card, Choice, Field, Toggle } from '../components/SettingsControls.tsx'
import type { SettingsPanelProps } from '../SettingsPanel.tsx'
import { MAX_PRICE_PER_MILLION, normalizedPrice, type WallpaperSettings } from '../store.ts'

/** Local copy of the kind label so the panel does not import the connect layer. */
export function harnessEndpointKindLabel(kind: HarnessEndpointScan['kind']): string {
  switch (kind) {
    case 'official-desktop': return t('harness.subject.kind.embedded-shell')
    default: return 'Web / CLI'
  }
}

/**
 * 后端 → 显示名。三种都在：提示语与"此刻在用哪一格"仍然会遇到 Harness（托盘、自动切换、
 * 主体退出后的复位都可能把壁纸切过去），缺一个就会退化成内部 id。
 *
 * 三个名字都是 getter：语言是启动后才回填的，写成普通字段就会永远停在 import 时那一种。
 */
export const BACKEND_MODE_LABELS: Record<BackendMode, string> = {
  get 'deepseek-web'() { return backendModeLabel('deepseek-web') },
  get 'deepseek-api'() { return backendModeLabel('deepseek-api') },
  // Harness 那一格是**产品名**：两种语言里都一样，所以它不是词条（见 `backendModeMessage`）。
  get harness() { return 'DeepSeek Harness' },
}

/**
 * 「聊天模式」下拉里的候选：**只有两种聊天后端**。
 *
 * Harness 刻意不在这里（用户要求从这个下拉里删掉它）：它不是"换一个聊天后端"，而是
 * "换一个主体来对话"——连不连、什么时候连，由桌面上的那个开关以及「DSH 就绪时自动切换」管，
 * 在这里再给一个入口只会让两条路互相打架。
 */
export const CHAT_MODE_OPTIONS: Array<{ value: BackendMode; label: string }> = [
  // `label` 也是 getter：这张表在 import 时建好，普通字段会把语言钉死在那一刻（实测：切到
  // 英文后这个下拉里仍是中文），而 `BACKEND_MODE_LABELS` 那三个名字本来就是 getter。
  { value: 'deepseek-web', get label() { return BACKEND_MODE_LABELS['deepseek-web'] } },
  { value: 'deepseek-api', get label() { return BACKEND_MODE_LABELS['deepseek-api'] } },
]

/**
 * 候选 + **当前值**。
 *
 * 当前值可能是 Harness：那时它必须留在候选里并标出来源，否则 `Choice` 会退回第一个候选，
 * 控件显示成"网页入口"而壁纸其实在 Harness 上——一个会骗人的开关。
 */
export function chatModeOptions(): Array<{ value: BackendMode; label: string }> {
  // 这个下拉只回答一件事：**滑槽在左边时，聊天连接哪个通道**。滑槽在右端时一定是 Harness，
  // 而那是滑槽自己的含义 —— 与这里无关，所以候选**恒为两项**，不为运行中的后端追加任何项。
  return [...CHAT_MODE_OPTIONS]
}

/**
 * 同一句的**词条**形态：当别的句子的参数、或要存进状态时用它 —— 渲染期才求值，切语言跟着变。
 *
 * Harness 那一格返回的是**字符串**而不是词条：`DeepSeek Harness` 是产品名，两种语言里一样，
 * 把它塞进字典只会多一条永远相同的词条。`MessageParam` 本来就收字符串，所以它照样能当参数。
 */
export function backendModeMessage(backend: BackendMode): Message | string {
  if (backend === 'deepseek-web') return msg('settings.connections.backend.deepseek-web')
  if (backend === 'deepseek-api') return msg('settings.connections.backend.deepseek-api')
  // 与改动前一样：认不出来的值原样透出，而不是编一个名字。
  return backend === 'harness' ? 'DeepSeek Harness' : backend
}

export function backendModeLabel(backend: BackendMode): string {
  const message = backendModeMessage(backend)
  return typeof message === 'string' ? message : formatMessage(message)
}

/**
 * 模型列表那句说明里的"什么时候拉的"。
 *
 * 列表是**持久化缓存**（用户实测要求"刷新结果要持久化"），所以它可能是几天前拉的；不把时间写出来，
 * 用户会以为打开设置时刚拉过。解析不了就什么都不说，而不是编一个时间。
 */
export function catalogAgeSuffix(fetchedAt: string | undefined, now = Date.now()): string {
  if (!fetchedAt) return ''
  const at = Date.parse(fetchedAt)
  if (!Number.isFinite(at)) return ''
  const minutes = Math.max(0, Math.round((now - at) / 60_000))
  if (minutes < 1) return t('settings.connections.api.model.age.just-now')
  if (minutes < 60) return t('settings.connections.api.model.age.minutes', { minutes })
  const hours = Math.round(minutes / 60)
  if (hours < 48) return t('settings.connections.api.model.age.hours', { hours })
  return t('settings.connections.api.model.age.days', { days: Math.round(hours / 24) })
}

/**
 * 「模型」那一栏的候选：拉取到的目录 + **当前值**。
 *
 * 三件事都在这里说清：
 * 1. 显示名用「id · 官方名」（`deepseek-flash · DeepSeek-V4.1-Flash`），因为 API 认的是 id；
 * 2. **当前值必须留在候选里**，哪怕它已经不在目录中——否则控件会退回到"第一个候选"，看起来
 *    像设置被别人改掉了。官方改名时这条尤其重要：用户机器上多半存着一个旧名字；
 * 3. 旧名字要**标出来**（"不在当前目录里"），而不是静默地混在新名字中间。
 */
export function apiModelOptions(
  catalog: Array<{ id: string; name: string }>,
  current: string,
): Array<{ value: string; label: string }> {
  const options = catalog.map((model) => ({
    value: model.id,
    label: model.name && model.name !== model.id ? `${model.id} · ${model.name}` : model.id,
  }))
  const trimmed = current.trim()
  if (trimmed && !options.some((option) => option.value === trimmed)) {
    options.push({ value: trimmed, label: t('settings.connections.api.model.stale', { model: trimmed }) })
  }
  return options
}

export function PriceInput({
  label,
  value,
  onChange,
}: {
  label: string
  value: number | undefined
  onChange: (value: number | undefined) => void
}) {
  return <input
    aria-label={label}
    className="price-input"
    type="number"
    min="0"
    max={MAX_PRICE_PER_MILLION}
    step="0.0001"
    inputMode="decimal"
    placeholder={t('settings.connections.api.price.placeholder')}
    value={value ?? ''}
    onChange={(event) => {
      const raw = event.target.value.trim()
      const parsed = Number(raw)
      onChange(raw === '' ? undefined : normalizedPrice(parsed))
    }}
  />
}

interface ConnectionsTabProps extends SettingsPanelProps {
  set: (patch: Partial<WallpaperSettings>) => void
}

export function ConnectionsTab(props: ConnectionsTabProps) {
  const { settings, set } = props
/**
   * The chosen subject, and whether it is the class that carries its own checkout.
   *
   * An unknown or not-yet-chosen id is treated as a checkout: that is what the
   * root-path/profile/launcher fields describe, and it is also the class a
   * profile written by an older version falls back to.
   */
  const selectedSubject = props.harnessTargets.find((target) => target.id === settings.dshLaunch.subjectId)
  const shellSelected = selectedSubject?.kind === 'embedded-shell'
  // 已安装的 CLI 没有"源码目录"这回事：它拿的是别人装好的东西，路径只有它的启动器有意义，
  // 而启动器由扫描决定、不由用户填写。所以给这一类别单独收起那一行，而不是让它显示一个空框。
  const cliSelected = selectedSubject?.kind === 'installed-cli'
  // 「打开」能做什么，由主体决定：官壳只有自己的窗口，源码树只有浏览器，只有"已安装的 CLI"真的有
  // 两条路可选。**单项不做成下拉** —— 那是一个点了没反应、也无法改变的控件。
  // 这条规则住在 connect/openRoutes.ts：它有单测，包括「没装 TUI 就不给选项」与「卸掉 TUI 之后不回落到死选项」。
  const openRoutes = openRoutesFor({
    hasSubject: Boolean(settings.dshLaunch.subjectId),
    shellSelected,
    subjectKind: selectedSubject?.kind,
    tuiAvailable: props.tuiAvailable,
  })
  // 存着的路线只有在**真的有两条路**时才作数：官壳/源码树即便档案里写着 tui，也仍然走它们唯一的路。
  const openRoute = selectedOpenRoute(openRoutes, settings.dshLaunch.window)
  /**
   * Clients that are actually running. The endpoint picker is only a *choice* when
   * there is more than one: with zero or one, ports and Bridge states are internal
   * bookkeeping the user cannot act on, so showing them would be disclosure without
   * a decision attached to it.
   */
  return <>
        <Card title={t('settings.connections.chat-mode.title')} description={t('settings.connections.chat-mode.description')}>
          <Field title={t('settings.connections.chat-mode.field-title')} detail={t('settings.connections.chat-mode.field-detail')}>
            {/* 值是**启动默认值**：这个控件给"滑槽在左边"这个位置赋予含义。滑槽在右端时一定是
                Harness，那由滑槽自己表达，这里不描述、也不需要一个不可选但可见的 Harness 项。 */}
            <Choice emptyLabel={t('settings.choice.empty')} label={t('settings.connections.chat-mode.field-title')} value={settings.defaultBackend} onChange={(value) => props.onSelectBackend(value as BackendMode)} options={chatModeOptions()} />
          </Field>
          <Field title={t('settings.connections.chat-mode.auto-switch.title')} detail={t('settings.connections.chat-mode.auto-switch.detail')}><Toggle label={t('settings.connections.chat-mode.auto-switch.toggle')} checked={settings.autoSwitchHarness} onChange={(value) => set({ autoSwitchHarness: value })} /></Field>
        </Card>
                {/*
          Wording rules for this card, applied to every sentence in it:
          * name what the user is choosing between, never how it is built — "客户端 /
            源码目录", not the design's "自带检出 / 执行主体", which stay internal;
          * every control says what it will *do*, including that it may start
            something. The text below used to promise "不会替你启动它", which stopped
            being true when 「拉起 UI」 gained the right to start a subject: a promise the
            code no longer keeps must not survive in a sentence;
          * no port, no Bridge state, no filesystem path, and no step the user cannot
            take themselves;
          * one name per thing. The autostart toggle used to call the same setting both
            "壁纸自身开机自启" and "你的系统自启设置"; the page has one name for it, so
            the toggle points at that page instead of introducing a second.
        */}
        <Card
          title={t('settings.connections.harness.title')}
          description={t('settings.connections.harness.description')}
        >
          {/* 旧的卡片描述留档（不再显示）：选择由谁来接管复杂工作：客户端自带运行环境，源码目录由本应用启动。
              已经在运行的实例不会被接管或关闭。 */}
          {/*
            One control for "who runs it", because that is one question. The scanned
            subjects used to be a row each with its own 采用 button, plus a separate row
            prompting for a default when several source trees exist; a select answers all
            of it in one line, and that prompt becomes this line's description. There is
            deliberately no manual path entry: fewer items was the request, and typing a
            directory is discovery work the scan already does.
          */}
          <Field
            title={t('settings.connections.subject.title')}
            detail={props.dshScanBusy
              ? t('settings.connections.subject.scanning')
              : props.harnessTargets.length === 0
                ? t('settings.connections.subject.none')
                : [formatSentence(props.subjectChoice), t('settings.connections.subject.found', { count: props.harnessTargets.length, age: props.subjectCatalogVerifiedAt ? `（${catalogAgeLabel(props.subjectCatalogVerifiedAt)}）` : '' })].filter(Boolean).join(' ')}
          >
            <span className="integration-actions">
              {props.harnessTargets.length > 0 && (
                <Choice
                  emptyLabel={t('settings.choice.empty')}
                  label={t('settings.connections.subject.title')}
                  value={settings.dshLaunch.subjectId ?? ''}
                  onChange={props.onSelectSubject}
                  options={[
                    /*
                      A subject stored before this list was rescanned stays visible:
                      dropping the user's choice because a scan has not run yet is the
                      "looks empty" failure the catalogue exists to prevent.
                    */
                    ...(settings.dshLaunch.subjectId && !props.harnessTargets.some((target) => sameSubject(target.id, settings.dshLaunch.subjectId))
                      ? [{ value: settings.dshLaunch.subjectId, label: t('settings.connections.subject.current', { path: displaySubjectPath(settings.dshLaunch.subjectId) }) }]
                      : []),
                    ...props.harnessTargets.map((target) => ({ value: target.id, label: subjectOptionLabel(target, props.harnessTargets) })),
                  ]}
                />
              )}
              <button className="settings-action secondary" disabled={props.dshScanBusy} onClick={props.onScanDsh}>
                {props.dshScanBusy ? t('settings.connections.subject.scanning-button') : props.harnessTargets.length > 0 ? t('settings.connections.subject.rescan') : t('settings.connections.subject.scan')}
              </button>
            </span>
          </Field>
          {!shellSelected && <>
            {/*
              这几项属于**源码目录**，所以选中客户端时整块消失；选中已安装的 CLI 时只有"源码目录"
              一行消失（其余几项对它仍然有意义：档案名、启动参数）。目录本身是只读的：没有手工
              填写的地方，列表由扫描填，可编辑的字段会是一个改了也没用的输入框。
            */}
            {!cliSelected && <Field title={t('settings.connections.root-path.title')} detail={t('settings.connections.root-path.detail')}><span className="settings-static">{displaySubjectPath(selectedSubject?.identity.rootPath ?? settings.dshLaunch.rootPath)}</span></Field>}
            {/* 由用户决定隐藏（2026-09-30）：它不是偏好，而是我们走哪条启动链的结果 —— 官壳用自己独占的 desktop、TUI 用它自己的 dsh-tui、CLI 与检出只认能提供 HTTP 的 web。理由见 connect/harnessProfiles.ts。 */}
          </>}
          {/*
            One action, named for what the user wants (see it), not for the two things it
            does (start it if needed, then show it). 「启动执行主体」 used to sit beside this
            as a peer row: two similar options for one intent, and the ambiguity was in
            the pair rather than in either label.
          */}
          <Field
            title={t('settings.connections.open.title')}
            detail={!selectedSubject
              ? t('settings.connections.open.detail.no-subject')
              : shellSelected
                ? t('settings.connections.open.detail.shell')
                : openRoutes.length > 1
                  ? t('settings.connections.open.detail.multi')
                  : t('settings.connections.open.detail.single')}
          >
            {/* 两条路才给下拉；只有一条路就写一行字，而不是做一个改不动、点了也没反应的控件。 */}
            {openRoutes.length > 1
              ? (
                  <Choice
                    emptyLabel={t('settings.choice.empty')}
                    label={t('settings.connections.open.window-label')}
                    value={openRoute}
                    onChange={(value) => props.onSelectWindow(value === 'tui' ? 'tui' : 'browser')}
                    options={openRoutes}
                  />
                )
              : openRoutes.length === 1
                ? <span className="settings-static">{openRoutes[0]!.label}</span>
                : null}
            <button className="settings-action" disabled={!settings.dshLaunch.subjectId || props.openBusy} onClick={openRoute === 'tui' ? props.onOpenTui : props.onOpenClient}>
              {props.openBusy ? t('settings.connections.open.busy') : t('settings.connections.open.action')}
            </button>
          </Field>
          <Field
            title={t('settings.connections.launch-with-wallpaper.title')}
            detail={shellSelected
              ? t('settings.connections.launch-with-wallpaper.shell')
              : t('settings.connections.launch-with-wallpaper.checkout')}
          >
            <Toggle
              label={t('settings.connections.launch-with-wallpaper.title')}
              checked={settings.dshLaunch.autoStartWithWallpaper}
              onChange={(value) => set({ dshLaunch: { ...settings.dshLaunch, autoStartWithWallpaper: value } })}
            />
          </Field>
          {/* 未读到不等于没开：状态由「系统」页的 probe 读取，占位值的 enabled 也是 false，
              所以这条警告只在确实读到是关的时候出现，否则它会为开着的自启报错。 */}
          {/* 这条警告**没有控件**：那句话是文案，不是控件 —— 走 `note` 落在文案列里。
              以前它是 `children`（见 `Field` 的注释），于是落进右边不收缩也不折行的控件列，
              把左边的标题与说明挤成一词一行，自己还横穿卡片被裁掉。 */}
          {settings.dshLaunch.autoStartWithWallpaper && autostartKnown(props.autostart) && !props.autostart.enabled && (
            <Field
              title={t('settings.connections.autostart-warning.title')}
              detail={t('settings.connections.autostart-warning.detail')}
              note={props.autostart.source === 'disabled-by-user'
                ? t('settings.connections.autostart-warning.disabled-by-user')
                : props.autostart.source === 'disabled-by-policy'
                  ? t('settings.connections.autostart-warning.disabled-by-policy')
                  : t('settings.connections.autostart-warning.not-configured')}
            />
          )}
          {/* 官壳不显示这一项（用户要求）：它的退出方式是托盘菜单，用户手里本来就有；
              而停它会当场关掉用户自己的客户端、并弹一条"宿主意外退出"的报错框 —— 与其预告这个
              后果，不如不给这个入口。另外两类（源码目录、已安装的 CLI）没有托盘也没有窗口，
              要停只能开终端敲命令，摩擦大得多，所以它们照常显示。
              这一行是**恢复**回来的：`a8e2e91` 把它换成了标题右上角的下拉 +「全部停止」，而那个
              下拉（以及并行实例本身）在这个 build 里冻住了 —— 不恢复它，就再没有任何地方能停掉
              本应用启动的实例。动作与从前逐字相同：`stop_managed_dsh` 不给 instanceKey 就是
              "停全部"，也就是 `App.tsx` / `SettingsWindow.tsx` 里 `onStopAllManagedDsh` 那一个。
              用 `managedDshBusy` 挡住重复点击（停止要起 taskkill 并等它结束）。 */}
          {!shellSelected && <Field title={t('settings.connections.managed.title')} detail={props.managedDsh.managed
            ? t('settings.connections.managed.yes')
            : t('settings.connections.managed.no')}><span className="integration-actions"><button className="settings-action secondary" disabled={props.managedDshBusy} onClick={props.onRefreshManagedDsh}>{t('settings.refresh')}</button>{/* 启用条件跟**是不是本应用启动的**走，不跟"有没有在跑"走：只要 3080 上有别的东西在跑，
              旧写法就会点亮一个点了没反应的按钮（实测：装机重启后壁纸丢了"这是我的孩子"的记录）。 */}
            <button className="settings-action secondary" disabled={!props.managedDsh.managed || props.managedDshBusy} onClick={props.onStopAllManagedDsh}>{t('settings.connections.managed.stop')}</button></span></Field>}
        </Card>
        <Card title={t('settings.connections.web.title')} description={t('settings.connections.web.description')}><Field title={t('settings.connections.web.page.title')} detail={t('settings.connections.web.page.detail')}><button className="settings-action" onClick={props.onRequestDeepSeekLogin}>{t('settings.connections.web.page.open')}</button></Field>{/* 适配器那行的说明里**不再显示本地 override 的文件路径**（用户要求）：那是一串
            `%APPDATA%\com.dsh.wallpaper\deepseek-web-adapter.override.json`，对"网页结构变了才需要动它"
            这件事没有任何帮助，只会把一行说明压成三行。路径仍然在原生侧读得到
            （`deepseekWebAdapterConfig.path`），「打开配置」按钮就是照着它打开文件的 —— 需要它的人
            按那个按钮，不需要它的人不必看见。 */}
        <Field title={t('settings.connections.web.adapter.title')} detail={props.deepseekWebAdapterConfig ? `${t('settings.connections.web.adapter.detail')}${props.deepseekWebAdapterConfig.source === 'local' ? t('settings.connections.web.adapter.source-local') : t('settings.connections.web.adapter.source-bundled')} · ${props.deepseekWebAdapterConfig.adapterVersion}${props.deepseekWebAdapterConfig.warning ? ` · ${props.deepseekWebAdapterConfig.warning}` : ''}` : t('settings.connections.web.adapter.reading')}><span className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshDeepSeekWebAdapterConfig}>{t('settings.refresh')}</button><button className="settings-action secondary" onClick={props.onOpenDeepSeekWebAdapterConfig}>{t('settings.connections.web.adapter.open')}</button><button className="settings-action secondary" onClick={props.onResetDeepSeekWebAdapterConfig}>{t('settings.connections.web.adapter.reset')}</button></span></Field></Card>
        <Card title={t('settings.connections.api.title')} description={t('settings.connections.api.description')}>
          {/* 只深耕 DeepSeek：地址不再暴露成设置项（值仍是默认的官方地址），少一个能填错的地方。
              用户的原话是"API 网址可以省略"。 */}
          <Field
            title={t('settings.connections.api.key.title')}
            detail={t('settings.connections.api.key.detail')}
          >
            <span className="api-key-actions">
              <input
                type="password"
                aria-label={t('settings.connections.api.key.aria')}
                placeholder={props.apiKeyStatus?.present ? t('settings.connections.api.key.replace') : t('settings.connections.api.key.placeholder')}
                value={props.apiKeyDraft}
                onChange={(event) => props.onApiKeyDraftChange(event.target.value)}
                onKeyDown={(event) => { if (event.key === 'Enter') props.onTestApiKey() }}
                autoComplete="off"
                spellCheck={false}
              />
              <button className="settings-action secondary" disabled={props.apiKeyBusy} onClick={props.onTestApiKey}>{t('settings.connections.api.key.test')}</button>
            </span>
          </Field>
          <Field title={t('settings.connections.api.saved.title')} detail={props.apiKeyStatus?.present ? t('settings.connections.api.saved.present') : t('settings.connections.api.saved.absent')}>
            <span className="api-key-actions">
              <code className="api-key-masked" aria-label={t('settings.connections.api.saved.aria')}>{props.apiKeyStatus?.present ? props.apiKeyStatus.masked ?? '••••' : t('settings.connections.api.saved.unconfigured')}</code>
              <button className="settings-action secondary" disabled={props.apiKeyBusy} onClick={props.onRefreshApiModels}>{t('settings.refresh')}</button>
            </span>
          </Field>
          <Field title={t('settings.connections.api.model.title')} detail={props.apiModelCatalog && props.apiModelCatalog.length > 0 ? t('settings.connections.api.model.available', { count: props.apiModelCatalog.length, age: catalogAgeSuffix(props.apiModelCatalogFetchedAt) }) : t('settings.connections.api.model.hint')}>
            {/* 用面板自己的 `Choice`，**不用** `input list` + `datalist` 那套原生下拉：用户实测
                它在设置窗里根本展不开（只有箭头在那儿摆着，点不动）。`Choice` 是本窗口里已经在用
                的下拉（显示器背景、素材用途），展开由自己控制。
                （这里刻意不写出标签原文，测试用"源码里不许出现该标签"来钉住这条。） */}
            <Choice
              emptyLabel={t('settings.choice.empty')}
              label={t('settings.connections.api.model.label')}
              value={settings.deepseekApi.model}
              onChange={(model) => set({ deepseekApi: { ...settings.deepseekApi, model } })}
              options={apiModelOptions(props.apiModelCatalog ?? [], settings.deepseekApi.model)}
              emptyMessage={props.apiModelCatalog && props.apiModelCatalog.length > 0 ? undefined : t('settings.connections.api.model.empty')}
            />
          </Field>
          {/* ---------------------------------------------------------------------------
          FREEZE（临时冻结，不是删除）：价格输入（「输入价格」/「输出价格」）与它们那两句说明。
          为什么关：用户要求界面上不再出现"计价"。这两项的**唯一**去处就是喂本轮／会话的估算读数，
          而那两个显示面上一批已经冻结（见 `ConversationBubble.tsx` 里 `UsageLine`、顶栏
          `dsh-chat__usage-rail` 与 `turnUsage`/`totalCost` 那几处 FREEZE）。于是「输入价格」那句
          说明 ——"输入、输出价格都配置后，才会显示本轮和会话估算费用"—— 说的显示面已经不在了，
          把它留在页面上就是留一句**假话**；而两个改了也看不见任何变化的输入框，比它们不在更坏
          （与「起别名」「启动参数」冻结时同一条理由）。
          关掉之后：连接页不再有这两个输入项，两条 `detail` 也一并停止渲染 —— 界面上不再出现
          `价格`／`费用`／估算数字／`price-input` 这些痕迹（渲染结果由 `settingsPricing.spec.ts`
          钉住）。
          数据与结构一行没动：`store.ts` 的 `priceInputPerMillion` / `priceOutputPerMillion` 字段、
          归一化与设置文档迁移全部照旧，`App.tsx` 照常读取并传给气泡（`apiPricingConfigured`）与
          原生侧；写进设置里的值只是暂时不显示。
          怎么恢复：取消下面这两行的注释即可，别处一个字都不用改 —— `PriceInput` 组件本身、
          `MAX_PRICE_PER_MILLION`、以及四条词条（标题／说明／标签／占位）都还在原位。
          --------------------------------------------------------------------------- */}
          {/* <Field title={t('settings.connections.api.price-input.title')} detail={t('settings.connections.api.price-input.detail')}><PriceInput label={t('settings.connections.api.price-input.label')} value={settings.deepseekApi.priceInputPerMillion} onChange={(priceInputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceInputPerMillion } })} /></Field> */}
          {/* <Field title={t('settings.connections.api.price-output.title')} detail={t('settings.connections.api.price-output.detail')}><PriceInput label={t('settings.connections.api.price-output.label')} value={settings.deepseekApi.priceOutputPerMillion} onChange={(priceOutputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceOutputPerMillion } })} /></Field> */}
        </Card>
  </>
}
