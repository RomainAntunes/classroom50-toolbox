//! The due dates in Linux calendar apps, as a calendar that reads the app's own `.ics` file:
//! an Evolution Data Server source (GNOME Calendar, Evolution) and an Akonadi iCal resource
//! (KOrganizer, Merkuro). Both watch the file, so later syncs only rewrite it. Everything
//! here is pure (paths, file text, D-Bus command lines, parsing) and compiled and tested on
//! every OS; `linux.rs` only runs it.
//!
//! Upstream facts this relies on, read in their sources:
//! - EDS: the registry (`e-source-registry-server.c`) monitors `$XDG_CONFIG_HOME/evolution/
//!   sources` and loads a `<uid>.source` key file created there (hidden files and other
//!   names are ignored), and drops the source when the file is deleted. `[Local Backend]`
//!   (`e-source-local.c`) holds `CustomFile`, a `GFile` stored as a URI (`e-source.c` reads
//!   it with `g_file_new_for_uri`), and `Writable`; the file backend
//!   (`e-cal-backend-file.c`) reports a custom file with `Writable=false` as read-only and
//!   reloads it 2 s after a file monitor sees it change.
//! - Akonadi: `org.freedesktop.Akonadi.Control` `/AgentManager` creates and removes agent
//!   instances (`akonadi_ical_resource_<n>`); each registers
//!   `org.freedesktop.Akonadi.Resource.<id>` (plus `.<AKONADI_INSTANCE>` when set) with its
//!   settings at `/Settings` (`org.kde.Akonadi.ICal.Settings`, generated from
//!   `icalresource.kcfg`: `setPath`, `setDisplayName`, `setReadOnly`, `setMonitorFile`,
//!   `save`) and `reconfigure` at `/` (`org.freedesktop.Akonadi.Agent.Control`), which
//!   reloads the saved settings and the file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The EDS source's UID, and so its file name (`<uid>.source`).
pub const SOURCE_UID: &str = "classroom50-toolbox";
/// The calendar's name in the calendar app: the product's, the same in every language.
pub const DISPLAY_NAME: &str = "Classroom 50";
/// The EDS source's color (GNOME's orange, apart from the default blue).
pub const COLOR: &str = "#e66100";
/// The file both calendars read, in the app data dir.
pub const ICS_FILE: &str = "due-dates.ics";
/// The Akonadi agent type that reads an `.ics` file.
pub const AKONADI_AGENT: &str = "akonadi_ical_resource";
/// `WrittenEvents` key of the `.ics` file's fingerprint; list ids never start with `@`.
pub const ICS_KEY: &str = "@linux/ics";

/// A Linux calendar store the app can add its calendar to. Serialized as its `id`, which the
/// UI words as `settings.calendar.store.<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum CalendarStore {
    /// Evolution Data Server: GNOME Calendar, Evolution.
    Gnome,
    /// Akonadi: KOrganizer, Merkuro.
    Kde,
}

impl CalendarStore {
    pub const ALL: [CalendarStore; 2] = [CalendarStore::Gnome, CalendarStore::Kde];

    /// The id in the `nativeCalendar` setting, and the `kind` the UI words.
    pub fn id(self) -> &'static str {
        match self {
            CalendarStore::Gnome => "gnome",
            CalendarStore::Kde => "kde",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    /// `WrittenEvents` key of what was registered in this store.
    pub fn key(self) -> &'static str {
        match self {
            CalendarStore::Gnome => "@linux/gnome",
            CalendarStore::Kde => "@linux/kde",
        }
    }
}

/// The stores found, the one matching the desktop first (both can be installed).
pub fn targets(eds: bool, akonadi: bool, desktop: Option<&str>) -> Vec<CalendarStore> {
    let mut list: Vec<CalendarStore> = CalendarStore::ALL
        .into_iter()
        .filter(|t| match t {
            CalendarStore::Gnome => eds,
            CalendarStore::Kde => akonadi,
        })
        .collect();
    let kde = desktop.is_some_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("KDE")));
    if kde {
        list.reverse();
    }
    list
}

/// What to do to make the stores hold exactly `target`'s calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Register(CalendarStore),
    Unregister(CalendarStore),
}

/// `registered` says which stores hold the calendar now (`WrittenEvents` keys).
pub fn plan(
    target: Option<CalendarStore>,
    registered: impl Fn(CalendarStore) -> bool,
) -> Vec<Step> {
    let mut steps = Vec::new();
    // Remove first: switching stores never leaves two calendars behind on a failure.
    for t in CalendarStore::ALL {
        if registered(t) && target != Some(t) {
            steps.push(Step::Unregister(t));
        }
    }
    if let Some(t) = target
        && !registered(t)
    {
        steps.push(Step::Register(t));
    }
    steps
}

/// XDG paths are POSIX paths. `Path::is_absolute` says no to `/usr/share` on Windows, where these
/// pure functions are unit-tested too, so the check is spelled out.
fn absolute(dir: &str) -> bool {
    dir.starts_with('/')
}

/// `$XDG_CONFIG_HOME/evolution/sources`, or `~/.config/...` when unset or not absolute.
pub fn eds_sources_dir(config_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    let base = match config_home.filter(|d| absolute(d)) {
        Some(dir) => PathBuf::from(dir),
        None => home?.join(".config"),
    };
    Some(base.join("evolution").join("sources"))
}

/// The XDG data dirs, most important first: `$XDG_DATA_HOME` (else `~/.local/share`), then
/// `$XDG_DATA_DIRS` (else `/usr/local/share:/usr/share`).
pub fn data_dirs(
    data_home: Option<&str>,
    data_dirs: Option<&str>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match data_home.filter(|d| absolute(d)) {
        Some(dir) => dirs.push(PathBuf::from(dir)),
        None => dirs.extend(home.map(|h| h.join(".local/share"))),
    }
    let system = data_dirs
        .filter(|d| !d.is_empty())
        .unwrap_or("/usr/local/share:/usr/share");
    dirs.extend(system.split(':').filter(|d| absolute(d)).map(PathBuf::from));
    dirs
}

/// A D-Bus service file of the EDS source registry (`<datadir>/dbus-1/services/`), named
/// after its bus name: `org.gnome.evolution.dataserver.Sources5.service` today.
pub fn is_eds_service_file(name: &str) -> bool {
    name.starts_with("org.gnome.evolution.dataserver.Sources") && name.ends_with(".service")
}

/// A `file://` URI as GLib writes it: bytes outside RFC 3986's unreserved set and `/` are
/// percent-encoded. `None` for a relative or non-UTF-8 path.
pub fn file_uri(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if !text.starts_with('/') {
        return None;
    }
    let mut uri = String::from("file://");
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    Some(uri)
}

/// The EDS source key file: a calendar "On This Computer" that reads `ics`, read-only so
/// nobody edits events the next sync would overwrite.
pub fn eds_source(ics: &Path) -> Option<String> {
    let uri = file_uri(ics)?;
    Some(format!(
        "[Data Source]\n\
         DisplayName={DISPLAY_NAME}\n\
         Enabled=true\n\
         Parent=local-stub\n\
         \n\
         [Calendar]\n\
         BackendName=local\n\
         Color={COLOR}\n\
         Selected=true\n\
         \n\
         [Local Backend]\n\
         CustomFile={uri}\n\
         Writable=false\n"
    ))
}

/// A program that calls D-Bus methods from the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// `dbus-send`, from the reference D-Bus implementation.
    DbusSend,
    /// `gdbus`, from GLib.
    Gdbus,
    /// `qdbus` under one of its names (`qdbus6` on Plasma 6, `qdbus-qt6`, `qdbus`).
    Qdbus(&'static str),
}

/// Tried in this order.
pub const BUSES: [Bus; 5] = [
    Bus::DbusSend,
    Bus::Gdbus,
    Bus::Qdbus("qdbus6"),
    Bus::Qdbus("qdbus-qt6"),
    Bus::Qdbus("qdbus"),
];

impl Bus {
    pub fn program(self) -> &'static str {
        match self {
            Bus::DbusSend => "dbus-send",
            Bus::Gdbus => "gdbus",
            Bus::Qdbus(name) => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Str(String),
    Bool(bool),
}

/// One method call on the session bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub service: String,
    pub path: &'static str,
    pub interface: &'static str,
    pub method: &'static str,
    pub args: Vec<Arg>,
}

/// The arguments of `bus` making `call`, as a vector (never a shell string).
pub fn call_args(bus: Bus, call: &Call) -> Vec<String> {
    let method = format!("{}.{}", call.interface, call.method);
    match bus {
        Bus::DbusSend => {
            let mut args = vec![
                "--session".to_owned(),
                "--print-reply".to_owned(),
                format!("--dest={}", call.service),
                call.path.to_owned(),
                method,
            ];
            // dbus-send takes what follows `string:` literally.
            args.extend(call.args.iter().map(|arg| match arg {
                Arg::Str(s) => format!("string:{s}"),
                Arg::Bool(b) => format!("boolean:{b}"),
            }));
            args
        }
        Bus::Gdbus => {
            let mut args = vec![
                "call".to_owned(),
                "--session".to_owned(),
                "--dest".to_owned(),
                call.service.clone(),
                "--object-path".to_owned(),
                call.path.to_owned(),
                "--method".to_owned(),
                method,
            ];
            // GVariant text: a quoted string can't be read as anything else.
            args.extend(call.args.iter().map(|arg| match arg {
                Arg::Str(s) => gvariant_string(s),
                Arg::Bool(b) => b.to_string(),
            }));
            args
        }
        Bus::Qdbus(_) => {
            let mut args = vec![call.service.clone(), call.path.to_owned(), method];
            args.extend(call.args.iter().map(|arg| match arg {
                Arg::Str(s) => s.clone(),
                Arg::Bool(b) => b.to_string(),
            }));
            args
        }
    }
}

fn gvariant_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\\' || c == '\'' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// The strings in a reply (a string, or an array of them), in order.
pub fn parse_strings(bus: Bus, stdout: &str) -> Vec<String> {
    match bus {
        // `   string "akonadi_ical_resource_0"`, one per line inside `array [ ... ]`.
        Bus::DbusSend => stdout
            .lines()
            .filter_map(|line| line.trim().strip_prefix("string \""))
            .filter_map(|rest| rest.strip_suffix('"'))
            .map(str::to_owned)
            .collect(),
        // `('akonadi_ical_resource_0',)` or `(['a', 'b'],)`.
        Bus::Gdbus => {
            let mut out = Vec::new();
            let mut chars = stdout.chars();
            while let Some(c) = chars.next() {
                if c != '\'' {
                    continue;
                }
                let mut value = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => value.extend(chars.next()),
                        '\'' => break,
                        c => value.push(c),
                    }
                }
                out.push(value);
            }
            out
        }
        // One value per line.
        Bus::Qdbus(_) => stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// Whether a failed call's error says nobody owns the bus name (the service isn't running).
pub fn is_service_unknown(stderr: &str) -> bool {
    stderr.contains("org.freedesktop.DBus.Error.ServiceUnknown")
        || stderr.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
        || (stderr.contains("Service '") && stderr.contains("does not exist"))
}

/// An instance id `createAgentInstance` hands out for the iCal agent, before it is used in
/// a bus name.
pub fn valid_instance_id(id: &str) -> bool {
    id.strip_prefix(AKONADI_AGENT)
        .and_then(|rest| rest.strip_prefix('_'))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// An `AKONADI_INSTANCE` value usable in a bus name (else the default instance).
fn instance_suffix(instance: Option<&str>) -> String {
    instance
        .filter(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .map(|i| format!(".{i}"))
        .unwrap_or_default()
}

fn control(instance: Option<&str>, method: &'static str, args: Vec<Arg>) -> Call {
    Call {
        service: format!(
            "org.freedesktop.Akonadi.Control{}",
            instance_suffix(instance)
        ),
        path: "/AgentManager",
        interface: "org.freedesktop.Akonadi.AgentManager",
        method,
        args,
    }
}

fn resource_service(id: &str, instance: Option<&str>) -> String {
    format!(
        "org.freedesktop.Akonadi.Resource.{id}{}",
        instance_suffix(instance)
    )
}

/// Every agent instance, to tell whether ours still exists.
pub fn list_instances(instance: Option<&str>) -> Call {
    control(instance, "agentInstances", Vec::new())
}

/// Creates an iCal resource; the reply is its id (empty when the type isn't installed).
pub fn create_instance(instance: Option<&str>) -> Call {
    control(
        instance,
        "createAgentInstance",
        vec![Arg::Str(AKONADI_AGENT.into())],
    )
}

pub fn remove_instance(id: &str, instance: Option<&str>) -> Call {
    control(instance, "removeAgentInstance", vec![Arg::Str(id.into())])
}

/// Points a new resource at `ics`, read-only and watching the file, then makes it load
/// the saved settings, names it and reads the file.
pub fn configure_instance(id: &str, ics: &Path, instance: Option<&str>) -> Option<Vec<Call>> {
    let path = ics.to_str()?.to_owned();
    let service = resource_service(id, instance);
    let settings = |method: &'static str, args: Vec<Arg>| Call {
        service: service.clone(),
        path: "/Settings",
        interface: "org.kde.Akonadi.ICal.Settings",
        method,
        args,
    };
    Some(vec![
        settings("setPath", vec![Arg::Str(path)]),
        settings("setDisplayName", vec![Arg::Str(DISPLAY_NAME.into())]),
        settings("setReadOnly", vec![Arg::Bool(true)]),
        settings("setMonitorFile", vec![Arg::Bool(true)]),
        settings("save", Vec::new()),
        Call {
            service: service.clone(),
            path: "/",
            interface: "org.freedesktop.Akonadi.Agent.Control",
            method: "reconfigure",
            args: Vec::new(),
        },
        control(
            instance,
            "setAgentInstanceName",
            vec![Arg::Str(id.into()), Arg::Str(DISPLAY_NAME.into())],
        ),
        control(
            instance,
            "agentInstanceSynchronize",
            vec![Arg::Str(id.into())],
        ),
    ])
}

/// The fingerprint of the whole file: each event's uid and fingerprint, in order. FNV-1a,
/// not `DefaultHasher`, whose output may change with the Rust version: the value is saved.
pub fn ics_fingerprint<'a>(events: impl IntoIterator<Item = (&'a str, u64)>) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash = (hash ^ u64::from(*b)).wrapping_mul(PRIME);
        }
    };
    for (uid, fingerprint) in events {
        feed(&(uid.len() as u64).to_le_bytes());
        feed(uid.as_bytes());
        feed(&fingerprint.to_le_bytes());
    }
    hash
}

/// What is left of a time limit for one step, at most `cap`; `None` once it has passed.
pub fn time_left(now: Instant, deadline: Instant, cap: Duration) -> Option<Duration> {
    let left = deadline.checked_duration_since(now)?;
    (!left.is_zero()).then(|| left.min(cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_follow_the_desktop() {
        assert_eq!(
            targets(true, true, Some("GNOME")),
            [CalendarStore::Gnome, CalendarStore::Kde]
        );
        assert_eq!(
            targets(true, true, Some("KDE")),
            [CalendarStore::Kde, CalendarStore::Gnome]
        );
        assert_eq!(targets(true, false, Some("KDE")), [CalendarStore::Gnome]);
        assert_eq!(targets(false, true, None), [CalendarStore::Kde]);
        assert!(targets(false, false, Some("KDE")).is_empty());
        assert_eq!(
            targets(true, true, Some("ubuntu:KDE")),
            [CalendarStore::Kde, CalendarStore::Gnome]
        );
    }

    #[test]
    fn ids_round_trip() {
        for t in CalendarStore::ALL {
            assert_eq!(CalendarStore::from_id(t.id()), Some(t));
            assert!(t.key().starts_with('@'));
        }
        assert_eq!(CalendarStore::from_id(""), None);
        assert_eq!(CalendarStore::from_id("GNOME"), None);
        // The UI reads the serialized form as the id.
        for t in CalendarStore::ALL {
            assert_eq!(serde_json::to_value(t).unwrap(), t.id());
        }
    }

    #[test]
    fn plans_switches_by_removing_first() {
        let none = |_| false;
        assert_eq!(
            plan(Some(CalendarStore::Gnome), none),
            [Step::Register(CalendarStore::Gnome)]
        );
        assert!(plan(None, none).is_empty());
        let gnome = |t| t == CalendarStore::Gnome;
        assert!(plan(Some(CalendarStore::Gnome), gnome).is_empty());
        assert_eq!(
            plan(Some(CalendarStore::Kde), gnome),
            [
                Step::Unregister(CalendarStore::Gnome),
                Step::Register(CalendarStore::Kde)
            ]
        );
        let both = |_| true;
        assert_eq!(
            plan(None, both),
            [
                Step::Unregister(CalendarStore::Gnome),
                Step::Unregister(CalendarStore::Kde)
            ]
        );
    }

    #[test]
    fn finds_the_evolution_sources_dir() {
        let home = Path::new("/home/ana");
        assert_eq!(
            eds_sources_dir(None, Some(home)),
            Some(PathBuf::from("/home/ana/.config/evolution/sources"))
        );
        assert_eq!(
            eds_sources_dir(Some("/xdg"), Some(home)),
            Some(PathBuf::from("/xdg/evolution/sources"))
        );
        assert_eq!(
            eds_sources_dir(Some("relative"), Some(home)),
            Some(PathBuf::from("/home/ana/.config/evolution/sources"))
        );
        assert_eq!(eds_sources_dir(None, None), None);
    }

    #[test]
    fn lists_data_dirs_like_xdg() {
        let home = Path::new("/home/ana");
        assert_eq!(
            data_dirs(None, None, Some(home)),
            [
                PathBuf::from("/home/ana/.local/share"),
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        );
        assert_eq!(
            data_dirs(Some("/d"), Some("/a:rel:/b"), Some(home)),
            [
                PathBuf::from("/d"),
                PathBuf::from("/a"),
                PathBuf::from("/b")
            ]
        );
    }

    #[test]
    fn recognizes_the_registry_service_file() {
        assert!(is_eds_service_file(
            "org.gnome.evolution.dataserver.Sources5.service"
        ));
        assert!(!is_eds_service_file(
            "org.gnome.evolution.dataserver.Calendar8.service"
        ));
        assert!(!is_eds_service_file(
            "org.gnome.evolution.dataserver.Sources5"
        ));
    }

    #[test]
    fn writes_file_uris_like_glib() {
        assert_eq!(
            file_uri(Path::new("/home/ana/.local/share/x/due-dates.ics")).as_deref(),
            Some("file:///home/ana/.local/share/x/due-dates.ics")
        );
        assert_eq!(
            file_uri(Path::new("/home/Ana María/a b#%.ics")).as_deref(),
            Some("file:///home/Ana%20Mar%C3%ADa/a%20b%23%25.ics")
        );
        assert_eq!(file_uri(Path::new("relative.ics")), None);
    }

    #[test]
    fn the_source_reads_the_file_read_only() {
        let text = eds_source(Path::new("/home/ana/due dates.ics")).unwrap();
        assert!(text.starts_with("[Data Source]\nDisplayName=Classroom 50\n"));
        assert!(text.contains("\nParent=local-stub\n"));
        assert!(text.contains("\n[Calendar]\nBackendName=local\n"));
        assert!(text.contains(
            "\n[Local Backend]\nCustomFile=file:///home/ana/due%20dates.ics\nWritable=false\n"
        ));
        // Every line is a group, a key=value or blank: nothing for GKeyFile to choke on.
        for line in text.lines() {
            assert!(
                line.is_empty() || line.starts_with('[') || line.contains('='),
                "{line}"
            );
        }
    }

    fn create() -> Call {
        create_instance(None)
    }

    #[test]
    fn builds_dbus_send_calls() {
        assert_eq!(
            call_args(Bus::DbusSend, &create()),
            [
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.Akonadi.Control",
                "/AgentManager",
                "org.freedesktop.Akonadi.AgentManager.createAgentInstance",
                "string:akonadi_ical_resource",
            ]
        );
        let calls = configure_instance(
            "akonadi_ical_resource_3",
            Path::new("/home/ana/it's, here.ics"),
            None,
        )
        .unwrap();
        assert_eq!(
            call_args(Bus::DbusSend, &calls[0]),
            [
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.Akonadi.Resource.akonadi_ical_resource_3",
                "/Settings",
                "org.kde.Akonadi.ICal.Settings.setPath",
                "string:/home/ana/it's, here.ics",
            ]
        );
        assert_eq!(call_args(Bus::DbusSend, &calls[2])[5], "boolean:true");
    }

    #[test]
    fn builds_gdbus_calls() {
        assert_eq!(
            call_args(Bus::Gdbus, &create()),
            [
                "call",
                "--session",
                "--dest",
                "org.freedesktop.Akonadi.Control",
                "--object-path",
                "/AgentManager",
                "--method",
                "org.freedesktop.Akonadi.AgentManager.createAgentInstance",
                "'akonadi_ical_resource'",
            ]
        );
        let calls = configure_instance("akonadi_ical_resource_3", Path::new("/a/it's\\.ics"), None)
            .unwrap();
        assert_eq!(call_args(Bus::Gdbus, &calls[0])[8], r"'/a/it\'s\\.ics'");
        assert_eq!(call_args(Bus::Gdbus, &calls[2])[8], "true");
    }

    #[test]
    fn builds_qdbus_calls() {
        assert_eq!(
            call_args(Bus::Qdbus("qdbus6"), &create()),
            [
                "org.freedesktop.Akonadi.Control",
                "/AgentManager",
                "org.freedesktop.Akonadi.AgentManager.createAgentInstance",
                "akonadi_ical_resource",
            ]
        );
    }

    #[test]
    fn configures_then_reloads_names_and_syncs() {
        let calls = configure_instance(
            "akonadi_ical_resource_3",
            Path::new("/a/b.ics"),
            Some("work"),
        )
        .unwrap();
        let methods: Vec<&str> = calls.iter().map(|c| c.method).collect();
        assert_eq!(
            methods,
            [
                "setPath",
                "setDisplayName",
                "setReadOnly",
                "setMonitorFile",
                "save",
                "reconfigure",
                "setAgentInstanceName",
                "agentInstanceSynchronize",
            ]
        );
        assert_eq!(
            calls[0].service,
            "org.freedesktop.Akonadi.Resource.akonadi_ical_resource_3.work"
        );
        assert_eq!(calls[6].service, "org.freedesktop.Akonadi.Control.work");
        assert_eq!(
            calls[6].args,
            [
                Arg::Str("akonadi_ical_resource_3".into()),
                Arg::Str("Classroom 50".into())
            ]
        );
        // An instance name that isn't a bus name element is ignored.
        assert_eq!(
            list_instances(Some("a.b")).service,
            "org.freedesktop.Akonadi.Control"
        );
        assert_eq!(
            remove_instance("akonadi_ical_resource_3", None).args,
            [Arg::Str("akonadi_ical_resource_3".into())]
        );
    }

    #[test]
    fn parses_replies() {
        let dbus_send = "method return time=1.2 sender=:1.9 -> destination=:1.80 serial=7 reply_serial=2\n   string \"akonadi_ical_resource_0\"\n";
        assert_eq!(
            parse_strings(Bus::DbusSend, dbus_send),
            ["akonadi_ical_resource_0"]
        );
        let array = "method return time=1.2 sender=:1.9 -> destination=:1.80 serial=7 reply_serial=2\n   array [\n      string \"akonadi_maildir_resource_0\"\n      string \"akonadi_ical_resource_2\"\n   ]\n";
        assert_eq!(
            parse_strings(Bus::DbusSend, array),
            ["akonadi_maildir_resource_0", "akonadi_ical_resource_2"]
        );
        assert_eq!(
            parse_strings(Bus::Gdbus, "('akonadi_ical_resource_0',)"),
            ["akonadi_ical_resource_0"]
        );
        assert_eq!(
            parse_strings(Bus::Gdbus, "(['a', 'it\\'s'],)"),
            ["a", "it's"]
        );
        assert!(parse_strings(Bus::Gdbus, "(@as [],)").is_empty());
        assert_eq!(parse_strings(Bus::Qdbus("qdbus"), "a\nb\n\n"), ["a", "b"]);
    }

    #[test]
    fn tells_a_missing_service() {
        assert!(is_service_unknown(
            "Error org.freedesktop.DBus.Error.ServiceUnknown: The name org.freedesktop.Akonadi.Control was not provided by any .service files"
        ));
        assert!(is_service_unknown(
            "Error: GDBus.Error:org.freedesktop.DBus.Error.ServiceUnknown: The name is not activatable"
        ));
        assert!(is_service_unknown(
            "Service 'org.freedesktop.Akonadi.Control' does not exist."
        ));
        assert!(!is_service_unknown(
            "Error org.freedesktop.DBus.Error.UnknownMethod: No such method"
        ));
    }

    #[test]
    fn checks_instance_ids() {
        assert!(valid_instance_id("akonadi_ical_resource_0"));
        assert!(valid_instance_id("akonadi_ical_resource_12"));
        assert!(!valid_instance_id("akonadi_ical_resource_"));
        assert!(!valid_instance_id("akonadi_ical_resource"));
        assert!(!valid_instance_id("akonadi_ical_resource_1.x"));
        assert!(!valid_instance_id("akonadi_maildir_resource_0"));
        assert!(!valid_instance_id(""));
    }

    #[test]
    fn the_file_fingerprint_follows_the_events() {
        let a = ics_fingerprint([("a", 1), ("b", 2)]);
        assert_eq!(a, ics_fingerprint([("a", 1), ("b", 2)]));
        assert_ne!(a, ics_fingerprint([("b", 2), ("a", 1)]));
        assert_ne!(a, ics_fingerprint([("a", 1), ("c", 2)]));
        assert_ne!(a, ics_fingerprint([("a", 1)]));
        // Saved in `calendar-events.json`, so it must not change between builds.
        assert_eq!(ics_fingerprint([]), 0xcbf2_9ce4_8422_2325);
        assert_ne!(
            ics_fingerprint([("ab", 1)]),
            ics_fingerprint([("a", 1), ("b", 1)])
        );
    }

    #[test]
    fn steps_stop_at_the_write_limit() {
        let now = Instant::now();
        let cap = Duration::from_secs(30);
        assert_eq!(
            time_left(now, now + Duration::from_secs(60), cap),
            Some(cap)
        );
        assert_eq!(
            time_left(now, now + Duration::from_secs(5), cap),
            Some(Duration::from_secs(5))
        );
        assert_eq!(time_left(now, now, cap), None);
        assert_eq!(time_left(now + Duration::from_secs(1), now, cap), None);
    }
}
