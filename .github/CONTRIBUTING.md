# Contributing to Classroom 50 Toolbox

Thanks for helping! Bug reports, translation fixes and code are all welcome. Everyone taking part
follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

The toolbox is for **students**. Teacher and TA tooling (creating classrooms, grading, closing
submissions) stays in the Classroom 50 web app and `gh teacher`, so ideas in that direction are
out of scope here.

Some problems belong upstream rather than here:

- `gh student` itself failing the same way in a terminal, or Classroom 50's web app:
  [foundation50/classroom50](https://github.com/foundation50/classroom50/issues).
- The GitHub CLI: [cli/cli](https://github.com/cli/cli/issues).

Security problems never go in a public issue: see the [security policy](SECURITY.md).

## Reporting a bug

Open a [bug report](https://github.com/RomainAntunes/classroom50-toolbox/issues/new?template=bug_report.yml).
The most useful thing you can attach is the app's diagnostics: **Settings > About > Copy
diagnostics** copies a report with your version, OS, tools and recent log lines, where accounts,
classes, repositories, emails and your home folder are already replaced with placeholders. Read
it before you paste it.

Barriers for people using a screen reader, the keyboard alone, magnification or reduced motion
have their own [accessibility form](https://github.com/RomainAntunes/classroom50-toolbox/issues/new?template=accessibility.yml).
[ACCESSIBILITY.md](ACCESSIBILITY.md) says what the app supports today.

## Suggesting a feature

Open a [feature request](https://github.com/RomainAntunes/classroom50-toolbox/issues/new?template=feature_request.yml)
describing the problem first, then the change you have in mind. For anything big, please wait for
a reply before you write code, so your time isn't spent on something that can't be merged.

## Improving a translation

Each language is one flat JSON file in [`src/lib/i18n/`](../src/lib/i18n). The Japanese, Korean,
Turkish, Chinese, Arabic and Hebrew catalogs were made with AI and haven't been reviewed by native
speakers yet, so corrections there are especially welcome.

- To fix wording, edit the value in your language's file and open a pull request. If you'd rather
  not touch the code, open a [translation issue](https://github.com/RomainAntunes/classroom50-toolbox/issues/new?template=translation.yml)
  quoting the text and what it should say.
- Keep GitHub's own words and the git verbs (commit, push, pull) as students see them in their
  tools, and use the word schools use for handing in work for Submit.
- Keep every `{placeholder}` as it is. A string with a count has one key per plural category
  (`.one`, `.other`, plus `.few` or `.many` where your language has them).
- The catalogs use a fixed form of address: `vous` (fr), `tú` (es), `du` (de), `tu` (it, pt-PT),
  `você` (pt-BR), `je` (nl), `ty` (pl), `siz` (tr), です/ます (ja), 해요체 (ko), 你 (zh-Hans).
  Arabic and Hebrew never gender the student.
- `pnpm test` checks that every catalog has exactly the keys, plural forms and placeholders of
  `en.json`.

Adding a new language is one catalog file, one entry in `catalogs` in
[`src/lib/i18n/index.ts`](../src/lib/i18n/index.ts), its code in `CFBundleLocalizations` in
`src-tauri/Info.plist` and a `src-tauri/infoplist/<code>.lproj/InfoPlist.strings`. Open an issue
first so we can agree on the form of address.

## Setting up

You need [Rust](https://rustup.rs), Node.js LTS, [pnpm](https://pnpm.io) and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
pnpm install
pnpm tauri dev     # run the app: Vite on :1420 plus the Rust core
```

The development build lives in the tray only (no Dock icon) and starts hidden: open it from the
tray icon. `cargo` embeds the built frontend, so run `pnpm build` once before using `cargo`
directly on a fresh checkout.

**Development builds never write to GitHub.** Accepting, submitting, inviting and pushing are
refused in debug builds, so you can run the app against your real classes safely. Please don't
weaken or work around that guard, and don't run those commands by hand while testing: test write
paths with unit tests or a throwaway repository.

## Making a change

The [CLAUDE.md](../CLAUDE.md) file at the repository root describes the architecture and the
upstream contracts in detail. The short version:

- The Rust core in `src-tauri/` owns all state and I/O. The Svelte UI in `src/` only renders and
  calls typed Tauri commands; each command in `commands/` has a twin in `src/lib/api.ts`.
- Every `gh` or `git` call goes through `process.rs`, with arguments as a vector, never a shell
  string. Mutations shell out to `gh student` instead of reimplementing it.
- OS-specific behavior lives in `src-tauri/src/platform/`, one file per OS with the same
  functions. The UI only branches on the OS for theming.
- Every user-facing string goes through `t()` and lives in `src/lib/i18n/en.json`. Add new keys to
  every catalog (English text is fine as a placeholder in a pull request; say so and it will be
  translated before release).
- Styles use the theme tokens and logical properties (`margin-inline-start`, not `margin-left`),
  so right-to-left languages lay out correctly.
- Every action works with the keyboard, and what a color or icon says is spelled out for screen
  readers. See "How we work on accessibility" in [ACCESSIBILITY.md](ACCESSIBILITY.md).
- No em-dashes in code, comments or UI strings.
- A new Tauri plugin API called from JS needs its permission in
  `src-tauri/capabilities/default.json`.

## Testing

Run these before you open a pull request; CI runs them on macOS, Windows and Linux:

```sh
pnpm lint                 # prettier --check and svelte-check
pnpm test                 # vitest
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

For a UI change, also run the UI flows in WebKit with Rust faked:

```sh
pnpm exec playwright install webkit   # once
pnpm test:e2e
```

A new UI flow, or a change to one, gets a scenario in `tests/e2e` that checks both what is shown
and what was or wasn't invoked (a cancel invokes no write). `pnpm e2e:shots` takes screenshots of
every skin, light and dark, which helps for visual changes.

## Commits and pull requests

- Work on a branch, and keep each commit to one concern.
- Commit messages are one line in the `type: summary` form: `feat`, `fix`, `perf`, `docs`,
  `test`, `refactor`, `chore` or `ci`, for example `fix: keep the clone folder after a cancel`.
  Release notes are drafted from the `feat`, `fix` and `perf` subjects, so write them for students.
- Fill in the pull request template: what changes, how you tested it, and screenshots for anything
  visible.
- Don't bump the version or edit `src/lib/changelog/`: the maintainer does that when preparing a
  release.

## License

The project is licensed under [GPL-3.0](../LICENSE), like Classroom 50. By contributing, you agree
that your contributions are licensed under the same terms.
