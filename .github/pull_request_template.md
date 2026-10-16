<!--
Thanks for contributing! CONTRIBUTING.md has the setup and conventions:
https://github.com/RomainAntunes/classroom50-toolbox/blob/main/.github/CONTRIBUTING.md
-->

## What changes

<!-- What this pull request does, for students or for the code. Link the issue it closes. -->

Closes #

## How it was tested

<!-- Which flows you ran by hand, on which OS, and which tests you added. -->

## Screenshots

<!-- For a visible change: before and after, light and dark. Delete this section otherwise. -->

## Checklist

- [ ] `pnpm lint` and `pnpm test` pass.
- [ ] `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass in `src-tauri`.
- [ ] A new or changed UI flow has a scenario in `tests/e2e`, and `pnpm test:e2e` passes.
- [ ] New strings are in `src/lib/i18n/en.json`, and every catalog has the same keys.
- [ ] It works with the keyboard alone and names what it shows for screen readers
      ([ACCESSIBILITY.md](https://github.com/RomainAntunes/classroom50-toolbox/blob/main/.github/ACCESSIBILITY.md)).
- [ ] Nothing writes to GitHub or to a clone without the student's click, and the development
      build guard is untouched.
- [ ] Commits follow `type: summary` (`feat`, `fix`, `perf`, `docs`, `test`, `chore`...).
