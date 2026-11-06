//! git over SSH or HTTPS: gh's protocol for github.com and whether SSH to GitHub works.

use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::github::Http;
use crate::ssh::{self, SshState, SshStatus};
use crate::tools::{self, GitProtocol};

/// A fresh probe, kept as the last status.
async fn probe(app: &AppHandle) -> SshStatus {
    let http = app.state::<Http>().0.clone();
    let active = tools::active_login().await;
    ssh::status(&http, active.as_deref()).await
}

/// Whether git reaches GitHub over SSH with the active account, and if not why. Each probe
/// connects to GitHub, so the last answer is reused for a minute unless `fresh`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn ssh_status(app: AppHandle, fresh: bool) -> SshStatus {
    app.state::<SshState>().status(fresh, probe(&app)).await
}

/// Adds GitHub's published SSH host keys `known_hosts` lacks, from a click the UI
/// confirmed, then probes again.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn trust_github_host(app: AppHandle, state: State<'_, SshState>) -> Result<SshStatus> {
    state.clear().await;
    let http = app.state::<Http>().0.clone();
    ssh::trust_github(&http).await?;
    Ok(state.status(true, probe(&app)).await)
}

/// Replaces the GitHub lines of `known_hosts` (an old or wrong host key) with GitHub's
/// published keys, from a click the UI confirmed, then probes again.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn replace_github_host_key(
    app: AppHandle,
    state: State<'_, SshState>,
) -> Result<SshStatus> {
    state.clear().await;
    let http = app.state::<Http>().0.clone();
    ssh::replace_github_host_key(&http).await?;
    Ok(state.status(true, probe(&app)).await)
}

/// Puts a key of this computer on the active account (`gh ssh-key add`, making
/// `~/.ssh/id_ed25519` first when there's none), then probes again. Needs the token's
/// `write:public_key` (`AuthStatus.canAddSshKeys`, `grant_scopes(["writePublicKey"])`).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn set_up_ssh_key(app: AppHandle, state: State<'_, SshState>) -> Result<SshStatus> {
    state.clear().await;
    let http = app.state::<Http>().0.clone();
    let active = tools::active_login().await;
    let status = ssh::set_up_key(&http, active.as_deref()).await?;
    state.store(status.clone()).await;
    Ok(status)
}

/// Sets gh's `git_protocol` for github.com (every gh account, and `gh repo clone` in a
/// terminal, follow it) and returns what gh reads back.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn set_git_protocol(
    state: State<'_, SshState>,
    protocol: GitProtocol,
) -> Result<GitProtocol> {
    state.clear().await;
    tools::set_git_protocol(protocol).await
}
