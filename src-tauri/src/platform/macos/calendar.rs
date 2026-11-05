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
use crate::platform::macos_calendar::{self as mac, Pick};
use crate::platform::{self, CalendarEvent, NativeCalendar, Written, WrittenEvents};

/// How long to wait for the user to answer the access prompt.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

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

fn by_id(store: &EKEventStore, id: &str) -> Option<Retained<EKCalendar>> {
    unsafe { store.calendarWithIdentifier(&NSString::from_str(id)) }
}

fn source_id(calendar: &EKCalendar) -> String {
    unsafe { calendar.source() }
        .map(|s| unsafe { s.sourceIdentifier() }.to_string())
        .unwrap_or_default()
}

/// The calendar to write to (`macos_calendar::pick`): `calendar_id`, else (deleted by the
/// user) the one the app made for it that the setting hasn't caught up with, else a
/// "Classroom 50" calendar in the deleted one's account, the default calendar's or On My
/// Mac: one already there by that title, else a new one, at most once an hour
/// (`platform::may_recreate`). Returns it with its id when it isn't `calendar_id`.
fn calendar(
    store: &EKEventStore,
    calendar_id: &str,
    written: &mut WrittenEvents,
) -> Result<(Retained<EKCalendar>, Option<String>)> {
    let sources = unsafe { store.sources() };
    let now = chrono::Utc::now().timestamp();
    let picked = mac::pick(
        written,
        calendar_id,
        |id| by_id(store, id).is_some(),
        sources.count() > 0,
        now,
    );
    match picked {
        Pick::Chosen => {
            return by_id(store, calendar_id)
                .map(|found| (found, None))
                .ok_or(Error::CalendarUnavailable);
        }
        Pick::Made(id) => {
            return by_id(store, &id)
                .map(|found| (found, Some(id)))
                .ok_or(Error::CalendarUnavailable);
        }
        Pick::NotReady => return Err(Error::CalendarUnavailable),
        Pick::Wait => return Err(Error::Unsupported("that calendar no longer exists".into())),
        Pick::Make => {}
    }
    platform::recreating(written, now);
    let same = mac::last_source(written)
        .and_then(|id| unsafe { store.sourceWithIdentifier(&NSString::from_str(id)) });
    let default =
        unsafe { store.defaultCalendarForNewEvents() }.and_then(|c| unsafe { c.source() });
    let local = sources
        .iter()
        .find(|s| unsafe { s.sourceType() } == EKSourceType::Local);
    let existing: Vec<mac::Existing> = unsafe { store.calendarsForEntityType(EKEntityType::Event) }
        .iter()
        .filter(|c| unsafe { c.allowsContentModifications() })
        .map(|c| mac::Existing {
            id: unsafe { c.calendarIdentifier() }.to_string(),
            source: source_id(&c),
            title: unsafe { c.title() }.to_string(),
        })
        .collect();
    let mut tried: Vec<String> = Vec::new();
    let mut error = Error::CalendarUnavailable;
    for (source, which) in [
        (same, "its account"),
        (default, "the default account"),
        (local, "On My Mac"),
    ] {
        let Some(source) = source else { continue };
        let source_id = unsafe { source.sourceIdentifier() }.to_string();
        if tried.contains(&source_id) {
            continue;
        }
        tried.push(source_id.clone());
        let reused = mac::reusable(&existing, &source_id).and_then(|id| by_id(store, id));
        let (calendar, how) = match reused {
            Some(found) => (found, "used the Classroom 50 calendar already"),
            None => match make(store, &source) {
                Ok(made) => (made, "made a new one"),
                Err(err) => {
                    error = err;
                    continue;
                }
            },
        };
        let id = unsafe { calendar.calendarIdentifier() }.to_string();
        mac::made(written, &id, &source_id);
        log::info!(
            "the chosen calendar was deleted: {how} in {which} (turning the sync off in Settings > Calendar stops this)"
        );
        return Ok((calendar, Some(id)));
    }
    log::warn!("the chosen calendar was deleted and a new one couldn't be made: {error}");
    Err(error)
}

/// A new event calendar named `NEW_TITLE` in `source`, saved.
fn make(store: &EKEventStore, source: &EKSource) -> Result<Retained<EKCalendar>> {
    unsafe {
        let calendar = EKCalendar::calendarForEntityType_eventStore(EKEntityType::Event, store);
        calendar.setTitle(&NSString::from_str(mac::NEW_TITLE));
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
    let (calendar, moved) = if calendar_id.is_empty() {
        mac::forget(written);
        (None, None)
    } else if events.is_empty() {
        // Nothing to write, but the calendar found still counts as the one chosen.
        (by_id(&store, calendar_id), None)
    } else {
        let (calendar, moved) = calendar(&store, calendar_id, written)?;
        (Some(calendar), moved)
    };
    let calendar_id = moved.as_deref().unwrap_or(calendar_id);
    if let Some(calendar) = &calendar {
        mac::wrote_to(written, calendar_id, &source_id(calendar));
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
