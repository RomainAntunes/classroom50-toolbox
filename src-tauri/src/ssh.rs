//! SSH to GitHub: whether git can reach github.com over SSH with the active account's key,
//! why not, and the fixes the app makes on a click (trusting GitHub's host keys, adding a
//! key to the account). Whether git uses SSH at all is gh's `git_protocol`
//! (`tools::git_protocol`), never an app setting.
//!
//! The probe runs ssh the way git would (`ssh_command`, git's own order) with
//! `BatchMode=yes`, so it never prompts: GitHub greets a key it knows with `Hi <login>!` and
//! closes (exit 1), and anything else is ssh's own error line, told apart by `classify`.
//! Writes outside the app's files: `trust_github` appends to the user's `known_hosts`,
//! `replace_github_host_key` removes GitHub's old lines there first (`ssh-keygen -R` keeps a
//! `.old` copy), and `set_up_key` may create `~/.ssh/id_ed25519` (never over a key) before
//! `gh ssh-key add`, which dev builds refuse first (`process::ensure_allowed`).

use std::ffi::OsString;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cooldown::Recent;
use crate::error::{Error, Result};
use crate::platform;
use crate::process;

const HOST: &str = "github.com";
/// The probe's target, as git connects for `git@github.com:owner/repo`.
const TARGET: &str = "git@github.com";
/// A public repository any key GitHub accepts can list, for a probe through git (a custom
/// `core.sshCommand`, whose greeting git doesn't show).
const PROBE_REPO: &str = "git@github.com:foundation50/classroom50.git";
/// `ConnectTimeout` is the TCP connect only; the whole probe gets this.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const KEYGEN_TIMEOUT: Duration = Duration::from_secs(30);
const ADD_KEY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long `ssh_status` reuses its last answer.
const STATUS_COOLDOWN: Duration = Duration::from_secs(60);
/// The key's title on github.com/settings/keys.
const KEY_TITLE: &str = "Classroom 50 Toolbox";
/// The comment of a key the app makes.
const KEY_COMMENT: &str = "classroom50-toolbox";
/// The files ssh tries when nothing names one (`ssh -G` lists them), in its order.
const DEFAULT_KEYS: [&str; 5] = [
    "id_rsa",
    "id_ecdsa",
    "id_ecdsa_sk",
    "id_ed25519",
    "id_ed25519_sk",
];

/// Where SSH to GitHub stands for the active account.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SshStatus {
    /// GitHub accepts a key of this computer as `login` (`None` when the probe went through
    /// a custom ssh command, which doesn't show who).
    Ready {
        login: Option<String>,
    },
    /// GitHub accepts a key, but as `login`, not gh's active account `active`, or as a
    /// deploy key (`owner/repo`): it reaches only that account's repositories.
    OtherAccount {
        login: String,
        active: Option<String>,
    },
    /// No key of this computer is on the active account. `localKey` is a public key there
    /// is to add (`~/.ssh/id_ed25519.pub`), `None` when one must be made first.
    NoKey {
        local_key: Option<String>,
    },
    /// A key of this computer is on the account, but ssh didn't sign in with it: it has a
    /// passphrase and no agent holds it, or ssh's config names another key.
    KeyNotOffered {
        local_key: String,
    },
    /// `known_hosts` has no key for GitHub: ssh refuses to connect without asking.
    HostKeyUnknown,
    /// `known_hosts` holds another key for GitHub, usually its RSA key from before 2023.
    HostKeyChanged,
    /// No connection: port 22 blocked, no network, no DNS.
    Unreachable {
        message: String,
    },
    NotInstalled,
    Failed {
        message: String,
    },
}

/// Why an SSH connection to GitHub failed (`Error::Ssh`); the UI words it as
/// `ssh.problem.<code>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshProblem {
    /// `Permission denied (publickey)`.
    KeyRejected,
    /// The key signs in as an account that can't reach the repository.
    OtherAccount,
    HostKeyUnknown,
    HostKeyChanged,
    Unreachable,
    NotInstalled,
    /// `gh ssh-key add` needs the `write:public_key` scope.
    NeedsScope,
    /// GitHub refused the key: another account already has it.
    KeyInUse,
    /// `~/.ssh/id_ed25519` exists without its `.pub`: the app won't make a key over it.
    PrivateKeyOnly,
}

impl SshProblem {
    pub const ALL: [Self; 9] = [
        Self::KeyRejected,
        Self::OtherAccount,
        Self::HostKeyUnknown,
        Self::HostKeyChanged,
        Self::Unreachable,
        Self::NotInstalled,
        Self::NeedsScope,
        Self::KeyInUse,
        Self::PrivateKeyOnly,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::KeyRejected => "keyRejected",
            Self::OtherAccount => "otherAccount",
            Self::HostKeyUnknown => "hostKeyUnknown",
            Self::HostKeyChanged => "hostKeyChanged",
            Self::Unreachable => "unreachable",
            Self::NotInstalled => "notInstalled",
            Self::NeedsScope => "needsScope",
            Self::KeyInUse => "keyInUse",
            Self::PrivateKeyOnly => "privateKeyOnly",
        }
    }
}

impl std::fmt::Display for SshProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::KeyRejected => "GitHub didn't accept an SSH key from this computer",
            Self::OtherAccount => {
                "the SSH key of this computer belongs to a GitHub account that can't reach this repository"
            }
            Self::HostKeyUnknown => "this computer doesn't know GitHub's SSH host key yet",
            Self::HostKeyChanged => {
                "the GitHub SSH host key saved on this computer doesn't match GitHub's"
            }
            Self::Unreachable => "can't reach GitHub over SSH; this network may block it",
            Self::NotInstalled => "ssh isn't installed",
            Self::NeedsScope => {
                "adding an SSH key needs access to your GitHub account's keys; grant it first"
            }
            Self::KeyInUse => "this SSH key is already on another GitHub account",
            Self::PrivateKeyOnly => {
                "~/.ssh/id_ed25519 has no public key next to it, and the app won't replace it"
            }
        })
    }
}

/// An SSH failure in git's or ssh's error output, `None` when it isn't one: the problem,
/// and for `OtherAccount` the account GitHub named when it did. Only ssh's and GitHub's SSH
/// wordings count, so an HTTPS failure is never taken for one, and only for github.com, so
/// a submodule or hook reaching another server isn't either. `OtherAccount` here only says
/// GitHub answered for an account (`Permission to o/r.git denied to X`, `Repository not
/// found`): whether that is the wrong key is `failure`'s to tell.
pub fn classify(stderr: &str) -> Option<(SshProblem, Option<String>)> {
    // ssh says "Host key verification failed" for a changed key too.
    if stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return about_github(
            stderr,
            " has changed and you have requested strict checking",
            "Host key for ",
        )
        .then_some((SshProblem::HostKeyChanged, None));
    }
    if stderr.contains("Host key verification failed") {
        return about_github(
            stderr,
            " and you have requested strict checking",
            " host key is known for ",
        )
        .then_some((SshProblem::HostKeyUnknown, None));
    }
    for line in stderr.lines().map(str::trim) {
        // `git@github.com: Permission denied (publickey).`, or `(publickey,keyboard-interactive)`.
        if let Some(at) = line.find("Permission denied (publickey") {
            let server = line[..at].trim_end().trim_end_matches(':');
            let host = server.rsplit_once('@').map_or(server, |(_, host)| host);
            if host.is_empty() || is_github_host(host) {
                return Some((SshProblem::KeyRejected, None));
            }
            return None;
        }
        // GitHub's SSH server; over HTTPS the same words come after `remote: `.
        if let Some(rest) = line.strip_prefix("ERROR: Permission to ") {
            let login = rest
                .split_once(" denied to ")
                .map(|(_, login)| login.trim_end_matches('.').trim().to_owned())
                .filter(|login| !login.is_empty());
            return Some((SshProblem::OtherAccount, login));
        }
        if line.starts_with("ERROR: Repository not found") {
            return Some((SshProblem::OtherAccount, None));
        }
        if let Some(host) = unreachable_host(line) {
            return is_github_host(host).then_some((SshProblem::Unreachable, None));
        }
        if line.contains("kex_exchange_identification")
            || line.contains("Connection timed out during banner exchange")
        {
            return Some((SshProblem::Unreachable, None));
        }
        // git without ssh: `error: cannot run ssh: No such file or directory`; never
        // `cannot run ssh-keygen` (SSH commit signing).
        if line.contains("cannot run ssh:") || line.contains("cannot spawn ssh:") {
            return Some((SshProblem::NotInstalled, None));
        }
    }
    None
}

/// Whether ssh's host key message names GitHub: `<before><host><after>` on one of its lines.
/// A message without that line (another ssh's wording) counts.
fn about_github(stderr: &str, after: &str, before: &str) -> bool {
    stderr
        .lines()
        .find_map(|line| {
            let (head, _) = line.split_once(after)?;
            let (_, host) = head.rsplit_once(before)?;
            Some(host.trim())
        })
        .is_none_or(is_github_host)
}

/// `github.com` or its port-443 endpoint `ssh.github.com`, as ssh names it (`[host]:port`
/// for another port).
fn is_github_host(host: &str) -> bool {
    let host = host.trim();
    let host = match host.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map_or(rest, |(host, _)| host),
        None => host,
    };
    host.eq_ignore_ascii_case(HOST) || host.eq_ignore_ascii_case("ssh.github.com")
}

/// The server of `ssh: connect to host <host> port 22: Connection refused` (or timed out, no
/// route, network unreachable) and `ssh: Could not resolve hostname <host>: ...`.
fn unreachable_host(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("ssh: connect to host ") {
        return rest.split_whitespace().next();
    }
    let rest = line.strip_prefix("ssh: Could not resolve hostname ")?;
    Some(rest.split_once(':').map_or(rest, |(host, _)| host))
}

/// GitHub's own refusal over SSH (`ERROR: This repository was archived so it is read-only.`),
/// without its `ERROR: `: what git's last line (`Could not read from remote repository.`)
/// hides.
pub fn github_error(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("ERROR: "))
        .map(|rest| rest.trim().to_owned())
        .filter(|rest| !rest.is_empty())
}

/// Who git signs in as over SSH, next to gh's active account: what tells a key of another
/// account from a repository the account itself can't write to or see (a read-only repo
/// after Close submission, access not granted yet). Each is read only when a failure needs
/// it.
pub enum Who<'a> {
    /// gh's local config, and the probe shared through `SshState`.
    Live {
        state: &'a SshState,
        http: reqwest::Client,
    },
    /// Known up front.
    #[cfg(test)]
    Known {
        active: Option<String>,
        probe: SshStatus,
    },
}

impl<'a> Who<'a> {
    pub fn live(app: &'a tauri::AppHandle) -> Self {
        use tauri::Manager;
        Self::Live {
            state: app.state::<SshState>().inner(),
            http: app.state::<crate::github::Http>().0.clone(),
        }
    }

    async fn active(&self) -> Option<String> {
        match self {
            Self::Live { .. } => crate::tools::active_login().await,
            #[cfg(test)]
            Self::Known { active, .. } => active.clone(),
        }
    }

    async fn probe(&self, active: &str) -> SshStatus {
        match self {
            Self::Live { state, http } => state.status(false, status(http, Some(active))).await,
            #[cfg(test)]
            Self::Known { probe, .. } => probe.clone(),
        }
    }
}

/// `Error::Ssh` for an SSH failure in `stderr` from a command that reached the network
/// (clone, pull, push), `None` when SSH isn't to blame and git's own words should stand.
/// GitHub naming gh's active account (`denied to <active>`) means the key works and the
/// account lacks access; `Repository not found` is the wrong key only when the probe greets
/// another account. An unknown active account blames nothing.
pub async fn failure(stderr: &str, who: &Who<'_>) -> Option<Error> {
    let (problem, login) = classify(stderr)?;
    if problem != SshProblem::OtherAccount {
        return Some(Error::Ssh {
            problem,
            login: None,
        });
    }
    let active = who.active().await?;
    let other = match login {
        Some(login) => (!login.eq_ignore_ascii_case(&active)).then_some(login),
        None => match who.probe(&active).await {
            SshStatus::OtherAccount { login, .. } => Some(login),
            _ => None,
        },
    }?;
    Some(Error::Ssh {
        problem,
        login: Some(other),
    })
}

/// The login of GitHub's greeting (`Hi alice! You've successfully authenticated, but GitHub
/// does not provide shell access.`). A deploy key is greeted as its `owner/repo`.
fn parse_greeting(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Hi ")?;
        let (login, after) = rest.split_once('!')?;
        (after.contains("successfully authenticated") && !login.is_empty() && !login.contains(' '))
            .then(|| login.to_owned())
    })
}

/// How git runs ssh for github.com.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshCommand {
    /// OpenSSH's `ssh`, run with the app's own arguments: a path, or a name on PATH.
    Direct(PathBuf),
    /// A command with arguments of its own (`ssh -i key`) or another client (`plink`):
    /// only git runs it as the user set it up, so the probe goes through git.
    ThroughGit,
}

/// git's order: `GIT_SSH_COMMAND`, `core.sshCommand`, `GIT_SSH`, then plain `ssh`, which
/// is `bundled` (Git for Windows' own, `platform::git_ssh_program`) where there is one, else
/// the one `on_path`. A command is a shell line for git: one word naming `ssh` is run
/// directly, anything else only through git. `None` when there's no ssh at all.
fn pick_command(
    git_ssh_command: Option<&str>,
    core_ssh_command: Option<&str>,
    git_ssh: Option<&str>,
    bundled: Option<PathBuf>,
    on_path: Option<PathBuf>,
) -> Option<SshCommand> {
    fn set(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|v| !v.is_empty())
    }
    if let Some(line) = set(git_ssh_command).or_else(|| set(core_ssh_command)) {
        let plain = !line.contains(char::is_whitespace)
            && !line.contains(['"', '\'', '$', '`', ';', '|', '&']);
        return Some(if plain && is_openssh(Path::new(line)) {
            SshCommand::Direct(PathBuf::from(line))
        } else {
            SshCommand::ThroughGit
        });
    }
    // GIT_SSH is a program, never a shell line: any path works, but only OpenSSH takes
    // the probe's options.
    if let Some(program) = set(git_ssh) {
        let program = PathBuf::from(program);
        return Some(if is_openssh(&program) {
            SshCommand::Direct(program)
        } else {
            SshCommand::ThroughGit
        });
    }
    bundled.or(on_path).map(SshCommand::Direct)
}

/// `ssh`, `/usr/bin/ssh`, `C:\Program Files\Git\usr\bin\ssh.exe`; not `plink`.
fn is_openssh(program: &Path) -> bool {
    program
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("ssh"))
}

/// The ssh git uses for github.com, `None` when there is none.
pub async fn ssh_command() -> Option<SshCommand> {
    let env = |name: &str| std::env::var(name).ok();
    let core = process::run(
        "git",
        ["config", "--get", "core.sshCommand"],
        Some(&std::env::temp_dir()),
    )
    .await
    .ok()
    .map(|out| out.stdout);
    pick_command(
        env("GIT_SSH_COMMAND").as_deref(),
        core.as_deref(),
        env("GIT_SSH").as_deref(),
        platform::git_ssh_program(),
        process::find_in_path("ssh"),
    )
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Git for Windows runs its own `usr\bin\ssh.exe`, found from `git`'s path: its install
/// root holds `cmd\git.exe`, `bin\git.exe` or `mingw64\bin\git.exe`.
pub(crate) fn bundled_ssh(git: &Path) -> Option<PathBuf> {
    let dir = git.parent()?;
    let name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
    };
    let root = match name(dir).as_deref() {
        Some("cmd") => dir.parent()?,
        Some("bin") => {
            let up = dir.parent()?;
            match name(up).as_deref() {
                Some("mingw64" | "mingw32" | "clangarm64" | "usr") => up.parent()?,
                _ => up,
            }
        }
        _ => return None,
    };
    Some(root.join("usr").join("bin").join("ssh.exe"))
}

/// What one connection attempt said.
#[derive(Debug, PartialEq, Eq)]
enum Probe {
    /// GitHub accepted a key, as `login` when ssh showed the greeting.
    Authenticated(Option<String>),
    /// ssh's or git's error output.
    Refused(String),
    TimedOut,
    NotInstalled,
}

async fn probe(command: &SshCommand) -> Probe {
    let result = match command {
        SshCommand::Direct(ssh) => {
            let args = [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                TARGET,
            ];
            process::run_with_timeout(&ssh.to_string_lossy(), args, None, PROBE_TIMEOUT).await
        }
        SshCommand::ThroughGit => {
            let args = ["ls-remote", PROBE_REPO, "HEAD"];
            let dir = std::env::temp_dir();
            match process::run_with_timeout("git", args, Some(&dir), PROBE_TIMEOUT).await {
                Ok(_) => return Probe::Authenticated(None),
                other => other,
            }
        }
    };
    match result {
        // GitHub closes with 1 after its greeting; a 0 would be a server that let us in.
        Ok(out) => match parse_greeting(&format!("{}\n{}", out.stderr, out.stdout)) {
            Some(login) => Probe::Authenticated(Some(login)),
            None => Probe::Refused(out.stderr),
        },
        Err(Error::CommandFailed { stderr, .. }) => match parse_greeting(&stderr) {
            Some(login) => Probe::Authenticated(Some(login)),
            None => Probe::Refused(stderr),
        },
        Err(Error::TimedOut { .. }) => Probe::TimedOut,
        Err(Error::NotInstalled { .. }) => Probe::NotInstalled,
        Err(err) => Probe::Refused(err.to_string()),
    }
}

/// Where SSH to GitHub stands for `active` (gh's account), by a probe; when GitHub turns
/// every key down, this computer's public keys are compared with the account's
/// (`https://github.com/<login>.keys`, public) to tell a missing key from one ssh didn't use.
pub async fn status(http: &reqwest::Client, active: Option<&str>) -> SshStatus {
    let Some(command) = ssh_command().await else {
        return SshStatus::NotInstalled;
    };
    match probe(&command).await {
        Probe::NotInstalled => SshStatus::NotInstalled,
        Probe::TimedOut => SshStatus::Unreachable {
            message: format!(
                "no answer from {HOST} within {} seconds",
                PROBE_TIMEOUT.as_secs()
            ),
        },
        Probe::Authenticated(login) => greeted(login, active),
        Probe::Refused(stderr) => match classify(&stderr) {
            Some((SshProblem::KeyRejected, _)) => {
                let config = ssh_config(Some(&command)).await;
                let local = local_keys(&config.identity_files);
                let on_github = match active {
                    Some(login) => account_keys(http, login).await.unwrap_or_default(),
                    None => Vec::new(),
                };
                key_status(&local, &on_github)
            }
            Some((SshProblem::HostKeyUnknown, _)) => SshStatus::HostKeyUnknown,
            Some((SshProblem::HostKeyChanged, _)) => SshStatus::HostKeyChanged,
            Some((SshProblem::Unreachable, _)) => SshStatus::Unreachable {
                message: last_line(&stderr),
            },
            Some((SshProblem::NotInstalled, _)) => SshStatus::NotInstalled,
            _ => SshStatus::Failed {
                message: last_line(&stderr),
            },
        },
    }
}

/// GitHub greeted `login`: ready when it is gh's account (or gh has none to compare with).
fn greeted(login: Option<String>, active: Option<&str>) -> SshStatus {
    match (login, active) {
        (Some(login), Some(active))
            if login.contains('/') || !login.eq_ignore_ascii_case(active) =>
        {
            SshStatus::OtherAccount {
                login,
                active: Some(active.to_owned()),
            }
        }
        (Some(login), None) if login.contains('/') => SshStatus::OtherAccount {
            login,
            active: None,
        },
        (login, _) => SshStatus::Ready { login },
    }
}

/// GitHub turned every key down: a local key the account has wasn't offered, else no key
/// of this computer is on it (the first one there is, to add).
fn key_status(local: &[PublicKey], on_github: &[(String, String)]) -> SshStatus {
    let known = local.iter().find(|key| {
        on_github
            .iter()
            .any(|(kind, blob)| *kind == key.kind && *blob == key.blob)
    });
    match known {
        Some(key) => SshStatus::KeyNotOffered {
            local_key: key.path.display().to_string(),
        },
        None => SshStatus::NoKey {
            local_key: preferred(local).map(|key| key.path.display().to_string()),
        },
    }
}

/// ssh's last useful line.
fn last_line(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("Pseudo-terminal"))
        .unwrap_or_default()
        .to_owned()
}

/// What `ssh -G github.com` resolves from the user's ssh config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SshConfig {
    known_hosts: Vec<PathBuf>,
    identity_files: Vec<PathBuf>,
    hostname: Option<String>,
    port: Option<u16>,
}

impl SshConfig {
    /// The file `trust_github` writes: ssh's first user file, else `~/.ssh/known_hosts`.
    fn known_hosts_file(&self, home: Option<&Path>) -> Option<PathBuf> {
        self.known_hosts
            .first()
            .cloned()
            .or_else(|| home.map(|h| h.join(".ssh").join("known_hosts")))
    }

    /// How `known_hosts` names the server: `github.com`, or `[ssh.github.com]:443` for the
    /// port-443 endpoint some set up when port 22 is blocked.
    fn host_pattern(&self) -> String {
        let host = self.hostname.as_deref().unwrap_or(HOST);
        match self.port {
            Some(port) if port != 22 => format!("[{host}]:{port}"),
            _ => host.to_owned(),
        }
    }
}

/// Reads `ssh -G` (`key value` lines, keys in lowercase). `~` is the home folder, and with
/// `windows` an MSYS path (`/c/Users/a`, Git for Windows' ssh) becomes `C:/Users/a`.
fn parse_ssh_g(stdout: &str, home: Option<&Path>, windows: bool) -> SshConfig {
    let mut config = SshConfig::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.trim().split_once(' ') else {
            continue;
        };
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "hostname" => config.hostname = Some(value.to_owned()),
            "port" => config.port = value.parse().ok(),
            "identityfile" => config.identity_files.push(local_path(value, home, windows)),
            "userknownhostsfile" => config.known_hosts.extend(
                value
                    .split_whitespace()
                    .map(|path| local_path(path, home, windows)),
            ),
            _ => {}
        }
    }
    config
}

fn local_path(raw: &str, home: Option<&Path>, windows: bool) -> PathBuf {
    if let Some(home) = home {
        if raw == "~" {
            return home.to_path_buf();
        }
        if let Some(rest) = raw.strip_prefix("~/") {
            return home.join(rest);
        }
    }
    if windows {
        let bytes = raw.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b'/'
        {
            let drive = (bytes[1] as char).to_ascii_uppercase();
            return PathBuf::from(format!("{drive}:/{}", &raw[3..]));
        }
    }
    PathBuf::from(raw)
}

/// `ssh -G github.com` with `command`'s ssh (or PATH's, for one only git runs). Without an
/// answer: ssh's defaults under the home folder.
async fn ssh_config(command: Option<&SshCommand>) -> SshConfig {
    let home = platform::home_dir();
    let ssh = match command {
        Some(SshCommand::Direct(ssh)) => Some(ssh.clone()),
        _ => process::find_in_path("ssh"),
    };
    let read = match &ssh {
        Some(ssh) => process::run_with_timeout(
            &ssh.to_string_lossy(),
            ["-G", HOST],
            None,
            Duration::from_secs(10),
        )
        .await
        .ok()
        .map(|out| parse_ssh_g(&out.stdout, home.as_deref(), cfg!(windows))),
        None => None,
    };
    let mut config = read.unwrap_or_default();
    if config.identity_files.is_empty()
        && let Some(home) = &home
    {
        config.identity_files = DEFAULT_KEYS
            .iter()
            .map(|name| home.join(".ssh").join(name))
            .collect();
    }
    config
}

/// A public key file of this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicKey {
    /// The `.pub` file.
    path: PathBuf,
    kind: String,
    blob: String,
}

/// `ssh-ed25519 AAAA... comment` -> its type and data.
fn parse_public_key(line: &str) -> Option<(String, String)> {
    let mut words = line.split_whitespace();
    let kind = words.next()?;
    let blob = words.next()?;
    let valid_kind = kind
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'@'));
    let valid_blob = blob
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
    (valid_kind && valid_blob && blob.len() > 16).then(|| (kind.to_owned(), blob.to_owned()))
}

/// `<file>.pub` of each identity file that has one.
fn local_keys(identity_files: &[PathBuf]) -> Vec<PublicKey> {
    identity_files
        .iter()
        .filter_map(|file| {
            let mut path = file.clone().into_os_string();
            path.push(".pub");
            let path = PathBuf::from(path);
            let text = std::fs::read_to_string(&path).ok()?;
            let (kind, blob) = parse_public_key(text.lines().next()?)?;
            Some(PublicKey { path, kind, blob })
        })
        .collect()
}

/// The key to add to GitHub: Ed25519 first, then ECDSA, then RSA; never a security key's
/// (`sk-...`), which needs the device at each use.
fn preferred(keys: &[PublicKey]) -> Option<&PublicKey> {
    let rank = |key: &PublicKey| match key.kind.as_str() {
        "ssh-ed25519" => Some(0),
        kind if kind.starts_with("ecdsa-") => Some(1),
        "ssh-rsa" => Some(2),
        _ => None,
    };
    keys.iter()
        .filter_map(|key| Some((rank(key)?, key)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, key)| key)
}

/// The public keys on `login`'s account, as GitHub publishes them (`<type> <data>` lines).
async fn account_keys(http: &reqwest::Client, login: &str) -> Result<Vec<(String, String)>> {
    if !crate::student::valid_login(login) {
        return Ok(Vec::new());
    }
    let url = format!("https://github.com/{login}.keys");
    let text = http
        .get(crate::testenv::github_url(&url).as_ref())
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(Error::network)?
        .text()
        .await
        .map_err(Error::network)?;
    Ok(text.lines().filter_map(parse_public_key).collect())
}

/// The part of `GET /meta` the app reads.
#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    ssh_keys: Vec<String>,
}

/// GitHub's SSH host keys (`ssh_keys`, `<type> <data>`), each checked to be one before it
/// goes into `known_hosts`.
fn host_keys(meta: &Meta) -> Result<Vec<(String, String)>> {
    let keys: Vec<(String, String)> = meta
        .ssh_keys
        .iter()
        .filter_map(|key| parse_public_key(key))
        .collect();
    if keys.is_empty() {
        return Err(Error::GitHub("no SSH host keys in /meta".into()));
    }
    Ok(keys)
}

/// GitHub's SSH host keys, from `api.github.com/meta`.
async fn github_host_keys(http: &reqwest::Client) -> Result<Vec<(String, String)>> {
    let meta: Meta = http
        .get(crate::testenv::github_url("https://api.github.com/meta").as_ref())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(Error::network)?
        .json()
        .await
        .map_err(Error::network)?;
    host_keys(&meta)
}

/// `ssh-keygen -F` output to the keys it found (`<host or hash> <type> <data>`); comment
/// lines and marked ones (`@revoked`, `@cert-authority`) aside.
fn parse_found(stdout: &str) -> Vec<(String, String)> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('@'))
        .filter_map(|line| {
            let (_, key) = line.split_once(char::is_whitespace)?;
            parse_public_key(key.trim())
        })
        .collect()
}

/// The `known_hosts` lines to append: GitHub's keys `present` lacks, under `pattern`.
fn missing_host_keys(
    pattern: &str,
    github: &[(String, String)],
    present: &[(String, String)],
) -> Vec<String> {
    github
        .iter()
        .filter(|key| !present.contains(key))
        .map(|(kind, blob)| format!("{pattern} {kind} {blob}"))
        .collect()
}

/// `ssh-keygen`, next to the ssh git runs when that is a path (Git for Windows' own), else
/// PATH's.
fn keygen(command: Option<&SshCommand>) -> String {
    if let Some(SshCommand::Direct(ssh)) = command
        && ssh.is_absolute()
    {
        let name = if cfg!(windows) {
            "ssh-keygen.exe"
        } else {
            "ssh-keygen"
        };
        let sibling = ssh.with_file_name(name);
        if sibling.is_file() {
            return sibling.to_string_lossy().into_owned();
        }
    }
    "ssh-keygen".into()
}

/// The keys `known_hosts` holds for `pattern` (hashed lines included, which only
/// `ssh-keygen` can match).
async fn known_keys(keygen: &str, file: &Path, pattern: &str) -> Result<Vec<(String, String)>> {
    if !file.is_file() {
        return Ok(Vec::new());
    }
    let args = [
        OsString::from("-F"),
        OsString::from(pattern),
        OsString::from("-f"),
        file.as_os_str().to_owned(),
    ];
    match process::run_with_timeout(keygen, args, None, Duration::from_secs(10)).await {
        Ok(out) => Ok(parse_found(&out.stdout)),
        // Not found.
        Err(Error::CommandFailed { code: Some(1), .. }) => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

/// Appends `lines` to `file`, making its folder (`0700`) and the file (`0600`) when they
/// don't exist; an existing file keeps its mode.
fn append_lines(file: &Path, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    if let Some(dir) = file.parent()
        && !dir.exists()
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }
    let ends_open = std::fs::read(file)
        .ok()
        .is_some_and(|bytes| bytes.last().is_some_and(|b| *b != b'\n'));
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(file)?;
    let mut text = String::new();
    if ends_open {
        text.push('\n');
    }
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    out.write_all(text.as_bytes())?;
    Ok(())
}

/// Adds GitHub's published host keys `known_hosts` lacks (none removed).
pub async fn trust_github(http: &reqwest::Client) -> Result<()> {
    let command = ssh_command().await;
    let config = ssh_config(command.as_ref()).await;
    let file = config
        .known_hosts_file(platform::home_dir().as_deref())
        .ok_or_else(|| Error::Unsupported("no home folder for ~/.ssh".into()))?;
    let github = github_host_keys(http).await?;
    let pattern = config.host_pattern();
    let keygen = keygen(command.as_ref());
    let present = known_keys(&keygen, &file, &pattern).await?;
    append_lines(&file, &missing_host_keys(&pattern, &github, &present))
}

/// Removes every `known_hosts` line for GitHub (`ssh-keygen -R`, which keeps the old file as
/// `known_hosts.old`), then adds GitHub's published keys.
pub async fn replace_github_host_key(http: &reqwest::Client) -> Result<()> {
    let command = ssh_command().await;
    let config = ssh_config(command.as_ref()).await;
    let file = config
        .known_hosts_file(platform::home_dir().as_deref())
        .ok_or_else(|| Error::Unsupported("no home folder for ~/.ssh".into()))?;
    // Read first: nothing is removed when GitHub can't be asked for the new keys.
    let github = github_host_keys(http).await?;
    let pattern = config.host_pattern();
    let keygen = keygen(command.as_ref());
    if file.is_file() {
        let args = [
            OsString::from("-R"),
            OsString::from(&pattern),
            OsString::from("-f"),
            file.as_os_str().to_owned(),
        ];
        process::run_with_timeout(&keygen, args, None, Duration::from_secs(10)).await?;
    }
    let present = known_keys(&keygen, &file, &pattern).await?;
    append_lines(&file, &missing_host_keys(&pattern, &github, &present))
}

/// The arguments of `gh ssh-key add` for `public`.
fn add_key_args(public: &Path) -> Vec<OsString> {
    vec![
        "ssh-key".into(),
        "add".into(),
        public.as_os_str().to_owned(),
        "--title".into(),
        KEY_TITLE.into(),
        "--type".into(),
        "authentication".into(),
    ]
}

/// Puts a key of this computer on the active account: an existing public key ssh would
/// use, else a new `~/.ssh/id_ed25519` without a passphrase (only when neither file exists),
/// added with `gh ssh-key add`. Then probes again.
pub async fn set_up_key(http: &reqwest::Client, active: Option<&str>) -> Result<SshStatus> {
    // Dev builds refuse the upload; refuse before a key is made.
    process::ensure_allowed("gh", add_key_args(Path::new("key.pub")))?;
    let command = ssh_command().await;
    let config = ssh_config(command.as_ref()).await;
    let home = platform::home_dir();
    let mut candidates = config.identity_files.clone();
    if let Some(home) = &home {
        for name in DEFAULT_KEYS {
            let path = home.join(".ssh").join(name);
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    let local = local_keys(&candidates);
    let public = match preferred(&local) {
        Some(key) => key.path.clone(),
        None => {
            let home =
                home.ok_or_else(|| Error::Unsupported("no home folder for ~/.ssh".into()))?;
            generate(
                &keygen(command.as_ref()),
                &home.join(".ssh").join("id_ed25519"),
            )
            .await?
        }
    };
    add_key(&public).await?;
    Ok(status(http, active).await)
}

/// `ssh-keygen -t ed25519` at `private`, never over a key: an existing `.pub` would have
/// been reused, a lone private key is `PrivateKeyOnly`. Returns the `.pub`.
async fn generate(keygen: &str, private: &Path) -> Result<PathBuf> {
    let mut public = private.as_os_str().to_owned();
    public.push(".pub");
    let public = PathBuf::from(public);
    if private.exists() || public.exists() {
        return Err(Error::Ssh {
            problem: SshProblem::PrivateKeyOnly,
            login: None,
        });
    }
    if let Some(dir) = private.parent()
        && !dir.exists()
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }
    let args = [
        OsString::from("-t"),
        "ed25519".into(),
        "-N".into(),
        "".into(),
        "-f".into(),
        private.as_os_str().to_owned(),
        "-C".into(),
        KEY_COMMENT.into(),
        "-q".into(),
    ];
    process::run_with_timeout(keygen, args, None, KEYGEN_TIMEOUT).await?;
    Ok(public)
}

async fn add_key(public: &Path) -> Result<()> {
    match process::run_with_timeout("gh", add_key_args(public), None, ADD_KEY_TIMEOUT).await {
        Ok(_) => Ok(()),
        Err(Error::CommandFailed {
            program,
            code,
            stderr,
        }) => Err(add_key_failure(program, code, stderr)),
        Err(err) => Err(err),
    }
}

/// gh's refusal of `gh ssh-key add`, typed where the UI has a fix.
fn add_key_failure(program: String, code: Option<i32>, stderr: String) -> Error {
    let ssh = |problem| Error::Ssh {
        problem,
        login: None,
    };
    if code == Some(4) {
        return Error::NotSignedIn;
    }
    // `This API operation needs the "admin:public_key" scope`; GitHub answers 404 to a
    // token without it.
    if stderr.contains("public_key") || stderr.contains("HTTP 404") {
        return ssh(SshProblem::NeedsScope);
    }
    if stderr.contains("key is already in use") {
        return ssh(SshProblem::KeyInUse);
    }
    Error::CommandFailed {
        program,
        code,
        stderr,
    }
}

/// The last status, reused for `STATUS_COOLDOWN` since each probe connects to GitHub.
/// Every fix (`trust_github`, `set_up_key`) and protocol change forgets it.
#[derive(Default)]
pub struct SshState {
    last: tokio::sync::Mutex<Option<Recent<SshStatus>>>,
}

impl SshState {
    /// The last status while fresh, unless `fresh`; else `read`'s, kept. The lock is held
    /// across the read, so calls that overlap share one probe.
    pub async fn status(&self, fresh: bool, read: impl Future<Output = SshStatus>) -> SshStatus {
        let mut last = self.last.lock().await;
        if !fresh
            && let Some(status) = last
                .as_ref()
                .and_then(|recent| recent.fresh(Instant::now(), STATUS_COOLDOWN))
        {
            return status.clone();
        }
        let status = read.await;
        *last = Some(Recent::new(Instant::now(), status.clone()));
        status
    }

    pub async fn store(&self, status: SshStatus) {
        *self.last.lock().await = Some(Recent::new(Instant::now(), status));
    }

    pub async fn clear(&self) {
        *self.last.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
    const EC: &str = "AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=";
    /// GitHub's RSA host key from before March 2023.
    const OLD_RSA: &str = "AAAAB3NzaC1yc2EAAAABIwAAAQEAq2A7hRGmdnm9tUDbO9IDSwBK6TbQa+PXYPCPy6rbTrTtw7PHkccKrpp0yVhp5HdEIcKr6pLlVDBfOLX9QUsyCOV0wzfjIJNlGEYsdlLJizHhbn2mUjvSAHQqZETYP81eFzLQNnPHt4EVVUh7VfDESU84KezmD5QlWpXLmvU31/yMf+Se8xhHTvKSCZIFImWwoG6mbUoWf9nzpIoaSjB+weqqUUmpaaasXVal72J+UX2B+2RPW3RcT0eOzQgqlJL3RKrTJvdsjE3JEAvGq3lGHSZXy28G3skua2SmVi/w4yCE6gbODqnTWlg7+wC604ydGXA8VJiS5ap43JXiUFFAaQ==";

    /// Real output (OpenSSH 10.2, git 2.50) for each way SSH to GitHub fails.
    #[test]
    fn classifies_ssh_failures() {
        let changed = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
            @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
            @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
            Offending ED25519 key in /home/a/.ssh/known_hosts:1\n\
            Host key for github.com has changed and you have requested strict checking.\n\
            Host key verification failed.\n\
            fatal: Could not read from remote repository.\n";
        let changed_elsewhere =
            changed.replace("Host key for github.com", "Host key for gitlab.com");
        type Case<'a> = (&'a str, Option<(SshProblem, Option<&'a str>)>);
        let cases: [Case; 22] = [
            (changed, Some((SshProblem::HostKeyChanged, None))),
            (
                "Host key verification failed.\r\nfatal: Could not read from remote repository.\r\n",
                Some((SshProblem::HostKeyUnknown, None)),
            ),
            (
                "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n",
                Some((SshProblem::KeyRejected, None)),
            ),
            (
                "ERROR: Permission to acme-cs/lab-2-alex.git denied to bob-other.\nfatal: Could not read from remote repository.\n",
                Some((SshProblem::OtherAccount, Some("bob-other"))),
            ),
            (
                "ERROR: Repository not found.\nfatal: Could not read from remote repository.\n",
                Some((SshProblem::OtherAccount, None)),
            ),
            (
                "ssh: connect to host github.com port 22: Operation timed out\n",
                Some((SshProblem::Unreachable, None)),
            ),
            (
                "ssh: connect to host github.com port 22: Connection refused\n",
                Some((SshProblem::Unreachable, None)),
            ),
            (
                "ssh: Could not resolve hostname github.com: nodename nor servname provided, or not known\n",
                Some((SshProblem::Unreachable, None)),
            ),
            (
                "kex_exchange_identification: Connection closed by remote host\nConnection closed by 140.82.121.4 port 22\n",
                Some((SshProblem::Unreachable, None)),
            ),
            (
                "error: cannot run ssh: No such file or directory\nfatal: unable to fork\n",
                Some((SshProblem::NotInstalled, None)),
            ),
            // HTTPS failures are not SSH ones.
            (
                "remote: Repository not found.\nfatal: repository 'https://github.com/o/r.git/' not found\n",
                None,
            ),
            (
                "remote: Permission to o/r.git denied to bob.\nfatal: unable to access 'https://github.com/o/r.git/': The requested URL returned error: 403\n",
                None,
            ),
            // SSH commit signing without ssh-keygen is no missing ssh.
            (
                "error: cannot run ssh-keygen: No such file or directory\nfatal: failed to write commit object\n",
                None,
            ),
            (
                "error: cannot spawn ssh: No such file or directory\n",
                Some((SshProblem::NotInstalled, None)),
            ),
            // Another server (a submodule, a hook) is no GitHub SSH problem.
            (
                "ssh: connect to host gitlab.com port 22: Connection refused\n",
                None,
            ),
            (
                "ssh: Could not resolve hostname gitlab.example.org: Name or service not known\n",
                None,
            ),
            (
                "git@gitlab.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n",
                None,
            ),
            (
                "No ED25519 host key is known for gitlab.com and you have requested strict checking.\nHost key verification failed.\n",
                None,
            ),
            (changed_elsewhere.as_str(), None),
            // GitHub's port-443 endpoint is GitHub.
            (
                "ssh: connect to host ssh.github.com port 443: Operation timed out\n",
                Some((SshProblem::Unreachable, None)),
            ),
            (
                "No ED25519 host key is known for [ssh.github.com]:443 and you have requested strict checking.\nHost key verification failed.\n",
                Some((SshProblem::HostKeyUnknown, None)),
            ),
            (
                "ERROR: Permission to acme-cs/lab-2-alex.git denied to alice.\nfatal: Could not read from remote repository.\n",
                Some((SshProblem::OtherAccount, Some("alice"))),
            ),
        ];
        for (stderr, expected) in cases {
            let expected = expected.map(|(p, login)| (p, login.map(str::to_owned)));
            assert_eq!(classify(stderr), expected, "{stderr}");
        }
        assert_eq!(
            classify(
                "fatal: unable to access 'https://github.com/o/r/': Could not resolve host: github.com"
            ),
            None
        );
    }

    #[test]
    fn keeps_githubs_own_refusal() {
        assert_eq!(
            github_error(
                "ERROR: This repository was archived so it is read-only.\nfatal: Could not read from remote repository.\n"
            )
            .as_deref(),
            Some("This repository was archived so it is read-only.")
        );
        assert_eq!(
            github_error("fatal: Could not read from remote repository.\n"),
            None
        );
    }

    /// GitHub naming an account is the wrong key only when it isn't gh's.
    #[tokio::test]
    async fn blames_ssh_only_for_another_accounts_key() {
        let who = |active: Option<&str>, probe: SshStatus| Who::Known {
            active: active.map(str::to_owned),
            probe,
        };
        let ready = SshStatus::Ready {
            login: Some("alice".into()),
        };
        let bob = SshStatus::OtherAccount {
            login: "bob".into(),
            active: Some("alice".into()),
        };
        let denied = |to: &str| {
            format!(
                "ERROR: Permission to acme-cs/lab-2-alex.git denied to {to}.\nfatal: Could not read from remote repository.\n"
            )
        };
        let not_found =
            "ERROR: Repository not found.\nfatal: Could not read from remote repository.\n";
        let other = |err: Option<Error>| match err {
            Some(Error::Ssh {
                problem: SshProblem::OtherAccount,
                login,
            }) => login,
            err => panic!("{err:?}"),
        };
        // gh's own account: no write access (a read-only repo), not SSH's fault.
        assert!(
            failure(&denied("ALICE"), &who(Some("alice"), bob.clone()))
                .await
                .is_none()
        );
        assert_eq!(
            other(failure(&denied("bob"), &who(Some("alice"), ready.clone())).await).as_deref(),
            Some("bob")
        );
        // Without gh's account nothing is blamed.
        assert!(
            failure(&denied("bob"), &who(None, bob.clone()))
                .await
                .is_none()
        );
        // Not found: the probe tells.
        assert!(
            failure(not_found, &who(Some("alice"), ready.clone()))
                .await
                .is_none()
        );
        assert!(
            failure(
                not_found,
                &who(Some("alice"), SshStatus::Ready { login: None })
            )
            .await
            .is_none()
        );
        assert_eq!(
            other(failure(not_found, &who(Some("alice"), bob)).await).as_deref(),
            Some("bob")
        );
        assert!(matches!(
            failure("Host key verification failed.", &who(None, ready)).await,
            Some(Error::Ssh {
                problem: SshProblem::HostKeyUnknown,
                login: None
            })
        ));
    }

    #[test]
    fn reads_githubs_greeting() {
        assert_eq!(
            parse_greeting(
                "Hi RomainAntunes! You've successfully authenticated, but GitHub does not provide shell access."
            )
            .as_deref(),
            Some("RomainAntunes")
        );
        // A deploy key.
        assert_eq!(
            parse_greeting("Hi acme-cs/lab-2-alex! You've successfully authenticated, but GitHub does not provide shell access.")
                .as_deref(),
            Some("acme-cs/lab-2-alex")
        );
        assert_eq!(parse_greeting("Hi there! How are you?"), None);
        assert_eq!(parse_greeting("Permission denied (publickey)."), None);
    }

    #[test]
    fn a_greeting_is_ready_for_ghs_account_only() {
        let ready = |login: &str| SshStatus::Ready {
            login: Some(login.into()),
        };
        assert_eq!(greeted(Some("Alice".into()), Some("alice")), ready("Alice"));
        assert_eq!(greeted(Some("alice".into()), None), ready("alice"));
        assert_eq!(
            greeted(None, Some("alice")),
            SshStatus::Ready { login: None }
        );
        assert_eq!(
            greeted(Some("bob".into()), Some("alice")),
            SshStatus::OtherAccount {
                login: "bob".into(),
                active: Some("alice".into())
            }
        );
        assert_eq!(
            greeted(Some("o/r".into()), None),
            SshStatus::OtherAccount {
                login: "o/r".into(),
                active: None
            }
        );
    }

    #[test]
    fn picks_ssh_in_gits_order() {
        let path = Some(PathBuf::from("/usr/bin/ssh"));
        let direct = |p: &str| Some(SshCommand::Direct(PathBuf::from(p)));
        assert_eq!(
            pick_command(None, None, None, None, path.clone()),
            direct("/usr/bin/ssh")
        );
        assert_eq!(pick_command(None, None, None, None, None), None);
        // Git for Windows' own ssh before PATH's.
        let bundled = Some(PathBuf::from(r"C:\Program Files\Git\usr\bin\ssh.exe"));
        assert_eq!(
            pick_command(None, None, None, bundled.clone(), path.clone()),
            direct(r"C:\Program Files\Git\usr\bin\ssh.exe")
        );
        // GIT_SSH_COMMAND, then core.sshCommand, then GIT_SSH.
        assert_eq!(
            pick_command(
                Some("ssh -i ~/.ssh/school"),
                Some("ssh"),
                None,
                None,
                path.clone()
            ),
            Some(SshCommand::ThroughGit)
        );
        assert_eq!(
            pick_command(Some(" "), Some("/opt/ssh"), None, None, path.clone()),
            direct("/opt/ssh")
        );
        assert_eq!(
            pick_command(None, Some("plink"), None, None, path.clone()),
            Some(SshCommand::ThroughGit)
        );
        assert_eq!(
            pick_command(None, None, Some(r"C:\PuTTY\plink.exe"), None, path.clone()),
            Some(SshCommand::ThroughGit)
        );
        assert_eq!(
            pick_command(
                None,
                None,
                Some("C:/Windows/System32/OpenSSH/ssh.exe"),
                bundled,
                path
            ),
            direct("C:/Windows/System32/OpenSSH/ssh.exe")
        );
    }

    #[test]
    fn finds_git_for_windows_own_ssh() {
        let root = Path::new("C:/Program Files/Git");
        let ssh = root.join("usr").join("bin").join("ssh.exe");
        for git in ["cmd/git.exe", "bin/git.exe", "mingw64/bin/git.exe"] {
            assert_eq!(bundled_ssh(&root.join(git)), Some(ssh.clone()), "{git}");
        }
        assert_eq!(
            bundled_ssh(Path::new("/opt/homebrew/opt/git/libexec/git")),
            None
        );
    }

    #[test]
    fn reads_ssh_g() {
        let out = "user romain\nhostname github.com\nport 22\nbatchmode no\n\
            identityfile ~/.ssh/id_rsa\nidentityfile ~/.ssh/id_ed25519\n\
            globalknownhostsfile /etc/ssh/ssh_known_hosts /etc/ssh/ssh_known_hosts2\n\
            userknownhostsfile /home/a/.ssh/known_hosts /home/a/.ssh/known_hosts2\n";
        let home = Path::new("/home/a");
        let config = parse_ssh_g(out, Some(home), false);
        assert_eq!(
            config.identity_files,
            [home.join(".ssh/id_rsa"), home.join(".ssh/id_ed25519")]
        );
        assert_eq!(
            config.known_hosts[0],
            PathBuf::from("/home/a/.ssh/known_hosts")
        );
        assert_eq!(config.host_pattern(), "github.com");
        assert_eq!(
            config.known_hosts_file(None),
            Some(PathBuf::from("/home/a/.ssh/known_hosts"))
        );
        // Port 443 through ssh.github.com, from the user's config.
        let config = parse_ssh_g("hostname ssh.github.com\nport 443\n", None, false);
        assert_eq!(config.host_pattern(), "[ssh.github.com]:443");
        assert_eq!(
            config.known_hosts_file(Some(home)),
            Some(home.join(".ssh").join("known_hosts"))
        );
        // Git for Windows' ssh writes MSYS paths.
        let config = parse_ssh_g(
            "userknownhostsfile /c/Users/a/.ssh/known_hosts ~/.ssh/known_hosts2\n",
            Some(Path::new("C:/Users/a")),
            true,
        );
        assert_eq!(
            config.known_hosts,
            [
                PathBuf::from("C:/Users/a/.ssh/known_hosts"),
                Path::new("C:/Users/a").join(".ssh/known_hosts2")
            ]
        );
    }

    fn key(path: &str, kind: &str, blob: &str) -> PublicKey {
        PublicKey {
            path: path.into(),
            kind: kind.into(),
            blob: blob.into(),
        }
    }

    #[test]
    fn tells_a_missing_key_from_one_ssh_didnt_offer() {
        let rsa = key("/h/.ssh/id_rsa.pub", "ssh-rsa", OLD_RSA);
        let ed = key("/h/.ssh/id_ed25519.pub", "ssh-ed25519", ED);
        let on_github = [("ssh-ed25519".to_owned(), ED.to_owned())];
        assert_eq!(
            key_status(&[rsa.clone(), ed.clone()], &on_github),
            SshStatus::KeyNotOffered {
                local_key: "/h/.ssh/id_ed25519.pub".into()
            }
        );
        // Not on GitHub: the Ed25519 key is the one to add.
        assert_eq!(
            key_status(&[rsa.clone(), ed], &[]),
            SshStatus::NoKey {
                local_key: Some("/h/.ssh/id_ed25519.pub".into())
            }
        );
        assert_eq!(
            key_status(&[rsa], &[]),
            SshStatus::NoKey {
                local_key: Some("/h/.ssh/id_rsa.pub".into())
            }
        );
        assert_eq!(
            key_status(&[], &on_github),
            SshStatus::NoKey { local_key: None }
        );
        // A security key is never the one to add.
        let sk = key(
            "/h/.ssh/id_ed25519_sk.pub",
            "sk-ssh-ed25519@openssh.com",
            ED,
        );
        assert_eq!(preferred(&[sk]), None);
    }

    #[test]
    fn reads_public_keys() {
        assert_eq!(
            parse_public_key(&format!("ssh-ed25519 {ED} alice@laptop")),
            Some(("ssh-ed25519".into(), ED.into()))
        );
        assert_eq!(parse_public_key("ssh-ed25519"), None);
        assert_eq!(parse_public_key("ssh-ed25519 not\nbase64!"), None);
        assert_eq!(parse_public_key("<html> <body>"), None);
    }

    /// The mock GitHub's `/meta` (`e2e/mock/golden/meta.json`), read as GitHub's.
    #[test]
    fn reads_githubs_host_keys_from_meta() {
        let meta: Meta = serde_json::from_str(&crate::testenv::golden("meta")).unwrap();
        let keys = host_keys(&meta).unwrap();
        let kinds: Vec<&str> = keys.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds, ["ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"]);
        assert_eq!(keys[0].1, ED);
        let empty: Meta = serde_json::from_str(r#"{"ssh_keys":["not a key\nat all"]}"#).unwrap();
        assert!(host_keys(&empty).is_err());
        let none: Meta = serde_json::from_str("{}").unwrap();
        assert!(host_keys(&none).is_err());
    }

    #[test]
    fn adds_only_the_host_keys_known_hosts_lacks() {
        let found = format!(
            "# Host github.com found: line 1 \n\
             |1|P4WQD+FqD5/tPoiXtCVIy0Zcprw=|j0tsoUWvKkfjbDNksh3wcDi7knA= ssh-ed25519 {ED}\n\
             # Host github.com found: line 3 \n\
             github.com ssh-rsa {OLD_RSA}\n\
             @revoked github.com ecdsa-sha2-nistp256 {EC}\n"
        );
        let present = parse_found(&found);
        assert_eq!(
            present,
            [
                ("ssh-ed25519".to_owned(), ED.to_owned()),
                ("ssh-rsa".to_owned(), OLD_RSA.to_owned())
            ]
        );
        let github = [
            ("ssh-ed25519".to_owned(), ED.to_owned()),
            ("ecdsa-sha2-nistp256".to_owned(), EC.to_owned()),
        ];
        assert_eq!(
            missing_host_keys("github.com", &github, &present),
            [format!("github.com ecdsa-sha2-nistp256 {EC}")]
        );
        assert!(missing_host_keys("github.com", &github, &github).is_empty());
    }

    #[test]
    fn appends_to_known_hosts_with_safe_modes() {
        let root = std::env::temp_dir().join(format!("c50tb-kh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join(".ssh").join("known_hosts");
        append_lines(&file, &[format!("github.com ssh-ed25519 {ED}")]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&root.join(".ssh")), 0o700);
            assert_eq!(mode(&file), 0o600);
            // An existing file keeps its mode.
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        // A last line without its newline gets one first.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        f.write_all(b"example.com ssh-ed25519 AAAA").unwrap();
        append_lines(&file, &["github.com ssh-rsa X".into()]).unwrap();
        append_lines(&file, &[]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            text,
            format!(
                "github.com ssh-ed25519 {ED}\nexample.com ssh-ed25519 AAAA\ngithub.com ssh-rsa X\n"
            )
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o644);
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The real `ssh-keygen` on a temp `known_hosts`: hashed and plain lines are found.
    #[cfg(unix)]
    #[tokio::test]
    async fn finds_host_keys_with_ssh_keygen() {
        if process::find_in_path("ssh-keygen").is_none() {
            return;
        }
        let root = std::env::temp_dir().join(format!("c50tb-keygen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("known_hosts");
        assert!(
            known_keys("ssh-keygen", &file, "github.com")
                .await
                .unwrap()
                .is_empty()
        );
        std::fs::write(
            &file,
            format!("example.com ssh-ed25519 {ED}\ngithub.com ssh-rsa {OLD_RSA}\n"),
        )
        .unwrap();
        assert_eq!(
            known_keys("ssh-keygen", &file, "github.com").await.unwrap(),
            [("ssh-rsa".to_owned(), OLD_RSA.to_owned())]
        );
        assert!(
            known_keys("ssh-keygen", &file, "[ssh.github.com]:443")
                .await
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn types_gh_ssh_key_add_refusals() {
        let failed = |code, stderr: &str| add_key_failure("gh".into(), Some(code), stderr.into());
        assert!(matches!(
            failed(
                1,
                "HTTP 404: Not Found (https://api.github.com/user/keys)\nThis API operation needs the \"admin:public_key\" scope. To request it, run:  gh auth refresh -h github.com -s admin:public_key"
            ),
            Error::Ssh {
                problem: SshProblem::NeedsScope,
                ..
            }
        ));
        assert!(matches!(
            failed(
                1,
                "HTTP 422: Validation Failed (https://api.github.com/user/keys)\nkey is already in use"
            ),
            Error::Ssh {
                problem: SshProblem::KeyInUse,
                ..
            }
        ));
        assert!(matches!(
            failed(
                4,
                "To get started with GitHub CLI, please run:  gh auth login"
            ),
            Error::NotSignedIn
        ));
        assert!(matches!(failed(1, "boom"), Error::CommandFailed { .. }));
    }

    #[test]
    fn dev_builds_refuse_adding_a_key_before_making_one() {
        let refused =
            process::ensure_allowed("gh", add_key_args(Path::new("/h/.ssh/id_ed25519.pub")));
        assert_eq!(
            cfg!(debug_assertions),
            matches!(refused, Err(Error::BlockedInDev(_)))
        );
    }

    #[tokio::test]
    async fn generates_a_key_only_where_none_is() {
        let root = std::env::temp_dir().join(format!("c50tb-genkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let private = root.join(".ssh").join("id_ed25519");
        std::fs::create_dir_all(root.join(".ssh")).unwrap();
        std::fs::write(&private, "secret").unwrap();
        // A lone private key is never replaced.
        assert!(matches!(
            generate("ssh-keygen", &private).await,
            Err(Error::Ssh {
                problem: SshProblem::PrivateKeyOnly,
                ..
            })
        ));
        assert_eq!(std::fs::read_to_string(&private).unwrap(), "secret");
        std::fs::remove_file(&private).unwrap();
        if process::find_in_path("ssh-keygen").is_some() {
            let public = generate("ssh-keygen", &private).await.unwrap();
            let text = std::fs::read_to_string(&public).unwrap();
            assert!(text.starts_with("ssh-ed25519 "), "{text}");
            assert!(text.trim_end().ends_with(KEY_COMMENT), "{text}");
            assert_eq!(local_keys(std::slice::from_ref(&private)).len(), 1);
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn reuses_a_status_for_a_minute_unless_asked_fresh() {
        let state = SshState::default();
        let ready = SshStatus::Ready { login: None };
        assert_eq!(state.status(false, async { ready.clone() }).await, ready);
        let other = SshStatus::HostKeyUnknown;
        assert_eq!(state.status(false, async { other.clone() }).await, ready);
        assert_eq!(state.status(true, async { other.clone() }).await, other);
        state.clear().await;
        assert_eq!(state.status(false, async { ready.clone() }).await, ready);
    }

    /// The suite's fake ssh (`e2e/bin/ssh`) answers like GitHub's.
    #[cfg(unix)]
    #[test]
    fn the_suites_fake_ssh_reads_like_ssh() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../e2e/bin/ssh");
        let run = |args: &[&str]| {
            std::process::Command::new("sh")
                .arg(script)
                .args(args)
                .env_remove("C50_FAKE_SSH")
                .env("HOME", "/home/ada")
                .output()
                .unwrap()
        };
        let out = run(&[
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            TARGET,
        ]);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            parse_greeting(&String::from_utf8_lossy(&out.stderr)).as_deref(),
            Some("ada-e2e")
        );
        let out = run(&["-G", HOST]);
        let config = parse_ssh_g(&String::from_utf8_lossy(&out.stdout), None, false);
        assert_eq!(
            config.known_hosts.first(),
            Some(&PathBuf::from("/home/ada/.ssh/known_hosts"))
        );
        assert_eq!(config.host_pattern(), "github.com");
    }
}
