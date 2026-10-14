# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A tray app for [Classroom 50](https://github.com/foundation50/classroom50) students, in the spirit of JetBrains Toolbox. It runs from the macOS menu bar, the Windows notification area and the Linux tray. It:

1. Installs prerequisites: git, GitHub CLI (`gh`), and the `gh student` extension (`foundation50/gh-student`).
2. Discovers and tracks Classroom 50 assignment repos: due dates, submission state, grades.
3. Runs student actions: accept, clone, submit, and open in an editor, terminal or file manager.

The first release is for students only. Keep the domain layer role-agnostic so a teacher mode (`gh teacher`) can be added later without a rewrite.

- Repo: `github.com/RomainAntunes/classroom50-toolbox`. License: GPL-3.0, the same as Classroom 50.
- Status: base scaffold. Tray, popover, tool detection and install, local scan and the Pages manifest fetch are done. Not yet built: the GitHub API client, deadline tracking and notifications, grades, accept and submit, settings persistence, and the login flow.

## Stack and decisions (settled, don't re-litigate)

- **Tauri v2.** The Rust core lives in `src-tauri/`, and the UI is **Svelte 5 + TypeScript** (Vite) in `src/`. Performance matters: keep the webview bundle small and do the work in Rust, not JS.
- **Native look per OS through theming, not a cross-platform skin.** macOS gets vibrancy/blur and SF system fonts, Windows 11 gets Mica plus Fluent spacing and controls, and Linux follows GNOME/libadwaita (and respects KDE). Choose the theme from the platform at runtime and follow the system light/dark setting. The main surface is a popover-style window anchored to the tray icon, not a normal app window.
- **Auth reuses gh.** Run `gh student login` (which requests `admin:org read:org repo workflow`), then read the token with `gh auth token`. There is no separate OAuth app, and the app never stores its own token copy.
- **Installs go through the OS package manager.** That means Homebrew (macOS), winget (Windows) and apt/dnf/pacman (Linux), followed by `gh extension install foundation50/gh-student`. Upgrades use `gh extension upgrade gh-student`. Elevation (sudo/UAC) must be explicit and user-initiated.
- **Alerts:** native OS notifications (approaching due dates, a new grade), a tray icon state or badge (something due soon or failing tests), and autostart at login (launch hidden).
- **Release:** a GitHub Actions matrix builds dmg, msi/nsis, AppImage, deb and rpm on a version tag and publishes them to GitHub Releases. The Tauri updater reads `https://github.com/RomainAntunes/classroom50-toolbox/releases/latest/download/latest.json`. Updater artifacts are signed with the Tauri key (kept in CI secrets). OS code signing (Apple notarization, Authenticode) is deferred.
- **i18n:** English only, but every user-facing string goes through the i18n layer from the start.

## Classroom 50 contracts the app depends on

Classroom 50 has no server. All state lives in GitHub, so the app reads GitHub directly. Upstream publishes JSON Schemas in `schemas/` that are meant for GUI clients like this one. Validate against them instead of guessing field names.

- **`.classroom50.yaml`** (schema `repo-config-v1`) sits at the root of every accepted repo. Its required keys are `classroom` and `assignment`, and it may also hold `secret` (the unlisted-classroom key). Its presence is what makes a repo a Classroom 50 project.
- **`assignments.json`** (schema `assignments-v1`) is the published list of a classroom's assignments. It's public and needs no token: `https://<org>.github.io/classroom50/<classroom>[/<secret>]/assignments.json` (see `classroom50::assignments_url`). It holds due dates, release dates (`--available-from`), submission mode and group settings. A due date only marks work as **late** and never blocks it. Only the teacher's "Close submission" makes a repo read-only, so don't show a deadline as "locked".
- **Submissions:** depending on the assignment's mode, either every push to the default branch counts or only `submit/<UTC-timestamp>-<short-sha>` tags do (pushed by `gh student submit`). A teacher-named milestone tag pattern may also grade.
- **Grades:** each graded submission produces a **GitHub Release** on the student repo containing `result.json` (schema `result-v1`: `score`, `max-score`, `tests`, `commit`, `datetime`, ...). An optional feedback PR is opened at accept.
- **Group assignments** are GitHub Teams named `classroom50-group-<hash>-<n>`, owning `<classroom>-<assignment>-group-<n>`.
- Student CLI surface: `gh student accept <org> <classroom> <assignment> [--key]`, `submit [--key]`, `invite`, `team list|add`, `whoami`. Shell out to these for mutations instead of reimplementing accept or submit logic, which has many steps and changes upstream. Reading over the GitHub REST API is fine.
- Upstream reference: the wiki at `github.com/foundation50/classroom50/wiki`, especially `gh-student`, `How-Classroom-50-Works`, `Glossary` and `Autograding-Basics`.

## Project discovery (three sources, merged and de-duplicated by `owner/repo`)

1. **GitHub API:** the user's org repos that contain `.classroom50.yaml`.
2. **Local scan:** user-chosen folders, watched for clones containing `.classroom50.yaml`. This gives local state (uncommitted changes, commits not yet submitted).
3. **Clones made by the app** (accept or clone), registered when they're created. A plain `git clone` is picked up by source 2 once the folder is watched.

Remote data (due dates, releases) is cached locally. Refresh it on a timer and on tray open, and respect GitHub rate limits through conditional requests (ETags).

## Commands

```sh
pnpm install
pnpm tauri dev                     # run the app (Vite on :1420 + cargo run)
pnpm tauri build                   # release bundles for the current OS
pnpm lint                          # prettier --check + svelte-check
pnpm format                        # prettier --write
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test                       # all Rust tests
cd src-tauri && cargo test parses_github_remotes  # single test (name filter)
```

`cargo build` and `cargo clippy` embed the built frontend through `generate_context!`, so run `pnpm build` once on a fresh checkout before using cargo directly.

The dev build is tray-only (no Dock icon, starts hidden). Open it from the tray icon, or launch the binary a second time: the single-instance plugin shows the popover.

## Layout

- `src-tauri/src/lib.rs` wires the plugins, managed state, tray and popover setup, and the command list.
- `commands.rs` is the IPC surface. Each command has a typed twin in `src/lib/api.ts`, and the two must stay in sync because Rust serializes with camelCase.
- `error.rs` holds the one `Error` enum, which crosses IPC as `{ kind, message }`.
- `process.rs` is the only place that spawns programs. It adds the usual install dirs to PATH, because a Finder-launched app has no `/opt/homebrew/bin`. It also sets `GH_PROMPT_DISABLED` and hides console windows on Windows.
- `platform/{macos,windows,linux}.rs` implement the same functions (install command, extra PATH dirs, open terminal) and are selected with `cfg(target_os)`.
- `tools.rs` detects and installs git, gh and gh-student, and reads gh's auth.
- `classroom50.rs` holds the typed upstream contracts and Pages URLs. `discovery.rs` is the local scanner.
- `tray.rs`, `popover.rs` and `updater.rs` handle the tray icon and menu, the anchored window (hide on blur, vibrancy or Mica) and self-update.
- `src/lib/theme/theme.css` holds the semantic tokens plus a skin per `data-os`. Components use tokens only.
- `src/lib/i18n` is a flat `en.json` read through `t(key, params)`. New strings go there.
- The updater public key is in `tauri.conf.json`. The private key and its password are in `~/.tauri/classroom50-toolbox.key{,.password}` locally and in the CI secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Losing them strands existing installs on their current version.
- Release: bump `version` in `package.json` (`tauri.conf.json` and the updater read it from there; also bump `Cargo.toml`), then push a `vX.Y.Z` tag. CI creates a draft release with `latest.json`, and publishing the draft ships the update.

## Conventions

- Keep platform-specific code (package managers, tray behavior, autostart, opening editors) in `platform/`, one file per OS exposing the same functions. The UI never branches on OS for behavior, only for theming.
- Every call to `gh` or `git` goes through one Rust process wrapper that captures stderr and exit codes and returns typed errors. Never build shell strings; pass arguments as a vector.
- Rust owns all state and I/O. Svelte only renders and invokes typed Tauri commands, and receives events for progress and refreshes. (The watched-folder list is in `localStorage` for now and should move to Rust with the settings work.)
- Tauri permissions are allow-listed in `src-tauri/capabilities/default.json`. A plugin API called from JS needs its permission added there.
- Don't use em-dashes in code, comments or UI strings.
