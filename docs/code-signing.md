# Code signing

**Status: on hold.** Releases ship unsigned for now. Signing costs money (macOS) or needs a
track record the project doesn't have yet (Windows, free route), so it waits until the toolbox
has more users or funding. Everything below is ready to switch on when that happens.

Until then, students see one warning on first launch, explained in the README and on the
download page: macOS asks for Privacy & Security › Open Anyway, and Windows SmartScreen shows
More info › Run anyway.

The updater is not affected: update artifacts are already signed with the Tauri key
(`TAURI_SIGNING_PRIVATE_KEY`), which is a separate thing from OS code signing.

## macOS: Developer ID and notarization

There is no free option: a Developer ID certificate needs the
[Apple Developer Program](https://developer.apple.com/programs/) (99 USD a year).

1. Enroll in the Apple Developer Program.
2. In Xcode or on developer.apple.com, create a **Developer ID Application** certificate. Export
   it from Keychain Access as a `.p12` with a password.
3. Create an app-specific password at [account.apple.com](https://account.apple.com) for
   notarization, and note the Team ID from the membership page.
4. Add these repository secrets (Settings › Secrets and variables › Actions):

   | Secret                       | Value                                                            |
   | ---------------------------- | ---------------------------------------------------------------- |
   | `APPLE_CERTIFICATE`          | `base64 -i certificate.p12` output                               |
   | `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password                                              |
   | `APPLE_SIGNING_IDENTITY`     | `Developer ID Application: <Name> (<TEAMID>)`                    |
   | `APPLE_ID`                   | the Apple account email                                          |
   | `APPLE_PASSWORD`             | the app-specific password                                        |
   | `APPLE_TEAM_ID`              | the Team ID                                                      |
   | `KEYCHAIN_PASSWORD`          | any random string (the temporary keychain on the runner uses it) |

5. Uncomment the macOS lines in `.github/workflows/release.yml` (marked "Code signing").
   Tauri signs and notarizes during `tauri build` when these variables are set.
6. Remove the macOS first-launch step from the README and `site/index.html`.

Notarization can also use an App Store Connect API key (`APPLE_API_ISSUER`, `APPLE_API_KEY`,
`APPLE_API_KEY_PATH`) instead of the Apple ID and password.

## Windows

### Free: SignPath Foundation (preferred)

[SignPath Foundation](https://signpath.org) signs open source projects for free. The
certificate is issued to SignPath Foundation, which appears as the publisher, with the project
named in the signature. Keys stay on SignPath's HSMs, so no signing key lives in this repo or
in CI; the workflow only holds an API token that can submit requests, and each request still
needs a manual approval in SignPath.

#### What SignPath requires (checked 2026-10-18)

From the [conditions](https://signpath.org/terms.html) and the
[GitHub trusted build system docs](https://docs.signpath.io/trusted-build-systems/github):

- An OSI-approved license with no commercial dual licensing, and no proprietary components
  (system libraries are fine). GPL-3.0-only, all dependencies open source: met (cargo-deny's
  license check in CI keeps it that way).
- Actively maintained, already released in the form that gets signed, and its functionality
  described on its download page: met (releases since v0.1, the download page).
- Only binaries built from the project's own repository, by the team that maintains it: met.
- **A verifiable reputation.** There is no written threshold; SignPath has asked other projects
  for public references (articles, community posts) when stars and downloads weren't enough.
  On 2026-10-18 the repo has 0 stars and 27 installer downloads over all releases, so this is
  the one condition not met yet.
- **A code signing policy** on the project's home page (here: the README and the download
  page) with the attribution line, the team's roles and a privacy statement. A draft is below.
- **Roles**: authors (may change the code without review), reviewers (review every change from
  someone else) and approvers (approve each signing request).
- **Multi-factor authentication** for every team member, on GitHub and on SignPath.
- **Metadata on every signed file**: product name and a version consistent across the build,
  enforced by the artifact configuration below.
- **Origin verification**: the signing request comes from a GitHub Actions run, the unsigned
  files are a workflow artifact (`actions/upload-artifact`) of that run, and for open source
  projects every job of the workflow up to the request runs on GitHub-hosted runners. SignPath
  reads the repository, branch and commit from GitHub, not from the build, and the release
  signing policy can be limited to `main`.

#### How it fits Tauri's updater

The release build already signs each updater artifact with the Tauri key: `tauri build` writes
`<installer>.sig` over the installer's bytes, and tauri-action copies it into `latest.json`.
With `createUpdaterArtifacts: true` (not `v1Compatible`) the Windows updater artifacts are the
NSIS `-setup.exe` and the `.msi` themselves. Authenticode embeds its signature in the file, so
an installer signed after the build no longer matches its `.sig`, and every installed app
would refuse that update. There are two ways around it:

1. **Sign inside the build** with `bundle > windows > signCommand`. Tauri then signs the app's
   exe after writing the bundle type into it, then the installer, and only then the updater
   `.sig`, so the order is right by construction. This is the way for a certificate the runner
   can use directly (Azure Artifact Signing, a cloud HSM). It doesn't work with SignPath
   Foundation: its origin verification takes the file from a workflow artifact through its
   GitHub action, which a command run by the bundler can't do.
2. **Sign after the build, then sign the updater artifacts again.** This is the plan:
   1. `build` runs as today: tauri-action uploads the unsigned installers, their `.sig` and
      `latest.json` to the draft.
   2. `updater-urls` runs as today, turning `latest.json`'s links into links by asset name.
   3. A new `sign-windows` job (in `release.yml`, commented out) downloads the draft's
      `-setup.exe` and `.msi` files (x64 and Arm64), uploads them as one workflow artifact,
      submits it with `signpath/github-action-submit-signing-request` and waits for the
      approval. Then, for each signed installer, it runs
      `tauri signer sign --app-version <version>` with the updater key (passing the version as
      `tauri build` does), replaces the asset and its `.sig` in the draft, and writes the new
      signature into every `latest.json` entry whose link ends with that file name, failing
      if one has none. It runs after `updater-urls` because replacing an asset gives it a new
      API id, which the links `updater-urls` rewrites are keyed by.

   What this leaves unsigned: the app's own exe inside the installers. SignPath can't open an
   NSIS installer to sign what's inside, and signing the exe before bundling doesn't survive
   either: the bundler writes the bundle type into the exe for each installer
   (`__TAURI_BUNDLE_TYPE_VAR_NSS` or `_MSI`, which `updater.rs` reads to know how to update),
   which breaks a signature made earlier, and `--no-binary-patching` would keep the signature
   but lose the bundle type, so the app would stop updating. SmartScreen judges the file a
   student downloads, which is the installer, so this is enough to remove the warning. Signing
   the exe too would mean a SignPath round per installer type before bundling: write the
   bundle type into a copy of the exe, sign it, then `tauri bundle --bundles <type>
--no-binary-patching` with it in place, sign the installer, and sign it for the updater.

#### Artifact configuration (SignPath project settings, slug `windows-installers`)

```xml
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" required="true" />
  </parameters>
  <zip-file>
    <pe-file path="Classroom.50.Toolbox_*-setup.exe" max-matches="unbounded"
             product-name="Classroom 50 Toolbox" product-version="${version}">
      <authenticode-sign />
    </pe-file>
    <msi-file path="Classroom.50.Toolbox_*.msi" min-matches="0" max-matches="unbounded"
              product-name="Classroom 50 Toolbox">
      <authenticode-sign />
    </msi-file>
  </zip-file>
</artifact-configuration>
```

The `zip-file` root is the archive `actions/upload-artifact` makes. Check the restrictions
against the first test-signing run: the NSIS installer's `ProductVersion` is the app version
(`0.12.0`, `0.12.0-beta.2`), while an MSI's `ProductVersion` is numeric only, which is why the
MSI line restricts the product name alone. Pre-releases have no MSI, hence `min-matches="0"`
there.

#### Application checklist

1. Turn on two-factor authentication on the GitHub account (and on every account that gets
   write access later).
2. Wait for a reputation SignPath can verify: downloads across several releases
   (`gh api repos/RomainAntunes/classroom50-toolbox/releases --paginate --jq
'[.[].assets[] | select(.name | test("sig$|json$") | not) | .download_count] | add'`),
   stars, classes or schools using it, and public mentions (a post on the Classroom 50
   discussions, a blog or forum thread). Collect the links.
3. Publish the code signing policy below in the README (a "Code signing" section next to
   "Privacy") and on the download page, and keep the README's "Privacy" section accurate,
   since the policy links to it.
4. Apply at [signpath.org/apply](https://signpath.org/apply): repository
   `https://github.com/RomainAntunes/classroom50-toolbox`, home page
   `https://romainantunes.github.io/classroom50-toolbox/`, download page (the same, or the
   releases page), privacy URL (the README's `#privacy`), a one-line description ("A tray app
   for Classroom 50 students: installs git, gh and gh-student, tracks assignments and
   grades, and submits through gh student"), the reputation links, and GitHub Actions as the
   build system.
5. Once accepted, in SignPath: create the project `classroom50-toolbox`, link GitHub as a
   trusted build system and install the SignPath GitHub App on the repository, add the
   artifact configuration above as `windows-installers`, create a `test-signing` policy and a
   `release-signing` policy (manual approval, trusted build system and origin verification
   on, limited to the `main` branch), and a CI user with an API token.
6. In the repository: the secret `SIGNPATH_API_TOKEN` and the variable
   `SIGNPATH_ORGANIZATION_ID` (Settings › Secrets and variables › Actions).
7. Uncomment `sign-windows` in `release.yml`, add it to `installer-sizes`'s `needs`, and point
   it at `test-signing` for a first beta (`pnpm bump X.Y.Z-beta.N`). Check on a Windows
   machine: the installer's Properties › Digital Signatures names SignPath Foundation, the
   install works, and the previous beta updates to it (the updater checks the new `.sig`).
   Then switch to `release-signing`.
8. Change the README's and the download page's SmartScreen note: a new signature can still
   warn until it has built reputation with Microsoft, but the publisher is named.

#### Draft code signing policy (to publish once accepted)

> ## Code signing policy
>
> Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
> [SignPath Foundation](https://signpath.org).
>
> The Windows installers attached to this project's
> [releases](https://github.com/RomainAntunes/classroom50-toolbox/releases) are built by
> GitHub Actions from this repository's `main` branch and signed through SignPath. Only
> installers built from this repository are signed. The macOS and Linux builds are not
> covered by this policy.
>
> Team roles:
>
> - Committers and reviewers: [Romain Antunes](https://github.com/RomainAntunes)
> - Approvers: [Romain Antunes](https://github.com/RomainAntunes)
>
> Changes from anyone else come as pull requests and are reviewed before they are merged.
> Every team member uses multi-factor authentication on GitHub and SignPath, and every signing
> request is approved by hand.
>
> Privacy: this program talks to GitHub only (its API with the token `gh` already holds,
> classes' published assignment lists on GitHub Pages, avatars, and GitHub Releases to check
> for updates) and sends nothing to the project or anyone else. See
> [Privacy](https://github.com/RomainAntunes/classroom50-toolbox#privacy).

### Paid: Azure Artifact Signing

Microsoft's service (formerly Trusted Signing) is about 10 USD a month, but its public trust
certificates are only available to organizations in the US, Canada, the EU and the UK, so not to
an individual in Switzerland today. If that changes, Tauri supports it through
`bundle > windows > signCommand` in `tauri.conf.json`:

```json
"windows": {
  "signCommand": "artifact-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> -d \"Classroom 50 Toolbox\" %1"
}
```

with the secrets `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` and `AZURE_TENANT_ID` passed to the
Windows build step. A classic OV or EV Authenticode certificate also works, but costs more
(from about 200 USD a year) and needs a hardware token or a cloud HSM.

## Linux

Nothing to do. AppImage, deb and rpm installs don't warn about unsigned packages.
