//! The student's groups on team assignments. Each group is a secret GitHub Team named
//! `classroom50-group-<hash>-<n>`, where the hash comes from the classroom and assignment,
//! so a project's group is found from the user's own team memberships (as `gh student team
//! list` does), and its members are read with one aliased query. Read-only.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::github;

const GROUP_TEAM_PREFIX: &str = "classroom50-group-";
const GROUP_HASH_HEX_LEN: usize = 16;
const GROUP_RECORD_SCHEMA: &str = "classroom50/group/v1";
/// Groups are small; this also bounds the query's cost.
const MAX_MEMBERS: u32 = 50;
/// Teams per aliased query.
const BATCH: usize = 25;

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
    nodes: Vec<Option<Member>>,
}

/// Reads the members of each group team. Names travel as variables, never spliced into the
/// query text.
pub async fn load(http: &reqwest::Client, token: &str, teams: &[GroupTeam]) -> Result<Vec<Group>> {
    let mut groups = Vec::new();
    for batch in teams.chunks(BATCH) {
        let (query, variables) = members_query(batch);
        let mut data: Map<String, Value> = github::graphql(http, token, &query, variables).await?;
        for (i, team) in batch.iter().enumerate() {
            let members = data
                .remove(&format!("g{i}"))
                .and_then(|value| serde_json::from_value::<TeamData>(value).ok())
                .and_then(|org| org.team)
                .map(|team| {
                    let members = team.members.nodes.into_iter().flatten();
                    // GitHub answers "" for a profile without a public email.
                    members
                        .map(|m| Member {
                            email: m.email.filter(|e| !e.trim().is_empty()),
                            ..m
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Some(group) = group(team, members) {
                groups.push(group);
            }
        }
    }
    Ok(groups)
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
            "g{i}: organization(login: $o{i}) {{ team(slug: $s{i}) {{ members(first: {MAX_MEMBERS}) {{ nodes {{ login name email }} }} }} }}"
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
            url: String::new(),
            classroom: None,
            assignment: None,
        }];
        assert!(find(&groups, "org", "CS50", "project").is_some());
        assert!(find(&groups, "org", "cs50", "other").is_none());
        assert!(find(&groups, "else", "cs50", "project").is_none());
    }
}
