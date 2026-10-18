//! IPC surface for the Svelte UI. Keep these thin: the logic lives in the domain modules.
//! Every command here has a typed wrapper in `src/lib/api.ts`. One file per area; each is
//! re-exported whole, so `commands::<name>` (and the hidden items `#[tauri::command]` makes
//! next to each function) reads the same from `lib.rs`.

mod accounts;
mod app;
mod calendar;
mod deeplink;
mod developer;
mod diagnostics;
mod editors;
#[cfg(test)]
mod fixtures;
mod git;
mod projects;
mod settings;
mod student;
mod tools;
mod updater;

pub use accounts::*;
pub use app::*;
pub use calendar::*;
pub use deeplink::*;
pub use developer::*;
pub use diagnostics::*;
pub use editors::*;
pub use git::*;
pub use projects::*;
pub use settings::*;
pub use student::*;
pub use tools::*;
pub use updater::*;
