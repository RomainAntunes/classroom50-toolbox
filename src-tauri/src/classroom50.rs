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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubmissionMode {
    EveryPush,
    Tag,
}

/// One entry of a classroom's published `assignments.json` (`assignments-v1`).
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
    #[serde(default)]
    pub available_from: Option<DateTime<FixedOffset>>,
    /// Absent means every push to the default branch is graded.
    #[serde(default)]
    pub submission_mode: Option<SubmissionMode>,
    #[serde(default)]
    pub locked: bool,
    /// The only state that actually stops submissions (repos become read-only).
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub feedback_pr: bool,
    #[serde(default)]
    pub pass_threshold: Option<u8>,
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

/// Public, token-less location of a classroom's assignment list, as built by gh-student:
/// `https://<org>.github.io/classroom50/<classroom>[/<secret>]/assignments.json`.
pub fn assignments_url(org: &str, classroom: &str, secret: Option<&str>) -> String {
    let segment = match secret.filter(|s| !s.is_empty()) {
        Some(secret) => format!("{classroom}/{secret}"),
        None => classroom.to_owned(),
    };
    format!(
        "https://{}.github.io/{CONFIG_REPO}/{segment}/assignments.json",
        org.to_ascii_lowercase()
    )
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
            assignments_url("CS50", "fall-2026", None),
            "https://cs50.github.io/classroom50/fall-2026/assignments.json"
        );
        assert_eq!(
            assignments_url("cs50", "fall-2026", Some("ab12")),
            "https://cs50.github.io/classroom50/fall-2026/ab12/assignments.json"
        );
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
}
