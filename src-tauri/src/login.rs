//! In-app GitHub sign-in through gh's device flow.
//!
//! Without a TTY, `gh auth login --web` prints the one-time code and the device URL on
//! stderr, then polls until the user approves in the browser. We forward the code to the UI
//! and wait for gh to exit. The token stays in gh's own store (keychain where available).

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};

use tokio::sync::oneshot;

use crate::error::{Error, Result};
use crate::process;
use crate::tools::{self, AuthStatus};

pub const DEVICE_CODE_EVENT: &str = "login-device-code";
/// Sent however a sign-in ends (success, failure or cancel), so every view resets.
pub const ENDED_EVENT: &str = "login-ended";

/// The scopes `gh student login` requests; one sign-in covers gh-student too.
const SCOPES: &str = "admin:org,read:org,repo,workflow";
const DEFAULT_DEVICE_URL: &str = "https://github.com/login/device";

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

pub async fn login(app: &AppHandle, state: &LoginState) -> Result<AuthStatus> {
    let result = run(app, state).await;
    if let Ok(mut cancel) = state.cancel.lock() {
        cancel.take();
    }
    let _ = app.emit(ENDED_EVENT, ());
    result
}

async fn run(app: &AppHandle, state: &LoginState) -> Result<AuthStatus> {
    // Checked before anything is spawned or a running sign-in is abandoned.
    require_gh(process::find_in_path("gh"))?;
    let (cancel_tx, cancel_rx) = oneshot::channel();
    if let Ok(mut cancel) = state.cancel.lock() {
        // A sign-in still running from an earlier click is abandoned.
        if let Some(previous) = cancel.replace(cancel_tx) {
            let _ = previous.send(());
        }
    }
    let mut child = process::spawn_piped(
        "gh",
        [
            "auth",
            "login",
            "--web",
            "--hostname",
            "github.com",
            "--git-protocol",
            "https",
            "--scopes",
            SCOPES,
        ],
    )?;
    let stderr = child.stderr.take().expect("stderr is piped");

    let emitter = app.clone();
    let reader = tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut code: Option<String> = None;
        let mut transcript = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(found) = parse_code(&line) {
                code = Some(found);
            }
            let url = parse_url(&line);
            if let (Some(code), Some(url)) = (&code, url) {
                let _ = emitter.emit(
                    DEVICE_CODE_EVENT,
                    DeviceCode {
                        code: code.clone(),
                        url,
                    },
                );
            }
            transcript.push(line);
        }
        transcript.join("\n")
    });

    let status = tokio::select! {
        status = child.wait() => status?,
        _ = cancel_rx => {
            let _ = child.kill().await;
            return Err(Error::Cancelled);
        }
    };
    let transcript = reader.await.unwrap_or_default();
    if !status.success() {
        return Err(Error::CommandFailed {
            program: "gh auth login".into(),
            code: status.code(),
            stderr: transcript,
        });
    }
    Ok(tools::auth_status().await)
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

/// `! First copy your one-time code: ABCD-1234`
fn parse_code(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("one-time code:")?;
    let code = rest.trim();
    (!code.is_empty()).then(|| code.to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_without_gh() {
        assert!(matches!(
            require_gh(None),
            Err(Error::NotInstalled { program }) if program == "gh"
        ));
        assert!(require_gh(Some(PathBuf::from("/opt/homebrew/bin/gh"))).is_ok());
    }

    #[test]
    fn parses_device_flow_output() {
        assert_eq!(
            parse_code("! First copy your one-time code: AB12-CD34").as_deref(),
            Some("AB12-CD34")
        );
        assert_eq!(parse_code("something else"), None);
        assert_eq!(
            parse_url(
                "Open this URL to continue in your web browser: https://github.com/login/device"
            )
            .as_deref(),
            Some("https://github.com/login/device")
        );
        assert_eq!(
            parse_url("Press Enter to open https://github.com/login/device in your browser...")
                .as_deref(),
            Some("https://github.com/login/device")
        );
        assert_eq!(parse_url("✓ Logged in as alice"), None);
    }
}
