import { t } from '../../i18n/index.ts'
import { OfficialPersonaCards } from '../../persona/OfficialPersonaCards.tsx'
import { Card } from '../components/SettingsControls.tsx'
import type { AppearanceAssetSummary } from '../../features/appearance/appearanceViewModel.ts'
import type { AppearanceSlot } from '../../appearance/theme/index.ts'

interface PersonasTabProps {
  appearanceAssets: AppearanceAssetSummary[]
  appearanceOverrides: Partial<Record<AppearanceSlot, string>>
}

export function PersonasTab({ appearanceAssets, appearanceOverrides }: PersonasTabProps) {
  return <Card title={t('settings.personas.list.title')} description={t('settings.personas.list.description')}>
    <OfficialPersonaCards assets={appearanceAssets} overrides={appearanceOverrides} />
  </Card>
}
