//! Whether git signs in to github.com with gh's login, for a push or pull outside the app
//! (a terminal, an editor). The app's own Pull and Commit & push pass gh's credentials for
//! the one command (`pull::GH_CREDENTIALS`) and need none of this. Setting it up runs
//! `gh auth setup-git`, which writes the helper to the user's global git config, so it only
//! runs from a click.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::process;

/// The helper gh writes (`!/path/to/gh auth git-credential`).
const GH_HELPER: &str = "auth git-credential";

/// The helpers git asks for github.com's passwords, in order.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GitCredentials {
    /// gh is one of them: `git push` to github.com signs in with gh's account.
    pub uses_gh: bool,
    /// The others by short name (`osxkeychain`, `manager`, `store`...), which setting up gh
    /// replaces for github.com (it resets the list there first).
    pub others: Vec<String>,
}

/// Reads every credential helper git knows (system, global and included files, never a
/// repo's: it runs from the temp folder).
pub async fn status() -> Result<GitCredentials> {
    let out = process::run(
        "git",
        [
            "config",
            "--includes",
            "--get-regexp",
            r"^credential\..*helper$",
        ],
        Some(&std::env::temp_dir()),
    )
    .await;
    match out {
        Ok(out) => Ok(parse(&out.stdout)),
        // No match at all.
        Err(Error::CommandFailed { code: Some(1), .. }) => Ok(GitCredentials::default()),
        Err(err) => Err(err),
    }
}

/// Runs `gh auth setup-git` for github.com, then reads the result back.
pub async fn set_up() -> Result<GitCredentials> {
    let out = process::run(
        "gh",
        ["auth", "setup-git", "--hostname", "github.com"],
        None,
    )
    .await;
    match out {
        Ok(_) => status().await,
        // `You are not logged into the GitHub host "github.com"`.
        Err(Error::CommandFailed {
            program, stderr, ..
        }) if program == "gh" && stderr.contains("not logged in") => Err(Error::NotSignedIn),
        Err(err) => Err(err),
    }
}

/// `git config --get-regexp` lines (`<key> <value>`, config order) to the helpers git uses
/// for `https://github.com`: the generic `credential.helper` and the github.com section's,
/// an empty value clearing those listed before it, as git reads them.
fn parse(stdout: &str) -> GitCredentials {
    let mut helpers: Vec<&str> = Vec::new();
    for line in stdout.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let Some(rest) = key.strip_prefix("credential.") else {
            continue;
        };
        let Some(scope) = rest.strip_suffix("helper") else {
            continue;
        };
        let applies = match scope.strip_suffix('.') {
            None => scope.is_empty(),
            Some(url) => {
                let url = url.trim_end_matches('/');
                url.eq_ignore_ascii_case("https://github.com")
            }
        };
        if !applies {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            helpers.clear();
        } else {
            helpers.push(value);
        }
    }
    let mut credentials = GitCredentials::default();
    for helper in helpers {
        if helper.contains(GH_HELPER) {
            credentials.uses_gh = true;
        } else {
            let name = short_name(helper);
            if !credentials.others.contains(&name) {
                credentials.others.push(name);
            }
        }
    }
    credentials
}

/// `osxkeychain`, `!/usr/local/bin/git-credential-manager` or
/// `C:/Program\ Files/Git/mingw64/bin/git-credential-manager.exe` -> `osxkeychain`, `manager`;
/// a shell snippet (`!f() { ... }; f`) -> `f()`.
fn short_name(helper: &str) -> String {
    let command = helper.trim_start_matches('!');
    // A program's path may hold spaces, its git-credential- name doesn't.
    let first = match command.rfind("git-credential-") {
        Some(at) => &command[at..],
        None => command,
    };
    let first = first.split_whitespace().next().unwrap_or(first);
    let file = first.rsplit(['/', '\\']).next().unwrap_or(first);
    let file = file.strip_suffix(".exe").unwrap_or(file);
    file.strip_prefix("git-credential-")
        .unwrap_or(file)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gh_after_its_reset() {
        // What `gh auth setup-git` leaves, after a generic helper.
        let out = "credential.helper osxkeychain\n\
            credential.https://github.com.helper \n\
            credential.https://github.com.helper !/opt/homebrew/bin/gh auth git-credential\n\
            credential.https://gist.github.com.helper \n\
            credential.https://gist.github.com.helper !/opt/homebrew/bin/gh auth git-credential\n";
        assert_eq!(
            parse(out),
            GitCredentials {
                uses_gh: true,
                others: vec![]
            }
        );
    }

    #[test]
    fn names_the_other_helpers() {
        // Git for Windows' system config, and a user's own store.
        let out = "credential.helper manager\n\
            credential.helper C:/Program\\ Files/Git/mingw64/bin/git-credential-manager.exe\n\
            credential.https://dev.azure.com.usehttppath true\n\
            credential.https://gitlab.com.helper store\n";
        assert_eq!(
            parse(out),
            GitCredentials {
                uses_gh: false,
                others: vec!["manager".into()]
            }
        );
        assert_eq!(
            parse("credential.helper store --file ~/.git-creds\n").others,
            ["store"]
        );
        assert_eq!(
            parse("credential.helper !f() { echo; }; f\n").others,
            ["f()"]
        );
    }

    #[test]
    fn a_reset_drops_what_came_before() {
        let out = "credential.https://github.com/.helper !gh auth git-credential\n\
            credential.helper \n\
            credential.helper cache\n";
        assert_eq!(
            parse(out),
            GitCredentials {
                uses_gh: false,
                others: vec!["cache".into()]
            }
        );
        assert_eq!(parse(""), GitCredentials::default());
    }
}
