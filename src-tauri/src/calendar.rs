//! Due dates as an iCalendar file (RFC 5545) to import in Calendar, Outlook or Google
//! Calendar. One event per assignment at its due time, with an alarm at each reminder the
//! user picked. UIDs are the list's ids, so importing again updates events rather than
//! duplicating them.

use chrono::{DateTime, Utc};

use crate::classroom50::{Assignment, Classroom};
use crate::projects::Snapshot;

const PRODID: &str = "-//classroom50-toolbox//EN";

struct Event<'a> {
    uid: &'a str,
    assignment: &'a Assignment,
    classroom: Option<&'a Classroom>,
    class_slug: &'a str,
    url: Option<&'a str>,
}

/// The calendar of every assignment with a due date, accepted or not, minus those the user
/// hid (one by one or by class, keyed like `filter::classKey`).
pub fn ics(
    snapshot: &Snapshot,
    hidden: &[String],
    hidden_classes: &[String],
    reminder_hours: &[u32],
    now: DateTime<Utc>,
) -> (String, usize) {
    let class_hidden = |owner: &str, classroom: &str| {
        hidden_classes.contains(&format!("{owner}/{classroom}").to_lowercase())
    };
    let projects = snapshot
        .projects
        .iter()
        .filter(|p| {
            !hidden.contains(&p.id)
                && !class_hidden(p.owner.as_deref().unwrap_or(""), &p.config.classroom)
        })
        .filter_map(|p| {
            Some(Event {
                uid: &p.id,
                assignment: p.assignment.as_ref()?,
                classroom: p.classroom.as_ref(),
                class_slug: &p.config.classroom,
                url: p.web_url.as_deref(),
            })
        });
    let unaccepted = snapshot
        .unaccepted
        .iter()
        .filter(|u| !hidden.contains(&u.id) && !class_hidden(&u.classroom.org, &u.classroom.slug))
        .map(|u| Event {
            uid: &u.id,
            assignment: &u.assignment,
            classroom: Some(&u.classroom),
            class_slug: &u.classroom.slug,
            url: Some(&u.accept_url),
        });

    let mut out = String::new();
    line(&mut out, "BEGIN:VCALENDAR");
    line(&mut out, "VERSION:2.0");
    line(&mut out, &format!("PRODID:{PRODID}"));
    line(&mut out, "CALSCALE:GREGORIAN");
    line(&mut out, "METHOD:PUBLISH");
    let stamp = utc(now);
    let mut count = 0;
    for event in projects.chain(unaccepted) {
        let Some(due) = event.assignment.due else {
            continue;
        };
        let due = utc(due.with_timezone(&Utc));
        let class = event
            .classroom
            .and_then(|c| c.name.as_deref())
            .unwrap_or(event.class_slug);
        let summary = format!("{} · {class}", event.assignment.name);
        line(&mut out, "BEGIN:VEVENT");
        line(
            &mut out,
            &format!("UID:{}@classroom50-toolbox", escape(event.uid)),
        );
        line(&mut out, &format!("DTSTAMP:{stamp}"));
        line(&mut out, &format!("DTSTART:{due}"));
        line(&mut out, &format!("DTEND:{due}"));
        line(&mut out, &format!("SUMMARY:{}", escape(&summary)));
        if let Some(text) = event.assignment.description.as_deref() {
            line(&mut out, &format!("DESCRIPTION:{}", escape(text)));
        }
        if let Some(url) = event.url {
            line(&mut out, &format!("URL:{url}"));
        }
        // Free time: a deadline isn't a meeting.
        line(&mut out, "TRANSP:TRANSPARENT");
        for hours in reminder_hours {
            line(&mut out, "BEGIN:VALARM");
            line(&mut out, "ACTION:DISPLAY");
            line(&mut out, &format!("DESCRIPTION:{}", escape(&summary)));
            line(&mut out, &format!("TRIGGER:-PT{hours}H"));
            line(&mut out, "END:VALARM");
        }
        line(&mut out, "END:VEVENT");
        count += 1;
    }
    line(&mut out, "END:VCALENDAR");
    (out, count)
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
    use crate::projects::Unaccepted;

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
        let (ics, count) = ics(&snapshot, &[], &[], &[24, 1], Utc::now());
        assert_eq!(count, 1);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(ics.contains("DTSTART:20261102T225900Z\r\n"));
        assert!(ics.contains("SUMMARY:Lab 1\\; part A · POO\\, C\r\n"));
        assert!(ics.contains("UID:org/poo/lab1@classroom50-toolbox\r\n"));
        assert_eq!(ics.matches("BEGIN:VALARM").count(), 2);
        assert!(ics.contains("TRIGGER:-PT24H\r\n"));
        assert!(!ics.contains("No date"));
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
        assert_eq!(ics(&snapshot, &hidden, &[], &[], Utc::now()).1, 0);
        let classes = ["org/poo".to_owned()];
        assert_eq!(ics(&snapshot, &[], &classes, &[], Utc::now()).1, 0);
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
