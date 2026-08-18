import type { AppearanceSlot } from '../appearance/theme/types.ts'
import type { AppearanceAssetSummary } from '../features/appearance/appearanceViewModel.ts'
import { assetUrl } from '../settings/store.ts'
import { OFFICIAL_PERSONA_CARDS } from './officialCatalog.ts'

export interface OfficialPersonaCardsProps {
  assets: readonly AppearanceAssetSummary[]
  overrides: Partial<Record<AppearanceSlot, string>>
}

const FAMILY_LABEL = {
  deepseek: 'DeepSeek',
  harness: 'DeepSeek Harness',
} as const

const TIER_LABEL = {
  flash: 'Flash · 幼年',
  pro: 'Pro · 成年',
} as const

/**
 * A reference grid for the built-in forms. Cards deliberately contain no
 * controls: switching an age manually would break the backend/model mapping.
 * Per-slot artwork replacement remains in the appearance library instead.
 */
export function OfficialPersonaCards({ assets, overrides }: OfficialPersonaCardsProps) {
  const assetsById = new Map(assets.map((asset) => [asset.id, asset]))

  return <div className="official-persona-grid" role="list" aria-label="官方人物列表">
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
            <strong>{TIER_LABEL[card.tier]}</strong>
          </div>
          <small>{card.slot}</small>
          <p>{replacement ? `已替换：${replacement.originalName}` : hasReplacement ? '已替换：自定义素材' : '官方基础立绘'}</p>
        </div>
      </article>
    })}
  </div>
}
