import { useMemo, useState } from 'react'
import { APPEARANCE_SLOTS, type AppearanceSlot } from '../../appearance/theme/index.ts'
import { Button, Drawer, Glass, Icon, Menu, MenuItem, MenuLabel, MenuSeparator, Popover } from '../../ui/primitives/index.ts'
import { activeTheme, assetMeta, componentAssets, inboxAssets, overrideCount, SLOT_PRESENTATION, sortThemes, type AppearanceAssetSummary, type AppearanceThemeSummary } from './appearanceViewModel.ts'
import './AppearanceDrawer.css'
import { AssetClassificationPanel } from './AssetClassificationPanel.tsx'
import type { AssetClassificationRequest } from './appearanceViewModel.ts'

export interface AppearanceNotice {
  tone: 'success' | 'info' | 'warning' | 'error'
  message: string
}

export interface AppearanceDrawerProps {
  open: boolean
  themes: AppearanceThemeSummary[]
  assets: AppearanceAssetSummary[]
  activeThemeId: string
  activeThemeVersion: string
  overrides: Partial<Record<AppearanceSlot, string>>
  busy?: boolean
  notice?: AppearanceNotice
  onClose: () => void
  onActivateTheme: (themeId: string, version: string) => void
  onSetOverride: (slot: AppearanceSlot, assetId: string) => void
  onClearOverride: (slot?: AppearanceSlot) => void
  onReviewInbox: (assetId?: string) => void
  onImport: () => void
  onImportFolder: () => void
  onExport: () => void
  onClassify?: (request: AssetClassificationRequest) => void
}

function Preview({ url, name, icon = 'image' }: { url?: string; name: string; icon?: 'image' | 'palette' | 'inbox' }) {
  return <span className="dsh-appearance__preview" aria-hidden="true">
    {url ? <img src={url} alt="" title={name} /> : <Icon name={icon} size={22} />}
  </span>
}

export function AppearanceDrawer(props: AppearanceDrawerProps) {
  const [openSlot, setOpenSlot] = useState<AppearanceSlot>()
  const [classifyingIds, setClassifyingIds] = useState<string[]>([])
  const orderedThemes = useMemo(() => sortThemes(props.themes), [props.themes])
  const current = useMemo(() => activeTheme(props.themes, props.activeThemeId, props.activeThemeVersion), [props.activeThemeId, props.activeThemeVersion, props.themes])
  const pending = useMemo(() => inboxAssets(props.assets), [props.assets])
  const overridesTotal = overrideCount(props.overrides)

  return <Drawer
    open={props.open}
    title="外观"
    description="主题决定整体风格，独立素材可以覆盖其中一个组件。"
    onClose={props.onClose}
    footer={<div className="dsh-appearance__footer-actions">
      <Button variant="secondary" onClick={props.onImport} disabled={props.busy}><Icon name="import" />导入</Button>
      <Button variant="primary" onClick={props.onExport} disabled={props.busy}><Icon name="export" />导出当前搭配</Button>
    </div>}
  >
    <div className="dsh-appearance" data-interaction-region="appearance-drawer">
      {props.notice && <div className={`dsh-appearance__notice dsh-appearance__notice--${props.notice.tone}`} role="status">{props.notice.message}</div>}
      <section className="dsh-appearance__section" aria-labelledby="appearance-themes-title">
        <div className="dsh-appearance__section-heading">
          <div><h3 id="appearance-themes-title">主题</h3><p>切换主题会清除当前的单项替换。</p></div>
        </div>
        <div className="dsh-appearance__theme-list">
          {orderedThemes.map((theme) => {
            const selected = theme.id === props.activeThemeId && theme.version === props.activeThemeVersion
            return <button
              key={`${theme.id}@${theme.version}`}
              className={`dsh-appearance__theme ${selected ? 'is-active' : ''}`}
              type="button"
              disabled={props.busy}
              aria-pressed={selected}
              onClick={() => props.onActivateTheme(theme.id, theme.version)}
            >
              <Preview url={theme.previewUrl} name={theme.name} icon="palette" />
              <span className="dsh-appearance__theme-copy">
                <strong>{theme.name}</strong>
                <small>{theme.author ? `${theme.author} · ` : ''}v{theme.version}</small>
              </span>
              <span className={`dsh-appearance__source dsh-appearance__source--${theme.source}`}>
                {theme.readonly && <Icon name="lock" size={11} />}{theme.source === 'official' ? '官方' : '用户'}
              </span>
              {selected && <span className="dsh-appearance__selected"><Icon name="check" size={14} /></span>}
            </button>
          })}
        </div>
      </section>

      <Glass as="section" className="dsh-appearance__current" strength="soft">
        <div className="dsh-appearance__current-title">
          <div><span>当前主题</span><strong>{current?.name ?? props.activeThemeId}</strong></div>
          <span>v{props.activeThemeVersion}</span>
        </div>
        <div className="dsh-appearance__status-grid">
          <div><strong>{current?.inheritedSlots?.length ?? 0}</strong><span>项继承官方基线</span></div>
          <div><strong>{overridesTotal}</strong><span>项独立素材覆盖</span></div>
        </div>
        {current?.inheritedSlots?.length ? <p className="dsh-appearance__inheritance">继承：{current.inheritedSlots.map((slot) => SLOT_PRESENTATION[slot].shortLabel).join('、')}</p> : null}
        {overridesTotal > 0 && <Button className="dsh-appearance__reset" variant="ghost" onClick={() => props.onClearOverride()} disabled={props.busy}><Icon name="refresh" />恢复主题默认</Button>}
      </Glass>

      <section className="dsh-appearance__section" aria-labelledby="appearance-components-title">
        <div className="dsh-appearance__section-heading">
          <div><h3 id="appearance-components-title">单项组件</h3><p>只显示已分类的独立素材，不会拆开主题包。</p></div>
        </div>
        <div className="dsh-appearance__slot-grid">
          {APPEARANCE_SLOTS.map((slot) => {
            const candidates = componentAssets(props.assets, slot)
            const activeAssetId = props.overrides[slot]
            const activeAsset = activeAssetId ? candidates.find((asset) => asset.id === activeAssetId) : undefined
            return <Popover
              key={slot}
              open={openSlot === slot}
              align="end"
              onOpenChange={(open) => setOpenSlot(open ? slot : undefined)}
              anchor={<button className={`dsh-appearance__slot ${activeAsset ? 'is-overridden' : ''}`} type="button" onClick={() => setOpenSlot(openSlot === slot ? undefined : slot)}>
                <Preview url={activeAsset?.previewUrl} name={activeAsset?.originalName ?? SLOT_PRESENTATION[slot].label} />
                <span><strong>{SLOT_PRESENTATION[slot].shortLabel}</strong><small>{activeAsset?.originalName ?? '使用主题默认'}</small></span>
                <span className="dsh-appearance__slot-count">{candidates.length}</span>
              </button>}
            >
              <Menu label={`${SLOT_PRESENTATION[slot].label}素材`}>
                <MenuLabel>{SLOT_PRESENTATION[slot].label}</MenuLabel>
                <MenuItem label="使用主题默认" checked={!activeAssetId} icon={<Icon name="palette" size={14} />} onSelect={() => { props.onClearOverride(slot); setOpenSlot(undefined) }} />
                {candidates.length > 0 && <MenuSeparator />}
                {candidates.map((asset) => <MenuItem key={asset.id} label={asset.originalName} checked={activeAssetId === asset.id} icon={<Preview url={asset.previewUrl} name={asset.originalName} />} onSelect={() => { props.onSetOverride(slot, asset.id); setOpenSlot(undefined) }} />)}
                {candidates.length === 0 && <div className="dsh-appearance__menu-empty">还没有适用于此组件的独立素材</div>}
              </Menu>
            </Popover>
          })}
        </div>
      </section>

      <section className="dsh-appearance__section" aria-labelledby="appearance-inbox-title">
        <div className="dsh-appearance__section-heading">
          <div><h3 id="appearance-inbox-title">待分类区 <span className="dsh-appearance__count">{pending.length}</span></h3><p>确认用途后，素材才会进入对应组件菜单。</p></div>
          {pending.length > 0 && <Button variant="ghost" onClick={() => { setClassifyingIds(pending.map((asset) => asset.id)); props.onReviewInbox() }} disabled={props.busy}>全部整理</Button>}
        </div>
        {pending.length === 0
          ? <button className="dsh-appearance__inbox-empty" type="button" onClick={props.onImport} disabled={props.busy}><Icon name="inbox" size={24} /><strong>待分类区是空的</strong><span>导入图片、字体、文件夹或主题包</span></button>
          : <div className="dsh-appearance__inbox-list">{pending.slice(0, 4).map((asset) => <button key={asset.id} className="dsh-appearance__inbox-card" type="button" onClick={() => { setClassifyingIds([asset.id]); props.onReviewInbox(asset.id) }} disabled={props.busy}>
              <Preview url={asset.previewUrl} name={asset.originalName} icon="inbox" />
              <span><strong>{asset.originalName}</strong><small>{assetMeta(asset)}</small></span>
              <span>分类</span>
            </button>)}</div>}
        {pending.length > 4 && <Button className="dsh-appearance__show-all" variant="ghost" onClick={() => props.onReviewInbox()} disabled={props.busy}>查看其余 {pending.length - 4} 项</Button>}
        {classifyingIds.length > 0 && <AssetClassificationPanel assets={pending.filter((asset) => classifyingIds.includes(asset.id))} busy={props.busy} onCancel={() => setClassifyingIds([])} onConfirm={(request) => { props.onClassify?.(request); if (props.onClassify) setClassifyingIds([]) }} />}
      </section>
      <section className="dsh-appearance__import-options"><Button variant="secondary" onClick={props.onImport} disabled={props.busy}><Icon name="import" />选择文件</Button><Button variant="secondary" onClick={props.onImportFolder} disabled={props.busy}><Icon name="inbox" />选择文件夹</Button></section>
    </div>
  </Drawer>
}
