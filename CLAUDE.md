# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A tray app for [Classroom 50](https://github.com/foundation50/classroom50) students, in the spirit of JetBrains Toolbox. It runs from the macOS menu bar, the Windows notification area and the Linux tray. It:

1. Installs prerequisites: git, GitHub CLI (`gh`), and the `gh student` extension (`foundation50/gh-student`).
2. Discovers and tracks Classroom 50 assignment repos: due dates, submission state, grades.
3. Runs student actions: accept, clone, submit, and open in an editor, terminal or file manager.

The first release is for students only. Keep the domain layer role-agnostic so a teacher mode (`gh teacher`) can be added later without a rewrite.

- Repo: `github.com/RomainAntunes/classroom50-toolbox`. License: GPL-3.0, the same as Classroom 50.
- Status: pre-scaffold. Only this file exists so far. Update the Commands section once the project has been generated.

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
- **`assignments.json`** (schema `assignments-v1`) is the published list of a classroom's assignments. It holds due dates, release dates (`--available-from`), submission mode and group settings. A due date only marks work as **late** and never blocks it. Only the teacher's "Close submission" makes a repo read-only, so don't show a deadline as "locked".
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

## Commands (planned once scaffolded; package manager: pnpm)

```sh
pnpm install
pnpm tauri dev                     # run the app with hot reload
pnpm tauri build                   # local release build for the current OS
pnpm check                         # svelte-check + TypeScript
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml                 # all Rust tests
cargo test --manifest-path src-tauri/Cargo.toml <test_name>     # single test
```

## Conventions

- Keep platform-specific code (package managers, tray behavior, autostart, opening editors) behind a small Rust trait with one implementation per OS (`cfg(target_os)`). The UI never branches on OS for behavior, only for theming.
- Every call to `gh` or `git` goes through one Rust process wrapper that captures stderr and exit codes and returns typed errors. Never build shell strings; pass arguments as a vector.
- Rust owns all state and I/O. Svelte only renders and invokes typed Tauri commands, and receives events for progress and refreshes.
