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

    /// Submit refused: the clone has no `.classroom50.yaml` and its name matches no
    /// assignment the app knows, so gh-student couldn't find it either.
    #[error("this repository has no .classroom50.yaml and doesn't match a published assignment")]
    UnknownAssignment,

    /// Submit refused: the clone has no `.classroom50.yaml` and this gh-student (installed
    /// version here) can't find the assignment from the repo's name (`tools::MIN_GH_STUDENT`).
    #[error("submitting a repository without .classroom50.yaml needs gh-student {min} or newer (installed: {version})", min = min_gh_student())]
    GhStudentOutdated { version: String },

    /// Submit refused: the teacher renamed the assignment and the clone's marker still
    /// names the old slug; `gh student submit` would push it back over the rewritten one.
    #[error("this assignment was renamed: pull the clone before you submit")]
    StaleMarker,

    /// Not a GitHub login, checked before it reaches `gh student`.
    #[error("\"{0}\" isn't a GitHub username")]
    InvalidUsername(String),

    /// Not a usable group name (`student::valid_team_name`), checked before it reaches gh.
    #[error("a group name needs 1 to 80 characters, on one line")]
    InvalidTeamName,

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

    /// Commit & push refused: GitHub has commits the clone lacks (pull first).
    #[error("GitHub has commits the clone doesn't have")]
    RemoteAhead,

    /// Commit & push refused: a merge, rebase, cherry-pick or revert is under way.
    #[error("a merge or rebase is in progress in the clone")]
    GitInProgress,

    /// Commit & push refused: no change to commit and no commit to push.
    #[error("nothing to commit or push")]
    NothingToPush,

    /// Commit refused: no change to commit.
    #[error("nothing to commit")]
    NothingToCommit,

    /// Commit refused: git has no `user.name` or `user.email` for this clone.
    #[error("git doesn't know your name and email yet")]
    NoIdentity,

    /// Commit refused: files GitHub would reject (over 100 MB), named here.
    #[error("files too large for GitHub: {}", .0.join(", "))]
    FileTooLarge(Vec<String>),

    /// GitHub refused the push: its branch moved since (not a fast-forward).
    #[error("GitHub refused the push because it has newer commits")]
    PushRejected,

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

    /// This copy wasn't installed from a release bundle (built from source, a bare binary),
    /// so the updater has nothing to replace.
    #[error("this copy of the toolbox can't update itself; download new versions from GitHub")]
    NoSelfUpdate,

    /// The calendar service didn't answer (Akonadi not running and not starting, no D-Bus).
    #[error("the calendar service isn't responding")]
    CalendarUnavailable,

    /// Not a shortcut the app accepts (`shortcut::normalize`).
    #[error("a shortcut needs one key and a modifier other than Shift")]
    InvalidShortcut,

    /// The OS refused the shortcut: another app or the system uses it.
    #[error("another app or the system already uses this shortcut")]
    ShortcutTaken,

    /// No system-wide shortcuts here (a Wayland session).
    #[error("system-wide shortcuts aren't available on this system")]
    ShortcutUnsupported,

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

/// What kind of error it is: `kind` in the IPC payload, worded by the UI as `error.<kind>`.
macro_rules! error_kinds {
    ($($variant:ident = $name:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        pub enum ErrorKind {
            $(
                #[serde(rename = $name)]
                $variant,
            )*
        }

        impl ErrorKind {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }
        }
    };
}

error_kinds! {
    NotInstalled = "notInstalled",
    CommandFailed = "commandFailed",
    TimedOut = "timedOut",
    Unsupported = "unsupported",
    ManualInstall = "manualInstall",
    BlockedInDev = "blockedInDev",
    Cancelled = "cancelled",
    Busy = "busy",
    PathExists = "pathExists",
    SchemaMismatch = "schemaMismatch",
    Student = "student",
    NeedsTeam = "needsTeam",
    SubmissionsClosed = "submissionsClosed",
    UnknownAssignment = "unknownAssignment",
    GhStudentOutdated = "ghStudentOutdated",
    StaleMarker = "staleMarker",
    InvalidUsername = "invalidUsername",
    InvalidTeamName = "invalidTeamName",
    NoTerminal = "noTerminal",
    LocalChanges = "localChanges",
    OtherBranch = "otherBranch",
    Diverged = "diverged",
    RemoteAhead = "remoteAhead",
    GitInProgress = "gitInProgress",
    NothingToPush = "nothingToPush",
    NothingToCommit = "nothingToCommit",
    NoIdentity = "noIdentity",
    FileTooLarge = "fileTooLarge",
    PushRejected = "pushRejected",
    NotSignedIn = "notSignedIn",
    TokenFromEnv = "tokenFromEnv",
    RateLimited = "rateLimited",
    GitHub = "github",
    Offline = "offline",
    NoSelfUpdate = "noSelfUpdate",
    CalendarAccess = "calendarAccess",
    CalendarUnavailable = "calendarUnavailable",
    InvalidShortcut = "invalidShortcut",
    ShortcutTaken = "shortcutTaken",
    ShortcutUnsupported = "shortcutUnsupported",
    RepoConfig = "repoConfig",
    Io = "io",
    Http = "http",
    Tauri = "tauri",
    Updater = "updater",
    Autostart = "autostart",
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

/// `1.56.0`, the gh-student `Error::GhStudentOutdated` asks for.
fn min_gh_student() -> String {
    let (major, minor, patch) = crate::tools::MIN_GH_STUDENT;
    format!("{major}.{minor}.{patch}")
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
            Error::GhStudentOutdated { version } => {
                vec![("version", version.clone()), ("min", min_gh_student())]
            }
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
        self.error_kind().as_str()
    }

    /// `kind()` as the enum the UI sees (`ErrorKind` in `src/lib/bindings.ts`).
    pub fn error_kind(&self) -> ErrorKind {
        match self {
            Error::NotInstalled { .. } => ErrorKind::NotInstalled,
            Error::CommandFailed { .. } => ErrorKind::CommandFailed,
            Error::TimedOut { .. } => ErrorKind::TimedOut,
            Error::Unsupported(_) => ErrorKind::Unsupported,
            Error::ManualInstall { .. } => ErrorKind::ManualInstall,
            Error::BlockedInDev(_) => ErrorKind::BlockedInDev,
            Error::Cancelled => ErrorKind::Cancelled,
            Error::Busy => ErrorKind::Busy,
            Error::PathExists(_) => ErrorKind::PathExists,
            Error::SchemaMismatch { .. } => ErrorKind::SchemaMismatch,
            Error::Student(_) => ErrorKind::Student,
            Error::NeedsTeam => ErrorKind::NeedsTeam,
            Error::SubmissionsClosed => ErrorKind::SubmissionsClosed,
            Error::UnknownAssignment => ErrorKind::UnknownAssignment,
            Error::GhStudentOutdated { .. } => ErrorKind::GhStudentOutdated,
            Error::StaleMarker => ErrorKind::StaleMarker,
            Error::InvalidUsername(_) => ErrorKind::InvalidUsername,
            Error::InvalidTeamName => ErrorKind::InvalidTeamName,
            Error::NoTerminal => ErrorKind::NoTerminal,
            Error::LocalChanges => ErrorKind::LocalChanges,
            Error::OtherBranch(_) => ErrorKind::OtherBranch,
            Error::Diverged => ErrorKind::Diverged,
            Error::RemoteAhead => ErrorKind::RemoteAhead,
            Error::GitInProgress => ErrorKind::GitInProgress,
            Error::NothingToPush => ErrorKind::NothingToPush,
            Error::NothingToCommit => ErrorKind::NothingToCommit,
            Error::NoIdentity => ErrorKind::NoIdentity,
            Error::FileTooLarge(_) => ErrorKind::FileTooLarge,
            Error::PushRejected => ErrorKind::PushRejected,
            Error::NotSignedIn => ErrorKind::NotSignedIn,
            Error::TokenFromEnv(_) => ErrorKind::TokenFromEnv,
            Error::RateLimited { .. } => ErrorKind::RateLimited,
            Error::GitHub(_) => ErrorKind::GitHub,
            Error::Offline => ErrorKind::Offline,
            Error::NoSelfUpdate => ErrorKind::NoSelfUpdate,
            Error::CalendarAccess => ErrorKind::CalendarAccess,
            Error::CalendarUnavailable => ErrorKind::CalendarUnavailable,
            Error::InvalidShortcut => ErrorKind::InvalidShortcut,
            Error::ShortcutTaken => ErrorKind::ShortcutTaken,
            Error::ShortcutUnsupported => ErrorKind::ShortcutUnsupported,
            Error::RepoConfig(_) => ErrorKind::RepoConfig,
            Error::Io(_) => ErrorKind::Io,
            Error::Http(_) => ErrorKind::Http,
            Error::Tauri(_) => ErrorKind::Tauri,
            Error::Updater(_) => ErrorKind::Updater,
            Error::Autostart(_) => ErrorKind::Autostart,
        }
    }
}

/// Errors cross the IPC boundary as `{ kind, message, params }` so the UI can branch on
/// `kind` and show its own wording (`error.<kind>` filled with `params`), else `message`.
/// What an `Error` serializes to; `CommandError` in `src/lib/bindings.ts`.
#[derive(Serialize)]
struct CommandError<'a> {
    kind: ErrorKind,
    message: String,
    /// `manualInstall`: where to download the tool by hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<&'a str>,
    /// `manualInstall`: the command to run in a terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<&'a str>,
    /// What the UI's wording of this kind (`error.<kind>`) fills in.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    params: BTreeMap<&'static str, String>,
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let (url, command) = match self {
            Error::ManualInstall { url, command, .. } => (url.as_deref(), command.as_deref()),
            _ => (None, None),
        };
        CommandError {
            kind: self.error_kind(),
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
            Error::UnknownAssignment,
            Error::GhStudentOutdated {
                version: "v1.55.0".into(),
            },
            Error::StaleMarker,
            Error::InvalidUsername("--help".into()),
            Error::InvalidTeamName,
            Error::NoTerminal,
            Error::LocalChanges,
            Error::OtherBranch("main".into()),
            Error::Diverged,
            Error::NotSignedIn,
            Error::TokenFromEnv("GH_TOKEN".into()),
            Error::Offline,
            Error::CalendarAccess,
            Error::CalendarUnavailable,
            Error::NoSelfUpdate,
            Error::InvalidShortcut,
            Error::ShortcutTaken,
            Error::ShortcutUnsupported,
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
