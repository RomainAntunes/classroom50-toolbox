# Accessibility

Classroom 50 Toolbox should work for every student, including people who use a screen reader, the
keyboard alone, magnification, voice control, or who are sensitive to motion. Every action in the
app is meant to work without a mouse, and nothing should be told by color alone.

This page says what works today, what doesn't yet, and how to tell us about a barrier. If you hit
one, skip to [Reporting a barrier](#reporting-a-barrier).

## What the app supports

### Keyboard

- The assignment list is a single Tab stop. Up and Down move between rows, Home and End jump to
  the first and last one, and only the current row's buttons are in the Tab order.
- Enter or Space on a row opens its details. The Menu key or Shift+F10 opens the row's menu (open
  in an editor, clone, pull, submit, copy a link, hide...).
- Escape closes the open prompt, then the details of the focused row, then hides the popover. In a
  search field with text, Escape clears it first.
- The list filters are a radio group: one Tab stop, arrow keys to choose.
- Prompts keep the focus inside them and give it back when they close. Focus rings are visible in
  every theme.
- An optional system-wide shortcut (Settings > General) shows or hides the toolbox from any app and
  puts the focus on the list. It is off by default.

### Screen readers

- The app speaks, through a polite live region, when a sync you started ends or fails, and when
  accept, clone, pull, commit, push, submit or a tool install finishes. It also reads the GitHub
  sign-in code. Failures open a dialog that is read out.
- Each row is a button named by the assignment and described by its status and grade. What a
  color, dot or symbol means (late, uncommitted, new) is spelled out in text for screen readers.
- Icon-only buttons have labels, each class is a list under its own heading, and the sync and
  install bars are progress bars.
- Native menus, dialogs and notifications come from the operating system, so they work with its
  screen reader as other apps do.

### Seeing the screen

- Text meets WCAG 2.2 AA contrast (4.5:1) on every background in every theme, light and dark, and
  focus rings keep 3:1.
- The app follows the system's light or dark setting, or the one you choose in Settings > General.
- It uses the system font of each platform.

### Motion

When the system asks for reduced motion (macOS: Reduce motion; Windows: Animation effects off;
GNOME: Reduce animation), the app stops its animations and transitions, scrolls without
animating, and shows progress bars that stand still.

### Language and reading direction

The app is available in 15 languages, follows the system language or the one you choose, and lays
out right to left for Arabic and Hebrew.

## Supported environments

The toolbox runs on macOS, Windows and Linux, inside the system's own web view: WKWebView on
macOS, WebView2 on Windows and WebKitGTK on Linux. It is meant to work with the screen reader of
each system: VoiceOver on macOS, Narrator, NVDA and JAWS on Windows, and Orca on Linux.

What is checked automatically on every change:

- the list's keyboard rules (which row each key moves to, in both reading directions), by unit
  tests;
- every language in every theme, by UI tests in WebKit that fail on untranslated text and on
  anything wider than the window.

Text contrast is checked by hand when a color changes. The app hasn't had a formal accessibility
audit, and we don't claim conformance with a WCAG level. Screen reader support is tested by hand,
not on every combination of system and screen reader.

## Known limitations

- **The window is a popover with a fixed size.** It closes when it loses focus, like a menu, so
  moving the focus to another app (a magnifier's controls, a dictation window) hides it. Reopen it
  from the tray icon or the optional shortcut. It can't be resized or zoomed, and it doesn't follow
  the system's text size setting: use the system's zoom or magnifier instead.
- **No high contrast theme of its own.** Windows contrast themes and the macOS Increase contrast
  and Reduce transparency settings aren't specifically supported yet. The translucent backgrounds
  (vibrancy on macOS, Mica on Windows) are drawn by the system.
- **The global shortcut isn't available under Wayland** on Linux, because it relies on X11. Use
  the tray icon instead.
- **On Linux, the popover opens in a corner of the screen** rather than under the tray icon,
  since Linux doesn't report where the icon is. On GNOME, tray icons need the AppIndicator
  extension (Ubuntu ships it).
- **Signing in to GitHub happens in your browser**, on GitHub's own device sign-in page. Its
  accessibility is GitHub's.
- **Six translations haven't been reviewed by native speakers** (Japanese, Korean, Turkish,
  Chinese, Arabic and Hebrew), so some wording may be unclear. Screen reader text is translated
  the same way.
- **The first launch on macOS and Windows shows a security warning**, because the installers
  aren't code-signed yet. The steps to get past it are in the [README](../README.md#install).

## Reporting a barrier

Open an [accessibility issue](https://github.com/RomainAntunes/classroom50-toolbox/issues/new?template=accessibility.yml).
If you'd rather not use GitHub, email [romain.antunes@proton.me](mailto:romain.antunes@proton.me).

It helps to say:

- what you were trying to do, and what happened instead;
- your operating system and its version, and the app's version (Settings > About);
- your assistive technology and its version, and the settings you use (reduced motion, larger
  text, a contrast theme...);
- any workaround you found, and a screenshot, recording or screen reader transcript if you can.

Every report gets the `accessibility` label and an answer. A barrier that stops you from doing
something is treated as a bug and fixed before new features. When there is a workaround, we'll
share it in the issue right away.

## How we work on accessibility

Contributors keep these in place when they change the app:

- Every action works with the keyboard, with a visible focus, and follows the list's keys
  (`src/lib/keyboard.ts`).
- Every button and control has a name in the catalogs, and what a color or icon says is spelled out
  in `.sr-only` text.
- Results of an action the user started are announced (`announce`,
  `src/lib/announce.svelte.ts`).
- Colors come from the theme tokens; a new color is checked for 4.5:1 against the card and the
  window in every theme, light and dark (3:1 for focus rings).
- Animations started from script go through `src/lib/motion.ts`, so reduced motion applies.
- Styles use logical properties, so right-to-left languages are mirrored correctly.
- A new or changed UI flow gets an end-to-end scenario in `tests/e2e`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the rest.

## Ownership

This page is maintained by [Romain Antunes](https://github.com/RomainAntunes), who maintains the
app. It is reviewed when a release changes how the app looks or is used, and whenever a report
shows it's out of date: if something here is wrong or missing, please
[open an issue](https://github.com/RomainAntunes/classroom50-toolbox/issues/new?template=accessibility.yml).

Last reviewed: 16 October 2026.
