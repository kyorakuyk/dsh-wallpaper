import { useMemo, useState } from 'react'
import type { AppearanceSlot } from '../../appearance/theme/index.ts'
import { t, useLanguage } from '../../i18n/index.ts'
import { Button, Glass, Icon } from '../../ui/primitives/index.ts'
import { assetMeta, compatibleSlots, sanitizeClassificationRequest, SLOT_PRESENTATION, type AppearanceAssetSummary, type AssetClassificationRequest } from './appearanceViewModel.ts'

export interface AssetClassificationPanelProps {
  assets: AppearanceAssetSummary[]
  busy?: boolean
  onCancel: () => void
  onConfirm: (request: AssetClassificationRequest) => void
}

export function AssetClassificationPanel({ assets, busy, onCancel, onConfirm }: AssetClassificationPanelProps) {
  // 面板里每一句话都是词条。
  useLanguage()
  const [selectedIds, setSelectedIds] = useState<string[]>(() => assets.map((asset) => asset.id))
  const [selectedSlots, setSelectedSlots] = useState<AppearanceSlot[]>([])
  const selectedAssets = useMemo(() => assets.filter((asset) => selectedIds.includes(asset.id)), [assets, selectedIds])
  const compatible = useMemo(() => new Set(selectedAssets.flatMap(compatibleSlots)), [selectedAssets])
  const request = sanitizeClassificationRequest(assets, selectedIds, selectedSlots)

  const toggleAsset = (assetId: string) => setSelectedIds((ids) => ids.includes(assetId) ? ids.filter((id) => id !== assetId) : [...ids, assetId])
  const toggleSlot = (slot: AppearanceSlot) => setSelectedSlots((slots) => slots.includes(slot) ? slots.filter((value) => value !== slot) : [...slots, slot])

  return <Glass className="dsh-classify" strength="soft" aria-label={t('appearance.classify.label')}>
    <div className="dsh-classify__heading"><div><strong>{t('appearance.classify.title')}</strong><p>{t('appearance.classify.description')}</p></div><Button variant="ghost" iconOnly onClick={onCancel} aria-label={t('appearance.classify.close')}><Icon name="close" /></Button></div>
    <div className="dsh-classify__assets">{assets.map((asset) => <label key={asset.id} className={`dsh-classify__asset ${selectedIds.includes(asset.id) ? 'is-selected' : ''}`}><input type="checkbox" checked={selectedIds.includes(asset.id)} onChange={() => toggleAsset(asset.id)} /><span><strong>{asset.originalName}</strong><small>{assetMeta(asset)}</small></span></label>)}</div>
    <fieldset className="dsh-classify__slots"><legend>{t('appearance.classify.slots')}</legend>{Object.entries(SLOT_PRESENTATION).map(([key, value]) => {
      const slot = key as AppearanceSlot
      return <label key={slot} className={compatible.has(slot) ? '' : 'is-disabled'}><input type="checkbox" checked={selectedSlots.includes(slot)} disabled={!compatible.has(slot)} onChange={() => toggleSlot(slot)} /><span><strong>{value.label}</strong><small>{value.description}</small></span></label>
    })}</fieldset>
    <div className="dsh-classify__actions"><Button variant="ghost" onClick={onCancel}>{t('appearance.classify.later')}</Button><Button variant="primary" disabled={busy || request.assetIds.length === 0 || request.slots.length === 0} onClick={() => onConfirm(request)}>{t('appearance.classify.confirm')}</Button></div>
  </Glass>
}
