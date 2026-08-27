import { useEffect, useRef, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { BackendMode, ModelTierRule } from '../domain/types.ts'
import { BACKGROUND_OPTIONS, MAX_PRICE_PER_MILLION, normalizedPrice, type WallpaperSettings } from './store.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../appearance/theme/index.ts'
import type { LockScreenDiagnostics, ManagedDshStatus } from '../native/runtime.ts'
import { OfficialPersonaCards } from '../persona/OfficialPersonaCards.tsx'
import './SettingsPanel.css'

type Page = 'general' | 'connections' | 'appearance' | 'personas' | 'system'

export interface SettingsPanelProps {
  settings: WallpaperSettings
  harnessStatus: 'offline' | 'web-only' | 'bridge-ready'
  onChange: (settings: WallpaperSettings) => void
  onRequestDeepSeekLogin: () => void
  onConfigureApiKey: () => void
  onClose: () => void
  interactionEnabled: boolean
  onSetInteractionEnabled: (enabled: boolean) => void
  translucentTb: { installed: boolean; running: boolean; source?: string }
  onRefreshTranslucentTb: () => void
  onLaunchTranslucentTb: () => void
  onInstallTranslucentTb: () => void
  dshCandidates: Array<{ rootPath: string; source: string }>
  onScanDsh: () => void
  onAdoptDsh: (rootPath: string) => void
  onLaunchDsh: () => void
  managedDsh: ManagedDshStatus
  onRefreshManagedDsh: () => void
  onStopManagedDsh: () => void
  appearanceAssets: AppearanceAssetSummary[]
  appearanceOverrides: Partial<Record<AppearanceSlot, string>>
  appearanceBusy: boolean
  onImportAppearance: () => void
  onClassifyAppearance: (assetId: string, slot: AppearanceSlot) => void
  onSelectAppearance: (slot: AppearanceSlot, assetId: string) => void
  onClearAppearance: (slot: AppearanceSlot) => void
  lockScreenDiagnostics?: LockScreenDiagnostics
  onRefreshLockScreenDiagnostics: () => void
  onRestoreLockScreen: () => void
  onClearStaleLockScreenBackup: () => void
  onSetLockScreenEnabled: (enabled: boolean) => void
  lockScreenBusy: boolean
  autostartBusy: boolean
}

const componentSlots: Array<{ slot: AppearanceSlot; label: string; detail: string }> = [
  { slot: 'desktop.background', label: '桌面背景', detail: '工作室场景的底图' },
  { slot: 'persona.deepseek.flash', label: 'DeepSeek Flash 立绘', detail: '蓝色幼年形态' },
  { slot: 'persona.deepseek.pro', label: 'DeepSeek Pro 立绘', detail: '蓝色成年形态' },
  { slot: 'persona.harness.flash', label: 'Harness Flash 立绘', detail: '黑红幼年形态' },
  { slot: 'persona.harness.pro', label: 'Harness Pro 立绘', detail: '黑红成年形态' },
]

const pages: Array<{ id: Page; icon: string; label: string; hint: string }> = [
  { id: 'general', icon: '⌂', label: '常规', hint: '启动与使用方式' },
  { id: 'connections', icon: '⌁', label: '连接', hint: 'DeepSeek 与 DSH' },
  { id: 'appearance', icon: '◐', label: '外观', hint: '背景与动画' },
  { id: 'personas', icon: '◇', label: '形态', hint: '模型映射规则' },
  { id: 'system', icon: '⚙', label: '系统', hint: 'Windows 集成' },
]

function Card({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return <section className="settings-card"><header><h2>{title}</h2>{description && <p>{description}</p>}</header><div className="settings-card__body">{children}</div></section>
}

function Field({ title, detail, children }: { title: string; detail?: string; children: ReactNode }) {
  return <div className="settings-field"><div className="settings-field__copy"><strong>{title}</strong>{detail && <span>{detail}</span>}</div><div className="settings-field__control">{children}</div></div>
}

function Toggle({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={`settings-toggle ${checked ? 'is-on' : ''}`} onClick={() => onChange(!checked)}><span /></button>
}

function Choice({ value, options, onChange, label, disabled = false, emptyMessage }: { value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void; label: string; disabled?: boolean; emptyMessage?: string }) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const current = options.find((option) => option.value === value)?.label ?? options[0]?.label ?? '请选择'
  useEffect(() => {
    const close = (event: MouseEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false) }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [])
  return <div className={`settings-choice ${open ? 'is-open' : ''}`} ref={root}>
    <button type="button" className="settings-choice__trigger" aria-label={label} aria-expanded={open} disabled={disabled} onClick={() => setOpen((shown) => !shown)}><span>{current}</span><i>⌄</i></button>
    {open && <div className="settings-choice__menu" role="listbox" aria-label={label}>{options.map((option) => <button type="button" key={option.value} className={option.value === value ? 'is-selected' : ''} role="option" aria-selected={option.value === value} onClick={() => { onChange(option.value); setOpen(false) }}>{option.label}</button>)}{emptyMessage && <span className="settings-choice__empty">{emptyMessage}</span>}</div>}
  </div>
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
    placeholder="未配置"
    value={value ?? ''}
    onChange={(event) => {
      const raw = event.target.value.trim()
      const parsed = Number(raw)
      onChange(raw === '' ? undefined : normalizedPrice(parsed))
    }}
  />
}

export function SettingsPanel(props: SettingsPanelProps) {
  const { settings, harnessStatus, onChange, onClose, translucentTb } = props
  const [page, setPage] = useState<Page>('general')
  const set = (patch: Partial<WallpaperSettings>) => onChange({ ...settings, ...patch })
  const updateRule = (index: number, patch: Partial<ModelTierRule>) => set({ modelTierRules: settings.modelTierRules.map((rule, i) => i === index ? { ...rule, ...patch } : rule) })
  const pageMeta = pages.find((item) => item.id === page)!

  return <div className="settings-app">
    <header className="settings-titlebar">
      <div className="settings-titlebar__drag" aria-hidden="true" onMouseDown={(event) => {
        if (event.button === 0) void invoke('start_settings_drag')
      }} />
      <div className="settings-brand"><span className="settings-brand__mark">DSH</span><div><strong>Wallpaper</strong><small>个性化控制中心</small></div></div>
      <button className="settings-window-close" aria-label="关闭设置" onClick={onClose}>×</button>
    </header>

    <aside className="settings-sidebar">
      <nav>{pages.map((item) => <button key={item.id} className={page === item.id ? 'is-active' : ''} onClick={() => setPage(item.id)}><span className="settings-nav__icon">{item.icon}</span><span><strong>{item.label}</strong><small>{item.hint}</small></span></button>)}</nav>
      <div className="settings-sidebar__status"><i className={harnessStatus === 'bridge-ready' ? 'is-online' : ''} /><span>{harnessStatus === 'bridge-ready' ? 'DSH Bridge 已连接' : harnessStatus === 'web-only' ? 'DSH 在线，缺少 Bridge' : 'DSH 当前离线'}</span></div>
    </aside>

    <main className="settings-content">
      <div className="settings-page-heading"><div><span>设置 / {pageMeta.label}</span><h1>{pageMeta.label}</h1></div><p>{pageMeta.hint}</p></div>

      {page === 'general' && <>
        <Card title="交互方式" description="决定会话气泡如何出现在桌面上。">
          <Field title="中央会话窗" detail="关闭后仅可通过托盘右键或此处重新显示；不会因失焦、切换应用或按 Esc 自动消失。"><Toggle label="显示中央会话窗" checked={props.interactionEnabled} onChange={props.onSetInteractionEnabled} /></Field>
          <Field title="气泡布局" detail="中央悬浮始终展开；任务栏停靠以胶囊按钮唤起。"><Choice label="气泡布局" value={settings.interactionLayout} onChange={(value) => set({ interactionLayout: value as WallpaperSettings['interactionLayout'] })} options={[{ value: 'floating', label: '中央玻璃悬浮' }, { value: 'taskbar-docked', label: '任务栏停靠胶囊' }]} /></Field>
          <Field title="历史抽屉默认展开" detail="启动或解锁后直接显示最近的对话。"><Toggle label="历史抽屉默认展开" checked={settings.historyStartsExpanded} onChange={(value) => set({ historyStartsExpanded: value })} /></Field>
        </Card>
        <Card title="会话生命周期"><Field title="新会话策略" detail="每个后端分别保留自己的最近会话。"><Choice label="新会话策略" value={settings.conversationPolicy} onChange={(value) => set({ conversationPolicy: value as WallpaperSettings['conversationPolicy'] })} options={[{ value: 'resume-last', label: '恢复最近会话' }, { value: 'new-on-unlock', label: '每次解锁新建' }, { value: 'daily', label: '每日新建' }]} /></Field></Card>
        <Card title="高级外观" description="环境渐变只作用于立绘；会话窗使用独立的亚克力透明度。">
          <Field title="环境渐变长度" detail={`从暗侧向亮侧延伸至 ${settings.portraitAmbientLength}%`}><input type="range" min="35" max="100" step="1" value={settings.portraitAmbientLength} onChange={(event) => set({ portraitAmbientLength: Number(event.target.value) })} /></Field>
          <Field title="环境渐变强度" detail={`${Math.round(settings.portraitAmbientStrength * 100)}%`}><input type="range" min="0" max="1" step="0.01" value={settings.portraitAmbientStrength} onChange={(event) => set({ portraitAmbientStrength: Number(event.target.value) })} /></Field>
          <Field title="中央会话窗透明度" detail={`${Math.round(settings.conversationOpacity * 100)}% · 仅影响亚克力底色，不影响文字可读性`}><input type="range" min="0.2" max="0.96" step="0.01" value={settings.conversationOpacity} onChange={(event) => set({ conversationOpacity: Number(event.target.value) })} /></Field>
          <Field title="中央会话窗磨砂" detail={`${settings.conversationBlur}px · 0 为纯透明玻璃，数值越高背景越柔和`}><input type="range" min="0" max="40" step="1" value={settings.conversationBlur} onChange={(event) => set({ conversationBlur: Number(event.target.value) })} /></Field>
        </Card>
      </>}

      {page === 'connections' && <>
        <Card title="默认后端" description="网页桥接不会在失败时自动切换到付费 API。">
          <Field title="启动时使用"><Choice label="启动时使用" value={settings.defaultBackend} onChange={(value) => set({ defaultBackend: value as BackendMode })} options={[{ value: 'deepseek-web', label: 'DeepSeek 网页入口（实验）' }, { value: 'deepseek-api', label: 'DeepSeek API（付费）' }, { value: 'harness', label: 'DeepSeek Harness' }]} /></Field>
          <Field title="DSH 就绪时自动切换" detail="仅检测到兼容的壁纸 Bridge 才会切换。"><Toggle label="DSH 自动切换" checked={settings.autoSwitchHarness} onChange={(value) => set({ autoSwitchHarness: value })} /></Field>
        </Card>
        <Card title="DeepSeek Harness 启动" description="自动扫描只建议候选路径；壁纸只启动自己登记的 DSH 进程，不会关闭其他 3080 服务。">
          <Field title="自动扫描"><button className="settings-action secondary" onClick={props.onScanDsh}>扫描 DSH</button></Field>
          {props.dshCandidates.map((candidate) => <Field key={candidate.rootPath} title={candidate.rootPath} detail={candidate.source}><button className="settings-action secondary" onClick={() => props.onAdoptDsh(candidate.rootPath)}>采用</button></Field>)}
          <Field title="DSH 根目录"><input value={settings.dshLaunch.rootPath ?? ''} placeholder="自动扫描或手动填写 dsh 项目目录" onChange={(e) => set({ dshLaunch: { ...settings.dshLaunch, rootPath: e.target.value || undefined } })} /></Field>
          <Field title="Profile"><input value={settings.dshLaunch.profile} placeholder="desktop" onChange={(e) => set({ dshLaunch: { ...settings.dshLaunch, profile: e.target.value || 'desktop' } })} /></Field>
          <Field title="启动命令"><input value={settings.dshLaunch.command ?? ''} placeholder="留空时使用 pnpm dsh" onChange={(e) => set({ dshLaunch: { ...settings.dshLaunch, command: e.target.value || undefined } })} /></Field>
          <Field title="启动 DSH" detail="仅启动此处配置的 profile，不会接管已有 3080 服务。"><button className="settings-action" disabled={!settings.dshLaunch.rootPath || props.managedDsh.running} onClick={props.onLaunchDsh}>{props.managedDsh.running ? `运行中 · PID ${props.managedDsh.pid}` : '启动'}</button></Field>
          <Field title="受管进程" detail={props.managedDsh.managed ? `${props.managedDsh.rootPath} · profile ${props.managedDsh.profile}` : '未由本应用启动 DSH；外部 DSH 不会被停止。'}><span className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshManagedDsh}>刷新</button><button className="settings-action secondary" disabled={!props.managedDsh.running} onClick={props.onStopManagedDsh}>停止本应用启动的 DSH</button></span></Field>
        </Card>
        <Card title="DeepSeek 网页入口（实验）" description="在应用内持久 WebView2 中打开 DeepSeek 官方页面，登录后可从桌面会话窗发送消息。"><Field title="官方页面" detail="页面和登录状态由独立 WebView2 配置目录保存；本应用不读取、复制或记录 Cookie。"><button className="settings-action" onClick={props.onRequestDeepSeekLogin}>打开应用内页面</button></Field></Card>
        <Card title="DeepSeek API" description="API 模式会产生实际费用，密钥只保存在 Windows 凭据管理器。">
          <Field title="API 地址"><input value={settings.deepseekApi.baseUrl} onChange={(e) => set({ deepseekApi: { ...settings.deepseekApi, baseUrl: e.target.value } })} /></Field>
          <Field title="模型"><input value={settings.deepseekApi.model} onChange={(e) => set({ deepseekApi: { ...settings.deepseekApi, model: e.target.value } })} /></Field>
          <Field title="访问密钥"><button className="settings-action secondary" onClick={props.onConfigureApiKey}>更新 API Key</button></Field>
          <Field title="输入价格" detail="人民币／每百万 input tokens。输入、输出价格都配置后，才会显示本轮和会话估算费用。"><PriceInput label="输入价格（人民币每百万 tokens）" value={settings.deepseekApi.priceInputPerMillion} onChange={(priceInputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceInputPerMillion } })} /></Field>
          <Field title="输出价格" detail="人民币／每百万 output tokens。留空不会伪造零费用；缓存 token 没有单独价格时会标为估算。"><PriceInput label="输出价格（人民币每百万 tokens）" value={settings.deepseekApi.priceOutputPerMillion} onChange={(priceOutputPerMillion) => set({ deepseekApi: { ...settings.deepseekApi, priceOutputPerMillion } })} /></Field>
        </Card>
      </>}

      {page === 'appearance' && <>
        <Card title="桌面背景" description="官方背景与用户主题资产将保持独立。"><div className="background-grid">{BACKGROUND_OPTIONS.map((background) => <button key={background.id} className={settings.background === background.id ? 'is-active' : ''} onClick={() => set({ background: background.id })}><span style={background.path ? { backgroundImage: `url(${background.path})` } : undefined} /><strong>{background.name}</strong>{settings.background === background.id && <i>当前</i>}</button>)}</div></Card>
        <Card title="素材库" description="导入的单张素材先选择用途，再出现在对应组件的枚举菜单中。主题包和插件将在后续版本单独处理。">
          <div className="asset-library-toolbar"><button className="settings-action" onClick={props.onImportAppearance} disabled={props.appearanceBusy}>导入图片素材</button><span>{props.appearanceAssets.filter((asset) => asset.status === 'inbox').length} 项待分类 · {props.appearanceAssets.filter((asset) => asset.status === 'classified').length} 项可用</span></div>
          {props.appearanceAssets.filter((asset) => asset.status === 'inbox').length > 0 && <div className="asset-inbox">{props.appearanceAssets.filter((asset) => asset.status === 'inbox').map((asset) => <div className="asset-inbox-row" key={asset.id}><span><strong>{asset.originalName}</strong><small>{asset.width && asset.height ? `${asset.width} × ${asset.height}` : '图片'}{asset.hasAlpha ? ' · 透明背景' : ''}</small></span><Choice label={`${asset.originalName} 的用途`} value="" onChange={(slot) => props.onClassifyAppearance(asset.id, slot as AppearanceSlot)} disabled={props.appearanceBusy} options={[{ value: '', label: '选择用途…' }, ...componentSlots.map(({ slot, label }) => ({ value: slot, label }))]} /></div>)}</div>}
          <div className="asset-component-list">{componentSlots.map(({ slot, label, detail }) => {
            const candidates = props.appearanceAssets.filter((asset) => asset.status === 'classified' && asset.slots.includes(slot))
            const selected = props.appearanceOverrides[slot] ?? ''
            return <div className="asset-component-row" key={slot}><span><strong>{label}</strong><small>{detail} · {candidates.length} 项可选</small></span><Choice label={label} value={selected} onChange={(id) => { if (id) props.onSelectAppearance(slot, id); else props.onClearAppearance(slot) }} disabled={props.appearanceBusy} emptyMessage={candidates.length === 0 ? '暂无此类素材，请先导入并指定用途' : undefined} options={[{ value: '', label: '使用官方默认' }, ...candidates.map((asset) => ({ value: asset.id, label: `${asset.originalName}${asset.width && asset.height ? ` (${asset.width} × ${asset.height})` : ''}` }))]} /></div>
          })}</div>
        </Card>
        <Card title="苏醒动画">
          <Field title="启用动画"><Toggle label="启用苏醒动画" checked={settings.animationsEnabled} onChange={(value) => set({ animationsEnabled: value })} /></Field>
          <Field title="每次解锁播放"><Toggle label="每次解锁播放" checked={settings.playWakeOnEveryUnlock} onChange={(value) => set({ playWakeOnEveryUnlock: value })} /></Field>
          <Field title="跳过苏醒过程"><Toggle label="跳过苏醒过程" checked={settings.skipWakeAnimation} onChange={(value) => set({ skipWakeAnimation: value })} /></Field>
          <Field title="动画速度" detail={`${settings.animationSpeed.toFixed(1)}×`}><input type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(e) => set({ animationSpeed: Number(e.target.value) })} /></Field>
          <Field title="氛围强度"><Choice label="氛围强度" value={settings.animationIntensity} onChange={(value) => set({ animationIntensity: value as WallpaperSettings['animationIntensity'] })} options={[{ value: 'low', label: '克制' }, { value: 'normal', label: '标准' }, { value: 'high', label: '鲜明' }]} /></Field>
        </Card>
      </>}

      {page === 'personas' && <>
        <Card title="官方人物列表" description="四张正式立绘是固定的后端／模型层级映射。此处只用于审阅；如需替换某张图，请到“外观 → 素材库”为对应槽位指定素材。">
          <OfficialPersonaCards assets={props.appearanceAssets} overrides={props.appearanceOverrides} />
          <p className="official-persona-note">DeepSeek 使用蓝色形态，Harness 使用黑红形态；Flash 始终对应幼年，Pro 始终对应成年。思考强度只影响氛围，不会改变年龄。</p>
        </Card>
        <Card title="模型与形态映射" description="模型层级决定年龄，思考强度只改变氛围。">
          <div className="rule-list">{settings.modelTierRules.length === 0 && <div className="settings-empty"><strong>尚未创建自定义规则</strong><span>未识别模型会保持当前形态，冷启动默认为 Flash。</span></div>}{settings.modelTierRules.map((rule, index) => <div className="rule-row" key={index}><select aria-label="后端" value={rule.backend} onChange={(e) => updateRule(index, { backend: e.target.value as ModelTierRule['backend'] })}><option value="*">全部后端</option><option value="deepseek-web">DeepSeek Web</option><option value="deepseek-api">DeepSeek API</option><option value="harness">Harness</option></select><select aria-label="匹配方式" value={rule.match} onChange={(e) => updateRule(index, { match: e.target.value as ModelTierRule['match'] })}><option value="exact">精确</option><option value="contains">包含</option><option value="regex">正则</option></select><input aria-label="模型名称" value={rule.pattern} placeholder="模型名称或表达式" onChange={(e) => updateRule(index, { pattern: e.target.value })} /><select aria-label="形态" value={rule.tier} onChange={(e) => updateRule(index, { tier: e.target.value as ModelTierRule['tier'] })}><option value="flash">Flash · 幼年</option><option value="pro">Pro · 成年</option></select><button aria-label="删除规则" onClick={() => set({ modelTierRules: settings.modelTierRules.filter((_, i) => i !== index) })}>×</button></div>)}</div>
          <button className="settings-action secondary add-rule" onClick={() => set({ modelTierRules: [...settings.modelTierRules, { backend: '*', pattern: '', match: 'contains', tier: 'flash' }] })}>＋ 新增映射规则</button>
        </Card>
      </>}

      {page === 'system' && <>
        <Card title="Windows 集成">
          <Field title="登录后自动启动" detail={props.autostartBusy ? '正在更新 Windows 启动任务，请稍候；设置中心仍可继续使用。' : 'MSIX 优先使用 Windows StartupTask，旧版/开发版回退到当前用户启动项；版本更新会保留此状态。'}><Toggle label="登录后自动启动" checked={settings.autostart} onChange={(value) => set({ autostart: value })} disabled={props.autostartBusy} /></Field>
          <Field title="接管锁屏图片" detail={props.lockScreenBusy ? '正在应用系统锁屏设置，请稍候。' : '使用内置且已审计的熟睡画面；密码界面仍由 Windows 原生安全桌面处理。正式版需要 MSIX 包身份。'}><Toggle label="接管锁屏图片" checked={settings.lockScreenEnabled} onChange={props.onSetLockScreenEnabled} disabled={props.lockScreenBusy} /></Field>
          <div className="lockscreen-diagnostics">
            <div className="lockscreen-diagnostics__row"><div><strong>接管状态</strong><small>{props.lockScreenDiagnostics?.managedImageActive ? '正在使用大肥鱼的熟睡画面' : '未检测到本应用的锁屏图片'}</small></div>{props.lockScreenDiagnostics?.managedImageActive ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRestoreLockScreen}>打开 Windows 锁屏设置</button> : props.lockScreenDiagnostics?.staleBackup ? <button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onClearStaleLockScreenBackup}>{props.lockScreenBusy ? '正在清理…' : '清理旧恢复点'}</button> : null}</div>
            <div className="lockscreen-diagnostics__row"><div><strong>接管前检查</strong><small>{props.lockScreenDiagnostics ? props.lockScreenDiagnostics.takeoverAvailable ? 'Windows 与当前应用身份允许尝试设置锁屏图片' : props.lockScreenDiagnostics.supported ? 'Windows 允许，但当前正式版需要 MSIX 包身份' : '当前系统不允许应用修改锁屏图片' : '正在读取系统状态…'}</small></div><button className="settings-action secondary" disabled={props.lockScreenBusy} onClick={props.onRefreshLockScreenDiagnostics}>{props.lockScreenBusy ? '正在应用…' : '刷新检查'}</button></div>
            {props.lockScreenDiagnostics && <ul><li>备份：{props.lockScreenDiagnostics.staleBackup ? '已保留，但当前锁屏已被外部更改' : props.lockScreenDiagnostics.backupValid ? '原静态图片可恢复' : props.lockScreenDiagnostics.backupExists ? '备份失效' : '尚未创建（首次接管时保存）'}</li><li>托管睡眠图：{props.lockScreenDiagnostics.managedImageReady ? '已准备' : '首次接管时准备'}</li>{props.lockScreenDiagnostics.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>}
          </div>
          <Field title="睡眠快捷键"><input className="short-input" value={settings.sleepHotkey} onChange={(e) => set({ sleepHotkey: e.target.value })} /></Field>
        </Card>
        <Card title="透明任务栏" description="通过松耦合方式连接独立安装的 TranslucentTB，本应用不会修改其配置。">
          <div className="integration-status"><div><i className={translucentTb.running ? 'is-online' : ''} /><span><strong>{translucentTb.running ? 'TranslucentTB 正在运行' : translucentTb.installed ? 'TranslucentTB 已安装' : 'TranslucentTB 未安装'}</strong><small>{translucentTb.source ?? '由用户独立安装和管理'}</small></span></div><div className="integration-actions"><button className="settings-action secondary" onClick={props.onRefreshTranslucentTb}>刷新</button><button className="settings-action" onClick={translucentTb.installed ? props.onLaunchTranslucentTb : props.onInstallTranslucentTb}>{translucentTb.installed ? '启动' : '前往商店'}</button></div></div>
        </Card>
      </>}
    </main>

    <footer className="settings-statusbar"><span>dsh-wallpaper · v0.2.0</span><span><i />设置会自动保存</span></footer>
  </div>
}
