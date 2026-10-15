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

    #[error("GitHub rate limit reached, retrying after {reset_at:?}")]
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

impl Error {
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
            Error::BlockedInDev(_) => "blockedInDev",
            Error::Cancelled => "cancelled",
            Error::Busy => "busy",
            Error::PathExists(_) => "pathExists",
            Error::SchemaMismatch { .. } => "schemaMismatch",
            Error::Student(_) => "student",
            Error::NeedsTeam => "needsTeam",
            Error::LocalChanges => "localChanges",
            Error::OtherBranch(_) => "otherBranch",
            Error::Diverged => "diverged",
            Error::NotSignedIn => "notSignedIn",
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

/// Errors cross the IPC boundary as `{ kind, message }` so the UI can branch on `kind`
/// and show `message` (or map `kind` to a translated string).
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Payload<'a> {
            kind: &'a str,
            message: String,
        }
        Payload {
            kind: self.kind(),
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}
