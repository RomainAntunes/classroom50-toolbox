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
    /// gh-student's scopes the token lacks (`gh auth refresh` adds them). Empty when it
    /// can't be told: offline, or a token without classic scopes (fine-grained, app).
    pub missing_scopes: Vec<String>,
    /// gh is using a token from this environment variable. gh can't store, switch or
    /// widen a login then, so the app doesn't offer to.
    pub env_token: Option<String>,
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
    /// How far along it is (0 to 1), guessed from what the command printed; `None` until
    /// a known step shows up (gh prints nothing while it installs an extension).
    pub progress: Option<f32>,
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

    fn start(&self, tool: Tool, kind: JobKind) -> Result<oneshot::Receiver<()>> {
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
                    progress: None,
                },
                cancel: Some(tx),
            },
        );
        Ok(rx)
    }

    /// Moves the bar forward when `line` marks a later step; it never steps back. Returns
    /// whether it moved.
    fn advance(&self, tool: Tool, line: &str) -> bool {
        let Some(next) = progress_of(line) else {
            return false;
        };
        if let Ok(mut running) = self.running.lock()
            && let Some(r) = running.get_mut(&tool)
            && r.job.progress.is_none_or(|current| next > current)
        {
            r.job.progress = Some(next);
            return true;
        }
        false
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

/// Runs `spec` as the one job of `tool`, telling the UI how far along it is.
async fn run_job(
    app: &AppHandle,
    tool: Tool,
    kind: JobKind,
    spec: &CommandSpec,
    envs: &[(&str, &str)],
    timeout: Duration,
) -> Result<()> {
    let jobs = app.state::<ToolJobs>();
    let cancel = jobs.start(tool, kind)?;
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
            if jobs.advance(tool, line) {
                emit(&jobs);
            }
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

/// The progress a line of install output stands for: a download percentage (`45.0%`,
/// Homebrew and winget print one on big downloads) as part of the download share, else the
/// package manager's own step markers (`platform::install_progress`).
fn progress_of(line: &str) -> Option<f32> {
    let percent = line
        .split_whitespace()
        .filter_map(|word| word.strip_suffix('%')?.parse::<f32>().ok())
        .rfind(|p| (0.0..=100.0).contains(p));
    match percent {
        Some(p) => Some(0.15 + 0.5 * p / 100.0),
        None => platform::install_progress(line),
    }
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

/// Reads gh's login and the scopes its token was granted, in one request (`X-OAuth-Scopes`
/// comes back with any authenticated call). The token itself never leaves Rust.
pub async fn auth_status() -> AuthStatus {
    let env_token = env_token_var().map(str::to_owned);
    let signed_out = AuthStatus {
        logged_in: false,
        login: None,
        missing_scopes: Vec::new(),
        env_token: env_token.clone(),
    };
    match process::run("gh", ["api", "--include", "user"], None).await {
        Ok(out) => {
            let (login, granted) = parse_user_response(&out.stdout);
            AuthStatus {
                logged_in: login.is_some(),
                login,
                missing_scopes: granted.map(|g| missing_scopes(&g)).unwrap_or_default(),
                env_token,
            }
        }
        Err(Error::NotInstalled { .. }) => signed_out,
        Err(err) => {
            // GitHub turned the token down (revoked, expired): signing in again fixes it.
            let rejected =
                matches!(&err, Error::CommandFailed { stderr, .. } if stderr.contains("HTTP 401"));
            // Otherwise (offline, GitHub down) gh's local state is trusted, so the
            // account stays on screen.
            if rejected || gh_token().await.is_err() {
                return signed_out;
            }
            AuthStatus {
                logged_in: true,
                login: active_login().await,
                missing_scopes: Vec::new(),
                env_token,
            }
        }
    }
}

/// The environment variable gh takes its github.com token from, if set (gh reads
/// `GH_TOKEN` first, then `GITHUB_TOKEN`). The app inherits it when started from a shell.
pub fn env_token_var() -> Option<&'static str> {
    ["GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .find(|var| std::env::var_os(var).is_some_and(|value| !value.is_empty()))
}

/// `gh api --include user`: the status line and headers, a blank line, then the JSON. The
/// scopes are `None` when the header is missing (a token without classic scopes).
fn parse_user_response(stdout: &str) -> (Option<String>, Option<Vec<String>>) {
    let stdout = stdout.replace("\r\n", "\n");
    let (head, body) = stdout.split_once("\n\n").unwrap_or(("", stdout.as_str()));
    let scopes = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("x-oauth-scopes").then(|| {
            value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
    });
    let login = serde_json::from_str::<serde_json::Value>(body.trim())
        .ok()
        .and_then(|user| user.get("login")?.as_str().map(str::to_owned))
        .filter(|login| !login.is_empty());
    (login, scopes)
}

/// gh-student's scopes not covered by `granted`. GitHub reports only the broadest scope it
/// granted, so `admin:org` stands for `read:org` too.
fn missing_scopes(granted: &[String]) -> Vec<String> {
    let has = |scope: &str| granted.iter().any(|g| g == scope);
    crate::login::REQUIRED_SCOPES
        .iter()
        .filter(|&&scope| {
            let covered = match scope {
                "read:org" => has("read:org") || has("write:org") || has("admin:org"),
                other => has(other),
            };
            !covered
        })
        .map(|scope| (*scope).to_owned())
        .collect()
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
        let mut cancel = jobs.start(Tool::Gh, JobKind::Install).unwrap();
        assert!(matches!(
            jobs.start(Tool::Gh, JobKind::Install),
            Err(Error::Busy)
        ));
        // Another tool may run at the same time.
        let _other = jobs.start(Tool::GhStudent, JobKind::Upgrade).unwrap();

        let progress = |jobs: &ToolJobs| {
            jobs.list()
                .into_iter()
                .find(|j| j.tool == Tool::Gh)
                .unwrap()
                .progress
        };
        assert!(!jobs.advance(Tool::Gh, "nothing known"));
        assert_eq!(progress(&jobs), None);
        assert!(jobs.advance(Tool::Gh, "###################### 50.0%"));
        assert_eq!(progress(&jobs), Some(0.4));
        // An earlier step printed late doesn't move the bar back.
        assert!(!jobs.advance(Tool::Gh, "  10%"));
        assert_eq!(progress(&jobs), Some(0.4));

        assert!(cancel.try_recv().is_err());
        jobs.cancel(Tool::Gh);
        assert!(cancel.try_recv().is_ok());

        jobs.finish(Tool::Gh);
        assert_eq!(jobs.list().len(), 1);
        assert!(jobs.start(Tool::Gh, JobKind::Install).is_ok());
    }

    #[test]
    fn reads_download_percentages() {
        assert_eq!(progress_of("######## 100.0%"), Some(0.65));
        assert_eq!(progress_of("  0%"), Some(0.15));
        assert_eq!(
            progress_of("disk 250%"),
            platform::install_progress("disk 250%")
        );
    }

    #[test]
    fn reads_login_and_scopes_from_the_user_response() {
        let stdout = "HTTP/2.0 200 OK\r\nContent-Type: application/json; charset=utf-8\r\n\
            X-Oauth-Scopes: admin:org, gist, repo, workflow\r\nX-Ratelimit-Limit: 5000\r\n\r\n\
            {\"login\":\"alice\",\"id\":1}";
        let (login, scopes) = parse_user_response(stdout);
        assert_eq!(login.as_deref(), Some("alice"));
        let scopes = scopes.unwrap();
        assert_eq!(scopes, ["admin:org", "gist", "repo", "workflow"]);
        assert!(missing_scopes(&scopes).is_empty());

        // gh's own defaults: what a student who ran `gh auth login` before has.
        let stdout = "HTTP/2.0 200 OK\nX-Oauth-Scopes: gist, read:org, repo\n\n{\"login\":\"bob\"}";
        let (_, scopes) = parse_user_response(stdout);
        assert_eq!(missing_scopes(&scopes.unwrap()), ["admin:org", "workflow"]);

        // A fine-grained token sends no scope header: nothing can be said to be missing.
        let (login, scopes) = parse_user_response("HTTP/2.0 200 OK\n\n{\"login\":\"carol\"}");
        assert_eq!(login.as_deref(), Some("carol"));
        assert_eq!(scopes, None);

        // An empty header is a token with no scopes at all.
        let (_, scopes) = parse_user_response("HTTP/2.0 200 OK\nX-Oauth-Scopes: \n\n{}");
        assert_eq!(missing_scopes(&scopes.unwrap()).len(), 4);
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
