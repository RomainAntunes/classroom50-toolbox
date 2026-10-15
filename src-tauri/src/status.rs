//! What GitHub says about an assignment repo beyond discovery: branches holding work that
//! isn't on the default branch, the newest grade (`result.json` on the `submit/*` release),
//! whether the head commit is being graded, and the teacher's activity on the Feedback PR. Read only for assignment repos, by a
//! cheap query that also runs between full syncs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::classroom50::GradeResult;
use crate::error::{Error, Result};

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
    pub published_at: Option<DateTime<Utc>>,
    pub url: String,
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

    /// The newest published `submit/*` release carrying a `result.json`.
    pub fn graded_release(&mut self) -> Option<GradedRelease> {
        self.releases
            .take()?
            .into_iter()
            .filter(|r| !r.is_draft && r.tag_name.starts_with(SUBMIT_PREFIX))
            .find_map(|r| {
                let result_url = r.result?.into_iter().next()?.url;
                Some(GradedRelease {
                    tag: r.tag_name,
                    published_at: r.published_at,
                    url: r.url,
                    result_url,
                })
            })
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
        published_at: release.published_at,
        url: release.url.clone(),
    }
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
         "result": {"nodes": [{"url": "r2"}]}}
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
        let release = fields.graded_release().unwrap();
        assert_eq!(release.tag, "submit/2026-10-13T12-00-05Z-def5678");
        assert_eq!(release.result_url, "r2");

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
    }
}
