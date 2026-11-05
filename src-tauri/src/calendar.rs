//! Due dates as an iCalendar file (RFC 5545) to import in Calendar, Outlook or Google
//! Calendar. One event per assignment at its due time, with an alarm at each reminder the
//! user picked. UIDs are the list's ids, so importing again updates events rather than
//! duplicating them; a renamed assignment keeps the UID of its id before the rename.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Manager};

use serde::Deserialize;

use crate::classroom50::{Assignment, GradingMode};
use crate::error::Result;
use crate::events::{IpcEvent, SettingsUpdated};
use crate::groups::Group;
use crate::phases::open_phases;
use crate::platform::{self, CalendarEvent, WrittenEvents};
use crate::projects::{self, ProjectsState, Snapshot};
use crate::settings::{Settings, SettingsStore};

const PRODID: &str = "-//classroom50-toolbox//EN";
/// Each deadline is a block of this length ending at the due time: a zero-length event
/// at 23:59 is a hairline at the bottom of the day.
const BLOCK: chrono::TimeDelta = chrono::TimeDelta::hours(1);
/// Calendar events always alert this many hours before the deadline, besides the
/// reminders picked for notifications.
const CALENDAR_ALERT_HOURS: u32 = 1;
/// What the app wrote into the OS calendar, by list id (app config dir: clearing the cache
/// must not orphan events).
const WRITTEN_FILE: &str = "calendar-events.json";

/// The words of the event notes, from the UI's i18n (`set_calendar_labels`), so Rust writes
/// no English of its own. `{name}`, `{n}`, `{score}` and `{max}` are filled in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(specta::Type), serde(rename = "CalendarLabels"))]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    pub class: String,
    pub group: String,
    pub group_number: String,
    pub graded: String,
    /// For an assignment the teacher doesn't grade, in place of a grade.
    pub not_graded: String,
    pub submitted: String,
    pub not_accepted: String,
    /// A phase's event title, `{name}` the assignment's and `{phase}` the phase's.
    pub phase_title: String,
    /// A phase ticked done.
    pub phase_done: String,
}

impl Default for Labels {
    /// Until the UI has sent its own (it loads at startup, hidden).
    fn default() -> Self {
        Self {
            class: "Class: {name}".into(),
            group: "Group: {name}".into(),
            group_number: "Group {n}".into(),
            graded: "Grade: {score}/{max}".into(),
            not_graded: "Not graded".into(),
            submitted: "Submitted".into(),
            not_accepted: "Not accepted yet".into(),
            phase_title: "{name}: {phase}".into(),
            phase_done: "Done".into(),
        }
    }
}

/// Where the assignment stands, for the notes.
enum Status<'a> {
    NotAccepted,
    Graded(&'a crate::status::Grade),
    Submitted,
    /// A phase ticked done.
    Done,
    Open,
}

/// The class, where the work stands (no grade for one the teacher doesn't grade, which
/// says so), the group with each member's email, then the assignment's own description.
/// A phase's event passes no assignment: its notes stop at the group.
fn notes(
    labels: &Labels,
    class: &str,
    status: Status,
    group: Option<&Group>,
    assignment: Option<&Assignment>,
) -> String {
    let mut lines = vec![labels.class.replace("{name}", class)];
    if assignment.is_some_and(|a| a.grading_mode() == GradingMode::Off) {
        lines.push(labels.not_graded.clone());
    }
    match status {
        Status::NotAccepted => lines.push(labels.not_accepted.clone()),
        Status::Graded(grade) => lines.push(
            labels
                .graded
                .replace("{score}", &grade.score.to_string())
                .replace("{max}", &grade.max_score.to_string()),
        ),
        Status::Submitted => lines.push(labels.submitted.clone()),
        Status::Done => lines.push(labels.phase_done.clone()),
        Status::Open => {}
    }
    if let Some(group) = group {
        let name = group.name.clone().unwrap_or_else(|| {
            labels
                .group_number
                .replace("{n}", &group.number.to_string())
        });
        lines.push(String::new());
        lines.push(labels.group.replace("{name}", &name));
        for member in &group.members {
            let mut line = match &member.name {
                Some(name) => format!("- {name} (@{})", member.login),
                None => format!("- @{}", member.login),
            };
            if let Some(email) = &member.email {
                line.push_str(&format!(" <{email}>"));
            }
            lines.push(line);
        }
    }
    if let Some(text) = assignment
        .and_then(|a| a.description.as_deref())
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        lines.push(String::new());
        lines.push(text.to_owned());
    }
    lines.join("\n")
}

/// Every assignment with a due date, accepted or not, and every phase the student set on a
/// project (`phases.rs`), minus those the user hid (one by one or by class, keyed like
/// `filter::classKey`).
pub fn events(
    snapshot: &Snapshot,
    hidden: &[String],
    hidden_classes: &[String],
    reminder_hours: &[u32],
    labels: &Labels,
) -> Vec<CalendarEvent> {
    let class_hidden = |owner: &str, classroom: &str| {
        hidden_classes.contains(&format!("{owner}/{classroom}").to_lowercase())
    };
    let visible = || {
        snapshot.projects.iter().filter(|p| {
            !hidden.contains(&p.id)
                && !class_hidden(p.owner.as_deref().unwrap_or(""), &p.config.classroom)
        })
    };
    let mut alarm_hours: Vec<u32> = reminder_hours.to_vec();
    alarm_hours.push(CALENDAR_ALERT_HOURS);
    alarm_hours.sort_unstable_by(|a, b| b.cmp(a));
    alarm_hours.dedup();
    let projects = visible().filter_map(|p| {
        let remote = p.remote.as_ref();
        // A grade the autograder left on an ungraded assignment isn't one.
        let graded = p
            .assignment
            .as_ref()
            .is_none_or(|a| a.grading_mode() != GradingMode::Off);
        let status = match (
            remote.and_then(|r| r.grade.as_ref()).filter(|_| graded),
            remote.and_then(|r| r.last_submission.as_ref()),
        ) {
            (Some(grade), _) => Status::Graded(grade),
            (None, Some(_)) => Status::Submitted,
            (None, None) => Status::Open,
        };
        Some((
            projects::previous_id(p).unwrap_or_else(|| p.id.clone()),
            p.assignment.as_ref()?,
            p.classroom.as_ref(),
            &p.config.classroom,
            p.web_url.as_deref(),
            status,
            p.group.as_ref(),
        ))
    });
    let unaccepted = snapshot
        .unaccepted
        .iter()
        .filter(|u| !hidden.contains(&u.id) && !class_hidden(&u.classroom.org, &u.classroom.slug))
        .map(|u| {
            (
                projects::previous_unaccepted_id(u).unwrap_or_else(|| u.id.clone()),
                &u.assignment,
                Some(&u.classroom),
                &u.classroom.slug,
                Some(u.accept_url.as_str()),
                Status::NotAccepted,
                u.group.as_ref(),
            )
        });
    let phases = visible().flat_map(|p| {
        let uid = projects::previous_id(p).unwrap_or_else(|| p.id.clone());
        let name = p
            .assignment
            .as_ref()
            .map_or(&p.config.assignment, |a| &a.name);
        let class = p
            .classroom
            .as_ref()
            .and_then(|c| c.name.as_deref())
            .unwrap_or(&p.config.classroom);
        let open: Vec<&str> = open_phases(p).map(|phase| phase.id.as_str()).collect();
        let alarm_hours = &alarm_hours;
        p.phases.iter().map(move |phase| {
            let status = if phase.done {
                Status::Done
            } else {
                Status::Open
            };
            let mut event = CalendarEvent {
                uid: format!("{uid}/phase/{}", phase.id),
                title: labels
                    .phase_title
                    .replace("{name}", name)
                    .replace("{phase}", &phase.name),
                notes: Some(notes(labels, class, status, p.group.as_ref(), None)),
                url: p.web_url.clone(),
                start: phase.due - BLOCK,
                due: phase.due,
                // Handed in, or nothing more can be: the event stays, silent.
                alarm_hours: if open.contains(&phase.id.as_str()) {
                    alarm_hours.clone()
                } else {
                    Vec::new()
                },
                fingerprint: 0,
            };
            event.fingerprint = fingerprint(&event);
            event
        })
    });
    projects
        .chain(unaccepted)
        .filter_map(|(uid, assignment, classroom, slug, url, status, group)| {
            let due = assignment.due?.with_timezone(&Utc);
            let class = classroom.and_then(|c| c.name.as_deref()).unwrap_or(slug);
            let alarm_hours = alarm_hours.clone();
            let mut event = CalendarEvent {
                uid,
                // Short enough for a week view; the class is in the notes.
                title: assignment.name.clone(),
                notes: Some(notes(labels, class, status, group, Some(assignment))),
                url: url.map(str::to_owned),
                start: due - BLOCK,
                due,
                alarm_hours,
                fingerprint: 0,
            };
            event.fingerprint = fingerprint(&event);
            Some(event)
        })
        .chain(phases)
        .collect()
}

fn fingerprint(event: &CalendarEvent) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        &event.title,
        &event.notes,
        &event.url,
        event.start,
        event.due,
        &event.alarm_hours,
    )
        .hash(&mut hasher);
    hasher.finish()
}

pub fn ics(
    snapshot: &Snapshot,
    hidden: &[String],
    hidden_classes: &[String],
    reminder_hours: &[u32],
    labels: &Labels,
    now: DateTime<Utc>,
) -> (String, usize) {
    let events = events(snapshot, hidden, hidden_classes, reminder_hours, labels);
    (ics_for_events(&events, now), events.len())
}

/// The iCalendar text of `events`, stamped `now`.
pub fn ics_for_events(events: &[CalendarEvent], now: DateTime<Utc>) -> String {
    let mut out = String::new();
    line(&mut out, "BEGIN:VCALENDAR");
    line(&mut out, "VERSION:2.0");
    line(&mut out, &format!("PRODID:{PRODID}"));
    line(&mut out, "CALSCALE:GREGORIAN");
    line(&mut out, "METHOD:PUBLISH");
    let stamp = utc(now);
    for event in events {
        let (start, due) = (utc(event.start), utc(event.due));
        line(&mut out, "BEGIN:VEVENT");
        line(
            &mut out,
            &format!("UID:{}@classroom50-toolbox", escape(&event.uid)),
        );
        line(&mut out, &format!("DTSTAMP:{stamp}"));
        line(&mut out, &format!("DTSTART:{start}"));
        line(&mut out, &format!("DTEND:{due}"));
        line(&mut out, &format!("SUMMARY:{}", escape(&event.title)));
        if let Some(text) = event.notes.as_deref() {
            line(&mut out, &format!("DESCRIPTION:{}", escape(text)));
        }
        if let Some(url) = event.url.as_deref() {
            line(&mut out, &format!("URL:{url}"));
        }
        // Free time: a deadline isn't a meeting.
        line(&mut out, "TRANSP:TRANSPARENT");
        for hours in &event.alarm_hours {
            line(&mut out, "BEGIN:VALARM");
            line(&mut out, "ACTION:DISPLAY");
            line(&mut out, &format!("DESCRIPTION:{}", escape(&event.title)));
            line(&mut out, &format!("TRIGGER;RELATED=END:-PT{hours}H"));
            line(&mut out, "END:VALARM");
        }
        line(&mut out, "END:VEVENT");
    }
    line(&mut out, "END:VCALENDAR");
    out
}

/// Writes the calendar of `snapshot` to `path`; returns the number of events.
pub fn export(
    path: &Path,
    snapshot: &Snapshot,
    settings: &Settings,
    labels: &Labels,
) -> Result<usize> {
    let (ics, count) = for_settings(snapshot, settings, labels);
    crate::github::write_atomic(path, ics.as_bytes())?;
    Ok(count)
}

fn for_settings(snapshot: &Snapshot, settings: &Settings, labels: &Labels) -> (String, usize) {
    ics(
        snapshot,
        &settings.hidden,
        &settings.hidden_classes,
        &settings.notify_hours_before,
        labels,
        Utc::now(),
    )
}

/// The events written into the OS calendar, persisted so later syncs update them in place.
pub struct CalendarState {
    path: PathBuf,
    /// The app data dir, for a file the OS calendar reads (Linux).
    data_dir: PathBuf,
    written: Mutex<WrittenEvents>,
    labels: Mutex<Labels>,
    /// A background write is running (`sync`); another sync then only asks it to go again
    /// (`again`), so writes behind a slow calendar (Akonadi starting) never pile up.
    busy: AtomicBool,
    again: AtomicBool,
    /// The last background write's error, so one that repeats (access turned off) is
    /// logged once rather than after every sync.
    last_error: Mutex<Option<String>>,
}

impl CalendarState {
    pub fn load(config_dir: &Path, data_dir: &Path) -> Self {
        let path = config_dir.join(WRITTEN_FILE);
        let written = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            data_dir: data_dir.to_owned(),
            written: Mutex::new(written),
            labels: Mutex::default(),
            busy: AtomicBool::new(false),
            again: AtomicBool::new(false),
            last_error: Mutex::default(),
        }
    }

    /// Logs a background write's failure when it differs from the last one, and the
    /// recovery once. Returns whether it logged.
    fn note_write(&self, error: Option<String>) -> bool {
        let Ok(mut last) = self.last_error.lock() else {
            return false;
        };
        let logged = match (&error, last.as_ref()) {
            (Some(err), Some(prev)) if err == prev => false,
            (Some(err), _) => {
                log::warn!("could not update the calendar: {err}");
                true
            }
            (None, Some(_)) => {
                log::info!("the calendar is updated again");
                true
            }
            (None, None) => false,
        };
        *last = error;
        logged
    }

    /// Returns true when the words changed.
    pub fn set_labels(&self, labels: Labels) -> bool {
        let Ok(mut current) = self.labels.lock() else {
            return false;
        };
        let changed = *current != labels;
        *current = labels;
        changed
    }

    pub fn labels(&self) -> Labels {
        self.labels.lock().map(|l| l.clone()).unwrap_or_default()
    }

    fn save(&self, written: &WrittenEvents) {
        match serde_json::to_vec(written) {
            Ok(json) => {
                if let Err(err) = crate::github::write_atomic(&self.path, &json) {
                    log::warn!("could not save {}: {err}", self.path.display());
                }
            }
            Err(err) => log::warn!("could not encode calendar events: {err}"),
        }
    }
}

/// Writes the due dates into `calendar_id` of the OS calendar now, or with `None` removes
/// every event the app wrote. Runs on a blocking thread: EventKit calls are synchronous.
/// Returns the calendar's new id when the chosen one was deleted and made again (macOS).
pub async fn write_native(app: &AppHandle, calendar_id: Option<String>) -> Result<Option<String>> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<CalendarState>();
        let Ok(mut written) = state.written.lock() else {
            return Ok(None);
        };
        let result = match calendar_id {
            Some(id) => {
                let settings = app.state::<SettingsStore>().get();
                let snapshot = app.state::<ProjectsState>().snapshot();
                let events = events(
                    &snapshot,
                    &settings.hidden,
                    &settings.hidden_classes,
                    &settings.notify_hours_before,
                    &state.labels(),
                );
                platform::write_calendar(&id, &events, &mut written, &state.data_dir)
            }
            None => platform::clear_calendar(&mut written, &state.data_dir).map(|()| None),
        };
        state.save(&written);
        result
    })
    .await
    .unwrap_or(Ok(None))
}

/// Points the `nativeCalendar` setting at the calendar made again in place of `old`
/// (deleted by the user), unless the user picked another one meanwhile, and tells the UI.
fn follow_calendar(app: &AppHandle, old: &str, new: String) {
    let store = app.state::<SettingsStore>();
    let moved = store.update(|settings| {
        (settings.native_calendar.as_deref() == Some(old)).then(|| Settings {
            native_calendar: Some(new),
            ..settings.clone()
        })
    });
    match moved {
        Ok(Some(saved)) => {
            let reported = crate::commands::reported_settings(app, saved);
            let _ = SettingsUpdated::emit(app, &reported);
        }
        Ok(None) => {}
        Err(err) => log::warn!("could not save the new calendar: {err}"),
    }
}

/// After a sync or a settings change: rewrites the exported file when its events changed,
/// so a calendar subscribed to it, or the next import, gets the new dates, and updates the
/// OS calendar the user picked.
pub fn sync(app: &AppHandle) {
    let settings = app.state::<SettingsStore>().get();
    let snapshot = app.state::<ProjectsState>().snapshot();
    if settings.native_calendar.is_some() {
        let state = app.state::<CalendarState>();
        state.again.store(true, Ordering::SeqCst);
        if !state.busy.swap(true, Ordering::SeqCst) {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<CalendarState>();
                loop {
                    while state.again.swap(false, Ordering::SeqCst) {
                        // Read again each round: the setting may have changed meanwhile.
                        let Some(id) = app.state::<SettingsStore>().get().native_calendar else {
                            break;
                        };
                        let error = match write_native(&app, Some(id.clone())).await {
                            Ok(Some(new)) => {
                                follow_calendar(&app, &id, new);
                                None
                            }
                            Ok(None) => None,
                            Err(err) => Some(err.to_string()),
                        };
                        let _ = state.note_write(error);
                    }
                    state.busy.store(false, Ordering::SeqCst);
                    // A sync that came in between the last check and now.
                    if !state.again.load(Ordering::SeqCst)
                        || state.busy.swap(true, Ordering::SeqCst)
                    {
                        break;
                    }
                }
            });
        }
    }
    if let Some(path) = &settings.calendar_file {
        let labels = app.state::<CalendarState>().labels();
        let (ics, _) = for_settings(&snapshot, &settings, &labels);
        let current = std::fs::read_to_string(path).unwrap_or_default();
        if without_stamps(&current) != without_stamps(&ics)
            && path.parent().is_some_and(Path::is_dir)
            && let Err(err) = crate::github::write_atomic(path, ics.as_bytes())
        {
            log::warn!("could not update {}: {err}", path.display());
        }
    }
}

/// The calendar minus its `DTSTAMP` lines, which change on every write.
fn without_stamps(ics: &str) -> String {
    ics.lines()
        .filter(|line| !line.starts_with("DTSTAMP:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn utc(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

/// TEXT values escape backslashes, semicolons, commas and newlines.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Appends a content line, folded at 75 octets (never inside a UTF-8 character), CRLF.
fn line(out: &mut String, content: &str) {
    let mut width = 0;
    for c in content.chars() {
        let len = c.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classroom50::{Assignment, Classroom};
    use crate::projects::Unaccepted;

    #[test]
    fn a_repeated_write_failure_is_logged_once() {
        let dir = std::env::temp_dir().join("c50-calendar-note-write");
        let state = CalendarState::load(&dir, &dir);
        let denied = || Some("calendar access isn't allowed".to_owned());
        assert!(state.note_write(denied()));
        assert!(!state.note_write(denied()));
        assert!(state.note_write(Some("that calendar no longer exists".into())));
        assert!(state.note_write(None), "the recovery is logged");
        assert!(!state.note_write(None));
        assert!(
            state.note_write(denied()),
            "a new failure after a success is logged"
        );
    }

    fn assignment(name: &str, due: Option<&str>) -> Assignment {
        serde_json::from_value(serde_json::json!({
            "slug": "lab1",
            "name": name,
            "mode": "individual",
            "due": due,
        }))
        .unwrap()
    }

    fn unaccepted(id: &str, name: &str, due: Option<&str>) -> Unaccepted {
        Unaccepted {
            id: id.into(),
            classroom: Classroom {
                org: "Org".into(),
                slug: "poo".into(),
                name: Some("POO, C".into()),
                term: None,
                active: true,
                secret: None,
                pages_base_url: None,
            },
            assignment: assignment(name, due),
            accept_url: "https://classroom50.org/org/poo/assignments/lab1/accept".into(),
            group: None,
        }
    }

    #[test]
    fn writes_one_event_per_due_assignment() {
        let snapshot = Snapshot {
            unaccepted: vec![
                unaccepted(
                    "org/poo/lab1",
                    "Lab 1; part A",
                    Some("2026-11-02T23:59:00+01:00"),
                ),
                unaccepted("org/poo/lab2", "No date", None),
            ],
            ..Snapshot::default()
        };
        let (ics, count) = ics(
            &snapshot,
            &[],
            &[],
            &[24, 1],
            &Labels::default(),
            Utc::now(),
        );
        assert_eq!(count, 1);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(ics.contains("DTSTART:20261102T215900Z\r\n"));
        assert!(ics.contains("DTEND:20261102T225900Z\r\n"));
        assert!(ics.contains("SUMMARY:Lab 1\\; part A\r\n"));
        assert!(ics.contains("DESCRIPTION:Class: POO\\, C\\nNot accepted yet\r\n"));
        assert!(ics.contains("UID:org/poo/lab1@classroom50-toolbox\r\n"));
        assert_eq!(ics.matches("BEGIN:VALARM").count(), 2);
        assert!(ics.contains("TRIGGER;RELATED=END:-PT24H\r\n"));
        // The 1h alert is always there, without doubling one already picked.
        let (only_week, _) =
            super::ics(&snapshot, &[], &[], &[168], &Labels::default(), Utc::now());
        assert!(only_week.contains("TRIGGER;RELATED=END:-PT1H\r\n"));
        assert!(!ics.contains("No date"));
    }

    #[test]
    fn says_an_ungraded_assignment_is_not_graded() {
        let off = serde_json::json!({"mode": "off"});
        let mut lab1 = unaccepted("org/poo/lab1", "Lab 1", Some("2026-11-02T23:59:00+01:00"));
        lab1.assignment.grading = serde_json::from_value(off.clone()).unwrap();
        // Accepted, with a release the autograder left anyway: no grade in the notes.
        let mut assignment = assignment("Lab 2", Some("2026-11-09T23:59:00+01:00"));
        assignment.grading = serde_json::from_value(off).unwrap();
        let mut remote: crate::projects::RemoteRepo = serde_json::from_value(serde_json::json!({
            "owner": "org", "name": "poo-lab2-alice", "url": "u",
            "archived": false, "canPush": true, "defaultBranch": "main",
            "head": null, "lastSubmission": null, "config": null
        }))
        .unwrap();
        remote.grade = Some(crate::status::Grade {
            tag: "submit/a".into(),
            score: 8,
            max_score: 10,
            tests_passed: 4,
            tests_total: 5,
            tests: Vec::new(),
            published_at: None,
            url: "u".into(),
            release_notes: None,
        });
        let lab2 = crate::projects::Project {
            id: "org/poo-lab2-alice".into(),
            owner: Some("org".into()),
            repo: Some("poo-lab2-alice".into()),
            config: crate::classroom50::RepoConfig {
                classroom: "poo".into(),
                assignment: "lab2".into(),
                secret: None,
            },
            path: None,
            local: None,
            remote: Some(remote),
            classroom: Some(lab1.classroom.clone()),
            assignment: Some(assignment),
            pending: None,
            needs_work: false,
            web_url: None,
            late_seconds: None,
            phases: Vec::new(),
            editor: None,
            last_editor: None,
            group: None,
        };
        let snapshot = Snapshot {
            projects: vec![lab2],
            unaccepted: vec![lab1],
            ..Snapshot::default()
        };
        let notes: Vec<String> = events(&snapshot, &[], &[], &[], &Labels::default())
            .into_iter()
            .filter_map(|e| e.notes)
            .collect();
        assert_eq!(
            notes,
            [
                "Class: POO, C\nNot graded",
                "Class: POO, C\nNot graded\nNot accepted yet"
            ]
        );
    }

    #[test]
    fn leaves_out_hidden_assignments_and_classes() {
        let snapshot = Snapshot {
            unaccepted: vec![unaccepted(
                "org/poo/lab1",
                "Lab 1",
                Some("2026-11-02T23:59:00Z"),
            )],
            ..Snapshot::default()
        };
        let hidden = ["org/poo/lab1".to_owned()];
        assert_eq!(
            ics(&snapshot, &hidden, &[], &[], &Labels::default(), Utc::now()).1,
            0
        );
        let classes = ["org/poo".to_owned()];
        assert_eq!(
            ics(
                &snapshot,
                &[],
                &classes,
                &[],
                &Labels::default(),
                Utc::now()
            )
            .1,
            0
        );
    }

    #[test]
    fn a_renamed_assignment_keeps_its_uid() {
        let mut item = unaccepted("org/poo/lab-one", "Lab 1", Some("2026-11-02T23:59:00Z"));
        item.assignment.slug = "lab-one".into();
        item.assignment.renamed_from = Some("lab1".into());
        let snapshot = Snapshot {
            unaccepted: vec![item],
            ..Snapshot::default()
        };
        let events = events(&snapshot, &[], &[], &[], &Labels::default());
        assert_eq!(events[0].uid, "org/poo/lab1");
    }

    #[test]
    fn writes_one_event_per_phase() {
        let project: crate::projects::Project = serde_json::from_value(serde_json::json!({
            "id": "org/poo-bdr-alex",
            "owner": "org",
            "repo": "poo-bdr-alex",
            "config": { "classroom": "poo", "assignment": "bdr" },
            "assignment": assignment("BDR", None),
            "phases": [
                { "id": "p1", "name": "Phase 1", "due": "2026-11-20T22:59:00Z", "done": true },
                { "id": "p2", "name": "Phase 2", "due": "2026-12-04T22:59:00Z" },
            ],
        }))
        .unwrap();
        let snapshot = Snapshot {
            projects: vec![project],
            ..Snapshot::default()
        };
        let events = events(&snapshot, &[], &[], &[24], &Labels::default());
        let titles: Vec<_> = events.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["BDR: Phase 1", "BDR: Phase 2"]);
        assert_eq!(events[0].uid, "org/poo-bdr-alex/phase/p1");
        assert_eq!(events[0].notes.as_deref(), Some("Class: poo\nDone"));
        // A phase ticked done stays on the calendar, without alarms.
        assert!(events[0].alarm_hours.is_empty());
        assert_eq!(events[1].alarm_hours, [24, 1]);
        assert_eq!(events[1].notes.as_deref(), Some("Class: poo"));

        let hidden = ["org/poo-bdr-alex".to_owned()];
        assert!(super::events(&snapshot, &hidden, &[], &[], &Labels::default()).is_empty());
    }

    #[test]
    fn ignores_stamps_when_comparing() {
        let snapshot = Snapshot {
            unaccepted: vec![unaccepted("a", "Lab 1", Some("2026-11-02T23:59:00Z"))],
            ..Snapshot::default()
        };
        let earlier = ics(
            &snapshot,
            &[],
            &[],
            &[],
            &Labels::default(),
            Utc::now() - chrono::TimeDelta::hours(1),
        )
        .0;
        let later = ics(&snapshot, &[], &[], &[], &Labels::default(), Utc::now()).0;
        assert_ne!(earlier, later);
        assert_eq!(without_stamps(&earlier), without_stamps(&later));
    }

    #[test]
    fn folds_long_lines_on_character_boundaries() {
        let mut out = String::new();
        line(&mut out, &format!("SUMMARY:{}", "é".repeat(60)));
        for part in out.trim_end().split("\r\n") {
            assert!(part.len() <= 75);
        }
        assert_eq!(
            out.replace("\r\n ", "").trim_end(),
            format!("SUMMARY:{}", "é".repeat(60))
        );
    }
}
