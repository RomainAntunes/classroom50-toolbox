//! Detection and installation of the student prerequisites: git, gh and gh-student.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::error::{Error, Result};
use crate::platform::{self, CommandSpec};
use crate::process::{self, Streaming};

pub const GH_STUDENT_REPO: &str = "foundation50/gh-student";

/// Payload: every running install or update (`ToolJob[]`), sent when one starts, prints a
/// line or ends.
pub const JOBS_EVENT: &str = "tool-jobs";

/// Package managers download and unpack; a slow link can take a while, a prompt nobody
/// can answer takes forever.
const PACKAGE_MANAGER_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// gh-student is a small script extension (a clone took about a second here).
const EXTENSION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Git,
    Gh,
    GhStudent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tool: Tool,
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub logged_in: bool,
    pub login: Option<String>,
}

pub async fn status(tool: Tool) -> ToolStatus {
    let version = match tool {
        Tool::Git => process::run("git", ["--version"], None)
            .await
            .ok()
            .and_then(|out| parse_git_version(&out.stdout)),
        Tool::Gh => process::run("gh", ["--version"], None)
            .await
            .ok()
            .and_then(|out| parse_gh_version(&out.stdout)),
        Tool::GhStudent => process::run("gh", ["extension", "list"], None)
            .await
            .ok()
            .and_then(|out| parse_extension_version(&out.stdout, GH_STUDENT_REPO)),
    };
    ToolStatus {
        tool,
        installed: version.is_some(),
        version,
    }
}

pub async fn statuses() -> Vec<ToolStatus> {
    let (git, gh, gh_student) =
        tokio::join!(status(Tool::Git), status(Tool::Gh), status(Tool::GhStudent));
    vec![git, gh, gh_student]
}

pub fn install_command(tool: Tool) -> Result<CommandSpec> {
    match tool {
        Tool::GhStudent => Ok(CommandSpec::new(
            "gh",
            &["extension", "install", GH_STUDENT_REPO],
        )),
        _ => platform::install_command(tool),
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Install,
    Upgrade,
}

/// An install or update in progress, kept in Rust so a view opened later (the popover
/// hidden and shown again, another page) still shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolJob {
    pub tool: Tool,
    pub kind: JobKind,
    /// The command line, shown until it prints something.
    pub command: String,
    /// The last line the command printed.
    pub line: Option<String>,
}

struct Running {
    job: ToolJob,
    cancel: Option<oneshot::Sender<()>>,
}

/// The running installs and updates, at most one per tool.
#[derive(Default)]
pub struct ToolJobs {
    running: Mutex<BTreeMap<Tool, Running>>,
}

impl ToolJobs {
    pub fn list(&self) -> Vec<ToolJob> {
        self.running
            .lock()
            .map(|running| running.values().map(|r| r.job.clone()).collect())
            .unwrap_or_default()
    }

    /// Stops the install or update of `tool`, if one is running.
    pub fn cancel(&self, tool: Tool) {
        let cancel = self
            .running
            .lock()
            .ok()
            .and_then(|mut running| running.get_mut(&tool)?.cancel.take());
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
        }
    }

    fn start(&self, tool: Tool, kind: JobKind, command: String) -> Result<oneshot::Receiver<()>> {
        let mut running = self
            .running
            .lock()
            .map_err(|_| Error::Unsupported("tool state is unavailable".into()))?;
        if running.contains_key(&tool) {
            return Err(Error::Busy);
        }
        let (tx, rx) = oneshot::channel();
        running.insert(
            tool,
            Running {
                job: ToolJob {
                    tool,
                    kind,
                    command,
                    line: None,
                },
                cancel: Some(tx),
            },
        );
        Ok(rx)
    }

    fn set_line(&self, tool: Tool, line: &str) {
        if let Ok(mut running) = self.running.lock()
            && let Some(r) = running.get_mut(&tool)
        {
            r.job.line = Some(line.to_owned());
        }
    }

    fn finish(&self, tool: Tool) {
        if let Ok(mut running) = self.running.lock() {
            running.remove(&tool);
        }
    }
}

pub async fn install(app: &AppHandle, tool: Tool) -> Result<()> {
    let spec = install_command(tool)?;
    if tool == Tool::GhStudent && process::find_in_path("gh").is_none() {
        return Err(Error::NotInstalled {
            program: "gh".into(),
        });
    }
    let (envs, timeout) = match tool {
        Tool::GhStudent => (&[][..], EXTENSION_TIMEOUT),
        _ => (platform::install_env(), PACKAGE_MANAGER_TIMEOUT),
    };
    run_job(app, tool, JobKind::Install, &spec, envs, timeout).await
}

/// Updates gh-student, which changes often upstream. git and gh are left to the OS
/// package manager, which updates them with the rest of the system.
pub async fn upgrade(app: &AppHandle, tool: Tool) -> Result<()> {
    if tool != Tool::GhStudent {
        return Err(Error::Unsupported(
            "git and gh are updated by your package manager".into(),
        ));
    }
    let spec = CommandSpec::new("gh", &["extension", "upgrade", "gh-student"]);
    run_job(app, tool, JobKind::Upgrade, &spec, &[], EXTENSION_TIMEOUT).await
}

/// Runs `spec` as the one job of `tool`, telling the UI about each line it prints.
async fn run_job(
    app: &AppHandle,
    tool: Tool,
    kind: JobKind,
    spec: &CommandSpec,
    envs: &[(&str, &str)],
    timeout: Duration,
) -> Result<()> {
    let jobs = app.state::<ToolJobs>();
    let command = std::iter::once(spec.program.as_str())
        .chain(spec.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let cancel = jobs.start(tool, kind, command)?;
    let emit = |jobs: &ToolJobs| {
        let _ = app.emit(JOBS_EVENT, jobs.list());
    };
    emit(&jobs);
    let result = process::run_streaming(
        &spec.program,
        &spec.args,
        Streaming { envs, timeout },
        async {
            // A dropped sender (the job ended) must not read as a cancel.
            if cancel.await.is_err() {
                std::future::pending::<()>().await;
            }
        },
        |line| {
            jobs.set_line(tool, line);
            emit(&jobs);
        },
    )
    .await;
    jobs.finish(tool);
    emit(&jobs);
    if let Err(err) = &result {
        log::warn!("{} {tool:?} failed: {err}", spec.program);
    }
    result
}

/// The newer gh-student version `upgrade` would install, if any (asks GitHub through
/// gh's dry run). Always `None` for git and gh.
pub async fn available_update(tool: Tool) -> Result<Option<String>> {
    if tool != Tool::GhStudent {
        return Ok(None);
    }
    let args = ["extension", "upgrade", "gh-student", "--dry-run"];
    let out = process::run("gh", args, None).await?;
    Ok(parse_dry_run(&out.stdout).or_else(|| parse_dry_run(&out.stderr)))
}

/// `[student]: would have upgraded from v1.52.2 to v1.58.1` -> `v1.58.1`;
/// `[student]: already up to date` -> `None`.
fn parse_dry_run(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, to) = line
            .split_once("would have upgraded from ")?
            .1
            .split_once(" to ")?;
        Some(to.trim().to_owned())
    })
}

/// Reads gh's stored login. The token itself never leaves Rust.
pub async fn auth_status() -> AuthStatus {
    let login = process::run("gh", ["api", "user", "--jq", ".login"], None)
        .await
        .ok()
        .map(|out| out.stdout)
        .filter(|login| !login.is_empty());
    AuthStatus {
        logged_in: login.is_some(),
        login,
    }
}

/// The account gh uses for github.com, read from its local config (no network). Lets a
/// sync notice `gh auth switch` before asking GitHub anything.
pub async fn active_login() -> Option<String> {
    process::run("gh", ["config", "get", "user", "-h", "github.com"], None)
        .await
        .ok()
        .map(|out| out.stdout)
        .filter(|login| !login.is_empty())
}

/// gh's token for github.com, read fresh for each use and kept only in memory.
pub async fn gh_token() -> Result<String> {
    match process::run("gh", ["auth", "token", "--hostname", "github.com"], None).await {
        Ok(out) if !out.stdout.is_empty() => Ok(out.stdout),
        Ok(_) | Err(Error::CommandFailed { .. }) => Err(Error::NotSignedIn),
        Err(err) => Err(err),
    }
}

/// `git version 2.50.1 (Apple Git-155)` -> `2.50.1`
fn parse_git_version(stdout: &str) -> Option<String> {
    stdout
        .strip_prefix("git version ")
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
}

/// `gh version 2.100.0 (2026-09-03)` -> `2.100.0`
fn parse_gh_version(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .next()?
        .strip_prefix("gh version ")
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
}

/// `gh extension list` prints `gh student\tfoundation50/gh-student\tv1.42.0` per extension.
fn parse_extension_version(stdout: &str, repo: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let mut columns = line.split('\t').map(str::trim);
        let _name = columns.next()?;
        if !columns.next()?.eq_ignore_ascii_case(repo) {
            return None;
        }
        Some(columns.next().unwrap_or("").to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_job_per_tool_and_cancel_reaches_it() {
        let jobs = ToolJobs::default();
        let mut cancel = jobs
            .start(Tool::Gh, JobKind::Install, "brew install gh".into())
            .unwrap();
        assert!(matches!(
            jobs.start(Tool::Gh, JobKind::Install, "brew install gh".into()),
            Err(Error::Busy)
        ));
        // Another tool may run at the same time.
        let _other = jobs
            .start(Tool::GhStudent, JobKind::Upgrade, "gh".into())
            .unwrap();

        jobs.set_line(Tool::Gh, "==> Pouring gh");
        let gh = jobs
            .list()
            .into_iter()
            .find(|j| j.tool == Tool::Gh)
            .unwrap();
        assert_eq!(gh.line.as_deref(), Some("==> Pouring gh"));
        assert_eq!(gh.command, "brew install gh");

        assert!(cancel.try_recv().is_err());
        jobs.cancel(Tool::Gh);
        assert!(cancel.try_recv().is_ok());

        jobs.finish(Tool::Gh);
        assert_eq!(jobs.list().len(), 1);
        assert!(
            jobs.start(Tool::Gh, JobKind::Install, "brew install gh".into())
                .is_ok()
        );
    }

    #[test]
    fn parses_git_version() {
        assert_eq!(
            parse_git_version("git version 2.50.1 (Apple Git-155)").as_deref(),
            Some("2.50.1")
        );
        assert_eq!(
            parse_git_version("git version 2.47.0.windows.1").as_deref(),
            Some("2.47.0.windows.1")
        );
        assert_eq!(parse_git_version("nope"), None);
    }

    #[test]
    fn parses_gh_version() {
        let out =
            "gh version 2.100.0 (2026-09-03)\nhttps://github.com/cli/cli/releases/tag/v2.100.0";
        assert_eq!(parse_gh_version(out).as_deref(), Some("2.100.0"));
    }

    #[test]
    fn reads_the_upgrade_dry_run() {
        assert_eq!(
            parse_dry_run("[student]: would have upgraded from v1.52.2 to v1.58.1").as_deref(),
            Some("v1.58.1")
        );
        assert_eq!(parse_dry_run("[student]: already up to date"), None);
    }

    #[test]
    fn finds_gh_student_in_extension_list() {
        let out = "gh teacher\tfoundation50/gh-teacher\tv1.42.0\ngh student\tfoundation50/gh-student\tv1.41.2";
        assert_eq!(
            parse_extension_version(out, GH_STUDENT_REPO).as_deref(),
            Some("v1.41.2")
        );
        assert_eq!(parse_extension_version("", GH_STUDENT_REPO), None);
    }
}
