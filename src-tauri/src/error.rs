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

    #[error("{0}")]
    Unsupported(String),

    #[error("{url} uses schema {found:?}, expected {expected:?}; update the toolbox")]
    SchemaMismatch {
        url: String,
        found: String,
        expected: &'static str,
    },

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
}

impl Error {
    fn kind(&self) -> &'static str {
        match self {
            Error::NotInstalled { .. } => "notInstalled",
            Error::CommandFailed { .. } => "commandFailed",
            Error::Unsupported(_) => "unsupported",
            Error::SchemaMismatch { .. } => "schemaMismatch",
            Error::RepoConfig(_) => "repoConfig",
            Error::Io(_) => "io",
            Error::Http(_) => "http",
            Error::Tauri(_) => "tauri",
            Error::Updater(_) => "updater",
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
