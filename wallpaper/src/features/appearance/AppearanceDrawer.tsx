import { useMemo, useState } from 'react'
import { APPEARANCE_SLOTS, type AppearanceSlot } from '../../appearance/theme/index.ts'
import { t, useLanguage } from '../../i18n/index.ts'
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
  // Subscribes this drawer to language changes: the copy below and the slot names it reads are
  // resolved through `t()` while rendering, so a switch has to re-render it.
  useLanguage()
  const [openSlot, setOpenSlot] = useState<AppearanceSlot>()
  const [classifyingIds, setClassifyingIds] = useState<string[]>([])
  const orderedThemes = useMemo(() => sortThemes(props.themes), [props.themes])
  const current = useMemo(() => activeTheme(props.themes, props.activeThemeId, props.activeThemeVersion), [props.activeThemeId, props.activeThemeVersion, props.themes])
  const pending = useMemo(() => inboxAssets(props.assets), [props.assets])
  const overridesTotal = overrideCount(props.overrides)

  return <Drawer
    open={props.open}
    title={t('appearance.drawer.title')}
    description={t('appearance.drawer.description')}
    onClose={props.onClose}
    footer={<div className="dsh-appearance__footer-actions">
      <Button variant="secondary" onClick={props.onImport} disabled={props.busy}><Icon name="import" />{t('appearance.drawer.import')}</Button>
      <Button variant="primary" onClick={props.onExport} disabled={props.busy}><Icon name="export" />{t('appearance.drawer.export')}</Button>
    </div>}
  >
    <div className="dsh-appearance" data-interaction-region="appearance-drawer">
      {props.notice && <div className={`dsh-appearance__notice dsh-appearance__notice--${props.notice.tone}`} role="status">{props.notice.message}</div>}
      <section className="dsh-appearance__section" aria-labelledby="appearance-themes-title">
        <div className="dsh-appearance__section-heading">
          <div><h3 id="appearance-themes-title">{t('appearance.drawer.themes-title')}</h3><p>{t('appearance.drawer.themes-hint')}</p></div>
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
                {theme.readonly && <Icon name="lock" size={11} />}{theme.source === 'official' ? t('appearance.drawer.source-official') : t('appearance.drawer.source-user')}
              </span>
              {selected && <span className="dsh-appearance__selected"><Icon name="check" size={14} /></span>}
            </button>
          })}
        </div>
      </section>

      <Glass as="section" className="dsh-appearance__current" strength="soft">
        <div className="dsh-appearance__current-title">
          <div><span>{t('appearance.drawer.current')}</span><strong>{current?.name ?? props.activeThemeId}</strong></div>
          <span>v{props.activeThemeVersion}</span>
        </div>
        <div className="dsh-appearance__status-grid">
          <div><strong>{current?.inheritedSlots?.length ?? 0}</strong><span>{t('appearance.drawer.inherited-count')}</span></div>
          <div><strong>{overridesTotal}</strong><span>{t('appearance.drawer.override-count')}</span></div>
        </div>
        {current?.inheritedSlots?.length ? <p className="dsh-appearance__inheritance">{t('appearance.drawer.inheritance', { slots: current.inheritedSlots.map((slot) => SLOT_PRESENTATION[slot].shortLabel).join(t('appearance.drawer.inheritance-separator')) })}</p> : null}
        {overridesTotal > 0 && <Button className="dsh-appearance__reset" variant="ghost" onClick={() => props.onClearOverride()} disabled={props.busy}><Icon name="refresh" />{t('appearance.drawer.reset')}</Button>}
      </Glass>

      <section className="dsh-appearance__section" aria-labelledby="appearance-components-title">
        <div className="dsh-appearance__section-heading">
          <div><h3 id="appearance-components-title">{t('appearance.drawer.components-title')}</h3><p>{t('appearance.drawer.components-hint')}</p></div>
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
                <span><strong>{SLOT_PRESENTATION[slot].shortLabel}</strong><small>{activeAsset?.originalName ?? t('appearance.drawer.use-default')}</small></span>
                <span className="dsh-appearance__slot-count">{candidates.length}</span>
              </button>}
            >
              <Menu label={t('appearance.drawer.slot-menu', { slot: SLOT_PRESENTATION[slot].label })}>
                <MenuLabel>{SLOT_PRESENTATION[slot].label}</MenuLabel>
                <MenuItem label={t('appearance.drawer.use-default')} checked={!activeAssetId} icon={<Icon name="palette" size={14} />} onSelect={() => { props.onClearOverride(slot); setOpenSlot(undefined) }} />
                {candidates.length > 0 && <MenuSeparator />}
                {candidates.map((asset) => <MenuItem key={asset.id} label={asset.originalName} checked={activeAssetId === asset.id} icon={<Preview url={asset.previewUrl} name={asset.originalName} />} onSelect={() => { props.onSetOverride(slot, asset.id); setOpenSlot(undefined) }} />)}
                {candidates.length === 0 && <div className="dsh-appearance__menu-empty">{t('appearance.drawer.menu-empty')}</div>}
              </Menu>
            </Popover>
          })}
        </div>
      </section>

      <section className="dsh-appearance__section" aria-labelledby="appearance-inbox-title">
        <div className="dsh-appearance__section-heading">
          <div><h3 id="appearance-inbox-title">{t('appearance.drawer.inbox-title')} <span className="dsh-appearance__count">{pending.length}</span></h3><p>{t('appearance.drawer.inbox-hint')}</p></div>
          {pending.length > 0 && <Button variant="ghost" onClick={() => { setClassifyingIds(pending.map((asset) => asset.id)); props.onReviewInbox() }} disabled={props.busy}>{t('appearance.drawer.review-all')}</Button>}
        </div>
        {pending.length === 0
          ? <button className="dsh-appearance__inbox-empty" type="button" onClick={props.onImport} disabled={props.busy}><Icon name="inbox" size={24} /><strong>{t('appearance.drawer.inbox-empty')}</strong><span>{t('appearance.drawer.inbox-empty-hint')}</span></button>
          : <div className="dsh-appearance__inbox-list">{pending.slice(0, 4).map((asset) => <button key={asset.id} className="dsh-appearance__inbox-card" type="button" onClick={() => { setClassifyingIds([asset.id]); props.onReviewInbox(asset.id) }} disabled={props.busy}>
              <Preview url={asset.previewUrl} name={asset.originalName} icon="inbox" />
              <span><strong>{asset.originalName}</strong><small>{assetMeta(asset)}</small></span>
              <span>{t('appearance.drawer.classify')}</span>
            </button>)}</div>}
        {pending.length > 4 && <Button className="dsh-appearance__show-all" variant="ghost" onClick={() => props.onReviewInbox()} disabled={props.busy}>{t('appearance.drawer.show-more', { count: pending.length - 4 })}</Button>}
        {classifyingIds.length > 0 && <AssetClassificationPanel assets={pending.filter((asset) => classifyingIds.includes(asset.id))} busy={props.busy} onCancel={() => setClassifyingIds([])} onConfirm={(request) => { props.onClassify?.(request); if (props.onClassify) setClassifyingIds([]) }} />}
      </section>
      <section className="dsh-appearance__import-options"><Button variant="secondary" onClick={props.onImport} disabled={props.busy}><Icon name="import" />{t('appearance.drawer.choose-file')}</Button><Button variant="secondary" onClick={props.onImportFolder} disabled={props.busy}><Icon name="inbox" />{t('appearance.drawer.choose-folder')}</Button></section>
    </div>
  </Drawer>
}
