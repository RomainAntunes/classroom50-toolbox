//! GitHub discovery: every repo the user can see, the details of those pushed since the
//! last sync, and the user's classroom and group teams.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::future::join_all;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::{
    Commit, Progress, ProjectsState, RemoteCache, RemoteRepo, Submission, keep_status, key,
};
use crate::classroom50::{Classroom, RepoConfig, SUBMIT_TAG_PREFIX};
use crate::error::Result;
use crate::github;
use crate::groups::{self, GroupTeam};
use crate::sync_progress::Phase;
use crate::tools;

/// Repos per GraphQL request when reading `.classroom50.yaml`; larger batches time out.
const DETAIL_BATCH: usize = 25;
/// Batches in flight at once on a cold cache; GitHub discourages more concurrency.
const PARALLEL_BATCHES: usize = 3;
/// 100 repos per page; anything past 2000 accessible repos isn't scanned.
const MAX_LIST_PAGES: usize = 20;

const LIST_QUERY: &str = r#"
query($cursor: String) {
  viewer {
    login
    repositories(first: 100, after: $cursor,
                 affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER],
                 ownerAffiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER],
                 orderBy: {field: PUSHED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        nameWithOwner pushedAt isTemplate isArchived viewerPermission
        templateRepository { nameWithOwner }
      }
    }
  }
}"#;

pub(super) const REPO_FRAGMENT: &str = r#"
fragment Repo on Repository {
  name
  owner { login }
  url
  isArchived
  viewerPermission
  defaultBranchRef {
    name target { oid ... on Commit { committedDate author { name user { login } } } }
  }
  config: object(expression: "HEAD:.classroom50.yaml") { ... on Blob { text } }
  submissions: refs(refPrefix: "refs/tags/submit/", first: 1,
                    orderBy: {field: ALPHABETICAL, direction: DESC}) {
    nodes { name target { oid ... on Tag { target { oid } } } }
  }
}"#;

#[derive(Deserialize)]
struct ListData {
    viewer: Viewer,
}

#[derive(Deserialize)]
struct Viewer {
    login: String,
    repositories: Connection<ListedRepo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    page_info: PageInfo,
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedRepo {
    name_with_owner: String,
    pushed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    is_template: bool,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    viewer_permission: Option<String>,
    #[serde(default)]
    template_repository: Option<NameWithOwner>,
}

impl ListedRepo {
    /// The template GitHub generated the repo from, only when the repo may be the user's
    /// own work: not a template itself (one made from the template), not archived. Whether
    /// a read-only one counts depends on the entry it matches (`by_template`).
    fn generated_from(&self) -> Option<String> {
        if self.is_template || self.is_archived {
            return None;
        }
        let template = self.template_repository.as_ref()?;
        Some(template.name_with_owner.clone())
    }

    /// The user may push to the repo, `None` when the list didn't say.
    fn can_push(&self) -> Option<bool> {
        let permission = self.viewer_permission.as_deref()?;
        Some(matches!(permission, "ADMIN" | "MAINTAIN" | "WRITE"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NameWithOwner {
    name_with_owner: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RepoNode {
    name: String,
    owner: Login,
    url: String,
    is_archived: bool,
    viewer_permission: Option<String>,
    default_branch_ref: Option<BranchRef>,
    config: Option<Blob>,
    submissions: Option<RefConnection>,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct BranchRef {
    name: String,
    target: Option<GitObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitObject {
    oid: String,
    committed_date: Option<DateTime<Utc>>,
    author: Option<CommitAuthor>,
    /// Set on annotated tags: the object the tag points at.
    target: Option<Box<GitObject>>,
}

#[derive(Deserialize)]
struct CommitAuthor {
    name: Option<String>,
    user: Option<Login>,
}

#[derive(Deserialize)]
struct Blob {
    text: Option<String>,
}

#[derive(Deserialize)]
struct RefConnection {
    nodes: Vec<RefNode>,
}

#[derive(Deserialize)]
struct RefNode {
    name: String,
    target: Option<GitObject>,
}

impl RepoNode {
    pub(super) fn into_remote(self) -> RemoteRepo {
        let config = self
            .config
            .and_then(|blob| blob.text)
            .and_then(|text| RepoConfig::parse(&text).ok());
        let last_submission = self
            .submissions
            .and_then(|refs| refs.nodes.into_iter().next())
            .and_then(|node| {
                let target = node.target?;
                let commit = target.target.map_or(target.oid, |peeled| peeled.oid);
                Some(Submission {
                    at: parse_submit_time(&node.name),
                    tag: format!("{SUBMIT_TAG_PREFIX}{}", node.name),
                    commit,
                })
            });
        let (default_branch, head) = match self.default_branch_ref {
            Some(branch) => (
                Some(branch.name),
                branch.target.map(|t| Commit {
                    oid: t.oid,
                    at: t.committed_date,
                    author: t.author.and_then(|a| a.user.map(|u| u.login).or(a.name)),
                }),
            ),
            None => (None, None),
        };
        RemoteRepo {
            owner: self.owner.login,
            name: self.name,
            url: self.url,
            archived: self.is_archived,
            can_push: matches!(
                self.viewer_permission.as_deref(),
                Some("ADMIN" | "MAINTAIN" | "WRITE")
            ),
            default_branch,
            head,
            last_submission,
            config,
            template: None,
            unmerged: Vec::new(),
            grade: None,
            history: Vec::new(),
            grading: None,
            feedback: None,
        }
    }
}

/// `2026-10-14T12-00-05Z-abc1234` (the part after `submit/`) -> its UTC instant.
fn parse_submit_time(name: &str) -> Option<DateTime<Utc>> {
    let stamp = name.get(..20)?;
    NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H-%M-%SZ")
        .ok()
        .map(|t| t.and_utc())
}

/// Lists every repo the user can see, inspects only those pushed since the last sync, and
/// reads the user's classroom teams. Results land in the persisted cache; progress is kept
/// even when a later request fails.
pub(super) async fn sync_remote(
    state: &ProjectsState,
    http: &reqwest::Client,
    progress: &Progress,
) -> Result<()> {
    let token = tools::gh_token().await?;
    // The teams query needs the login; gh knows it locally, so both run at once.
    let (login, listed, classrooms) = match tools::active_login().await {
        Some(active) => {
            let (repos, classrooms) = tokio::join!(
                list_repos(http, &token, progress),
                list_classrooms(http, &token, &active)
            );
            let (login, listed) = repos?;
            (login, listed, classrooms)
        }
        None => {
            let (login, listed) = list_repos(http, &token, progress).await?;
            let classrooms = list_classrooms(http, &token, &login).await;
            (login, listed, classrooms)
        }
    };
    progress.finish_phase(Phase::Listing);
    if let Ok(mut stats) = state.stats.lock() {
        stats.repos_seen = listed.len();
    }

    let mut cache = state.remote();
    if cache.login.as_deref() != Some(login.as_str()) {
        cache = RemoteCache {
            login: Some(login),
            ..RemoteCache::default()
        };
    }
    let listed_keys: HashSet<String> = listed.iter().map(|r| key(&r.name_with_owner)).collect();
    cache.checked.retain(|k, _| listed_keys.contains(k));
    cache.repos.retain(|k, _| listed_keys.contains(k));

    let stale: Vec<&ListedRepo> = listed
        .iter()
        .filter(|r| cache.checked.get(&key(&r.name_with_owner)) != Some(&r.pushed_at))
        .collect();
    // Group members are read last, as one more step.
    progress.set_total(Phase::Details, stale.len().div_ceil(DETAIL_BATCH) + 1);
    let mut result = Ok(());
    'requests: for group in stale.chunks(DETAIL_BATCH * PARALLEL_BATCHES) {
        let batches: Vec<_> = group.chunks(DETAIL_BATCH).collect();
        let names: Vec<Vec<&str>> = batches
            .iter()
            .map(|batch| batch.iter().map(|r| r.name_with_owner.as_str()).collect())
            .collect();
        let responses = join_all(
            names
                .iter()
                .map(|batch| fetch_nodes::<RepoNode>(http, &token, batch, "Repo", REPO_FRAGMENT)),
        )
        .await;
        for (batch, response) in batches.into_iter().zip(responses) {
            progress.advance(Phase::Details);
            let mut nodes = match response {
                Ok(nodes) => nodes,
                Err(err) => {
                    result = Err(err);
                    break 'requests;
                }
            };
            for (i, listed) in batch.iter().enumerate() {
                // A missing alias means the repo vanished or became inaccessible.
                let Some(node) = nodes.remove(&format!("r{i}")) else {
                    continue;
                };
                let k = key(&listed.name_with_owner);
                cache.checked.insert(k.clone(), listed.pushed_at);
                match node {
                    Some(node) => {
                        let mut repo = node.into_remote();
                        if let Some(old) = cache.repos.get(&k) {
                            keep_status(&mut repo, old);
                        }
                        cache.repos.insert(k, repo)
                    }
                    None => cache.repos.remove(&k),
                };
            }
        }
    }
    // The template comes with the list, so every repo has it, read or not this time.
    for listed in &listed {
        if let Some(repo) = cache.repos.get_mut(&key(&listed.name_with_owner)) {
            repo.template = listed.generated_from();
            // Fresher than the details, which are only re-read after a push: closing
            // submissions changes the permission, not `pushedAt`.
            if let Some(can_push) = listed.can_push() {
                repo.can_push = can_push;
            }
        }
    }
    // Repos never read (the sync stopped first) are unknown: not proof of anything missing.
    cache.incomplete = listed_keys.iter().any(|k| !cache.checked.contains_key(k));
    match classrooms {
        Ok((classrooms, group_teams)) => {
            cache.classrooms = classrooms;
            match groups::load(http, &token, &state.http_cache, &group_teams).await {
                Ok(groups) => cache.groups = groups,
                Err(err) => log::warn!("could not read group members: {err}"),
            }
        }
        // Teams are a bonus over marker files; keep the last known list.
        Err(err) => log::warn!("could not read classroom teams: {err}"),
    }
    state.store_remote(cache);
    progress.finish_phase(Phase::Details);
    result
}

const TEAMS_QUERY: &str = r#"
query($login: String!, $cursor: String) {
  viewer {
    organizations(first: 100, after: $cursor) {
      pageInfo { hasNextPage endCursor }
      nodes {
        login
        teams(first: 100, userLogins: [$login], query: "classroom50-") {
          nodes { slug description }
        }
      }
    }
  }
}"#;

#[derive(Deserialize)]
struct TeamsData {
    viewer: TeamsViewer,
}

#[derive(Deserialize)]
struct TeamsViewer {
    organizations: Connection<OrgNode>,
}

#[derive(Deserialize)]
struct OrgNode {
    login: String,
    teams: TeamConnection,
}

#[derive(Deserialize)]
struct TeamConnection {
    nodes: Vec<Option<TeamNode>>,
}

#[derive(Deserialize)]
struct TeamNode {
    slug: String,
    description: Option<String>,
}

/// The classrooms the user is enrolled in, from their `classroom50-<classroom>` teams, and
/// their group teams.
async fn list_classrooms(
    http: &reqwest::Client,
    token: &str,
    login: &str,
) -> Result<(Vec<Classroom>, Vec<GroupTeam>)> {
    let mut classrooms = Vec::new();
    let mut group_teams = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_LIST_PAGES {
        let variables = json!({ "login": login, "cursor": cursor });
        let data: TeamsData = github::graphql(http, token, TEAMS_QUERY, variables).await?;
        let page = data.viewer.organizations;
        for org in page.nodes.into_iter().flatten() {
            for team in org.teams.nodes.into_iter().flatten() {
                if groups::is_group_team(&team.slug) {
                    group_teams.push(GroupTeam {
                        org: org.login.clone(),
                        slug: team.slug,
                        description: team.description,
                    });
                } else if let Some(classroom) =
                    Classroom::from_team(&org.login, &team.slug, team.description.as_deref())
                {
                    classrooms.push(classroom);
                }
            }
        }
        match page.page_info.end_cursor {
            Some(next) if page.page_info.has_next_page => cursor = Some(next),
            _ => break,
        }
    }
    Ok((classrooms, group_teams))
}

async fn list_repos(
    http: &reqwest::Client,
    token: &str,
    progress: &Progress,
) -> Result<(String, Vec<ListedRepo>)> {
    let mut repos = Vec::new();
    let mut cursor: Option<String> = None;
    let mut login = String::new();
    for _ in 0..MAX_LIST_PAGES {
        let data: ListData =
            github::graphql(http, token, LIST_QUERY, json!({ "cursor": cursor })).await?;
        login = data.viewer.login;
        let page = data.viewer.repositories;
        repos.extend(page.nodes.into_iter().flatten());
        // The page count is only known at the end; the last run's stands in until then.
        progress.advance(Phase::Listing);
        match page.page_info.end_cursor {
            Some(next) if page.page_info.has_next_page => cursor = Some(next),
            _ => break,
        }
    }
    Ok((login, repos))
}

/// One aliased query (`r0`, `r1`, ...) spreading `fragment` on each `owner/name`. Names
/// travel as variables, never spliced into the query text.
pub(super) async fn fetch_nodes<T: DeserializeOwned>(
    http: &reqwest::Client,
    token: &str,
    names: &[&str],
    fragment: &str,
    fragments: &str,
) -> Result<HashMap<String, Option<T>>> {
    let mut params = Vec::new();
    let mut fields = String::new();
    let mut variables = Map::new();
    for (i, name_with_owner) in names.iter().enumerate() {
        let Some((owner, name)) = name_with_owner.split_once('/') else {
            continue;
        };
        params.push(format!("$o{i}: String!, $n{i}: String!"));
        fields.push_str(&format!(
            "r{i}: repository(owner: $o{i}, name: $n{i}) {{ ...{fragment} }}\n"
        ));
        variables.insert(format!("o{i}"), owner.into());
        variables.insert(format!("n{i}"), name.into());
    }
    let query = format!("query({}) {{\n{fields}}}\n{fragments}", params.join(", "));
    github::graphql(http, token, &query, Value::Object(variables)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::classroom50;
    use crate::projects::{Found, identify};

    #[test]
    fn parses_submit_tag_time() {
        let at = parse_submit_time("2026-10-14T12-00-05Z-abc1234").unwrap();
        assert_eq!(at.to_rfc3339(), "2026-10-14T12:00:05+00:00");
        assert_eq!(parse_submit_time("my-own-tag"), None);
    }

    #[test]
    fn reads_repo_nodes() {
        let json = r#"{
            "name": "fall-hello-alice", "owner": {"login": "CS50"},
            "url": "https://github.com/CS50/fall-hello-alice", "isArchived": false,
            "viewerPermission": "WRITE",
            "defaultBranchRef": {"name": "main", "target": {"oid": "aaa", "committedDate": "2026-10-07T20:50:13Z"}},
            "config": {"text": "classroom: fall\nassignment: hello\n"},
            "submissions": {"nodes": [{"name": "2026-10-14T12-00-05Z-bbbbbbb",
                                       "target": {"oid": "tagobj", "target": {"oid": "bbb"}}}]}
        }"#;
        let repo = serde_json::from_str::<RepoNode>(json)
            .unwrap()
            .into_remote();
        assert_eq!(repo.config.unwrap().assignment, "hello");
        assert!(repo.can_push);
        assert_eq!(repo.head.unwrap().oid, "aaa");
        let submission = repo.last_submission.unwrap();
        assert_eq!(submission.commit, "bbb");
        assert_eq!(submission.tag, "submit/2026-10-14T12-00-05Z-bbbbbbb");

        let plain: RepoNode = serde_json::from_str(
            r#"{"name": "dotfiles", "owner": {"login": "alice"}, "url": "u", "isArchived": false,
                "viewerPermission": "ADMIN", "defaultBranchRef": null, "config": null, "submissions": {"nodes": []}}"#,
        )
        .unwrap();
        assert!(plain.into_remote().config.is_none());
    }

    #[test]
    fn passes_templates_on_only_for_the_users_own_repos() {
        let listed = |is_template: bool, is_archived: bool, permission: Option<&str>| {
            serde_json::from_value::<ListedRepo>(json!({
                "nameWithOwner": "cs50/renamed",
                "pushedAt": null,
                "isTemplate": is_template,
                "isArchived": is_archived,
                "viewerPermission": permission,
                "templateRepository": {"nameWithOwner": "cs50/lab1-template"},
            }))
            .unwrap()
        };
        for permission in ["ADMIN", "MAINTAIN", "WRITE"] {
            let repo = listed(false, false, Some(permission));
            assert_eq!(repo.generated_from().as_deref(), Some("cs50/lab1-template"));
            assert_eq!(repo.can_push(), Some(true));
        }
        // Read only: passed on, `by_template` decides from the entry.
        for permission in ["READ", "TRIAGE"] {
            let repo = listed(false, false, Some(permission));
            assert!(repo.generated_from().is_some());
            assert_eq!(repo.can_push(), Some(false));
        }
        assert_eq!(listed(false, false, None).can_push(), None);
        // A template made from the template, an old term's archived repo.
        assert_eq!(listed(true, false, Some("ADMIN")).generated_from(), None);
        assert_eq!(listed(false, true, Some("ADMIN")).generated_from(), None);
    }

    /// The WebDriver suite's mock GitHub answers read like GitHub's (`testenv::golden`).
    #[test]
    fn reads_the_suites_mock_listing_and_teams() {
        use crate::testenv::golden;
        let list: ListData = serde_json::from_str(&golden("list")).unwrap();
        assert_eq!(list.viewer.login, "ada-e2e");
        let nodes: Vec<&ListedRepo> = list.viewer.repositories.nodes.iter().flatten().collect();
        assert_eq!(nodes.len(), 6);
        let templates: Vec<Option<String>> = nodes.iter().map(|n| n.generated_from()).collect();
        assert_eq!(templates[3].as_deref(), Some("c50-e2e-lab/blog-starter"));
        assert_eq!(nodes[3].can_push(), Some(true));
        // A read-only copy generated from the same template.
        assert_eq!(nodes[4].name_with_owner, "c50-e2e-lab/blog-solution");
        assert_eq!(templates[4], templates[3]);
        assert_eq!(nodes[4].can_push(), Some(false));
        assert!(templates.iter().filter(|t| t.is_some()).count() == 2);
        // That repo matches no name: its template says which assignment it is.
        let web = Classroom::from_team("c50-e2e-lab", "classroom50-web", None).unwrap();
        let manifest = classroom50::parse_assignments("web", &golden("manifest-web")).unwrap();
        let manifests = HashMap::from([(web.key(), manifest)]);
        let classrooms = BTreeMap::from([(web.key(), web)]);
        let found = Found {
            owner: Some("c50-e2e-lab"),
            name: Some("ada-writes"),
            config: None,
            template: templates[3].as_deref(),
            can_push: true,
        };
        let config = identify(found, &classrooms, &manifests).unwrap();
        assert_eq!(config.assignment, "blog");
        // The read-only copy: Blog is open, so it is no student's work.
        let copy = Found {
            name: Some("blog-solution"),
            template: templates[4].as_deref(),
            can_push: false,
            ..found
        };
        assert!(identify(copy, &classrooms, &manifests).is_none());
        assert!(!list.viewer.repositories.page_info.has_next_page);

        let teams: TeamsData = serde_json::from_str(&golden("teams")).unwrap();
        let classrooms: Vec<Classroom> = teams
            .viewer
            .organizations
            .nodes
            .into_iter()
            .flatten()
            .flat_map(|org| {
                org.teams
                    .nodes
                    .into_iter()
                    .flatten()
                    .filter_map(move |team| {
                        Classroom::from_team(&org.login, &team.slug, team.description.as_deref())
                    })
            })
            .collect();
        let names: Vec<_> = classrooms.iter().map(|c| c.name.as_deref()).collect();
        assert_eq!(names, [Some("Algorithms"), Some("Web Programming")]);
        assert!(classrooms.iter().all(|c| c.active && c.secret.is_none()));
    }
}
