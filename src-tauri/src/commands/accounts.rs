//! gh's accounts: sign-in, scopes, switching, avatars, sign-out.

use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::login::{self, ExtraScope, LoginState};
use crate::projects;
use crate::ssh::SshState;
use crate::tools::{self, AuthStatus, GitProtocol};

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn auth_status() -> AuthStatus {
    tools::auth_status().await
}

/// The github.com accounts gh is signed in to, the active one first.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn list_accounts() -> Result<Vec<crate::accounts::Account>> {
    crate::accounts::list().await
}

/// Switches gh to `login`, then syncs that account's projects.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn switch_account(app: AppHandle, login: String) -> Result<AuthStatus> {
    crate::accounts::switch(&login).await?;
    // SSH's status names the active account.
    app.state::<SshState>().clear();
    let status = tools::auth_status().await;
    tauri::async_runtime::spawn(async move {
        projects::refresh(&app).await;
    });
    Ok(status)
}

/// The account's avatar as a `data:` URL (cached, revalidated daily with its ETag).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn account_avatar(app: AppHandle, login: String) -> Result<String> {
    let http = app.state::<crate::github::Http>().0.clone();
    let cache_dir = app.path().app_cache_dir()?;
    crate::accounts::avatar(&http, &cache_dir, &login).await
}

/// Runs gh's device flow; the code arrives through the `login-device-code` event. The new
/// account is gh's active one afterwards, so its projects are synced right away. `protocol`
/// sets gh's `git_protocol` for github.com (`None` keeps it); `ssh_key` also asks for the
/// scope `set_up_ssh_key` needs.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn login(
    app: AppHandle,
    state: State<'_, LoginState>,
    protocol: Option<GitProtocol>,
    ssh_key: bool,
) -> Result<AuthStatus> {
    let request = login::Request {
        protocol,
        extra: if ssh_key {
            vec![ExtraScope::WritePublicKey]
        } else {
            Vec::new()
        },
    };
    let result = login::login(&app, &state, login::Flow::SignIn, &request).await;
    if protocol.is_some() {
        app.state::<SshState>().clear();
    }
    signed_in(app.clone(), result)
}

/// Adds gh-student's missing scopes, and `extra`, to the active account (`gh auth refresh`,
/// which keeps the token's other scopes), through the same device flow as `login`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn grant_scopes(
    app: AppHandle,
    state: State<'_, LoginState>,
    extra: Vec<ExtraScope>,
) -> Result<AuthStatus> {
    let request = login::Request {
        protocol: None,
        extra,
    };
    signed_in(
        app.clone(),
        login::login(&app, &state, login::Flow::GrantScopes, &request).await,
    )
}

fn signed_in(app: AppHandle, result: Result<AuthStatus>) -> Result<AuthStatus> {
    if result.as_ref().is_ok_and(|status| status.logged_in) {
        // A view that asks for a sync too shares this one.
        tauri::async_runtime::spawn(async move {
            projects::refresh(&app).await;
        });
    }
    result
}

/// Signs `login` out of gh, then syncs whichever account is left (or clears the list).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn sign_out(app: AppHandle, login: String) -> Result<AuthStatus> {
    crate::accounts::sign_out(&login).await?;
    app.state::<SshState>().clear();
    let status = tools::auth_status().await;
    tauri::async_runtime::spawn(async move {
        projects::refresh(&app).await;
    });
    Ok(status)
}

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn cancel_login(state: State<'_, LoginState>) {
    state.cancel();
}

/// Whether git signs in to github.com with gh's account outside the app.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn git_credentials() -> Result<crate::credentials::GitCredentials> {
    crate::credentials::status().await
}

/// `gh auth setup-git`: git signs in to github.com with gh's account everywhere. Writes the
/// user's global git config, so only from a click the UI confirmed.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn set_up_git_credentials() -> Result<crate::credentials::GitCredentials> {
    crate::credentials::set_up().await
}
