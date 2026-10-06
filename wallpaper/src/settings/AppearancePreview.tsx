import { t } from '../i18n/index.ts'

export function AppearancePreview({ backgroundUrl, portraitUrl, ambientStrength, bubbleText }: {
  backgroundUrl?: string
  portraitUrl: string
  ambientStrength: number
  bubbleText: string
}) {
  const strength = Math.max(0, Math.min(1, Number.isFinite(ambientStrength) ? ambientStrength : 0))
  const filter = `brightness(${1 - 0.24 * strength}) saturate(${1 - 0.18 * strength}) drop-shadow(-3px 0 ${4 + 6 * strength}px rgba(90, 184, 245, ${0.25 + 0.4 * strength}))`

  return <div className="appearance-preview">
    <div className="appearance-preview__stage" role="img" aria-label={t('settings.appearance.preview.label')}>
      {backgroundUrl && <img className="appearance-preview__background" src={backgroundUrl} alt="" draggable={false} onError={(event) => { event.currentTarget.hidden = true }} />}
      <div className="appearance-preview__foot-shadow" aria-hidden="true" />
      <img className="appearance-preview__portrait" src={portraitUrl} alt="" draggable={false} style={{ filter }} onError={(event) => { event.currentTarget.hidden = true }} />
      <div className="appearance-preview__bubble">{bubbleText}</div>
    </div>
    <p>{t('settings.appearance.preview.caption')}</p>
  </div>
}
