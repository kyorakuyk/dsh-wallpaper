import type { Dispatch, SetStateAction } from 'react'
import { t } from '../../i18n/index.ts'
import { assetUrl } from '../../runtime/assets.ts'
import { preferredDisplayId } from '../../runtime/displayLayout.ts'
import { displayNumbers } from '../displayLayoutMap.ts'
import { DisplayLayoutMap } from '../DisplayLayoutMap.tsx'
import { SettingsIcon } from '../SettingsIcon.tsx'
import { BACKGROUND_OPTIONS, type WallpaperSettings } from '../store.ts'
import { Card, Choice, Field, Segmented, Toggle } from '../components/SettingsControls.tsx'
import type { SettingsPanelProps } from '../SettingsPanel.tsx'

function displayLabel(number: number): string {
  return number === 1
    ? t('settings.general.display.primary-name')
    : t('settings.general.display.secondary-name', { number })
}

interface GeneralTabProps extends SettingsPanelProps {
  set: (patch: Partial<WallpaperSettings>) => void
  selectedDisplayId: string | undefined
  setSelectedDisplayId: (value: string | undefined) => void
  showDisplayNumbers: boolean
  setShowDisplayNumbers: Dispatch<SetStateAction<boolean>>
  language: WallpaperSettings['language']
  onLanguageChange: (value: string) => void
}

export function GeneralTab(props: GeneralTabProps) {
  const { settings, language, set, selectedDisplayId, setSelectedDisplayId, showDisplayNumbers, setShowDisplayNumbers } = props
  const monitorNumbers = displayNumbers(props.desktopDisplays)
  const displayOptions = props.desktopDisplays.map((display, index) => ({
    value: display.id,
    label: displayLabel(monitorNumbers[index]),
    number: monitorNumbers[index],
  }))
  const selectedDisplayIdForView = preferredDisplayId(props.desktopDisplays, selectedDisplayId ?? settings.multiScreen.portraitDisplayId) ?? ''
  const selectedDisplayLabel = displayOptions.find((option) => option.value === selectedDisplayIdForView)?.label ?? ''
  const portraitDisplayId = preferredDisplayId(props.desktopDisplays, settings.multiScreen.portraitDisplayId)
  const conversationDisplayId = preferredDisplayId(props.desktopDisplays, settings.multiScreen.conversationDisplayId)
  const backgroundUrlForDisplay = (displayId: string) => {
    const backgroundId = settings.multiScreen.backgrounds[displayId] ?? settings.background
    const path = BACKGROUND_OPTIONS.find((background) => background.id === backgroundId)?.path
    return path ? assetUrl(path) : undefined
  }
  const setDisplayBackground = (displayId: string, value: string) => {
    const backgrounds = { ...settings.multiScreen.backgrounds }
    if (value) backgrounds[displayId] = value as WallpaperSettings['background']
    else delete backgrounds[displayId]
    set({ multiScreen: { ...settings.multiScreen, backgrounds } })
  }
  return <>
        <Card title={t('settings.general.interaction.title')} description={t('settings.general.interaction.description')}>
          <Field title={t('settings.general.conversation-window.title')} detail={t('settings.general.conversation-window.detail')}><Toggle label={t('settings.general.conversation-window.toggle')} checked={props.interactionEnabled} onChange={props.onSetInteractionEnabled} /></Field>
          <Field title={t('settings.general.bubble-layout.title')} detail={t('settings.general.bubble-layout.detail')}><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.bubble-layout.title')} value={settings.interactionLayout} onChange={(value) => set({ interactionLayout: value as WallpaperSettings['interactionLayout'] })} options={[{ value: 'floating', label: t('settings.general.bubble-layout.floating') }, { value: 'taskbar-docked', label: t('settings.general.bubble-layout.taskbar-docked') }]} /></Field>
          <Field title={t('settings.general.history-drawer.title')} detail={t('settings.general.history-drawer.detail')}><Toggle label={t('settings.general.history-drawer.toggle')} checked={settings.historyStartsExpanded} onChange={(value) => set({ historyStartsExpanded: value })} /></Field>
          <Field title={t('settings.general.shortcut.title')} detail={t('settings.general.shortcut.detail')}><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.shortcut.title')} value={settings.sendShortcut} onChange={(value) => set({ sendShortcut: value as WallpaperSettings['sendShortcut'] })} options={[{ value: 'Enter', label: t('settings.general.shortcut.enter') }, { value: 'Ctrl+Enter', label: t('settings.general.shortcut.ctrl-enter') }]} /></Field>
        </Card>
        <Card title={t('language.card')}>
          <Field title={t('language.label')} detail={t('language.hint')}>
            <Choice emptyLabel={t('settings.choice.empty')} label={t('language.label')} value={language} options={[{ value: 'zh', label: t('language.zh') }, { value: 'en', label: t('language.en') }]} onChange={props.onLanguageChange} />
          </Field>
        </Card>
        {props.desktopDisplays.length > 1 && <Card title={t('settings.general.multi-screen.title', { count: props.desktopDisplays.length })} description={t('settings.general.multi-screen.description')}>
          <Field title={t('settings.general.multi-screen.toggle')} detail={settings.multiScreen.enabled ? t('settings.general.multi-screen.on') : t('settings.general.multi-screen.off')}><Toggle label={t('settings.general.multi-screen.toggle')} checked={settings.multiScreen.enabled} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, enabled: value } })} /></Field>
          {settings.multiScreen.enabled && <>
            <DisplayLayoutMap displays={props.desktopDisplays} labels={displayOptions.map((option) => option.label)} numbers={displayOptions.map((option) => option.number)} backgroundUrlFor={backgroundUrlForDisplay} portraitDisplayId={portraitDisplayId} conversationDisplayId={conversationDisplayId} selectedId={selectedDisplayIdForView} onSelect={setSelectedDisplayId} showNumbers={showDisplayNumbers} onToggleNumbers={() => setShowDisplayNumbers((shown) => !shown)} />
            <div className="settings-display-background">
              <div className="settings-display-background__heading">
                <strong>{t('settings.general.display-map.background-of', { display: selectedDisplayLabel })}</strong>
                <span>{t('settings.general.display-map.background-hint')}</span>
              </div>
              <div className="settings-display-background-grid" role="group" aria-label={t('settings.general.display-map.background-of', { display: selectedDisplayLabel })}>
                <button type="button" className={`settings-display-background__follow ${!settings.multiScreen.backgrounds[selectedDisplayIdForView] ? 'is-active' : ''}`} aria-pressed={!settings.multiScreen.backgrounds[selectedDisplayIdForView]} onClick={() => setDisplayBackground(selectedDisplayIdForView, '')}>
                  <span><SettingsIcon name="check" size={16} /></span>
                  <strong>{t('settings.general.display-map.follow-global')}</strong>
                </button>
                {BACKGROUND_OPTIONS.map((background) => <button type="button" key={background.id} data-background={background.id} className={settings.multiScreen.backgrounds[selectedDisplayIdForView] === background.id ? 'is-active' : ''} aria-pressed={settings.multiScreen.backgrounds[selectedDisplayIdForView] === background.id} onClick={() => setDisplayBackground(selectedDisplayIdForView, background.id)}>
                  <span style={background.path ? { backgroundImage: `url(${assetUrl(background.path)})` } : undefined} />
                  <strong>{background.name}</strong>
                </button>)}
              </div>
            </div>
            <Field title={t('settings.general.display.portrait.title')} detail={t('settings.general.display.portrait.detail')}>
              {props.desktopDisplays.length <= 4
                ? <Segmented label={t('settings.general.display.portrait.title')} value={portraitDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, portraitDisplayId: value || undefined } })} options={displayOptions} />
                : <Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.display.portrait.title')} value={portraitDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, portraitDisplayId: value || undefined } })} options={displayOptions} />}
            </Field>
            <Field title={t('settings.general.display.conversation.title')} detail={t('settings.general.display.conversation.detail')}>
              {props.desktopDisplays.length <= 4
                ? <Segmented label={t('settings.general.display.conversation.title')} value={conversationDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, conversationDisplayId: value || undefined } })} options={displayOptions} />
                : <Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.display.conversation.title')} value={conversationDisplayId ?? ''} onChange={(value) => set({ multiScreen: { ...settings.multiScreen, conversationDisplayId: value || undefined } })} options={displayOptions} />}
            </Field>
          </>}
          <div className="integration-actions"><button className="settings-action secondary" onClick={() => void props.onRefreshDesktopDisplays()}>{t('settings.general.display.refresh')}</button></div>
        </Card>}
        <Card title={t('settings.general.conversation-lifecycle.title')}><Field title={t('settings.general.conversation-policy.title')} detail={t('settings.general.conversation-policy.detail')}><Choice emptyLabel={t('settings.choice.empty')} label={t('settings.general.conversation-policy.title')} value={settings.conversationPolicy} onChange={(value) => set({ conversationPolicy: value as WallpaperSettings['conversationPolicy'] })} options={[{ value: 'resume-last', label: t('settings.general.conversation-policy.resume-last') }, { value: 'new-on-unlock', label: t('settings.general.conversation-policy.new-on-unlock') }, { value: 'daily', label: t('settings.general.conversation-policy.daily') }]} /></Field></Card>
        <Card title={t('settings.general.advanced.title')} description={t('settings.general.advanced.description')}>
          <Field title={t('settings.general.ambient-length.title')} detail={t('settings.general.ambient-length.detail', { percent: settings.portraitAmbientLength })}><input type="range" min="35" max="100" step="1" value={settings.portraitAmbientLength} onChange={(event) => set({ portraitAmbientLength: Number(event.target.value) })} /></Field>
          <Field title={t('settings.general.ambient-strength.title')} detail={`${Math.round(settings.portraitAmbientStrength * 100)}%`}><input type="range" min="0" max="1" step="0.01" value={settings.portraitAmbientStrength} onChange={(event) => set({ portraitAmbientStrength: Number(event.target.value) })} /></Field>
          <Field title={t('settings.general.conversation-opacity.title')} detail={t('settings.general.conversation-opacity.detail', { percent: Math.round(settings.conversationOpacity * 100) })}><input type="range" min="0.2" max="0.96" step="0.01" value={settings.conversationOpacity} onChange={(event) => set({ conversationOpacity: Number(event.target.value) })} /></Field>
          <Field title={t('settings.general.conversation-blur.title')} detail={t('settings.general.conversation-blur.detail', { pixels: settings.conversationBlur })}><input type="range" min="0" max="40" step="1" value={settings.conversationBlur} onChange={(event) => set({ conversationBlur: Number(event.target.value) })} /></Field>
        </Card>
  </>
}
