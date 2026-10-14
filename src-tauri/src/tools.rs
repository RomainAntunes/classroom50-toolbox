//! Detection and installation of the student prerequisites: git, gh and gh-student.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::platform::{self, CommandSpec};
use crate::process;

pub const GH_STUDENT_REPO: &str = "foundation50/gh-student";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

pub async fn install(tool: Tool) -> Result<()> {
    let spec = install_command(tool)?;
    if tool == Tool::GhStudent && process::find_in_path("gh").is_none() {
        return Err(Error::NotInstalled {
            program: "gh".into(),
        });
    }
    process::run(&spec.program, &spec.args, None).await?;
    Ok(())
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

pub async fn gh_token() -> Result<String> {
    Ok(process::run("gh", ["auth", "token"], None).await?.stdout)
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
    fn finds_gh_student_in_extension_list() {
        let out = "gh teacher\tfoundation50/gh-teacher\tv1.42.0\ngh student\tfoundation50/gh-student\tv1.41.2";
        assert_eq!(
            parse_extension_version(out, GH_STUDENT_REPO).as_deref(),
            Some("v1.41.2")
        );
        assert_eq!(parse_extension_version("", GH_STUDENT_REPO), None);
    }
}
