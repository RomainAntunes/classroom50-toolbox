//! In-app GitHub sign-in through gh's device flow.
//!
//! Without a TTY, `gh auth login --web` prints the one-time code and the device URL on
//! stderr, then polls until the user approves in the browser. We forward the code to the UI
//! and wait for gh to exit. The token stays in gh's own store (keychain where available).

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Notify;

use crate::error::{Error, Result};
use crate::process;
use crate::tools::{self, AuthStatus};

pub const DEVICE_CODE_EVENT: &str = "login-device-code";

/// The scopes `gh student login` requests; one sign-in covers gh-student too.
const SCOPES: &str = "admin:org,read:org,repo,workflow";
const DEFAULT_DEVICE_URL: &str = "https://github.com/login/device";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub code: String,
    pub url: String,
}

#[derive(Default)]
pub struct LoginState {
    cancel: Notify,
}

impl LoginState {
    pub fn cancel(&self) {
        self.cancel.notify_waiters();
    }
}

pub async fn login(app: &AppHandle, state: &LoginState) -> Result<AuthStatus> {
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
        () = state.cancel.notified() => {
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
