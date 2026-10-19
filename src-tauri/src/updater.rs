//! Self-update from GitHub Releases (`latest.json`, signed with the Tauri updater key).
//!
//! One state machine (`Machine`) goes idle, checking, downloading, ready (or error) and is
//! sent as `update-state` on every change. Checks run at every start, every 6 hours while the app runs
//! and from Settings; a newer version downloads at once, in memory, and installs on
//! "Restart now" or, where the platform allows it, when the user quits.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::alerts::Notice;
use crate::error::{Error, ErrorKind, Result};
use crate::events::{self, IpcEvent};
use crate::settings::{SettingsStore, UpdateChannel};

const STABLE_URL: &str =
    "https://github.com/RomainAntunes/classroom50-toolbox/releases/latest/download/latest.json";
/// Rewritten by `beta-channel.yml` whenever a release is published: the newest of the newest
/// pre-release and the newest stable, so beta users also get the stable that follows.
const BETA_URL: &str = "https://github.com/RomainAntunes/classroom50-toolbox/releases/download/beta-channel/latest.json";

const MEMORY_FILE: &str = "update-check.json";
/// Startup stays quick: the first check waits for the sync and the UI to settle.
const STARTUP_DELAY: Duration = Duration::from_secs(20);
/// The background tick only compares clocks; a check runs when one is due.
const TICK: Duration = Duration::from_secs(60);
/// Checks while the app runs. A check made just before a release is published (or one that
/// read a stale feed) then hides it for this long at most.
const INTERVAL: chrono::TimeDelta = chrono::TimeDelta::hours(6);
/// After a failed check (offline, GitHub down), the next background one waits this long.
const RETRY: chrono::TimeDelta = chrono::TimeDelta::hours(1);
/// Progress events at most this often (about 10 a second).
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// How the running app was installed, which decides what updating can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Bundle {
    /// macOS `.app` (from the dmg).
    App,
    Msi,
    Nsis,
    AppImage,
    Deb,
    Rpm,
    /// Not installed from a bundle (built from source, a bare binary): no self-update.
    Unbundled,
}

impl Bundle {
    pub fn current() -> Self {
        use tauri::utils::config::BundleType;
        match tauri::utils::platform::bundle_type() {
            Some(BundleType::App | BundleType::Dmg) => Self::App,
            Some(BundleType::Msi) => Self::Msi,
            Some(BundleType::Nsis) => Self::Nsis,
            Some(BundleType::AppImage) => Self::AppImage,
            Some(BundleType::Deb) => Self::Deb,
            Some(BundleType::Rpm) => Self::Rpm,
            _ => Self::Unbundled,
        }
    }

    pub fn can_update(self) -> bool {
        self != Self::Unbundled
    }

    /// Installing needs no prompt, so it can run while the app quits: the `.app` swap,
    /// the NSIS installer (per user, passive, it shows its own progress) or the AppImage
    /// rewrite. A deb or rpm goes through `pkexec`, and the MSI installs for all users, so
    /// Windows asks for administrator rights; a prompt popping up after Quit would be a
    /// surprise, so those install on "Restart now" only.
    pub fn installs_on_quit(self) -> bool {
        matches!(self, Self::App | Self::Nsis | Self::AppImage)
    }

    /// `pkexec dpkg -i` / `pkexec rpm -U` (the updater plugin's own deb and rpm support).
    pub fn asks_password(self) -> bool {
        matches!(self, Self::Deb | Self::Rpm)
    }

    /// Pre-releases ship no MSI (WiX takes numeric versions only), and an MSI install fed
    /// the NSIS installer would end up installed twice, so MSI installs stay on stable.
    pub fn has_beta(self) -> bool {
        self != Self::Msi
    }
}

/// The `latest.json` a channel reads, given how the app was installed.
pub fn endpoint(channel: UpdateChannel, bundle: Bundle) -> &'static str {
    match channel {
        UpdateChannel::Beta if bundle.has_beta() => BETA_URL,
        _ => STABLE_URL,
    }
}

/// An error as the UI shows it (`errorMessage`): `error.<kind>`, else the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    kind: ErrorKind,
    message: String,
}

impl From<&Error> for Failure {
    fn from(err: &Error) -> Self {
        Self {
            kind: err.error_kind(),
            message: err.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(
    tag = "phase",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateState {
    Idle,
    Checking,
    Downloading {
        version: String,
        downloaded: u64,
        /// `None` when the server sent no length: the bar is indeterminate.
        total: Option<u64>,
    },
    Ready {
        version: String,
        notes: Option<String>,
    },
    /// The last check the user started, or the last download, failed.
    Error {
        error: Failure,
    },
}

/// What `update-state` carries and `update_status` returns.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    state: UpdateState,
    /// "Later" was chosen: the banner stays, without the question.
    deferred: bool,
    /// "Restart now" is running.
    installing: bool,
    /// The dev build's fake update (`simulate_update`): Restart only logs.
    simulated: bool,
    last_check: Option<DateTime<Utc>>,
    bundle: Bundle,
    can_update: bool,
    install_on_quit: bool,
    asks_password: bool,
    has_beta: bool,
}

/// The update state and what goes with it, without Tauri, so its transitions are tested.
/// `P` is the downloaded update (the plugin's `Update` and its bytes; `()` in tests).
/// Each check gets a generation; a reset (a channel change) bumps it, so a check or
/// download still running from before has its result dropped.
#[derive(Debug)]
struct Machine<P> {
    state: UpdateState,
    pending: Option<P>,
    generation: u64,
    deferred: bool,
    simulated: bool,
    installing: bool,
    last_emit: Option<Instant>,
}

impl<P> Default for Machine<P> {
    fn default() -> Self {
        Self {
            state: UpdateState::Idle,
            pending: None,
            generation: 0,
            deferred: false,
            simulated: false,
            installing: false,
            last_emit: None,
        }
    }
}

impl<P> Machine<P> {
    /// Starts a check, unless one is running or an update is downloading or waiting to be
    /// installed (the next start checks again). Returns its generation.
    fn begin_check(&mut self) -> Option<u64> {
        match self.state {
            UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Ready { .. } => {
                None
            }
            UpdateState::Idle | UpdateState::Error { .. } => {
                self.generation += 1;
                self.state = UpdateState::Checking;
                Some(self.generation)
            }
        }
    }

    /// The check found nothing newer. False when the check was reset meanwhile.
    fn up_to_date(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = UpdateState::Idle;
        true
    }

    /// The check found `version`; its download starts.
    fn found(&mut self, generation: u64, version: String) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = UpdateState::Downloading {
            version,
            downloaded: 0,
            total: None,
        };
        self.last_emit = None;
        true
    }

    /// A downloaded chunk. True when it's time to tell the UI: at most every
    /// `PROGRESS_EVERY`, and always for the last chunk.
    fn progress(&mut self, generation: u64, chunk: u64, length: Option<u64>, now: Instant) -> bool {
        if generation != self.generation {
            return false;
        }
        let UpdateState::Downloading {
            downloaded, total, ..
        } = &mut self.state
        else {
            return false;
        };
        *downloaded += chunk;
        *total = length;
        let done = length.is_some_and(|length| *downloaded >= length);
        let due = self
            .last_emit
            .is_none_or(|last| now.duration_since(last) >= PROGRESS_EVERY);
        if done || due {
            self.last_emit = Some(now);
            true
        } else {
            false
        }
    }

    /// The download finished and its signature checked out.
    fn downloaded(
        &mut self,
        generation: u64,
        version: String,
        notes: Option<String>,
        pending: P,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = UpdateState::Ready { version, notes };
        self.pending = Some(pending);
        self.deferred = false;
        true
    }

    /// A check or download failed. A background check goes back to idle quietly; one the
    /// user started, and any download, shows the error.
    fn failed(&mut self, generation: u64, failure: Failure, quiet: bool) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = if quiet {
            UpdateState::Idle
        } else {
            UpdateState::Error { error: failure }
        };
        self.pending = None;
        true
    }

    /// Forgets any check, download or downloaded update (the channel changed).
    fn reset(&mut self) {
        self.generation += 1;
        self.state = UpdateState::Idle;
        self.pending = None;
        self.deferred = false;
        self.simulated = false;
        self.installing = false;
    }
}

/// When the last check ran, kept in the app cache dir so a restart doesn't check again
/// needlessly and a laptop that slept through the day catches up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Memory {
    /// The last check that got an answer.
    checked_at: Option<DateTime<Utc>>,
    /// The app version and channel that check ran for: another one checks at once.
    version: Option<String>,
    channel: Option<UpdateChannel>,
    /// The last check that failed, which delays the next background one.
    failed_at: Option<DateTime<Utc>>,
}

impl Memory {
    fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) {
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(
                &tmp,
                serde_json::to_vec(self).map_err(std::io::Error::other)?,
            )?;
            std::fs::rename(&tmp, path)
        };
        if let Err(err) = write() {
            log::warn!("could not save {}: {err}", path.display());
        }
    }
}

/// Whether the next check is due (every start checks anyway): once `INTERVAL` has passed
/// since the last one, measured on the wall clock so the time a laptop slept counts, and
/// not within `RETRY` of a failure. Another version or channel checks at once, after the
/// same wait when the last try failed (offline right after an update).
fn due(memory: &Memory, now: DateTime<Utc>, version: &str, channel: UpdateChannel) -> bool {
    let rested = memory.failed_at.is_none_or(|at| now - at >= RETRY);
    let Some(checked_at) = memory.checked_at else {
        return rested;
    };
    if memory.version.as_deref() != Some(version) || memory.channel != Some(channel) {
        return rested;
    }
    // The clock went back: the stored time means nothing.
    if checked_at > now + chrono::TimeDelta::minutes(5) {
        return true;
    }
    now - checked_at >= INTERVAL && rested
}

/// The wall clock moved further than the monotonic one between two ticks: the machine
/// slept. The check then waits a tick, for the network to come back.
fn woke(wall: chrono::TimeDelta, monotonic: Duration) -> bool {
    wall.to_std()
        .is_ok_and(|wall| wall > monotonic + Duration::from_secs(120))
}

struct Pending {
    /// `None` for the dev build's simulated update.
    update: Option<Update>,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending")
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// The downloaded update lives in memory until installed: the plugin checks its signature
/// on these very bytes, so nothing on disk could be swapped between the check and the
/// install, and a release is about 10 MB. A restart drops it; the next check downloads
/// it again.
pub struct UpdaterState {
    machine: Mutex<Machine<Pending>>,
    memory: Mutex<Memory>,
    memory_path: PathBuf,
    bundle: Bundle,
    quitting: AtomicBool,
}

impl UpdaterState {
    pub fn load(cache_dir: &Path) -> Self {
        let memory_path = cache_dir.join(MEMORY_FILE);
        Self {
            machine: Mutex::new(Machine::default()),
            memory: Mutex::new(Memory::load(&memory_path)),
            memory_path,
            bundle: Bundle::current(),
            quitting: AtomicBool::new(false),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        let machine = self.machine.lock().unwrap_or_else(|e| e.into_inner());
        let last_check = self
            .memory
            .lock()
            .map(|memory| memory.checked_at)
            .unwrap_or_default();
        UpdateStatus {
            state: machine.state.clone(),
            deferred: machine.deferred,
            installing: machine.installing,
            simulated: machine.simulated,
            last_check,
            bundle: self.bundle,
            can_update: self.bundle.can_update(),
            install_on_quit: self.bundle.installs_on_quit(),
            asks_password: self.bundle.asks_password(),
            has_beta: self.bundle.has_beta(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Machine<Pending>) -> T) -> T {
        let mut machine = self.machine.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut machine)
    }

    fn remember(&self, f: impl FnOnce(&mut Memory)) {
        let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut memory);
        memory.save(&self.memory_path);
    }

    fn checked(&self, version: String, channel: UpdateChannel) {
        self.remember(|memory| {
            memory.checked_at = Some(Utc::now());
            memory.version = Some(version);
            memory.channel = Some(channel);
            memory.failed_at = None;
        });
    }
}

fn emit(app: &AppHandle) {
    let status = app.state::<UpdaterState>().status();
    if let Err(err) = events::UpdateStateChanged::emit(app, &status) {
        log::warn!("could not emit {}: {err}", events::UpdateStateChanged::NAME);
    }
}

fn channel(app: &AppHandle) -> UpdateChannel {
    app.state::<SettingsStore>().get().update_channel
}

/// A request that never got an answer means offline, as for GitHub's API.
fn classify(err: tauri_plugin_updater::Error) -> Error {
    match err {
        tauri_plugin_updater::Error::Reqwest(err) if err.is_connect() || err.is_timeout() => {
            Error::Offline
        }
        other => Error::Updater(other),
    }
}

/// Background checks: one shortly after every start, then whenever one is due. Release builds
/// only; dev builds have "Simulate update".
pub fn start(app: AppHandle) {
    if cfg!(debug_assertions) || !app.state::<UpdaterState>().bundle.can_update() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        if let Err(err) = check(&app, true).await {
            log::info!("update check at startup failed: {err}");
        }
        let mut last = (Utc::now(), Instant::now());
        loop {
            tokio::time::sleep(TICK).await;
            let now = (Utc::now(), Instant::now());
            let slept = woke(now.0 - last.0, now.1 - last.1);
            last = now;
            if slept {
                log::info!("woke from sleep; the update check waits a minute");
                continue;
            }
            check_if_due(&app).await;
        }
    });
}

async fn check_if_due(app: &AppHandle) {
    let version = app.package_info().version.to_string();
    let channel = channel(app);
    let is_due = app
        .state::<UpdaterState>()
        .memory
        .lock()
        .map(|memory| due(&memory, Utc::now(), &version, channel))
        .unwrap_or(false);
    if is_due && let Err(err) = check(app, true).await {
        log::info!("background update check failed: {err}");
    }
}

/// Checks the channel's `latest.json`; a newer version starts downloading at once. Returns
/// the state after the check. `quiet` (a background check) keeps a failure out of the UI.
pub async fn check(app: &AppHandle, quiet: bool) -> Result<UpdateStatus> {
    let state = app.state::<UpdaterState>();
    if !quiet && cfg!(debug_assertions) {
        return Err(Error::BlockedInDev("self-update".into()));
    }
    if !state.bundle.can_update() {
        return Err(Error::NoSelfUpdate);
    }
    let Some(generation) = state.with(Machine::begin_check) else {
        return Ok(state.status());
    };
    emit(app);

    let channel = channel(app);
    let version = app.package_info().version.to_string();
    let found = async {
        let url = endpoint(channel, state.bundle)
            .parse()
            .map_err(|_| Error::Unsupported("bad update URL".into()))?;
        let running = version.clone();
        app.updater_builder()
            .endpoints(vec![url])?
            // The plugin's own rule, plus a line saying what the feed held.
            .version_comparator(move |current, remote| {
                log::info!(
                    "update feed ({channel:?}) says {}, running {running}",
                    remote.version
                );
                remote.version > current
            })
            .build()?
            .check()
            .await
            .map_err(classify)
    }
    .await;

    match found {
        Ok(None) => {
            state.checked(version, channel);
            if state.with(|machine| machine.up_to_date(generation)) {
                emit(app);
            }
            Ok(state.status())
        }
        Ok(Some(update)) => {
            // Counted as done once the download succeeds, so a failed download is retried
            // after `RETRY`, not the next day.
            log::info!("update {} found; downloading", update.version);
            if state.with(|machine| machine.found(generation, update.version.clone())) {
                emit(app);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    download(app, generation, update, version, channel).await
                });
            }
            Ok(state.status())
        }
        Err(err) => {
            state.remember(|memory| memory.failed_at = Some(Utc::now()));
            if state.with(|machine| machine.failed(generation, Failure::from(&err), quiet)) {
                emit(app);
            }
            Err(err)
        }
    }
}

/// Downloads `update`; `version` and `channel` are the running app's, remembered as
/// checked once the bytes are in.
async fn download(
    app: AppHandle,
    generation: u64,
    update: Update,
    version: String,
    channel: UpdateChannel,
) {
    let state = app.state::<UpdaterState>();
    let result = update
        .download(
            |chunk, length| {
                let tell = state.with(|machine| {
                    machine.progress(generation, chunk as u64, length, Instant::now())
                });
                if tell {
                    emit(&app);
                }
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            state.checked(version, channel);
            let version = update.version.clone();
            let notes = update.body.clone();
            let pending = Pending {
                update: Some(update),
                bytes,
            };
            if state.with(|machine| machine.downloaded(generation, version.clone(), notes, pending))
            {
                log::info!("update {version} downloaded");
                emit(&app);
                announce_ready(&app, version);
            }
        }
        Err(err) => {
            let err = classify(err);
            log::warn!("update download failed: {err}");
            state.remember(|memory| memory.failed_at = Some(Utc::now()));
            if state.with(|machine| machine.failed(generation, Failure::from(&err), false)) {
                emit(&app);
            }
        }
    }
}

/// The popover is usually hidden: a system notification says the update is ready.
fn announce_ready(app: &AppHandle, version: String) {
    if let Err(err) = events::Notify::emit(app, &Notice::UpdateReady { version }) {
        log::warn!("could not emit {}: {err}", events::Notify::NAME);
    }
}

/// "Restart now": installs the downloaded update and relaunches. On Windows the plugin
/// starts the installer and exits; the installer relaunches the app. A deb or rpm asks for
/// the password first (`pkexec`).
pub async fn install_now(app: &AppHandle) -> Result<()> {
    let state = app.state::<UpdaterState>();
    let taken = state.with(|machine| {
        if machine.installing {
            return Err(Error::Busy);
        }
        let Some(pending) = machine.pending.take() else {
            return Ok(None);
        };
        machine.installing = true;
        Ok(Some(pending))
    })?;
    let Some(pending) = taken else {
        return Ok(());
    };
    emit(app);
    if pending.update.is_none() {
        log::info!("simulated update: would install and restart now");
        state.with(|machine| {
            machine.pending = Some(pending);
            machine.installing = false;
        });
        emit(app);
        return Ok(());
    }
    let installed = tauri::async_runtime::spawn_blocking(move || {
        let result = match &pending.update {
            Some(update) => update.install(&pending.bytes).map_err(classify),
            None => Ok(()),
        };
        (result, pending)
    })
    .await;
    match installed {
        Ok((Ok(()), _)) => {
            log::info!("update installed; restarting");
            app.restart();
        }
        Ok((Err(err), pending)) => {
            log::warn!("update install failed: {err}");
            state.with(|machine| {
                machine.pending = Some(pending);
                machine.installing = false;
            });
            emit(app);
            Err(err)
        }
        Err(err) => {
            state.with(|machine| machine.installing = false);
            emit(app);
            Err(Error::Unsupported(format!(
                "the update install stopped: {err}"
            )))
        }
    }
}

/// "Later": the banner stays, without the question.
pub fn defer(app: &AppHandle) {
    app.state::<UpdaterState>()
        .with(|machine| machine.deferred = true);
    emit(app);
}

/// Quit from the tray or Settings. A downloaded update installs first where that needs no
/// prompt (`Bundle::installs_on_quit`), so the next launch is the new version; the app
/// never relaunches itself here.
pub fn quit(app: &AppHandle) {
    let state = app.state::<UpdaterState>();
    // "Restart now" is installing: quitting now could leave an AppImage half written or
    // cut a deb or rpm install short. The restart that follows ends the app anyway.
    if state.with(|machine| machine.installing) {
        log::info!("quit ignored while the update installs");
        return;
    }
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    let pending = if state.bundle.installs_on_quit() {
        state.with(|machine| machine.pending.take())
    } else {
        None
    };
    let Some(pending) = pending else {
        app.exit(0);
        return;
    };
    let Some(update) = pending.update else {
        log::info!("simulated update: would install on quit");
        app.exit(0);
        return;
    };
    let app = app.clone();
    // Off the main thread: on macOS a bundle that needs admin rights asks through
    // AppleScript, which the plugin runs on the main thread and waits for.
    tauri::async_runtime::spawn_blocking(move || {
        log::info!("installing update {} before quitting", update.version);
        // Windows: the plugin starts the installer and exits the process itself.
        let update = update.restart_after_install(false);
        if let Err(err) = update.install(&pending.bytes) {
            log::warn!("update install on quit failed: {err}");
        }
        app.exit(0);
    });
}

/// The channel changed: what was found or downloaded for the old one is dropped, and the
/// new one is checked (quietly).
pub fn channel_changed(app: &AppHandle) {
    app.state::<UpdaterState>().with(Machine::reset);
    emit(app);
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = check(&app, true).await {
            log::info!("update check after a channel change failed: {err}");
        }
    });
}

/// Dev builds only: runs the whole UI path with a fake download of a few seconds, up to
/// the ready banner and its notification. Restart then only logs.
pub async fn simulate(app: &AppHandle) -> Result<()> {
    if !cfg!(debug_assertions) {
        return Err(Error::Unsupported(
            "the update simulation is for dev builds".into(),
        ));
    }
    const TOTAL: u64 = 12_400_000;
    const STEPS: u64 = 40;
    let state = app.state::<UpdaterState>();
    let current = &app.package_info().version;
    let version = format!("{}.{}.0", current.major, current.minor + 1);
    let generation = state.with(|machine| {
        machine.reset();
        machine.simulated = true;
        machine.begin_check()
    });
    let Some(generation) = generation else {
        return Ok(());
    };
    emit(app);
    tokio::time::sleep(Duration::from_millis(800)).await;
    if !state.with(|machine| machine.found(generation, version.clone())) {
        return Ok(());
    }
    emit(app);
    for _ in 0..STEPS {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let tell = state.with(|machine| {
            machine.progress(generation, TOTAL / STEPS, Some(TOTAL), Instant::now())
        });
        if tell {
            emit(app);
        }
    }
    let pending = Pending {
        update: None,
        bytes: Vec::new(),
    };
    let notes = Some("Simulated release notes.".to_owned());
    if state.with(|machine| machine.downloaded(generation, version.clone(), notes, pending)) {
        emit(app);
        announce_ready(app, version);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn checked(when: &str) -> Memory {
        Memory {
            checked_at: Some(at(when)),
            version: Some("0.10.0".into()),
            channel: Some(UpdateChannel::Stable),
            failed_at: None,
        }
    }

    #[test]
    fn walks_through_a_check_and_download() {
        let mut machine = Machine::<()>::default();
        let generation = machine.begin_check().unwrap();
        assert_eq!(machine.state, UpdateState::Checking);
        // A second check while one runs doesn't start.
        assert_eq!(machine.begin_check(), None);
        assert!(machine.found(generation, "0.11.0".into()));
        let start = Instant::now();
        assert!(machine.progress(generation, 10, Some(100), start));
        // Throttled: the next chunk 10 ms later isn't sent, the last one always is.
        assert!(!machine.progress(generation, 10, Some(100), start + Duration::from_millis(10)));
        assert!(machine.progress(
            generation,
            10,
            Some(100),
            start + Duration::from_millis(150)
        ));
        assert!(machine.progress(
            generation,
            70,
            Some(100),
            start + Duration::from_millis(160)
        ));
        assert_eq!(
            machine.state,
            UpdateState::Downloading {
                version: "0.11.0".into(),
                downloaded: 100,
                total: Some(100)
            }
        );
        machine.deferred = true;
        assert!(machine.downloaded(generation, "0.11.0".into(), None, ()));
        assert!(matches!(machine.state, UpdateState::Ready { .. }));
        assert!(machine.pending.is_some());
        assert!(!machine.deferred);
        // Nothing more is checked until it's installed.
        assert_eq!(machine.begin_check(), None);
    }

    #[test]
    fn up_to_date_goes_back_to_idle() {
        let mut machine = Machine::<()>::default();
        let generation = machine.begin_check().unwrap();
        assert!(machine.up_to_date(generation));
        assert_eq!(machine.state, UpdateState::Idle);
        assert!(machine.begin_check().is_some());
    }

    #[test]
    fn failures_are_quiet_in_the_background_only() {
        let failure = Failure::from(&Error::Offline);
        let mut machine = Machine::<()>::default();
        let generation = machine.begin_check().unwrap();
        assert!(machine.failed(generation, failure.clone(), true));
        assert_eq!(machine.state, UpdateState::Idle);
        let generation = machine.begin_check().unwrap();
        assert!(machine.failed(generation, failure.clone(), false));
        assert_eq!(
            machine.state,
            UpdateState::Error {
                error: failure.clone()
            }
        );
        assert_eq!(failure.kind.as_str(), "offline");
        // An error doesn't stop the next check.
        assert!(machine.begin_check().is_some());
    }

    #[test]
    fn a_reset_drops_what_the_old_check_brings() {
        let mut machine = Machine::<()>::default();
        let generation = machine.begin_check().unwrap();
        assert!(machine.found(generation, "0.11.0-beta.1".into()));
        machine.reset();
        assert_eq!(machine.state, UpdateState::Idle);
        assert!(!machine.progress(generation, 10, Some(100), Instant::now()));
        assert!(!machine.downloaded(generation, "0.11.0-beta.1".into(), None, ()));
        assert!(!machine.failed(generation, Failure::from(&Error::Offline), false));
        assert!(!machine.up_to_date(generation));
        assert_eq!(machine.state, UpdateState::Idle);
        assert!(machine.pending.is_none());
    }

    #[test]
    fn unknown_length_still_reports_progress() {
        let mut machine = Machine::<()>::default();
        let generation = machine.begin_check().unwrap();
        machine.found(generation, "0.11.0".into());
        assert!(machine.progress(generation, 5, None, Instant::now()));
        assert!(matches!(
            machine.state,
            UpdateState::Downloading {
                downloaded: 5,
                total: None,
                ..
            }
        ));
    }

    #[test]
    fn another_version_or_channel_checks_at_once() {
        let stable = UpdateChannel::Stable;
        let now = at("2026-10-16T12:00:00Z");
        assert!(due(&Memory::default(), now, "0.10.0", stable));
        let recent = checked("2026-10-16T11:30:00Z");
        assert!(!due(&recent, now, "0.10.0", stable));
        assert!(due(&recent, now, "0.11.0", stable));
        assert!(due(&recent, now, "0.10.0", UpdateChannel::Beta));
        // Offline right after an update: the next try waits like any failure.
        let failed = Memory {
            failed_at: Some(at("2026-10-16T11:45:00Z")),
            ..recent.clone()
        };
        assert!(!due(&failed, now, "0.11.0", stable));
        assert!(due(&failed, at("2026-10-16T12:45:00Z"), "0.11.0", stable));
    }

    #[test]
    fn checks_every_six_hours_and_catches_up_after_sleep() {
        let stable = UpdateChannel::Stable;
        let memory = checked("2026-10-15T12:00:00Z");
        assert!(!due(&memory, at("2026-10-15T17:59:00Z"), "0.10.0", stable));
        assert!(due(&memory, at("2026-10-15T18:00:00Z"), "0.10.0", stable));
        // A laptop that slept for three days checks on the first tick after waking.
        assert!(due(&memory, at("2026-10-18T08:00:00Z"), "0.10.0", stable));
        // A clock set back.
        assert!(due(&memory, at("2026-10-14T08:00:00Z"), "0.10.0", stable));
    }

    #[test]
    fn waits_after_a_failure() {
        let stable = UpdateChannel::Stable;
        let mut memory = checked("2026-10-14T12:00:00Z");
        memory.failed_at = Some(at("2026-10-16T11:30:00Z"));
        assert!(!due(&memory, at("2026-10-16T12:00:00Z"), "0.10.0", stable));
        assert!(due(&memory, at("2026-10-16T12:30:00Z"), "0.10.0", stable));
        // Never checked, failed just now: the tick waits (the next start checks anyway).
        let memory = Memory {
            failed_at: Some(at("2026-10-16T11:50:00Z")),
            ..Memory::default()
        };
        assert!(!due(&memory, at("2026-10-16T12:00:00Z"), "0.10.0", stable));
        assert!(due(&memory, at("2026-10-16T13:00:00Z"), "0.10.0", stable));
    }

    #[test]
    fn tells_sleep_from_a_slow_tick() {
        let minute = Duration::from_secs(60);
        assert!(!woke(chrono::TimeDelta::seconds(61), minute));
        assert!(woke(chrono::TimeDelta::hours(8), minute));
        assert!(!woke(chrono::TimeDelta::seconds(-30), minute));
    }

    #[test]
    fn picks_the_channel_file() {
        assert_eq!(endpoint(UpdateChannel::Stable, Bundle::Nsis), STABLE_URL);
        assert_eq!(endpoint(UpdateChannel::Beta, Bundle::Nsis), BETA_URL);
        assert_eq!(endpoint(UpdateChannel::Beta, Bundle::App), BETA_URL);
        assert_eq!(endpoint(UpdateChannel::Beta, Bundle::AppImage), BETA_URL);
        // No MSI pre-releases: an MSI install stays on stable.
        assert_eq!(endpoint(UpdateChannel::Beta, Bundle::Msi), STABLE_URL);
    }

    #[test]
    fn knows_what_each_install_can_do() {
        assert!(Bundle::App.installs_on_quit() && !Bundle::App.asks_password());
        assert!(Bundle::Nsis.installs_on_quit() && !Bundle::Msi.installs_on_quit());
        assert!(Bundle::AppImage.installs_on_quit());
        assert!(!Bundle::Deb.installs_on_quit() && Bundle::Deb.asks_password());
        assert!(!Bundle::Rpm.installs_on_quit() && Bundle::Rpm.asks_password());
        assert!(!Bundle::Unbundled.can_update() && !Bundle::Unbundled.installs_on_quit());
        assert!(!Bundle::Msi.has_beta() && Bundle::Nsis.has_beta());
    }

    #[test]
    fn memory_round_trips() {
        let dir = std::env::temp_dir().join(format!("c50tb-updater-{}", std::process::id()));
        let path = dir.join(MEMORY_FILE);
        let memory = checked("2026-10-16T11:30:00Z");
        memory.save(&path);
        assert_eq!(Memory::load(&path), memory);
        std::fs::write(&path, "{broken").unwrap();
        assert_eq!(Memory::load(&path), Memory::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn status_serializes_for_the_ui() {
        let state = UpdateState::Downloading {
            version: "0.11.0".into(),
            downloaded: 1,
            total: None,
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["phase"], "downloading");
        assert_eq!(json["total"], serde_json::Value::Null);
        assert_eq!(serde_json::to_value(Bundle::AppImage).unwrap(), "appImage");
    }
}
