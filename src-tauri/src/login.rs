//! In-app GitHub sign-in through gh's device flow.
//!
//! `gh student login` can't be used: it refuses to run without a terminal. So the app runs
//! the command it wraps, `gh auth login --web` with the same scopes. Without a TTY (stdin
//! is null and `GH_PROMPT_DISABLED` is set) gh is non-interactive: it never prompts, never
//! waits for Enter and never opens a browser itself. It prints the one-time code and the
//! device URL on stderr, then polls until the user approves in the browser. We forward the
//! code to the UI (which opens the URL) and wait for gh to exit. The token stays in gh's
//! own store (keychain where available).
//!
//! The same flow widens an existing login with `gh auth refresh`, which keeps the account's
//! current scopes and adds the ones gh-student needs.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::error::{Error, Result};
use crate::events::{IpcEvent, LoginDeviceCode, LoginEnded};
use crate::process;
use crate::tools::{self, AuthStatus};

/// The scopes `gh student login` requests (upstream `contract.RequiredOAuthScopes`), on
/// top of gh's own `repo read:org gist`; one sign-in covers gh-student too.
pub const REQUIRED_SCOPES: [&str; 4] = ["admin:org", "read:org", "repo", "workflow"];
/// Not needed by gh-student: lets GraphQL read `User.email` (a group member's public
/// address, for the calendar notes). Requested on sign-in, never reported as missing.
const EXTRA_SCOPES: [&str; 1] = ["user:email"];
const DEFAULT_DEVICE_URL: &str = "https://github.com/login/device";
/// GitHub's device codes expire after 15 minutes and gh then gives up on its own; this
/// only catches a gh that hangs past that.
const TIMEOUT: Duration = Duration::from_secs(16 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// `gh auth login`: a new account, or a fresh token for one gh already holds.
    SignIn,
    /// `gh auth refresh`: adds the missing scopes to the active account's token.
    GrantScopes,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub code: String,
    pub url: String,
}

/// The cancel switch of the sign-in in progress, if any. A channel, unlike a notification,
/// can't miss a cancel that arrives between two polls.
#[derive(Default)]
pub struct LoginState {
    cancel: Mutex<Option<oneshot::Sender<()>>>,
}

impl LoginState {
    pub fn cancel(&self) {
        if let Some(cancel) = self.cancel.lock().ok().and_then(|mut c| c.take()) {
            let _ = cancel.send(());
        }
    }
}

pub async fn login(app: &AppHandle, state: &LoginState, flow: Flow) -> Result<AuthStatus> {
    let result = run(app, state, flow).await;
    if let Ok(mut cancel) = state.cancel.lock() {
        cancel.take();
    }
    let _ = LoginEnded::emit(app, &());
    result
}

async fn run(app: &AppHandle, state: &LoginState, flow: Flow) -> Result<AuthStatus> {
    // Checked before anything is spawned or a running sign-in is abandoned.
    require_gh(process::find_in_path("gh"))?;
    // gh refuses to store a login while one of these is set, after the device flow
    // would have been shown; say so before.
    if let Some(var) = tools::env_token_var() {
        return Err(Error::TokenFromEnv(var.into()));
    }
    let (cancel_tx, cancel_rx) = oneshot::channel();
    if let Ok(mut cancel) = state.cancel.lock() {
        // A sign-in still running from an earlier click is abandoned.
        if let Some(previous) = cancel.replace(cancel_tx) {
            let _ = previous.send(());
        }
    }
    let mut child = process::spawn_piped("gh", args(flow))?;

    // gh writes everything to stderr; stdout is drained too so it can never fill up.
    let (tx, mut rx) = mpsc::unbounded_channel();
    forward_lines(child.stdout.take(), tx.clone());
    forward_lines(child.stderr.take(), tx);
    let emitter = app.clone();
    let reader = tauri::async_runtime::spawn(async move {
        let mut parser = DeviceParser::default();
        let mut transcript = Vec::new();
        while let Some(line) = rx.recv().await {
            let line = clean(&line);
            if let Some(device) = parser.feed(&line) {
                let _ = LoginDeviceCode::emit(&emitter, &device);
            }
            if !line.is_empty() {
                transcript.push(line);
            }
        }
        transcript
    });

    let waited = tokio::select! {
        status = tokio::time::timeout(TIMEOUT, child.wait()) => status,
        _ = cancel_rx => {
            process::kill_tree(&mut child).await;
            return Err(Error::Cancelled);
        }
    };
    let status = match waited {
        Ok(status) => status?,
        Err(_) => {
            process::kill_tree(&mut child).await;
            let transcript = reader.await.unwrap_or_default();
            return Err(Error::TimedOut {
                program: program_name(flow).into(),
                minutes: TIMEOUT.as_secs() / 60,
                output: failure_message(&transcript),
            });
        }
    };
    let transcript = reader.await.unwrap_or_default();
    if !status.success() {
        if let Some(var) = env_token_refusal(&transcript) {
            return Err(Error::TokenFromEnv(var));
        }
        return Err(Error::CommandFailed {
            program: program_name(flow).into(),
            code: status.code(),
            stderr: failure_message(&transcript),
        });
    }
    Ok(tools::auth_status().await)
}

/// No `--git-protocol`: gh's default is already https, and passing it would overwrite the
/// protocol of a user who chose ssh. Non-interactive gh doesn't set up git's credential
/// helper either, so the user's git config is left alone.
fn args(flow: Flow) -> Vec<String> {
    let scopes = REQUIRED_SCOPES
        .iter()
        .chain(EXTRA_SCOPES.iter())
        .copied()
        .collect::<Vec<_>>()
        .join(",");
    let mut args: Vec<String> = match flow {
        Flow::SignIn => vec!["auth".into(), "login".into(), "--web".into()],
        Flow::GrantScopes => vec!["auth".into(), "refresh".into()],
    };
    args.extend([
        "--hostname".into(),
        "github.com".into(),
        "--scopes".into(),
        scopes,
    ]);
    args
}

fn program_name(flow: Flow) -> &'static str {
    match flow {
        Flow::SignIn => "gh auth login",
        Flow::GrantScopes => "gh auth refresh",
    }
}

/// Sends each line `pipe` prints to `tx` until it closes. `lines()` splits on `\n` and
/// drops a `\r` before it (Windows).
fn forward_lines<R>(pipe: Option<R>, tx: mpsc::UnboundedSender<String>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let Some(pipe) = pipe else { return };
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

/// Sign-in is gh's device flow, so it needs gh on PATH.
fn require_gh(found: Option<PathBuf>) -> Result<()> {
    match found {
        Some(_) => Ok(()),
        None => Err(Error::NotInstalled {
            program: "gh".into(),
        }),
    }
}

/// Removes ANSI escape sequences (gh colors only on a TTY, but `CLICOLOR_FORCE` would
/// override that) and surrounding whitespace, `\r` included.
fn clean(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Parameters, then one final byte in `@`..=`~`.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// Reads gh's device-flow lines. gh prints the code, then the URL; the pair is reported
/// once, when both are known.
#[derive(Default)]
struct DeviceParser {
    code: Option<String>,
    sent: bool,
}

impl DeviceParser {
    fn feed(&mut self, line: &str) -> Option<DeviceCode> {
        if let Some(code) = parse_code(line) {
            self.code = Some(code);
            self.sent = false;
            return None;
        }
        let url = parse_url(line)?;
        let code = self.code.clone()?;
        if self.sent {
            return None;
        }
        self.sent = true;
        Some(DeviceCode { code, url })
    }
}

/// `! First copy your one-time code: ABCD-1234`, or with gh's clipboard setting on
/// (`gh config set clipboard enabled`, recent gh) `! One-time code (ABCD-1234) copied to
/// clipboard`. gh has no translations, so the wording is the same in every locale.
fn parse_code(line: &str) -> Option<String> {
    let code = if let Some((_, rest)) = line.split_once("one-time code:") {
        rest.trim()
    } else {
        let (_, rest) = line.split_once("One-time code (")?;
        rest.split_once(')')?.0.trim()
    };
    is_device_code(code).then(|| code.to_owned())
}

/// GitHub's user codes are 8 letters or digits split by a hyphen (`WDJB-MJHT`).
fn is_device_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 16
        && code.contains('-')
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `Open this URL to continue in your web browser: https://github.com/login/device`
/// (or the TTY wording `Press Enter to open https://github.com/login/device in your browser...`).
fn parse_url(line: &str) -> Option<String> {
    if !line.contains("github.com/login/device") {
        return None;
    }
    let url = line
        .split_whitespace()
        .find(|word| word.starts_with("https://"))
        .unwrap_or(DEFAULT_DEVICE_URL);
    Some(url.trim_end_matches(['.', ',']).to_owned())
}

/// `The value of the GH_TOKEN environment variable is being used for authentication.`
fn env_token_refusal(transcript: &[String]) -> Option<String> {
    transcript.iter().find_map(|line| {
        let (_, rest) = line.split_once("The value of the ")?;
        let (var, _) = rest.split_once(" environment variable")?;
        Some(var.trim().to_owned())
    })
}

/// What went wrong, without the device-flow lines: gh's error is its last lines.
fn failure_message(transcript: &[String]) -> String {
    let useful: Vec<&str> = transcript
        .iter()
        .map(String::as_str)
        .filter(|line| parse_code(line).is_none() && parse_url(line).is_none())
        .collect();
    let start = useful.len().saturating_sub(3);
    useful[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(output: &str) -> Vec<DeviceCode> {
        let mut parser = DeviceParser::default();
        output
            .split('\n')
            .filter_map(|line| parser.feed(&clean(line)))
            .collect()
    }

    fn code(code: &str) -> Vec<DeviceCode> {
        vec![DeviceCode {
            code: code.into(),
            url: DEFAULT_DEVICE_URL.into(),
        }]
    }

    #[test]
    fn refuses_without_gh() {
        assert!(matches!(
            require_gh(None),
            Err(Error::NotInstalled { program }) if program == "gh"
        ));
        assert!(require_gh(Some(PathBuf::from("/opt/homebrew/bin/gh"))).is_ok());
    }

    /// `gh auth login --web` with no TTY (cli/cli `internal/authflow/flow.go`): an empty
    /// notice line, the code, the URL, then the result after the browser step.
    #[test]
    fn parses_non_interactive_login() {
        let stderr = "\n\
            ! First copy your one-time code: AB12-CD34\n\
            Open this URL to continue in your web browser: https://github.com/login/device\n\
            ✓ Authentication complete.\n\
            ✓ Logged in as alice\n";
        assert_eq!(feed_all(stderr), code("AB12-CD34"));
    }

    /// Windows: the same lines through a pipe end in `\r\n`.
    #[test]
    fn parses_crlf_output() {
        let stderr = "\r\n! First copy your one-time code: WDJB-MJHT\r\n\
            Open this URL to continue in your web browser: https://github.com/login/device\r\n";
        assert_eq!(feed_all(stderr), code("WDJB-MJHT"));
    }

    /// `gh config set clipboard enabled` replaces the code line.
    #[test]
    fn parses_clipboard_wording() {
        let stderr = "! One-time code (9F3K-2QXA) copied to clipboard\n\
            Open this URL to continue in your web browser: https://github.com/login/device\n";
        assert_eq!(feed_all(stderr), code("9F3K-2QXA"));
        // The copy failed: gh falls back to the usual line.
        let failed = "! Failed to copy one-time code to clipboard\n  exit status 1\n\
            ! First copy your one-time code: 9F3K-2QXA\n\
            Open this URL to continue in your web browser: https://github.com/login/device\n";
        assert_eq!(feed_all(failed), code("9F3K-2QXA"));
    }

    /// Forced colors (`CLICOLOR_FORCE=1`) wrap the code and the label in escapes.
    #[test]
    fn parses_colored_output() {
        let stderr = "\u{1b}[0;33m!\u{1b}[0m First copy your one-time code: \u{1b}[0;1;39mAB12-CD34\u{1b}[0m\n\
            \u{1b}[0;1;39mOpen this URL\u{1b}[0m to continue in your web browser: https://github.com/login/device\n";
        assert_eq!(feed_all(stderr), code("AB12-CD34"));
    }

    /// The TTY wording (gh run with a terminal) still yields the URL.
    #[test]
    fn parses_interactive_wording() {
        let stderr = "! First copy your one-time code: AB12-CD34\n\
            Press Enter to open https://github.com/login/device in your browser... \n";
        assert_eq!(feed_all(stderr), code("AB12-CD34"));
    }

    #[test]
    fn reports_the_code_once() {
        let mut parser = DeviceParser::default();
        assert_eq!(
            parser.feed("! First copy your one-time code: AB12-CD34"),
            None
        );
        assert!(
            parser
                .feed("Open this URL: https://github.com/login/device")
                .is_some()
        );
        assert_eq!(
            parser.feed("Open this URL: https://github.com/login/device"),
            None
        );
        // A URL with no code before it is not enough.
        let mut fresh = DeviceParser::default();
        assert_eq!(fresh.feed("https://github.com/login/device"), None);
    }

    #[test]
    fn ignores_lines_that_only_look_like_codes() {
        assert_eq!(parse_code("something else"), None);
        assert_eq!(parse_code("! First copy your one-time code: "), None);
        assert_eq!(parse_url("✓ Logged in as alice"), None);
    }

    #[test]
    fn reads_the_env_token_refusal() {
        let transcript = vec![
            "The value of the GH_TOKEN environment variable is being used for authentication."
                .to_owned(),
            "To have GitHub CLI store credentials instead, first clear the value from the environment."
                .to_owned(),
        ];
        assert_eq!(env_token_refusal(&transcript).as_deref(), Some("GH_TOKEN"));
        assert_eq!(env_token_refusal(&["✓ Logged in as alice".into()]), None);
    }

    #[test]
    fn failure_message_drops_the_device_lines() {
        let transcript: Vec<String> = [
            "! First copy your one-time code: AB12-CD34",
            "Open this URL to continue in your web browser: https://github.com/login/device",
            "failed to authenticate via web browser: access_denied",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            failure_message(&transcript),
            "failed to authenticate via web browser: access_denied"
        );
    }

    #[test]
    fn requests_gh_students_scopes_without_touching_the_git_protocol() {
        let login = args(Flow::SignIn);
        assert_eq!(&login[..3], ["auth", "login", "--web"]);
        assert!(login.contains(&"admin:org,read:org,repo,workflow,user:email".to_owned()));
        assert!(!login.iter().any(|a| a == "--git-protocol"));
        let refresh = args(Flow::GrantScopes);
        assert_eq!(&refresh[..2], ["auth", "refresh"]);
        assert!(refresh.contains(&"--hostname".to_owned()));
    }
}
