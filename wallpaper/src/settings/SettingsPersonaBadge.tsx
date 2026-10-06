import type { BackendMode } from '../domain/types.ts'
import { harnessStateLabel } from '../connect/harnessLabels.ts'
import type { HarnessAvailability } from '../connect/harness.ts'
import { officialPersonaCardFor } from '../persona/officialCatalog.ts'
import { assetUrl } from '../runtime/assets.ts'
import { BACKGROUND_OPTIONS, type BackgroundId } from './store.ts'

export function SettingsPersonaBadge({ backend, backgroundId, harnessStatus }: {
  backend: BackendMode
  backgroundId: BackgroundId
  harnessStatus: HarnessAvailability
}) {
  const persona = officialPersonaCardFor(backend, 'pro')
  const background = BACKGROUND_OPTIONS.find((option) => option.id === backgroundId)
  const connectionClass = harnessStatus === 'bridge-ready'
    ? 'is-online'
    : harnessStatus === 'offline'
      ? ''
      : 'is-pending'

  return <div className={`settings-persona-badge settings-persona-badge--${persona.family}`}>
    {background?.path && <img className="settings-persona-badge__bg" src={assetUrl(background.path)} alt="" onError={(event) => { event.currentTarget.hidden = true }} />}
    <img className="settings-persona-badge__portrait" src={assetUrl(persona.portraitPath)} alt="" onError={(event) => { event.currentTarget.hidden = true }} />
    <div className="settings-persona-badge__caption">
      <strong>{persona.name}</strong>
      <span><i className={connectionClass} />{harnessStateLabel(harnessStatus)}</span>
    </div>
  </div>
}
