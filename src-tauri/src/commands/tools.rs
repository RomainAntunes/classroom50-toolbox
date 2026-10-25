//! git, gh and gh-student: their status, installs and updates.

use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::platform;
use crate::tools::{self, Tool, ToolChecks, ToolJob, ToolJobs, ToolStatus};

#[tauri::command]
#[cfg_attr(test, specta::specta)]
/// Reused for a few seconds (`ToolChecks`), so a burst of Check again runs gh once.
pub async fn tool_statuses(app: AppHandle) -> Vec<ToolStatus> {
    app.state::<ToolChecks>().statuses().await
}

/// What `install_tool` will run, so the UI can show it before asking for elevation.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn install_command(tool: Tool) -> Result<platform::InstallPlan> {
    tools::install_plan(tool)
}

/// Installs `tool` (`tool-jobs` events report its output) and returns every tool's status:
/// gh-student depends on gh, so installing gh changes what the other rows can do.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn install_tool(
    app: AppHandle,
    checks: State<'_, ToolChecks>,
    tool: Tool,
) -> Result<Vec<ToolStatus>> {
    checks.forget(tool).await;
    tools::install(&app, tool).await?;
    let statuses = tools::statuses().await;
    checks.store(statuses.clone()).await;
    Ok(statuses)
}

/// The installs and updates still running, for a view opened after they started.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn tool_jobs(state: State<'_, ToolJobs>) -> Vec<ToolJob> {
    state.list()
}

/// Stops the install or update of `tool`; its command then fails with `cancelled`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub fn cancel_tool_job(state: State<'_, ToolJobs>, tool: Tool) {
    state.cancel(tool);
}

/// The newer version `upgrade_tool` would install, if any; reused for a while
/// (`ToolChecks`), since it asks GitHub.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn tool_update(checks: State<'_, ToolChecks>, tool: Tool) -> Result<Option<String>> {
    checks.update(tool).await
}

/// Updates gh-student and returns its new status.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn upgrade_tool(
    app: AppHandle,
    checks: State<'_, ToolChecks>,
    tool: Tool,
) -> Result<ToolStatus> {
    checks.forget(tool).await;
    let upgraded = tools::upgrade(&app, tool).await;
    // Whether or not it worked, the version may have moved: read it afresh next time.
    checks.forget(tool).await;
    upgraded?;
    Ok(tools::status(tool).await)
}

/// How gh clones github.com repos, so the row's clone URLs lead with the one it uses.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub async fn git_protocol() -> crate::tools::GitProtocol {
    crate::tools::git_protocol().await
}
