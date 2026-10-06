import type { DesktopDisplayInfo } from '../native/runtime.ts'
import { t } from '../i18n/index.ts'
import { SettingsIcon } from './SettingsIcon.tsx'
import { layoutDisplays } from './displayLayoutMap.ts'

export function DisplayLayoutMap({ displays, labels, backgroundUrlFor, portraitDisplayId, conversationDisplayId, selectedId, onSelect, showNumbers, onToggleNumbers }: {
  displays: DesktopDisplayInfo[]
  labels: string[]
  backgroundUrlFor: (displayId: string) => string | undefined
  portraitDisplayId?: string
  conversationDisplayId?: string
  selectedId: string
  onSelect: (displayId: string) => void
  showNumbers: boolean
  onToggleNumbers: () => void
}) {
  const layout = layoutDisplays(displays, 640, 240)

  return <div className="settings-display-map-panel">
    <div className="settings-display-map-frame" role="group" aria-label={t('settings.general.display-map.label')}>
      <div className="settings-display-map__canvas" style={{ width: `min(100%, ${layout.width}px)`, aspectRatio: `${layout.width} / ${layout.height}` }}>
        {displays.map((display, index) => {
          const tile = layout.tiles[index]
          const label = labels[index] ?? display.name
          const backgroundUrl = backgroundUrlFor(display.id)
          const metrics = t('settings.general.display.metrics', {
            width: display.bounds.width,
            height: display.bounds.height,
            scale: Math.round(display.scaleFactor * 100),
          })

          return <button
            type="button"
            key={display.id}
            className={`settings-display-map__tile ${selectedId === display.id ? 'is-selected' : ''}`}
            aria-pressed={selectedId === display.id}
            aria-label={t('settings.general.display-map.select', { display: label })}
            data-small={tile.width < 160}
            onClick={() => onSelect(display.id)}
            style={{
              left: `${tile.left / layout.width * 100}%`,
              top: `${tile.top / layout.height * 100}%`,
              width: `${tile.width / layout.width * 100}%`,
              height: `${tile.height / layout.height * 100}%`,
            }}
          >
            {backgroundUrl && <img className="settings-display-map__background" src={backgroundUrl} alt="" draggable={false} onError={(event) => { event.currentTarget.hidden = true }} />}
            <span className="settings-display-map__tags">
              {portraitDisplayId === display.id && <span><SettingsIcon name="person" size={12} />{t('settings.general.display-map.portrait-tag')}</span>}
              {conversationDisplayId === display.id && <span><SettingsIcon name="chat" size={12} />{t('settings.general.display-map.chat-tag')}</span>}
            </span>
            <span className="settings-display-map__info">
              <strong>{label}</strong>
              {!tile || tile.width >= 160 ? <small>{metrics}</small> : null}
            </span>
            {showNumbers && <span className="settings-display-map__number" aria-hidden="true">{index + 1}</span>}
          </button>
        })}
      </div>
    </div>
    <div className="settings-display-map__actions">
      <button type="button" className="settings-action secondary" onClick={onToggleNumbers}>
        <SettingsIcon name="monitor" size={16} />
        {t(showNumbers ? 'settings.general.display-map.hide-numbers' : 'settings.general.display-map.identify')}
      </button>
    </div>
  </div>
}
