//! When a sync is due: the cadence set by the nearest deadline, the popover opening,
//! waking from sleep, the network coming back, GitHub's limits.

use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Manager};

use super::{Mode, ProjectsState, Snapshot, refresh_in_background, refresh_local};
use crate::github;
use crate::settings::{Settings, SettingsStore};

/// A full sync (repo list, teams) is due after this long while work is due this week.
const FULL_SYNC_AGE: chrono::TimeDelta = chrono::TimeDelta::minutes(15);
/// Opening the popover re-reads the status at most this often.
const MIN_STATUS_ON_OPEN: chrono::TimeDelta = chrono::TimeDelta::minutes(1);
/// How old the full sync may get when the popover opens on an assignment to accept.
const MIN_FULL_ON_OPEN: chrono::TimeDelta = chrono::TimeDelta::minutes(2);

/// Starts whatever is due in the background: a full sync when the last one is older than
/// `FULL_SYNC_AGE`, else a status check when the last GitHub read is older than
/// `min_status_age`. Nothing while GitHub asks to back off.
fn refresh_due(app: &AppHandle, min_status_age: chrono::TimeDelta, on_open: bool) {
    if !may_call_github(app) {
        return;
    }
    let state = app.state::<ProjectsState>();
    let snapshot = state.snapshot();
    let full_attempt = state.full_attempt.lock().ok().and_then(|at| *at);
    let now = Utc::now();
    let app = app.clone();
    let (_, mut full_age) = cadence(&snapshot, now);
    // Only a full sync finds a repo accepted elsewhere (the web page, the CLI), so while an
    // assignment waits for it, opening the popover looks again.
    if on_open && awaiting_accept(&snapshot, &app.state::<SettingsStore>().get(), now) {
        full_age = full_age.min(MIN_FULL_ON_OPEN);
    }
    let Some(mode) = due_mode(&snapshot, full_attempt, now, full_age, min_status_age) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        refresh_in_background(&app, mode).await;
    });
}

/// What is due: a full sync once the last one that worked is `full_age` old, retried after a
/// failure no more often than status checks run; else a status check once the last GitHub
/// read is `min_status_age` old.
fn due_mode(
    snapshot: &Snapshot,
    full_attempt: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    full_age: chrono::TimeDelta,
    min_status_age: chrono::TimeDelta,
) -> Option<Mode> {
    let older = |at: Option<DateTime<Utc>>, age| at.is_none_or(|at| now - at >= age);
    if older(snapshot.synced_at, full_age) && older(full_attempt, min_status_age) {
        Some(Mode::Full)
    } else if older(snapshot.checked_at, min_status_age) {
        Some(Mode::Status)
    } else {
        None
    }
}

/// Background requests wait while GitHub asks to back off, and keep a reserve of the
/// budget for the user's own tools: they pause below 10% until the window resets.
pub(super) fn may_call_github(app: &AppHandle) -> bool {
    let state = app.state::<ProjectsState>();
    let now = Utc::now();
    let backing_off = state
        .snapshot()
        .issue
        .and_then(|issue| issue.retry_after)
        .is_some_and(|until| now < until);
    let low_budget = github::rate_limit()
        .or(state.stats().rate_limit)
        .is_some_and(|r| {
            r.remaining.saturating_mul(10) < r.limit && r.reset_at.is_none_or(|at| now < at)
        });
    if low_budget {
        log::info!("GitHub budget below 10%; background syncs paused until it resets");
    }
    // Offline, only the reachability probe in the background loop runs.
    !backing_off && !low_budget && !state.is_offline()
}

/// When the popover opens.
pub fn refresh_if_stale(app: &AppHandle) {
    refresh_due(app, MIN_STATUS_ON_OPEN, true);
}

/// Whether the list offers an assignment the user could accept right now (not hidden,
/// locked or upcoming).
fn awaiting_accept(snapshot: &Snapshot, settings: &Settings, now: DateTime<Utc>) -> bool {
    snapshot.unaccepted.iter().any(|item| {
        let class = format!("{}/{}", item.classroom.org, item.classroom.slug).to_ascii_lowercase();
        !item.assignment.locked
            && item.assignment.available_from.is_none_or(|at| at <= now)
            && !settings.hidden.contains(&item.id)
            && !settings.hidden_classes.contains(&class)
    })
}

/// At startup, re-reads the disk on top of the saved snapshot (no request). Then a tick
/// every 30 s decides what is due, at a pace set by the nearest unfinished deadline, and
/// reacts to waking from sleep and to the network coming back.
pub fn start_background_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if app.state::<ProjectsState>().snapshot().checked_at.is_some() {
            refresh_local(&app, false).await;
        }
        let mut wall = std::time::SystemTime::now();
        let mut monotonic = std::time::Instant::now();
        loop {
            // The monotonic clock stops while the machine sleeps; the wall clock doesn't.
            let (wall_now, monotonic_now) =
                (std::time::SystemTime::now(), std::time::Instant::now());
            let slept = wall_now
                .duration_since(wall)
                .unwrap_or_default()
                .saturating_sub(monotonic_now - monotonic)
                > Duration::from_secs(60);
            (wall, monotonic) = (wall_now, monotonic_now);
            // Riding this tick, the tray menu's dated labels need no timer of their own.
            crate::tray::check_expiry(&app);

            let state = app.state::<ProjectsState>();
            if state.is_offline() {
                if github_reachable().await {
                    log::info!("network is back; syncing");
                    state.clear_offline();
                    refresh_due(&app, chrono::TimeDelta::zero(), false);
                }
            } else if slept {
                log::info!("woke from sleep; syncing");
                // Give Wi-Fi a moment to reconnect.
                tokio::time::sleep(Duration::from_secs(5)).await;
                refresh_due(&app, chrono::TimeDelta::zero(), false);
            } else {
                let (status_age, _) = cadence(&state.snapshot(), Utc::now());
                refresh_due(&app, status_age, false);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

const TICK: Duration = Duration::from_secs(30);

/// How old the last status check and the last full sync may get, from the nearest
/// deadline of unfinished work: every minute in its last 3 hours, every 2 within a day,
/// every 5 within a week, and 15 (full sync hourly) when nothing is due that soon.
fn cadence(snapshot: &Snapshot, now: DateTime<Utc>) -> (chrono::TimeDelta, chrono::TimeDelta) {
    use chrono::TimeDelta as T;
    let soonest = snapshot
        .projects
        .iter()
        .filter(|p| crate::alerts::needs_work(p))
        .filter_map(crate::alerts::open_due)
        .filter(|due| *due > now)
        .min();
    // A few seconds under each period, so a 30 s tick doesn't skip a turn.
    let slack = T::seconds(10);
    match soonest.map(|due| due - now) {
        Some(left) if left <= T::hours(3) => (T::minutes(1) - slack, FULL_SYNC_AGE),
        Some(left) if left <= T::hours(24) => (T::minutes(2) - slack, FULL_SYNC_AGE),
        Some(left) if left <= T::days(7) => (T::minutes(5) - slack, FULL_SYNC_AGE),
        _ => (T::minutes(15) - slack, T::hours(1)),
    }
}

/// A TCP handshake with the API host: no HTTP request, so nothing counts against any limit.
async fn github_reachable() -> bool {
    if !crate::simulate::reachable() {
        return false;
    }
    let (host, port) =
        crate::testenv::probe_addr().unwrap_or_else(|| ("api.github.com".to_owned(), 443));
    let connect = tokio::net::TcpStream::connect((host.as_str(), port));
    matches!(
        tokio::time::timeout(Duration::from_secs(3), connect).await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::classroom50::Classroom;
    use crate::projects::fixtures::config;
    use crate::projects::{Project, Unaccepted};

    #[test]
    fn opening_looks_for_repos_accepted_elsewhere() {
        let now = Utc::now();
        let poo = Classroom::from_team("HEIGVD-POO", "classroom50-26-poo-c", None).unwrap();
        let item = |slug: &str, extra: serde_json::Value| {
            let mut assignment = json!({"slug": slug, "name": slug, "mode": "team"});
            assignment
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            Unaccepted {
                id: format!("heigvd-poo/26-poo-c/{slug}"),
                classroom: poo.clone(),
                assignment: serde_json::from_value(assignment).unwrap(),
                accept_url: String::new(),
                group: None,
            }
        };
        let later = (now + chrono::TimeDelta::days(1)).to_rfc3339();
        let mut snapshot = Snapshot {
            unaccepted: vec![
                item("labo-4", json!({"locked": true})),
                item("labo-5", json!({"available_from": later})),
            ],
            ..Snapshot::default()
        };
        let mut settings = Settings::default();
        assert!(!awaiting_accept(&snapshot, &settings, now));
        snapshot.unaccepted.push(item("labo-6", json!({})));
        assert!(awaiting_accept(&snapshot, &settings, now));
        settings.hidden = vec!["heigvd-poo/26-poo-c/labo-6".into()];
        assert!(!awaiting_accept(&snapshot, &settings, now));
        settings.hidden.clear();
        settings.hidden_classes = vec!["heigvd-poo/26-poo-c".into()];
        assert!(!awaiting_accept(&snapshot, &settings, now));
    }

    #[test]
    fn retries_a_failed_full_sync_at_the_status_pace() {
        use chrono::TimeDelta as T;
        let now = Utc::now();
        let (full_age, status_age) = (T::hours(1), T::minutes(5));
        // The last full sync that worked is 50 min old, a failed one ran 10 min ago.
        let snapshot = Snapshot {
            synced_at: Some(now - T::minutes(50)),
            checked_at: Some(now - T::minutes(50)),
            ..Snapshot::default()
        };
        let attempt = Some(now - T::minutes(10));
        assert_eq!(
            due_mode(&snapshot, attempt, now, full_age, status_age),
            Some(Mode::Status)
        );
        // Due by age: retried, but not on every tick after a failure.
        let old = Snapshot {
            synced_at: Some(now - T::hours(2)),
            ..snapshot.clone()
        };
        assert_eq!(
            due_mode(&old, attempt, now, full_age, status_age),
            Some(Mode::Full)
        );
        let just_failed = Some(now - T::minutes(1));
        assert_eq!(
            due_mode(&old, just_failed, now, full_age, status_age),
            Some(Mode::Status)
        );
        // Network back or waking up asks with no minimum age: right away.
        assert_eq!(
            due_mode(&old, just_failed, now, full_age, T::zero()),
            Some(Mode::Full)
        );
        let fresh = Snapshot {
            synced_at: Some(now),
            checked_at: Some(now),
            ..Snapshot::default()
        };
        assert_eq!(due_mode(&fresh, Some(now), now, full_age, status_age), None);
    }

    #[test]
    fn checks_more_often_as_a_deadline_nears() {
        let now = Utc::now();
        let snapshot = |hours: i64| Snapshot {
            projects: vec![Project {
                id: "x".into(),
                owner: None,
                repo: None,
                config: config("hello"),
                path: None,
                local: None,
                remote: None,
                classroom: None,
                assignment: Some(
                    serde_json::from_value(json!({
                        "slug": "hello", "name": "Hello", "mode": "individual",
                        "due": (now + chrono::TimeDelta::hours(hours)).to_rfc3339()
                    }))
                    .unwrap(),
                ),
                pending: None,
                needs_work: false,
                web_url: None,
                late_seconds: None,
                editor: None,
                group: None,
            }],
            ..Snapshot::default()
        };
        let status_minutes = |hours| cadence(&snapshot(hours), now).0.num_minutes();
        assert_eq!(status_minutes(2), 0); // just under a minute
        assert_eq!(status_minutes(20), 1);
        assert_eq!(status_minutes(72), 4);
        assert_eq!(status_minutes(24 * 30), 14);
        assert_eq!(
            cadence(&snapshot(24 * 30), now).1,
            chrono::TimeDelta::hours(1)
        );
    }
}
