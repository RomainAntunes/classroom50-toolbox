//! `classroom50-toolbox://` links: `accept/<org>/<classroom>/<assignment>[?k=<key>]` starts
//! the accept flow, `open/<owner>/<repo>` shows that project. Any web page can fire them,
//! so a link is parsed strictly here, nothing is done without the student confirming in
//! the UI, and gh only ever sees the validated parts. A link that doesn't parse is logged
//! and ignored, with no UI.
//!
//! A link waits in `DeepLinkState` until the UI takes it (`take_deep_link`), so one that
//! started the app before the webview loaded isn't lost; `DeepLinkWaiting` tells a loaded
//! UI to take it now.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::classroom50::{self, Classroom};
use crate::error::Result;
use crate::events::{DeepLinkWaiting, IpcEvent};
use crate::github::Http;
use crate::projects::{self, ProjectsState, Snapshot, Unaccepted};
use crate::student;

pub const SCHEME: &str = "classroom50-toolbox";

/// Longer than any valid link (two 39-character logins, two 100-character slugs, a key).
const MAX_LEN: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Link {
    Accept {
        org: String,
        classroom: String,
        assignment: String,
        /// The unlisted classroom's key (`?k=`).
        key: Option<String>,
    },
    Open {
        owner: String,
        repo: String,
    },
}

impl Link {
    #[cfg(test)]
    fn url(&self) -> String {
        match self {
            Self::Accept {
                org,
                classroom,
                assignment,
                key,
            } => {
                let query = key.as_ref().map(|k| format!("?k={k}")).unwrap_or_default();
                format!("{SCHEME}://accept/{org}/{classroom}/{assignment}{query}")
            }
            Self::Open { owner, repo } => format!("{SCHEME}://open/{owner}/{repo}"),
        }
    }
}

/// Reads a link, or `None` for anything that isn't exactly one of the two forms: scheme
/// and action as written, the right number of segments, GitHub names and Classroom 50
/// slugs as upstream allows them, `k` (alone) on accept only, no fragment. One trailing
/// slash is tolerated, since some browsers add it.
pub fn parse(url: &str) -> Option<Link> {
    if url.len() > MAX_LEN {
        return None;
    }
    let rest = url.strip_prefix(SCHEME)?.strip_prefix("://")?;
    if rest.contains('#') {
        return None;
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let path = path.strip_suffix('/').unwrap_or(path);
    let segments: Vec<&str> = path.split('/').collect();
    match segments.as_slice() {
        ["accept", org, classroom, assignment]
            if student::valid_login(org)
                && classroom50::valid_slug(classroom)
                && classroom50::valid_slug(assignment) =>
        {
            let key = match query {
                None => None,
                Some(query) => {
                    let key = query.strip_prefix("k=")?;
                    if !classroom50::valid_secret(key) {
                        return None;
                    }
                    Some(key.to_owned())
                }
            };
            Some(Link::Accept {
                org: (*org).to_owned(),
                classroom: (*classroom).to_owned(),
                assignment: (*assignment).to_owned(),
                key,
            })
        }
        ["open", owner, repo]
            if query.is_none() && student::valid_login(owner) && valid_repo_name(repo) =>
        {
            Some(Link::Open {
                owner: (*owner).to_owned(),
                repo: (*repo).to_owned(),
            })
        }
        _ => None,
    }
}

/// A GitHub repository name: letters, digits, `.`, `-` and `_`, at most 100, not `.` or `..`.
fn valid_repo_name(name: &str) -> bool {
    (1..=100).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// What a link leads to, as the UI acts on it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    /// Show this project: an `open` link, or an `accept` link to an assignment already
    /// accepted.
    Project { id: String, link: Link },
    /// Offer to accept. `listed` is false for an assignment outside the list (an upcoming
    /// one, a class the last sync didn't see), which `accept_assignment` then finds here.
    Accept { item: Box<Unaccepted>, listed: bool },
    /// An `accept` link to an assignment outside the list. Nothing was requested yet: the
    /// UI asks with the link's own names first, then `fetch_unlisted` reads the manifest.
    Unlisted { link: Link },
    /// Not in the list (open) or not published in that class (accept).
    Missing { link: Link },
}

/// The same link again within this is dropped (a page firing it in a loop, a double click).
const REPEAT: Duration = Duration::from_secs(3);
/// A link being handled longer than this no longer blocks others (a UI that never said
/// it was done).
const BUSY_MAX: Duration = Duration::from_secs(30 * 60);

/// Which links get through: none while the UI handles one (its dialogs are open), and not
/// the same link twice within `REPEAT`. Dropped links are not queued.
#[derive(Debug, Default)]
struct Gate {
    busy_since: Option<Instant>,
    last: Option<(Link, Instant)>,
}

impl Gate {
    fn admit(&mut self, link: &Link, now: Instant) -> bool {
        if self
            .busy_since
            .is_some_and(|at| now.duration_since(at) < BUSY_MAX)
        {
            return false;
        }
        if self
            .last
            .as_ref()
            .is_some_and(|(last, at)| last == link && now.duration_since(*at) < REPEAT)
        {
            return false;
        }
        self.last = Some((link.clone(), now));
        true
    }

    fn start(&mut self, now: Instant) {
        self.busy_since = Some(now);
    }

    fn finish(&mut self) {
        self.busy_since = None;
    }
}

#[derive(Default)]
pub struct DeepLinkState {
    gate: Mutex<Gate>,
    pending: Mutex<Option<Link>>,
    /// The `Unlisted` link the student is being asked about.
    unlisted: Mutex<Option<Link>>,
    /// Assignments resolved from a link outside the list, by unaccepted id.
    linked: Mutex<Vec<Unaccepted>>,
}

impl DeepLinkState {
    fn admit(&self, link: &Link) -> bool {
        self.gate
            .lock()
            .map(|mut g| g.admit(link, Instant::now()))
            .unwrap_or(false)
    }

    /// The UI is done with the link it took: dialogs closed, the next one may come.
    pub fn finish(&self) {
        if let Ok(mut gate) = self.gate.lock() {
            gate.finish();
        }
    }

    fn set_pending(&self, link: Link) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(link);
        }
    }

    fn take_pending(&self) -> Option<Link> {
        self.pending.lock().ok().and_then(|mut p| p.take())
    }

    fn set_unlisted(&self, link: Option<Link>) {
        if let Ok(mut unlisted) = self.unlisted.lock() {
            *unlisted = link;
        }
    }

    fn take_unlisted(&self) -> Option<Link> {
        self.unlisted.lock().ok().and_then(|mut u| u.take())
    }

    fn remember(&self, item: Unaccepted) {
        if let Ok(mut linked) = self.linked.lock() {
            linked.retain(|u| u.id != item.id);
            linked.push(item);
        }
    }

    pub fn linked(&self, id: &str) -> Option<Unaccepted> {
        let linked = self.linked.lock().ok()?;
        linked.iter().find(|u| u.id == id).cloned()
    }
}

/// A link the OS handed over (or the dev field): kept for the UI, which is shown. Invalid
/// ones are logged by length only, and dropped.
pub fn handle(app: &AppHandle, url: &str) {
    let Some(link) = parse(url) else {
        log::warn!(
            "ignored an invalid link ({} chars{})",
            url.chars().count(),
            if url.starts_with(&format!("{SCHEME}:")) {
                ""
            } else {
                ", another scheme"
            }
        );
        return;
    };
    let state = app.state::<DeepLinkState>();
    if !state.admit(&link) {
        log::info!(
            "dropped link (one is open, or a repeat): {}",
            describe(&link)
        );
        return;
    }
    log::info!("link: {}", describe(&link));
    state.set_pending(link);
    crate::popover::show(app);
    let _ = DeepLinkWaiting::emit(app, &());
}

/// The link for the log: its action only. The names it holds may be a class the
/// diagnostics report doesn't know yet, so they would survive its masking.
fn describe(link: &Link) -> &'static str {
    match link {
        Link::Accept { key: Some(_), .. } => "accept, with a key",
        Link::Accept { .. } => "accept",
        Link::Open { .. } => "open",
    }
}

/// Whether the arguments of a second launch are a link of ours, which `handle` deals with
/// (the single-instance plugin hands them to the deep-link plugin first).
pub fn is_link_launch(args: &[String]) -> bool {
    args.len() == 2 && args[1].starts_with(&format!("{SCHEME}:"))
}

/// Listens for links and picks up the one that started the app, if any.
pub fn setup(app: &AppHandle) {
    use tauri_plugin_deep_link::DeepLinkExt;

    let app_handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            handle(&app_handle, url.as_str());
        }
    });
    // Windows and Linux: a cold start gets the link as its argument, read before this ran.
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            handle(app, url.as_str());
        }
    }
    register_appimage(app);
}

/// deb and rpm declare the scheme in their `.desktop` file, but an AppImage has none
/// installed: register one at runtime (a `.desktop` file in the user's data dir pointing
/// at the AppImage, then `xdg-mime default`). Every start, since the file names the
/// AppImage's path, which moves when the user moves it; the plugin rewrites the file only
/// when it changed.
fn register_appimage(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    if app.env().appimage.is_some() {
        use tauri_plugin_deep_link::DeepLinkExt;
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(err) = app.deep_link().register(SCHEME) {
                log::warn!("could not register {SCHEME}:// links: {err}");
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}

/// Resolves what the snapshot alone can tell: the project of an `open` link or of an
/// accepted assignment, or a listed assignment to accept. `None` means an `accept` link
/// whose assignment needs its class's manifest.
fn resolve_local(snapshot: &Snapshot, link: &Link) -> Option<Target> {
    match link {
        Link::Open { owner, repo } => {
            let project = snapshot.projects.iter().find(|p| {
                p.owner
                    .as_deref()
                    .is_some_and(|o| o.eq_ignore_ascii_case(owner))
                    && p.repo
                        .as_deref()
                        .is_some_and(|r| r.eq_ignore_ascii_case(repo))
            });
            Some(match project {
                Some(p) => Target::Project {
                    id: p.id.clone(),
                    link: link.clone(),
                },
                None => Target::Missing { link: link.clone() },
            })
        }
        Link::Accept {
            org,
            classroom,
            assignment,
            ..
        } => {
            if let Some(p) = projects::assignment_project(snapshot, org, classroom, assignment) {
                return Some(Target::Project {
                    id: p.id.clone(),
                    link: link.clone(),
                });
            }
            // A link made before the assignment was renamed names its old slug.
            let id = unaccepted_id(org, classroom, assignment);
            snapshot
                .unaccepted
                .iter()
                .find(|u| u.id.eq_ignore_ascii_case(&id))
                .or_else(|| {
                    snapshot.unaccepted.iter().find(|u| {
                        u.classroom.org.eq_ignore_ascii_case(org)
                            && u.classroom.slug.eq_ignore_ascii_case(classroom)
                            && u.assignment.answers_to(assignment)
                    })
                })
                .map(|u| Target::Accept {
                    item: Box::new(u.clone()),
                    listed: true,
                })
        }
    }
}

fn unaccepted_id(org: &str, classroom: &str, assignment: &str) -> String {
    format!("{org}/{classroom}/{assignment}").to_ascii_lowercase()
}

/// The classroom an `accept` link names: the one the last sync read when it is the
/// user's (the link's key wins when it has one), else just what the link says. Always
/// read from `https://<org>.github.io/`: a synced custom Pages host isn't used, so a link
/// can only make the app ask the org's own GitHub Pages (which redirects to a custom
/// domain when the org has one).
fn link_classroom(known: Option<Classroom>, org: &str, slug: &str, key: Option<&str>) -> Classroom {
    let mut classroom = known.unwrap_or_else(|| Classroom {
        org: org.to_owned(),
        slug: slug.to_owned(),
        name: None,
        term: None,
        active: true,
        secret: None,
        pages_base_url: None,
    });
    classroom.pages_base_url = None;
    if let Some(key) = key {
        classroom.secret = Some(key.to_owned());
    }
    classroom
}

/// Takes the waiting link, if any, and works out where it leads from the list alone: no
/// request is made before the student agrees. Until the UI calls `finish`, other links
/// are dropped.
pub fn take(app: &AppHandle) -> Option<Target> {
    let state = app.state::<DeepLinkState>();
    let link = state.take_pending()?;
    if let Ok(mut gate) = state.gate.lock() {
        gate.start(Instant::now());
    }
    let projects = app.state::<ProjectsState>();
    let target = resolve_local(&projects.snapshot(), &link);
    state.set_unlisted(target.is_none().then(|| link.clone()));
    Some(target.unwrap_or(Target::Unlisted { link }))
}

/// After the student agreed to an `Unlisted` link: reads its class's published manifest
/// (public, no token) and returns the assignment to accept, which `accept_assignment`
/// then finds here, or `Missing`.
pub async fn fetch_unlisted(app: &AppHandle) -> Result<Option<Target>> {
    let Some(link) = app.state::<DeepLinkState>().take_unlisted() else {
        return Ok(None);
    };
    let Link::Accept {
        org,
        classroom: slug,
        assignment: wanted,
        key,
    } = &link
    else {
        return Ok(None);
    };
    let projects = app.state::<ProjectsState>();
    let classroom = link_classroom(
        projects.known_classroom(org, slug),
        org,
        slug,
        key.as_deref(),
    );
    let http = app.state::<Http>();
    let manifest = projects.manifest(&http.0, &classroom).await?;
    let Some(assignment) = linked_assignment(manifest, wanted) else {
        return Ok(Some(Target::Missing { link }));
    };
    let item = Unaccepted {
        id: unaccepted_id(org, slug, &assignment.slug),
        accept_url: classroom50::accept_url(
            &classroom.org,
            &classroom.slug,
            &assignment.slug,
            classroom.secret.as_deref(),
        ),
        classroom,
        assignment,
        group: None,
    };
    app.state::<DeepLinkState>().remember(item.clone());
    Ok(Some(Target::Accept {
        item: Box::new(item),
        listed: false,
    }))
}

/// The open assignment of `manifest` an accept link names: by its slug, else by the slug
/// it was renamed from (a link made before the rename). The validated slug, not the
/// manifest's spelling, is what reaches gh: the link's own, or the current one when the
/// link has the old one and the manifest's passes the same check.
fn linked_assignment(
    manifest: Vec<classroom50::Assignment>,
    wanted: &str,
) -> Option<classroom50::Assignment> {
    let mut assignment = classroom50::assignment_for_slug(wanted, &manifest)
        .filter(|a| !a.closed)?
        .clone();
    if assignment.slug.eq_ignore_ascii_case(wanted) {
        assignment.slug = wanted.to_owned();
    } else if !classroom50::valid_slug(&assignment.slug) {
        return None;
    }
    Some(assignment)
}

/// Dev builds only: handles `url` as if the OS had opened it.
pub fn simulate(app: &AppHandle, url: &str) -> Result<()> {
    if !cfg!(debug_assertions) {
        return Err(crate::error::Error::Unsupported(
            "simulated links are for development builds".to_owned(),
        ));
    }
    handle(app, url.trim());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(org: &str, classroom: &str, assignment: &str, key: Option<&str>) -> Link {
        Link::Accept {
            org: org.to_owned(),
            classroom: classroom.to_owned(),
            assignment: assignment.to_owned(),
            key: key.map(str::to_owned),
        }
    }

    #[test]
    fn reads_accept_and_open_links() {
        assert_eq!(
            parse("classroom50-toolbox://accept/HEIG-VD/26-poo-c/labo-3"),
            Some(accept("HEIG-VD", "26-poo-c", "labo-3", None))
        );
        assert_eq!(
            parse("classroom50-toolbox://accept/cs50/fall-2026/hello?k=ab12cd"),
            Some(accept("cs50", "fall-2026", "hello", Some("ab12cd")))
        );
        assert_eq!(
            parse("classroom50-toolbox://open/heigvd-poo/26-poo-c-labo-3-Alice_B.x"),
            Some(Link::Open {
                owner: "heigvd-poo".to_owned(),
                repo: "26-poo-c-labo-3-Alice_B.x".to_owned(),
            })
        );
        // Some browsers add a slash.
        assert_eq!(
            parse("classroom50-toolbox://open/cs50/hello/"),
            parse("classroom50-toolbox://open/cs50/hello")
        );
    }

    #[test]
    fn writes_links_it_reads_back() {
        for link in [
            accept("cs50", "fall-2026", "hello", None),
            accept("cs50", "fall-2026", "hello", Some("k3y9")),
            Link::Open {
                owner: "Alice".to_owned(),
                repo: "fall-2026-hello-alice".to_owned(),
            },
        ] {
            assert_eq!(parse(&link.url()), Some(link));
        }
    }

    #[test]
    fn rejects_everything_else() {
        let long = format!("classroom50-toolbox://open/cs50/{}", "a".repeat(101));
        let bad = [
            "",
            "https://classroom50.org/cs50/fall/assignments/hello/accept",
            "classroom50-toolbox:accept/cs50/fall-2026/hello",
            "classroom50-toolbox:/accept/cs50/fall-2026/hello",
            "classroom50-toolboxx://accept/cs50/fall-2026/hello",
            "other://accept/cs50/fall-2026/hello",
            // Wrong action or segment count.
            "classroom50-toolbox://submit/cs50/fall-2026/hello",
            "classroom50-toolbox://Accept/cs50/fall-2026/hello",
            "classroom50-toolbox://accept/cs50/fall-2026",
            "classroom50-toolbox://accept/cs50/fall-2026/hello/extra",
            "classroom50-toolbox://accept/cs50//hello",
            "classroom50-toolbox://open/cs50",
            "classroom50-toolbox://open/cs50/hello/extra",
            "classroom50-toolbox://open/cs50/hello//",
            // Names and slugs.
            "classroom50-toolbox://accept/-cs50/fall-2026/hello",
            "classroom50-toolbox://accept/cs50/Fall-2026/hello",
            "classroom50-toolbox://accept/cs50/fall_2026/hello",
            "classroom50-toolbox://accept/cs50/fall-2026/h",
            "classroom50-toolbox://accept/cs50/fall-2026/%2e%2e",
            "classroom50-toolbox://accept/cs50/fall-2026/--key",
            "classroom50-toolbox://open/cs50/..",
            "classroom50-toolbox://open/cs50/.",
            "classroom50-toolbox://open/cs50/a%20b",
            "classroom50-toolbox://open/cs50/a b",
            "classroom50-toolbox://open/c--s/hello",
            &long,
            // Queries and fragments.
            "classroom50-toolbox://accept/cs50/fall-2026/hello?",
            "classroom50-toolbox://accept/cs50/fall-2026/hello?k=",
            "classroom50-toolbox://accept/cs50/fall-2026/hello?k=AB12",
            "classroom50-toolbox://accept/cs50/fall-2026/hello?k=ab12&x=1",
            "classroom50-toolbox://accept/cs50/fall-2026/hello?key=ab12",
            "classroom50-toolbox://accept/cs50/fall-2026/hello#k=ab12",
            "classroom50-toolbox://open/cs50/hello?k=ab12",
            "classroom50-toolbox://open/cs50/hello#x",
        ];
        for url in bad {
            assert_eq!(parse(url), None, "{url}");
        }
        assert_eq!(parse(&"classroom50-toolbox://open/a/b".repeat(20)), None);
    }

    #[test]
    fn drops_links_while_one_is_open_and_quick_repeats() {
        let a = accept("cs50", "fall-2026", "hello", None);
        let b = Link::Open {
            owner: "cs50".to_owned(),
            repo: "hello".to_owned(),
        };
        let t0 = Instant::now();
        let at = |secs: u64| t0 + Duration::from_secs(secs);
        let mut gate = Gate::default();
        assert!(gate.admit(&a, t0));
        // The same link again: dropped within 3 s, let through after.
        assert!(!gate.admit(&a, t0 + Duration::from_millis(2900)));
        assert!(gate.admit(&a, at(4)));
        // While the UI handles one, nothing gets through, not even another link.
        gate.start(at(5));
        assert!(!gate.admit(&b, at(6)));
        assert!(!gate.admit(&b, at(60)));
        gate.finish();
        assert!(gate.admit(&b, at(61)));
        // A UI that never finished stops blocking after a while.
        gate.start(at(100));
        assert!(!gate.admit(&a, at(100) + BUSY_MAX - Duration::from_secs(1)));
        assert!(gate.admit(&a, at(100) + BUSY_MAX));
    }

    #[test]
    fn spots_link_launches() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(is_link_launch(&args(&[
            "app",
            "classroom50-toolbox://open/a/b"
        ])));
        assert!(is_link_launch(&args(&["app", "classroom50-toolbox:junk"])));
        assert!(!is_link_launch(&args(&["app"])));
        assert!(!is_link_launch(&args(&[
            "app",
            "--flag",
            "classroom50-toolbox://x"
        ])));
    }

    fn snapshot() -> Snapshot {
        let classroom = link_classroom(None, "HEIG-VD", "26-poo-c", None);
        let assignment = |slug: &str| -> classroom50::Assignment {
            serde_json::from_value(serde_json::json!({
                "slug": slug, "name": slug, "mode": "individual"
            }))
            .unwrap()
        };
        Snapshot {
            projects: vec![projects::Project {
                id: "heig-vd/26-poo-c-labo-1-alice".to_owned(),
                owner: Some("HEIG-VD".to_owned()),
                repo: Some("26-poo-c-labo-1-alice".to_owned()),
                config: classroom50::RepoConfig {
                    classroom: "26-poo-c".to_owned(),
                    assignment: "labo-1".to_owned(),
                    secret: None,
                },
                path: None,
                local: None,
                remote: None,
                classroom: None,
                assignment: None,
                pending: None,
                web_url: None,
                late_seconds: None,
                editor: None,
                group: None,
            }],
            unaccepted: vec![Unaccepted {
                id: "heig-vd/26-poo-c/labo-2".to_owned(),
                classroom,
                assignment: assignment("labo-2"),
                accept_url: String::new(),
                group: None,
            }],
            ..Snapshot::default()
        }
    }

    #[test]
    fn fetches_only_from_the_orgs_github_pages() {
        let mut custom = link_classroom(None, "CS50", "fall-2026", None);
        custom.pages_base_url = Some("https://elsewhere.example/classroom50".to_owned());
        for classroom in [
            link_classroom(None, "CS50", "fall-2026", Some("k3y9")),
            link_classroom(Some(custom), "CS50", "fall-2026", None),
        ] {
            assert!(
                classroom
                    .assignments_url()
                    .starts_with("https://cs50.github.io/classroom50/fall-2026/"),
                "{}",
                classroom.assignments_url()
            );
        }
    }

    #[test]
    fn resolves_links_against_the_list() {
        let snapshot = snapshot();
        let resolve = |url: &str| resolve_local(&snapshot, &parse(url).unwrap());
        let project = |target: Option<Target>| match target {
            Some(Target::Project { id, .. }) => Some(id),
            _ => None,
        };
        // Names compared without regard to case.
        assert_eq!(
            project(resolve(
                "classroom50-toolbox://open/heig-vd/26-POO-C-labo-1-alice"
            ))
            .as_deref(),
            Some("heig-vd/26-poo-c-labo-1-alice")
        );
        assert!(matches!(
            resolve("classroom50-toolbox://open/heig-vd/other"),
            Some(Target::Missing { .. })
        ));
        // Already accepted: the project instead.
        assert_eq!(
            project(resolve(
                "classroom50-toolbox://accept/heig-vd/26-poo-c/labo-1"
            ))
            .as_deref(),
            Some("heig-vd/26-poo-c-labo-1-alice")
        );
        assert!(matches!(
            resolve("classroom50-toolbox://accept/heig-vd/26-poo-c/labo-2"),
            Some(Target::Accept { listed: true, .. })
        ));
        // Not listed: the manifest decides.
        assert!(resolve("classroom50-toolbox://accept/heig-vd/26-poo-c/labo-3").is_none());
    }

    #[test]
    fn links_made_before_a_rename_still_lead_there() {
        let renamed = |slug: &str, old: &str| -> classroom50::Assignment {
            serde_json::from_value(serde_json::json!({
                "slug": slug, "name": slug, "mode": "individual", "renamed_from": old
            }))
            .unwrap()
        };
        let mut snapshot = snapshot();
        snapshot.projects[0].assignment = Some(renamed("labo-1", "labo-one"));
        snapshot.unaccepted[0].assignment = renamed("labo-2", "labo-two");
        let resolve = |url: &str| resolve_local(&snapshot, &parse(url).unwrap());
        assert!(matches!(
            resolve("classroom50-toolbox://accept/heig-vd/26-poo-c/labo-one"),
            Some(Target::Project { id, .. }) if id == "heig-vd/26-poo-c-labo-1-alice"
        ));
        assert!(matches!(
            resolve("classroom50-toolbox://accept/heig-vd/26-poo-c/labo-two"),
            Some(Target::Accept { item, listed: true }) if item.id == "heig-vd/26-poo-c/labo-2"
        ));
        assert!(resolve("classroom50-toolbox://accept/heig-vd/other/labo-two").is_none());

        // Unlisted: the manifest's current slug is what gh gets.
        let manifest = vec![
            renamed("ps3", "problem-set-three"),
            renamed("ps4", "ps-four"),
        ];
        let found = linked_assignment(manifest.clone(), "problem-set-three").unwrap();
        assert_eq!(found.slug, "ps3");
        assert_eq!(
            linked_assignment(manifest.clone(), "ps4").unwrap().slug,
            "ps4"
        );
        assert!(linked_assignment(manifest, "ps5").is_none());
        let mut closed = renamed("ps3", "problem-set-three");
        closed.closed = true;
        assert!(linked_assignment(vec![closed], "problem-set-three").is_none());
    }

    #[test]
    fn a_link_key_overrides_the_known_one() {
        let known = link_classroom(None, "cs50", "fall-2026", Some("abcd"));
        assert_eq!(known.secret.as_deref(), Some("abcd"));
        assert_eq!(
            known.assignments_url(),
            "https://cs50.github.io/classroom50/fall-2026/abcd/assignments.json"
        );
        let mut synced = known.clone();
        synced.name = Some("CS50".to_owned());
        synced.secret = Some("old1".to_owned());
        synced.pages_base_url = Some("https://evil.example".to_owned());
        let classroom = link_classroom(Some(synced.clone()), "cs50", "fall-2026", None);
        assert_eq!(classroom.secret.as_deref(), Some("old1"));
        assert_eq!(classroom.name.as_deref(), Some("CS50"));
        let classroom = link_classroom(Some(synced), "cs50", "fall-2026", Some("new1"));
        assert_eq!(classroom.secret.as_deref(), Some("new1"));
        assert_eq!(classroom.name.as_deref(), Some("CS50"));
    }
}
