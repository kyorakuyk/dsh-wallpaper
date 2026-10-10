import type { SharedDict } from './zh.shared.ts'

/**
 * English entries — **the half both editions need** (see `zh.shared.ts` for why the dictionary is split).
 *
 * Typed as `SharedDict`, so a key added to `zh.shared.ts` without an English sentence over here fails
 * `pnpm typecheck` instead of leaving a Chinese sentence in an English window.
 */
export const enShared: SharedDict = {
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

  // ---------------------------------------------------------------------------
  // The Lite settings window (`lite/LiteSettingsWindow.tsx`): the line under the brand, three
  // cards, the option tiles, and the notice bar. `Wallpaper Lite`, `DSH`, and `Windows 11` are
  // product names and read the same in both languages, so they are not entries. The frozen cards
  // (lock screen, sign-in wallpaper, TranslucentTB) now live in the repository archive and take no entries.
  // ---------------------------------------------------------------------------
  'lite.settings.brand.tagline': 'A lightweight desktop wallpaper',
  'lite.settings.window.close': 'Close settings',
  'lite.settings.notice.close': 'Dismiss the notice',
  'lite.settings.hero.title': 'Let the desktop wake up quietly',
  'lite.settings.hero.description': 'Only the lock screen, the wake animation, the wallpaper, and the portrait. The Windows password page stays with Windows.',
  'lite.settings.system.title': 'Lock screen and startup',
  'lite.settings.autostart.title': 'Start at sign-in',
  'lite.settings.autostart.busy': 'Updating the startup task.',
  'lite.settings.wake.title': 'Wake animation',
  'lite.settings.wake.enabled.title': 'Play the wake animation',
  'lite.settings.wake.enabled.detail': 'Plays the four official frames after an unlock.',
  'lite.settings.wake.every-unlock.title': 'Play on every unlock',
  'lite.settings.wake.every-unlock.detail': 'With this off it plays once, when the app starts.',
  'lite.settings.wake.skip.title': 'Skip the animation',
  'lite.settings.wake.skip.detail': 'Go straight to the static wallpaper and portrait.',
  'lite.settings.wake.speed.title': 'Animation speed',
  'lite.settings.scene.title': 'Wallpaper and portrait',
  'lite.settings.scene.background': 'Wallpaper background',
  'lite.settings.scene.portrait': 'Portrait on the right',
  'lite.settings.scene.custom-background': 'Custom background · choose a file',
  'lite.settings.scene.custom-portrait': 'Custom portrait · choose a file',
  'lite.settings.scene.current': 'Current',
  'lite.settings.scene.footnote': 'The first release uses the official built-in assets by default, and you can import one background and one portrait of your own. Theme packages, plug-ins, and per-slot replacement arrive in the full edition.',
  'lite.settings.footer.autosave': 'Settings save automatically',
  // What the notice bar says (failures carry `{error}`; a successful import has one per slot).
  'lite.settings.save-failed': 'Could not save the settings: {error}',
  'lite.settings.load-failed': 'Could not read the local settings: {error}',
  'lite.settings.autostart.read-failed': 'Could not read the autostart state: {error}',
  'lite.settings.autostart.update-failed': 'Could not update autostart: {error}',
  'lite.settings.custom-image.read-failed': 'Could not read your custom assets: {error}',
  'lite.settings.custom-image.import-failed': 'Could not import the image: {error}',
  'lite.settings.custom-image.background-imported': 'Imported the custom wallpaper background.',
  'lite.settings.custom-image.portrait-imported': 'Imported the custom portrait.',

  // ---------------------------------------------------------------------------
  // Lite's wallpaper and portrait candidates (`lite/settings.ts`, `lite/persona.ts`): the words on
  // the option tiles. The ids are internal identifiers and only the label is an entry; the `custom`
  // tile is named by the asset the user imported, so it has no name of its own.
  // ---------------------------------------------------------------------------
  'lite.option.background.workspace': 'Late-night studio',
  'lite.option.background.deepsea-2': 'Deep-sea dome',
  'lite.option.background.deepsea-3': 'Deep-sea study',
  'lite.option.portrait.blue-adult': 'Blue adult form',
  'lite.option.portrait.blue-child': 'Blue child form',
  'lite.option.portrait.black-adult': 'Black-and-red adult form',
  'lite.option.portrait.black-child': 'Black-and-red child form',
  // Lite's image picker dialogs (`lite/native.ts`).
  'lite.native.pick.background': 'Choose a wallpaper background',
  'lite.native.pick.portrait': 'Choose a portrait',
  'lite.native.image-filter': 'Images',
  // The two scene accessible names Lite draws as well (sleep and wake; the idle background and the
  // portrait click are full-edition only, so those two live in `en.full.ts`).
  'scene.sleep.alt': 'The whale girl asleep',
  'scene.sleep.hint': 'Press Esc or type your password to wake up',
  'scene.wake.frame': 'Waking {index}',
  'scene.wake.alt': 'The whale girl waking up',
  'lite.scene.background.alt': 'Wallpaper background',
}
