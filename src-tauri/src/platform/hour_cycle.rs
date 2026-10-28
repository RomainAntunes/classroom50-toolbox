//! Whether the OS shows times on a 12- or 24-hour clock, from what each OS stores. Pure
//! parsers, compiled and tested on every OS; each platform file only reads.
// Each OS uses one parser; the others are only exercised by the tests.
#![cfg_attr(not(test), allow(dead_code))]

use serde::Serialize;

/// The clock times are shown on: `Intl.DateTimeFormat`'s `hourCycle` ("10:30 PM" or "22:30").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum HourCycle {
    H12,
    H23,
}

/// The first letter outside single quotes that `decide` knows, in a date pattern where
/// `'...'` is literal text (`''`, a quote, toggles twice and changes nothing).
fn first_unquoted(pattern: &str, decide: impl Fn(char) -> Option<HourCycle>) -> Option<HourCycle> {
    let mut quoted = false;
    for c in pattern.chars() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && let Some(cycle) = decide(c) {
            return Some(cycle);
        }
    }
    None
}

/// An ICU pattern, as macOS's `dateFormatFromTemplate("j")` gives it (`h:mm a`, `HH:mm`):
/// `h` and `K` count hours on a 12-hour clock, `H` and `k` on a 24-hour one; with no hour,
/// a day period (`a`, `b`, `B`) means 12-hour.
pub fn from_icu_pattern(pattern: &str) -> Option<HourCycle> {
    first_unquoted(pattern, |c| match c {
        'h' | 'K' => Some(HourCycle::H12),
        'H' | 'k' => Some(HourCycle::H23),
        _ => None,
    })
    .or_else(|| {
        first_unquoted(pattern, |c| {
            matches!(c, 'a' | 'b' | 'B').then_some(HourCycle::H12)
        })
    })
}

/// Windows' short time format (`sShortTime`: `HH:mm`, `h:mm tt`): `h` is 12-hour, `H` 24.
pub fn from_windows_pattern(pattern: &str) -> Option<HourCycle> {
    first_unquoted(pattern, |c| match c {
        'h' => Some(HourCycle::H12),
        'H' => Some(HourCycle::H23),
        _ => None,
    })
}

/// `gsettings get org.gnome.desktop.interface clock-format`, which prints `'24h'` or `'12h'`.
pub fn from_gnome_clock_format(output: &str) -> Option<HourCycle> {
    match output.trim().trim_matches('\'') {
        "24h" => Some(HourCycle::H23),
        "12h" => Some(HourCycle::H12),
        _ => None,
    }
}

/// Whether GNOME's clock setting is the desktop's own (`XDG_CURRENT_DESKTOP`, a
/// colon-separated list such as `ubuntu:GNOME`). Elsewhere the key may exist with its
/// default, `24h`, which says nothing about the user's clock.
pub fn is_gnome(current_desktop: Option<&str>) -> bool {
    current_desktop.is_some_and(|desktops| {
        desktops
            .split(':')
            .any(|desktop| desktop.trim().eq_ignore_ascii_case("gnome"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_icu_patterns() {
        assert_eq!(from_icu_pattern("h:mm a"), Some(HourCycle::H12));
        assert_eq!(from_icu_pattern("a h:mm"), Some(HourCycle::H12));
        assert_eq!(from_icu_pattern("K:mm a"), Some(HourCycle::H12));
        assert_eq!(from_icu_pattern("h B"), Some(HourCycle::H12));
        assert_eq!(from_icu_pattern("HH:mm"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("H:mm"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("kk:mm"), Some(HourCycle::H23));
        // macOS's "j" template alone.
        assert_eq!(from_icu_pattern("HH"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("h a"), Some(HourCycle::H12));
    }

    #[test]
    fn skips_quoted_icu_text() {
        assert_eq!(from_icu_pattern("H 'h'"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("'h' HH:mm"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("HH 'Uhr'"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("'o''clock' h a"), Some(HourCycle::H12));
        // The day period counts only without an hour, and never quoted.
        assert_eq!(from_icu_pattern("'a' HH"), Some(HourCycle::H23));
        assert_eq!(from_icu_pattern("a"), Some(HourCycle::H12));
        assert_eq!(from_icu_pattern("'a'"), None);
        assert_eq!(from_icu_pattern(""), None);
        assert_eq!(from_icu_pattern("mm"), None);
    }

    #[test]
    fn reads_windows_patterns() {
        assert_eq!(from_windows_pattern("HH:mm"), Some(HourCycle::H23));
        assert_eq!(from_windows_pattern("H:mm"), Some(HourCycle::H23));
        assert_eq!(from_windows_pattern("h:mm tt"), Some(HourCycle::H12));
        assert_eq!(from_windows_pattern("tt h:mm"), Some(HourCycle::H12));
        assert_eq!(from_windows_pattern("hh:mm"), Some(HourCycle::H12));
        assert_eq!(from_windows_pattern("'h' HH:mm"), Some(HourCycle::H23));
        assert_eq!(from_windows_pattern("HH 'h' mm"), Some(HourCycle::H23));
        assert_eq!(from_windows_pattern("tt"), None);
        assert_eq!(from_windows_pattern(""), None);
    }

    #[test]
    fn reads_gnome_clock_formats() {
        assert_eq!(from_gnome_clock_format("'24h'\n"), Some(HourCycle::H23));
        assert_eq!(from_gnome_clock_format("'12h'\n"), Some(HourCycle::H12));
        assert_eq!(from_gnome_clock_format("'12h'"), Some(HourCycle::H12));
        assert_eq!(from_gnome_clock_format(" 24h "), Some(HourCycle::H23));
        assert_eq!(from_gnome_clock_format(""), None);
        assert_eq!(from_gnome_clock_format("'auto'\n"), None);
    }

    #[test]
    fn knows_gnome_desktops() {
        assert!(is_gnome(Some("GNOME")));
        assert!(is_gnome(Some("ubuntu:GNOME")));
        assert!(is_gnome(Some("GNOME-Classic:GNOME")));
        assert!(is_gnome(Some("pop:GNOME")));
        assert!(!is_gnome(Some("KDE")));
        assert!(!is_gnome(Some("X-Cinnamon")));
        assert!(!is_gnome(Some("")));
        assert!(!is_gnome(None));
    }

    /// The command's answer reaches the UI as `Intl`'s `hourCycle` values.
    #[test]
    fn serializes_as_intl_hour_cycles() {
        assert_eq!(serde_json::to_string(&HourCycle::H12).unwrap(), "\"h12\"");
        assert_eq!(serde_json::to_string(&HourCycle::H23).unwrap(), "\"h23\"");
    }
}
