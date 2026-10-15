use std::collections::BTreeMap;

use serde::{Serialize, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{program}` is not installed or not on PATH")]
    NotInstalled { program: String },

    #[error("`{program}` exited with code {code:?}: {stderr}")]
    CommandFailed {
        program: String,
        code: Option<i32>,
        stderr: String,
    },

    /// A long command (an install) ran past its time limit and was stopped.
    #[error("`{program}` was stopped after {minutes} minutes without finishing: {output}")]
    TimedOut {
        program: String,
        minutes: u64,
        output: String,
    },

    #[error("{0}")]
    Unsupported(String),

    /// The app can't install a tool here (no package manager, no way to ask for a password);
    /// the user does it by hand, from `url` or with `command` in a terminal.
    #[error("{reason}")]
    ManualInstall {
        reason: ManualReason,
        url: Option<String>,
        command: Option<String>,
    },

    #[error("`{0}` is disabled in development builds")]
    BlockedInDev(String),

    #[error("cancelled")]
    Cancelled,

    /// The same job (an install of the same tool) is already running.
    #[error("already running")]
    Busy,

    #[error("{} already exists", .0.display())]
    PathExists(std::path::PathBuf),

    #[error("{url} uses schema {found:?}, expected {expected:?}; update the toolbox")]
    SchemaMismatch {
        url: String,
        found: String,
        expected: &'static str,
    },

    /// `gh student` refused; the message is meant for the student.
    #[error("{0}")]
    Student(String),

    #[error("you are not in a group for this assignment yet")]
    NeedsTeam,

    /// Submit refused: the teacher closed submissions or archived the repo.
    #[error("submissions are closed for this assignment")]
    SubmissionsClosed,

    /// Not a GitHub login, checked before it reaches `gh student`.
    #[error("\"{0}\" isn't a GitHub username")]
    InvalidUsername(String),

    /// Linux: none of the terminal emulators the app knows is installed.
    #[error("no terminal emulator found")]
    NoTerminal,

    /// Pull refused: uncommitted changes to tracked files.
    #[error("the clone has uncommitted changes")]
    LocalChanges,

    /// Pull refused: the clone isn't on the default branch (named here).
    #[error("the clone isn't on {0}")]
    OtherBranch(String),

    /// Pull refused: the clone and GitHub both have new commits.
    #[error("the clone and GitHub have both changed")]
    Diverged,

    #[error("not signed in to GitHub")]
    NotSignedIn,

    /// gh takes its token from this environment variable, so it can't store a sign-in.
    #[error("the GitHub CLI is using the {0} environment variable; unset it to sign in here")]
    TokenFromEnv(String),

    #[error("GitHub rate limit reached{}", retry_at(reset_at))]
    RateLimited {
        reset_at: Option<chrono::DateTime<chrono::Utc>>,
    },

    #[error("GitHub API: {0}")]
    GitHub(String),

    #[error("can't reach GitHub")]
    Offline,

    /// The OS's calendar store refused access (never granted, or turned off since).
    #[error("calendar access isn't allowed")]
    CalendarAccess,

    #[error("invalid .classroom50.yaml: {0}")]
    RepoConfig(#[from] serde_norway::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    #[error(transparent)]
    Updater(#[from] tauri_plugin_updater::Error),

    #[error(transparent)]
    Autostart(#[from] tauri_plugin_autostart::Error),
}

/// Why the app can't install a tool itself (`Error::ManualInstall`); the UI words it as
/// `setup.manualReason.<code>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualReason {
    NoHomebrew,
    NoWinget,
    OldWinget,
    ReadOnlySystem,
    NoPackageManager,
    NoPkexec,
}

impl ManualReason {
    pub const ALL: [Self; 6] = [
        Self::NoHomebrew,
        Self::NoWinget,
        Self::OldWinget,
        Self::ReadOnlySystem,
        Self::NoPackageManager,
        Self::NoPkexec,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::NoHomebrew => "noHomebrew",
            Self::NoWinget => "noWinget",
            Self::OldWinget => "oldWinget",
            Self::ReadOnlySystem => "readOnlySystem",
            Self::NoPackageManager => "noPackageManager",
            Self::NoPkexec => "noPkexec",
        }
    }
}

impl std::fmt::Display for ManualReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoHomebrew => "Homebrew isn't installed",
            Self::NoWinget => "winget (App Installer) isn't installed",
            Self::OldWinget => {
                "winget is out of date; update App Installer from the Microsoft Store"
            }
            Self::ReadOnlySystem => {
                "this system's packages are read-only; install it with Homebrew or in a toolbox"
            }
            Self::NoPackageManager => {
                "no supported package manager found (apt, dnf, pacman, zypper)"
            }
            Self::NoPkexec => {
                "pkexec (polkit) isn't installed, so the app can't ask for your password"
            }
        })
    }
}

/// ", retrying after 14:05 UTC", or nothing when GitHub didn't say.
fn retry_at(reset_at: &Option<chrono::DateTime<chrono::Utc>>) -> String {
    reset_at
        .map(|at| format!(", retrying after {}", at.format("%H:%M UTC")))
        .unwrap_or_default()
}

impl Error {
    /// What the UI fills into its own wording of this error (`error.<kind>` in `en.json`),
    /// which must say what `Display` says: the message is the fallback for kinds the UI
    /// doesn't word.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            Error::NotInstalled { program } => vec![("program", program.clone())],
            Error::BlockedInDev(command) => vec![("command", command.clone())],
            Error::PathExists(path) => vec![("path", path.display().to_string())],
            Error::OtherBranch(branch) => vec![("branch", branch.clone())],
            Error::TokenFromEnv(var) => vec![("var", var.clone())],
            Error::InvalidUsername(name) => vec![("username", name.clone())],
            Error::ManualInstall { reason, .. } => vec![("reason", reason.code().to_owned())],
            _ => Vec::new(),
        }
    }

    /// A request that never got an answer (no network, DNS, timeout) means offline; other
    /// HTTP errors stay as they are.
    pub fn network(err: reqwest::Error) -> Self {
        if err.is_connect() || err.is_timeout() {
            Error::Offline
        } else {
            Error::Http(err)
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Error::NotInstalled { .. } => "notInstalled",
            Error::CommandFailed { .. } => "commandFailed",
            Error::TimedOut { .. } => "timedOut",
            Error::Unsupported(_) => "unsupported",
            Error::ManualInstall { .. } => "manualInstall",
            Error::BlockedInDev(_) => "blockedInDev",
            Error::Cancelled => "cancelled",
            Error::Busy => "busy",
            Error::PathExists(_) => "pathExists",
            Error::SchemaMismatch { .. } => "schemaMismatch",
            Error::Student(_) => "student",
            Error::NeedsTeam => "needsTeam",
            Error::SubmissionsClosed => "submissionsClosed",
            Error::InvalidUsername(_) => "invalidUsername",
            Error::NoTerminal => "noTerminal",
            Error::LocalChanges => "localChanges",
            Error::OtherBranch(_) => "otherBranch",
            Error::Diverged => "diverged",
            Error::NotSignedIn => "notSignedIn",
            Error::TokenFromEnv(_) => "tokenFromEnv",
            Error::RateLimited { .. } => "rateLimited",
            Error::GitHub(_) => "github",
            Error::Offline => "offline",
            Error::CalendarAccess => "calendarAccess",
            Error::RepoConfig(_) => "repoConfig",
            Error::Io(_) => "io",
            Error::Http(_) => "http",
            Error::Tauri(_) => "tauri",
            Error::Updater(_) => "updater",
            Error::Autostart(_) => "autostart",
        }
    }
}

/// Errors cross the IPC boundary as `{ kind, message, params }` so the UI can branch on
/// `kind` and show its own wording (`error.<kind>` filled with `params`), else `message`.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Payload<'a> {
            kind: &'a str,
            message: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            url: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            command: Option<&'a str>,
            #[serde(skip_serializing_if = "BTreeMap::is_empty")]
            params: BTreeMap<&'static str, String>,
        }
        let (url, command) = match self {
            Error::ManualInstall { url, command, .. } => (url.as_deref(), command.as_deref()),
            _ => (None, None),
        };
        Payload {
            kind: self.kind(),
            message: self.to_string(),
            url,
            command,
            params: self.params().into_iter().collect(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI words these errors itself (`errorMessage`); English must read as `Display`.
    #[test]
    fn en_json_words_errors_like_display() {
        let en: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../../src/lib/i18n/en.json")).unwrap();
        let errors = [
            Error::NotInstalled {
                program: "gh".into(),
            },
            Error::BlockedInDev("gh student submit".into()),
            Error::Cancelled,
            Error::Busy,
            Error::PathExists("/tmp/lab-1".into()),
            Error::NeedsTeam,
            Error::SubmissionsClosed,
            Error::InvalidUsername("--help".into()),
            Error::NoTerminal,
            Error::LocalChanges,
            Error::OtherBranch("main".into()),
            Error::Diverged,
            Error::NotSignedIn,
            Error::TokenFromEnv("GH_TOKEN".into()),
            Error::Offline,
            Error::CalendarAccess,
        ];
        for err in errors {
            let key = format!("error.{}", err.kind());
            let mut text = en.get(&key).unwrap_or_else(|| panic!("{key}")).clone();
            for (name, value) in err.params() {
                text = text.replace(&format!("{{{name}}}"), &value);
            }
            assert_eq!(text, err.to_string(), "{key}");
        }
        for reason in ManualReason::ALL {
            let key = format!("setup.manualReason.{}", reason.code());
            assert_eq!(en.get(&key), Some(&reason.to_string()), "{key}");
        }
    }

    #[test]
    fn rate_limit_says_when_it_retries() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-15T14:05:00Z")
            .unwrap()
            .to_utc();
        let err = Error::RateLimited { reset_at: Some(at) };
        assert_eq!(
            err.to_string(),
            "GitHub rate limit reached, retrying after 14:05 UTC"
        );
        let err = Error::RateLimited { reset_at: None };
        assert_eq!(err.to_string(), "GitHub rate limit reached");
    }
}
