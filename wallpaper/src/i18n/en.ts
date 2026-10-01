import type { Dict } from './zh'

/**
 * English entries.
 *
 * Written, not machine-translated: the point of this file is that an English reader gets English,
 * not a word-for-word shadow of the Chinese. Keys must match `zh.ts` exactly - the `Dict` type sees
 * to that, so a missing or extra key fails `pnpm typecheck` rather than showing up as a stray
 * Chinese string in the middle of an English window.
 */
export const en: Dict = {
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

  // Windows autostart: which path carries it, and what to say when a change did not take effect.
  'autostart.detail.startup-task': 'Windows starts this app at sign-in through its startup task.',
  'autostart.detail.startup-task.disabled': 'The Windows startup task is not enabled.',
  'autostart.detail.run': 'This app starts at sign-in from your own startup entry.',
  'autostart.detail.disabled-by-user': 'Windows has autostart turned off for this app.',
  'autostart.detail.disabled-by-policy': 'A Windows policy keeps this app from starting at sign-in.',
  'autostart.detail.unsupported': 'This version of Windows cannot start this app at sign-in.',
  'autostart.detail.reading': 'Reading the startup state from Windows…',
  // The direction the user asked for, filled into the two sentences below.
  'autostart.refusal.wanted-on': 'turned on',
  'autostart.refusal.wanted-off': 'turned off',
  'autostart.refusal.with-reason': 'Autostart was not {wanted}: {reason}',
  'autostart.refusal.generic': 'Autostart was not {wanted}; Windows did not accept the change, so check this app under "Startup apps" in Windows settings.',

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
}
