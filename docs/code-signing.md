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
named in the signature. Keys stay on SignPath's HSMs, so no secret key lives in this repo.

The project qualifies on most points already (GPL-3.0, public source, built by GitHub Actions,
released, documented on its download page). What it still needs:

- **Reputation**: SignPath asks for verifiable use. Apply once there are real numbers to show:
  downloads over several releases, stars, classes using it.
- **A code signing policy** on the README and the download page (SignPath gives a template).
- **Multi-factor authentication** on the GitHub account of everyone with write access.

Apply at [signpath.org/apply](https://signpath.org/apply) and read the
[terms](https://signpath.org/terms.html). Once accepted, the release workflow uploads the
unsigned Windows installers to SignPath (their `submit-signing-request` GitHub Action) and
attaches the signed files to the draft release. SignPath's onboarding provides the exact
workflow steps for the project.

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
