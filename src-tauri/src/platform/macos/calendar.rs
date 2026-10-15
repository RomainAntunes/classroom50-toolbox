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
    EKEventStore, EKSpan,
};
use objc2_foundation::{NSArray, NSDate, NSError, NSString, NSURL};

use crate::error::{Error, Result};
use crate::platform::{CalendarEvent, NativeCalendar, Written, WrittenEvents};

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
        })
        .collect();
    list.sort_by(|a, b| (&a.account, &a.title).cmp(&(&b.account, &b.title)));
    Ok(list)
}

fn date(at: chrono::DateTime<chrono::Utc>) -> Retained<NSDate> {
    NSDate::dateWithTimeIntervalSince1970(at.timestamp() as f64)
}

fn fill(event: &EKEvent, calendar: &EKCalendar, item: &CalendarEvent) {
    let due = date(item.due);
    let alarms: Vec<Retained<EKAlarm>> = item
        .alarm_hours
        .iter()
        .map(|h| unsafe { EKAlarm::alarmWithRelativeOffset(-(f64::from(*h) * 3600.0)) })
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
        event.setStartDate(Some(&due));
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

pub fn write_calendar(
    calendar_id: &str,
    events: &[CalendarEvent],
    written: &mut WrittenEvents,
) -> Result<()> {
    let store = store(false)?;
    let calendar = if events.is_empty() {
        None
    } else {
        let found = unsafe { store.calendarWithIdentifier(&NSString::from_str(calendar_id)) };
        Some(found.ok_or_else(|| Error::Unsupported("that calendar no longer exists".into()))?)
    };

    // Gone from the list, or written to another calendar: remove.
    let stale: Vec<String> = written
        .iter()
        .filter(|(uid, w)| w.calendar_id != calendar_id || !events.iter().any(|e| &e.uid == *uid))
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
    Ok(())
}
