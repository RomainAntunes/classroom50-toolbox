//! gh's accounts: sign-in, scopes, switching, avatars, sign-out.

use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::login::{self, LoginState};
use crate::projects;
use crate::tools::{self, AuthStatus};

#[tauri::command]
pub async fn auth_status() -> AuthStatus {
    tools::auth_status().await
}

/// The github.com accounts gh is signed in to, the active one first.
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<crate::accounts::Account>> {
    crate::accounts::list().await
}

/// Switches gh to `login`, then syncs that account's projects.
#[tauri::command]
pub async fn switch_account(app: AppHandle, login: String) -> Result<AuthStatus> {
    crate::accounts::switch(&login).await?;
    let status = tools::auth_status().await;
    tauri::async_runtime::spawn(async move {
        projects::refresh(&app).await;
    });
    Ok(status)
}

/// The account's avatar as a `data:` URL (cached for a week).
#[tauri::command]
pub async fn account_avatar(app: AppHandle, login: String) -> Result<String> {
    let http = app.state::<crate::github::Http>().0.clone();
    let cache_dir = app.path().app_cache_dir()?;
    crate::accounts::avatar(&http, &cache_dir, &login).await
}

/// Runs gh's device flow; the code arrives through the `login-device-code` event. The new
/// account is gh's active one afterwards, so its projects are synced right away.
#[tauri::command]
pub async fn login(app: AppHandle, state: State<'_, LoginState>) -> Result<AuthStatus> {
    signed_in(
        app.clone(),
        login::login(&app, &state, login::Flow::SignIn).await,
    )
}

/// Adds gh-student's missing scopes to the active account (`gh auth refresh`), through the
/// same device flow as `login`.
#[tauri::command]
pub async fn grant_scopes(app: AppHandle, state: State<'_, LoginState>) -> Result<AuthStatus> {
    signed_in(
        app.clone(),
        login::login(&app, &state, login::Flow::GrantScopes).await,
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
pub async fn sign_out(app: AppHandle, login: String) -> Result<AuthStatus> {
    crate::accounts::sign_out(&login).await?;
    let status = tools::auth_status().await;
    tauri::async_runtime::spawn(async move {
        projects::refresh(&app).await;
    });
    Ok(status)
}

#[tauri::command]
pub fn cancel_login(state: State<'_, LoginState>) {
    state.cancel();
}
