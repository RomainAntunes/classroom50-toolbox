# Security policy

## Supported versions

Security fixes go into the latest release. The app checks for updates every day and installs
them when you restart it, so staying on the newest version only takes a restart.

| Version                        | Supported |
| ------------------------------ | --------- |
| Latest stable release          | Yes       |
| Latest beta (the beta channel) | Yes       |
| Older releases                 | No        |
| Development builds from `main` | No        |

## Reporting a vulnerability

Please don't open a public issue, pull request or discussion for a security problem.

Report it privately through GitHub instead: go to the repository's **Security** tab and click
**Report a vulnerability**, or open
[a new advisory](https://github.com/RomainAntunes/classroom50-toolbox/security/advisories/new)
directly. Only the maintainer sees the report. GitHub's guide,
[Privately reporting a security vulnerability](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability),
explains the steps.

A useful report says:

- what an attacker can do, and what they need first (a web page you visit, a file on disk, a
  repository you clone, a network position...);
- how to reproduce it, step by step, with the version (Settings > About) and your OS;
- a proof of concept if you have one.

Don't include anyone's GitHub token, private repository content or personal data. If logs help,
Settings > About > Copy diagnostics makes a report with tokens, emails, accounts, classes and
repositories replaced by placeholders; check it before you attach it.

## What happens next

The toolbox is maintained by one person in their spare time, so please allow some time:

1. You get an answer within 7 days, confirming the report or asking for details.
2. Once the problem is confirmed, the fix is prepared in the advisory's temporary private fork and
   ships in a new release. The app updates itself, so a fix reaches people within a day or two of
   its release.
3. The advisory is then published, crediting you unless you'd rather stay anonymous. A CVE is
   requested through GitHub when the issue warrants one.

Please keep the details private until the advisory is published.

## Scope

In scope: the Classroom 50 Toolbox app and its installers, its update feed, and the download
page at `romainantunes.github.io/classroom50-toolbox`. Examples of what matters most:

- a `classroom50-toolbox://` link, a repository, a manifest or a GitHub response that makes the
  app run a command, write a file or reach GitHub with the student's account without their click;
- the GitHub token leaking out of `gh` (the app reads it per request and never stores it), for
  instance into logs or the diagnostics report;
- an update that installs without a valid signature;
- a development build writing to GitHub (accept, submit, invite, push), which it must refuse.

Out of scope, please report these where they belong:

- the `gh student` extension and Classroom 50 itself:
  [foundation50/classroom50](https://github.com/foundation50/classroom50/security);
- the GitHub CLI: [cli/cli](https://github.com/cli/cli/security/policy);
- GitHub's own services: [GitHub's bug bounty](https://bounty.github.com);
- Tauri and other dependencies, unless the toolbox uses them in an unsafe way: report to their
  maintainers.

The installers aren't code-signed yet, so macOS and Windows warn on the first launch. That is a
known limitation with a plan in [`docs/code-signing.md`](../docs/code-signing.md), not a
vulnerability. Updates themselves are signed and checked before they install.
