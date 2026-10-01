import type { FullOnlyDict } from './zh.full.ts'

/**
 * English entries — **the half only the full edition says** (`zh.full.ts` holds the Chinese side of the
 * same split). Typed as `FullOnlyDict`, so the two languages cannot drift apart one key at a time.
 */
export const enFull: FullOnlyDict = {
  'language.label': 'Interface language',
  'language.hint': 'Takes effect immediately, and the choice is saved.',
  'language.zh': '简体中文',
  'language.en': 'English',

  'nav.general.label': 'General',
  'nav.general.hint': 'Startup and usage',
  'nav.connections.label': 'Connections',
  'nav.connections.hint': 'DeepSeek and DSH',
  'nav.appearance.label': 'Appearance',
  'nav.appearance.hint': 'Background and animation',
  'nav.personas.label': 'Personas',
  'nav.personas.hint': 'Model mapping rules',
  'nav.history.label': 'History',
  'nav.history.hint': 'API conversations',
  'nav.system.label': 'System',
  'nav.system.hint': 'Windows integration',

  'statusbar.autosave': 'Changes are saved automatically',

  'language.card': 'Language',

  // The six settings pages (`settings/SettingsPanel.tsx`). Keys name a place, not a meaning, and the
  // sentences that come from elsewhere (the "Open" routes, the slot names) keep their own keys.
  'settings.brand.subtitle': 'Personalisation control centre',
  'settings.window.close': 'Close settings',
  'settings.page.heading': 'Settings / {page}',
  'settings.refresh': 'Refresh',
  'settings.open': 'Open',
  'settings.choice.empty': 'Choose one',

  'settings.general.interaction.title': 'Interaction',
  'settings.general.interaction.description': 'Decides how conversation bubbles appear on the desktop.',
  'settings.general.conversation-window.title': 'Centre conversation window',
  'settings.general.conversation-window.detail': 'Once closed, reopen it from the tray menu or from here',
  'settings.general.conversation-window.toggle': 'Show the centre conversation window',
  'settings.general.bubble-layout.title': 'Bubble layout',
  'settings.general.bubble-layout.detail': 'Floating in the centre stays open; the taskbar dock waits behind its capsule button.',
  'settings.general.bubble-layout.floating': 'Floating glass in the centre',
  'settings.general.bubble-layout.taskbar-docked': 'Docked capsule in the taskbar',
  'settings.general.history-drawer.title': 'History drawer starts open',
  'settings.general.history-drawer.detail': 'Show the most recent conversation right after launch or unlock.',
  'settings.general.history-drawer.toggle': 'History drawer starts open',
  'settings.general.shortcut.title': 'Send shortcut',
  'settings.general.shortcut.detail': 'Developers who keep sending by accident can switch to Ctrl+Enter.',
  'settings.general.shortcut.enter': 'Enter sends, Ctrl+Enter adds a line',
  'settings.general.shortcut.ctrl-enter': 'Ctrl+Enter sends, Enter adds a line',

  'settings.general.multi-screen.title': 'Multi-screen desktop · {count} displays detected',
  'settings.general.multi-screen.description': 'Each display fills with its own background; the conversation window and the portrait can be sent to different displays. A display without its own choice follows the global background.',
  'settings.general.multi-screen.toggle': 'Per-display backgrounds',
  'settings.general.multi-screen.on': 'Each display renders separately; changing one leaves the others alone.',
  'settings.general.multi-screen.off': 'Off keeps the single scene that spans the virtual desktop; turning it on reveals the per-display choices.',
  'settings.general.display.number': 'Display {number}',
  'settings.general.display.fallback': 'Display {index}',
  'settings.general.display.metrics': '{width} × {height} px · {scale}% scale',
  'settings.general.display.primary': ' · primary',
  'settings.general.display.background': 'Background for {display}',
  'settings.general.display.follow-global': 'Follow the global background',
  'settings.general.display.conversation.title': 'Display for the conversation window',
  'settings.general.display.conversation.detail': 'Moves the conversation layer only; other displays keep their backgrounds.',
  'settings.general.display.portrait.title': 'Display for the portrait',
  'settings.general.display.portrait.detail': 'The portrait and its bubble mount only on the chosen display.',
  'settings.general.display.refresh': 'Re-detect displays',

  'settings.general.conversation-lifecycle.title': 'Conversation lifetime',
  'settings.general.conversation-policy.title': 'New conversation policy',
  'settings.general.conversation-policy.detail': 'Web mode pins one DeepSeek conversation address; every other backend keeps its own most recent conversation.',
  'settings.general.conversation-policy.resume-last': 'Resume the last conversation',
  'settings.general.conversation-policy.new-on-unlock': 'Start one at every unlock',
  'settings.general.conversation-policy.daily': 'Start one each day',

  'settings.general.advanced.title': 'Advanced appearance (experimental)',
  'settings.general.advanced.description': 'The ambient gradient affects the portrait only; the conversation window has its own acrylic opacity.',
  'settings.general.ambient-length.title': 'Ambient gradient length',
  'settings.general.ambient-length.detail': 'Reaches {percent}% from the dark side towards the lit side',
  'settings.general.ambient-strength.title': 'Ambient gradient strength',
  'settings.general.conversation-opacity.title': 'Conversation window opacity',
  'settings.general.conversation-opacity.detail': '{percent}% · affects the acrylic tint only, never how readable the text is',
  'settings.general.conversation-blur.title': 'Conversation window blur',
  'settings.general.conversation-blur.detail': '{pixels}px · 0 is plain transparent glass, and higher values soften the background further',

  'settings.connections.chat-mode.title': 'Chat mode',
  'settings.connections.chat-mode.description': 'One of two. Changing the channel is remembered as the default for the next launch; the web bridge never falls back to the paid API on its own.',
  'settings.connections.chat-mode.field-title': 'In use',
  'settings.connections.chat-mode.field-detail': 'With the slider on the left, chat takes the channel chosen here; a change is remembered as the next launch default.',
  'settings.connections.chat-mode.auto-switch.title': 'Switch when DSH is ready',
  'settings.connections.chat-mode.auto-switch.detail': 'Switches to harness mode once the harness reports ready.',
  'settings.connections.chat-mode.auto-switch.toggle': 'Automatic DSH switch',
  'settings.connections.backend.deepseek-web': 'DeepSeek Web entry (experimental)',
  'settings.connections.backend.deepseek-api': 'DeepSeek API (paid)',

  'settings.connections.harness.title': 'DeepSeek Harness connection',
  'settings.connections.harness.description': 'For the first run, and for machines that hold more than one different dsh',
  'settings.connections.subject.title': 'How to run it',
  'settings.connections.subject.scanning': 'Searching in the background for anything recognisable; one moment.',
  'settings.connections.subject.none': 'Press "Scan" to find the DSH copies this machine can run.',
  'settings.connections.subject.found': 'Found {count} to choose from{age}.',
  'settings.connections.subject.current': 'Current: {path}',
  'settings.connections.subject.scanning-button': 'Scanning…',
  'settings.connections.subject.rescan': 'Scan again',
  'settings.connections.subject.scan': 'Scan',
  'settings.connections.root-path.title': 'Source folder',
  'settings.connections.root-path.detail': 'Where this source lives; the scan uses it as a hint for where to look next time.',
  'settings.connections.open.title': 'Open its interface',
  'settings.connections.open.detail.no-subject': 'Choose how to run it above first.',
  'settings.connections.open.detail.shell': 'Brings its own window to the front; if it is not running, it is started first.',
  'settings.connections.open.detail.multi': 'The browser uses its web interface; the TUI opens in a new terminal window. Either one is started first if it is not running.',
  'settings.connections.open.detail.single': 'Uses its web interface in your default browser; if it is not running, it is started first.',
  'settings.connections.open.window-label': 'Window to raise',
  'settings.connections.open.busy': 'Working…',
  'settings.connections.open.action': 'Open',
  'settings.connections.launch-with-wallpaper.title': 'Start DSH with the wallpaper',
  'settings.connections.launch-with-wallpaper.shell': 'Runs this client when the wallpaper starts (it shows its window straight away when it cannot start quietly). To make that happen at sign-in too, turn on the wallpaper\u2019s own autostart under "General". An instance already running is never taken over or restarted.',
  'settings.connections.launch-with-wallpaper.checkout': 'Runs this source folder when the wallpaper starts. To make that happen at sign-in too, turn on the wallpaper\u2019s own autostart under "General". An instance already running is never taken over or stopped.',
  'settings.connections.autostart-warning.title': 'The wallpaper does not autostart yet',
  'settings.connections.autostart-warning.detail': 'Starting DSH at sign-in depends on the wallpaper starting itself at sign-in.',
  'settings.connections.autostart-warning.disabled-by-user': 'Windows Task Manager has disabled this app\u2019s startup entry, so "Start DSH with the wallpaper" only takes effect after you open the wallpaper yourself.',
  'settings.connections.autostart-warning.disabled-by-policy': 'A system policy disables this app\u2019s startup entry, so "Start DSH with the wallpaper" only takes effect after you open the wallpaper yourself.',
  'settings.connections.autostart-warning.not-configured': 'The wallpaper has no autostart entry yet, so "Start DSH with the wallpaper" only takes effect after you open the wallpaper yourself. Turn the wallpaper\u2019s autostart on under "General".',
  'settings.connections.managed.title': 'DSH started by this app',
  'settings.connections.managed.yes': 'This DSH was started by this app, and can be stopped here.',
  'settings.connections.managed.no': 'This app has not started a DSH; instances started by anyone else are never stopped.',
  'settings.connections.managed.stop': 'Stop the DSH started by this app',

  'settings.connections.web.title': 'DeepSeek Web entry (experimental)',
  'settings.connections.web.description': 'Chat inside the wallpaper with your web account; after signing in it connects directly.',
  'settings.connections.web.page.title': 'Page',
  'settings.connections.web.page.detail': 'The page and its sign-in state live in a separate WebView2 profile; this app never reads, copies, or records cookies.',
  'settings.connections.web.page.open': 'Open the in-app page',
  'settings.connections.web.adapter.title': 'Web adapter (advanced)',
  'settings.connections.web.adapter.detail': 'Only worth touching when the page structure changes; leave it alone otherwise.',
  'settings.connections.web.adapter.source-local': 'Local override',
  'settings.connections.web.adapter.source-bundled': 'Bundled default',
  'settings.connections.web.adapter.reading': 'Reading the configuration state…',
  'settings.connections.web.adapter.open': 'Open the configuration',
  'settings.connections.web.adapter.reset': 'Restore the default',
  'settings.connections.api.title': 'DeepSeek API',
  'settings.connections.api.description': 'API mode costs real money, and the key is kept in Windows Credential Manager only.',
  'settings.connections.api.key.title': 'Access key',
  'settings.connections.api.key.detail': 'Paste your DeepSeek API key here, confirm it works with "Test", and the available models are pulled in automatically.',
  'settings.connections.api.key.aria': 'DeepSeek API key',
  'settings.connections.api.key.replace': 'Paste a new key to replace it',
  'settings.connections.api.key.placeholder': 'sk-…',
  'settings.connections.api.key.test': 'Test',
  'settings.connections.api.saved.title': 'Saved',
  'settings.connections.api.saved.present': 'This is the masked form of the entry in Credential Manager.',
  'settings.connections.api.saved.absent': 'No API key has been saved yet.',
  'settings.connections.api.saved.aria': 'Saved API key (masked)',
  'settings.connections.api.saved.unconfigured': 'Not configured',
  'settings.connections.api.model.title': 'Model',
  'settings.connections.api.model.available': '{count} models available{age}. Press "Refresh" after the names change.',
  'settings.connections.api.model.hint': 'Press "Refresh" to pull the available models; the list is remembered and shown next time the settings open.',
  'settings.connections.api.model.label': 'DeepSeek API model',
  'settings.connections.api.model.empty': 'No model list has been pulled yet; press "Refresh" first.',
  'settings.connections.api.model.stale': '{model} (not in the current catalogue)',
  'settings.connections.api.model.age.just-now': ' (pulled just now)',
  'settings.connections.api.model.age.minutes': ' (pulled {minutes} min ago)',
  'settings.connections.api.model.age.hours': ' (pulled {hours} h ago)',
  'settings.connections.api.model.age.days': ' (pulled {days} d ago)',
  // FREEZE（临时冻结，不是删除）: the display of this price group goes with the two price inputs
  // (see the FREEZE on those two Field rows in `settings/SettingsPanel.tsx`). The wording of the
  // title / detail / label / placeholder entries is untouched, so uncommenting those rows is the
  // whole restore. The "input price" detail promised estimates that are frozen, hence not rendered.
  'settings.connections.api.price-input.title': 'Input price',
  'settings.connections.api.price-input.detail': 'CNY per million input tokens. Estimates for a turn or a conversation appear only once both prices are set.',
  'settings.connections.api.price-input.label': 'Input price (CNY per million tokens)',
  'settings.connections.api.price-output.title': 'Output price',
  'settings.connections.api.price-output.detail': 'CNY per million output tokens. Leaving it empty never fakes a zero cost; cached tokens are marked as estimated when they have no price of their own.',
  'settings.connections.api.price-output.label': 'Output price (CNY per million tokens)',
  'settings.connections.api.price.placeholder': 'Not configured',

  'settings.appearance.background.title': 'Desktop background',
  'settings.appearance.background.description': 'The built-in backgrounds and your own assets stay independent.',
  'settings.appearance.background.current': 'Current',
  'settings.appearance.library.title': 'Asset library (import is experimental)',
  'settings.appearance.library.description': 'An imported asset is given a purpose first, and then appears in that component\u2019s menu. Theme packages and plugins are handled separately in a later version.',
  'settings.appearance.library.import': 'Import image assets',
  'settings.appearance.library.counts': '{inbox} to sort · {usable} ready',
  'settings.appearance.asset.purpose': 'Purpose of {name}',
  'settings.appearance.asset.choose-purpose': 'Choose a purpose…',
  'settings.appearance.asset.image': 'Image',
  'settings.appearance.asset.transparent': ' · transparent',
  'settings.appearance.component.detail': '{detail} · {count} to choose from',
  'settings.appearance.component.none': 'No assets of this kind yet; import one and give it this purpose',
  'settings.appearance.component.use-default': 'Use the default',
  'settings.appearance.component.desktop-background.label': 'Desktop background',
  'settings.appearance.component.desktop-background.detail': 'The backdrop of the studio scene',
  'settings.appearance.component.persona-deepseek-flash.label': 'DeepSeek Flash portrait',
  'settings.appearance.component.persona-deepseek-flash.detail': 'Blue, young form',
  'settings.appearance.component.persona-deepseek-pro.label': 'DeepSeek Pro portrait',
  'settings.appearance.component.persona-deepseek-pro.detail': 'Blue, adult form',
  'settings.appearance.component.persona-harness-flash.label': 'Harness Flash portrait',
  'settings.appearance.component.persona-harness-flash.detail': 'Black-red, young form',
  'settings.appearance.component.persona-harness-pro.label': 'Harness Pro portrait',
  'settings.appearance.component.persona-harness-pro.detail': 'Black-red, adult form',
  'settings.appearance.wake.title': 'Wake animation (in development)',
  'settings.appearance.wake.enabled.title': 'Enable the animation',
  'settings.appearance.wake.enabled.toggle': 'Enable the wake animation',
  'settings.appearance.wake.every-unlock.title': 'Play at every unlock',
  'settings.appearance.wake.every-unlock.toggle': 'Play at every unlock',
  'settings.appearance.wake.skip.title': 'Skip the waking up',
  'settings.appearance.wake.skip.toggle': 'Skip the waking up',

  'settings.personas.list.title': 'Personas',
  'settings.personas.list.description': 'The four official portraits are a fixed backend-and-model-tier mapping. This page is for review only; to replace one of the images, go to "Appearance \u2192 Asset library" and give that slot an asset.',

  'settings.history.title': 'API conversations',
  'settings.history.description': 'In DeepSeek API mode the transcripts are kept on this machine, encrypted for the current Windows user. Deleting affects this archive only, never the DeepSeek Web entry or Harness conversations.',
  'settings.history.summary': '{conversations} conversations · {messages} messages · {size}',
  'settings.history.budget': ' (budget {size}; the oldest records are dropped once it is exceeded)',
  'settings.history.refresh': 'Refresh',
  'settings.history.refreshing': 'Reading…',
  'settings.history.clear': 'Clear all API history',
  'settings.history.usage': 'Using {percent}% of the app budget; the hard ceiling is {limit}, and at the ceiling this run stops writing to disk instead of overwriting existing records.',
  'settings.history.loading.title': 'Reading the API conversations…',
  'settings.history.loading.detail': 'The first read has to decrypt the local archive.',
  'settings.history.empty.title': 'No API conversations to delete',
  'settings.history.empty.detail': 'Conversations you can manage appear here once you have sent a message in DeepSeek API mode.',
  'settings.history.active': 'Current',
  'settings.history.row-meta': '{messages} messages · {size} · last active {time}',
  'settings.history.row-delete': 'Delete API conversation {id}',
  'settings.history.delete': 'Delete',
  'settings.history.today': 'Today {time}',

  'settings.system.data.title': 'Data and folders',
  'settings.system.data.description': 'The "desktop session" workspace lives inside the wallpaper\u2019s own data folder: an upgrade keeps it, and uninstalling leaves it behind. To remove the data completely, press "Clear all user data" below before uninstalling.',
  'settings.system.workspace.title': 'Workspace',
  'settings.system.workspace.missing': ' (not created yet; the bridge makes it here the first time it needs it)',
  'settings.system.memory.title': 'Project memory',
  'settings.system.memory.present': ' (kept by the assistant; long-standing requests such as how it should speak live here)',
  'settings.system.memory.absent': ' (not yet: it appears the first time you or the assistant note something down)',
  'settings.system.reading': 'Reading…',
  'settings.system.memory.open': 'Open project memory',
  'settings.system.memory.opening': 'Opening…',
  'settings.system.clear.title': 'Clear all user data',
  'settings.system.clear.action': 'Clear all user data',
  'settings.system.clear.clearing': 'Clearing…',
  'settings.system.clear.detail': 'Deletes this app\u2019s desktop-session workspace and the API key held in Credential Manager. Settings and the web sign-in state live in the WebView2 profile and have to be deleted by hand after quitting the app (the paths are listed below).',
  'settings.system.clear.confirm.intro': 'This will delete:',
  'settings.system.clear.confirm.workspace': '\u00b7 the desktop-session workspace ({workspace})',
  'settings.system.clear.confirm.workspace-fallback': 'the "desktop session" folder inside the data folder',
  'settings.system.clear.confirm.credentials': '\u00b7 the DeepSeek API key in Credential Manager',
  'settings.system.clear.confirm.manual-heading': 'You have to delete these yourself (the app is running, so it cannot do it cleanly):',
  'settings.system.clear.confirm.settings': '\u00b7 settings and the web sign-in state: {data}',
  'settings.system.clear.confirm.data-fallback': '%LOCALAPPDATA%\\com.dsh.wallpaper (delete after quitting)',
  'settings.system.clear.confirm.bridge': '\u00b7 bridge credentials: ~/.dsh/wallpaper',
  'settings.system.clear.confirm.continue': 'Continue?',
  'settings.system.clear.removed': 'Deleted {path}',
  'settings.system.clear.credential-removed': 'Deleted the API key from Credential Manager',
  'settings.system.clear.credential-absent': 'Credential Manager held no such key (nothing to delete)',
  'settings.system.clear.manual': 'Delete by hand after quitting the app: {what} \u2192 {path}',
  'settings.system.clear.join': '; ',
  'settings.system.clear.failed': 'Clearing failed: {error}',

  'settings.system.windows.title': 'Windows integration',
  'settings.system.autostart.title': 'Start at sign-in',
  'settings.system.autostart.toggle': 'Start at sign-in',
  'settings.system.autostart.busy': 'Updating the Windows startup task; one moment. The settings centre stays usable.',

  // System · updates (`features/update/*`): the card's fixed wording. The outcomes, the failure reasons
  // and the two actions live in the "update detection" section at the end of this file, because the
  // portrait bubble says exactly the same sentences.
  'settings.system.update.title': 'Updates',
  'settings.system.update.description': 'Updates come from GitHub Releases. The wallpaper checks once after you enter the inner desktop, and at most once every six hours afterwards; the button here is not limited by that.',
  'settings.system.update.current.title': 'Installed version',
  'settings.system.update.current.unavailable': 'The installed version cannot be read, so updates are not checked',
  'settings.system.update.last-check.title': 'Last check',
  'settings.system.update.last-check.never': 'Not checked yet',
  'settings.system.update.check': 'Check for updates',
  'settings.system.update.checking': 'Checking…',

  // ---------------------------------------------------------------------------
  // What the settings window itself says (`settings/SettingsWindow.tsx`): the notice bar, the notices,
  // the bridge feedback and the autostart warnings. All of them are assembled at run time (the
  // placeholders carry ports, subject names, error objects), so the keys name the step, not the
  // sentence. How long a notice stays up is decided by the call site's tone, never by reading these
  // words back - a `/失败|错误|…/` guess stops working the moment the sentence is an entry.
  // ---------------------------------------------------------------------------

  // The notice bar itself, plus the three failures that cut across pages.
  'settings.window.notice.dismiss': 'Dismiss this notice',
  'settings.window.notice.sync-failed': 'Could not publish the settings: {error}',
  'settings.window.notice.appearance-read-failed': 'Could not read the asset library: {error}',
  'settings.window.notice.displays-read-failed': 'Could not read the display list: {error}',

  // Scanning for execution subjects.
  'settings.window.scan.done': 'Scan finished: {count} execution subjects to choose from.',
  'settings.window.scan.none': 'No DSH project and no installed client were found; you can type a DSH project root and scan again.',
  'settings.window.scan.failed': 'Scanning for DSH failed: {error}',

  // The endpoint scan, and clearing a pinned port that belongs to a different subject.
  'settings.window.endpoints.stale-port': 'The port pinned in settings ({port}) does not belong to the current subject, so it was cleared; "Open" will use this subject\'s own port.',
  'settings.window.endpoints.none': 'No Harness to connect to was found. Start the official desktop client, or get the local DSH CLI running (dsh web), then scan again.',
  'settings.window.endpoints.unavailable': 'Found {count} Harness instances, but none of them can hold a conversation right now; see the endpoint list for details.',
  'settings.window.endpoints.found': 'Found {count} Harness instances ready to connect.',
  'settings.window.endpoints.failed': 'Scanning for endpoints failed: {error}',

  // The result of one "Open its interface". `{label}` is the subject class word (also an entry).
  'settings.window.reach.start-failed': 'The launch failed; the log holds the record of this attempt.',
  'settings.window.reach.no-port': 'There is no interface to open: the subject is not listening on any local port.',
  'settings.window.reach.browser-opened': 'Opened 127.0.0.1:{port} in your default browser.',
  'settings.window.reach.raised': 'Brought the {label} window to the front.',
  'settings.window.reach.restored': 'The {label} window is back; Windows refused the foreground switch, so click it once.',
  'settings.window.reach.failed': 'Could not open the client interface: {error}',

  // Stopping the DSH instances this app started.
  'settings.window.managed.stopped-all': 'Stopped every DSH this app started.',
  'settings.window.managed.stopped-one': 'Stopped that DSH instance.',

  // Autostart: the two ways Windows says no, a failed read, and a change that did not take effect.
  'settings.window.autostart.disabled-by-user': 'Windows has autostart turned off for DSH Wallpaper; allow it in Windows settings.',
  'settings.window.autostart.disabled-by-policy': 'A Windows policy forbids DSH Wallpaper from starting at sign-in.',
  'settings.window.autostart.read-failed': 'Could not read the autostart state: {error}',
  'settings.window.autostart.update-failed': 'Could not update autostart: {error}',

  // Aligning the bridge of a subject.
  'settings.window.bridge.failed': 'Installing the bridge failed: {error}',

  // The web adapter configuration: opening it, restoring the defaults (with its confirmation), and
  // the failures of each.
  'settings.window.adapter.opened': 'Opened the web adapter configuration; once you save it, the next web status, history, or send reads the new configuration.',
  'settings.window.adapter.open-failed': 'Could not open the web adapter configuration: {error}',
  'settings.window.adapter.reset-confirm': 'Restoring the default web adapter configuration overwrites the current local override file. Continue?',
  'settings.window.adapter.reset-done': 'The web adapter configuration is back to its defaults.',
  'settings.window.adapter.reset-failed': 'Could not restore the web adapter configuration: {error}',

  // The API key and the model catalogue: reading the state, an address that offers no catalogue, a
  // missing key, the results of testing and saving it, and refreshing the catalogue alone.
  'settings.window.api-key.read-failed': 'Could not read the API key state: {error}',
  'settings.window.api-key.models-unsupported': 'That API address does not offer a model list (HTTP 404/405).',
  'settings.window.api-key.models-read-failed': 'Could not read the model list: {error}',
  'settings.window.api-key.missing': 'Enter a DeepSeek API key first.',
  'settings.window.api-key.saved': 'The API key is saved in Windows Credential Manager and the model list is updated.',
  'settings.window.api-key.usable': 'The saved API key works and the model list is updated.',
  'settings.window.api-key.save-failed': 'Could not save the API key: {error}',
  'settings.window.api-key.models-refreshed': 'The model list is refreshed.',

  // Switching the chat mode (`{label}` is a backend display name, also an entry).
  'settings.window.backend.switched': 'Switched to {label}; the running wallpaper uses it immediately.',
  'settings.window.backend.failed': 'Could not switch the chat backend: {error}',

  // API conversations: deleting one (with its confirmation), clearing them all (likewise), and the
  // failures of each.
  'settings.window.history.delete-confirm': 'Delete the local record of API conversation {id}? This cannot be undone.',
  'settings.window.history.deleted': 'Deleted the local record of API conversation {id}.',
  'settings.window.history.gone': 'That conversation no longer exists; the list is refreshed.',
  'settings.window.history.delete-failed': 'Could not delete the API conversation: {error}',
  'settings.window.history.clear-confirm': 'Clear the local records of all {count} API conversations? This cannot be undone, but it does not affect the DeepSeek Web entry or Harness sessions.',
  'settings.window.history.cleared': 'Cleared the local records of {count} API conversations.',
  'settings.window.history.nothing-to-clear': 'There are no API conversation records to clear.',
  'settings.window.history.clear-failed': 'Could not clear the API history: {error}',

  // Importing, classifying, applying and resetting assets.
  'settings.window.appearance.import-failed': 'Import failed: {error}',
  'settings.window.appearance.classify-failed': 'Classifying the asset failed: {error}',
  'settings.window.appearance.sync-failed': 'Could not publish the appearance change: {error}',
  'settings.window.appearance.apply-failed': 'Could not apply the asset: {error}',
  'settings.window.appearance.reset-failed': 'Could not restore the default: {error}',

  // "Open the TUI": with no TUI installed the native sentence (which says what to do) is shown; this
  // is only the fallback for when native cannot even be asked.
  'settings.window.tui.missing': 'No TUI (dst) was found on this machine.',
  'settings.window.tui.launched': 'The TUI is up in a new terminal window.',
  'settings.window.tui.failed': 'Could not open the TUI: {error}',

  // The DeepSeek in-app page.
  'settings.window.deepseek-web.open-failed': 'Could not open the DeepSeek in-app page: {error}',

  // The two outcomes of "Open the project memory". The file name is the name on disk (native creates
  // it), so it is not translated in either language.
  'settings.window.memory.selected': 'Selected "项目记忆.md" in File Explorer.',
  'settings.window.memory.opened-folder': 'There is no "项目记忆.md" yet: the desktop session folder is open, and it appears there the first time you or the assistant writes something down.',

  // ---------------------------------------------------------------------------
  // The names of the routes the "Open" card offers (`connect/openRoutes.ts`). They are fetched by
  // that **pure function** on every call, which is why `t()` sits inside it and the names are not
  // hoisted into a module-level constant - that would fix the language at import time.
  // ---------------------------------------------------------------------------
  'open.route.shell-window': 'Official client window',
  'open.route.browser': 'Browser',
  'open.route.tui': 'TUI in a terminal',

  // Harness connection state. The first group explains each state that is not ready;
  // `bridge-ready` is the absence of a problem, so it has no sentence and its empty
  // string stays in code - the dictionaries may not carry an empty entry.
  'harness.detail.offline': 'Could not reach the DSH wallpaper Bridge on this machine.',
  'harness.detail.web-only': 'DSH is running, but the wallpaper Bridge is missing, stopped, or incompatible.',
  'harness.detail.bridge-loading': 'The DSH wallpaper Bridge is up and loading the session service.',
  'harness.detail.bridge-auth-unavailable': 'The DSH wallpaper Bridge is up, but its local access token is unavailable. Restart the wallpaper app.',
  'harness.detail.bridge-incompatible': 'The DSH wallpaper Bridge is the wrong version or lacks what this wallpaper needs (an outdated copy may be sitting in the profile). Update the Bridge and try again.',
  // The short label on the status dot in the bubble and the settings sidebar.
  'harness.label.connecting': 'Connecting',
  'harness.label.bridge-ready': 'DSH Bridge connected',
  'harness.label.bridge-auth-unavailable': 'DSH Bridge token unavailable',
  'harness.label.bridge-incompatible': 'DSH Bridge version mismatch',
  'harness.label.web-only': 'DSH online, no Bridge',
  'harness.label.offline': 'DSH offline',

  // Execution subjects: the class words, the name segment, scan age, and launch results.
  'harness.subject.kind.embedded-shell': 'Client',
  'harness.subject.kind.installed-cli': 'Installed CLI',
  'harness.subject.kind.checkout': 'Source folder',
  'harness.subject.detail.shell': 'The client brings its own runtime, so there is no path to fill in.',
  'harness.subject.choice-prompt': 'Found {count} deepseek harness source trees on this computer. Choose the one to use by default.',
  'harness.subject.age.just-now': 'Verified just now',
  'harness.subject.age.minutes': 'Verified {minutes} min ago',
  'harness.subject.age.hours': 'Verified {hours} h ago',
  'harness.subject.age.days': 'Verified {days} d ago',
  'harness.subject.option.checkout': 'Source folder · {name}{version}',
  'harness.subject.instance.port-unknown': 'port unknown',
  'harness.subject.launch.started': '{label} started.',
  'harness.subject.launch.started-hidden': '{label} started with its window in the background; use "Raise window" to bring it up.',
  'harness.subject.launch.started-unconfirmed': '{label} took the launch request but did not answer before the timeout; it may still be starting, so refresh in a moment.',
  'harness.subject.launch.already-running': '{label} is already running; this app will not take it over, restart it, or stop it.',
  'harness.subject.launch.unknown-target': 'Nothing has been chosen to start yet. Scan and pick one first.',
  'harness.subject.launch.root-path-invalid': 'That folder is not a usable DSH source folder. Scan again and pick one.',
  'harness.subject.launch.launcher-missing': 'Could not find the program that starts it: make sure Node.js or pnpm is installed and runs directly from a command prompt.',
  'harness.subject.launch.profile-invalid': 'Invalid profile: letters, digits, hyphens, and underscores only.',
  'harness.subject.launch.port-occupied-external': 'Another program on this machine already holds that port (this app did not start it), so nothing was started twice and nothing will be taken over or stopped.',
  'harness.subject.launch.failed': 'The launch did not succeed. The log holds the full record of this attempt for troubleshooting.',

  // Theme-package validation: one sentence per problem. The slot, version and path come from the
  // package being inspected, so they are filled in rather than baked into the sentence.
  'appearance.validation.component-invalid': '{slot} has an invalid component declaration',
  'appearance.validation.single-asset-required': '{slot} must be declared as a single-file asset',
  'appearance.validation.wake-sequence-frames-required': 'wake.sequence must contain at least one frame',
  'appearance.validation.wake-sequence-frame-invalid': 'wake.sequence contains an invalid frame or duration',
  'appearance.validation.skin-required': 'chat.skin must be declared as a skin',
  'appearance.validation.skin-textures': 'chat.skin textures must be an array of paths',
  'appearance.validation.manifest-object': 'theme.json must be an object',
  'appearance.validation.schema-version': 'schemaVersion {version} is not supported',
  'appearance.validation.kind-theme': 'kind must be theme',
  'appearance.validation.id-format': 'The theme id is malformed',
  'appearance.validation.version-format': 'The theme version must be a semantic version',
  'appearance.validation.name-empty': 'The theme name cannot be empty',
  'appearance.validation.optional-text-type': 'An optional text field has the wrong type',
  'appearance.validation.min-app-version': 'compatibility.minAppVersion is malformed',
  'appearance.validation.baseline-lock': 'baseline must pin a valid id and version',
  'appearance.validation.components-object': 'components must be an object',
  'appearance.validation.unknown-slot': 'Unknown appearance slot {slot}',
  'appearance.validation.files-array': 'files must be an array of file entries',
  'appearance.validation.file-entry': 'files contains an invalid entry',
  'appearance.validation.ui-declaration': 'The ui declaration is invalid',
  'appearance.validation.listed-path-unsafe': 'Unsafe path in the file list: {path}',
  'appearance.validation.listed-hash-format': 'Malformed SHA-256: {path}',
  'appearance.validation.listed-duplicate': 'Duplicate entry in the file list: {path}',
  'appearance.validation.package-path-unsafe': 'Unsafe path inside the package: {path}',
  'appearance.validation.package-symlink': 'A theme package may not contain symbolic links: {path}',
  'appearance.validation.package-duplicate': 'Duplicate file inside the package: {path}',
  'appearance.validation.reference-path-unsafe': 'Unsafe asset reference path: {path}',
  'appearance.validation.reference-unlisted': 'Asset reference is missing from files: {path}',
  'appearance.validation.missing-file': 'The theme package is missing a file: {path}',
  'appearance.validation.size-mismatch': 'File size does not match: {path}',
  'appearance.validation.hash-mismatch': 'File hash does not match: {path}',
  'appearance.validation.unexpected-file': 'The theme package contains an undeclared file: {path}',

  // Appearance slots and the media line under an asset.
  'appearance.slot.desktop.background.label': 'Desktop background',
  'appearance.slot.desktop.background.short': 'Background',
  'appearance.slot.desktop.background.description': 'The backdrop of the desktop scene',
  'appearance.slot.lockscreen.image.label': 'Lock-screen image (reserved)',
  'appearance.slot.lockscreen.image.short': 'Lock screen',
  'appearance.slot.lockscreen.image.description': 'Assets can be filed here in advance; the lock screen itself still uses the built-in sleeping frame',
  'appearance.slot.wake.sequence.label': 'Wake animation',
  'appearance.slot.wake.sequence.short': 'Wake',
  'appearance.slot.wake.sequence.description': 'The ordered frames played after unlock',
  'appearance.slot.persona.deepseek.flash.label': 'DeepSeek Flash portrait',
  'appearance.slot.persona.deepseek.flash.short': 'Blue · young',
  'appearance.slot.persona.deepseek.flash.description': 'The DeepSeek Flash model form',
  'appearance.slot.persona.deepseek.pro.label': 'DeepSeek Pro portrait',
  'appearance.slot.persona.deepseek.pro.short': 'Blue · adult',
  'appearance.slot.persona.deepseek.pro.description': 'The DeepSeek Pro model form',
  'appearance.slot.persona.harness.flash.label': 'Harness Flash portrait',
  'appearance.slot.persona.harness.flash.short': 'Black-red · young',
  'appearance.slot.persona.harness.flash.description': 'The Harness Flash model form',
  'appearance.slot.persona.harness.pro.label': 'Harness Pro portrait',
  'appearance.slot.persona.harness.pro.short': 'Black-red · adult',
  'appearance.slot.persona.harness.pro.description': 'The Harness Pro model form',
  'appearance.slot.chat.skin.label': 'Chat bubble skin',
  'appearance.slot.chat.skin.short': 'Bubble',
  'appearance.slot.chat.skin.description': 'Declarative glass material and textures',
  'appearance.slot.ui.font.label': 'Interface font',
  'appearance.slot.ui.font.short': 'Font',
  'appearance.slot.ui.font.description': 'The font used by chat and menus',
  'appearance.asset.meta.font': 'Font',
  'appearance.asset.meta.sequence': 'Frame sequence',
  'appearance.asset.meta.skin': 'Bubble skin',
  'appearance.asset.meta.image': 'Image',
  // The half-sentence appended to a size when the asset carries an alpha channel.
  'appearance.asset.meta.transparent': ' · transparent',

  // The appearance drawer. The inheritance line is a prefix plus a separator, because the Chinese
  // enumeration comma and the English comma are not the same character.
  'appearance.drawer.title': 'Appearance',
  'appearance.drawer.description': 'The theme sets the overall style; a loose asset can override one of its components.',
  'appearance.drawer.import': 'Import',
  'appearance.drawer.export': 'Export this setup',
  'appearance.drawer.themes-title': 'Themes',
  'appearance.drawer.themes-hint': 'Switching themes clears the per-component overrides.',
  'appearance.drawer.source-official': 'Official',
  'appearance.drawer.source-user': 'User',
  'appearance.drawer.current': 'Current theme',
  'appearance.drawer.inherited-count': 'inherit the official baseline',
  'appearance.drawer.override-count': 'overridden by loose assets',
  'appearance.drawer.inheritance': 'Inherited: {slots}',
  'appearance.drawer.inheritance-separator': ', ',
  'appearance.drawer.reset': 'Reset to the theme default',
  'appearance.drawer.components-title': 'Components',
  'appearance.drawer.components-hint': 'Only classified loose assets appear here; a theme package is never split apart.',
  'appearance.drawer.use-default': 'Use the theme default',
  'appearance.drawer.slot-menu': '{slot} assets',
  'appearance.drawer.menu-empty': 'No loose assets fit this component yet',
  'appearance.drawer.inbox-title': 'Unsorted',
  'appearance.drawer.inbox-hint': 'An asset joins a component menu once you confirm what it is for.',
  'appearance.drawer.review-all': 'Sort all',
  'appearance.drawer.inbox-empty': 'Nothing is waiting to be sorted',
  'appearance.drawer.inbox-empty-hint': 'Import images, fonts, folders, or theme packages',
  'appearance.drawer.classify': 'Classify',
  'appearance.drawer.show-more': 'Show the other {count}',
  'appearance.drawer.choose-file': 'Choose files',
  'appearance.drawer.choose-folder': 'Choose a folder',

  // Chat bubble: backend blurbs, activity states, usage and cost, composer.
  'chat.backend.deepseek-web.name': 'DeepSeek Web bridge',
  'chat.backend.deepseek-web.description': 'Free · experimental',
  'chat.backend.deepseek-api.description': 'Pay per use',
  'chat.backend.harness.description': 'Local tool session',
  'chat.activity.idle': 'Idle',
  'chat.activity.sending': 'Sending',
  'chat.activity.thinking': 'Thinking',
  'chat.activity.streaming': 'Replying',
  'chat.activity.tool': 'Using a tool',
  'chat.activity.done': 'Done',
  'chat.cost.approx': '≈ ',
  'chat.usage.unavailable': 'Not reported',
  'chat.usage.price-unconfigured': 'No price configured',
  'chat.usage.cost-unavailable': 'Cost not reported',
  'chat.composer.disabled': 'Not available in this mode',
  'chat.composer.busy': 'The big fish is still working on your last message…',
  'chat.composer.placeholder': 'What should we work on today?',

  // The tooltip on the host indicator. `{label}` is the subject class word, `{alias}` the name the
  // user gave that subject. The four short words on the element itself (`Web` / `API` / `Desktop` /
  // `TUI`) are not entries: they read the same in both languages.
  'chat.host.alias': ' (alias: {alias})',
  // FREEZE（临时冻结，不是删除）: the billing half of the tooltip is gone (the interface must no
  // longer show "pricing"), while the sentence saying which channel this is stays. The deleted
  // wording is copied here verbatim, so restoring it needs no git archaeology:
  //   'chat.host.deepseek-api': 'Your own DeepSeek API key, billed per token.'
  // Why: this renders as the `title` of the host badge in the footer (`connect/conversationHost.ts`),
  // and the readout that half described was frozen in the previous batch (the two readouts in
  // `ConversationBubble.tsx`). Naming a price in a tooltip points at something no longer on screen.
  // After: the tooltip only says which channel this is. Restore: append the sentence above.
  'chat.host.deepseek-api': 'Your own DeepSeek API key.',
  // FREEZE（临时冻结，不是删除）: same, minus "no API charges". The deleted wording, verbatim:
  //   'chat.host.deepseek-web': 'DeepSeek Web quota; no API charges.'
  // After: the tooltip only says which channel this is (Web quota). Restore: append it again.
  'chat.host.deepseek-web': 'DeepSeek Web quota.',
  'chat.host.embedded-shell': 'The DeepSeek Harness client on this machine, which brings its own window.',
  'chat.host.terminal': '{label}, with its interface in a terminal.',
  'chat.host.browser': '{label}, with its interface in a browser.',

  // ---------------------------------------------------------------------------
  // What the native runtime hands back to the interface (`native/runtime.ts`). These are the answers a
  // browser preview gets - no Tauri, no Windows - and `t()` is called when they are produced, for the
  // same reason as the sentences in `connect/*`.
  //
  // They are **data**: once one is put into an object (`AutostartStatus.reason`, an adapter status's
  // `path`), it is fixed in the language of that moment and the interface does not re-translate it.
  // That is exactly what the plan's "structured state, localised by the interface" is meant to
  // collect, batch by batch; this batch makes them speak the current language instead of hard-coded
  // Chinese.
  // ---------------------------------------------------------------------------
  'runtime.autostart.unsupported': 'This version of Windows cannot start this app at sign-in.',
  // The four "desktop build only" refusals (calling these in a browser preview throws).
  'runtime.credentials.desktop-only': 'Windows Credential Manager is available in the desktop build only',
  'runtime.workspace.desktop-only': 'The desktop session workspace self-check is available in the desktop build only',
  'runtime.memory.desktop-only': 'Opening the project memory is available in the desktop build only',
  'runtime.tui.desktop-only': 'Opening the TUI is available in the desktop build only',
  // The three browser-preview answers for the web adapter configuration (`path` carries a sentence for
  // the interface, not a path).
  'runtime.web-adapter.preview-config-path': 'A browser preview has no local web adapter configuration',
  'runtime.web-adapter.preview-open': 'A browser preview cannot open the web adapter configuration',
  'runtime.web-adapter.preview-reset': 'A browser preview cannot restore the web adapter configuration',
  // A browser preview has no real displays; this names the placeholder one.
  'runtime.display.preview-name': 'Preview display',

  // ---------------------------------------------------------------------------
  // The desktop surfaces (plan step 4). Keys are named by **position**:
  // `app.bubble.*` is `App.tsx` (the wallpaper shell: the portrait bubble, the notice bar, the
  // conversation window, the sign-in overlay), `chat.bubble.*` is
  // `features/chat/ConversationBubble.tsx` (the bubble itself), and `lite.settings.*` is
  // `lite/LiteSettingsWindow.tsx`.
  //
  // Some entries carry a leading (or trailing) space: they are the half-sentence that follows
  // another one, and English needs the space where Chinese does not - the same reason as
  // `chat.cost.approx`.
  // ---------------------------------------------------------------------------

  // The **shared prefix** of the disconnected notice. It is not a whole sentence: prefix + state
  // detail (the `harness.detail.*` entries) + the half-sentence below make one.
  'app.bubble.harness.disconnected-prefix': 'The DSH wallpaper Bridge is currently unavailable. ',
  // The continuation of the two sentences above (leading space, so it follows the state detail).
  'app.bubble.harness.selection-unavailable': ' Harness mode can only be selected once a compatible Bridge is ready.',
  'app.bubble.harness.session-preserved': ' The current Harness session and its transcript are kept; once the Bridge is back you can carry on, or switch backend yourself.',
  // The frame that orders those three parts into one sentence. The whole thing is stored in the
  // runtime state as a `Message` (key + params), so all three `{...}` are entries rather than
  // sentences: switching language re-translates every part. No spaces live here - the two halves
  // that need one carry it themselves (above).
  'app.bubble.harness.disconnected-frame': '{prefix}{detail}{preserved}',
  'app.bubble.harness.selection-frame': '{prefix}{detail}{unavailable}',

  // One sentence per autostart failure code (`dshAutostartNotice`). `dshAutostart.spec.ts` pins
  // several of these words verbatim, so they are not to be reworded casually.
  'app.bubble.autostart.root-path-missing': 'DSH is set to start with the wallpaper, but no execution subject has been chosen yet; scan and pick one in the settings centre.',
  'app.bubble.autostart.unknown-target': 'DSH is set to start with the wallpaper, but the chosen execution subject is unavailable; scan again and pick one in the settings centre.',
  'app.bubble.autostart.started-unconfirmed': 'The chosen client was asked to start, but it did not answer within the timeout; if its interface never appears, check that the client is still installed.',
  'app.bubble.autostart.root-path-invalid': 'DSH is set to start with the wallpaper, but the configured source folder is not a recognisable DSH project; fix it in the settings centre.',
  'app.bubble.autostart.launcher-missing': 'DSH is set to start with the wallpaper, but neither Node.js nor pnpm was found. Check in the settings centre that both are installed and can be run straight from a command prompt.',
  'app.bubble.autostart.profile-invalid': 'DSH is set to start with the wallpaper, but the configured profile name is invalid (letters, digits, hyphens, and underscores only); fix it in the settings centre.',
  'app.bubble.autostart.launch-args-invalid': 'DSH is set to start with the wallpaper, but the launch arguments are invalid. Fix them in the settings centre and try again.',
  'app.bubble.autostart.spawn-failed': 'DSH is set to start with the wallpaper, but the process failed to start. Check the source folder and the launch arguments in the settings centre.',
  'app.bubble.autostart.failed': 'Could not start DSH automatically: {error}',

  // Launch supervision (`harnessLaunchOutcome`): the "it exited" verdict, and the four specific
  // causes a timeout is resolved to.
  'app.bubble.launch.exited-early': 'DSH exited soon after starting; check the DSH configuration or its startup log.',
  'app.bubble.launch.token-unavailable': 'DSH started, but the Bridge\u2019s local token is unavailable; restart the wallpaper app or check the permissions on the token folder.',
  'app.bubble.launch.incompatible': 'DSH started, but the Bridge version or capability is incompatible; update the Bridge and try again.',
  'app.bubble.launch.bridge-loading': 'DSH started, but the Bridge is still loading the session service; if it stays there, check the DSH log.',
  'app.bubble.launch.timeout': 'Starting DSH timed out; the process is still running but the Bridge has not come up.',
  'app.bubble.launch.subject-failed': 'The DSH subject failed to start; see the launch record in the log.',

  // Why the model directory has nothing to show (`reason` on `connect/modelDirectory.ts`).
  'app.bubble.model.harness-reading': 'Reading the Harness model directory…',
  'app.bubble.model.endpoint-reading': 'Reading the endpoint model directory…',
  'app.bubble.model.web-decided': 'The Web entry\u2019s model is decided by the DeepSeek page',
  'app.bubble.model.harness-not-running': 'Harness is not running',
  'app.bubble.model.harness-read-failed': 'Could not read the Harness model directory: {error}',
  'app.bubble.model.endpoint-read-failed': 'Could not read the endpoint model list: {error}',

  // The notice bar and the overlays: a failure is always said out loud, one sentence each.
  'app.bubble.notice.raise-window-failed': 'Could not open the client window: {error}',
  'app.bubble.notice.display-read-failed': 'Could not read the display layout: {error}',
  'app.bubble.notice.display-subscribe-failed': 'Could not subscribe to display events: {error}',
  'app.bubble.notice.system-subscribe-failed': 'Could not subscribe to session events: {error}',
  'app.bubble.notice.appearance-subscribe-failed': 'Could not subscribe to appearance changes: {error}',
  'app.bubble.notice.leave-inner-failed': 'Could not leave the inner desktop: {error}',
  'app.bubble.notice.open-link-failed': 'Could not open the link: {error}',
  // What follows the approval summary the host itself produced (that summary is runtime text,
  // not an entry).
  'app.bubble.notice.approval': '{summary}; open Harness to handle it.',

  // What the bubble above the portrait says.
  'app.bubble.question.prompt': 'I would like your opinion: {question}',
  'app.bubble.question.options': ' ({options})',
  'app.bubble.thinking': 'Thinking it over…',

  // The overlay shown when the Web entry needs a sign-in.
  'app.bubble.auth.title': 'Sign in to the DeepSeek Web entry',
  'app.bubble.auth.body': 'The official page is already open inside the app; sign in there. The sign-in state is kept only in a separate WebView2 profile, and this app never reads or copies cookies. Once you are signed in, come back to the desktop to keep sending.',
  'app.bubble.auth.confirm': 'I have signed in',

  // ---------------------------------------------------------------------------
  // The conversation bubble itself (`features/chat/ConversationBubble.tsx`): labels, hints, the
  // accessible names of its buttons, and the usage row in the footer.
  // ---------------------------------------------------------------------------
  'chat.bubble.surface.label': 'AI conversation',
  'chat.bubble.session-title': 'Desktop session',
  'chat.bubble.raise-window': 'Open the client window',
  'chat.bubble.close': 'Collapse the conversation',
  'chat.bubble.history.label': 'Current transcript',
  'chat.bubble.history.kept-note': 'The previous Harness session (that backend has exited). It returns to this transcript automatically once it is back online; you can also carry on from the left right now.',
  'chat.bubble.history.load-earlier': 'Load {hidden} earlier messages',
  // The two directions of one button: expanded it says "Hide transcript", collapsed "Transcript".
  'chat.bubble.history.toggle-expanded': 'Hide transcript',
  'chat.bubble.history.toggle-collapsed': 'Transcript',
  // The status dot and the mode switch: failure, starting, and the label/tooltip each way.
  'chat.bubble.harness.failed': 'Connection failed',
  'chat.bubble.harness.starting': 'DSH is starting',
  'chat.bubble.mode-switch.to-deepseek': 'Switch to DeepSeek mode',
  'chat.bubble.mode-switch.to-harness': 'Switch to Harness mode',
  'chat.bubble.mode-switch.start': 'Start DSH',
  'chat.bubble.mode-switch.configure': 'Configure DSH',
  'chat.bubble.mode-switch.title-to-deepseek': 'Current: Harness, click to switch back to DeepSeek',
  'chat.bubble.mode-switch.title-to-harness': 'Current: DeepSeek, click to switch to Harness',
  'chat.bubble.mode-switch.start-title': 'Start the configured DSH backend',
  'chat.bubble.mode-switch.configure-title': 'Configure the DSH source folder and profile first',
  // The agent preset dropdown: its name, the fallback entry, and the suffix for a broken preset.
  'chat.bubble.preset.label': 'Choose the DSH mode',
  'chat.bubble.preset.standard': 'Standard mode',
  'chat.bubble.preset.unavailable': ' (unavailable)',
  // The composer and the command/permission menus.
  'chat.bubble.composer.label': 'Message',
  'chat.bubble.composer.send': 'Send message',
  'chat.bubble.composer.stop': 'Stop generating',
  'chat.bubble.command-menu': 'Commands',
  'chat.bubble.command-menu.label': 'Choose a command',
  'chat.bubble.permission.label': 'Choose a permission',
  // The model picker (a dropdown drawn inside the island).
  'chat.bubble.model.label': 'Choose a model',
  'chat.bubble.model.switch': 'Switch model',
  'chat.bubble.model.unsupported': 'This backend cannot switch models inside the wallpaper',
  'chat.bubble.model.unavailable': 'No model to switch to',
  // The footer usage row: the small line under each message (` · Cache N` carries a leading
  // space), then the two cells for this turn and the session.
  'chat.bubble.usage.tokens': 'Input {input} · Output {output}',
  'chat.bubble.usage.cache': ' · Cache {cacheRead}',
  'chat.bubble.turn-usage.label': 'This turn',
  'chat.bubble.turn-usage.label-unavailable': 'This turn is not reported',
  'chat.bubble.turn-usage.input': 'In {input}',
  'chat.bubble.turn-usage.output': 'Out {output}',
  'chat.bubble.turn-usage.cache': 'Cache {cacheRead}',
  'chat.bubble.turn-usage.cost': 'Cost {cost}',
  'chat.bubble.session-cost': 'Session {cost}',
  'chat.bubble.session-cost.unavailable': 'Session cost not reported',

  // ---------------------------------------------------------------------------
  // The built-in backgrounds (`BACKGROUND_OPTIONS` in `settings/store.ts`) and personas
  // (`persona/registry.ts`). Both name a picture, so they are named after their place; `default` is
  // the plain CSS gradient.
  // ---------------------------------------------------------------------------
  'settings.appearance.background.workspace': 'Late-night studio',
  'settings.appearance.background.deepsea-2': 'Deep-sea dome',
  'settings.appearance.background.deepsea-3': 'Deep-sea study',
  'settings.appearance.background.default': 'Default gradient theme',
  'persona.builtin.blue-child': 'Blue child whale girl',
  'persona.builtin.blue-adult': 'Blue adult whale girl',
  'persona.builtin.black-adult': 'Black-and-red adult whale girl',
  'persona.builtin.black-child': 'Black-and-red child whale girl',

  // What a built-in persona says when its manifest carries no bubble copy (`persona/types.ts`).
  'persona.bubble.morning': 'Good morning! What shall we do today?',
  'persona.bubble.done': 'All done ~ anything else?',
  'persona.bubble.harness-online': 'DeepSeek Harness detected; switch forms?',
  'persona.bubble.harness-offline': 'Harness has gone offline; back to the Web mode.',
  'persona.bubble.chat-open': 'What would you like to talk about?',

  // The four official persona cards (`persona/officialCatalog.ts`) and the two labels on each card
  // (`OfficialPersonaCards.tsx`). `DeepSeek` and `DeepSeek Harness` are product names that read the
  // same in both languages, so they are not entries.
  'persona.official.deepseek.flash.name': 'DeepSeek Flash · Blue child',
  'persona.official.deepseek.pro.name': 'DeepSeek Pro · Blue adult',
  'persona.official.harness.flash.name': 'Harness Flash · Black-and-red child',
  'persona.official.harness.pro.name': 'Harness Pro · Black-and-red adult',
  'persona.official.tier.flash': 'Flash · Child',
  'persona.official.tier.pro': 'Pro · Adult',
  'persona.official.list.label': 'Persona list',
  'persona.official.replaced': 'Replaced: {name}',
  'persona.official.replaced-custom': 'Replaced: custom asset',
  'persona.official.baseline': 'Baseline artwork',

  // ---------------------------------------------------------------------------
  // The connection surface: endpoints, launch arguments, the model directory, and installing the
  // Bridge (`connect/*`).
  // ---------------------------------------------------------------------------
  'connect.endpoint.kind.desktop': 'Desktop client',
  'connect.endpoint.kind.web': 'Web / CLI',
  // What is said when the stored subject is a shell this build no longer supports (two halves).
  'connect.endpoint.unsupported-shell':
    'The previously chosen desktop client is no longer supported: it keeps the local interface '
    + 'behind its own authorisation, so every wallpaper request is refused. Switched back to the official desktop client.',
  // Scan results (`scanSummary`).
  'connect.scan.none': 'Scanned {count} ports; no reachable Harness found',
  'connect.scan.found': 'Scanned {count} ports; found {bridges} reachable Harness instances ({ready} of them usable)',
  // The two ways "open the interface" can come up empty; `{kind}` is one of the names above.
  'connect.raise.no-window': '{kind} has no window to raise; its interface may be in the browser, so use “Open in browser” instead.',
  'connect.raise.not-running': '{kind} is not running. Start it first, then scan again.',
  // Validating 「launch arguments」 and reading the port out of them (`launchArgs.ts`).
  'connect.launch-args.too-many': 'At most {max} launch arguments.',
  'connect.launch-args.too-long': 'One launch argument cannot exceed {max} characters.',
  'connect.launch-args.control-chars': 'Launch arguments cannot contain control characters.',
  // Why the model directory has nothing to offer (`modelDirectory.ts`): shown while the picker is
  // disabled.
  'connect.model.no-options': 'The current Harness offers no selectable model',
  'connect.model.bridge-empty': 'The Bridge did not return a model directory',
  'connect.model.endpoint-empty': 'The endpoint did not return a model directory',
  // The outcome of installing the Bridge (`bridgeInstall.ts`): failure, confirmation, installed.
  'connect.bridge.failed': 'Could not install the Bridge ({profile}): {detail}',
  'connect.bridge.no-detail': 'No further information',
  'connect.bridge.needs-confirmation': 'The Bridge is installed, but a plug-in in {profile} needs you to confirm a version exemption: run dsh plugin allow-version in a terminal (original text: {detail})',
  'connect.bridge.installed': 'Installed the Bridge for {profiles}.',

  // ---------------------------------------------------------------------------
  // Appearance assets (`native/appearance.ts`, `features/appearance/*`).
  // ---------------------------------------------------------------------------
  'appearance.native.desktop-only': 'Appearance storage is only available in the desktop edition',
  'appearance.native.import-desktop-only': 'Importing appearance assets is only available in the desktop edition',
  'appearance.native.classify-desktop-only': 'Classifying assets is only available in the desktop edition',
  // The asset import dialogs: their titles and the two filter names.
  'appearance.import.title': 'Import a theme or standalone assets',
  'appearance.import.filter.content': 'Appearance content',
  'appearance.import.filter.all': 'All files',
  'appearance.import.folder-title': 'Import an asset folder',
  // The asset classification panel (`AssetClassificationPanel.tsx`).
  'appearance.classify.label': 'Asset classification',
  'appearance.classify.title': 'Tidy up standalone assets',
  'appearance.classify.description': 'Choose the assets first, then one or more uses.',
  'appearance.classify.close': 'Close the classification',
  'appearance.classify.slots': 'Available positions',
  'appearance.classify.later': 'Tidy up later',
  'appearance.classify.confirm': 'Confirm the classification',

  // ---------------------------------------------------------------------------
  // What is left in the settings centre and the desktop shell: probe failures, scene alt text, and
  // the accessible names of the drawer and the toast.
  // ---------------------------------------------------------------------------
  // The prefix of a probe failure (`settingsProbes.ts`), followed by `: ` and the raw error.
  'settings.probe.error': '{message}: {error}',
  'settings.probe.managed-dsh': 'Could not read the managed DSH state',
  'settings.probe.web-adapter-config': 'Could not read the Web adapter configuration',
  'settings.probe.autostart-status': 'Could not read the autostart state',
  'settings.probe.desktop-displays': 'Could not read the display list',
  'settings.probe.api-history': 'Could not read the API conversation history',
  'settings.probe.update': 'The update check did not finish',
  // The idle background and the portrait click (sleep and wake are shared with Lite, in `en.shared.ts`).
  'scene.idle.background': 'Background',
  'scene.idle.portrait-title': 'Click to start talking',
  // The floating ball's accessible name and tooltip (`floating/BallWindow.tsx`).
  'ball.open-island': 'Open the AI input island',
  // The drawer's close button and the toast (`ui/primitives/*`).
  'ui.drawer.close': 'Close {title}',
  'ui.toast.close': 'Dismiss the notice',
  'widgets.host.label': 'Desktop widgets',
  // Why a widget manifest was rejected (`widgets/sdk.ts`); the versions are the host's own numbers.
  'widgets.manifest.id-invalid': 'A widget ID may only use lower-case letters, digits, dots, hyphens, or underscores.',
  'widgets.manifest.api-version': 'The widget needs API v{apiVersion}, but this host only supports v{hostVersion}.',
  'widgets.manifest.display-name-required': 'A widget needs a display name.',
  'widgets.manifest.default-size': 'A widget’s default size must be greater than zero.',
  'widgets.manifest.min-size': 'A widget’s minimum size must be greater than zero.',
  'widgets.manifest.min-exceeds-default': 'A widget’s minimum size cannot exceed its default size.',
  'widgets.manifest.max-below-default': 'A widget’s maximum size cannot be smaller than its default size.',
  'widgets.manifest.workspaces': 'A widget must declare which workspaces it can appear in.',

  // ---------------------------------------------------------------------------
  // What is left inside the chat backends: adapter errors, the receipt for a session swap, the
  // preview replies, and the debug ruler and link hint.
  // ---------------------------------------------------------------------------
  'chat.native.archived-notice': 'That session was archived; the message was sent again in today’s new session.',
  'chat.native.blocked-notice': 'DSH refused this turn (the session was archived); the message was sent again in today’s new session.',
  'chat.native.new-session-failed': 'Switching to a new session did not work either: {error}',
  'chat.native.reconnect-failed': '{error} Reconnecting to this endpoint did not work either: {connectError}',
  'chat.native.archived-failed': '{error} Switching to a new session did not work either: {failure}',
  'chat.native.closed': 'The connection is already closed',
  'chat.native.no-new-session': 'The Bridge did not hand out a new session',
  'chat.web.unrecognised': 'The DeepSeek page structure is unrecognised; the Web bridge needs an update.',
  'chat.web.not-connected': 'The DeepSeek Web experimental entry is not connected yet',
  'chat.web.empty-message': 'The message cannot be empty.',
  'chat.web.busy': 'The previous message on the DeepSeek page is still being handled; wait for it to finish or press Stop.',
  'chat.preview.harness-reply': 'Harness is connected. The real desktop app hands this message to a standard DSH session.',
  'chat.preview.browser-reply': 'This is a browser preview reply. The real app connects to the DeepSeek Web bridge or to the API you enabled.',
  // The debug ruler: the difference between two content edges (`LayoutProbe.tsx`, which only
  // appears with those two commented-out lines switched back on).
  'chat.layout-probe.delta': 'Δ {a}→{b}: left {left}  right {right}',
  // A link's title: the address followed by the gesture hint (`MarkdownBody.tsx`).
  'chat.markdown.link-hint': '{href} (middle-click or Enter to open)',

  // ---------------------------------------------------------------------------
  // Update detection (`features/update/*`): the sentences the portrait bubble and the settings system
  // page share. The native side returns codes and numbers only, so every entry here answers one code:
  // `update.outcome.*` are the five conclusions, `update.skip.*` / `update.failure.*` are the reasons
  // nested inside them, and `update.notice.*` are the "that did not work" lines.
  // ---------------------------------------------------------------------------
  'update.outcome.available': 'Version {version} is available',
  // The conclusion says "there is an update" while the report carries no version: native never produces
  // this, but saying so beats printing an empty version number.
  'update.outcome.available-unversioned': 'An update is available, but its version number could not be read',
  'update.outcome.up-to-date': 'Up to date',
  'update.outcome.no-asset': 'Version {version} is out, but this release has no installable package',
  // `{reason}` carries one of the `update.skip.*` / `update.failure.*` sentences below.
  'update.outcome.skipped': 'No check this time: {reason}',
  'update.outcome.failed': 'The check failed: {reason}',
  // The report says it skipped or failed without a reason code (native never produces this): say only
  // that half rather than picking a reason - that would read like a fact.
  'update.outcome.skipped-unstated': 'No check this time',
  'update.outcome.failed-unstated': 'The check failed',
  'update.skip.throttled': 'it has been less than six hours since the last check',
  'update.skip.version-unavailable': 'the installed version cannot be read',
  'update.failure.network': 'the network is unavailable or the request timed out',
  'update.failure.http-status': 'the server answered {status}',
  'update.failure.http-status-unknown': 'the server answered with an error status',
  'update.failure.malformed-response': 'the release information could not be read',
  'update.action.download': 'Download',
  'update.action.dismiss': 'Ignore',
  // With no installable asset in the release, the primary button says what it really does.
  'update.action.release-page': 'Open the release page',
  'update.notice.dismiss-unpersisted': 'This dismissal was not written to the state file; the version will be offered again next launch.',
  'update.notice.dismiss-failed': 'Could not ignore it: {error}',
  'update.notice.check-failed': 'The update check did not finish: {error}',
  'update.notice.open-release-failed': 'Could not open the release page: {error}',
}
