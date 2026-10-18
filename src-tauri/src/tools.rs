//! Detection and installation of the student prerequisites: git, gh and gh-student.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use crate::error::{Error, Result};
use crate::events::{IpcEvent, ToolJobsChanged};
use crate::github::Http;
use crate::platform::{self, InstallPlan};
use crate::process::{self, Streaming};

pub const GH_STUDENT_REPO: &str = "foundation50/gh-student";

/// The oldest gh the app works with: `gh auth status --json` (the account list) came in
/// 2.81.0. Debian's and Ubuntu's own gh is older; installing again from the app upgrades it.
pub const MIN_GH: (u32, u32, u32) = (2, 81, 0);

/// The oldest gh-student the app works with: `gh student submit` finds the assignment of a
/// repo without `.classroom50.yaml` (`no_autograder` and `empty_repo` assignments) from the
/// repo's name since 1.56.0. Older ones are updated with `gh extension upgrade`.
pub const MIN_GH_STUDENT: (u32, u32, u32) = (1, 56, 0);

/// Package managers download and unpack; a slow link can take a while, a prompt nobody
/// can answer takes forever.
const PACKAGE_MANAGER_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// gh-student is one small binary from a GitHub release.
const EXTENSION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Apple's Command Line Tools are a large download the user starts in Apple's dialog.
const DETACHED_TIMEOUT: Duration = Duration::from_secs(45 * 60);
/// How often a detached install (`InstallPlan::detached`) is checked for the tool.
const DETACHED_POLL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Git,
    Gh,
    GhStudent,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tool: Tool,
    pub installed: bool,
    pub version: Option<String>,
    /// Installed, but older than the app needs (`MIN_GH`, `MIN_GH_STUDENT`): installing gh
    /// again upgrades it, gh-student is upgraded. A version that can't be read isn't.
    pub outdated: bool,
}

impl ToolStatus {
    pub fn new(tool: Tool, installed: bool, version: Option<String>) -> Self {
        let min = match tool {
            Tool::Git => None,
            Tool::Gh => Some(MIN_GH),
            Tool::GhStudent => Some(MIN_GH_STUDENT),
        };
        let outdated = min.is_some_and(|min| {
            version
                .as_deref()
                .and_then(parse_semver)
                .is_some_and(|v| v < min)
        });
        Self {
            tool,
            installed,
            version,
            outdated,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
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
    let (installed, version) = match tool {
        Tool::Git => found(
            process::run("git", ["--version"], None)
                .await
                .ok()
                .and_then(|out| parse_git_version(&out.stdout)),
        ),
        Tool::Gh => found(
            process::run("gh", ["--version"], None)
                .await
                .ok()
                .and_then(|out| parse_gh_version(&out.stdout)),
        ),
        Tool::GhStudent => gh_student_status().await,
    };
    ToolStatus::new(tool, installed, version)
}

fn found(version: Option<String>) -> (bool, Option<String>) {
    (version.is_some(), version)
}

/// `gh extension list` needs a signed-in gh (it exits 4 otherwise), so when it fails the
/// extension is looked up where gh keeps it.
async fn gh_student_status() -> (bool, Option<String>) {
    if process::find_in_path("gh").is_none() {
        return (false, None);
    }
    match process::run("gh", ["extension", "list"], None).await {
        Ok(out) => found(parse_extension_version(&out.stdout, GH_STUDENT_REPO)),
        Err(_) => gh_data_dir()
            .map(|dir| installed_extension(&dir.join("extensions").join("gh-student")))
            .unwrap_or((false, None)),
    }
}

/// gh's data dir, as go-gh's `config.DataDir` finds it.
fn gh_data_dir() -> Option<PathBuf> {
    let env = |var: &str| std::env::var_os(var).filter(|v| !v.is_empty());
    if let Some(xdg) = env("XDG_DATA_HOME") {
        return Some(PathBuf::from(xdg).join("gh"));
    }
    if cfg!(windows)
        && let Some(local) = env("LOCALAPPDATA")
    {
        return Some(PathBuf::from(local).join("GitHub CLI"));
    }
    platform::home_dir().map(|home| home.join(".local").join("share").join("gh"))
}

/// An extension folder: a binary one has `manifest.yml` (`tag: v1.58.1`), a script one is a
/// git clone (no version without running git).
fn installed_extension(dir: &Path) -> (bool, Option<String>) {
    if let Ok(manifest) = std::fs::read_to_string(dir.join("manifest.yml")) {
        let tag = manifest.lines().find_map(|line| {
            let tag = line.strip_prefix("tag:")?.trim().trim_matches(['"', '\'']);
            (!tag.is_empty()).then(|| tag.to_owned())
        });
        return (true, tag);
    }
    (dir.join(".git").exists(), None)
}

/// `2.100.0` -> `(2, 100, 0)`; a suffix (`2.47.0.windows.1`, `2.81.0-rc1`) is ignored.
fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.trim_start_matches('v').split(['.', '-']);
    let mut next = || parts.next()?.parse::<u32>().ok();
    Some((next()?, next()?, next().unwrap_or(0)))
}

pub async fn statuses() -> Vec<ToolStatus> {
    let (git, gh, gh_student) =
        tokio::join!(status(Tool::Git), status(Tool::Gh), status(Tool::GhStudent));
    vec![git, gh, gh_student]
}

/// What installing `tool` runs here. gh-student goes through gh (`gh extension install`
/// needs no sign-in: gh skips its auth check for it, and reads the public release anonymously).
pub fn install_plan(tool: Tool) -> Result<InstallPlan> {
    match tool {
        Tool::GhStudent => Ok(InstallPlan::new(
            "gh",
            &["extension", "install", GH_STUDENT_REPO],
            format!("gh extension install {GH_STUDENT_REPO}"),
        )),
        _ => platform::install_plan(tool),
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Install,
    Upgrade,
}

/// An install or update in progress, kept in Rust so a view opened later (the popover
/// hidden and shown again, another page) still shows it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
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
    cancel: watch::Sender<bool>,
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
        if let Ok(running) = self.running.lock()
            && let Some(r) = running.get(&tool)
        {
            r.cancel.send_replace(true);
        }
    }

    fn start(&self, tool: Tool, kind: JobKind) -> Result<watch::Receiver<bool>> {
        let mut running = self
            .running
            .lock()
            .map_err(|_| Error::Unsupported("tool state is unavailable".into()))?;
        if running.contains_key(&tool) {
            return Err(Error::Busy);
        }
        let (tx, rx) = watch::channel(false);
        running.insert(
            tool,
            Running {
                job: ToolJob {
                    tool,
                    kind,
                    progress: None,
                },
                cancel: tx,
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

/// Resolves once the job is cancelled; never when it ends without a cancel.
async fn cancelled(mut cancel: watch::Receiver<bool>) {
    if cancel.wait_for(|cancelled| *cancelled).await.is_err() {
        std::future::pending::<()>().await;
    }
}

pub async fn install(app: &AppHandle, tool: Tool) -> Result<()> {
    let plan = install_plan(tool)?;
    if tool == Tool::GhStudent && process::find_in_path("gh").is_none() {
        return Err(Error::NotInstalled {
            program: "gh".into(),
        });
    }
    let timeout = match tool {
        Tool::GhStudent => EXTENSION_TIMEOUT,
        _ => PACKAGE_MANAGER_TIMEOUT,
    };
    run_job(app, tool, JobKind::Install, plan, timeout).await
}

/// Updates gh-student, which changes often upstream. git and gh are left to the OS
/// package manager, which updates them with the rest of the system (an outdated gh is
/// installed again instead, see `MIN_GH`).
pub async fn upgrade(app: &AppHandle, tool: Tool) -> Result<()> {
    if tool != Tool::GhStudent {
        return Err(Error::Unsupported(
            "git and gh are updated by your package manager".into(),
        ));
    }
    let plan = InstallPlan::new(
        "gh",
        &["extension", "upgrade", "gh-student"],
        "gh extension upgrade gh-student",
    );
    run_job(app, tool, JobKind::Upgrade, plan, EXTENSION_TIMEOUT).await
}

/// Runs `plan` as the one job of `tool`, telling the UI how far along it is.
async fn run_job(
    app: &AppHandle,
    tool: Tool,
    kind: JobKind,
    plan: InstallPlan,
    timeout: Duration,
) -> Result<()> {
    let jobs = app.state::<ToolJobs>();
    let cancel = jobs.start(tool, kind)?;
    let emit = |jobs: &ToolJobs| {
        let _ = ToolJobsChanged::emit(app, &jobs.list());
    };
    emit(&jobs);
    let result = run_plan(app, &jobs, tool, plan, timeout, cancel, &emit).await;
    jobs.finish(tool);
    emit(&jobs);
    if let Err(err) = &result {
        log::warn!("{kind:?} {tool:?} failed: {err}");
    }
    result
}

async fn run_plan(
    app: &AppHandle,
    jobs: &ToolJobs,
    tool: Tool,
    mut plan: InstallPlan,
    timeout: Duration,
    cancel: watch::Receiver<bool>,
    emit: &(dyn Fn(&ToolJobs) + Sync),
) -> Result<()> {
    // Kept until the command is done: it reads the file.
    let mut fetched = None;
    if let Some(url) = plan.fetch {
        let file = tokio::select! {
            file = fetch(app, url) => file?,
            () = cancelled(cancel.clone()) => return Err(Error::Cancelled),
        };
        plan.args.push(file.path().to_string_lossy().into_owned());
        fetched = Some(file);
    }
    let result = process::run_streaming(
        &plan.program,
        &plan.args,
        Streaming {
            envs: plan.env,
            timeout,
        },
        cancelled(cancel.clone()),
        |line| {
            if jobs.advance(tool, line) {
                emit(jobs);
            }
        },
    )
    .await;
    drop(fetched);
    let result = match result {
        Err(Error::CommandFailed {
            program,
            code,
            stderr,
        }) => platform::interpret_failure(&program, code).unwrap_or(Err(Error::CommandFailed {
            program,
            code,
            stderr,
        })),
        other => other,
    }
    .map_err(signed_out);
    if !plan.detached {
        return result;
    }
    // `xcode-select --install` fails when the tools are already there.
    if result.is_err() && status(tool).await.installed {
        return Ok(());
    }
    result?;
    wait_for(tool, &plan.program, cancel).await
}

/// Waits for an install the program only started (Apple's dialog) to put the tool in place.
async fn wait_for(tool: Tool, program: &str, cancel: watch::Receiver<bool>) -> Result<()> {
    let deadline = tokio::time::sleep(DETACHED_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        if status(tool).await.installed {
            return Ok(());
        }
        tokio::select! {
            () = tokio::time::sleep(DETACHED_POLL) => {}
            () = cancelled(cancel.clone()) => return Err(Error::Cancelled),
            () = &mut deadline => {
                return Err(Error::TimedOut {
                    program: program.to_owned(),
                    minutes: DETACHED_TIMEOUT.as_secs() / 60,
                    output: String::new(),
                });
            }
        }
    }
}

/// A file downloaded for an install (GitHub's apt keyring), in a folder only the user can
/// write to, removed when dropped.
struct Fetched {
    dir: PathBuf,
    file: PathBuf,
}

impl Fetched {
    fn path(&self) -> &Path {
        &self.file
    }
}

impl Drop for Fetched {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn fetch(app: &AppHandle, url: &str) -> Result<Fetched> {
    let http = app.state::<Http>().0.clone();
    let bytes = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(Error::network)?
        .bytes()
        .await
        .map_err(Error::network)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("c50-install-{}-{nanos}", std::process::id()));
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir)?;
    let name = url.rsplit('/').next().unwrap_or("download");
    let fetched = Fetched {
        file: dir.join(name),
        dir,
    };
    std::fs::write(fetched.path(), &bytes)?;
    Ok(fetched)
}

/// The progress a line of install output stands for: a download percentage (`45.0%`,
/// Homebrew and winget print one on big downloads) or size (`12.0 MB / 64.2 MB`, winget in
/// any display language) as part of the download share, else the package manager's own step
/// markers (`platform::install_progress`).
fn progress_of(line: &str) -> Option<f32> {
    let percent = line
        .split_whitespace()
        .filter_map(|word| word.strip_suffix('%')?.parse::<f32>().ok())
        .rfind(|p| (0.0..=100.0).contains(p))
        .or_else(|| downloaded_share(line).map(|share| share * 100.0));
    match percent {
        Some(p) => Some(0.15 + 0.5 * p / 100.0),
        None => platform::install_progress(line),
    }
}

/// `1.00 MB / 9.50 MB` -> about 0.105.
fn downloaded_share(line: &str) -> Option<f32> {
    let (done, total) = line.split_once(" / ")?;
    let bytes = |text: &str, last: bool| -> Option<f64> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (number, unit) = if last {
            (*words.get(words.len().checked_sub(2)?)?, *words.last()?)
        } else {
            (*words.first()?, *words.get(1)?)
        };
        let scale = match unit.to_ascii_uppercase().as_str() {
            "B" => 1.0,
            "KB" | "KIB" => 1024.0,
            "MB" | "MIB" => 1024.0 * 1024.0,
            "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
            _ => return None,
        };
        Some(number.replace(',', ".").parse::<f64>().ok()? * scale)
    };
    let (done, total) = (bytes(done, true)?, bytes(total, false)?);
    (total > 0.0 && done <= total).then(|| (done / total) as f32)
}

/// The newer gh-student version `upgrade` would install, if any (asks GitHub through
/// gh's dry run). Always `None` for git and gh.
pub async fn available_update(tool: Tool) -> Result<Option<String>> {
    if tool != Tool::GhStudent {
        return Ok(None);
    }
    let args = ["extension", "upgrade", "gh-student", "--dry-run"];
    let out = process::run("gh", args, None).await.map_err(signed_out)?;
    Ok(parse_dry_run(&out.stdout).or_else(|| parse_dry_run(&out.stderr)))
}

/// gh exits 4 when the command needs a signed-in gh (`gh help exit-codes`): every
/// `gh extension` command but `install` does.
fn signed_out(err: Error) -> Error {
    match err {
        Error::CommandFailed {
            program,
            code: Some(4),
            ..
        } if program == "gh" => Error::NotSignedIn,
        other => other,
    }
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

/// How gh clones github.com repos (`git_protocol`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum GitProtocol {
    Https,
    Ssh,
}

/// gh's `git_protocol` for github.com, read from its local config (no network): the host's
/// value, else the global one. HTTPS, gh's default, when unset or gh can't say.
pub async fn git_protocol() -> GitProtocol {
    process::run(
        "gh",
        ["config", "get", "git_protocol", "--host", "github.com"],
        None,
    )
    .await
    .map_or(GitProtocol::Https, |out| parse_git_protocol(&out.stdout))
}

/// `ssh` -> `Ssh`; anything else is gh's default, HTTPS.
fn parse_git_protocol(stdout: &str) -> GitProtocol {
    if stdout.trim().eq_ignore_ascii_case("ssh") {
        GitProtocol::Ssh
    } else {
        GitProtocol::Https
    }
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
    fn reads_gh_git_protocol() {
        assert_eq!(parse_git_protocol("ssh"), GitProtocol::Ssh);
        assert_eq!(parse_git_protocol("ssh\n"), GitProtocol::Ssh);
        assert_eq!(parse_git_protocol("https"), GitProtocol::Https);
        assert_eq!(parse_git_protocol(""), GitProtocol::Https);
    }

    #[test]
    fn one_job_per_tool_and_cancel_reaches_it() {
        let jobs = ToolJobs::default();
        let cancel = jobs.start(Tool::Gh, JobKind::Install).unwrap();
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

        assert!(!*cancel.borrow());
        jobs.cancel(Tool::Gh);
        assert!(*cancel.borrow());

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
    fn reads_winget_download_sizes() {
        let share = downloaded_share("  \u{2588}\u{2588}\u{2592}  1.00 MB / 10.0 MB").unwrap();
        assert!((share - 0.1).abs() < 0.001, "{share}");
        let share = downloaded_share("512 KB / 1,00 MB").unwrap();
        assert!((share - 0.5).abs() < 0.001, "{share}");
        assert_eq!(downloaded_share("12 MB / 1 MB"), None);
        assert_eq!(downloaded_share("read / write"), None);
        assert_eq!(progress_of("10.0 MB / 10.0 MB"), Some(0.65));
    }

    #[test]
    fn finds_an_outdated_gh() {
        assert_eq!(parse_semver("2.100.0"), Some((2, 100, 0)));
        assert_eq!(parse_semver("v1.58.1"), Some((1, 58, 1)));
        assert_eq!(parse_semver("2.47.0.windows.1"), Some((2, 47, 0)));
        assert_eq!(parse_semver("2.81.0-rc1"), Some((2, 81, 0)));
        assert_eq!(parse_semver("nope"), None);
        assert!(parse_semver("2.46.0").unwrap() < MIN_GH);
        assert!(parse_semver("2.81.0").unwrap() >= MIN_GH);
        assert!(parse_semver("2.100.0").unwrap() >= MIN_GH);
        let gh = |v: &str| ToolStatus::new(Tool::Gh, true, Some(v.into())).outdated;
        assert!(gh("2.46.0"));
        assert!(!gh("2.81.0"));
    }

    #[test]
    fn finds_an_outdated_gh_student() {
        let outdated = |v: &str| ToolStatus::new(Tool::GhStudent, true, Some(v.into())).outdated;
        assert!(outdated("v1.55.0"));
        assert!(outdated("v1.52.2"));
        assert!(outdated("1.9.0"));
        assert!(!outdated("v1.56.0"));
        assert!(!outdated("v1.58.1"));
        assert!(!outdated("v2.0.0"));
        // A git clone of the extension has no version: not called outdated.
        assert!(!ToolStatus::new(Tool::GhStudent, true, None).outdated);
        assert!(!ToolStatus::new(Tool::GhStudent, false, None).outdated);
        // git has no minimum.
        assert!(!ToolStatus::new(Tool::Git, true, Some("1.0.0".into())).outdated);
    }

    #[test]
    fn reads_an_installed_extension_without_gh() {
        let dir = std::env::temp_dir().join(format!("c50-ext-{}", std::process::id()));
        let binary = dir.join("binary");
        let script = dir.join("script");
        std::fs::create_dir_all(&binary).unwrap();
        std::fs::create_dir_all(script.join(".git")).unwrap();
        std::fs::write(
            binary.join("manifest.yml"),
            "owner: foundation50\nname: gh-student\nhost: github.com\ntag: v1.58.1\nispinned: false\n",
        )
        .unwrap();
        assert_eq!(installed_extension(&binary), (true, Some("v1.58.1".into())));
        assert_eq!(installed_extension(&script), (true, None));
        assert_eq!(installed_extension(&dir.join("missing")), (false, None));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_signed_out_gh_is_not_signed_in() {
        let failed = |program: &str, code| Error::CommandFailed {
            program: program.into(),
            code: Some(code),
            stderr: "To get started with GitHub CLI, please run:  gh auth login".into(),
        };
        assert!(matches!(signed_out(failed("gh", 4)), Error::NotSignedIn));
        assert!(matches!(
            signed_out(failed("gh", 1)),
            Error::CommandFailed { .. }
        ));
        assert!(matches!(
            signed_out(failed("git", 4)),
            Error::CommandFailed { .. }
        ));
    }

    #[test]
    fn gh_student_installs_through_gh() {
        let plan = install_plan(Tool::GhStudent).unwrap();
        assert_eq!(plan.program, "gh");
        assert_eq!(plan.args, ["extension", "install", GH_STUDENT_REPO]);
        assert!(!plan.detached && plan.fetch.is_none());
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

    /// The WebDriver suite's fake gh answers like gh does, for every parser it feeds.
    #[cfg(unix)]
    #[test]
    fn the_suites_fake_gh_reads_like_gh() {
        use crate::testenv::fake_gh;
        let version = parse_gh_version(&fake_gh(&["--version"])).unwrap();
        assert!(parse_semver(&version).unwrap() >= MIN_GH, "{version}");
        assert_eq!(
            parse_extension_version(&fake_gh(&["extension", "list"]), GH_STUDENT_REPO).as_deref(),
            Some("v1.58.1")
        );
        let (login, scopes) = parse_user_response(&fake_gh(&["api", "--include", "user"]));
        assert_eq!(login.as_deref(), Some("ada-e2e"));
        assert_eq!(missing_scopes(&scopes.unwrap()), Vec::<String>::new());
        assert_eq!(
            fake_gh(&["config", "get", "user", "-h", "github.com"]),
            "ada-e2e"
        );
        assert_eq!(
            parse_git_protocol(&fake_gh(&[
                "config",
                "get",
                "git_protocol",
                "--host",
                "github.com"
            ])),
            GitProtocol::Https
        );
        assert!(!fake_gh(&["auth", "token", "--hostname", "github.com"]).is_empty());
        // The dry run says "already up to date" on stderr: no update to offer.
        assert_eq!(
            parse_dry_run(&fake_gh(&[
                "extension",
                "upgrade",
                "gh-student",
                "--dry-run"
            ])),
            None
        );
    }
}
