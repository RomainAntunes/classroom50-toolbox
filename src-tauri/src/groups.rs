//! The student's groups on team assignments. Each group is a secret GitHub Team named
//! `classroom50-group-<hash>-<n>`, where the hash comes from the classroom and assignment,
//! so a project's group is found from the user's own team memberships (as `gh student team
//! list` does), and its members are read with one aliased query. Read-only.
//!
//! Emails don't come from GraphQL: its `email` field needs `user:email` or `read:user`,
//! which `gh student login` doesn't grant, and one such field fails the whole query. The
//! REST profile (`users/{login}`) gives the public email with any token, so each member's
//! is read from there, at most daily, revalidated with its ETag.

use std::collections::HashMap;

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::github::{self, HttpCache};

const GROUP_TEAM_PREFIX: &str = "classroom50-group-";
const GROUP_HASH_HEX_LEN: usize = 16;
const GROUP_RECORD_SCHEMA: &str = "classroom50/group/v1";
/// Groups are small; this also bounds the query's cost.
const MAX_MEMBERS: u32 = 50;
/// Teams per aliased query.
const BATCH: usize = 25;
/// How long a member's profile is trusted before it is revalidated (a free `304` when it
/// hasn't changed).
const PROFILE_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::days(1);
/// Profile requests in flight at once.
const PROFILE_BATCH: usize = 8;

/// A group team the user is on, as listed with their other `classroom50-` teams.
#[derive(Debug, Clone)]
pub struct GroupTeam {
    pub org: String,
    pub slug: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub org: String,
    pub slug: String,
    /// The `<n>` of the team name, which is also in the shared repo's name.
    pub number: u32,
    /// The display name the founders chose, when they did.
    pub name: Option<String>,
    pub members: Vec<Member>,
    /// Logins of the team's maintainers: a student-formed group's founder, the only one who
    /// can add members (`gh student team add`). None when the roles weren't read (a cache
    /// from before they were).
    #[serde(default)]
    pub founders: Option<Vec<String>>,
    pub url: String,
    /// Classroom and assignment from the team's record, when it has one.
    #[serde(default)]
    pub classroom: Option<String>,
    #[serde(default)]
    pub assignment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub login: String,
    pub name: Option<String>,
    /// The public email of their GitHub profile, else one they commit with in the clone.
    #[serde(default)]
    pub email: Option<String>,
}

/// `classroom50-group-<16 hex of sha256(classroom \0 assignment)>-`, both lowercased.
pub fn team_prefix(classroom: &str, assignment: &str) -> String {
    let digest = Sha256::digest(format!(
        "{}\0{}",
        classroom.to_lowercase(),
        assignment.to_lowercase()
    ));
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{GROUP_TEAM_PREFIX}{}-", &hex[..GROUP_HASH_HEX_LEN])
}

pub fn is_group_team(slug: &str) -> bool {
    slug.starts_with(GROUP_TEAM_PREFIX)
}

/// The user's group for an assignment, if they're on one.
pub fn find<'a>(
    groups: &'a [Group],
    org: &str,
    classroom: &str,
    assignment: &str,
) -> Option<&'a Group> {
    let prefix = team_prefix(classroom, assignment);
    groups
        .iter()
        .find(|group| group.org.eq_ignore_ascii_case(org) && group.slug.starts_with(&prefix))
}

#[derive(Deserialize)]
struct Record {
    schema: String,
    classroom: String,
    assignment: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct TeamData {
    team: Option<TeamMembers>,
}

#[derive(Deserialize)]
struct TeamMembers {
    members: MemberConnection,
}

#[derive(Deserialize)]
struct MemberConnection {
    edges: Vec<Option<MemberEdge>>,
}

#[derive(Deserialize)]
struct MemberEdge {
    /// `MAINTAINER` or `MEMBER`.
    role: String,
    node: Option<Member>,
}

/// The members of a team, and the logins of its maintainers.
fn members_and_founders(connection: MemberConnection) -> (Vec<Member>, Vec<String>) {
    let mut members = Vec::new();
    let mut founders = Vec::new();
    for edge in connection.edges.into_iter().flatten() {
        let Some(member) = edge.node else { continue };
        if edge.role == "MAINTAINER" {
            founders.push(member.login.clone());
        }
        members.push(member);
    }
    (members, founders)
}

#[derive(Deserialize)]
struct Profile {
    #[serde(default)]
    email: Option<String>,
}

/// Reads the members of each group team, then their public emails. Names travel as
/// variables, never spliced into the query text.
pub async fn load(
    http: &reqwest::Client,
    token: &str,
    cache: &HttpCache,
    teams: &[GroupTeam],
) -> Result<Vec<Group>> {
    let mut groups = Vec::new();
    for batch in teams.chunks(BATCH) {
        let (query, variables) = members_query(batch);
        let mut data: Map<String, Value> = github::graphql(http, token, &query, variables).await?;
        for (i, team) in batch.iter().enumerate() {
            let (members, founders) = data
                .remove(&format!("g{i}"))
                .and_then(|value| serde_json::from_value::<TeamData>(value).ok())
                .and_then(|org| org.team)
                .map(|team| members_and_founders(team.members))
                .map_or((Vec::new(), None), |(m, f)| (m, Some(f)));
            if let Some(mut group) = group(team, members) {
                group.founders = founders;
                groups.push(group);
            }
        }
    }
    public_emails(http, token, cache, &mut groups).await;
    Ok(groups)
}

/// Fills each member's public email from their REST profile, once per login. A profile
/// that can't be read leaves the email unset (the clone's commits may still give one);
/// it never fails the groups.
async fn public_emails(
    http: &reqwest::Client,
    token: &str,
    cache: &HttpCache,
    groups: &mut [Group],
) {
    let mut logins: Vec<String> = groups
        .iter()
        .flat_map(|g| g.members.iter().map(|m| m.login.to_lowercase()))
        .collect();
    logins.sort();
    logins.dedup();
    let mut emails = HashMap::new();
    for chunk in logins.chunks(PROFILE_BATCH) {
        let fetched = join_all(chunk.iter().map(|login| async move {
            let url = profile_url(login);
            match cache
                .get_api_text_or_cached(http, token, &url, PROFILE_MAX_AGE)
                .await
            {
                Ok(body) => profile_email(&body),
                Err(err) => {
                    log::debug!("could not read the profile of {login}: {err}");
                    None
                }
            }
        }))
        .await;
        emails.extend(chunk.iter().cloned().zip(fetched));
    }
    for member in groups.iter_mut().flat_map(|g| g.members.iter_mut()) {
        member.email = emails.get(&member.login.to_lowercase()).cloned().flatten();
    }
}

/// A login is `[A-Za-z0-9-]`, so it needs no escaping; anything else is dropped.
fn profile_url(login: &str) -> String {
    let login: String = login
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    format!("https://api.github.com/users/{login}")
}

/// The public email of a REST profile; GitHub answers `null` (or "") without one.
fn profile_email(body: &str) -> Option<String> {
    serde_json::from_str::<Profile>(body)
        .ok()?
        .email
        .map(|e| e.trim().to_owned())
        .filter(|e| e.contains('@'))
}

/// One aliased query (`g0`, `g1`, ...) reading each team's members. Org and team names
/// travel as variables, never spliced into the query text.
fn members_query(batch: &[GroupTeam]) -> (String, Value) {
    let mut declarations = Vec::new();
    let mut fields = Vec::new();
    let mut variables = Map::new();
    for (i, team) in batch.iter().enumerate() {
        declarations.push(format!("$o{i}: String!, $s{i}: String!"));
        fields.push(format!(
            "g{i}: organization(login: $o{i}) {{ team(slug: $s{i}) {{ members(first: {MAX_MEMBERS}) {{ edges {{ role node {{ login name }} }} }} }} }}"
        ));
        variables.insert(format!("o{i}"), json!(team.org));
        variables.insert(format!("s{i}"), json!(team.slug));
    }
    let query = format!(
        "query({}) {{ {} }}",
        declarations.join(", "),
        fields.join(" ")
    );
    (query, Value::Object(variables))
}

/// A group from its team. The description record is only trusted when its classroom and
/// assignment hash back to the team name, since the group's founders can edit it.
fn group(team: &GroupTeam, members: Vec<Member>) -> Option<Group> {
    let rest = team.slug.strip_prefix(GROUP_TEAM_PREFIX)?;
    let (_, number) = rest.rsplit_once('-')?;
    let number = number.parse().ok()?;
    let record = team
        .description
        .as_deref()
        .and_then(|text| serde_json::from_str::<Record>(text).ok())
        .filter(|record| record.schema == GROUP_RECORD_SCHEMA)
        .filter(|record| {
            team.slug
                .starts_with(&team_prefix(&record.classroom, &record.assignment))
        });
    Some(Group {
        url: format!("https://github.com/orgs/{}/teams/{}", team.org, team.slug),
        org: team.org.clone(),
        slug: team.slug.clone(),
        number,
        name: record
            .as_ref()
            .and_then(|r| r.name.clone())
            .filter(|name| !name.trim().is_empty()),
        classroom: record.as_ref().map(|r| r.classroom.clone()),
        assignment: record.map(|r| r.assignment),
        members,
        founders: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream's cross-language vectors (`cli/shared/testdata/group_vectors.json`).
    #[test]
    fn derives_upstream_team_prefixes() {
        assert_eq!(
            team_prefix("cs50", "project"),
            "classroom50-group-cee5352cb880dff5-"
        );
        assert_eq!(
            team_prefix("Sept-2026", "GroupProject"),
            "classroom50-group-d2537674d06f6f92-"
        );
        assert_eq!(
            team_prefix("intro-cs-fall", "lab-05"),
            "classroom50-group-5411372e7b63533f-"
        );
    }

    #[test]
    fn trusts_the_record_only_when_it_matches_the_team() {
        let team = |description: &str| GroupTeam {
            org: "o".into(),
            slug: "classroom50-group-cee5352cb880dff5-3".into(),
            description: Some(description.into()),
        };
        let good = group(
            &team(r#"{"schema":"classroom50/group/v1","classroom":"cs50","assignment":"project","name":"The Sharks"}"#),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(good.number, 3);
        assert_eq!(good.name.as_deref(), Some("The Sharks"));
        assert_eq!(good.assignment.as_deref(), Some("project"));
        let forged = group(
            &team(r#"{"schema":"classroom50/group/v1","classroom":"cs50","assignment":"other","name":"X"}"#),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(forged.name, None);
        assert!(group(&team("not json"), Vec::new()).is_some());
    }

    #[test]
    fn passes_team_names_as_variables() {
        let team = |org: &str, slug: &str| GroupTeam {
            org: org.into(),
            slug: slug.into(),
            description: None,
        };
        let (query, variables) = members_query(&[
            team("HEIGVD-POO", "classroom50-group-3eaa7f60afc3ad98-3"),
            team("evil\") { x }", "s"),
        ]);
        assert!(query.starts_with("query($o0: String!, $s0: String!, $o1: String!, $s1: String!)"));
        assert!(query.contains("g1: organization(login: $o1) { team(slug: $s1)"));
        assert!(!query.contains("HEIGVD") && !query.contains("evil"));
        assert_eq!(variables["o0"], "HEIGVD-POO");
        assert_eq!(variables["s0"], "classroom50-group-3eaa7f60afc3ad98-3");
        assert_eq!(variables["o1"], "evil\") { x }");
    }

    #[test]
    fn never_asks_graphql_for_emails() {
        let team = GroupTeam {
            org: "o".into(),
            slug: "s".into(),
            description: None,
        };
        let (query, _) = members_query(&[team]);
        assert!(query.contains("edges { role node { login name } }"));
        assert!(!query.contains("email"));
    }

    #[test]
    fn reads_founders_from_member_roles() {
        let connection: MemberConnection = serde_json::from_value(json!({
            "edges": [
                { "role": "MAINTAINER", "node": { "login": "alice", "name": "Alice" } },
                { "role": "MEMBER", "node": { "login": "bob", "name": null } },
                { "role": "MEMBER", "node": null },
                null
            ]
        }))
        .unwrap();
        let (members, founders) = members_and_founders(connection);
        let logins: Vec<_> = members.iter().map(|m| m.login.as_str()).collect();
        assert_eq!(logins, ["alice", "bob"]);
        assert_eq!(founders, ["alice"]);
    }

    #[test]
    fn reads_the_public_email_of_a_profile() {
        assert_eq!(
            profile_email(r#"{"login":"a","email":" a@b.ch "}"#).as_deref(),
            Some("a@b.ch")
        );
        assert_eq!(profile_email(r#"{"login":"a","email":null}"#), None);
        assert_eq!(profile_email(r#"{"login":"a","email":""}"#), None);
        assert_eq!(profile_email(r#"{"login":"a"}"#), None);
        assert_eq!(profile_email("not json"), None);
    }

    #[test]
    fn builds_profile_urls_from_logins_only() {
        assert_eq!(
            profile_url("Romain-Antunes"),
            "https://api.github.com/users/Romain-Antunes"
        );
        assert_eq!(
            profile_url("../repos/x?y"),
            "https://api.github.com/users/reposxy"
        );
    }

    #[test]
    fn reads_the_group_number_from_the_slug() {
        let number = |slug: &str| {
            group(
                &GroupTeam {
                    org: "o".into(),
                    slug: slug.into(),
                    description: None,
                },
                Vec::new(),
            )
            .map(|g| g.number)
        };
        assert_eq!(number("classroom50-group-cee5352cb880dff5-12"), Some(12));
        assert_eq!(number("classroom50-group-cee5352cb880dff5-x"), None);
        assert_eq!(number("classroom50-group-"), None);
        assert_eq!(number("classroom50-poo"), None);
    }

    #[test]
    fn finds_the_group_of_an_assignment() {
        let groups = vec![Group {
            org: "Org".into(),
            slug: "classroom50-group-cee5352cb880dff5-2".into(),
            number: 2,
            name: None,
            members: Vec::new(),
            founders: None,
            url: String::new(),
            classroom: None,
            assignment: None,
        }];
        assert!(find(&groups, "org", "CS50", "project").is_some());
        assert!(find(&groups, "org", "cs50", "other").is_none());
        assert!(find(&groups, "else", "cs50", "project").is_none());
    }
}
