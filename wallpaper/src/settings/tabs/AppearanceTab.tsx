import { t, type MessageKey } from '../../i18n/index.ts'
import { assetUrl } from '../../runtime/assets.ts'
import { officialPersonaCardFor } from '../../persona/officialCatalog.ts'
import type { AppearanceSlot } from '../../appearance/theme/index.ts'
import { AppearancePreview } from '../AppearancePreview.tsx'
import { Card, Choice, Field, Toggle } from '../components/SettingsControls.tsx'
import type { SettingsPanelProps } from '../SettingsPanel.tsx'
import { BACKGROUND_OPTIONS, type WallpaperSettings } from '../store.ts'

const componentSlots: Array<{ slot: AppearanceSlot; labelKey: MessageKey; detailKey: MessageKey }> = [
  { slot: 'desktop.background', labelKey: 'settings.appearance.component.desktop-background.label', detailKey: 'settings.appearance.component.desktop-background.detail' },
  { slot: 'persona.deepseek.flash', labelKey: 'settings.appearance.component.persona-deepseek-flash.label', detailKey: 'settings.appearance.component.persona-deepseek-flash.detail' },
  { slot: 'persona.deepseek.pro', labelKey: 'settings.appearance.component.persona-deepseek-pro.label', detailKey: 'settings.appearance.component.persona-deepseek-pro.detail' },
  { slot: 'persona.harness.flash', labelKey: 'settings.appearance.component.persona-harness-flash.label', detailKey: 'settings.appearance.component.persona-harness-flash.detail' },
  { slot: 'persona.harness.pro', labelKey: 'settings.appearance.component.persona-harness-pro.label', detailKey: 'settings.appearance.component.persona-harness-pro.detail' },
]

interface AppearanceTabProps extends SettingsPanelProps {
  set: (patch: Partial<WallpaperSettings>) => void
}

export function AppearanceTab(props: AppearanceTabProps) {
  const { settings, set } = props
  const backend = props.liveBackend ?? settings.defaultBackend
  const previewBackground = BACKGROUND_OPTIONS.find((background) => background.id === settings.background)
  const previewBackgroundUrl = previewBackground?.path ? assetUrl(previewBackground.path) : undefined
  const previewPortraitUrl = assetUrl(officialPersonaCardFor(backend, 'pro').portraitPath)
  return <>
        <AppearancePreview backgroundUrl={previewBackgroundUrl} portraitUrl={previewPortraitUrl} ambientStrength={settings.portraitAmbientStrength} bubbleText={t('settings.appearance.preview.bubble')} />
        <Card title={t('settings.appearance.background.title')} description={t('settings.appearance.background.description')}><div className="background-grid">{BACKGROUND_OPTIONS.map((background) => <button key={background.id} data-background={background.id} className={settings.background === background.id ? 'is-active' : ''} onClick={() => set({ background: background.id })}><span style={background.path ? { backgroundImage: `url(${assetUrl(background.path)})` } : undefined} /><strong>{background.name}</strong>{settings.background === background.id && <i>{t('settings.appearance.background.current')}</i>}</button>)}</div></Card>
        <Card title={t('settings.appearance.library.title')} description={t('settings.appearance.library.description')}>
          <div className="asset-library-toolbar"><button className="settings-action" onClick={props.onImportAppearance} disabled={props.appearanceBusy}>{t('settings.appearance.library.import')}</button><span>{t('settings.appearance.library.counts', { inbox: props.appearanceAssets.filter((asset) => asset.status === 'inbox').length, usable: props.appearanceAssets.filter((asset) => asset.status === 'classified').length })}</span></div>
          {props.appearanceAssets.filter((asset) => asset.status === 'inbox').length > 0 && <div className="asset-inbox">{props.appearanceAssets.filter((asset) => asset.status === 'inbox').map((asset) => <div className="asset-inbox-row" key={asset.id}><span><strong>{asset.originalName}</strong><small>{asset.width && asset.height ? `${asset.width} × ${asset.height}` : t('settings.appearance.asset.image')}{asset.hasAlpha ? t('settings.appearance.asset.transparent') : ''}</small></span><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.appearance.asset.purpose', { name: asset.originalName })} value="" onChange={(slot) => props.onClassifyAppearance(asset.id, slot as AppearanceSlot)} disabled={props.appearanceBusy} options={[{ value: '', label: t('settings.appearance.asset.choose-purpose') }, ...componentSlots.map(({ slot, labelKey }) => ({ value: slot, label: t(labelKey) }))]} /></div>)}</div>}
          <div className="asset-component-list">{componentSlots.map(({ slot, labelKey, detailKey }) => {
            const label = t(labelKey)
            const detail = t(detailKey)
            const candidates = props.appearanceAssets.filter((asset) => asset.status === 'classified' && asset.slots.includes(slot))
            const selected = props.appearanceOverrides[slot] ?? ''
            return <div className="asset-component-row" key={slot}><span><strong>{label}</strong><small>{t('settings.appearance.component.detail', { detail, count: candidates.length })}</small></span><Choice emptyLabel={t('settings.choice.empty')} label={label} value={selected} onChange={(id) => { if (id) props.onSelectAppearance(slot, id); else props.onClearAppearance(slot) }} disabled={props.appearanceBusy} emptyMessage={candidates.length === 0 ? t('settings.appearance.component.none') : undefined} options={[{ value: '', label: t('settings.appearance.component.use-default') }, ...candidates.map((asset) => ({ value: asset.id, label: `${asset.originalName}${asset.width && asset.height ? ` (${asset.width} × ${asset.height})` : ''}` }))]} /></div>
          })}</div>
        </Card>
        <Card title={t('settings.appearance.wake.title')}>
          <Field title={t('settings.appearance.wake.enabled.title')}><Toggle label={t('settings.appearance.wake.enabled.toggle')} checked={settings.animationsEnabled} onChange={(value) => set({ animationsEnabled: value })} /></Field>
          <Field title={t('settings.appearance.wake.every-unlock.title')}><Toggle label={t('settings.appearance.wake.every-unlock.toggle')} checked={settings.playWakeOnEveryUnlock} onChange={(value) => set({ playWakeOnEveryUnlock: value })} /></Field>
          <Field title={t('settings.appearance.wake.skip.title')}><Toggle label={t('settings.appearance.wake.skip.toggle')} checked={settings.skipWakeAnimation} onChange={(value) => set({ skipWakeAnimation: value })} /></Field>
          {/* 动画速度与氛围强度先冻结前端（用户要求）：这两项还在开发中，暴露出来只会让设置显得
              比实际能用的多。设置字段与后端行为都保留着（见 store.ts 的 animationSpeed /
              animationIntensity），将来解冻时把这两行还原即可，不需要重新接线。 */}
          {/* <Field title="动画速度" detail={`${settings.animationSpeed.toFixed(1)}×`}><input type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(e) => set({ animationSpeed: Number(e.target.value) })} /></Field> */}
          {/* <Field title="氛围强度"><Choice label="氛围强度" value={settings.animationIntensity} onChange={(value) => set({ animationIntensity: value as WallpaperSettings['animationIntensity'] })} options={[{ value: 'low', label: '克制' }, { value: 'normal', label: '标准' }, { value: 'high', label: '鲜明' }]} /></Field> */}
        </Card>
  </>
}
