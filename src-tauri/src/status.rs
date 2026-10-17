//! What GitHub says about an assignment repo beyond discovery: branches holding work that
//! isn't on the default branch, the newest grade (`result.json` on the `submit/*` release),
//! whether the head commit is being graded, and the teacher's activity on the Feedback PR. Read only for assignment repos, by a
//! cheap query that also runs between full syncs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::classroom50::GradeResult;
use crate::error::{Error, Result};
use crate::failures::{self, ReleaseNotes};

/// Extends the discovery fragment (`Repo`). `compare(headRef: "HEAD")` is taken from each
/// branch, so `behindBy` counts the branch's commits missing from the default branch.
pub const FRAGMENT: &str = r#"
fragment Status on Repository {
  ...Repo
  branches: refs(refPrefix: "refs/heads/", first: 20) {
    nodes { name compare(headRef: "HEAD") { behindBy } }
  }
  releases(first: 10, orderBy: {field: CREATED_AT, direction: DESC}) {
    nodes {
      tagName isDraft publishedAt url
      result: releaseAssets(first: 1, name: "result.json") { nodes { url } }
    }
  }
  grading: defaultBranchRef {
    target {
      ... on Commit {
        autograde: status { context(name: "classroom50/autograde") { state description targetUrl } }
        checkSuites(last: 5, filterBy: {appId: 15368}) {
          nodes { status conclusion workflowRun { url workflow { name } } }
        }
      }
    }
  }
  feedback: pullRequests(baseRefName: "feedback", first: 1,
                         orderBy: {field: CREATED_AT, direction: DESC}) {
    nodes {
      url
      comments(last: 50) { nodes { author { login } createdAt } }
      reviews(last: 50) { nodes { author { login } submittedAt } }
    }
  }
}"#;

/// The Feedback PR's base branch (`contract.FeedbackBaseBranch` upstream). It only ever
/// trails the default branch, so it never counts as unmerged work.
const FEEDBACK_BRANCH: &str = "feedback";
const SUBMIT_PREFIX: &str = "submit/";
/// The workflow every autograded repo runs (`.github/workflows/autograde.yaml`, a shim that
/// calls the teacher's runner). It reports through the `classroom50/autograde` commit status.
const AUTOGRADE_WORKFLOW: &str = "Autograde";
const STATUS_PREFIX: &str = "classroom50 autograde: ";

/// A branch with commits that aren't on the default branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub commits: u32,
}

/// The newest autograder result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grade {
    /// The `submit/*` tag of the graded release; a new tag means a new grade.
    pub tag: String,
    pub score: u32,
    pub max_score: u32,
    pub tests_passed: u32,
    pub tests_total: u32,
    /// Each test, in the autograder's order. Why one failed is only in the release body
    /// (`url`); `result.json` doesn't carry it, so the body is read once per tag
    /// (`apply_release_body`).
    #[serde(default)]
    pub tests: Vec<GradeTest>,
    pub published_at: Option<DateTime<Utc>>,
    pub url: String,
    /// What the release body says beyond each test's output; `None` until the body was
    /// read, which only happens when a test failed.
    #[serde(default)]
    pub release_notes: Option<ReleaseNotes>,
}

impl Grade {
    /// Grades cached before the test list was kept have none: download them again.
    pub fn has_tests(&self) -> bool {
        self.tests.len() == self.tests_total as usize
    }

    /// A test failed and the release body, which says why, hasn't been read yet. Grades
    /// cached before the body was read have no `release_notes` and are read once too.
    pub fn needs_release_notes(&self) -> bool {
        self.release_notes.is_none() && self.tests_passed < self.tests_total
    }

    /// Hands each failed test its output from the release body (`None`: the release or its
    /// body is gone), matched by name in order, so two tests of the same name each get
    /// their own.
    pub fn apply_release_body(&mut self, body: Option<&str>) {
        let parsed = failures::parse(body.unwrap_or_default());
        let mut outputs = failures::cap_outputs(parsed.tests);
        for test in self.tests.iter_mut().filter(|t| !t.passed) {
            let Some(i) = outputs.iter().position(|(t, _)| t.name == test.name) else {
                continue;
            };
            let (found, clipped) = outputs.remove(i);
            test.output_clipped = clipped;
            test.output = (!found.output.is_empty()).then_some(found.output);
            test.timed_out_after = found.timed_out_after;
        }
        let notes =
            (!parsed.notes.is_empty()).then(|| failures::clip(&parsed.notes, failures::NOTES_CAP));
        self.release_notes = Some(ReleaseNotes {
            notes_clipped: notes.as_ref().is_some_and(|n| n.clipped),
            notes: notes.map(|n| n.text),
        });
    }
}

/// A past grade, for the history: the score without the test list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeSummary {
    pub tag: String,
    pub score: u32,
    pub max_score: u32,
    pub tests_passed: u32,
    pub tests_total: u32,
    pub published_at: Option<DateTime<Utc>>,
    pub url: String,
}

impl From<&Grade> for GradeSummary {
    fn from(grade: &Grade) -> Self {
        Self {
            tag: grade.tag.clone(),
            score: grade.score,
            max_score: grade.max_score,
            tests_passed: grade.tests_passed,
            tests_total: grade.tests_total,
            published_at: grade.published_at,
            url: grade.url.clone(),
        }
    }
}

/// One test of a grade. Its own type, camelCase both ways, so the saved snapshot reads
/// back (`TestResult` reads `result.json`'s kebab-case names).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeTest {
    pub name: String,
    pub passed: bool,
    pub score: u32,
    pub max_score: u32,
    /// Why it failed, from the release body: the runner's captured output, capped
    /// (`output_clipped`: the release has more).
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub output_clipped: bool,
    /// The runner stopped it at its time limit, in seconds (the release body's `timed out
    /// after <N>s` line, not repeated in `output`). `None` in grades read before.
    #[serde(default)]
    pub timed_out_after: Option<u32>,
}

/// The autograder's run on the head commit, while it says something the grade doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Grading {
    /// Queued or running; the grade is published at the end.
    Running { url: Option<String> },
    /// Grading itself broke (failing tests are a grade, not this): the runner reported an
    /// error, or the workflow failed before reporting anything.
    Failed {
        url: Option<String>,
        message: Option<String>,
    },
}

/// Teacher activity on the Feedback PR: comments and reviews not written by the student.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub url: String,
    pub comments: u32,
    pub latest_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StatusFields {
    branches: Option<Nodes<BranchNode>>,
    releases: Option<Nodes<ReleaseNode>>,
    grading: Option<GradingRef>,
    feedback: Option<Nodes<PullNode>>,
}

#[derive(Debug, Deserialize)]
struct Nodes<T> {
    nodes: Vec<Option<T>>,
}

impl<T> Nodes<T> {
    fn into_iter(self) -> impl Iterator<Item = T> {
        self.nodes.into_iter().flatten()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchNode {
    name: String,
    compare: Option<Comparison>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Comparison {
    behind_by: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseNode {
    tag_name: String,
    is_draft: bool,
    published_at: Option<DateTime<Utc>>,
    url: String,
    result: Option<Nodes<AssetNode>>,
}

#[derive(Debug, Deserialize)]
struct AssetNode {
    /// A pre-signed download URL, valid for about an hour; needs no token.
    url: String,
}

#[derive(Debug, Deserialize)]
struct GradingRef {
    target: Option<GradingCommit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GradingCommit {
    autograde: Option<CommitStatus>,
    check_suites: Option<Nodes<CheckSuite>>,
}

#[derive(Debug, Deserialize)]
struct CommitStatus {
    context: Option<StatusContext>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusContext {
    state: String,
    description: Option<String>,
    target_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckSuite {
    status: String,
    conclusion: Option<String>,
    workflow_run: Option<WorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    url: String,
    workflow: Workflow,
}

#[derive(Debug, Deserialize)]
struct Workflow {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullNode {
    url: String,
    comments: Nodes<Activity>,
    reviews: Nodes<Activity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Activity {
    author: Option<Author>,
    #[serde(alias = "submittedAt")]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct Author {
    login: String,
}

/// The newest graded release, still to be downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradedRelease {
    pub tag: String,
    pub published_at: Option<DateTime<Utc>>,
    pub url: String,
    pub result_url: String,
}

impl StatusFields {
    pub fn unmerged(&self, default_branch: Option<&str>) -> Vec<Branch> {
        let Some(branches) = &self.branches else {
            return Vec::new();
        };
        branches
            .nodes
            .iter()
            .flatten()
            .filter(|b| Some(b.name.as_str()) != default_branch && b.name != FEEDBACK_BRANCH)
            .filter_map(|b| {
                let commits = b.compare.as_ref()?.behind_by;
                (commits > 0).then(|| Branch {
                    name: b.name.clone(),
                    commits,
                })
            })
            .collect()
    }

    /// The published `submit/*` releases carrying a `result.json`, newest first.
    pub fn graded_releases(&mut self) -> Vec<GradedRelease> {
        self.releases
            .take()
            .into_iter()
            .flat_map(Nodes::into_iter)
            .filter(|r| !r.is_draft && r.tag_name.starts_with(SUBMIT_PREFIX))
            .filter_map(|r| {
                let result_url = r.result?.into_iter().next()?.url;
                Some(GradedRelease {
                    tag: r.tag_name,
                    published_at: r.published_at,
                    url: r.url,
                    result_url,
                })
            })
            .collect()
    }

    /// The Autograde run on the head commit, when it's still going or broke. The runner
    /// posts its commit status before publishing the release, so the run being done is
    /// what says the grade is in.
    pub fn grading(&mut self) -> Option<Grading> {
        let commit = self.grading.take()?.target?;
        let run = commit.check_suites.and_then(|suites| {
            suites
                .into_iter()
                .filter(|s| {
                    s.workflow_run
                        .as_ref()
                        .is_some_and(|r| r.workflow.name == AUTOGRADE_WORKFLOW)
                })
                .last()
        });
        let run_url = |suite: &CheckSuite| suite.workflow_run.as_ref().map(|r| r.url.clone());
        if let Some(suite) = run.as_ref().filter(|s| s.status != "COMPLETED") {
            return Some(Grading::Running {
                url: run_url(suite),
            });
        }
        if let Some(context) = commit.autograde.and_then(|s| s.context) {
            return (context.state == "ERROR").then(|| Grading::Failed {
                url: context.target_url,
                message: context
                    .description
                    .map(|d| d.strip_prefix(STATUS_PREFIX).unwrap_or(&d).to_owned()),
            });
        }
        let suite = run?;
        matches!(
            suite.conclusion.as_deref(),
            Some("FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE")
        )
        .then(|| Grading::Failed {
            url: run_url(&suite),
            message: None,
        })
    }

    /// Comments and reviews on the Feedback PR by anyone but `login` (the student).
    pub fn feedback(&mut self, login: &str) -> Option<Feedback> {
        let pull = self.feedback.take()?.into_iter().next()?;
        let mut feedback = Feedback {
            url: pull.url,
            comments: 0,
            latest_at: None,
        };
        let others = pull
            .comments
            .into_iter()
            .chain(pull.reviews.into_iter())
            .filter(|a| {
                a.author
                    .as_ref()
                    .is_some_and(|au| !au.login.eq_ignore_ascii_case(login))
            });
        for activity in others {
            feedback.comments += 1;
            feedback.latest_at = feedback.latest_at.max(activity.created_at);
        }
        Some(feedback)
    }
}

/// Downloads and reads `result.json`. The signed URL carries its own authorization, so no
/// token is sent.
pub async fn download_grade(http: &reqwest::Client, release: &GradedRelease) -> Result<Grade> {
    crate::simulate::check()?;
    let response = http
        .get(&release.result_url)
        .send()
        .await
        .map_err(Error::network)?;
    if !response.status().is_success() {
        return Err(Error::GitHub(format!(
            "result.json of {}: HTTP {}",
            release.tag,
            response.status()
        )));
    }
    let result: GradeResult = serde_json::from_slice(&response.bytes().await?)
        .map_err(|err| Error::GitHub(format!("result.json of {}: {err}", release.tag)))?;
    Ok(grade_from(release, &result))
}

fn grade_from(release: &GradedRelease, result: &GradeResult) -> Grade {
    Grade {
        tag: release.tag.clone(),
        score: result.score,
        max_score: result.max_score,
        tests_passed: result.tests.iter().filter(|t| t.passed).count() as u32,
        tests_total: result.tests.len() as u32,
        tests: result
            .tests
            .iter()
            .map(|t| GradeTest {
                name: t.name.clone(),
                passed: t.passed,
                score: t.score,
                max_score: t.max_score,
                output: None,
                output_clipped: false,
                timed_out_after: None,
            })
            .collect(),
        published_at: release.published_at,
        url: release.url.clone(),
        release_notes: None,
    }
}

/// A release's body by tag, read once per graded tag with a failed test. Not part of
/// `FRAGMENT`, which runs for ten releases on every status check.
const RELEASE_BODY: &str = r#"
query($owner: String!, $name: String!, $tag: String!) {
  repository(owner: $owner, name: $name) { release(tagName: $tag) { description } }
}"#;

#[derive(Deserialize)]
struct BodyData {
    repository: Option<BodyRepo>,
}

#[derive(Deserialize)]
struct BodyRepo {
    release: Option<BodyRelease>,
}

#[derive(Deserialize)]
struct BodyRelease {
    description: Option<String>,
}

/// The body of `owner/name`'s release `tag`; `None` when the release or its body is gone.
pub async fn release_body(
    http: &reqwest::Client,
    token: &str,
    owner: &str,
    name: &str,
    tag: &str,
) -> Result<Option<String>> {
    let data: BodyData = crate::github::graphql(
        http,
        token,
        RELEASE_BODY,
        serde_json::json!({ "owner": owner, "name": name, "tag": tag }),
    )
    .await?;
    Ok(data
        .repository
        .and_then(|r| r.release)
        .and_then(|r| r.description))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = r#"{
      "branches": {"nodes": [
        {"name": "main", "compare": {"behindBy": 0}},
        {"name": "feedback", "compare": {"behindBy": 0}},
        {"name": "exercise-2", "compare": {"behindBy": 3}},
        {"name": "old", "compare": {"behindBy": 0}}
      ]},
      "releases": {"nodes": [
        {"tagName": "v1", "isDraft": false, "publishedAt": null, "url": "u0",
         "result": {"nodes": []}},
        {"tagName": "submit/2026-10-14T12-00-05Z-abc1234", "isDraft": true,
         "publishedAt": null, "url": "u1", "result": {"nodes": [{"url": "r1"}]}},
        {"tagName": "submit/2026-10-13T12-00-05Z-def5678", "isDraft": false,
         "publishedAt": "2026-10-13T12:01:00Z", "url": "u2",
         "result": {"nodes": [{"url": "r2"}]}},
        {"tagName": "submit/2026-10-12T09-00-00Z-0123abc", "isDraft": false,
         "publishedAt": "2026-10-12T09:01:00Z", "url": "u3",
         "result": {"nodes": [{"url": "r3"}]}}
      ]},
      "feedback": {"nodes": [{
        "url": "https://github.com/o/r/pull/1",
        "comments": {"nodes": [
          {"author": {"login": "teacher"}, "createdAt": "2026-10-14T08:00:00Z"},
          {"author": {"login": "Alice"}, "createdAt": "2026-10-14T09:00:00Z"}
        ]},
        "reviews": {"nodes": [
          {"author": {"login": "ta"}, "submittedAt": "2026-10-14T10:00:00Z"}
        ]}
      }]}
    }"#;

    #[test]
    fn reads_branches_grades_and_feedback() {
        let mut fields: StatusFields = serde_json::from_str(NODE).unwrap();
        assert_eq!(
            fields.unmerged(Some("main")),
            vec![Branch {
                name: "exercise-2".into(),
                commits: 3
            }]
        );
        // Drafts and non-submit releases aren't grades; newest first.
        let releases = fields.graded_releases();
        let urls: Vec<&str> = releases.iter().map(|r| r.result_url.as_str()).collect();
        assert_eq!(urls, ["r2", "r3"]);
        assert_eq!(releases[0].tag, "submit/2026-10-13T12-00-05Z-def5678");

        let feedback = fields.feedback("alice").unwrap();
        assert_eq!(feedback.comments, 2);
        assert_eq!(
            feedback.latest_at,
            Some("2026-10-14T10:00:00Z".parse().unwrap())
        );
    }

    fn grading(json: &str) -> Option<Grading> {
        let mut fields: StatusFields =
            serde_json::from_str(&format!(r#"{{"grading": {{"target": {json}}}}}"#)).unwrap();
        fields.grading()
    }

    fn suite(name: &str, status: &str, conclusion: &str) -> String {
        format!(
            r#"{{"status": "{status}", "conclusion": {conclusion},
                "workflowRun": {{"url": "run-{name}", "workflow": {{"name": "{name}"}}}}}}"#
        )
    }

    #[test]
    fn reads_the_autograde_run() {
        let running = suite("Autograde", "IN_PROGRESS", "null");
        let pages = suite("pages-build-deployment", "COMPLETED", r#""SUCCESS""#);
        assert_eq!(
            grading(&format!(
                r#"{{"autograde": null, "checkSuites": {{"nodes": [{running}, {pages}]}}}}"#
            )),
            Some(Grading::Running {
                url: Some("run-Autograde".into())
            })
        );

        // Graded, tests failing or not: the grade says it all.
        let done = suite("Autograde", "COMPLETED", r#""SUCCESS""#);
        let failing = format!(
            r#"{{"autograde": {{"context": {{"state": "FAILURE",
                "description": "classroom50 autograde: 3/10 (1/4 tests passed)",
                "targetUrl": "t"}}}}, "checkSuites": {{"nodes": [{done}]}}}}"#
        );
        assert_eq!(grading(&failing), None);

        let error = format!(
            r#"{{"autograde": {{"context": {{"state": "ERROR",
                "description": "classroom50 autograde: grading error: timed out",
                "targetUrl": "t"}}}}, "checkSuites": {{"nodes": [{done}]}}}}"#
        );
        assert_eq!(
            grading(&error),
            Some(Grading::Failed {
                url: Some("t".into()),
                message: Some("grading error: timed out".into())
            })
        );

        // The workflow broke before posting a status.
        let crashed = suite("Autograde", "COMPLETED", r#""STARTUP_FAILURE""#);
        assert_eq!(
            grading(&format!(
                r#"{{"autograde": null, "checkSuites": {{"nodes": [{crashed}]}}}}"#
            )),
            Some(Grading::Failed {
                url: Some("run-Autograde".into()),
                message: None
            })
        );

        // No autograder: nothing to say.
        assert_eq!(
            grading(r#"{"autograde": null, "checkSuites": {"nodes": []}}"#),
            None
        );
        assert_eq!(grading("{}"), None);
    }

    #[test]
    fn grades_from_result_json() {
        let result: GradeResult = serde_json::from_str(
            r#"{"schema": "classroom50/result/v1", "classroom": "c", "assignment": "a",
                "assignment_type": "individual", "owner": "alice",
                "submission": "submit/x", "commit": "c", "release": "r", "review": "v",
                "datetime": "2026-10-13T12:00:00Z", "score": 7, "max-score": 10,
                "tests": [
                  {"test-name": "t1", "passed": true, "score": 7, "max-score": 7},
                  {"test-name": "t2", "passed": false, "score": 0, "max-score": 3}
                ]}"#,
        )
        .unwrap();
        let release = GradedRelease {
            tag: "submit/x".into(),
            published_at: None,
            url: "u".into(),
            result_url: "r".into(),
        };
        let grade = grade_from(&release, &result);
        assert_eq!((grade.score, grade.max_score), (7, 10));
        assert_eq!((grade.tests_passed, grade.tests_total), (1, 2));
        assert_eq!(
            grade.tests[1],
            GradeTest {
                name: "t2".into(),
                passed: false,
                score: 0,
                max_score: 3,
                output: None,
                output_clipped: false,
                timed_out_after: None,
            }
        );
        assert!(grade.has_tests());
        assert!(grade.needs_release_notes());

        // The saved snapshot reads back.
        let saved: Grade = serde_json::from_str(&serde_json::to_string(&grade).unwrap()).unwrap();
        assert_eq!(saved, grade);
        let old: Grade = serde_json::from_str(
            r#"{"tag": "t", "score": 1, "maxScore": 2, "testsPassed": 1, "testsTotal": 2,
                "publishedAt": null, "url": "u"}"#,
        )
        .unwrap();
        assert!(!old.has_tests());
        assert!(old.needs_release_notes());
        // A test saved before timeouts were read has none.
        let before: GradeTest = serde_json::from_str(
            r#"{"name": "t", "passed": false, "score": 0, "maxScore": 1,
                "output": "timed out after 10s", "outputClipped": false}"#,
        )
        .unwrap();
        assert_eq!(before.timed_out_after, None);
        let timed_out = GradeTest {
            timed_out_after: Some(10),
            ..before
        };
        let json = serde_json::to_value(&timed_out).unwrap();
        assert_eq!(json["timedOutAfter"], 10);
    }

    fn test(name: &str, passed: bool) -> GradeTest {
        GradeTest {
            name: name.into(),
            passed,
            score: u32::from(passed),
            max_score: 1,
            output: None,
            output_clipped: false,
            timed_out_after: None,
        }
    }

    #[test]
    fn hands_each_failed_test_its_output() {
        let mut grade = Grade {
            tag: "submit/x".into(),
            score: 1,
            max_score: 4,
            tests_passed: 1,
            tests_total: 4,
            tests: vec![
                test("ok", true),
                test("same", false),
                test("same", false),
                test("silent", false),
            ],
            published_at: None,
            url: "u".into(),
            release_notes: None,
        };
        let body = "### classroom50 autograde: 1/4\n\n\
                    <details><summary>Failure details</summary>\n\n**same**\n\n```\nfirst\n```\n\n\
                    **same**\n\n```\nsecond\n```\n\n**ok**\n\n```\nnot this one\n```\n</details>\n";
        grade.apply_release_body(Some(body));
        let outputs: Vec<Option<&str>> = grade.tests.iter().map(|t| t.output.as_deref()).collect();
        assert_eq!(outputs, [None, Some("first"), Some("second"), None]);
        assert_eq!(grade.release_notes, Some(ReleaseNotes::default()));
        assert!(!grade.needs_release_notes());

        // A release gone: read, nothing to show, not asked again.
        let mut gone = grade.clone();
        gone.release_notes = None;
        gone.tests[1].output = None;
        gone.apply_release_body(None);
        assert_eq!(gone.tests[1].output, None);
        assert!(!gone.needs_release_notes());

        // All passed: nothing to read.
        let mut passed = grade.clone();
        passed.release_notes = None;
        passed.tests_passed = 4;
        assert!(!passed.needs_release_notes());
    }

    /// The mock's grade reads like a real one, and its release body names the failed
    /// test's output (`testenv::golden`).
    #[test]
    fn reads_the_suites_mock_grade() {
        use crate::testenv::golden;
        let result: GradeResult = serde_json::from_str(&golden("result-hello")).unwrap();
        let release = GradedRelease {
            tag: result.submission.clone(),
            published_at: None,
            url: result.release.clone(),
            result_url: String::new(),
        };
        let mut grade = grade_from(&release, &result);
        assert_eq!((grade.score, grade.max_score), (7, 12));
        assert!(grade.needs_release_notes());
        let body: BodyData = serde_json::from_str(&golden("release-body")).unwrap();
        let body = body
            .repository
            .and_then(|r| r.release)
            .and_then(|r| r.description);
        grade.apply_release_body(body.as_deref());
        let failed: Vec<_> = grade.tests.iter().filter(|t| !t.passed).collect();
        assert_eq!(failed.len(), 2);
        assert_eq!(failed[0].name, "prints goodbye");
        assert_eq!(failed[0].timed_out_after, None);
        assert!(
            failed[0]
                .output
                .as_deref()
                .unwrap()
                .contains("-goodbye, world"),
            "{:?}",
            failed[0].output
        );
        // The runner stopped it: the line becomes `timed_out_after`, the rest is output.
        assert_eq!(failed[1].name, "asks for a name");
        assert_eq!(failed[1].timed_out_after, Some(10));
        assert_eq!(
            failed[1].output.as_deref(),
            Some("--- input ---\nAda\n\n--- actual stdout ---\nWhat's your name?")
        );
    }
}
