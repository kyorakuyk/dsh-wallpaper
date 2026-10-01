import type { AppearanceSlot } from '../appearance/theme/types.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import { assetUrl } from '../settings/store.ts'
import { t, useLanguage } from '../i18n/index.ts'
import { OFFICIAL_PERSONA_CARDS } from './officialCatalog.ts'

export interface OfficialPersonaCardsProps {
  assets: readonly AppearanceAssetSummary[]
  overrides: Partial<Record<AppearanceSlot, string>>
}

// 两段产品名（`DeepSeek` / `DeepSeek Harness`）两种语言里一样，所以不翻译；tier 那一段有中文。
const FAMILY_LABEL = {
  deepseek: 'DeepSeek',
  harness: 'DeepSeek Harness',
} as const

/**
 * A reference grid for the built-in forms. Cards deliberately contain no
 * controls: switching an age manually would break the backend/model mapping.
 * Per-slot artwork replacement remains in the appearance library instead.
 */
export function OfficialPersonaCards({ assets, overrides }: OfficialPersonaCardsProps) {
  // 卡片上的字都来自词条，语言一变就要重渲染一次。
  useLanguage()
  const assetsById = new Map(assets.map((asset) => [asset.id, asset]))

  return <div className="official-persona-grid" role="list" aria-label={t('persona.official.list.label')}>
    {OFFICIAL_PERSONA_CARDS.map((card) => {
      const replacement = overrides[card.slot] ? assetsById.get(overrides[card.slot]!) : undefined
      const hasReplacement = Boolean(overrides[card.slot])
      return <article
        className={`official-persona-card official-persona-card--${card.family}`}
        data-persona-id={card.id}
        data-slot={card.slot}
        key={card.id}
        role="listitem"
      >
        <div className="official-persona-card__portrait" aria-hidden="true">
          <img src={assetUrl(card.portraitPath)} alt="" loading="lazy" />
        </div>
        <div className="official-persona-card__copy">
          <div className="official-persona-card__labels">
            <span>{FAMILY_LABEL[card.family]}</span>
            <strong>{t(card.tier === 'flash' ? 'persona.official.tier.flash' : 'persona.official.tier.pro')}</strong>
          </div>
          <small>{card.slot}</small>
          <p>{replacement
            ? t('persona.official.replaced', { name: replacement.originalName })
            : hasReplacement ? t('persona.official.replaced-custom') : t('persona.official.baseline')}</p>
        </div>
      </article>
    })}
  </div>
}
