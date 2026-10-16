//! Hooks for the WebDriver smoke suite (`e2e/`), read from the environment at the first use.
//! Development builds only: a release build never reads these variables.
//!
//! - `C50_TEST_GITHUB`: the base URL of a mock GitHub (`http://127.0.0.1:<port>`). The API
//!   (GraphQL and REST), the Pages manifests, the avatars and the reachability probe go there
//!   instead (`github_url`, `probe_addr`).
//! - `C50_TEST_BIN`: a folder searched before PATH for programs, so the suite's fake `gh` runs
//!   while git stays the real one (`bin_dir`).
//! - `C50_TEST_OPEN=1`: the popover shows at start and doesn't hide when it loses focus, since
//!   the suite drives a window nobody clicks the tray icon for (`keep_open`).

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::OnceLock;

const GITHUB_VAR: &str = "C50_TEST_GITHUB";
const BIN_VAR: &str = "C50_TEST_BIN";
const OPEN_VAR: &str = "C50_TEST_OPEN";

/// `C50_TEST_GITHUB`, without a trailing slash.
fn github_base() -> Option<&'static str> {
    static BASE: OnceLock<Option<String>> = OnceLock::new();
    BASE.get_or_init(|| hook(GITHUB_VAR).map(|base| base.trim_end_matches('/').to_owned()))
        .as_deref()
}

/// `url`, sent to the mock GitHub when the suite runs one.
pub fn github_url(url: &str) -> Cow<'_, str> {
    match github_base() {
        Some(base) => Cow::Owned(rewrite(base, url)),
        None => Cow::Borrowed(url),
    }
}

/// Where the reachability probe connects instead of `api.github.com:443`.
pub fn probe_addr() -> Option<(String, u16)> {
    let url = reqwest::Url::parse(github_base()?).ok()?;
    Some((url.host_str()?.to_owned(), url.port_or_known_default()?))
}

/// The folder searched first for programs.
pub fn bin_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| hook(BIN_VAR).map(PathBuf::from)).clone()
}

/// The popover shows at start and stays open.
pub fn keep_open() -> bool {
    static OPEN: OnceLock<bool> = OnceLock::new();
    *OPEN.get_or_init(|| hook(OPEN_VAR).is_some_and(|v| v == "1"))
}

fn hook(name: &str) -> Option<String> {
    let value = gate(cfg!(debug_assertions), || std::env::var(name).ok());
    if value.is_some() {
        log::warn!("test hook {name} is set");
    }
    value
}

/// A hook's value: only in a development build (`dev`, where the variable is read at all),
/// and only when set to something.
fn gate(dev: bool, read: impl FnOnce() -> Option<String>) -> Option<String> {
    if !dev {
        return None;
    }
    read().filter(|value| !value.trim().is_empty())
}

/// GitHub's hosts mapped under `base`: `api.github.com/<path>` to `<base>/<path>`,
/// `github.com/<path>` to `<base>/web/<path>`, `<org>.github.io/<path>` to
/// `<base>/pages/<org>/<path>`. Anything else (a custom Pages host, a URL the mock handed out)
/// is left as is.
fn rewrite(base: &str, url: &str) -> String {
    let Some(rest) = url.strip_prefix("https://") else {
        return url.to_owned();
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    if host == "api.github.com" {
        return format!("{base}/{path}");
    }
    if host == "github.com" {
        return format!("{base}/web/{path}");
    }
    match host.strip_suffix(".github.io") {
        Some(org) if !org.is_empty() && !org.contains('.') => {
            format!("{base}/pages/{org}/{path}")
        }
        _ => url.to_owned(),
    }
}

/// Runs the suite's fake gh (`e2e/bin/gh`), so unit tests can check that it prints what the
/// parsers read: the suite itself only runs in CI. Its standard output, or a panic.
#[cfg(all(test, unix))]
pub(crate) fn fake_gh(args: &[&str]) -> String {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../e2e/bin/gh");
    let out = std::process::Command::new("sh")
        .arg(script)
        .args(args)
        .env_remove("C50_FAKE_GH_LOG")
        .env_remove("C50_FAKE_GH_LOGIN")
        .output()
        .expect("sh runs the fake gh");
    assert!(out.status.success(), "fake gh {args:?}: {out:?}");
    String::from_utf8(out.stdout)
        .expect("UTF-8")
        .trim_end()
        .to_owned()
}

/// An answer of the suite's mock GitHub (`e2e/mock/golden/<name>.json`, which
/// `e2e/mock/github.test.js` keeps equal to what the mock sends), for Rust to read as it
/// reads GitHub's.
#[cfg(test)]
pub(crate) fn golden(name: &str) -> String {
    let path = format!(
        "{}/../e2e/mock/golden/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_builds_ignore_the_hooks() {
        let read = || Some("http://127.0.0.1:9".to_owned());
        assert_eq!(gate(false, read), None);
        assert_eq!(gate(true, read).as_deref(), Some("http://127.0.0.1:9"));
        assert_eq!(gate(true, || Some(" ".to_owned())), None);
        assert_eq!(gate(true, || None), None);
        // A release build doesn't even look at the environment.
        assert_eq!(gate(false, || panic!("read in a release build")), None);
    }

    #[test]
    fn sends_github_hosts_to_the_mock() {
        let base = "http://127.0.0.1:4545";
        assert_eq!(
            rewrite(base, "https://api.github.com/graphql"),
            "http://127.0.0.1:4545/graphql"
        );
        assert_eq!(
            rewrite(base, "https://api.github.com/users/ada"),
            "http://127.0.0.1:4545/users/ada"
        );
        assert_eq!(
            rewrite(base, "https://github.com/ada.png?size=64"),
            "http://127.0.0.1:4545/web/ada.png?size=64"
        );
        assert_eq!(
            rewrite(
                base,
                "https://CS50.github.io/classroom50/fall/k3y9/assignments.json"
            ),
            "http://127.0.0.1:4545/pages/cs50/classroom50/fall/k3y9/assignments.json"
        );
        for kept in [
            "https://pages.cs50.edu/c50/fall/assignments.json",
            "https://evil.github.io.example/x",
            "https://a.b.github.io/x",
            "http://127.0.0.1:4545/assets/result.json",
        ] {
            assert_eq!(rewrite(base, kept), kept);
        }
    }
}
