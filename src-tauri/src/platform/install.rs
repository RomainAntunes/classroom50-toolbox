//! How git and gh get installed on each OS. The plans are pure functions of what is found on
//! the machine (`Probe`), kept apart from the `cfg(target_os)` modules so every OS's plan is
//! unit-tested on any of them.

// Each OS calls only its own plan; the tests cover them all.
#![cfg_attr(not(test), allow(dead_code))]

use serde::Serialize;

use crate::error::{Error, ManualReason, Result};
use crate::tools::Tool;

/// Homebrew without the slow, unrelated work it does first: updating itself and every tap
/// with git (most of a 9 s install of a small formula, minutes on a slow link, all of it
/// silent), upgrading formulae that depend on the new one, cleaning up. The formula data
/// still refreshes: with auto-update off Homebrew would keep its cached API JSON forever
/// (`fetch_api_files!`), so `FORCE_API_AUTO_UPDATE` downloads it when it is older than a few
/// minutes. A formula installed but outdated is upgraded (`HOMEBREW_NO_INSTALL_UPGRADE` is
/// left unset), which "Update" on an outdated gh relies on.
pub const BREW_ENV: &[(&str, &str)] = &[
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_FORCE_API_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK", "1"),
    ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
    ("HOMEBREW_NO_ENV_HINTS", "1"),
];

/// GitHub's own package repository, the one `docs/install_linux.md` upstream recommends:
/// Debian's and Ubuntu's `gh` (2.4 to 2.46 on current releases) is far older than what the
/// app needs (`tools::MIN_GH`), and RHEL-like systems have none.
pub const GH_APT_KEYRING_URL: &str =
    "https://cli.github.com/packages/githubcli-archive-keyring.gpg";

/// Download pages for doing it by hand.
pub const GIT_DOWNLOADS: &str = "https://git-scm.com/downloads";
/// Has an installer for every OS (a `.pkg` on macOS, an `.msi` on Windows).
pub const GH_DOWNLOADS: &str = "https://cli.github.com/";
pub const GH_LINUX_DOCS: &str = "https://github.com/cli/cli/blob/trunk/docs/install_linux.md";
/// App Installer, which brings winget (the Microsoft Store's web page: the opener only
/// allows http and https links).
pub const WINGET_STORE: &str = "https://apps.microsoft.com/detail/9NBLGGH4NNS1";
/// gh's own releases, which list the installer of every architecture (`windows_amd64.msi`).
pub const GH_RELEASES: &str = "https://github.com/cli/cli/releases/latest";
/// Swaps an Arm64 gh for the x64 one, which Windows on Arm runs emulated. gh keeps its
/// sign-in (the credential store and its config folder outlive the uninstall).
pub const GH_X64: &str = "winget uninstall --id GitHub.cli --exact\nwinget install --id GitHub.cli --exact --source winget --architecture x64";

/// What installing a tool runs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub program: String,
    pub args: Vec<String>,
    /// The same install typed in a terminal (`sudo` instead of `pkexec`, no script plumbing),
    /// shown when it fails.
    pub manual: String,
    /// Environment for the program.
    #[serde(skip)]
    pub env: &'static [(&'static str, &'static str)],
    /// The program only starts the install and returns (Apple's Command Line Tools dialog):
    /// the job then waits for the tool to show up.
    #[serde(skip)]
    pub detached: bool,
    /// A file to download first (unprivileged, over HTTPS); its path is appended to `args`.
    #[serde(skip)]
    pub fetch: Option<&'static str>,
}

impl InstallPlan {
    pub fn new(program: &str, args: &[&str], manual: impl Into<String>) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            manual: manual.into(),
            env: &[],
            detached: false,
            fetch: None,
        }
    }

    fn brew(package: &str) -> Self {
        Self {
            env: BREW_ENV,
            ..Self::new(
                "brew",
                &["install", package],
                format!("brew install {package}"),
            )
        }
    }
}

/// What the machine has, as far as installing goes.
pub struct Probe<'a> {
    /// Whether `program` is on the search path.
    pub has: &'a dyn Fn(&str) -> bool,
    /// Linux: the system's packages are read-only (Fedora Atomic, Bazzite, Bluefin: an ostree
    /// deployment) or declared in a config file (NixOS).
    pub immutable: bool,
}

fn gh_student() -> Error {
    Error::Unsupported("gh-student is installed through gh".into())
}

fn download_page(tool: Tool) -> &'static str {
    match tool {
        Tool::Git => GIT_DOWNLOADS,
        _ => GH_DOWNLOADS,
    }
}

/// macOS. git comes with Apple's Command Line Tools: `xcode-select --install` opens Apple's
/// own installer (no password, no Homebrew needed, and the compilers a C assignment needs
/// come with it). Homebrew itself needs those tools, so a Mac without git rarely has it. gh
/// comes from Homebrew; without it, from GitHub's `.pkg`.
pub fn macos(tool: Tool, probe: &Probe) -> Result<InstallPlan> {
    match tool {
        Tool::Git => Ok(InstallPlan {
            detached: true,
            ..InstallPlan::new("xcode-select", &["--install"], "xcode-select --install")
        }),
        Tool::Gh if (probe.has)("brew") => Ok(InstallPlan::brew("gh")),
        Tool::Gh => Err(Error::ManualInstall {
            reason: ManualReason::NoHomebrew,
            url: Some(GH_DOWNLOADS.into()),
            command: None,
        }),
        Tool::GhStudent => Err(gh_student()),
    }
}

/// Windows: winget, which installs per user when the package allows it and otherwise lets
/// the installer ask for elevation (UAC) itself. The default install mode shows the
/// installer's progress window and can raise UAC; `--silent` would hide both. No prompt can
/// be answered (no console), so every agreement is accepted up front and prompts are
/// disabled.
pub fn windows(tool: Tool, probe: &Probe) -> Result<InstallPlan> {
    let id = match tool {
        Tool::Git => "Git.Git",
        Tool::Gh => "GitHub.cli",
        Tool::GhStudent => return Err(gh_student()),
    };
    if !(probe.has)("winget") {
        return Err(Error::ManualInstall {
            reason: ManualReason::NoWinget,
            url: Some(download_page(tool).into()),
            command: None,
        });
    }
    let args = [
        "install",
        "--id",
        id,
        "--exact",
        "--source",
        "winget",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ];
    Ok(InstallPlan::new(
        "winget",
        &args,
        format!("winget install --id {id} --exact --source winget"),
    ))
}

/// winget's own result codes (`returnCodes.md` in winget-cli), as the `i32` the exit code
/// reads as.
mod winget_code {
    pub const INVALID_CL_ARGUMENTS: i32 = 0x8A15_0002_u32 as i32;
    pub const UPDATE_NOT_APPLICABLE: i32 = 0x8A15_002B_u32 as i32;
    pub const UPGRADE_VERSION_NOT_NEWER: i32 = 0x8A15_004F_u32 as i32;
    pub const PACKAGE_ALREADY_INSTALLED: i32 = 0x8A15_0061_u32 as i32;
    pub const INSTALL_CANCELLED_BY_USER: i32 = 0x8A15_010C_u32 as i32;
}

/// Linux: the distribution's package manager, through `pkexec` (the desktop's own password
/// prompt; there is no terminal for `sudo`). gh comes from GitHub's repository on apt, dnf
/// and zypper (see `GH_APT_KEYRING_URL`), from Arch's own `github-cli` on pacman. Systems
/// whose packages are read-only use Homebrew when it is there (Bluefin and Aurora ship it).
pub fn linux(tool: Tool, probe: &Probe) -> Result<InstallPlan> {
    let has = probe.has;
    if tool == Tool::GhStudent {
        return Err(gh_student());
    }
    let package = if tool == Tool::Git { "git" } else { "gh" };
    let manual_page = || {
        Some(
            if tool == Tool::Git {
                GIT_DOWNLOADS
            } else {
                GH_LINUX_DOCS
            }
            .into(),
        )
    };
    if probe.immutable {
        if has("brew") {
            return Ok(InstallPlan::brew(package));
        }
        return Err(Error::ManualInstall {
            reason: ManualReason::ReadOnlySystem,
            url: manual_page(),
            command: None,
        });
    }
    let Some(manager) = ["apt-get", "dnf", "pacman", "zypper"]
        .into_iter()
        .find(|m| has(m))
    else {
        if has("brew") {
            return Ok(InstallPlan::brew(package));
        }
        return Err(Error::ManualInstall {
            reason: ManualReason::NoPackageManager,
            url: manual_page(),
            command: None,
        });
    };
    let (manual, mut plan) = linux_plan(manager, tool);
    if !has("pkexec") {
        // Without polkit there's no password prompt the app can show.
        if has("brew") {
            return Ok(InstallPlan::brew(package));
        }
        return Err(Error::ManualInstall {
            reason: ManualReason::NoPkexec,
            url: manual_page(),
            command: Some(manual),
        });
    }
    // Asks on the desktop's polkit agent; with none (a bare window manager) it would ask on
    // a terminal the app doesn't have, so it fails instead.
    plan.insert(0, "--disable-internal-agent".into());
    Ok(InstallPlan {
        program: "pkexec".into(),
        args: plan,
        manual,
        env: &[],
        detached: false,
        fetch: (manager == "apt-get" && tool == Tool::Gh).then_some(GH_APT_KEYRING_URL),
    })
}

/// `pkexec` passes no environment on, so apt's settings live in the script.
const APT_GIT: &str = "set -e
export DEBIAN_FRONTEND=noninteractive
apt-get install -y -o DPkg::Lock::Timeout=120 git";

/// GitHub's apt repository, set up as `install_linux.md` does, with the keyring the app
/// downloaded (`$1`). Only that repository's index is refreshed, so a broken third-party
/// source elsewhere can't fail the install, and failing to fetch it is an error (apt only
/// warns by default, then installs the distribution's old gh; older apt ignores the option). Re-running it upgrades an outdated gh.
const APT_GH: &str = r#"set -e
export DEBIAN_FRONTEND=noninteractive
install -d -m 755 /etc/apt/keyrings /etc/apt/sources.list.d
install -m 644 "$1" /etc/apt/keyrings/githubcli-archive-keyring.gpg
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" > /etc/apt/sources.list.d/github-cli.list
apt-get update -o APT::Update::Error-Mode=any -o Dir::Etc::sourcelist=sources.list.d/github-cli.list -o Dir::Etc::sourceparts=- -o APT::Get::List-Cleanup=0
apt-get install -y -o DPkg::Lock::Timeout=120 gh"#;

/// GitHub's rpm repository (the content of `https://cli.github.com/packages/rpm/gh-cli.repo`),
/// written where both dnf 4 and 5 read it (their `config-manager` syntaxes differ). `install`
/// then `upgrade`, so an outdated gh from Fedora's own repository is replaced.
const DNF_GH: &str = "set -e
cat > /etc/yum.repos.d/gh-cli.repo <<'EOF'
[gh-cli]
name=packages for the GitHub CLI
baseurl=https://cli.github.com/packages/rpm
enabled=1
gpgcheck=1
gpgkey=https://cli.github.com/packages/githubcli-archive-keyring.asc
EOF
dnf install -y gh
dnf upgrade -y gh";

/// The same repository for zypper. `--from` takes gh from it even when openSUSE's own (older)
/// gh is installed: zypper won't switch vendors otherwise.
const ZYPPER_GH: &str = "set -e
cat > /etc/zypp/repos.d/gh-cli.repo <<'EOF'
[gh-cli]
name=packages for the GitHub CLI
baseurl=https://cli.github.com/packages/rpm
enabled=1
autorefresh=1
type=rpm-md
gpgcheck=1
gpgkey=https://cli.github.com/packages/githubcli-archive-keyring.asc
EOF
zypper --non-interactive --gpg-auto-import-keys refresh gh-cli
zypper --non-interactive install --from gh-cli gh";

/// The terminal command, and what `pkexec` runs. Scripts are constants run with `sh -c`;
/// nothing from outside is spliced into them (the keyring path is a positional argument).
fn linux_plan(manager: &str, tool: Tool) -> (String, Vec<String>) {
    let script =
        |script: &str| -> Vec<String> { ["sh", "-c", script, "sh"].map(str::to_owned).to_vec() };
    let words = |words: &[&str]| -> Vec<String> { words.iter().map(|w| (*w).to_owned()).collect() };
    match (manager, tool) {
        ("apt-get", Tool::Git) => ("sudo apt-get install -y git".into(), script(APT_GIT)),
        ("apt-get", _) => (
            [
                "sudo mkdir -p -m 755 /etc/apt/keyrings",
                "wget -qO- https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null",
                "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null",
                "sudo apt update && sudo apt install -y gh",
            ]
            .join("\n"),
            script(APT_GH),
        ),
        ("dnf", Tool::Git) => ("sudo dnf install -y git".into(), words(&["dnf", "install", "-y", "git"])),
        ("dnf", _) => (
            "sudo dnf config-manager addrepo --from-repofile=https://cli.github.com/packages/rpm/gh-cli.repo && sudo dnf install gh".into(),
            script(DNF_GH),
        ),
        // `--needed`: nothing to do when it is already up to date. Arch's github-cli is
        // current. No `-y`: refreshing the database alone is a partial upgrade.
        ("pacman", Tool::Git) => (
            "sudo pacman -Syu git".into(),
            words(&["pacman", "-S", "--needed", "--noconfirm", "git"]),
        ),
        ("pacman", _) => (
            "sudo pacman -Syu github-cli".into(),
            words(&["pacman", "-S", "--needed", "--noconfirm", "github-cli"]),
        ),
        ("zypper", Tool::Git) => (
            "sudo zypper install git".into(),
            words(&["zypper", "--non-interactive", "install", "git"]),
        ),
        _ => (
            "sudo zypper addrepo https://cli.github.com/packages/rpm/gh-cli.repo && sudo zypper install gh".into(),
            script(ZYPPER_GH),
        ),
    }
}

/// What a failed install's exit code, or the tail of its `output`, means beyond "it failed".
pub fn interpret_failure(program: &str, code: Option<i32>, output: &str) -> Option<Result<()>> {
    use winget_code::*;
    // gh-student ships no `windows-arm64` binary, and gh only falls back to amd64 on Macs
    // (Rosetta), so an Arm64 gh refuses the extension (`<name> unsupported for <platform>.`).
    // gh suggests an issue on gh-student's repo, which has issues turned off.
    if program == "gh" && output.contains("gh-student unsupported for windows-arm64") {
        return Some(Err(Error::ManualInstall {
            reason: ManualReason::WindowsArm,
            url: Some(GH_RELEASES.into()),
            command: Some(GH_X64.into()),
        }));
    }
    match (program, code?) {
        // Installed after all (only the app didn't see it, e.g. PATH not refreshed), or up
        // to date: a success, and the status is read again.
        (
            "winget",
            UPDATE_NOT_APPLICABLE | UPGRADE_VERSION_NOT_NEWER | PACKAGE_ALREADY_INSTALLED,
        ) => Some(Ok(())),
        ("winget", INSTALL_CANCELLED_BY_USER) => Some(Err(Error::Cancelled)),
        // An App Installer from before `--disable-interactivity` (winget 1.4).
        ("winget", INVALID_CL_ARGUMENTS) => Some(Err(Error::ManualInstall {
            reason: ManualReason::OldWinget,
            url: Some(WINGET_STORE.into()),
            command: None,
        })),
        // The password prompt was dismissed.
        ("pkexec", 126) => Some(Err(Error::Cancelled)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe<'a>(has: &'a dyn Fn(&str) -> bool, immutable: bool) -> Probe<'a> {
        Probe { has, immutable }
    }

    fn line(plan: &InstallPlan) -> String {
        std::iter::once(plan.program.as_str())
            .chain(plan.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn macos_git_comes_from_apple_and_gh_from_homebrew() {
        let brew = |p: &str| p == "brew";
        let git = macos(Tool::Git, &probe(&brew, false)).unwrap();
        assert_eq!(line(&git), "xcode-select --install");
        assert!(git.detached);

        let gh = macos(Tool::Gh, &probe(&brew, false)).unwrap();
        assert_eq!(line(&gh), "brew install gh");
        assert!(gh.env.contains(&("HOMEBREW_NO_AUTO_UPDATE", "1")));
        assert!(gh.env.contains(&("HOMEBREW_FORCE_API_AUTO_UPDATE", "1")));
        assert!(
            !gh.env
                .iter()
                .any(|(k, _)| *k == "HOMEBREW_NO_INSTALL_UPGRADE")
        );

        // No Homebrew: git still installs, gh points at GitHub's installer.
        let none = |_: &str| false;
        assert!(macos(Tool::Git, &probe(&none, false)).is_ok());
        let err = macos(Tool::Gh, &probe(&none, false)).unwrap_err();
        assert!(
            matches!(&err, Error::ManualInstall { url: Some(url), .. } if url == GH_DOWNLOADS),
            "{err:?}"
        );
    }

    #[test]
    fn windows_uses_winget_with_no_prompt() {
        let winget = |p: &str| p == "winget";
        let gh = windows(Tool::Gh, &probe(&winget, false)).unwrap();
        assert_eq!(
            line(&gh),
            "winget install --id GitHub.cli --exact --source winget \
             --accept-package-agreements --accept-source-agreements --disable-interactivity"
        );
        let git = windows(Tool::Git, &probe(&winget, false)).unwrap();
        assert!(git.args.contains(&"Git.Git".to_owned()));
        // The installer shows its progress and may raise UAC.
        assert!(!git.args.iter().any(|a| a == "--silent" || a == "-h"));

        let err = windows(Tool::Git, &probe(&|_| false, false)).unwrap_err();
        assert!(
            matches!(&err, Error::ManualInstall { url: Some(url), .. } if url == GIT_DOWNLOADS),
            "{err:?}"
        );
    }

    #[test]
    fn reads_wingets_result_codes() {
        // As `ExitStatus::code` reports them on Windows.
        assert!(matches!(
            interpret_failure("winget", Some(-1_978_335_189), ""),
            Some(Ok(()))
        ));
        assert!(matches!(
            interpret_failure("winget", Some(-1_978_335_135), ""),
            Some(Ok(()))
        ));
        assert!(matches!(
            interpret_failure("winget", Some(-1_978_334_964), ""),
            Some(Err(Error::Cancelled))
        ));
        assert!(matches!(
            interpret_failure("winget", Some(-1_978_335_230), ""),
            Some(Err(Error::ManualInstall { .. }))
        ));
        assert!(interpret_failure("winget", Some(1), "").is_none());
        assert!(matches!(
            interpret_failure("pkexec", Some(126), ""),
            Some(Err(Error::Cancelled))
        ));
        assert!(interpret_failure("pkexec", Some(127), "").is_none());
        assert!(interpret_failure("brew", None, "").is_none());
    }

    #[test]
    fn sends_windows_on_arm_to_the_x64_gh() {
        // gh 2.81's words, its issue command cut short.
        let output = "gh-student unsupported for windows-arm64.\n\nTo request support for \
            windows-arm64, open an issue on the extension's repo by running the following \
            command:\n\n\t`gh issue create -R foundation50/gh-student --title ...`";
        let Some(Err(Error::ManualInstall {
            reason,
            url,
            command,
        })) = interpret_failure("gh", Some(1), output)
        else {
            panic!("not a manual install");
        };
        assert_eq!(reason, ManualReason::WindowsArm);
        assert_eq!(url.as_deref(), Some(GH_RELEASES));
        assert!(command.unwrap().contains("--architecture x64"));
        // Another platform, or another program, keeps gh's own words.
        let freebsd = output.replace("windows-arm64", "freebsd-arm64");
        assert!(interpret_failure("gh", Some(1), &freebsd).is_none());
        assert!(interpret_failure("winget", Some(1), output).is_none());
    }

    #[test]
    fn linux_elevates_with_pkexec() {
        let apt = |p: &str| matches!(p, "apt-get" | "pkexec");
        let git = linux(Tool::Git, &probe(&apt, false)).unwrap();
        assert_eq!(git.program, "pkexec");
        assert_eq!(
            git.args[..4],
            ["--disable-internal-agent", "sh", "-c", APT_GIT]
        );
        assert_eq!(git.fetch, None);
        assert_eq!(git.manual, "sudo apt-get install -y git");

        // gh from GitHub's repository, with the keyring downloaded first and passed as $1.
        let gh = linux(Tool::Gh, &probe(&apt, false)).unwrap();
        assert_eq!(
            gh.args[..5],
            ["--disable-internal-agent", "sh", "-c", APT_GH, "sh"]
        );
        assert_eq!(gh.fetch, Some(GH_APT_KEYRING_URL));

        let pacman = |p: &str| matches!(p, "pacman" | "pkexec");
        let gh = linux(Tool::Gh, &probe(&pacman, false)).unwrap();
        assert_eq!(
            line(&gh),
            "pkexec --disable-internal-agent pacman -S --needed --noconfirm github-cli"
        );

        let dnf = |p: &str| matches!(p, "dnf" | "pkexec");
        assert_eq!(
            line(&linux(Tool::Git, &probe(&dnf, false)).unwrap()),
            "pkexec --disable-internal-agent dnf install -y git"
        );
        assert!(
            linux(Tool::Gh, &probe(&dnf, false))
                .unwrap()
                .args
                .contains(&DNF_GH.to_owned())
        );

        let zypper = |p: &str| matches!(p, "zypper" | "pkexec");
        assert!(
            linux(Tool::Gh, &probe(&zypper, false))
                .unwrap()
                .args
                .contains(&ZYPPER_GH.to_owned())
        );
    }

    #[test]
    fn linux_without_pkexec_or_a_package_manager() {
        // No polkit: the command for a terminal, or Homebrew when it is there.
        let apt = |p: &str| p == "apt-get";
        let err = linux(Tool::Git, &probe(&apt, false)).unwrap_err();
        assert!(
            matches!(&err, Error::ManualInstall { command: Some(c), .. } if c == "sudo apt-get install -y git"),
            "{err:?}"
        );
        let apt_brew = |p: &str| matches!(p, "apt-get" | "brew");
        assert_eq!(
            line(&linux(Tool::Gh, &probe(&apt_brew, false)).unwrap()),
            "brew install gh"
        );

        // Read-only systems: Homebrew, never the package manager.
        let ostree = |p: &str| matches!(p, "dnf" | "pkexec" | "brew");
        assert_eq!(
            line(&linux(Tool::Gh, &probe(&ostree, true)).unwrap()),
            "brew install gh"
        );
        let bare = |p: &str| matches!(p, "dnf" | "pkexec");
        assert!(matches!(
            linux(Tool::Gh, &probe(&bare, true)),
            Err(Error::ManualInstall { .. })
        ));

        assert!(matches!(
            linux(Tool::Git, &probe(&|_| false, false)),
            Err(Error::ManualInstall { .. })
        ));
    }

    /// The scripts run as root through `sh -c`: they must parse, and take the keyring as $1.
    #[cfg(unix)]
    #[test]
    fn linux_scripts_parse() {
        for script in [APT_GIT, APT_GH, DNF_GH, ZYPPER_GH] {
            let status = std::process::Command::new("sh")
                .args(["-n", "-c", script])
                .status()
                .unwrap();
            assert!(status.success(), "{script}");
            assert!(!script.contains('\u{2014}'));
        }
        assert!(APT_GH.contains("\"$1\""));
    }
}
