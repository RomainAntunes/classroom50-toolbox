//! What GitHub says about an assignment repo beyond discovery: branches holding work that
//! isn't on the default branch, the newest grade (`result.json` on the `submit/*` release)
//! and the teacher's activity on the Feedback PR. Read only for assignment repos, by a
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
    let response = http.get(&release.result_url).send().await?;
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
