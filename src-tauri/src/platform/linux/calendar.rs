//! Runs `linux_calendar`: detects Evolution Data Server and Akonadi, writes the `.ics` file
//! both read, and adds or removes the calendar that reads it. The logic (paths, file text,
//! D-Bus command lines, replies) is in `platform/linux_calendar.rs`, tested on every OS.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::platform::linux_calendar::{self as cal, Bus, Call, Step, Target};
use crate::platform::{CalendarEvent, NativeCalendar, Written, WrittenEvents, home_dir};
use crate::process;

/// One D-Bus call's limit: past the bus's own 25 s reply timeout, so a call that starts
/// Akonadi (its control service is D-Bus activated) fails with the bus's error, not ours.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for Akonadi to start, and for a new resource to appear on the bus.
const START_WAIT: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_millis(500);
/// The whole of one write (registering, unregistering, waiting for Akonadi), so
/// `calendar::write_native` never holds its lock for minutes behind a slow bus.
const WRITE_LIMIT: Duration = Duration::from_secs(60);
/// Removing a resource whose configuration failed, past `WRITE_LIMIT` if need be.
const CLEANUP_LIMIT: Duration = Duration::from_secs(10);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn sources_dir() -> Option<std::path::PathBuf> {
    cal::eds_sources_dir(env("XDG_CONFIG_HOME").as_deref(), home_dir().as_deref())
}

fn bus() -> Option<Bus> {
    cal::BUSES
        .into_iter()
        .find(|b| process::find_in_path(b.program()).is_some())
}

fn eds_installed() -> bool {
    let dirs = cal::data_dirs(
        env("XDG_DATA_HOME").as_deref(),
        env("XDG_DATA_DIRS").as_deref(),
        home_dir().as_deref(),
    );
    let service = dirs.iter().any(|dir| {
        std::fs::read_dir(dir.join("dbus-1/services")).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_str().is_some_and(cal::is_eds_service_file))
        })
    });
    // The registry made this folder the first time it ran.
    service || sources_dir().is_some_and(|d| d.is_dir())
}

fn akonadi_installed() -> bool {
    process::find_in_path(cal::AKONADI_AGENT).is_some()
        && process::find_in_path("akonadictl").is_some()
        && bus().is_some()
}

fn detected() -> Vec<Target> {
    cal::targets(
        eds_installed(),
        akonadi_installed(),
        env("XDG_CURRENT_DESKTOP").as_deref(),
    )
}

pub fn has_native_calendar() -> bool {
    !detected().is_empty()
}

/// The stores found; each holds one calendar, "Classroom 50", that the app adds.
pub fn native_calendars(_ask: bool) -> Result<Vec<NativeCalendar>> {
    Ok(detected()
        .into_iter()
        .map(|t| NativeCalendar {
            id: t.id().to_owned(),
            title: cal::DISPLAY_NAME.to_owned(),
            account: String::new(),
            kind: Some(t.id()),
        })
        .collect())
}

pub fn write_calendar(
    calendar_id: &str,
    events: &[CalendarEvent],
    written: &mut WrittenEvents,
    dir: &Path,
) -> Result<()> {
    let target = Target::from_id(calendar_id);
    if !calendar_id.is_empty() && target.is_none() {
        return Err(Error::Unsupported("that calendar no longer exists".into()));
    }
    let deadline = Instant::now() + WRITE_LIMIT;
    let ics = dir.join(cal::ICS_FILE);
    if target.is_some() {
        write_file(&ics, events, written)?;
    }
    for step in cal::plan(target, |t| written.contains_key(t.key())) {
        match step {
            Step::Register(t) => {
                let id = register(t, &ics, deadline)?;
                written.insert(
                    t.key().to_owned(),
                    Written {
                        event_id: id,
                        calendar_id: t.id().to_owned(),
                        fingerprint: 0,
                    },
                );
            }
            Step::Unregister(t) => {
                let id = written.get(t.key()).map(|w| w.event_id.clone());
                // Best effort: a store that broke or went away (Akonadi uninstalled or not
                // starting, no bus program) must not keep the sync from being turned off
                // or moved. What may be left behind is logged.
                if let Err(err) = unregister(t, id.as_deref(), deadline) {
                    log::warn!("could not remove the {} calendar: {err}", t.id());
                }
                written.remove(t.key());
            }
        }
    }
    if target.is_none() {
        if let Err(err) = std::fs::remove_file(&ics)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(err.into());
        }
        written.remove(cal::ICS_KEY);
    }
    Ok(())
}

/// Rewrites the file when an event changed (or it is gone); the calendars reload it.
fn write_file(ics: &Path, events: &[CalendarEvent], written: &mut WrittenEvents) -> Result<()> {
    let fingerprint = cal::ics_fingerprint(events.iter().map(|e| (e.uid.as_str(), e.fingerprint)));
    let same = written
        .get(cal::ICS_KEY)
        .is_some_and(|w| w.fingerprint == fingerprint);
    if same && ics.is_file() {
        return Ok(());
    }
    let text = crate::calendar::ics_for_events(events, chrono::Utc::now());
    crate::github::write_atomic(ics, text.as_bytes())?;
    written.insert(
        cal::ICS_KEY.to_owned(),
        Written {
            event_id: ics.display().to_string(),
            calendar_id: String::new(),
            fingerprint,
        },
    );
    Ok(())
}

/// Adds the calendar to `target`; returns what to remove later (the source file, the
/// Akonadi instance id).
fn register(target: Target, ics: &Path, deadline: Instant) -> Result<String> {
    match target {
        Target::Gnome => {
            let dir = sources_dir().ok_or(Error::CalendarUnavailable)?;
            let text = cal::eds_source(ics)
                .ok_or_else(|| Error::Unsupported("the calendar file's path isn't UTF-8".into()))?;
            let file = dir.join(format!("{}.source", cal::SOURCE_UID));
            // The registry ignores the temporary file (not `.source`) and loads the renamed one.
            crate::github::write_atomic(&file, text.as_bytes())?;
            Ok(file.display().to_string())
        }
        Target::Kde => {
            let bus = bus().ok_or(Error::CalendarUnavailable)?;
            let instance = env("AKONADI_INSTANCE");
            let instance = instance.as_deref();
            let id = create_resource(bus, instance, deadline)?;
            let configured = cal::configure_instance(&id, ics, instance)
                .ok_or_else(|| Error::Unsupported("the calendar file's path isn't UTF-8".into()))
                .and_then(|calls| {
                    // The new resource takes a moment to claim its bus name.
                    let (first, rest) = calls.split_first().ok_or(Error::CalendarUnavailable)?;
                    call_until_up(bus, first, false, deadline)?;
                    rest.iter()
                        .try_for_each(|c| call(bus, c, deadline).map(drop))
                });
            if let Err(err) = configured {
                // Don't leave an unconfigured resource behind.
                let cleanup = Instant::now() + CLEANUP_LIMIT;
                let _ = call(bus, &cal::remove_instance(&id, instance), cleanup);
                return Err(err);
            }
            Ok(id)
        }
    }
}

fn unregister(target: Target, id: Option<&str>, deadline: Instant) -> Result<()> {
    match target {
        Target::Gnome => {
            // Recomputed, never the stored path: only ever delete our own file.
            let Some(dir) = sources_dir() else {
                return Ok(());
            };
            match std::fs::remove_file(dir.join(format!("{}.source", cal::SOURCE_UID))) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
                _ => Ok(()),
            }
        }
        Target::Kde => {
            let Some(id) = id.filter(|id| cal::valid_instance_id(id)) else {
                return Ok(());
            };
            let bus = bus().ok_or(Error::CalendarUnavailable)?;
            let instance = env("AKONADI_INSTANCE");
            let instance = instance.as_deref();
            // Removed by the user already (KOrganizer's settings): nothing to do.
            let listed = call_until_up(bus, &cal::list_instances(instance), true, deadline)?;
            if !cal::parse_strings(bus, &listed).iter().any(|i| i == id) {
                return Ok(());
            }
            call(bus, &cal::remove_instance(id, instance), deadline).map(drop)
        }
    }
}

/// Creates the iCal resource, starting Akonadi first when it isn't running.
fn create_resource(bus: Bus, instance: Option<&str>, limit: Instant) -> Result<String> {
    let deadline = (Instant::now() + START_WAIT).min(limit);
    let create = cal::create_instance(instance);
    loop {
        let reply = call_until_up(bus, &create, true, limit)?;
        // Empty while Akonadi is still reading its agent types.
        if let Some(id) = cal::parse_strings(bus, &reply)
            .into_iter()
            .find(|id| cal::valid_instance_id(id))
        {
            return Ok(id);
        }
        if Instant::now() >= deadline {
            return Err(Error::CalendarUnavailable);
        }
        std::thread::sleep(RETRY);
    }
}

/// Makes `c`, waiting while its service isn't on the bus yet; with `start`, runs
/// `akonadictl start` once first. Never past `limit`.
fn call_until_up(bus: Bus, c: &Call, start: bool, limit: Instant) -> Result<String> {
    let deadline = (Instant::now() + START_WAIT).min(limit);
    let mut started = false;
    loop {
        match call(bus, c, limit) {
            Err(Error::CommandFailed { stderr, .. }) if cal::is_service_unknown(&stderr) => {
                if Instant::now() >= deadline {
                    return Err(Error::CalendarUnavailable);
                }
                if start
                    && !started
                    && let Some(timeout) = cal::time_left(Instant::now(), limit, CALL_TIMEOUT)
                {
                    started = true;
                    let _ = run("akonadictl", vec!["start".to_owned()], timeout);
                }
                std::thread::sleep(RETRY);
            }
            other => return other,
        }
    }
}

/// One call, cut short at `limit`; none at all past it.
fn call(bus: Bus, c: &Call, limit: Instant) -> Result<String> {
    let timeout =
        cal::time_left(Instant::now(), limit, CALL_TIMEOUT).ok_or(Error::CalendarUnavailable)?;
    run(bus.program(), cal::call_args(bus, c), timeout)
}

/// Runs on the blocking thread `calendar::write_native` uses.
fn run(program: &str, args: Vec<String>, timeout: Duration) -> Result<String> {
    tauri::async_runtime::block_on(process::run_with_timeout(program, args, None, timeout))
        .map(|out| out.stdout)
}
