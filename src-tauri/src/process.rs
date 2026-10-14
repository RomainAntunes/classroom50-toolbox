//! The single entry point for running external programs (`git`, `gh`, package managers).
//! Arguments are always passed as a vector, never through a shell.

use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use tokio::process::Command;

use crate::error::{Error, Result};
use crate::platform;

#[derive(Debug, Clone)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program` to completion and returns its trimmed output, or `CommandFailed` on a
/// non-zero exit.
pub async fn run<I, S>(program: &str, args: I, cwd: Option<&Path>) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = match cmd.output().await {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(Error::NotInstalled {
                program: program.to_owned(),
            });
        }
        Err(err) => return Err(err.into()),
    };

    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr)
        .trim_end()
        .to_owned();
    if !output.status.success() {
        return Err(Error::CommandFailed {
            program: program.to_owned(),
            code: output.status.code(),
            stderr,
        });
    }
    Ok(Output { stdout, stderr })
}

/// Starts `program` without waiting for it (terminals, editors).
pub fn spawn_detached<I, S>(program: &str, args: I, cwd: Option<&Path>) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .env("PATH", search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Err(Error::NotInstalled {
            program: program.to_owned(),
        }),
        Err(err) => Err(err.into()),
    }
}

/// Looks `program` up on the augmented search path, like `which`.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{program}.exe"), format!("{program}.cmd")]
    } else {
        vec![program.to_owned()]
    };
    std::env::split_paths(&search_path()).find_map(|dir| {
        candidates
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
    })
}

fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PATH", search_path())
        // gh must never block on an interactive prompt or print an upgrade banner.
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1");
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// GUI apps don't inherit the login shell's PATH (a Finder-launched macOS app has no
/// `/opt/homebrew/bin`), so the inherited PATH is extended with the usual install locations.
fn search_path() -> &'static OsString {
    static PATH: OnceLock<OsString> = OnceLock::new();
    PATH.get_or_init(|| {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        for extra in platform::extra_path_dirs() {
            if !dirs.contains(&extra) {
                dirs.push(extra);
            }
        }
        std::env::join_paths(dirs).unwrap_or_default()
    })
}
