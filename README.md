# Classroom 50 Toolbox

A tray app for [Classroom 50](https://github.com/foundation50/classroom50) students on macOS,
Windows and Linux, in the spirit of JetBrains Toolbox. It installs what Classroom 50 needs, finds
your assignments, and keeps due dates, submissions and grades one click away in the menu bar or
notification area.

> Early development. Not affiliated with the Fifty Foundation.

## What it does

- **Sets up your machine.** Checks for git, the GitHub CLI (`gh`) and the `gh student`
  extension, installs whichever is missing through your package manager (Homebrew, winget,
  apt/dnf/pacman), and updates `gh student` when a new version is out.
- **Signs you in** to GitHub through `gh`, with a one-time code in your browser. Several
  accounts are supported; the app never keeps its own copy of your token.
- **Lists your assignments**, from GitHub and from the folders on your disk (including the
  projects your editor opened recently: JetBrains IDEs, VS Code and its forks such as Cursor,
  Zed, Sublime Text, Eclipse, NetBeans or Visual Studio): due dates, late work, uncommitted or
  unsubmitted changes, branches not merged yet, grades with the tests that failed, the
  autograder running or failing, and teacher feedback.
- **Shows your group** on team assignments, with who is in it. Search finds assignments by
  teammate too.
- **Shows the assignments you haven't accepted yet**, and accepts them for you.
- **Clones, pulls and submits** your work, and opens a project in the editor you last used for
  it, your terminal, your file manager or on GitHub.
- **Notifies you** before a deadline, when a grade or feedback arrives, when a new assignment is
  published, when a teammate pushes, and when a classmate joins your group.
- Starts at login, hidden in the tray, and updates itself.

## Install

Download the installer for your platform from the
[latest release](https://github.com/RomainAntunes/classroom50-toolbox/releases/latest):

| Platform              | File                                                          |
| --------------------- | ------------------------------------------------------------- |
| macOS (Apple Silicon) | `_aarch64.dmg`                                                |
| macOS (Intel)         | `_x64.dmg`                                                    |
| Windows               | `_x64-setup.exe` (or the `.msi`)                              |
| Linux                 | `.AppImage`, `.deb` (Debian, Ubuntu) or `.rpm` (Fedora, RHEL) |

Builds aren't code-signed yet, so the first launch needs one extra step:

- **macOS:** open the app once, then go to System Settings › Privacy & Security and click
  **Open Anyway**.
- **Windows:** SmartScreen shows "Windows protected your PC": click **More info › Run anyway**.
- **Linux on GNOME:** tray icons need the AppIndicator extension (Ubuntu ships it).

After that the app updates itself.

## Getting started

1. Click the tray icon. If git, `gh` or `gh student` is missing, install it from the Setup
   section.
2. Sign in with GitHub. Use the account your classes know you by.
3. Your classrooms and assignments appear. Accept one, clone it, and the row follows its state
   from then on. Projects you already cloned are found in the folders your editor opened
   recently; add other folders in Settings.

## Privacy

The app talks to GitHub only, with the token `gh` already holds, and never sends data anywhere
else. It reads your repositories and never changes them on its own: accepting, cloning, pulling
and submitting only happen when you click.

## Support the project

The toolbox is free and built by a student in their spare time. Its first goal is to pay for
code signing, so students no longer have to get past a security warning on first launch: the
Apple Developer Program costs 99 USD a year. If the toolbox helps you or your class,
[sponsoring it on GitHub](https://github.com/sponsors/RomainAntunes) gets it there faster.
Starring the repo and telling other students and teachers about it helps too.

## Develop

Requirements: [Rust](https://rustup.rs), Node.js LTS, pnpm, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
pnpm install
pnpm tauri dev      # run the app
pnpm lint           # prettier + svelte-check
cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings
```

Development builds never write to GitHub: accepting, submitting and pushing are refused, so you
can run them against your real classes safely.

## License

[GPL-3.0](LICENSE), like Classroom 50 itself.
