//! "Copy diagnostics" (Settings > About): a Markdown report a student pastes into a GitHub
//! issue. It gathers what the app already knows (versions, tool and sign-in status, sync
//! state, counts, the tail of the log file) and masks everything that names a person or a
//! class before the UI shows it: the home folder becomes `~` (and the folders under it
//! `<dir>`, well-known ones aside), logins, orgs, classrooms, classroom keys, custom Pages
//! hosts, repos and members' names become stable placeholders (`user-1`, `org-1`,
//! `class-1`, `key-1`, `host-1`, `repo-1`, `name-1`), emails `<email>`, anything shaped like
//! a GitHub token `<token>`, and the key in any Pages manifest URL or `k=` parameter, known
//! or not, `<path>` or `<key>`. Counts, kinds, versions and timings are kept. The token
//! itself is never read here.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::accounts::Account;
use crate::classroom50::{Classroom, RepoConfig};
use crate::groups::Group;
use crate::projects::{Snapshot, SyncStats};
use crate::settings::Settings;
use crate::tools::{AuthStatus, Tool, ToolStatus};

/// Lines of the log file the report ends with.
const LOG_LINES: usize = 200;
/// Only the end of the log file is read; it can grow to tens of MB.
const LOG_TAIL_BYTES: u64 = 256 * 1024;
/// A longer line (a dumped response) is cut, so one line can't fill the report.
const LOG_LINE_CHARS: usize = 400;

/// What the UI knows and Rust doesn't.
#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UiFacts {
    /// `__COMMIT__`, the commit the frontend was built from.
    pub commit: Option<String>,
    /// The `data-os` skin in use (a dev build may preview another OS's).
    pub skin: String,
    /// The catalog in use (`en`, `fr`...).
    pub locale: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub text: String,
    /// How many distinct names, keys, emails and tokens the text had and were masked
    /// (the home folder and the folders under it aside).
    pub masked: usize,
}

/// Everything the report shows, gathered by `commands::diagnostics_report`.
pub struct Facts {
    pub now: DateTime<Utc>,
    pub version: String,
    pub debug: bool,
    pub os: String,
    pub arch: String,
    pub ui: UiFacts,
    pub tools: Vec<ToolStatus>,
    pub auth: AuthStatus,
    /// `None` when gh couldn't list them.
    pub accounts: Option<Vec<Account>>,
    pub snapshot: Snapshot,
    /// Names the last discovery knew, which older log lines may hold.
    pub discovery: Discovery,
    pub stats: SyncStats,
    pub settings: Settings,
    pub editors: Vec<String>,
    /// The end of the log file, `None` when there is none.
    pub log: Option<String>,
    pub home: Option<String>,
}

/// Builds the masked report.
pub fn report(facts: &Facts) -> Report {
    let (text, masked) = Redactor::from_facts(facts).apply_counted(&render(facts));
    Report { text, masked }
}

// ---------------------------------------------------------------------------------------
// Rendering

fn render(facts: &Facts) -> String {
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    let time = |at: Option<DateTime<Utc>>| {
        at.map_or_else(
            || "never".to_owned(),
            |at| at.to_rfc3339_opts(SecondsFormat::Secs, true),
        )
    };
    let yes_no = |b: bool| if b { "yes" } else { "no" };

    line("### Classroom 50 Toolbox diagnostics".into());
    line(String::new());
    let commit = facts
        .ui
        .commit
        .as_deref()
        .filter(|c| !c.is_empty())
        .map_or(String::new(), |c| format!(" ({c})"));
    let build = if facts.debug { "debug" } else { "release" };
    line(format!(
        "- Version: {}{commit}, {build} build",
        facts.version
    ));
    line(format!("- OS: {} ({})", facts.os, facts.arch));
    line(format!(
        "- Skin: {}, language: {} (setting: {})",
        facts.ui.skin, facts.ui.locale, facts.settings.language
    ));
    line(format!("- Generated: {}", time(Some(facts.now))));

    line(String::new());
    line("#### Tools".into());
    for tool in &facts.tools {
        let name = match tool.tool {
            Tool::Git => "git",
            Tool::Gh => "gh",
            Tool::GhStudent => "gh-student",
        };
        let state = match (&tool.version, tool.installed) {
            (_, false) => "not installed".to_owned(),
            (Some(version), true) => version.clone(),
            (None, true) => "installed, version unknown".to_owned(),
        };
        let outdated = if tool.outdated { " (outdated)" } else { "" };
        line(format!("- {name}: {state}{outdated}"));
    }

    line(String::new());
    line("#### GitHub sign-in".into());
    let auth = &facts.auth;
    line(format!(
        "- Signed in: {}{}",
        yes_no(auth.logged_in),
        auth.login
            .as_deref()
            .map_or(String::new(), |l| format!(" as {l}"))
    ));
    line(format!(
        "- Missing scopes: {}",
        if auth.missing_scopes.is_empty() {
            "none".to_owned()
        } else {
            auth.missing_scopes.join(", ")
        }
    ));
    line(format!(
        "- Token from the environment: {}",
        auth.env_token.as_deref().unwrap_or("no")
    ));
    line(format!(
        "- Accounts in gh: {}",
        facts
            .accounts
            .as_ref()
            .map_or("unknown".to_owned(), |a| a.len().to_string())
    ));
    if let Some(login) = &facts.snapshot.login
        && !auth
            .login
            .as_ref()
            .is_some_and(|l| l.eq_ignore_ascii_case(login))
    {
        line(format!("- Data belongs to: {login}"));
    }

    line(String::new());
    line("#### Sync".into());
    let run = |run: &Option<crate::projects::LastRun>| {
        run.as_ref().map_or("never".to_owned(), |r| {
            format!(
                "{} ({:.1} s)",
                r.at.to_rfc3339_opts(SecondsFormat::Secs, true),
                r.seconds
            )
        })
    };
    let stats = &facts.stats;
    line(format!(
        "- Last full sync: {} (list from {})",
        run(&stats.full),
        time(facts.snapshot.synced_at)
    ));
    line(format!("- Last status check: {}", run(&stats.status)));
    line(format!("- Last local scan: {}", run(&stats.local)));
    line(format!(
        "- Last GitHub read: {}",
        time(facts.snapshot.checked_at)
    ));
    match &facts.snapshot.issue {
        None => line("- Last error: none".into()),
        Some(issue) => {
            line(format!("- Last error: {} ({})", issue.kind, issue.message));
            if let Some(until) = issue.retry_after() {
                line(format!("- Backing off until: {}", time(Some(until))));
            }
        }
    }
    line(format!(
        "- Offline: {}",
        yes_no(
            facts
                .snapshot
                .issue
                .as_ref()
                .is_some_and(|i| i.kind == "offline")
        )
    ));
    line(format!(
        "- GraphQL budget: {}",
        stats.rate_limit.map_or("unknown".to_owned(), |r| format!(
            "{}/{} left, resets {}",
            r.remaining,
            r.limit,
            time(r.reset_at)
        ))
    ));
    for (mode, phases) in &stats.phases {
        let costs: Vec<String> = phases
            .iter()
            .map(|(phase, cost)| {
                format!("{phase:?} {:.1} s/{} steps", cost.seconds, cost.steps).to_lowercase()
            })
            .collect();
        line(format!("- Phases ({mode}): {}", costs.join(", ")));
    }

    line(String::new());
    line("#### Counts".into());
    let projects = &facts.snapshot.projects;
    let mut classes: Vec<String> = projects
        .iter()
        .filter_map(|p| p.classroom.as_ref())
        .chain(facts.snapshot.unaccepted.iter().map(|u| &u.classroom))
        .map(|c| format!("{}/{}", c.org, c.slug).to_lowercase())
        .collect();
    classes.sort();
    classes.dedup();
    let settings = &facts.settings;
    line(format!("- Classes: {}", classes.len()));
    line(format!(
        "- Projects: {} ({} with a clone, {} on GitHub)",
        projects.len(),
        projects.iter().filter(|p| p.path.is_some()).count(),
        projects.iter().filter(|p| p.remote.is_some()).count()
    ));
    line(format!(
        "- Not accepted: {}",
        facts.snapshot.unaccepted.len()
    ));
    line(format!("- Repos seen on GitHub: {}", stats.repos_seen));
    line(format!(
        "- Watched folders: {} (editor projects scanned: {})",
        settings.watched_folders.len(),
        yes_no(settings.scan_editor_projects)
    ));
    line(format!(
        "- Scan roots: {}, watch targets: {}",
        stats.scan_roots, stats.watch_targets
    ));
    line(format!(
        "- Editors found: {}",
        if facts.editors.is_empty() {
            "none".to_owned()
        } else {
            facts.editors.join(", ")
        }
    ));
    line(format!(
        "- Hidden: {} items, {} classes",
        settings.hidden.len(),
        settings.hidden_classes.len()
    ));

    line(String::new());
    line("#### Settings".into());
    line(format!(
        "- Appearance: {}, commit actions: {}, launch at login: {}",
        label(&settings.appearance),
        label(&settings.commit_actions),
        yes_no(settings.launch_at_login)
    ));
    line(format!(
        "- Reminders: {:?} h before, calendar file: {}, calendar app: {}",
        settings.notify_hours_before,
        yes_no(settings.calendar_file.is_some()),
        yes_no(settings.native_calendar.is_some())
    ));

    line(String::new());
    match &facts.log {
        None => line("#### Log\n\nNo log file.".into()),
        Some(log) => {
            line(format!("#### Log (last {LOG_LINES} lines)"));
            line(String::new());
            let fence = fence_for(log);
            line(format!("{fence}text"));
            line(log.trim_end().to_owned());
            line(fence);
        }
    }
    out
}

/// The serialized name of a settings enum (`"system"`, `"push"`).
fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A Markdown fence longer than any backtick run inside, so the log can't close it.
fn fence_for(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat((longest + 1).max(3))
}

// ---------------------------------------------------------------------------------------
// The log file

/// The last `LOG_LINES` lines of the log file, each cut at `LOG_LINE_CHARS`. Right after a
/// rotation the file is short, so the newest older file the plugin kept fills in the lines
/// before it.
pub fn read_log(path: &Path) -> Option<String> {
    let current = read_tail(path, LOG_LINES)?;
    let count = if current.is_empty() {
        0
    } else {
        current.lines().count()
    };
    if count >= LOG_LINES {
        return Some(current);
    }
    let older = previous_log(path)
        .and_then(|older| read_tail(&older, LOG_LINES - count))
        .filter(|older| !older.is_empty());
    Some(match older {
        Some(older) if current.is_empty() => older,
        Some(older) => format!("{older}\n{current}"),
        None => current,
    })
}

/// The last `count` lines of the file at `path`, read from its last `LOG_TAIL_BYTES`.
fn read_tail(path: &Path, count: usize) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(last_lines(&text, count, start > 0))
}

/// The newest file rotated out of `path`: tauri-plugin-log names them
/// `<name>_<YYYY-MM-DD_HH-MM-SS>.log` next to it, so the largest name is the newest.
fn previous_log(path: &Path) -> Option<std::path::PathBuf> {
    let prefix = format!("{}_", path.file_stem()?.to_str()?);
    std::fs::read_dir(path.parent()?)
        .ok()?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            (name.starts_with(&prefix) && name.ends_with(".log")).then_some(name)
        })
        .max()
        .map(|name| path.with_file_name(name))
}

/// The last `count` lines of `text`; `cut` drops the first one, which reading from the
/// middle of the file starts partway through.
fn last_lines(text: &str, count: usize, cut: bool) -> String {
    let lines: Vec<&str> = text.lines().skip(usize::from(cut)).collect();
    let from = lines.len().saturating_sub(count);
    lines[from..]
        .iter()
        .map(|line| {
            let line = line.trim_end_matches('\r');
            match line.char_indices().nth(LOG_LINE_CHARS) {
                Some((at, _)) => format!("{}... (cut)", &line[..at]),
                None => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------
// Redaction

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    User,
    Org,
    Class,
    Repo,
    Name,
    /// An unlisted classroom's key, part of its Pages URL and accept link.
    Key,
    /// A custom Pages host (`pages_base_url`), which often names the school.
    Host,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::User => "user",
            Kind::Org => "org",
            Kind::Class => "class",
            Kind::Repo => "repo",
            Kind::Name => "name",
            Kind::Key => "key",
            Kind::Host => "host",
        }
    }
}

/// Names the last discovery knew besides the snapshot's (`projects::ProjectsState::
/// discovery_names`): every classroom team, group and assignment repo, so log lines from
/// before a class was left or a group changed are masked too.
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub classrooms: Vec<Classroom>,
    pub groups: Vec<Group>,
    /// Owner, name and marker file of each repo of a classroom's org.
    pub repos: Vec<(String, String, Option<RepoConfig>)>,
}

/// Replaces known names with placeholders, numbered per kind in the order they're added,
/// so the same name always reads the same within a report.
#[derive(Debug, Default)]
pub struct Redactor {
    /// Case-folded name to placeholder; the first kind a name is added with wins.
    terms: HashMap<String, String>,
    counters: BTreeMap<Kind, usize>,
    /// The home folder, as written, replaced with `~`.
    home: Vec<String>,
}

impl Redactor {
    pub fn from_facts(facts: &Facts) -> Self {
        let mut r = Self::default();
        if let Some(home) = &facts.home {
            r.home(home);
        }
        // Users first: the signed-in account is `user-1`.
        let logins = [facts.auth.login.as_deref(), facts.snapshot.login.as_deref()];
        for login in logins.into_iter().flatten() {
            r.add(Kind::User, login);
        }
        for account in facts.accounts.iter().flatten() {
            r.add(Kind::User, &account.login);
        }
        let snapshot = &facts.snapshot;
        let known = &facts.discovery;
        let groups = snapshot
            .projects
            .iter()
            .filter_map(|p| p.group.as_ref())
            .chain(snapshot.unaccepted.iter().filter_map(|u| u.group.as_ref()))
            .chain(&known.groups);
        for group in groups {
            for member in &group.members {
                r.add(Kind::User, &member.login);
                if let Some(name) = &member.name {
                    r.add(Kind::Name, name);
                }
            }
            r.add(Kind::Org, &group.org);
            if let Some(name) = &group.name {
                r.add(Kind::Name, name);
            }
            if let Some(classroom) = &group.classroom {
                r.add(Kind::Class, classroom);
            }
        }
        let classrooms = snapshot
            .projects
            .iter()
            .filter_map(|p| p.classroom.as_ref())
            .chain(snapshot.unaccepted.iter().map(|u| &u.classroom))
            .chain(&known.classrooms);
        for classroom in classrooms {
            r.add(Kind::Org, &classroom.org);
            r.add(Kind::Class, &classroom.slug);
            if let Some(name) = &classroom.name {
                r.add(Kind::Class, name);
            }
            if let Some(secret) = &classroom.secret {
                r.add(Kind::Key, secret);
            }
            if let Some(host) = classroom.pages_base_url.as_deref().and_then(url_host) {
                r.add(Kind::Host, host);
            }
        }
        for project in &snapshot.projects {
            if let Some(owner) = &project.owner {
                r.add(Kind::Org, owner);
            }
            r.add(Kind::Class, &project.config.classroom);
            if let Some(secret) = &project.config.secret {
                r.add(Kind::Key, secret);
            }
            if let Some(repo) = &project.repo {
                r.add(Kind::Repo, repo);
            }
            // The clone's folder, usually the repo's name but not always.
            if let Some(folder) = project.path.as_deref().and_then(Path::file_name) {
                r.add(Kind::Repo, &folder.to_string_lossy());
            }
        }
        for (owner, name, config) in &known.repos {
            r.add(Kind::Org, owner);
            r.add(Kind::Repo, name);
            if let Some(config) = config {
                r.add(Kind::Class, &config.classroom);
                if let Some(secret) = &config.secret {
                    r.add(Kind::Key, secret);
                }
            }
        }
        // The OS account name, in paths outside the home folder.
        if let Some(user) = facts.home.as_deref().and_then(|h| Path::new(h).file_name()) {
            r.add(Kind::User, &user.to_string_lossy());
        }
        r
    }

    /// Masks `path` (its other-slash form, and with backslashes doubled as `{:?}` writes
    /// them) as `~`.
    pub fn home(&mut self, path: &str) {
        let path = path.trim_end_matches(['/', '\\']);
        if path.is_empty() {
            return;
        }
        let forms = [
            path.to_owned(),
            path.replace('\\', "/"),
            path.replace('\\', "\\\\"),
        ];
        for form in forms {
            if !self.home.contains(&form) {
                self.home.push(form);
            }
        }
    }

    /// Masks `value` as the next placeholder of `kind`, unless it is already masked.
    pub fn add(&mut self, kind: Kind, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        let key = fold(value);
        if self.terms.contains_key(&key) {
            return;
        }
        let n = self.counters.entry(kind).or_default();
        *n += 1;
        self.terms.insert(key, format!("{}-{n}", kind.prefix()));
    }

    /// The placeholder of a known name, matched like in the text.
    fn placeholder(&self, value: &str) -> Option<&str> {
        self.terms.get(&fold(value)).map(String::as_str)
    }

    #[cfg(test)]
    pub fn apply(&self, text: &str) -> String {
        self.apply_counted(text).0
    }

    /// Masks tokens, emails, classroom keys and Pages URLs, then the home folder and every
    /// known name, longest first, in one pass so a placeholder is never masked again, and
    /// last what paths under the home folder say beyond well-known folders and masked
    /// names. Also returns how many distinct names and secrets it actually masked.
    pub fn apply_counted(&self, text: &str) -> (String, usize) {
        let mut needles: Vec<(Vec<char>, &str)> = self
            .home
            .iter()
            .map(|h| (fold(h).chars().collect(), "~"))
            .chain(
                self.terms
                    .iter()
                    .map(|(k, v)| (k.chars().collect(), v.as_str())),
            )
            .collect();
        needles.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

        let mut out = String::with_capacity(text.len());
        let mut used: HashSet<&str> = HashSet::new();
        let mut secrets: HashSet<&str> = HashSet::new();
        for segment in self.split_secrets(text) {
            match segment {
                Segment::Plain(plain) => replace_terms(plain, &needles, &mut out, &mut used),
                Segment::Masked { original, mask } => {
                    secrets.insert(original);
                    out.push_str(&mask);
                }
            }
        }
        // The home folder isn't a name: it is counted apart (see `Report::masked`).
        let names = used.iter().filter(|p| **p != "~").count();
        (mask_home_paths(&out), names + secrets.len())
    }

    /// Splits `text` into plain runs, which names are masked in, and the secrets found in
    /// it with their masks: tokens, emails, `k=` keys and Pages manifest URLs.
    fn split_secrets<'a>(&self, text: &'a str) -> Vec<Segment<'a>> {
        let mut spans: Vec<(usize, usize, String)> = Vec::new();
        let fixed = |spans: Vec<(usize, usize)>, mask: &str| {
            spans
                .into_iter()
                .map(|(s, e)| (s, e, mask.to_owned()))
                .collect::<Vec<_>>()
        };
        spans.extend(fixed(token_spans(text), "<token>"));
        spans.extend(fixed(email_spans(text), "<email>"));
        spans.extend(fixed(key_spans(text), "<key>"));
        spans.extend(
            manifest_url_spans(text)
                .into_iter()
                .map(|(s, e)| (s, e, self.mask_manifest_url(&text[s..e]))),
        );
        spans.sort();
        let mut out = Vec::new();
        let mut at = 0;
        let mut last: Option<(usize, usize, String)> = None;
        for (start, end, mask) in spans {
            match &mut last {
                // Overlapping secrets are masked as one, under the first one's mask.
                Some((_, last_end, _)) if start < *last_end => {
                    *last_end = (*last_end).max(end);
                }
                _ => {
                    if let Some((s, e, mask)) = last.take() {
                        out.push(Segment::Masked {
                            original: &text[s..e],
                            mask,
                        });
                        at = e;
                    }
                    if start > at {
                        out.push(Segment::Plain(&text[at..start]));
                    }
                    last = Some((start, end, mask));
                }
            }
        }
        if let Some((s, e, mask)) = last {
            out.push(Segment::Masked {
                original: &text[s..e],
                mask,
            });
            at = e;
        }
        if at < text.len() {
            out.push(Segment::Plain(&text[at..]));
        }
        out
    }

    /// `https://<host>/<path>/assignments.json` with the host and every path segment but
    /// `classroom50` replaced by its placeholder when known, else by `<org>` (of
    /// `<org>.github.io`), `<host>` or `<path>`: the key of an unlisted class, a custom
    /// Pages host or a class this report doesn't know never shows.
    fn mask_manifest_url(&self, url: &str) -> String {
        let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
        let mut parts = rest.split('/');
        let host = parts.next().unwrap_or_default();
        let host = match host.to_ascii_lowercase().strip_suffix(".github.io") {
            Some(org) => format!("{}.github.io", self.placeholder(org).unwrap_or("<org>")),
            None => self.placeholder(host).unwrap_or("<host>").to_owned(),
        };
        let path: Vec<&str> = parts
            .map(|segment| match segment {
                "classroom50" | MANIFEST_FILE => segment,
                _ => self.placeholder(segment).unwrap_or("<path>"),
            })
            .collect();
        format!("{scheme}://{host}/{}", path.join("/"))
    }
}

enum Segment<'a> {
    Plain(&'a str),
    Masked { original: &'a str, mask: String },
}

/// Case folding for matching names: full Unicode lowercase per char (so `É` matches `é`
/// and `İ` its two-char lowercase), with the final sigma folded like any sigma.
fn fold_char(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase().map(|c| if c == 'ς' { 'σ' } else { c })
}

fn fold(text: &str) -> String {
    text.chars().flat_map(fold_char).collect()
}

/// Whether `c` can be part of a name, so a match next to it is inside a longer word.
fn word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// The byte length of the text at the start of `text` that folds to `needle`, if any. A
/// char whose fold the needle only partly covers doesn't match.
fn folded_match(text: &str, needle: &[char]) -> Option<usize> {
    let mut want = needle.iter();
    let mut len = 0;
    for c in text.chars() {
        for f in fold_char(c) {
            if want.next() != Some(&f) {
                return None;
            }
        }
        len += c.len_utf8();
        if want.len() == 0 {
            return Some(len);
        }
    }
    None
}

/// Appends `text` to `out` with each needle (matched with Unicode case folding, not inside
/// a longer word) replaced by its placeholder, recording the placeholders used.
fn replace_terms<'n>(
    text: &str,
    needles: &[(Vec<char>, &'n str)],
    out: &mut String,
    used: &mut HashSet<&'n str>,
) {
    let mut i = 0;
    let mut previous: Option<char> = None;
    while i < text.len() {
        let rest = &text[i..];
        let first: Vec<char> = rest
            .chars()
            .next()
            .map(fold_char)
            .into_iter()
            .flatten()
            .collect();
        let found = previous
            .is_none_or(|c| !word_char(c))
            .then(|| {
                needles.iter().find_map(|(needle, placeholder)| {
                    if !needle.starts_with(&first[..first.len().min(needle.len())]) {
                        return None;
                    }
                    let len = folded_match(rest, needle)?;
                    rest[len..]
                        .chars()
                        .next()
                        .is_none_or(|c| !word_char(c))
                        .then_some((len, *placeholder))
                })
            })
            .flatten();
        match found {
            Some((len, placeholder)) => {
                out.push_str(placeholder);
                used.insert(placeholder);
                previous = rest[..len].chars().last();
                i += len;
            }
            None => {
                let c = rest.chars().next().unwrap_or_default();
                out.push(c);
                i += c.len_utf8();
                previous = Some(c);
            }
        }
    }
}

/// Folder names that say nothing about the student: the editors' default project folders
/// (`editors::DEFAULT_ROOTS`), the usual folders of a home dir and git's own.
const KEPT_FOLDERS: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "AppData",
    "Roaming",
    "Local",
    "LocalLow",
    "source",
    "workspace",
    "GitHub",
    ".git",
    ".config",
    ".local",
    "share",
    ".cache",
    ".idea",
    ".vscode",
    ".classroom50.yaml",
    "node_modules",
    "target",
];

/// Whether a folder in a path under `~` can stay as written.
fn kept_folder(name: &str) -> bool {
    let placeholder = name.split_once('-').is_some_and(|(kind, n)| {
        ["user", "org", "class", "repo", "name", "key", "host"].contains(&kind)
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit())
    });
    placeholder
        || (name.starts_with('<') && name.ends_with('>'))
        || KEPT_FOLDERS.contains(&name)
        || crate::editors::DEFAULT_ROOTS
            .iter()
            .any(|(root, _)| root.split('/').any(|part| part == name))
}

/// Paths under the home folder (`~/...` once masked) keep `~`, well-known folders and
/// masked names; every other folder or file becomes `<dir>`, so an unrelated folder the
/// watcher saw change (`~/Documents/cover-letter.docx`) doesn't show. Past `.git`, git's
/// own files stay. A path after a `"` runs to the closing quote (`{:?}`, which may hold
/// spaces), else to whitespace or punctuation that ends a path in a sentence.
fn mask_home_paths(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let starts_path = c == '~'
            && previous.is_none_or(|p| !word_char(p) && !matches!(p, '/' | '\\' | '~'))
            && matches!(rest[1..].chars().next(), Some('/' | '\\'));
        if !starts_path {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            previous = Some(c);
            continue;
        }
        let quoted = previous == Some('"');
        let end = rest
            .char_indices()
            .find(|&(_, c)| {
                if quoted {
                    c == '"'
                } else {
                    c.is_whitespace() || "\"'`<>|,;:()[]{}".contains(c)
                }
            })
            .map_or(rest.len(), |(at, _)| at);
        let (path, after) = rest.split_at(end);
        let mut in_git = false;
        let mut piece = String::new();
        let flush = |piece: &mut String, out: &mut String, in_git: &mut bool| {
            if piece.is_empty() || piece == "~" || *in_git || kept_folder(piece) {
                out.push_str(piece);
            } else {
                // A sentence's full stop isn't part of the name.
                out.push_str("<dir>");
                if piece.ends_with('.') && !piece.starts_with('.') {
                    out.push('.');
                }
            }
            *in_git |= piece == ".git";
            piece.clear();
        };
        for c in path.chars() {
            if matches!(c, '/' | '\\') {
                flush(&mut piece, &mut out, &mut in_git);
                out.push(c);
            } else {
                piece.push(c);
            }
        }
        flush(&mut piece, &mut out, &mut in_git);
        previous = path.chars().last();
        rest = after;
    }
    out
}

const MANIFEST_FILE: &str = "assignments.json";

/// The host of an `https://` URL.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then_some(host)
}

/// Byte ranges of URLs ending in `/assignments.json`: a class's Pages manifest, which
/// holds the classroom key of an unlisted class and maybe a custom host.
fn manifest_url_spans(text: &str) -> Vec<(usize, usize)> {
    let url_byte = |b: u8| !b.is_ascii_whitespace() && !b"\"'`<>()[]{}|,;".contains(&b);
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let suffix = format!("/{MANIFEST_FILE}");
    for (at, _) in text.match_indices(&suffix) {
        let end = at + suffix.len();
        let mut start = at;
        while start > 0 && url_byte(bytes[start - 1]) {
            start -= 1;
        }
        let url = &text[start..end];
        // Text glued before the scheme (`url=https://...`) isn't part of the URL.
        let scheme = url.find("https://").or_else(|| url.find("http://"));
        if let Some(offset) = scheme {
            spans.push((start + offset, end));
        }
    }
    spans
}

/// Byte ranges of the value of a `k=` query parameter: the classroom key in an accept
/// link (`?k=<key>`).
fn key_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    for (at, _) in text.match_indices("k=") {
        if at == 0 || !matches!(bytes[at - 1], b'?' | b'&') {
            continue;
        }
        let start = at + 2;
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end > start {
            spans.push((start, end));
        }
    }
    spans
}

const TOKEN_PREFIXES: &[&str] = &["github_pat_", "gho_", "ghp_", "ghs_", "ghu_", "ghr_"];

/// Byte ranges of anything shaped like a GitHub token: a known prefix, not inside a word,
/// followed by at least 16 letters, digits or underscores.
fn token_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let token_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let starts_word = i == 0 || !token_byte(bytes[i - 1]);
        let prefix = starts_word
            .then(|| {
                TOKEN_PREFIXES.iter().find(|p| {
                    bytes.len() >= i + p.len()
                        && bytes[i..i + p.len()].eq_ignore_ascii_case(p.as_bytes())
                })
            })
            .flatten();
        if let Some(prefix) = prefix {
            let body_start = i + prefix.len();
            let mut end = body_start;
            while end < bytes.len() && token_byte(bytes[end]) {
                end += 1;
            }
            if end - body_start >= 16 {
                spans.push((i, end));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    spans
}

/// Byte ranges of email addresses: `local@domain.tld`, non-ASCII letters included (a
/// non-ASCII byte is part of a letter, so the range stays on char boundaries). `git@host`
/// (an ssh remote) is kept.
fn email_spans(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let local_byte = |b: u8| b.is_ascii_alphanumeric() || b >= 0x80 || b"._%+-".contains(&b);
    let domain_byte = |b: u8| b.is_ascii_alphanumeric() || b >= 0x80 || b == b'.' || b == b'-';
    let mut spans = Vec::new();
    for (at, _) in text.match_indices('@') {
        let mut start = at;
        while start > 0 && local_byte(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len() && domain_byte(bytes[end]) {
            end += 1;
        }
        let local = &text[start..at];
        let domain = text[at + 1..end].trim_end_matches(['.', '-']);
        let end = at + 1 + domain.len();
        let tld_ok = domain
            .rsplit_once('.')
            .is_some_and(|(host, tld)| !host.is_empty() && tld.chars().count() >= 2);
        if !local.is_empty() && local != "git" && tld_ok {
            spans.push((start, end));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        let mut r = Redactor::default();
        r.home("/Users/alice");
        r.add(Kind::User, "alice");
        r.add(Kind::User, "Bob");
        r.add(Kind::Org, "cs-school");
        r.add(Kind::Class, "cs50");
        r.add(Kind::Repo, "cs50-hello-alice");
        r
    }

    #[test]
    fn masks_home_logins_orgs_classes_and_repos() {
        let r = redactor();
        assert_eq!(
            r.apply("cloned cs-school/cs50-hello-alice into /Users/alice/code"),
            "cloned org-1/repo-1 into ~/code"
        );
        assert_eq!(r.apply("bob pushed"), "user-2 pushed");
        assert_eq!(r.apply("class cs50 by ALICE"), "class class-1 by user-1");
        // Only what the text holds counts, the home folder aside.
        assert_eq!(r.apply_counted("cs50 cs50 /Users/alice bob@uni.edu").1, 2);
    }

    #[test]
    fn longest_name_wins_and_placeholders_stay_put() {
        let r = redactor();
        // The repo contains the login and the class; it is masked whole.
        assert_eq!(r.apply("CS50-Hello-Alice"), "repo-1");
        // A login that looks like a placeholder isn't masked twice.
        let mut r = Redactor::default();
        r.add(Kind::User, "user");
        r.add(Kind::Repo, "user-1");
        assert_eq!(r.apply("user user-1"), "user-1 repo-1");
    }

    #[test]
    fn a_login_inside_another_word_is_kept() {
        let r = redactor();
        assert_eq!(r.apply("alicebob malice bob2"), "alicebob malice bob2");
        // Separators count as boundaries, so a login inside an unknown repo is masked.
        assert_eq!(r.apply("cs50-world-alice"), "class-1-world-user-1");
        assert_eq!(r.apply("alice_x"), "user-1_x");
    }

    #[test]
    fn home_is_masked_with_either_slash_and_only_whole() {
        let mut r = Redactor::default();
        r.home(r"C:\Users\alice\");
        assert_eq!(r.apply(r"C:\Users\alice\IdeaProjects"), r"~\IdeaProjects");
        assert_eq!(r.apply("c:/users/alice/x"), "~/<dir>");
        assert_eq!(r.apply(r"C:\Users\alice2"), r"C:\Users\alice2");
    }

    #[test]
    fn numbering_is_per_kind_and_stable() {
        let mut r = Redactor::default();
        r.add(Kind::User, "a1");
        r.add(Kind::Org, "o1");
        r.add(Kind::User, "A1");
        r.add(Kind::Org, "a1");
        r.add(Kind::User, "a2");
        r.add(Kind::User, "  ");
        assert_eq!(
            r.apply_counted("a1 o1 a2"),
            ("user-1 org-1 user-2".to_owned(), 3)
        );
    }

    #[test]
    fn masks_emails_but_not_ssh_remotes() {
        let r = redactor();
        assert_eq!(
            r.apply("from 123+alice@users.noreply.github.com and Bob.Smith@uni.edu."),
            "from <email> and <email>."
        );
        assert_eq!(
            r.apply("git@github.com:cs-school/cs50-hello-alice.git"),
            "git@github.com:org-1/repo-1.git"
        );
        assert_eq!(r.apply("a @ b and x@localhost"), "a @ b and x@localhost");
    }

    #[test]
    fn strips_tokens() {
        let r = Redactor::default();
        let token = format!("gho_{}", "A1b2".repeat(9));
        assert_eq!(r.apply(&format!("token={token};")), "token=<token>;");
        assert_eq!(
            r.apply(&format!("Bearer github_pat_{}", "x_9".repeat(20))),
            "Bearer <token>"
        );
        assert_eq!(
            r.apply("ghp_short gho_ xghp_abcdefghijklmnopqrstuv"),
            "ghp_short gho_ xghp_abcdefghijklmnopqrstuv"
        );
    }

    #[test]
    fn handles_non_ascii_text() {
        let mut r = Redactor::default();
        r.add(Kind::Name, "Zoë Müller");
        r.add(Kind::User, "zoe");
        assert_eq!(
            r.apply("Zoë Müller (zoe) a écrit"),
            "name-1 (user-1) a écrit"
        );
        assert_eq!(r.apply("éalice zoë"), "éalice zoë");
    }

    #[test]
    fn matches_names_with_unicode_case_folding() {
        let mut r = Redactor::default();
        r.add(Kind::Name, "Élodie Martin");
        r.add(Kind::User, "ángel");
        r.add(Kind::User, "İrem");
        r.add(Kind::Repo, "TP-Élise");
        r.add(Kind::Name, "ΟΔΥΣΣΕΥΣ");
        assert_eq!(
            r.apply("ÉLODIE MARTIN, élodie martin, Élodie Martin"),
            "name-1, name-1, name-1"
        );
        assert_eq!(r.apply("ÁNGEL Ángel"), "user-1 user-1");
        // `İ` lowercases to two chars: the text matches either way it is written.
        assert_eq!(r.apply("İrem i\u{307}rem İREM"), "user-2 user-2 user-2");
        assert_eq!(r.apply("~/tp-élise ~/TP-ÉLISE"), "~/repo-1 ~/repo-1");
        assert_eq!(r.apply("οδυσσευς Οδυσσευς"), "name-2 name-2");
        // Still whole words only, and a partly matched char is no match.
        assert_eq!(r.apply("ángeles irem"), "ángeles irem");
    }

    #[test]
    fn masks_classroom_keys_and_pages_urls() {
        let mut r = Redactor::default();
        r.add(Kind::Org, "cs-school");
        r.add(Kind::Class, "cs50");
        r.add(Kind::Key, "s3cr3t");
        r.add(Kind::Host, "pages.school.edu");
        assert_eq!(
            r.apply("could not fetch https://cs-school.github.io/classroom50/cs50/s3cr3t/assignments.json: 404"),
            "could not fetch https://org-1.github.io/classroom50/class-1/key-1/assignments.json: 404"
        );
        // A key or org the report doesn't know is masked all the same.
        assert_eq!(
            r.apply("(https://left.github.io/classroom50/algo/abcd1234/assignments.json)"),
            "(https://<org>.github.io/classroom50/<path>/<path>/assignments.json)"
        );
        assert_eq!(
            r.apply("url=https://pages.school.edu/c50/cs50/assignments.json"),
            "url=https://host-1/<path>/class-1/assignments.json"
        );
        assert_eq!(
            r.apply("https://other.example.org/x/assignments.json"),
            "https://<host>/<path>/assignments.json"
        );
        assert_eq!(
            r.apply("accept?k=zz99&x=1 and /accept?a=1&k=Q2 but ok=yes"),
            "accept?k=<key>&x=1 and /accept?a=1&k=<key> but ok=yes"
        );
        // The known key alone, anywhere.
        assert_eq!(r.apply("key s3cr3t"), "key key-1");
    }

    #[test]
    fn masks_folders_under_home() {
        let r = redactor();
        assert_eq!(
            r.apply("rescanning after a change in /Users/alice/Developer/acme/pitch.key"),
            "rescanning after a change in ~/Developer/<dir>/<dir>"
        );
        assert_eq!(
            r.apply(r#"watching "/Users/alice/My Notes/Lettre Dupont.docx" now"#),
            r#"watching "~/<dir>/<dir>" now"#
        );
        assert_eq!(
            r.apply("in /Users/alice/IdeaProjects/cs50-hello-alice/.git/refs/remotes/origin."),
            "in ~/IdeaProjects/repo-1/.git/refs/remotes/origin."
        );
        let mut w = Redactor::default();
        w.home(r"C:\Users\alice");
        // `{:?}` doubles the backslashes.
        assert_eq!(
            w.apply(r#"cannot watch "C:\\Users\\alice\\AppData\\Secret Plan": denied"#),
            r#"cannot watch "~\\AppData\\<dir>": denied"#
        );
        assert_eq!(
            w.apply(r"in C:\Users\alice\Desktop\plan.txt"),
            r"in ~\Desktop\<dir>"
        );
        assert_eq!(r.apply("a ~ b, ~/x."), "a ~ b, ~/<dir>.");
        // A path outside home isn't touched beyond the names.
        assert_eq!(r.apply("/opt/homebrew/bin/gh"), "/opt/homebrew/bin/gh");
    }

    #[test]
    fn overlapping_secrets_are_masked_whole() {
        let r = Redactor::default();
        let token = format!("ghp_{}", "a".repeat(20));
        assert_eq!(r.apply(&format!("{token}.b@c.com end")), "<token> end");
    }

    #[test]
    fn masks_emails_with_non_ascii_letters() {
        let r = Redactor::default();
        assert_eq!(
            r.apply("from zoë@uni.edu and élise.m@école.fr."),
            "from <email> and <email>."
        );
    }

    #[test]
    fn the_report_names_nobody() {
        let project: crate::projects::Project = serde_json::from_value(serde_json::json!({
            "id": "cs-school/cs50-hello-alice",
            "owner": "CS-School",
            "repo": "cs50-hello-alice",
            "config": { "classroom": "cs50", "assignment": "hello" },
            "path": "/Users/alice/IdeaProjects/hello-clone",
            "classroom": {
                "org": "cs-school", "slug": "cs50", "name": "Intro to CS",
                "term": null, "active": true, "secret": null, "pagesBaseUrl": null
            },
            "group": {
                "org": "cs-school", "slug": "g", "number": 1, "name": "The Owls",
                "members": [
                    { "login": "alice", "name": "Alice Doe" },
                    { "login": "carol99", "name": null, "email": "carol@uni.edu" }
                ],
                "url": "https://github.com/orgs/cs-school/teams/g"
            }
        }))
        .unwrap();
        let token = format!("ghp_{}", "Z9".repeat(18));
        let facts = Facts {
            now: Utc::now(),
            version: "0.9.0".into(),
            debug: false,
            os: "macOS 26.5".into(),
            arch: "aarch64".into(),
            ui: UiFacts {
                commit: Some("fd6c24e".into()),
                skin: "macos".into(),
                locale: "fr".into(),
            },
            tools: vec![ToolStatus {
                tool: Tool::Gh,
                installed: true,
                version: Some("2.81.0".into()),
                outdated: false,
            }],
            auth: AuthStatus {
                logged_in: true,
                login: Some("Alice".into()),
                missing_scopes: vec!["workflow".into()],
                env_token: None,
            },
            accounts: Some(vec![Account {
                login: "alice-alt".into(),
                active: false,
            }]),
            snapshot: Snapshot {
                projects: vec![project],
                login: Some("alice".into()),
                ..Snapshot::default()
            },
            discovery: Discovery {
                classrooms: vec![Classroom {
                    org: "old-school".into(),
                    slug: "algo".into(),
                    name: None,
                    term: None,
                    active: false,
                    secret: Some("s3cr3tkey".into()),
                    pages_base_url: Some("https://pages.lycee-dupont.fr/c50".into()),
                }],
                groups: Vec::new(),
                repos: vec![("old-school".into(), "algo-tp1-élodie".into(), None)],
            },
            stats: SyncStats::default(),
            settings: Settings::default(),
            editors: vec!["IntelliJ IDEA".into()],
            log: Some(format!(
                "[INFO] cloned CS-School/cs50-hello-alice to /Users/alice/IdeaProjects/hello-clone\n\
                 [WARN] Alice Doe <carol@uni.edu> carol99 pushed; token {token}\n\
                 [INFO] Intro to CS synced for alice-alt in /private/alice/x\n\
                 [WARN] could not fetch https://pages.lycee-dupont.fr/c50/algo/s3cr3tkey/assignments.json: 404\n\
                 [WARN] could not fetch https://gone-org.github.io/classroom50/math/zz9key/assignments.json: 404\n\
                 [INFO] cloned old-school/ALGO-TP1-ÉLODIE\n\
                 [INFO] rescanning after a change in \"/Users/alice/Documents/Lettre Dupont.docx\""
            )),
            home: Some("/Users/alice".into()),
        };
        let report = report(&facts);
        let text = report.text.to_lowercase();
        for secret in [
            "alice",
            "carol",
            "cs-school",
            "cs50",
            "hello-clone",
            "intro to cs",
            "owls",
            "uni.edu",
            "ghp_",
            "/users/",
            "old-school",
            "s3cr3tkey",
            "lycee-dupont",
            "gone-org",
            "zz9key",
            "élodie",
            "dupont",
        ] {
            assert!(!text.contains(secret), "{secret} leaked:\n{}", report.text);
        }
        assert!(report.text.contains("- Signed in: yes as user-1"));
        assert!(report.text.contains("- gh: 2.81.0"));
        assert!(report.text.contains("- Missing scopes: workflow"));
        assert!(
            report
                .text
                .contains("- Version: 0.9.0 (fd6c24e), release build")
        );
        assert!(
            report
                .text
                .contains("cloned org-1/repo-1 to ~/IdeaProjects/repo-2")
        );
        assert!(report.text.contains("<token>"));
        assert!(report.text.contains("```text\n"));
        assert!(
            report
                .text
                .contains("https://host-1/<path>/class-3/key-1/assignments.json: 404")
        );
        assert!(
            report
                .text
                .contains("https://<org>.github.io/classroom50/<path>/<path>/assignments.json")
        );
        assert!(report.text.contains("\"~/Documents/<dir>\""));
        assert!(report.masked >= 9, "{}", report.masked);
    }

    #[test]
    fn keeps_the_end_of_the_log() {
        let text = "partial\nl1\r\nl2\nl3\n";
        assert_eq!(last_lines(text, 2, true), "l2\nl3");
        assert_eq!(last_lines(text, 10, true), "l1\nl2\nl3");
        assert_eq!(last_lines(text, 10, false), "partial\nl1\nl2\nl3");
        let long = "é".repeat(LOG_LINE_CHARS + 5);
        let cut = last_lines(&long, 1, false);
        assert!(cut.ends_with("... (cut)"));
        assert_eq!(cut.chars().count(), LOG_LINE_CHARS + "... (cut)".len());
    }

    #[test]
    fn fences_outlast_backticks() {
        assert_eq!(fence_for("plain"), "```");
        assert_eq!(fence_for("a ```` b `"), "`````");
    }

    #[test]
    fn reads_the_tail_of_a_log_file() {
        let dir = std::env::temp_dir().join(format!("c50tb-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        let lines: Vec<String> = (0..300).map(|n| format!("line {n}")).collect();
        std::fs::write(&path, lines.join("\n")).unwrap();
        let log = read_log(&path).unwrap();
        assert_eq!(log.lines().count(), LOG_LINES);
        assert!(log.starts_with("line 100\n"));
        assert!(log.ends_with("line 299"));
        assert_eq!(read_log(&dir.join("missing.log")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fills_a_short_log_from_the_newest_rotated_file() {
        let dir = std::env::temp_dir().join(format!("c50tb-rotated-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        let write = |name: &str, from: usize, to: usize| {
            let lines: Vec<String> = (from..to).map(|n| format!("line {n}")).collect();
            std::fs::write(dir.join(name), lines.join("\n")).unwrap();
        };
        write("app_2026-10-01_08-00-00.log", 0, 300);
        write("app_2026-10-10_08-00-00.log", 300, 600);
        write("app.log", 600, 650);
        write("other_2026-10-20_08-00-00.log", 900, 1000);
        let log = read_log(&path).unwrap();
        assert_eq!(log.lines().count(), LOG_LINES);
        assert!(log.starts_with("line 450\n"));
        assert!(log.contains("line 599\nline 600\n"));
        assert!(log.ends_with("line 649"));

        // An empty current file shows the older one's tail alone.
        std::fs::write(&path, "").unwrap();
        let log = read_log(&path).unwrap();
        assert!(log.starts_with("line 400\n") && log.ends_with("line 599"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
