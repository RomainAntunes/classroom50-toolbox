# Classroom 50 Toolbox

A tray app for [Classroom 50](https://github.com/foundation50/classroom50) students on macOS,
Windows and Linux. It installs what Classroom 50 needs (git, the GitHub CLI and the
`gh student` extension), finds your assignment repositories, and keeps their due dates,
submissions and grades one click away in the menu bar or notification area.

> Early development. Not affiliated with the Fifty Foundation.

## Install

Download the installer for your platform from the
[latest release](https://github.com/RomainAntunes/classroom50-toolbox/releases/latest). The app
updates itself from there.

Builds aren't code-signed yet: macOS asks you to confirm the first launch (right-click › Open),
and Windows SmartScreen shows "More info › Run anyway".

## Develop

Requirements: [Rust](https://rustup.rs), Node.js LTS, pnpm, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
pnpm install
pnpm tauri dev
```

## License

[GPL-3.0](LICENSE), like Classroom 50 itself.
