//! The macOS calendar's decisions, pure so they are tested on every OS: which calendar a
//! write goes to when the chosen one is gone, and the markers kept for it in `WrittenEvents`.
//! The EventKit glue (`macos/calendar.rs`) asks the store and carries them out.

use super::{Written, WrittenEvents, may_recreate, recreated};

/// The chosen calendar last found (`calendar_id`) and its account (`event_id`, the source's
/// identifier), updated at every write that names a calendar, events or not, so one the user
/// deletes is made again in the same account.
pub const CALENDAR_KEY: &str = "@macos/calendar";
/// The calendar the app made (or took over by its title) for a chosen one the user deleted
/// (`calendar_id`, `event_id` its source), kept until a write finds the chosen calendar and
/// it is another: until then the setting may not have caught up with it.
pub const MADE_KEY: &str = "@macos/made";
/// The title of a calendar made again after the chosen one was deleted.
pub const NEW_TITLE: &str = "Classroom 50";

/// Where a write with events goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    /// The chosen calendar.
    Chosen,
    /// The calendar the app made for a deleted chosen one, which the setting hasn't caught
    /// up with (the update was lost): never a calendar the user picked and left.
    Made(String),
    /// The store has no accounts yet (opened right after login, iCloud not loaded): nothing
    /// is made and no attempt counts, so a later write tries again.
    NotReady,
    /// A "Classroom 50" calendar is needed (one reused by its title, else a new one).
    Make,
    /// Making one failed within the hour (`may_recreate`).
    Wait,
}

/// Picks the calendar for `chosen`, `exists` telling whether the store holds a calendar id
/// and `sources_ready` whether it lists any account.
pub fn pick(
    written: &WrittenEvents,
    chosen: &str,
    exists: impl Fn(&str) -> bool,
    sources_ready: bool,
    now: i64,
) -> Pick {
    if exists(chosen) {
        return Pick::Chosen;
    }
    if !sources_ready {
        return Pick::NotReady;
    }
    if let Some(made) = written.get(MADE_KEY)
        && exists(&made.calendar_id)
    {
        return Pick::Made(made.calendar_id.clone());
    }
    if may_recreate(written, now) {
        Pick::Make
    } else {
        Pick::Wait
    }
}

/// Records a write to `calendar_id` (in the account `source`). A calendar other than the
/// one the app made means the user chose it: the made one is no fallback anymore.
pub fn wrote_to(written: &mut WrittenEvents, calendar_id: &str, source: &str) {
    if written
        .get(MADE_KEY)
        .is_some_and(|m| m.calendar_id != calendar_id)
    {
        written.remove(MADE_KEY);
    }
    written.insert(CALENDAR_KEY.to_owned(), marker(calendar_id, source));
}

/// Records the calendar made (or reused by its title) for a deleted chosen one.
pub fn made(written: &mut WrittenEvents, calendar_id: &str, source: &str) {
    recreated(written);
    written.insert(MADE_KEY.to_owned(), marker(calendar_id, source));
}

/// The sync was turned off: nothing to follow anymore.
pub fn forget(written: &mut WrittenEvents) {
    written.remove(CALENDAR_KEY);
    written.remove(MADE_KEY);
}

/// The account the chosen calendar was last found in, to make the new one there.
pub fn last_source(written: &WrittenEvents) -> Option<&str> {
    written
        .get(CALENDAR_KEY)
        .map(|w| w.event_id.as_str())
        .filter(|s| !s.is_empty())
}

/// A writable calendar of the store: its id, its source's id and its title.
pub struct Existing {
    pub id: String,
    pub source: String,
    pub title: String,
}

/// A "Classroom 50" calendar already in `source` (made earlier: a full sync of the account
/// can change a calendar's identifier), reused rather than making a second one.
pub fn reusable<'a>(calendars: &'a [Existing], source: &str) -> Option<&'a str> {
    calendars
        .iter()
        .find(|c| c.source == source && c.title == NEW_TITLE)
        .map(|c| c.id.as_str())
}

fn marker(calendar_id: &str, source: &str) -> Written {
    Written {
        event_id: source.to_owned(),
        calendar_id: calendar_id.to_owned(),
        fingerprint: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::recreating;

    fn store<'a>(ids: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |id| ids.contains(&id)
    }

    #[test]
    fn never_falls_back_to_a_calendar_the_user_left() {
        let mut written = WrittenEvents::new();
        // Due dates in "Work" (A), then the user picks "School" (B) while no due date shows.
        assert_eq!(
            pick(&written, "A", store(&["A", "B"]), true, 0),
            Pick::Chosen
        );
        wrote_to(&mut written, "A", "icloud");
        wrote_to(&mut written, "B", "icloud");
        // B deleted in Calendar, due dates appear: a new calendar, never A.
        assert_eq!(pick(&written, "B", store(&["A"]), true, 0), Pick::Make);
        assert_eq!(last_source(&written), Some("icloud"));
    }

    #[test]
    fn follows_the_calendar_it_made_until_the_user_picks_another() {
        let mut written = WrittenEvents::new();
        wrote_to(&mut written, "A", "icloud");
        // A deleted: M made, but the setting still names A (its update was lost).
        assert_eq!(pick(&written, "A", store(&[]), true, 0), Pick::Make);
        recreating(&mut written, 0);
        made(&mut written, "M", "icloud");
        wrote_to(&mut written, "M", "icloud");
        assert!(may_recreate(&written, 1));
        assert_eq!(
            pick(&written, "A", store(&["M"]), true, 10),
            Pick::Made("M".into())
        );
        // The setting follows: still the made one.
        assert_eq!(pick(&written, "M", store(&["M"]), true, 10), Pick::Chosen);
        wrote_to(&mut written, "M", "icloud");
        assert!(written.contains_key(MADE_KEY));
        // The user picks B, then deletes it: M is no fallback anymore.
        wrote_to(&mut written, "B", "google");
        assert!(!written.contains_key(MADE_KEY));
        assert_eq!(pick(&written, "B", store(&["M"]), true, 10), Pick::Make);
        assert_eq!(last_source(&written), Some("google"));
    }

    #[test]
    fn waits_for_the_accounts_and_after_a_failure() {
        let mut written = WrittenEvents::new();
        wrote_to(&mut written, "A", "icloud");
        // At login the store may list no account yet: the chosen calendar looks gone.
        assert_eq!(pick(&written, "A", store(&[]), false, 0), Pick::NotReady);
        assert!(may_recreate(&written, 0), "not ready counts as no attempt");
        recreating(&mut written, 100);
        assert_eq!(pick(&written, "A", store(&[]), true, 200), Pick::Wait);
        assert_eq!(pick(&written, "A", store(&[]), true, 3700), Pick::Make);
    }

    #[test]
    fn reuses_a_classroom_calendar_in_the_same_account() {
        let calendars = [
            Existing {
                id: "W".into(),
                source: "icloud".into(),
                title: "Work".into(),
            },
            Existing {
                id: "G".into(),
                source: "google".into(),
                title: NEW_TITLE.into(),
            },
            Existing {
                id: "C".into(),
                source: "icloud".into(),
                title: NEW_TITLE.into(),
            },
        ];
        assert_eq!(reusable(&calendars, "icloud"), Some("C"));
        assert_eq!(reusable(&calendars, "google"), Some("G"));
        assert_eq!(reusable(&calendars, "local"), None);
    }

    #[test]
    fn turning_off_forgets_both_markers() {
        let mut written = WrittenEvents::new();
        made(&mut written, "M", "icloud");
        wrote_to(&mut written, "M", "icloud");
        forget(&mut written);
        assert!(written.is_empty());
    }
}
