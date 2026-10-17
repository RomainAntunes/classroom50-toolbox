//! Typed views of the Classroom 50 data contracts. Field names follow the upstream JSON
//! Schemas in `foundation50/classroom50/schemas/`; only the fields the toolbox reads are
//! typed, and unknown fields are ignored so newer upstream files still parse.

use std::path::Path;

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Marker file committed into every accepted assignment repo (`repo-config-v1`).
pub const REPO_CONFIG_FILE: &str = ".classroom50.yaml";
/// The per-org config repo created by `gh teacher init`; also the GitHub Pages path.
pub const CONFIG_REPO: &str = "classroom50";
pub const ASSIGNMENTS_SCHEMA: &str = "classroom50/assignments/v1";
pub const TEAM_SCHEMA: &str = "classroom50/team/v1";
/// Student team `classroom50-<classroom>`; staff teams add `-<role>`.
pub const TEAM_PREFIX: &str = "classroom50-";
const GROUP_TEAM_PREFIX: &str = "classroom50-group-";
const STAFF_ROLES: &[&str] = &["teacher", "hta", "ta"];
pub const SUBMIT_TAG_PREFIX: &str = "submit/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoConfig {
    pub classroom: String,
    pub assignment: String,
    /// Capability secret of an unlisted classroom; part of its Pages URL.
    #[serde(default)]
    pub secret: Option<String>,
}

impl RepoConfig {
    pub fn parse(yaml: &str) -> Result<Self> {
        Ok(serde_norway::from_str(yaml)?)
    }

    /// Reads `<dir>/.classroom50.yaml`; `Ok(None)` when the directory isn't a Classroom 50 repo.
    pub fn read(dir: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(dir.join(REPO_CONFIG_FILE)) {
            Ok(yaml) => Self::parse(&yaml).map(Some),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssignmentMode {
    Individual,
    Group,
    Team,
}

/// Who forms the groups of a `team` assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamFormation {
    Teacher,
    Student,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubmissionMode {
    EveryPush,
    Tag,
}

/// One entry of a classroom's published `assignments.json` (`assignments-v1`). Read in
/// snake_case from upstream and, through the aliases, in camelCase from `snapshot.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Assignment {
    pub slug: String,
    pub name: String,
    pub mode: AssignmentMode,
    #[serde(default)]
    pub description: Option<String>,
    /// Submissions after this are marked late; nothing is blocked.
    #[serde(default)]
    pub due: Option<DateTime<FixedOffset>>,
    #[serde(default, alias = "availableFrom")]
    pub available_from: Option<DateTime<FixedOffset>>,
    /// Absent means every push to the default branch is graded.
    #[serde(default, alias = "submissionMode")]
    pub submission_mode: Option<SubmissionMode>,
    #[serde(default)]
    pub locked: bool,
    /// The only state that actually stops submissions (repos become read-only).
    #[serde(default)]
    pub closed: bool,
    #[serde(default, alias = "feedbackPr")]
    pub feedback_pr: bool,
    #[serde(default, alias = "passThreshold")]
    pub pass_threshold: Option<u8>,
    /// `team` mode only. `gh student accept` doesn't ask, so the UI explains it first.
    #[serde(default, alias = "teamFormation")]
    pub team_formation: Option<TeamFormation>,
    /// `"public"` when accepting creates a public repo; `gh student accept` only warns.
    #[serde(default, alias = "repoVisibility")]
    pub repo_visibility: Option<String>,
    /// The slug before `gh teacher assignment rename` (one rename at most). Repos the
    /// rename skipped keep the old name and marker, and clones made before it keep an
    /// `origin` with the old name (GitHub redirects it), so both still count.
    #[serde(default, alias = "renamedFrom")]
    pub renamed_from: Option<String>,
    /// The repo each student repo is generated from; absent on `empty_repo` assignments.
    #[serde(default)]
    pub template: Option<Template>,
}

/// An assignment's starter repo (`assignments-v1` `template`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub owner: String,
    pub repo: String,
    #[serde(default)]
    pub branch: Option<String>,
}

impl Assignment {
    /// `owner/repo` of the template, as GitHub's `templateRepository` names it.
    pub fn template_name(&self) -> Option<String> {
        let template = self.template.as_ref()?;
        Some(format!("{}/{}", template.owner, template.repo))
    }

    /// The slug a rename left behind, when there was one.
    pub fn old_slug(&self) -> Option<&str> {
        self.renamed_from
            .as_deref()
            .filter(|old| !old.is_empty() && !old.eq_ignore_ascii_case(&self.slug))
    }

    /// Whether a marker naming `slug` belongs to this assignment: its slug or the one it
    /// was renamed from, without regard to case.
    pub fn answers_to(&self, slug: &str) -> bool {
        self.slug.eq_ignore_ascii_case(slug)
            || self
                .old_slug()
                .is_some_and(|old| old.eq_ignore_ascii_case(slug))
    }
}

#[derive(Debug, Deserialize)]
struct AssignmentsFile {
    schema: String,
    assignments: Vec<Assignment>,
}

/// Grading output attached to the release of each graded submission (`result-v1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct GradeResult {
    pub classroom: String,
    pub assignment: String,
    pub owner: String,
    /// The `submit/<UTC-timestamp>-<short-sha>` tag that was graded.
    pub submission: String,
    pub commit: String,
    pub release: String,
    pub review: String,
    /// The submission instant (graded commit's committer date).
    pub datetime: DateTime<Utc>,
    pub score: u32,
    #[serde(rename(deserialize = "max-score"))]
    pub max_score: u32,
    pub tests: Vec<TestResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct TestResult {
    #[serde(rename(deserialize = "test-name"))]
    pub name: String,
    pub passed: bool,
    pub score: u32,
    #[serde(rename(deserialize = "max-score"))]
    pub max_score: u32,
}

/// A classroom the student belongs to. Read from the description of its secret student
/// team (`classroom-team-v1`), which exists so members can enumerate their classrooms
/// without access to the private config repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classroom {
    pub org: String,
    pub slug: String,
    pub name: Option<String>,
    pub term: Option<String>,
    /// `false` once the teacher archived the classroom.
    pub active: bool,
    pub secret: Option<String>,
    /// Custom Pages host for orgs off `github.io`.
    pub pages_base_url: Option<String>,
}

#[derive(Deserialize)]
struct TeamRecord {
    schema: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    term: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    pages_base_url: Option<String>,
}

impl Classroom {
    /// A classroom known only from a repo's `.classroom50.yaml`.
    pub fn from_config(org: &str, config: &RepoConfig) -> Self {
        Self {
            org: org.to_owned(),
            slug: config.classroom.clone(),
            name: None,
            term: None,
            active: true,
            secret: config.secret.clone(),
            pages_base_url: None,
        }
    }

    /// Reads a team the user belongs to. `None` for group, staff and unrelated teams.
    /// Student teams created before the description record existed still count.
    pub fn from_team(org: &str, slug: &str, description: Option<&str>) -> Option<Self> {
        let short = slug.strip_prefix(TEAM_PREFIX)?;
        if slug.starts_with(GROUP_TEAM_PREFIX) || short.is_empty() {
            return None;
        }
        let record = description
            .and_then(|d| serde_json::from_str::<TeamRecord>(d).ok())
            .filter(|r| r.schema == TEAM_SCHEMA);
        if record.is_none()
            && STAFF_ROLES
                .iter()
                .any(|role| short.ends_with(&format!("-{role}")))
        {
            return None;
        }
        let record = record.unwrap_or(TeamRecord {
            schema: TEAM_SCHEMA.to_owned(),
            name: None,
            term: None,
            active: None,
            secret: None,
            pages_base_url: None,
        });
        Some(Self {
            org: org.to_owned(),
            slug: short.to_owned(),
            name: record.name.filter(|n| !n.is_empty()),
            term: record.term.filter(|t| !t.is_empty()),
            active: record.active.unwrap_or(true),
            secret: record.secret.filter(|s| !s.is_empty()),
            pages_base_url: record.pages_base_url.filter(|u| u.starts_with("https://")),
        })
    }

    /// Identity used to merge the same classroom seen through teams and repo configs.
    pub fn key(&self) -> (String, String) {
        (
            self.org.to_ascii_lowercase(),
            self.slug.to_ascii_lowercase(),
        )
    }

    /// `text` with this classroom's key, when it has one, replaced by `<key>`: for log
    /// lines that quote its Pages URL or accept link.
    pub fn hide_key(&self, text: &str) -> String {
        match self.secret.as_deref().filter(|s| !s.is_empty()) {
            Some(secret) => text.replace(secret, "<key>"),
            None => text.to_owned(),
        }
    }

    pub fn assignments_url(&self) -> String {
        assignments_url(
            &self.org,
            &self.slug,
            self.secret.as_deref(),
            self.pages_base_url.as_deref(),
        )
    }
}

/// Public, token-less location of a classroom's assignment list, as built by gh-student:
/// `<base>/<classroom>[/<secret>]/assignments.json`, where the base defaults to
/// `https://<org>.github.io/classroom50`.
pub fn assignments_url(
    org: &str,
    classroom: &str,
    secret: Option<&str>,
    pages_base_url: Option<&str>,
) -> String {
    let segment = match secret.filter(|s| !s.is_empty()) {
        Some(secret) => format!("{classroom}/{secret}"),
        None => classroom.to_owned(),
    };
    let base = match pages_base_url {
        Some(base) => base.trim_end_matches('/').to_owned(),
        None => format!(
            "https://{}.github.io/{CONFIG_REPO}",
            org.to_ascii_lowercase()
        ),
    };
    format!("{base}/{segment}/assignments.json")
}

/// Where the Classroom 50 web app runs (`web/CNAME` upstream).
pub const WEB_APP_URL: &str = "https://classroom50.org";

/// The student's page for an assignment in the Classroom 50 web app: submissions, grades
/// and feedback. Org and slugs are GitHub names, which need no escaping.
pub fn submission_url(org: &str, classroom: &str, assignment: &str) -> String {
    format!("{WEB_APP_URL}/{org}/{classroom}/assignments/{assignment}/submission")
}

/// The web app's accept page for an assignment; `k` carries an unlisted classroom's key.
pub fn accept_url(org: &str, classroom: &str, assignment: &str, secret: Option<&str>) -> String {
    let url = format!("{WEB_APP_URL}/{org}/{classroom}/assignments/{assignment}/accept");
    match secret.filter(|s| !s.is_empty()) {
        Some(secret) => format!("{url}?k={secret}"),
        None => url,
    }
}

/// How a repo name matched an assignment (`assignment_for_repo`).
#[derive(Debug, Clone)]
pub struct NameMatch<'a> {
    pub assignment: &'a Assignment,
    /// Through the slug it was renamed from: a repo the rename skipped, or the old name
    /// of a renamed one.
    pub old_slug: bool,
    /// What follows `<classroom>-<slug>-`: the username or `group-<n>`.
    pub suffix: String,
}

/// The assignment a repo belongs to by the upstream naming rule
/// `<classroom>-<assignment>-<username or group>` (all lowercase). Needed because
/// `no_autograder` and `empty_repo` assignments commit no `.classroom50.yaml`. Like
/// gh-student's markerless submit, a slug or the one it was renamed from (`renamed_from`)
/// matches, and the longest prefix wins so `lab1` doesn't claim `lab1-extra` repos; a
/// current slug wins over an old one of the same length.
pub fn assignment_for_repo<'a>(
    repo: &str,
    classroom: &str,
    assignments: &'a [Assignment],
) -> Option<NameMatch<'a>> {
    let lower = repo.to_ascii_lowercase();
    let classroom = classroom.to_ascii_lowercase();
    assignments
        .iter()
        .flat_map(|assignment| {
            let current = Some((assignment.slug.as_str(), false));
            let old = assignment.old_slug().map(|old| (old, true));
            current
                .into_iter()
                .chain(old)
                .map(move |(slug, old_slug)| (assignment, slug, old_slug))
        })
        .filter_map(|(assignment, slug, old_slug)| {
            let prefix = format!("{classroom}-{}-", slug.to_ascii_lowercase());
            (lower.len() > prefix.len() && lower.starts_with(&prefix)).then(|| {
                let rank = (prefix.len(), !old_slug);
                let found = NameMatch {
                    assignment,
                    old_slug,
                    suffix: repo[prefix.len()..].to_owned(),
                };
                (rank, found)
            })
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, found)| found)
}

/// The assignment a marker's `assignment` names: its slug, else the slug it was renamed
/// from (a marker the rename didn't rewrite, or a clone not pulled since).
pub fn assignment_for_slug<'a>(
    slug: &str,
    assignments: &'a [Assignment],
) -> Option<&'a Assignment> {
    assignments
        .iter()
        .find(|a| a.slug.eq_ignore_ascii_case(slug))
        .or_else(|| assignments.iter().find(|a| a.answers_to(slug)))
}

/// The name a renamed assignment gave repo `repo` (`gh teacher assignment rename` keeps
/// the suffix): `None` unless the name matches by the old slug.
pub fn renamed_repo_name(
    repo: &str,
    classroom: &str,
    assignments: &[Assignment],
) -> Option<String> {
    let found = assignment_for_repo(repo, classroom, assignments)?;
    found.old_slug.then(|| {
        format!(
            "{}-{}-{}",
            classroom.to_ascii_lowercase(),
            found.assignment.slug.to_ascii_lowercase(),
            found.suffix.to_ascii_lowercase()
        )
    })
}

/// The name repo `repo` had before its assignment was renamed: `None` unless it matches
/// the current slug of an assignment with a `renamed_from`.
pub fn previous_repo_name(
    repo: &str,
    classroom: &str,
    assignments: &[Assignment],
) -> Option<String> {
    let found = assignment_for_repo(repo, classroom, assignments)?;
    let old = found.assignment.old_slug().filter(|_| !found.old_slug)?;
    Some(format!(
        "{}-{}-{}",
        classroom.to_ascii_lowercase(),
        old.to_ascii_lowercase(),
        found.suffix.to_ascii_lowercase()
    ))
}

/// A classroom or assignment slug as upstream's schemas allow it: `^[a-z0-9][a-z0-9-]{1,99}$`.
pub fn valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (2..=100).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || *b == b'-')
}

/// An unlisted classroom's key as upstream's schemas allow it: `^[a-z0-9]{4,64}$`.
pub fn valid_secret(secret: &str) -> bool {
    (4..=64).contains(&secret.len())
        && secret
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

pub fn parse_assignments(url: &str, json: &str) -> Result<Vec<Assignment>> {
    let file: AssignmentsFile = serde_json::from_str(json).map_err(std::io::Error::other)?;
    if file.schema != ASSIGNMENTS_SCHEMA {
        return Err(Error::SchemaMismatch {
            url: url.to_owned(),
            found: file.schema,
            expected: ASSIGNMENTS_SCHEMA,
        });
    }
    Ok(file.assignments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_the_key_in_log_lines() {
        let mut classroom = Classroom::from_config(
            "cs-school",
            &RepoConfig {
                classroom: "cs50".into(),
                assignment: "hello".into(),
                secret: Some("s3cr3t".into()),
            },
        );
        let line = format!("could not fetch {}: 404", classroom.assignments_url());
        assert_eq!(
            classroom.hide_key(&line),
            "could not fetch https://cs-school.github.io/classroom50/cs50/<key>/assignments.json: 404"
        );
        classroom.secret = None;
        assert_eq!(classroom.hide_key("s3cr3t"), "s3cr3t");
    }

    #[test]
    fn checks_slugs_and_secrets_like_the_schemas() {
        assert!(valid_slug("cs50-fall-2026"));
        assert!(valid_slug("26-poo-c"));
        assert!(!valid_slug("a"));
        assert!(!valid_slug("-lab"));
        assert!(!valid_slug("Lab1"));
        assert!(!valid_slug("lab_1"));
        assert!(!valid_slug("lab%2f1"));
        assert!(valid_slug(&"a".repeat(100)));
        assert!(!valid_slug(&"a".repeat(101)));
        assert!(valid_secret("k3y9"));
        assert!(!valid_secret("abc"));
        assert!(!valid_secret("ABCD"));
        assert!(!valid_secret("ab-cd"));
        assert!(!valid_secret(&"a".repeat(65)));
    }

    #[test]
    fn parses_repo_config_and_ignores_unknown_keys() {
        let yaml = "schema: classroom50/repo-config/v1\nclassroom: cs50-fall-2026\nassignment: hello\nowner:\n  username: alice\n  id: 42\n";
        let config = RepoConfig::parse(yaml).unwrap();
        assert_eq!(config.classroom, "cs50-fall-2026");
        assert_eq!(config.assignment, "hello");
        assert_eq!(config.secret, None);
    }

    #[test]
    fn repo_config_requires_identity() {
        assert!(RepoConfig::parse("classroom: x\n").is_err());
    }

    #[test]
    fn builds_pages_urls() {
        assert_eq!(
            assignments_url("CS50", "fall-2026", None, None),
            "https://cs50.github.io/classroom50/fall-2026/assignments.json"
        );
        assert_eq!(
            assignments_url("cs50", "fall-2026", Some("ab12"), None),
            "https://cs50.github.io/classroom50/fall-2026/ab12/assignments.json"
        );
        assert_eq!(
            assignments_url("cs50", "fall", None, Some("https://pages.cs50.edu/c50/")),
            "https://pages.cs50.edu/c50/fall/assignments.json"
        );
    }

    #[test]
    fn builds_submission_urls() {
        assert_eq!(
            submission_url("HEIGVD-202609-BDR-A", "bdr-a", "lab2"),
            "https://classroom50.org/HEIGVD-202609-BDR-A/bdr-a/assignments/lab2/submission"
        );
    }

    #[test]
    fn reads_classroom_teams() {
        let poo = Classroom::from_team(
            "HEIGVD-POO",
            "classroom50-26-poo-c",
            Some(r#"{"schema":"classroom50/team/v1","name":"2026-POO-C","term":"2026-2027-S1"}"#),
        )
        .unwrap();
        assert_eq!(poo.slug, "26-poo-c");
        assert_eq!(poo.name.as_deref(), Some("2026-POO-C"));
        assert!(poo.active && poo.secret.is_none());

        let unlisted = Classroom::from_team(
            "o",
            "classroom50-x",
            Some(r#"{"schema":"classroom50/team/v1","active":false,"secret":"abcd1234"}"#),
        )
        .unwrap();
        assert!(!unlisted.active);
        assert_eq!(unlisted.secret.as_deref(), Some("abcd1234"));

        // A pre-record student team still counts; group and staff teams don't.
        assert!(Classroom::from_team("o", "classroom50-bdr-a", None).is_some());
        assert!(Classroom::from_team("o", "classroom50-group-3eaa7f60afc3ad98-3", None).is_none());
        assert!(Classroom::from_team("o", "classroom50-26-poo-c-ta", None).is_none());
        assert!(Classroom::from_team("o", "developers", None).is_none());
    }

    fn assignment(slug: &str, renamed_from: Option<&str>) -> Assignment {
        serde_json::from_value(serde_json::json!({
            "slug": slug, "name": slug, "mode": "individual", "renamed_from": renamed_from
        }))
        .unwrap()
    }

    #[test]
    fn matches_repos_by_name() {
        let list: Vec<Assignment> = ["lab01", "labo-3-hogwarts", "lab1", "lab1-extra"]
            .into_iter()
            .map(|slug| assignment(slug, None))
            .collect();
        let find = |repo: &str, classroom: &str| {
            assignment_for_repo(repo, classroom, &list).map(|m| m.assignment.slug.clone())
        };
        assert_eq!(
            find("26-poo-c-labo-3-hogwarts-group-5", "26-poo-c").as_deref(),
            Some("labo-3-hogwarts")
        );
        assert_eq!(
            find("26-POO-C-lab01-alice", "26-poo-c").as_deref(),
            Some("lab01")
        );
        assert_eq!(
            find("fall-lab1-extra-bob", "fall").as_deref(),
            Some("lab1-extra")
        );
        assert_eq!(find("fall-lab1-bob", "fall").as_deref(), Some("lab1"));
        assert_eq!(find("26-poo-c-lab01-", "26-poo-c"), None);
        assert_eq!(find("hogwarts", "26-poo-c"), None);
        let found = assignment_for_repo("fall-lab1-Bob", "fall", &list).unwrap();
        assert_eq!((found.suffix.as_str(), found.old_slug), ("Bob", false));
    }

    #[test]
    fn matches_renamed_assignments_by_their_old_slug() {
        let list = vec![
            assignment("ps3", Some("problem-set-three-with-a-long-name")),
            assignment("hw", None),
            assignment("hw-extra", Some("hw-x")),
        ];
        let find = |repo: &str| {
            assignment_for_repo(repo, "cs", &list).map(|m| (m.assignment.slug.as_str(), m.old_slug))
        };
        // The new name, and a straggler the rename skipped.
        assert_eq!(find("cs-ps3-alice"), Some(("ps3", false)));
        assert_eq!(
            find("cs-problem-set-three-with-a-long-name-alice"),
            Some(("ps3", true))
        );
        // The longest prefix wins, old slug or not: `hw-x` beats `hw`.
        assert_eq!(find("cs-hw-x-bob"), Some(("hw-extra", true)));
        assert_eq!(find("cs-hw-bob"), Some(("hw", false)));
        assert_eq!(find("cs-hw-extra-bob"), Some(("hw-extra", false)));
        // A tie between a current slug and an old one: the current one wins.
        let tie = vec![assignment("lab", Some("old")), assignment("old", None)];
        let found = assignment_for_repo("cs-old-ada", "cs", &tie).unwrap();
        assert_eq!(
            (found.assignment.slug.as_str(), found.old_slug),
            ("old", false)
        );
        let tie = vec![assignment("old", None), assignment("lab", Some("old"))];
        let found = assignment_for_repo("cs-old-ada", "cs", &tie).unwrap();
        assert_eq!(found.assignment.slug, "old");
        // A `renamed_from` equal to the slug, or empty, is no second name.
        let same = [assignment("lab", Some("LAB")), assignment("x", Some(""))];
        assert!(same.iter().all(|a| a.old_slug().is_none()));
    }

    #[test]
    fn markers_may_name_the_old_slug() {
        let list = vec![assignment("ps3", Some("ps-three")), assignment("ps4", None)];
        let slug = |marker: &str| assignment_for_slug(marker, &list).map(|a| a.slug.as_str());
        assert_eq!(slug("ps3"), Some("ps3"));
        assert_eq!(slug("PS-Three"), Some("ps3"));
        assert_eq!(slug("ps4"), Some("ps4"));
        assert_eq!(slug("ps5"), None);
        // The current slug wins over another assignment's old one.
        let tie = vec![assignment("lab", Some("old")), assignment("old", None)];
        assert_eq!(
            assignment_for_slug("old", &tie).map(|a| a.slug.as_str()),
            Some("old")
        );
    }

    #[test]
    fn maps_names_across_a_rename() {
        let list = vec![
            assignment("ps3", Some("problem-set-three")),
            assignment("ps4", None),
        ];
        assert_eq!(
            renamed_repo_name("CS-problem-set-three-Alice", "cs", &list).as_deref(),
            Some("cs-ps3-alice")
        );
        assert_eq!(
            renamed_repo_name("cs-problem-set-three-group-2", "cs", &list).as_deref(),
            Some("cs-ps3-group-2")
        );
        assert_eq!(renamed_repo_name("cs-ps3-alice", "cs", &list), None);
        assert_eq!(renamed_repo_name("cs-ps4-alice", "cs", &list), None);
        assert_eq!(
            previous_repo_name("cs-ps3-alice", "cs", &list).as_deref(),
            Some("cs-problem-set-three-alice")
        );
        assert_eq!(
            previous_repo_name("cs-problem-set-three-alice", "cs", &list),
            None
        );
        assert_eq!(previous_repo_name("cs-ps4-alice", "cs", &list), None);
    }

    #[test]
    fn parses_assignments_manifest() {
        let json = r#"{
            "schema": "classroom50/assignments/v1",
            "assignments": [
                {"slug": "hello", "name": "Hello", "mode": "individual", "autograder": "default",
                 "due": "2026-10-20T23:59:00-04:00", "submission_mode": "tag", "closed": false,
                 "some_future_field": 1},
                {"slug": "mario", "name": "Mario", "mode": "team", "autograder": "default", "max_group_size": 3}
            ]
        }"#;
        let assignments = parse_assignments("u", json).unwrap();
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].submission_mode, Some(SubmissionMode::Tag));
        assert_eq!(
            assignments[0].due.unwrap().with_timezone(&Utc).to_rfc3339(),
            "2026-10-21T03:59:00+00:00"
        );
        assert_eq!(assignments[1].mode, AssignmentMode::Team);
        assert_eq!(assignments[1].due, None);
    }

    #[test]
    fn rejects_unknown_manifest_schema() {
        let json = r#"{"schema": "classroom50/assignments/v2", "assignments": []}"#;
        assert!(matches!(
            parse_assignments("u", json),
            Err(Error::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn parses_grade_result() {
        let json = r#"{
            "schema": "classroom50/result/v1", "classroom": "fall-2026", "assignment": "hello",
            "assignment_type": "individual", "owner": "alice",
            "submission": "submit/2026-10-14T12-00-00Z-abc1234",
            "commit": "https://github.com/cs50/x/commit/abc", "release": "https://github.com/cs50/x/releases/tag/t",
            "review": "https://github.com/cs50/x/compare/a...b", "datetime": "2026-10-14T12:00:00Z",
            "score": 3, "max-score": 4,
            "tests": [{"test-name": "compiles", "passed": true, "score": 1, "max-score": 1, "output": "ok"}]
        }"#;
        let result: GradeResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.max_score, 4);
        assert_eq!(result.tests[0].name, "compiles");
        assert!(result.submission.starts_with(SUBMIT_TAG_PREFIX));
    }

    #[test]
    fn assignments_survive_the_saved_snapshot() {
        let upstream: Assignment = serde_json::from_value(serde_json::json!({
            "slug": "lab", "name": "Lab", "mode": "team",
            "available_from": "2026-10-01T08:00:00Z", "submission_mode": "tag",
            "feedback_pr": true, "team_formation": "student", "repo_visibility": "public"
        }))
        .unwrap();
        let saved = serde_json::to_value(&upstream).unwrap();
        assert!(saved.get("feedbackPr").is_some());
        let restored: Assignment = serde_json::from_value(saved).unwrap();
        assert!(restored.feedback_pr);
        assert_eq!(restored.submission_mode, Some(SubmissionMode::Tag));
        assert_eq!(restored.team_formation, Some(TeamFormation::Student));
        assert_eq!(restored.available_from, upstream.available_from);
        assert_eq!(restored.repo_visibility.as_deref(), Some("public"));
    }

    /// The mock's manifests (`testenv::golden`) read like published ones.
    #[test]
    fn reads_the_suites_mock_manifests() {
        let algo = parse_assignments("algo", &crate::testenv::golden("manifest-algo")).unwrap();
        let slugs: Vec<&str> = algo.iter().map(|a| a.slug.as_str()).collect();
        assert_eq!(slugs, ["hello", "sorting", "graphs"]);
        assert_eq!(algo[0].submission_mode, Some(SubmissionMode::Tag));
        assert!(algo.iter().all(|a| a.due.is_some()));
        let web = parse_assignments("web", &crate::testenv::golden("manifest-web")).unwrap();
        assert_eq!(
            assignment_for_repo("web-portfolio-ada-e2e", "web", &web)
                .map(|m| m.assignment.slug.as_str()),
            Some("portfolio")
        );
        assert_eq!(web[0].old_slug(), Some("website"));
        assert_eq!(
            renamed_repo_name("web-website-ada-e2e", "web", &web).as_deref(),
            Some("web-portfolio-ada-e2e")
        );
    }
}
