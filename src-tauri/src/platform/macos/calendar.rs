//! EventKit: the calendars of macOS's Calendar app (iCloud, Google, Exchange and local
//! accounts alike), read and written in place. Access is asked once, by the user's own
//! action; background syncs never prompt.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_event_kit::{
    EKAlarm, EKAuthorizationStatus, EKCalendar, EKEntityType, EKEvent, EKEventAvailability,
    EKEventStore, EKSource, EKSourceType, EKSpan,
};
use objc2_foundation::{NSArray, NSDate, NSError, NSString, NSURL};

use crate::error::{Error, Result};
use crate::platform::{self, CalendarEvent, NativeCalendar, Written, WrittenEvents};

/// How long to wait for the user to answer the access prompt.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);
/// The calendar the last write used (`calendar_id`) and its account (`event_id`, the
/// source's identifier), so one the user deletes is made again in the same account.
const CALENDAR_KEY: &str = "@macos/calendar";
/// The title of a calendar made again after the chosen one was deleted.
const NEW_TITLE: &str = "Classroom 50";

fn status() -> EKAuthorizationStatus {
    unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) }
}

fn has_access(status: EKAuthorizationStatus) -> bool {
    // `Authorized` before macOS 14 has the value `FullAccess` has since.
    status == EKAuthorizationStatus::FullAccess
}

/// Shows the system prompt and waits for the answer.
fn request_access(store: &EKEventStore) -> bool {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        let _ = tx.send(granted.as_bool());
    });
    unsafe {
        if objc2::available!(macos = 14.0) {
            store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&block));
        } else {
            #[allow(deprecated)]
            store
                .requestAccessToEntityType_completion(EKEntityType::Event, RcBlock::as_ptr(&block));
        }
    }
    rx.recv_timeout(PROMPT_TIMEOUT).unwrap_or(false)
}

fn store(ask: bool) -> Result<Retained<EKEventStore>> {
    let store = unsafe { EKEventStore::new() };
    match status() {
        s if has_access(s) => Ok(store),
        EKAuthorizationStatus::NotDetermined if ask && request_access(&store) => {
            // Calendars appear in a store made after access was granted.
            Ok(unsafe { EKEventStore::new() })
        }
        _ => Err(Error::CalendarAccess),
    }
}

pub fn native_calendars(ask: bool) -> Result<Vec<NativeCalendar>> {
    let store = store(ask)?;
    let calendars = unsafe { store.calendarsForEntityType(EKEntityType::Event) };
    let mut list: Vec<NativeCalendar> = calendars
        .iter()
        .filter(|c| unsafe { c.allowsContentModifications() })
        .map(|c| NativeCalendar {
            id: unsafe { c.calendarIdentifier() }.to_string(),
            title: unsafe { c.title() }.to_string(),
            account: unsafe { c.source() }
                .map(|s| unsafe { s.title() }.to_string())
                .unwrap_or_default(),
            kind: None,
        })
        .collect();
    list.sort_by(|a, b| (&a.account, &a.title).cmp(&(&b.account, &b.title)));
    Ok(list)
}

fn date(at: chrono::DateTime<chrono::Utc>) -> Retained<NSDate> {
    NSDate::dateWithTimeIntervalSince1970(at.timestamp() as f64)
}

fn fill(event: &EKEvent, calendar: &EKCalendar, item: &CalendarEvent) {
    let (start, due) = (date(item.start), date(item.due));
    // EventKit offsets count from the start; the hours count back from the deadline (end).
    let block = (item.due - item.start).num_seconds() as f64;
    let alarms: Vec<Retained<EKAlarm>> = item
        .alarm_hours
        .iter()
        .map(|h| unsafe { EKAlarm::alarmWithRelativeOffset(block - f64::from(*h) * 3600.0) })
        .collect();
    unsafe {
        event.setCalendar(Some(calendar));
        event.setTitle(Some(&NSString::from_str(&item.title)));
        event.setNotes(item.notes.as_deref().map(NSString::from_str).as_deref());
        let url = item
            .url
            .as_deref()
            .and_then(|u| NSURL::URLWithString(&NSString::from_str(u)));
        event.setURL(url.as_deref());
        event.setStartDate(Some(&start));
        event.setEndDate(Some(&due));
        event.setAvailability(EKEventAvailability::Free);
        event.setAlarms(Some(&NSArray::from_retained_slice(&alarms)));
    }
}

fn find(store: &EKEventStore, written: Option<&Written>) -> Option<Retained<EKEvent>> {
    let id = NSString::from_str(&written?.event_id);
    unsafe { store.eventWithIdentifier(&id) }
}

fn failed(err: Retained<NSError>) -> Error {
    Error::Unsupported(format!("Calendar: {}", err.localizedDescription()))
}

/// The calendar to write to: `calendar_id`, else (deleted by the user) one made earlier
/// that the setting hasn't caught up with, else a new "Classroom 50" calendar in the
/// deleted one's account, the default calendar's or On My Mac, at most once an hour
/// (`platform::may_recreate`). Returns it with its new id when it isn't `calendar_id`.
fn calendar(
    store: &EKEventStore,
    calendar_id: &str,
    written: &mut WrittenEvents,
) -> Result<(Retained<EKCalendar>, Option<String>)> {
    let by_id = |id: &str| unsafe { store.calendarWithIdentifier(&NSString::from_str(id)) };
    if let Some(found) = by_id(calendar_id) {
        return Ok((found, None));
    }
    let last = written.get(CALENDAR_KEY).cloned();
    if let Some(last) = &last
        && last.calendar_id != calendar_id
        && let Some(found) = by_id(&last.calendar_id)
    {
        return Ok((found, Some(last.calendar_id.clone())));
    }
    let now = chrono::Utc::now().timestamp();
    if !platform::may_recreate(written, now) {
        return Err(Error::Unsupported("that calendar no longer exists".into()));
    }
    platform::recreating(written, now);
    let same = last
        .as_ref()
        .and_then(|l| unsafe { store.sourceWithIdentifier(&NSString::from_str(&l.event_id)) });
    let default =
        unsafe { store.defaultCalendarForNewEvents() }.and_then(|c| unsafe { c.source() });
    let local = unsafe { store.sources() }
        .iter()
        .find(|s| unsafe { s.sourceType() } == EKSourceType::Local);
    let mut tried: Vec<String> = Vec::new();
    let mut error = Error::CalendarUnavailable;
    for (source, which) in [
        (same, "its account"),
        (default, "the default account"),
        (local, "On My Mac"),
    ] {
        let Some(source) = source else { continue };
        let id = unsafe { source.sourceIdentifier() }.to_string();
        if tried.contains(&id) {
            continue;
        }
        tried.push(id);
        match make(store, &source) {
            Ok(made) => {
                platform::recreated(written);
                log::info!("the chosen calendar was deleted: made a new one in {which}");
                let id = unsafe { made.calendarIdentifier() }.to_string();
                return Ok((made, Some(id)));
            }
            Err(err) => error = err,
        }
    }
    log::warn!("the chosen calendar was deleted and a new one couldn't be made: {error}");
    Err(error)
}

/// A new event calendar named `NEW_TITLE` in `source`, saved.
fn make(store: &EKEventStore, source: &EKSource) -> Result<Retained<EKCalendar>> {
    unsafe {
        let calendar = EKCalendar::calendarForEntityType_eventStore(EKEntityType::Event, store);
        calendar.setTitle(&NSString::from_str(NEW_TITLE));
        calendar.setSource(Some(source));
        store
            .saveCalendar_commit_error(&calendar, true)
            .map_err(failed)?;
        Ok(calendar)
    }
}

pub fn write_calendar(
    calendar_id: &str,
    events: &[CalendarEvent],
    written: &mut WrittenEvents,
    _dir: &std::path::Path,
) -> Result<Option<String>> {
    let store = store(false)?;
    let (calendar, moved) = if events.is_empty() {
        (None, None)
    } else {
        let (calendar, moved) = calendar(&store, calendar_id, written)?;
        (Some(calendar), moved)
    };
    let calendar_id = moved.as_deref().unwrap_or(calendar_id);
    match &calendar {
        Some(calendar) => {
            let source = unsafe { calendar.source() }
                .map(|s| unsafe { s.sourceIdentifier() }.to_string())
                .unwrap_or_default();
            written.insert(
                CALENDAR_KEY.to_owned(),
                Written {
                    event_id: source,
                    calendar_id: calendar_id.to_owned(),
                    fingerprint: 0,
                },
            );
        }
        None if calendar_id.is_empty() => {
            written.remove(CALENDAR_KEY);
        }
        None => {}
    }

    // Gone from the list, or written to another calendar: remove.
    let stale: Vec<String> = written
        .iter()
        .filter(|(uid, w)| {
            !uid.starts_with('@')
                && (w.calendar_id != calendar_id || !events.iter().any(|e| &e.uid == *uid))
        })
        .map(|(uid, _)| uid.clone())
        .collect();
    for uid in stale {
        if let Some(event) = find(&store, written.get(&uid)) {
            unsafe { store.removeEvent_span_commit_error(&event, EKSpan::ThisEvent, true) }
                .map_err(failed)?;
        }
        written.remove(&uid);
    }

    if let Some(calendar) = calendar {
        for item in events {
            let known = written.get(&item.uid);
            let existing = find(&store, known);
            if existing.is_some() && known.is_some_and(|w| w.fingerprint == item.fingerprint) {
                continue;
            }
            // Deleted by the user in Calendar, or new: write it (again). Each save commits:
            // a new event only gets its identifier then.
            let event = existing.unwrap_or_else(|| unsafe { EKEvent::eventWithEventStore(&store) });
            fill(&event, &calendar, item);
            unsafe { store.saveEvent_span_commit_error(&event, EKSpan::ThisEvent, true) }
                .map_err(failed)?;
            if let Some(id) = unsafe { event.eventIdentifier() } {
                written.insert(
                    item.uid.clone(),
                    Written {
                        event_id: id.to_string(),
                        calendar_id: calendar_id.to_owned(),
                        fingerprint: item.fingerprint,
                    },
                );
            }
        }
    }
    Ok(moved)
}
