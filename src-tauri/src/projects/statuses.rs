//! Assignment status: the status fragment read for the assignment repos, new grades
//! downloaded once per tag.

use std::collections::HashMap;

use futures_util::future::join_all;
use serde::Deserialize;

use super::{
    Progress, ProjectsState, REPO_FRAGMENT, RemoteRepo, RepoNode, SyncIssue, fetch_nodes, key,
};
use crate::classroom50::RepoConfig;
use crate::error::{Error, Result};
use crate::status::{self, Grade, GradeSummary, StatusFields};
use crate::sync_progress::Phase;
use crate::tools;

/// Repos per status request. GitHub's response time grows with the batch, so a few small
/// requests in parallel answer faster than one big one.
const STATUS_BATCH: usize = 3;
/// Status requests in flight at once.
const MAX_STATUS_REQUESTS: usize = 4;

pub(crate) struct StatusAuth {
    token: String,
    login: String,
}

/// The token and login for a status read, or `None` when signed out or never synced.
pub(super) async fn status_auth(state: &ProjectsState) -> Option<StatusAuth> {
    let login = state.remote().login?;
    let token = tools::gh_token().await.ok()?;
    Some(StatusAuth { token, login })
}

#[derive(Deserialize)]
struct StatusNode {
    #[serde(flatten)]
    repo: RepoNode,
    #[serde(flatten)]
    status: StatusFields,
}

/// Status fields survive a discovery re-read, which doesn't fetch them.
pub(super) fn keep_status(repo: &mut RemoteRepo, old: &RemoteRepo) {
    repo.unmerged.clone_from(&old.unmerged);
    repo.grade.clone_from(&old.grade);
    repo.history.clone_from(&old.history);
    repo.grading.clone_from(&old.grading);
    repo.feedback.clone_from(&old.feedback);
}

/// Re-reads the assignment repos with the status fragment, downloads new grades and
/// stores the result in the cache. A repo that can't be read keeps its last data.
pub(super) async fn refresh_statuses(
    state: &ProjectsState,
    http: &reqwest::Client,
    auth: &StatusAuth,
    repos: Vec<(RemoteRepo, RepoConfig)>,
    progress: &Progress,
) -> Vec<(RemoteRepo, RepoConfig)> {
    let fragments = format!("{REPO_FRAGMENT}{}", status::FRAGMENT);
    let names: Vec<String> = repos
        .iter()
        .map(|(r, _)| format!("{}/{}", r.owner, r.name))
        .collect();
    let batches: Vec<Vec<&str>> = names
        .chunks(STATUS_BATCH)
        .map(|batch| batch.iter().map(String::as_str).collect())
        .collect();
    progress.set_total(Phase::Statuses, batches.len());
    let mut responses = Vec::with_capacity(batches.len());
    for group in batches.chunks(MAX_STATUS_REQUESTS) {
        // GitHub asked to slow down: no more requests this round.
        if responses
            .iter()
            .any(|r: &Result<_>| matches!(r, Err(Error::RateLimited { .. })))
        {
            break;
        }
        responses.extend(
            join_all(group.iter().map(|batch| async {
                let response =
                    fetch_nodes::<StatusNode>(http, &auth.token, batch, "Status", &fragments).await;
                progress.advance(Phase::Statuses);
                response
            }))
            .await,
        );
    }

    let mut nodes: HashMap<usize, StatusNode> = HashMap::new();
    for (b, response) in responses.into_iter().enumerate() {
        match response {
            Ok(batch) => {
                for (alias, node) in batch {
                    let Some(i) = alias
                        .strip_prefix('r')
                        .and_then(|i| i.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    if let Some(node) = node {
                        nodes.insert(b * STATUS_BATCH + i, node);
                    }
                }
            }
            Err(err) => {
                log::warn!("status check failed: {err}");
                if let Ok(mut issue) = state.status_issue.lock() {
                    issue.get_or_insert_with(|| SyncIssue::from(&err));
                }
            }
        }
    }

    let mut updated = Vec::with_capacity(repos.len());
    for (i, (old, config)) in repos.into_iter().enumerate() {
        let Some(StatusNode { repo, mut status }) = nodes.remove(&i) else {
            updated.push((old, config));
            continue;
        };
        let mut repo = repo.into_remote();
        repo.template.clone_from(&old.template);
        repo.unmerged = status.unmerged(repo.default_branch.as_deref());
        repo.feedback = status.feedback(&auth.login);
        repo.grading = status.grading();
        let releases = status.graded_releases();
        repo.grade = match releases.first() {
            Some(release)
                if old
                    .grade
                    .as_ref()
                    .is_some_and(|g| g.tag == release.tag && g.has_tests()) =>
            {
                old.grade.clone()
            }
            Some(release) => match status::download_grade(http, release).await {
                Ok(grade) => Some(grade),
                Err(err) => {
                    log::warn!("{err}");
                    old.grade.clone()
                }
            },
            None => None,
        };
        if let Some(grade) = repo.grade.as_mut().filter(|g| g.needs_release_notes()) {
            match status::release_body(http, &auth.token, &repo.owner, &repo.name, &grade.tag).await
            {
                Ok(body) => grade.apply_release_body(body.as_deref()),
                Err(err) => log::warn!("release body of {}: {err}", grade.tag),
            }
        }
        repo.history = grade_history(&releases, repo.grade.as_ref(), &old, |release| {
            status::download_grade(http, release)
        })
        .await;
        updated.push((repo, config));
    }

    let mut cache = state.remote();
    for (repo, _) in &updated {
        cache
            .repos
            .insert(key(&format!("{}/{}", repo.owner, repo.name)), repo.clone());
    }
    state.store_remote(cache);
    updated
}

/// The summary of each graded release, newest first. A result is downloaded once per tag;
/// after that it comes from the last sync.
async fn grade_history<'a, F, Fut>(
    releases: &'a [status::GradedRelease],
    newest: Option<&Grade>,
    old: &RemoteRepo,
    mut download: F,
) -> Vec<GradeSummary>
where
    F: FnMut(&'a status::GradedRelease) -> Fut,
    Fut: std::future::Future<Output = Result<Grade>>,
{
    let mut history = Vec::with_capacity(releases.len());
    for release in releases {
        let known = newest
            .filter(|g| g.tag == release.tag)
            .map(GradeSummary::from)
            .or_else(|| old.history.iter().find(|g| g.tag == release.tag).cloned());
        let summary = match known {
            Some(summary) => summary,
            None => match download(release).await {
                Ok(grade) => GradeSummary::from(&grade),
                Err(err) => {
                    log::warn!("{err}");
                    continue;
                }
            },
        };
        history.push(summary);
    }
    history
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::fixtures::remote;

    #[tokio::test]
    async fn downloads_each_grade_of_the_history_once() {
        let release = |tag: &str| status::GradedRelease {
            tag: tag.into(),
            published_at: None,
            url: format!("page-{tag}"),
            result_url: format!("result-{tag}"),
        };
        let grade = |tag: &str, score: u32| Grade {
            tag: tag.into(),
            score,
            max_score: 10,
            tests_passed: 0,
            tests_total: 0,
            tests: Vec::new(),
            published_at: None,
            url: format!("page-{tag}"),
            release_notes: None,
        };
        let releases = [release("c"), release("b"), release("a"), release("broken")];
        let newest = grade("c", 9);
        let mut old = remote("r", "h", None);
        old.history = vec![GradeSummary::from(&grade("b", 7))];
        let mut downloaded = Vec::new();
        let history = grade_history(&releases, Some(&newest), &old, |r| {
            downloaded.push(r.tag.clone());
            let result = if r.tag == "broken" {
                Err(Error::GitHub("gone".into()))
            } else {
                Ok(grade(&r.tag, 5))
            };
            async move { result }
        })
        .await;
        // The newest grade and the known one are reused; a failed download is left out.
        assert_eq!(downloaded, ["a", "broken"]);
        let scores: Vec<(&str, u32)> = history.iter().map(|g| (g.tag.as_str(), g.score)).collect();
        assert_eq!(scores, [("c", 9), ("b", 7), ("a", 5)]);
    }

    #[test]
    fn reads_the_suites_mock_repos_and_statuses() {
        use crate::testenv::golden;
        let mut details: HashMap<String, Option<RepoNode>> =
            serde_json::from_str(&golden("details")).unwrap();
        assert_eq!(details.len(), 7);
        assert!(details["r6"].is_none(), "an unknown repo is null");
        let hello = details.remove("r0").flatten().unwrap().into_remote();
        assert_eq!(
            hello.config.as_ref().map(|c| c.assignment.as_str()),
            Some("hello")
        );
        assert!(hello.can_push && !hello.archived);
        assert_eq!(
            hello.head.as_ref().unwrap().author.as_deref(),
            Some("ada-e2e")
        );
        let submission = hello.last_submission.unwrap();
        assert!(submission.tag.starts_with("submit/") && submission.at.is_some());
        assert_ne!(submission.commit, hello.head.unwrap().oid);
        let portfolio = details.remove("r2").flatten().unwrap().into_remote();
        assert!(portfolio.config.is_none());

        let mut statuses: HashMap<String, Option<StatusNode>> =
            serde_json::from_str(&golden("status")).unwrap();
        let mut hello = statuses.remove("r0").flatten().unwrap();
        let released = hello.status.graded_releases();
        assert_eq!(released.len(), 1);
        assert!(released[0].result_url.ends_with("/result.json"));
        assert_eq!(hello.status.grading(), None);
        assert_eq!(hello.status.feedback("ada-e2e"), None);
        assert!(hello.status.unmerged(Some("main")).is_empty());
        let mut sorting = statuses.remove("r1").flatten().unwrap();
        assert!(sorting.status.graded_releases().is_empty());
        assert!(sorting.repo.into_remote().config.is_some());
    }
}
