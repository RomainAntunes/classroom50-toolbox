//! Milestone tags: an assignment's `submission_tags` (`assignments-v1`), teacher-named tag
//! patterns such as `phase1` or `v*`. A student's git tag matching one is a submission of
//! that milestone (a glob's matches are one set); with the default autograder pushing it
//! also triggers grading, which still publishes its release under `submit/*`. The app only
//! reads them: it never creates or pushes these tags.
//!
//! Patterns are the subset of GitHub Actions tag filters upstream supports, matched as its
//! `contract.MatchesSubmissionTag` (Go) and `matchesSubmissionTag` (web) do, which translate
//! a pattern into an anchored regex character by character:
//!
//! - any other character matches itself (case-sensitive);
//! - `*` matches any run, empty included, not crossing `/`; `**` crosses `/`;
//! - `?` makes the preceding character optional, `+` repeats it once or more;
//! - `[abc]` and `[a-z]` are character classes, an unclosed `[` is literal.
//!
//! A pattern outside the schema's charset, or with a leading `?`/`+` or a `+` right after
//! another quantifier, matches nothing (upstream fails closed on those), and so does one
//! the regex engines would reject (`[z-a]`, `a???`). Upstream pins its copies to a shared
//! golden file, `fixtures/submission-tag-match-cases.json` here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The most patterns an assignment may name (`SubmissionTagsCap`); more are not read.
pub const MAX_PATTERNS: usize = 20;
/// Tags read per pattern, newest commit first: the tags whose name holds the pattern's
/// literal part (`literal`), so a few cover a glob's set and its near misses.
const TAGS_PER_PATTERN: usize = 10;
/// For a pattern with no literal part (`*`, `[pv]*`) every tag is read, `submit/*` ones too.
const TAGS_UNFILTERED: usize = 50;

/// The tag fields read for each milestone pattern (`selection`). Only included in a status
/// query that has a repo with milestones, since GitHub rejects an unused fragment.
pub const FRAGMENT: &str = r#"
fragment Milestone on Ref {
  name
  target {
    oid
    ... on Commit { committedDate }
    ... on Tag { tagger { date } target { oid ... on Commit { committedDate } } }
  }
}"#;

/// One milestone pattern of a repo's assignment, with the newest tag that matches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    /// The teacher's pattern: a tag name or a glob.
    pub pattern: String,
    /// `None`: no tag matches it yet.
    pub tag: Option<MilestoneTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct MilestoneTag {
    pub name: String,
    /// The tagged commit.
    pub commit: String,
    /// When the tag was made (an annotated tag's date), else the tagged commit's date.
    pub at: Option<DateTime<Utc>>,
}

/// The schema's charset for one pattern (`^[A-Za-z0-9._/*?+\[\]-]+$`).
fn in_charset(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/*?+[]-".contains(&b))
}

/// Charset-safe and free of what the regex engines read differently: a leading `?` or `+`
/// (nothing to repeat), a `+` right after `*`, `?` or `+` (a possessive quantifier in
/// Python only). Upstream's `IsSafeSubmissionTagPattern`.
pub fn is_safe(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    in_charset(pattern)
        && !matches!(bytes.first(), Some(b'?' | b'+'))
        && !bytes
            .windows(2)
            .any(|w| matches!(w[0], b'*' | b'?' | b'+') && w[1] == b'+')
}

/// The patterns worth reading: the safe ones, at most `MAX_PATTERNS`, each once.
pub fn readable(patterns: &[String]) -> Vec<&str> {
    let mut read: Vec<&str> = Vec::new();
    for pattern in patterns.iter().take(MAX_PATTERNS) {
        if is_safe(pattern) && !read.contains(&pattern.as_str()) {
            read.push(pattern);
        }
    }
    read
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Atom {
    Char(char),
    /// Inclusive ranges; a single character is a range of one.
    Class(Vec<(char, char)>),
    /// `*`: any run without `/`.
    Star,
    /// `**`: any run.
    DoubleStar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Once,
    Optional,
    OneOrMore,
}

/// A compiled pattern: atoms with how often each may repeat (a star is its own repeat).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern(Vec<(Atom, Repeat)>);

impl Pattern {
    /// `None` when the pattern matches nothing: unsafe, or a regex the engines reject.
    pub fn compile(pattern: &str) -> Option<Self> {
        if !is_safe(pattern) {
            return None;
        }
        let chars: Vec<char> = pattern.chars().collect();
        let mut atoms: Vec<(Atom, Repeat)> = Vec::new();
        // Quantifiers on the last atom: a regex allows one, then one `?` (lazy, which
        // doesn't change a whole-string match); a star allows only the lazy `?`.
        let mut quantifiers = 0;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match c {
                '?' | '+' => {
                    let (atom, repeat) = atoms.last_mut()?;
                    let star = matches!(atom, Atom::Star | Atom::DoubleStar);
                    match (star, quantifiers, c) {
                        // The lazy `?` after a first quantifier, or after a star.
                        (true, 0, '?') | (false, 1, '?') => {}
                        (false, 0, '?') => *repeat = Repeat::Optional,
                        (false, 0, _) => *repeat = Repeat::OneOrMore,
                        _ => return None,
                    }
                    quantifiers += 1;
                    i += 1;
                    continue;
                }
                '*' if chars.get(i + 1) == Some(&'*') => {
                    atoms.push((Atom::DoubleStar, Repeat::Once));
                    i += 2;
                }
                '*' => {
                    atoms.push((Atom::Star, Repeat::Once));
                    i += 1;
                }
                '[' => match chars[i + 1..].iter().position(|&c| c == ']') {
                    Some(len) => {
                        atoms.push((
                            Atom::Class(class(&chars[i + 1..i + 1 + len])?),
                            Repeat::Once,
                        ));
                        i += len + 2;
                    }
                    None => {
                        atoms.push((Atom::Char('['), Repeat::Once));
                        i += 1;
                    }
                },
                c => {
                    atoms.push((Atom::Char(c), Repeat::Once));
                    i += 1;
                }
            }
            quantifiers = 0;
        }
        Some(Self(atoms))
    }

    /// Whether the whole of `tag` matches.
    pub fn matches(&self, tag: &str) -> bool {
        let tag: Vec<char> = tag.chars().collect();
        // reach[a][t]: atoms[a..] match tag[t..]; filled from the end.
        let (atoms, len) = (&self.0, tag.len());
        let mut reach = vec![vec![false; len + 1]; atoms.len() + 1];
        reach[atoms.len()][len] = true;
        for a in (0..atoms.len()).rev() {
            let (atom, repeat) = &atoms[a];
            for t in (0..=len).rev() {
                let one = |t: usize| t < len && accepts(atom, tag[t]);
                reach[a][t] = match (atom, repeat) {
                    (Atom::Star | Atom::DoubleStar, _) => {
                        reach[a + 1][t] || (one(t) && reach[a][t + 1])
                    }
                    (_, Repeat::Once) => one(t) && reach[a + 1][t + 1],
                    (_, Repeat::Optional) => reach[a + 1][t] || (one(t) && reach[a + 1][t + 1]),
                    (_, Repeat::OneOrMore) => one(t) && (reach[a + 1][t + 1] || reach[a][t + 1]),
                };
            }
        }
        reach[0][0]
    }

    /// The longest run of characters every match holds as is, for GitHub's `refs(query:)`
    /// substring filter: an optional character, a class or a star ends a run, and so does
    /// a repeated one, which also starts the next (`ab+c` holds `ab` and `bc`).
    pub fn literal(&self) -> String {
        let mut best = String::new();
        let mut run = String::new();
        for (atom, repeat) in &self.0 {
            match (atom, repeat) {
                (Atom::Char(c), Repeat::Once) => run.push(*c),
                (Atom::Char(c), Repeat::OneOrMore) => {
                    run.push(*c);
                    if run.len() > best.len() {
                        best.clone_from(&run);
                    }
                    run = c.to_string();
                }
                _ => {
                    if run.len() > best.len() {
                        best.clone_from(&run);
                    }
                    run.clear();
                }
            }
        }
        if run.len() > best.len() {
            best = run;
        }
        best
    }
}

/// A class's members, `None` for one the engines reject: empty, or a range backwards.
/// `-` between two characters is a range, elsewhere itself (after a range too).
fn class(body: &[char]) -> Option<Vec<(char, char)>> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let lo = body[i];
        if body.get(i + 1) == Some(&'-') && i + 2 < body.len() {
            let hi = body[i + 2];
            if hi < lo {
                return None;
            }
            ranges.push((lo, hi));
            i += 3;
        } else {
            ranges.push((lo, lo));
            i += 1;
        }
    }
    (!ranges.is_empty()).then_some(ranges)
}

fn accepts(atom: &Atom, c: char) -> bool {
    match atom {
        Atom::Char(want) => c == *want,
        Atom::Class(ranges) => ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&c)),
        Atom::Star => c != '/',
        Atom::DoubleStar => true,
    }
}

/// Whether `tag` matches any of `patterns` (upstream's `MatchesSubmissionTag`).
#[cfg(test)]
fn matches_any(patterns: &[String], tag: &str) -> bool {
    patterns
        .iter()
        .filter_map(|p| Pattern::compile(p))
        .any(|p| p.matches(tag))
}

/// The tags of one pattern's read (`m<j>`), newest commit first.
#[derive(Debug, Deserialize)]
pub struct TagRefs {
    nodes: Vec<Option<TagRef>>,
}

#[derive(Debug, Deserialize)]
struct TagRef {
    name: String,
    target: Option<TagTarget>,
}

/// A lightweight tag points at its commit; an annotated one at a tag object first.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagTarget {
    oid: String,
    #[serde(default)]
    committed_date: Option<DateTime<Utc>>,
    #[serde(default)]
    tagger: Option<Tagger>,
    #[serde(default)]
    target: Option<Box<TagTarget>>,
}

#[derive(Debug, Deserialize)]
struct Tagger {
    date: Option<DateTime<Utc>>,
}

impl TagRef {
    fn into_tag(self) -> MilestoneTag {
        let target = self.target;
        let (commit, at) = match target {
            Some(TagTarget {
                target: Some(commit),
                tagger,
                ..
            }) => (
                commit.oid,
                tagger.and_then(|t| t.date).or(commit.committed_date),
            ),
            Some(commit) => (commit.oid, commit.committed_date),
            None => (String::new(), None),
        };
        MilestoneTag {
            name: self.name,
            commit,
            at,
        }
    }
}

/// The fields one aliased repo selects for `patterns` (`readable`), and the variables they
/// use, named after the alias's index `alias` so a batch's repos don't collide. Each
/// pattern reads the newest tags holding its literal part, `submit/*` ones left out that
/// way, so they never crowd a milestone out.
pub fn selection(alias: usize, patterns: &[&str]) -> (String, Vec<(String, String)>) {
    let mut fields = String::new();
    let mut variables = Vec::new();
    for (j, pattern) in patterns.iter().enumerate() {
        let literal = Pattern::compile(pattern)
            .map(|p| p.literal())
            .unwrap_or_default();
        let filter = if literal.is_empty() {
            format!("first: {TAGS_UNFILTERED}")
        } else {
            let name = format!("q{alias}_{j}");
            let filter = format!("query: ${name}, first: {TAGS_PER_PATTERN}");
            variables.push((name, literal));
            filter
        };
        fields.push_str(&format!(
            " m{j}: refs(refPrefix: \"refs/tags/\", {filter}, \
             orderBy: {{field: TAG_COMMIT_DATE, direction: DESC}}) {{ nodes {{ ...Milestone }} }}"
        ));
    }
    (fields, variables)
}

/// Each pattern with the newest of its read tags that matches it. `refs` are the reads in
/// `patterns`' order; `None` when one is missing, so the last known milestones stay.
pub fn newest(patterns: &[&str], refs: Vec<Option<TagRefs>>) -> Option<Vec<Milestone>> {
    if refs.len() != patterns.len() {
        return None;
    }
    patterns
        .iter()
        .zip(refs)
        .map(|(pattern, refs)| {
            let compiled = Pattern::compile(pattern);
            let tag = refs?
                .nodes
                .into_iter()
                .flatten()
                .find(|t| compiled.as_ref().is_some_and(|p| p.matches(&t.name)))
                .map(TagRef::into_tag);
            Some(Milestone {
                pattern: (*pattern).to_owned(),
                tag,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, tag: &str) -> bool {
        matches_any(&[pattern.to_owned()], tag)
    }

    /// Upstream's shared golden file, which its Go, web and Python matchers all pass.
    #[test]
    fn agrees_with_upstreams_golden_cases() {
        #[derive(Deserialize)]
        struct Cases {
            cases: Vec<Case>,
        }
        #[derive(Deserialize)]
        struct Case {
            patterns: Vec<String>,
            tag: String,
            matches: bool,
        }
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/submission-tag-match-cases.json"
        );
        let cases: Cases = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        assert!(cases.cases.len() > 40);
        for case in cases.cases {
            assert_eq!(
                matches_any(&case.patterns, &case.tag),
                case.matches,
                "{:?} on {:?}",
                case.patterns,
                case.tag
            );
        }
    }

    #[test]
    fn stars_cross_slashes_only_when_doubled() {
        assert!(matches("v*", "v1.2.3"));
        assert!(matches("v*", "v"));
        assert!(!matches("v*", "v1/rc"));
        assert!(matches("v**", "v1/rc"));
        assert!(matches("phase/*/done", "phase/2/done"));
        assert!(!matches("phase/*/done", "phase/2/b/done"));
        assert!(matches("phase/**/done", "phase/2/b/done"));
        assert!(matches("**", ""));
        assert!(matches("*", ""));
        assert!(matches("a*b*c", "a-x-b-y-c"));
        assert!(!matches("a*b*c", "a-x-b-y-d"));
        // A lazy `?` after a star changes nothing; a second quantifier is an error.
        assert!(matches("v*?", "v12"));
        assert!(matches("v**?", "v1/2"));
        assert!(!matches("v*??", "v1"));
    }

    #[test]
    fn question_marks_and_pluses_repeat_the_previous_character() {
        assert!(matches("colou?r", "color"));
        assert!(matches("colou?r", "colour"));
        assert!(!matches("colou?r", "colouur"));
        // `?` is not a glob's single character.
        assert!(!matches("v?", "v2"));
        assert!(matches("go+al", "gooooal"));
        assert!(!matches("go+al", "gal"));
        assert!(matches("[0-9]+", "2026"));
        assert!(!matches("[0-9]+", "20a6"));
        assert!(matches("v[0-9]?", "v"));
        // Lazy forms match the same; a third quantifier is an error.
        assert!(matches("ab??", "ab"));
        assert!(matches("ab+?", "abbb"));
        assert!(!matches("ab???", "a"));
        assert!(!matches("ab+??", "ab"));
        // Unsafe: nothing to repeat, or a `+` after a quantifier.
        assert!(!matches("?a", "a"));
        assert!(!matches("a?+", "a"));
        assert!(!matches("a*+", "a"));
    }

    #[test]
    fn classes_and_literal_brackets() {
        assert!(matches("phase[1-3]", "phase2"));
        assert!(!matches("phase[1-3]", "phase4"));
        assert!(matches("[a-c-]x", "-x"));
        assert!(matches("[-a]x", "-x"));
        // After a range, `-` is itself.
        assert!(matches("[a-b-d]", "-"));
        assert!(!matches("[a-b-d]", "c"));
        assert!(matches("[*]", "*"));
        assert!(matches("[/]", "/"));
        assert!(matches("a]", "a]"));
        assert!(matches("a[b", "a[b"));
        assert!(matches("a[+", "a[[["));
        // Empty or backwards: the engines reject it, nothing matches.
        assert!(!matches("a[]b", "ab"));
        assert!(!matches("a[]b]", "a]b"));
        assert!(!matches("[9-0]", "5"));
        // Case counts, dots are dots.
        assert!(!matches("Final", "final"));
        assert!(!matches("v1.0", "v1x0"));
    }

    #[test]
    fn keeps_only_safe_patterns_once() {
        let patterns: Vec<String> = ["phase1", "pha se", "phase1", "v*+", "v*", "!old"]
            .map(String::from)
            .to_vec();
        assert_eq!(readable(&patterns), ["phase1", "v*"]);
        let many: Vec<String> = (0..30).map(|i| format!("p{i}")).collect();
        assert_eq!(readable(&many).len(), MAX_PATTERNS);
    }

    #[test]
    fn finds_the_literal_part_for_githubs_filter() {
        let literal = |p: &str| Pattern::compile(p).unwrap().literal();
        assert_eq!(literal("phase1"), "phase1");
        assert_eq!(literal("v*"), "v");
        assert_eq!(literal("release/v[0-9]*"), "release/v");
        assert_eq!(literal("*-final"), "-final");
        assert_eq!(literal("colou?r"), "colo");
        assert_eq!(literal("ab+cdef"), "bcdef");
        assert_eq!(literal("abcd+e"), "abcd");
        assert_eq!(literal("*"), "");
        assert_eq!(literal("[pv]*"), "");
        // Whatever matches holds its literal part.
        for (pattern, tag) in [
            ("ab+cdef", "abbbcdef"),
            ("colou?r", "color"),
            ("x*-final", "x-final"),
        ] {
            let compiled = Pattern::compile(pattern).unwrap();
            assert!(compiled.matches(tag) && tag.contains(&compiled.literal()));
        }
    }

    #[test]
    fn selects_each_pattern_by_its_literal_part() {
        let (fields, variables) = selection(2, &["phase1", "*"]);
        assert_eq!(
            fields,
            " m0: refs(refPrefix: \"refs/tags/\", query: $q2_0, first: 10, \
             orderBy: {field: TAG_COMMIT_DATE, direction: DESC}) { nodes { ...Milestone } } \
             m1: refs(refPrefix: \"refs/tags/\", first: 50, \
             orderBy: {field: TAG_COMMIT_DATE, direction: DESC}) { nodes { ...Milestone } }"
        );
        assert_eq!(variables, [("q2_0".to_owned(), "phase1".to_owned())]);
        assert_eq!(selection(0, &[]), (String::new(), Vec::new()));
    }

    /// The mock's test sends what this file says Rust selects (`e2e/mock/golden`).
    #[test]
    fn selects_what_the_suites_mock_expects() {
        #[derive(Deserialize)]
        struct Golden {
            alias: usize,
            patterns: Vec<String>,
            fields: String,
            variables: std::collections::BTreeMap<String, String>,
        }
        let golden: Golden =
            serde_json::from_str(&crate::testenv::golden("milestones-selection")).unwrap();
        let patterns = readable(&golden.patterns);
        let (fields, variables) = selection(golden.alias, &patterns);
        assert_eq!(fields, golden.fields);
        assert_eq!(
            variables
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>(),
            golden.variables
        );
    }

    #[test]
    fn picks_the_newest_matching_tag_of_each_pattern() {
        let refs: Vec<Option<TagRefs>> = serde_json::from_str(
            r#"[
              {"nodes": [
                {"name": "phase10", "target": {"oid": "c10", "committedDate": "2026-10-20T10:00:00Z"}},
                {"name": "phase1", "target": {"oid": "tagobj",
                  "tagger": {"date": "2026-10-12T11:00:00+02:00"},
                  "target": {"oid": "c1", "committedDate": "2026-10-11T09:00:00Z"}}}
              ]},
              {"nodes": [
                {"name": "v2", "target": {"oid": "c2", "committedDate": "2026-10-15T08:00:00Z"}},
                {"name": "v1", "target": {"oid": "c0", "committedDate": "2026-10-10T08:00:00Z"}}
              ]},
              {"nodes": [{"name": "final-draft", "target": {"oid": "c3"}}]}
            ]"#,
        )
        .unwrap();
        let milestones = newest(&["phase1", "v*", "final"], refs).unwrap();
        assert_eq!(
            milestones,
            [
                Milestone {
                    pattern: "phase1".into(),
                    tag: Some(MilestoneTag {
                        name: "phase1".into(),
                        commit: "c1".into(),
                        // An annotated tag: its own date, which GitHub gives with the
                        // tagger's offset.
                        at: Some("2026-10-12T09:00:00Z".parse().unwrap()),
                    }),
                },
                Milestone {
                    pattern: "v*".into(),
                    tag: Some(MilestoneTag {
                        name: "v2".into(),
                        commit: "c2".into(),
                        at: Some("2026-10-15T08:00:00Z".parse().unwrap()),
                    }),
                },
                Milestone {
                    pattern: "final".into(),
                    tag: None,
                },
            ]
        );
        // A read missing: not known, the last milestones stay.
        assert_eq!(newest(&["phase1"], Vec::new()), None);
        assert_eq!(newest(&["phase1"], vec![None]), None);
        assert_eq!(newest(&[], Vec::new()), Some(Vec::new()));

        // Saved in the snapshot and read back.
        let saved = serde_json::to_string(&milestones).unwrap();
        assert!(saved.contains("\"pattern\":\"v*\""));
        let back: Vec<Milestone> = serde_json::from_str(&saved).unwrap();
        assert_eq!(back, milestones);
    }
}
